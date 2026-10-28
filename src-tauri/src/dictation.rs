// dictation.rs — the dictation pipeline and its state machine.
//
// Threads:
//   - audio (audio.rs): owns the cpal stream; one recording per Start
//   - speech worker (here): loads the model, transcribes one recording at a
//     time and inserts the text (typing.rs)
// State lives in one place (`DictState`); every change is mirrored to the
// tray icon, the recording overlay and the panel ("dictation-state").
//
// Push-to-talk (hotkey.rs) opens the microphone the moment the key goes
// down so the first words are never lost, but the press only becomes a real
// recording once confirmed (key held alone long enough); a quick tap or a
// shortcut chord discards the audio. Toggle mode and the panel's mic button
// use `toggle` instead.
//
// Switching models never interrupts dictation: a new model downloads in the
// background while the current one keeps working, and is only swapped in
// (a few seconds of loading) once it is on disk. If it fails to load, the
// previous model stays active.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use crate::audio::{self, AudioCmd, Empty, TARGET_SR};
use crate::models::{self, ModelSpec};
use crate::perms::{self, MicStatus};
use crate::settings::{self, Insert};
use crate::stats::StatsDb;
use crate::stt::{self, Stt};
use crate::{download, frontmost, sound, typing};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DictState {
    Loading,
    Idle,
    Recording,
    Processing,
    Failed,
}

impl DictState {
    pub fn as_str(self) -> &'static str {
        match self {
            DictState::Loading => "loading",
            DictState::Idle => "idle",
            DictState::Recording => "recording",
            DictState::Processing => "processing",
            DictState::Failed => "failed",
        }
    }
}

struct Job {
    pcm: Vec<f32>,
    request: stt::Request,
    insert: Insert,
    app: String,
}

enum WorkerMsg {
    Transcribe(Job),
    /// Load whatever model the settings now name (it is already on disk).
    Reload,
}

pub struct Dictation {
    audio_tx: Sender<AudioCmd>,
    worker_tx: Sender<WorkerMsg>,
    state: Arc<Mutex<DictState>>,
    target_app: Arc<Mutex<String>>,
    /// The microphone is open (confirmed recording or push-to-talk pre-roll).
    capturing: Arc<AtomicBool>,
    models_dir: PathBuf,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // a panic elsewhere must never wedge dictation behind a poisoned lock
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn set_state(app: &AppHandle, state: &Mutex<DictState>, s: DictState) {
    let prev = std::mem::replace(&mut *lock(state), s);
    crate::panel::update_tray(app, s);
    // emit before showing the overlay so it renders the new look first
    let _ = app.emit("dictation-state", s.as_str());
    crate::overlay::sync(app, s);
    sound::transition(prev, s);
}

/// Most recent problem, kept so a panel opened later can still show it.
/// Cleared by the next successful dictation.
static LAST_ERROR: Mutex<Option<(String, Instant)>> = Mutex::new(None);
/// After this long a transient error is old news and no longer shown.
const ERROR_SHOWN_FOR: Duration = Duration::from_secs(60);

pub fn emit_error(app: &AppHandle, msg: &str) {
    log::error!("{msg}");
    *lock(&LAST_ERROR) = Some((msg.to_string(), Instant::now()));
    let _ = app.emit("dictation-error", msg.to_string());
}

/// The error to show in the panel: always while dictation is `Failed`
/// (it explains why), otherwise only if it happened in the last minute.
pub fn last_error(failed: bool) -> Option<String> {
    lock(&LAST_ERROR)
        .as_ref()
        .filter(|(_, at)| failed || at.elapsed() < ERROR_SHOWN_FOR)
        .map(|(msg, _)| msg.clone())
}

/// Id of the model currently loaded (None while nothing is).
static ACTIVE_MODEL: Mutex<Option<&'static str>> = Mutex::new(None);

pub fn active_model() -> Option<&'static str> {
    *lock(&ACTIVE_MODEL)
}

pub fn init(app: AppHandle, db: Arc<Mutex<StatsDb>>, models_dir: PathBuf) -> Dictation {
    let state = Arc::new(Mutex::new(DictState::Loading));
    let target_app = Arc::new(Mutex::new(String::from("Unknown")));
    let capturing = Arc::new(AtomicBool::new(false));
    let (worker_tx, worker_rx) = mpsc::channel::<WorkerMsg>();

    {
        let app = app.clone();
        let state = state.clone();
        let dir = models_dir.clone();
        std::thread::Builder::new()
            .name("sotto-speech".into())
            .spawn(move || speech_worker(app, state, db, dir, worker_rx))
            .expect("spawn speech thread");
    }

    let audio_tx = {
        let level_app = app.clone();
        let app = app.clone();
        let state = state.clone();
        let target_app = target_app.clone();
        let capturing = capturing.clone();
        let worker_tx = worker_tx.clone();
        let on_finished = move |result: Result<Vec<f32>, Empty>| {
            capturing.store(false, Ordering::SeqCst);
            match result {
                Ok(pcm) => {
                    set_state(&app, &state, DictState::Processing);
                    let s = settings::get();
                    let job = Job {
                        pcm,
                        request: stt::Request {
                            language: stt::language_code(&s.language),
                            translate: s.translate,
                            vocabulary: s.vocabulary,
                        },
                        insert: s.insert,
                        app: lock(&target_app).clone(),
                    };
                    if worker_tx.send(WorkerMsg::Transcribe(job)).is_err() {
                        set_state(&app, &state, DictState::Failed);
                    }
                }
                Err(why) => {
                    if why == Empty::MicUnavailable {
                        emit_error(
                            &app,
                            "Microphone unavailable — check Sotto's Microphone permission",
                        );
                    }
                    let cur = *lock(&state);
                    if matches!(cur, DictState::Recording | DictState::Processing) {
                        set_state(&app, &state, DictState::Idle);
                    }
                }
            }
        };
        audio::spawn(on_finished, move |rms| {
            crate::overlay::level(&level_app, rms)
        })
    };

    Dictation {
        audio_tx,
        worker_tx,
        state,
        target_app,
        capturing,
        models_dir,
    }
}

fn load(dir: &Path, spec: &'static ModelSpec) -> Result<Stt, String> {
    let t0 = Instant::now();
    let stt = Stt::load(&download::path_for(dir, spec), spec)?;
    log::info!(
        "model {} ready in {:.1}s",
        spec.id,
        t0.elapsed().as_secs_f32()
    );
    Ok(stt)
}

/// Load (downloading first if needed) the model the settings name.
fn initial_model(app: &AppHandle, dir: &Path) -> Result<Stt, String> {
    let spec = models::find(&settings::get().model).unwrap_or(&models::MODELS[0]);
    download::ensure(app, dir, spec)?;
    load(dir, spec)
}

fn speech_worker(
    app: AppHandle,
    state: Arc<Mutex<DictState>>,
    db: Arc<Mutex<StatsDb>>,
    dir: PathBuf,
    rx: mpsc::Receiver<WorkerMsg>,
) {
    crate::system::make_thread_interactive();
    let mut stt = match initial_model(&app, &dir) {
        Ok(s) => {
            *lock(&ACTIVE_MODEL) = Some(s.spec().id);
            set_state(&app, &state, DictState::Idle);
            Some(s)
        }
        Err(e) => {
            emit_error(&app, &e);
            set_state(&app, &state, DictState::Failed);
            None
        }
    };
    let _ = app.emit("models-changed", ());

    while let Ok(msg) = rx.recv() {
        match msg {
            WorkerMsg::Reload => {
                let Some(spec) = models::find(&settings::get().model) else {
                    continue;
                };
                if stt.as_ref().map(|s| s.spec().id) == Some(spec.id) {
                    continue; // already active (e.g. reselected)
                }
                if !download::is_downloaded(&dir, spec) {
                    continue; // a download thread will send Reload when done
                }
                // never cut off a recording in progress: wait for it to end
                while *lock(&state) == DictState::Recording {
                    std::thread::sleep(Duration::from_millis(50));
                }
                // a finished recording queued behind this message keeps its
                // "processing" state; the job resets it to idle when done
                let job_queued = *lock(&state) == DictState::Processing;
                let had_model = stt.is_some();
                if !job_queued {
                    set_state(&app, &state, DictState::Loading);
                }
                match load(&dir, spec) {
                    Ok(new) => {
                        stt = Some(new); // the old model drops here
                        *lock(&ACTIVE_MODEL) = Some(spec.id);
                        if !job_queued {
                            set_state(&app, &state, DictState::Idle);
                        }
                    }
                    Err(e) => {
                        emit_error(&app, &format!("couldn't load {}: {e}", spec.name));
                        if !job_queued {
                            let back = if had_model {
                                DictState::Idle
                            } else {
                                DictState::Failed
                            };
                            set_state(&app, &state, back);
                        }
                    }
                }
                let _ = app.emit("models-changed", ());
            }
            WorkerMsg::Transcribe(job) => {
                let Some(engine) = stt.as_mut() else {
                    emit_error(
                        &app,
                        "no voice model is loaded — pick one in Settings → Model",
                    );
                    set_state(&app, &state, DictState::Failed);
                    continue;
                };
                let t0 = Instant::now();
                match engine.transcribe(&job.pcm, &job.request) {
                    Ok(text) if !text.is_empty() => {
                        log::info!(
                            "transcribed {:.1}s of audio in {:.2}s",
                            job.pcm.len() as f32 / TARGET_SR as f32,
                            t0.elapsed().as_secs_f32()
                        );
                        finish_job(&app, &db, &job, &text);
                    }
                    Ok(_) => log::info!("nothing intelligible was said"),
                    Err(e) => emit_error(&app, &format!("transcription failed: {e}")),
                }
                set_state(&app, &state, DictState::Idle);
            }
        }
    }
}

/// Insert the transcript, record stats, notify the panel.
fn finish_job(app: &AppHandle, db: &Mutex<StatsDb>, job: &Job, text: &str) {
    let inserted = match job.insert {
        Insert::Type => typing::type_text(text),
        Insert::Paste => typing::paste_text(text),
    };
    match inserted {
        Ok(()) => *lock(&LAST_ERROR) = None,
        Err(e) => emit_error(app, &format!("couldn't insert the text: {e}")),
    }
    let words = text.split_whitespace().count() as i64;
    let duration_ms = (job.pcm.len() as f64 / TARGET_SR as f64 * 1000.0) as i64;
    let category = frontmost::categorize(&job.app);
    if let Err(e) = lock(db).record_session(&job.app, category, words, duration_ms, 0) {
        log::error!("record session: {e}");
    }
    let _ = app.emit(
        "transcribed",
        serde_json::json!({ "text": text, "words": words, "app": job.app }),
    );
    let _ = app.emit("stats-updated", ());
}

impl Dictation {
    pub fn state(&self) -> DictState {
        *lock(&self.state)
    }

    pub fn models_dir(&self) -> &Path {
        &self.models_dir
    }

    /// Switch to the model the settings now name: download it in the
    /// background if needed (dictation keeps working meanwhile), then load.
    pub fn apply_model_setting(&self, app: &AppHandle) {
        let Some(spec) = models::find(&settings::get().model) else {
            return;
        };
        if download::is_downloaded(&self.models_dir, spec) {
            let _ = self.worker_tx.send(WorkerMsg::Reload);
            return;
        }
        let (app, dir, tx) = (app.clone(), self.models_dir.clone(), self.worker_tx.clone());
        std::thread::spawn(move || match download::ensure(&app, &dir, spec) {
            // only switch if the user still wants this model
            Ok(()) if settings::get().model == spec.id => {
                let _ = tx.send(WorkerMsg::Reload);
            }
            Ok(()) => {}
            Err(e) => log::warn!("model {} not downloaded: {e}", spec.id),
        });
    }

    /// Without microphone access macOS silently hands us zeros, so check
    /// first and tell the user instead of "recording" nothing.
    fn mic_allowed(&self, app: &AppHandle) -> bool {
        match perms::microphone_status() {
            MicStatus::Granted => true,
            MicStatus::Undetermined => {
                perms::request_microphone();
                emit_error(app, "Allow Sotto to use the microphone, then try again");
                false
            }
            MicStatus::Denied => {
                emit_error(
                    app,
                    "Microphone access is off — turn Sotto on in System Settings → \
                     Privacy & Security → Microphone",
                );
                false
            }
        }
    }

    /// Open the mic for this recording. False if busy / not ready.
    fn begin_capture(&self) -> bool {
        if self.state() != DictState::Idle || self.capturing.swap(true, Ordering::SeqCst) {
            return false;
        }
        // attribute the transcript to the app in focus right now
        *lock(&self.target_app) = frontmost::frontmost_app();
        let _ = self.audio_tx.send(AudioCmd::Start);
        true
    }

    /// Push-to-talk key went down: start buffering audio (not yet "recording").
    pub fn ptt_press(&self) -> bool {
        self.begin_capture()
    }

    /// Key held alone long enough: this is a real dictation.
    pub fn ptt_confirm(&self, app: &AppHandle) {
        if !self.capturing.load(Ordering::SeqCst) || self.state() != DictState::Idle {
            return;
        }
        if self.mic_allowed(app) {
            set_state(app, &self.state, DictState::Recording);
        } else {
            let _ = self.audio_tx.send(AudioCmd::Cancel);
        }
    }

    /// Key released. `held_long_enough` is the authoritative hold check:
    /// it also covers a confirm timer that fired late or not at all.
    pub fn ptt_release(&self, app: &AppHandle, held_long_enough: bool) {
        if !self.capturing.load(Ordering::SeqCst) {
            return;
        }
        let confirmed = match self.state() {
            DictState::Recording => true,
            DictState::Idle => held_long_enough && self.mic_allowed(app),
            _ => false,
        };
        if confirmed {
            let _ = self.audio_tx.send(AudioCmd::Stop);
            set_state(app, &self.state, DictState::Processing);
        } else {
            let _ = self.audio_tx.send(AudioCmd::Cancel);
        }
    }

    /// Throw the current recording away (shortcut chord, Esc).
    pub fn cancel(&self, app: &AppHandle) {
        if !self.capturing.load(Ordering::SeqCst) {
            return;
        }
        let _ = self.audio_tx.send(AudioCmd::Cancel);
        if self.state() == DictState::Recording {
            set_state(app, &self.state, DictState::Idle);
        }
    }

    /// Click-to-talk / toggle mode: start, or stop and transcribe.
    pub fn toggle(&self, app: &AppHandle) {
        match self.state() {
            DictState::Idle => {
                if self.mic_allowed(app) && self.begin_capture() {
                    set_state(app, &self.state, DictState::Recording);
                }
            }
            DictState::Recording => {
                let _ = self.audio_tx.send(AudioCmd::Stop);
                set_state(app, &self.state, DictState::Processing);
            }
            _ => {} // loading / processing / failed
        }
    }
}
