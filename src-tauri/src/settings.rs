// settings.rs — user preferences, persisted as JSON in the app-data dir.
//
// One process-wide copy behind a RwLock: the hotkey thread reads it on every
// key event, so reads must be cheap and never block on disk. Writes validate
// every field (unknown model ids / language codes fall back to defaults) and
// are saved atomically (write to a temp file, then rename).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

use crate::models;

/// Key that starts dictation. Modifier-only keys so they never type anything.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Hotkey {
    RightCommand,
    RightOption,
    RightControl,
    Fn,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// Hold the key while you talk (push-to-talk).
    Hold,
    /// Tap once to start, tap again to stop.
    Toggle,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Insert {
    /// Synthetic keystrokes — never touches the clipboard.
    Type,
    /// Clipboard + ⌘V, then the previous clipboard is restored. Faster for
    /// long text and works in apps that drop synthetic keystrokes.
    Paste,
}

/// Longest custom vocabulary passed to Whisper (its prompt window is small).
pub const MAX_VOCABULARY_CHARS: usize = 600;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Model catalog id (see models.rs).
    pub model: String,
    /// "auto" or a Whisper language code ("en", "ro", "ja", …).
    pub language: String,
    /// Output English whatever language is spoken (models that support it).
    pub translate: bool,
    /// Names, jargon and spellings Whisper should prefer.
    pub vocabulary: String,
    pub hotkey: Hotkey,
    pub mode: Mode,
    pub insert: Insert,
    pub sounds: bool,
    pub overlay: bool,
    /// Mirrors the login item; the OS registration is the source of truth.
    pub launch_at_login: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            model: models::DEFAULT_MODEL.to_string(),
            language: "auto".into(),
            translate: false,
            vocabulary: String::new(),
            hotkey: Hotkey::RightCommand,
            mode: Mode::Hold,
            insert: Insert::Type,
            sounds: true,
            overlay: true,
            launch_at_login: false,
        }
    }
}

impl Settings {
    /// Clamp every field to something the app can act on.
    pub fn validated(mut self) -> Self {
        if models::find(&self.model).is_none() {
            self.model = models::DEFAULT_MODEL.into();
        }
        self.language = self.language.trim().to_lowercase();
        if self.language != "auto" && whisper_rs::get_lang_id(&self.language).is_none() {
            self.language = "auto".into();
        }
        // NUL would abort the C prompt string; cap the length
        self.vocabulary = self
            .vocabulary
            .replace('\0', "")
            .chars()
            .take(MAX_VOCABULARY_CHARS)
            .collect::<String>()
            .trim()
            .to_string();
        self
    }
}

static STORE: OnceLock<RwLock<Settings>> = OnceLock::new();
static PATH: OnceLock<PathBuf> = OnceLock::new();

fn store() -> &'static RwLock<Settings> {
    STORE.get_or_init(|| RwLock::new(Settings::default()))
}

/// Load settings from `<app_data>/settings.json` (defaults if missing/bad).
pub fn load(app_data: &Path) {
    let path = app_data.join("settings.json");
    let loaded = fs::read_to_string(&path)
        .ok()
        .and_then(|s| match serde_json::from_str::<Settings>(&s) {
            Ok(v) => Some(v),
            Err(e) => {
                log::warn!("settings.json unreadable ({e}) — using defaults");
                None
            }
        })
        .unwrap_or_default()
        .validated();
    *store().write().unwrap_or_else(|p| p.into_inner()) = loaded;
    let _ = PATH.set(path);
}

/// Current settings (cheap clone).
pub fn get() -> Settings {
    store().read().unwrap_or_else(|p| p.into_inner()).clone()
}

/// Validate, store and persist. Returns (previous, saved).
pub fn replace(new: Settings) -> Result<(Settings, Settings), String> {
    let new = new.validated();
    let previous = {
        let mut guard = store().write().unwrap_or_else(|p| p.into_inner());
        std::mem::replace(&mut *guard, new.clone())
    };
    save(&new)?;
    Ok((previous, new))
}

fn save(s: &Settings) -> Result<(), String> {
    let Some(path) = PATH.get() else {
        return Ok(()); // not loaded yet (tests)
    };
    let json = serde_json::to_string_pretty(s).map_err(|e| format!("encode settings: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("save settings: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_values_fall_back_to_defaults() {
        let s = Settings {
            model: "nope".into(),
            language: "klingon".into(),
            vocabulary: "a\0b".into(),
            ..Settings::default()
        }
        .validated();
        assert_eq!(s.model, models::DEFAULT_MODEL);
        assert_eq!(s.language, "auto");
        assert_eq!(s.vocabulary, "ab");
    }

    #[test]
    fn real_language_codes_are_kept() {
        for code in ["en", "ro", "ja", "de", "auto"] {
            let s = Settings {
                language: code.into(),
                ..Settings::default()
            }
            .validated();
            assert_eq!(s.language, code);
        }
    }

    #[test]
    fn partial_json_fills_defaults() {
        let s: Settings = serde_json::from_str(r#"{"language":"ro","mode":"toggle"}"#).unwrap();
        assert_eq!(s.language, "ro");
        assert_eq!(s.mode, Mode::Toggle);
        assert_eq!(s.hotkey, Hotkey::RightCommand);
        assert!(s.sounds);
    }
}
