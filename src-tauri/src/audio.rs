// audio.rs — microphone capture on a dedicated thread.
//
// The cpal input stream is !Send on macOS, so it lives entirely inside this
// thread: opened on AudioCmd::Start, closed on Stop/Cancel. The user decides
// when a recording ends (push-to-talk release or a second click) — there is
// no silence auto-stop, so pausing to think never cuts a sentence off. Only
// a hard length cap and a "nobody ever spoke" timeout end it early.
//
// Finished audio is downmixed to mono and resampled to 16 kHz before it is
// handed to `on_finished`.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};

pub const TARGET_SR: usize = 16_000;

/// A 50 ms window counts as speech above this RMS level.
pub const SPEECH_RMS: f32 = 0.012;
/// Give up when nothing louder than room noise arrives for this long.
const NO_SPEECH_TIMEOUT: Duration = Duration::from_secs(20);
/// Hard cap on one recording (a forgotten click-to-talk session).
const MAX_RECORDING: Duration = Duration::from_secs(300);
/// Command poll + level-meter period (25 Hz feeds the recording overlay).
const POLL: Duration = Duration::from_millis(40);

pub enum AudioCmd {
    /// Open the microphone and start buffering.
    Start,
    /// Close the microphone and deliver the recording.
    Stop,
    /// Close the microphone and throw the recording away.
    Cancel,
}

/// Why a recording produced no audio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Empty {
    Cancelled,
    NoSpeech,
    MicUnavailable,
}

/// Spawn the capture thread. `on_finished` fires exactly once per Start with
/// either 16 kHz mono PCM or the reason there is none; `on_level` receives
/// the input RMS every POLL while the mic is open (drives the overlay).
pub fn spawn<F, L>(on_finished: F, on_level: L) -> Sender<AudioCmd>
where
    F: Fn(Result<Vec<f32>, Empty>) + Send + 'static,
    L: Fn(f32) + Send + 'static,
{
    let (tx, rx) = mpsc::channel::<AudioCmd>();
    std::thread::Builder::new()
        .name("sotto-audio".into())
        .spawn(move || {
            crate::system::make_thread_interactive();
            loop {
                match rx.recv() {
                    Ok(AudioCmd::Start) => on_finished(record(&rx, &on_level)),
                    Ok(AudioCmd::Stop | AudioCmd::Cancel) => {} // nothing running
                    Err(_) => break,
                }
            }
        })
        .expect("spawn audio thread");
    tx
}

fn record(cmd_rx: &Receiver<AudioCmd>, on_level: &dyn Fn(f32)) -> Result<Vec<f32>, Empty> {
    let host = cpal::default_host();
    let Some(device) = host.default_input_device() else {
        log::error!("no default input device (microphone)");
        return Err(Empty::MicUnavailable);
    };
    let cfg = device.default_input_config().map_err(|e| {
        log::error!("default input config: {e}");
        Empty::MicUnavailable
    })?;
    let src_sr = cfg.sample_rate().0 as usize;
    let channels = cfg.channels() as usize;
    // pre-size for a minute so the real-time callback rarely reallocates
    let shared = Arc::new(Mutex::new(Vec::<f32>::with_capacity(src_sr * 60)));

    let stream = match cfg.sample_format() {
        cpal::SampleFormat::F32 => {
            build_stream::<f32>(&device, &cfg.into(), channels, shared.clone())
        }
        cpal::SampleFormat::I16 => {
            build_stream::<i16>(&device, &cfg.into(), channels, shared.clone())
        }
        cpal::SampleFormat::U16 => {
            build_stream::<u16>(&device, &cfg.into(), channels, shared.clone())
        }
        f => {
            log::error!("unsupported input sample format: {f:?}");
            return Err(Empty::MicUnavailable);
        }
    }
    .map_err(|e| {
        log::error!("build input stream: {e}");
        Empty::MicUnavailable
    })?;
    stream.play().map_err(|e| {
        log::error!("start input stream: {e}");
        Empty::MicUnavailable
    })?;
    log::info!("recording: {src_sr} Hz, {channels} ch");

    let started = Instant::now();
    let mut speech_seen = false;
    let mut scanned = 0usize;
    let cancelled = loop {
        match cmd_rx.recv_timeout(POLL) {
            Ok(AudioCmd::Stop) => break false,
            Ok(AudioCmd::Cancel) => break true,
            Ok(AudioCmd::Start) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break true,
        }
        let level = {
            let buf = shared.lock().unwrap_or_else(|p| p.into_inner());
            let level = rms(&buf[scanned.min(buf.len())..]);
            scanned = buf.len();
            level
        };
        speech_seen |= level > SPEECH_RMS;
        on_level(level);
        if !speech_seen && started.elapsed() > NO_SPEECH_TIMEOUT {
            log::info!("no speech for {NO_SPEECH_TIMEOUT:?}, stopping");
            break false;
        }
        if started.elapsed() > MAX_RECORDING {
            log::info!("recording hit the {MAX_RECORDING:?} cap");
            break false;
        }
    };

    drop(stream); // closes the mic (and the orange indicator) right away
    let pcm = std::mem::take(&mut *shared.lock().unwrap_or_else(|p| p.into_inner()));
    if cancelled {
        return Err(Empty::Cancelled);
    }
    if !speech_seen || pcm.len() < src_sr / 4 {
        return Err(Empty::NoSpeech);
    }
    Ok(resample(&pcm, src_sr, TARGET_SR))
}

pub fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    shared: Arc<Mutex<Vec<f32>>>,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = channels.max(1);
    device.build_input_stream(
        config,
        move |data: &[T], _| {
            let mut buf = shared.lock().unwrap_or_else(|p| p.into_inner());
            for frame in data.chunks(channels) {
                let s: f32 = frame.iter().map(|&x| f32::from_sample(x)).sum::<f32>();
                buf.push(s / channels as f32);
            }
        },
        |e| log::error!("audio stream error: {e}"),
        None,
    )
}

/// Downsample with a moving-average low-pass first (kills the aliasing a bare
/// linear interpolation lets through), then linear interpolation.
fn resample(pcm: &[f32], src_sr: usize, dst_sr: usize) -> Vec<f32> {
    if src_sr == dst_sr || pcm.is_empty() {
        return pcm.to_vec();
    }
    let ratio = src_sr as f64 / dst_sr as f64;
    let filtered: Vec<f32> = if ratio > 1.0 {
        let w = ratio.round().max(1.0) as usize;
        let mut out = Vec::with_capacity(pcm.len());
        let mut acc = 0.0f32;
        for i in 0..pcm.len() {
            acc += pcm[i];
            if i >= w {
                acc -= pcm[i - w];
            }
            out.push(acc / (i + 1).min(w) as f32);
        }
        out
    } else {
        pcm.to_vec()
    };
    let out_len = (filtered.len() as f64 / ratio) as usize;
    let last = filtered.len() - 1;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let i0 = (pos as usize).min(last);
            let i1 = (i0 + 1).min(last);
            let frac = (pos - i0 as f64) as f32;
            filtered[i0] * (1.0 - frac) + filtered[i1] * frac
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_length_and_dc() {
        let pcm = vec![0.5f32; 48_000];
        let out = resample(&pcm, 48_000, 16_000);
        assert_eq!(out.len(), 16_000);
        assert!(out[100..].iter().all(|&v| (v - 0.5).abs() < 1e-5));
    }

    #[test]
    fn resample_identity_and_empty() {
        assert_eq!(resample(&[1.0, 2.0], 16_000, 16_000), vec![1.0, 2.0]);
        assert!(resample(&[], 48_000, 16_000).is_empty());
    }

    #[test]
    fn resample_attenuates_nyquist_tone() {
        // 24 kHz tone at 48 kHz (alternating ±1) must not survive at 16 kHz
        let pcm: Vec<f32> = (0..48_000)
            .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let out = resample(&pcm, 48_000, 16_000);
        assert!(rms(&out[10..]) < 0.4, "rms {}", rms(&out[10..]));
    }
}
