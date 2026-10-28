// stt.rs — speech-to-text on whisper.cpp (Metal GPU), via whisper-rs.
//
// One WhisperContext + one reusable WhisperState, both owned by the speech
// worker thread. whisper.cpp handles audio longer than 30 s internally.
// Post-processing removes the things Whisper is known to invent: non-speech
// tags ("[BLANK_AUDIO]", "(music)") and stock phrases it hallucinates on
// near-silence ("Thank you.", "Thanks for watching!").

use std::path::Path;

use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

use crate::audio::{rms, SPEECH_RMS, TARGET_SR};
use crate::models::ModelSpec;

/// Less voiced audio than this can't hold a word: skip the model entirely
/// (on silence Whisper invents "you", "." or "Thanks for watching!").
const MIN_VOICED_S: f32 = 0.25;
/// Below this much voiced audio, a lone stock phrase is treated as invented.
const MIN_VOICED_FOR_STOCK_PHRASE_S: f32 = 1.2;
/// Compared after lowercasing and stripping punctuation (see `clean`).
const STOCK_HALLUCINATIONS: [&str; 15] = [
    "thank you",
    "thank you very much",
    "thanks for watching",
    "thank you for watching",
    "thanks for listening",
    "please subscribe",
    "bye",
    "you",
    "mulțumesc",
    "vă mulțumesc",
    "să vă mulțumim pentru vizionare",
    "vă mulțumim pentru vizionare",
    "mulțumim pentru vizionare",
    "vă mulțumesc pentru vizionare",
    "nu uitați să vă abonați",
];

/// How to transcribe one recording (resolved from the user's settings).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Request {
    /// Whisper language code, or None to auto-detect.
    pub language: Option<&'static str>,
    /// Output English text whatever language is spoken.
    pub translate: bool,
    /// Custom vocabulary passed as Whisper's initial prompt.
    pub vocabulary: String,
}

/// Map a settings language ("auto", "en", "ro", …) to whisper.cpp's own
/// static code string, or None for auto-detection / unknown codes.
pub fn language_code(code: &str) -> Option<&'static str> {
    if code == "auto" {
        return None;
    }
    whisper_rs::get_lang_id(code).and_then(whisper_rs::get_lang_str)
}

/// Every language Whisper knows, as (code, English name).
pub fn languages() -> Vec<(&'static str, &'static str)> {
    (0..=whisper_rs::get_lang_max_id())
        .filter_map(|id| {
            Some((
                whisper_rs::get_lang_str(id)?,
                whisper_rs::get_lang_str_full(id)?,
            ))
        })
        .collect()
}

pub struct Stt {
    // field order matters: the state must drop before its context
    state: WhisperState,
    _ctx: WhisperContext,
    spec: &'static ModelSpec,
    threads: i32,
    /// Params for the last request, reused while settings don't change.
    /// whisper-rs never frees the prompt string it allocates, so building
    /// params once per configuration (not once per dictation) keeps that
    /// from growing.
    params: Option<(Request, FullParams<'static, 'static>)>,
}

impl Stt {
    pub fn load(model: &Path, spec: &'static ModelSpec) -> Result<Self, String> {
        let path = model
            .to_str()
            .ok_or_else(|| format!("model path is not UTF-8: {}", model.display()))?;
        let cp = WhisperContextParameters {
            use_gpu: true,
            flash_attn: true,
            ..Default::default()
        };
        let ctx = WhisperContext::new_with_params(path, cp)
            .map_err(|e| format!("load {}: {e}", model.display()))?;
        let state = ctx
            .create_state()
            .map_err(|e| format!("whisper state: {e}"))?;
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().clamp(1, 4) as i32)
            .unwrap_or(4);
        let mut stt = Self {
            state,
            _ctx: ctx,
            spec,
            threads,
            params: None,
        };
        // first run compiles the Metal pipelines — pay that now, not on the
        // user's first dictation
        let warmup = Request {
            language: Some("en"),
            ..Request::default()
        };
        let _ = stt.run(&vec![0.0; TARGET_SR], &warmup);
        Ok(stt)
    }

    pub fn spec(&self) -> &'static ModelSpec {
        self.spec
    }

    pub fn transcribe(&mut self, pcm: &[f32], req: &Request) -> Result<String, String> {
        let voiced = voiced_seconds(pcm);
        if voiced < MIN_VOICED_S {
            log::info!("only {voiced:.2}s of voice — nothing to transcribe");
            return Ok(String::new());
        }
        let raw = self.run(pcm, req)?;
        Ok(clean(&raw, voiced))
    }

    /// Adapt a request to what the loaded model can do.
    fn effective(&self, req: &Request) -> Request {
        let mut r = req.clone();
        if self.spec.english_only {
            r.language = Some("en");
            r.translate = false;
        } else if !self.spec.translates {
            r.translate = false;
        }
        r
    }

    fn params(&mut self, req: &Request) -> FullParams<'static, 'static> {
        let req = self.effective(req);
        if let Some((cached, p)) = &self.params {
            if *cached == req {
                return p.clone();
            }
        }
        let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        p.set_language(Some(req.language.unwrap_or("auto")));
        p.set_translate(req.translate);
        if !req.vocabulary.is_empty() {
            p.set_initial_prompt(&req.vocabulary);
        }
        p.set_n_threads(self.threads);
        p.set_no_context(true);
        p.set_no_timestamps(true);
        p.set_suppress_blank(true);
        p.set_suppress_nst(true);
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_special(false);
        p.set_print_timestamps(false);
        self.params = Some((req, p.clone()));
        p
    }

    fn run(&mut self, pcm: &[f32], req: &Request) -> Result<String, String> {
        let p = self.params(req);
        self.state
            .full(p, pcm)
            .map_err(|e| format!("whisper: {e}"))?;
        let mut text = String::new();
        for seg in self.state.as_iter() {
            let s = seg
                .to_str_lossy()
                .map_err(|e| format!("segment text: {e}"))?;
            text.push_str(&s);
            text.push(' ');
        }
        Ok(text)
    }
}

/// Seconds of audio whose 50 ms RMS is above the speech threshold.
fn voiced_seconds(pcm: &[f32]) -> f32 {
    const WIN: usize = TARGET_SR / 20;
    pcm.chunks(WIN).filter(|w| rms(w) > SPEECH_RMS).count() as f32 * 0.05
}

/// Strip non-speech tags, collapse whitespace, drop stock hallucinations.
fn clean(raw: &str, voiced_s: f32) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut depth = 0i32;
    for ch in raw.chars() {
        match ch {
            '[' | '(' => depth += 1,
            ']' | ')' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    let text = out.split_whitespace().collect::<Vec<_>>().join(" ");

    let norm: String = text
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .to_lowercase();
    let norm = norm.trim();
    if norm.is_empty() {
        return String::new();
    }
    if voiced_s < MIN_VOICED_FOR_STOCK_PHRASE_S && STOCK_HALLUCINATIONS.contains(&norm) {
        log::info!("dropped likely hallucination {text:?} ({voiced_s:.2}s voiced)");
        return String::new();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{clean, language_code, languages, Request, Stt};

    /// End-to-end on the real model. Run with:
    ///   SOTTO_TEST_MODEL=…/ggml-large-v3-turbo-q5_0.bin \
    ///   SOTTO_TEST_PCM=clip.pcm (f32 LE mono 16 kHz) SOTTO_TEST_EXPECT="text" \
    ///   [SOTTO_TEST_LANG=ro] cargo test --manifest-path src-tauri/Cargo.toml -- --ignored
    #[test]
    #[ignore]
    fn transcribes_real_audio() {
        let model = std::env::var("SOTTO_TEST_MODEL").expect("SOTTO_TEST_MODEL");
        let file = std::path::Path::new(&model)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap()
            .to_string();
        let spec = crate::models::MODELS
            .iter()
            .find(|m| m.file == file)
            .expect("SOTTO_TEST_MODEL must be a catalog model file");
        let bytes =
            std::fs::read(std::env::var("SOTTO_TEST_PCM").expect("SOTTO_TEST_PCM")).unwrap();
        let pcm: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let req = Request {
            language: std::env::var("SOTTO_TEST_LANG")
                .ok()
                .and_then(|l| language_code(&l)),
            ..Request::default()
        };
        let mut stt = Stt::load(std::path::Path::new(&model), spec).unwrap();
        let text = stt.transcribe(&pcm, &req).unwrap();
        let expect = std::env::var("SOTTO_TEST_EXPECT").expect("SOTTO_TEST_EXPECT");
        assert_eq!(text, expect);
        // silence and a lone click must type nothing
        assert_eq!(stt.transcribe(&vec![0.0; 16_000 * 3], &req).unwrap(), "");
        let mut click = vec![0.0f32; 16_000 * 3];
        for (i, v) in click[16_000..17_600].iter_mut().enumerate() {
            *v = (i as f32 * 0.3).sin() * 0.3;
        }
        assert_eq!(stt.transcribe(&click, &req).unwrap(), "");
        // a vocabulary must not break transcription
        let with_vocab = Request {
            vocabulary: "Sotto, whisper.cpp, Tauri".into(),
            ..req.clone()
        };
        assert!(!stt.transcribe(&pcm, &with_vocab).unwrap().is_empty());
    }

    #[test]
    fn language_codes_resolve() {
        assert_eq!(language_code("auto"), None);
        assert_eq!(language_code("en"), Some("en"));
        assert_eq!(language_code("ro"), Some("ro"));
        assert_eq!(language_code("xx"), None);
        let langs = languages();
        assert!(langs.len() >= 100, "{} languages", langs.len());
        assert!(langs.iter().any(|(c, n)| *c == "ja" && *n == "japanese"));
    }

    #[test]
    fn strips_tags_and_whitespace() {
        assert_eq!(
            clean(" Hello  [BLANK_AUDIO] world (music) ", 5.0),
            "Hello world"
        );
        assert_eq!(clean("[BLANK_AUDIO]", 5.0), "");
    }

    #[test]
    fn drops_stock_phrase_only_on_short_audio() {
        assert_eq!(clean(" Thank you.", 0.4), "");
        assert_eq!(clean(" Să vă mulțumim pentru vizionare!", 0.5), "");
        assert_eq!(clean(" .", 0.5), "");
        assert_eq!(clean(" Thank you.", 3.0), "Thank you.");
        assert_eq!(
            clean(" Thank you for the report.", 0.4),
            "Thank you for the report."
        );
    }

    #[test]
    fn keeps_diacritics() {
        assert_eq!(clean(" Bună ziua, ce faci?", 2.0), "Bună ziua, ce faci?");
    }
}
