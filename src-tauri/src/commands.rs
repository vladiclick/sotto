// commands.rs — IPC surface for the panel, settings window and overlay.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;

use crate::dictation::{self, DictState, Dictation};
use crate::models::{self, ModelSpec};
use crate::settings::{self, Settings};
use crate::stats::{DashboardStats, StatsDb};
use crate::{download, panel, perms, stt};

pub struct AppData {
    pub db: Arc<Mutex<StatsDb>>,
    pub dictation: Dictation,
}

pub const SETTINGS_WINDOW: &str = "settings";
const PROJECT_URL: &str = "https://github.com/vladiclick/sotto";

#[tauri::command]
pub fn get_dashboard(state: State<'_, AppData>) -> Result<DashboardStats, String> {
    state
        .db
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .dashboard()
}

#[tauri::command]
pub fn get_dictation_state(state: State<'_, AppData>) -> String {
    state.dictation.state().as_str().to_string()
}

#[tauri::command]
pub fn get_last_error(state: State<'_, AppData>) -> Option<String> {
    dictation::last_error(state.dictation.state() == DictState::Failed)
}

// ---- settings ----

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Settings {
    let mut s = settings::get();
    // the OS registration is the truth (the user can remove login items)
    if let Ok(enabled) = app.autolaunch().is_enabled() {
        s.launch_at_login = enabled;
    }
    s
}

#[tauri::command]
pub fn set_settings(
    app: AppHandle,
    state: State<'_, AppData>,
    settings: Settings,
) -> Result<Settings, String> {
    let (previous, mut saved) = settings::replace(settings)?;

    if saved.launch_at_login != previous.launch_at_login {
        let autolaunch = app.autolaunch();
        let result = if saved.launch_at_login {
            autolaunch.enable()
        } else {
            autolaunch.disable()
        };
        if let Err(e) = result {
            // keep the stored flag honest, then report
            saved.launch_at_login = previous.launch_at_login;
            let _ = settings::replace(saved.clone());
            let _ = app.emit("settings-changed", &saved);
            return Err(format!("couldn't change Launch at login: {e}"));
        }
    }
    if saved.model != previous.model {
        state.dictation.apply_model_setting(&app);
    }
    let _ = app.emit("settings-changed", &saved);
    Ok(saved)
}

#[derive(Serialize)]
pub struct Language {
    code: &'static str,
    name: String,
}

/// Every language Whisper supports, alphabetical by English name.
#[tauri::command]
pub fn get_languages() -> Vec<Language> {
    let mut list: Vec<Language> = stt::languages()
        .into_iter()
        .map(|(code, name)| Language {
            code,
            name: title_case(name),
        })
        .collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    list
}

fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ---- models ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    #[serde(flatten)]
    spec: &'static ModelSpec,
    downloaded: bool,
    downloading: bool,
    /// Currently loaded and transcribing.
    active: bool,
    /// Chosen in settings (may still be downloading / loading).
    selected: bool,
}

#[tauri::command]
pub fn list_models(state: State<'_, AppData>) -> Vec<ModelInfo> {
    let dir = state.dictation.models_dir();
    let selected = settings::get().model;
    let active = dictation::active_model();
    models::MODELS
        .iter()
        .map(|spec| ModelInfo {
            spec,
            downloaded: download::is_downloaded(dir, spec),
            downloading: download::is_downloading(spec.id),
            active: active == Some(spec.id),
            selected: selected == spec.id,
        })
        .collect()
}

/// Download without switching to it.
#[tauri::command]
pub fn download_model(app: AppHandle, state: State<'_, AppData>, id: String) -> Result<(), String> {
    let spec = models::find(&id).ok_or_else(|| format!("unknown model {id}"))?;
    let dir = state.dictation.models_dir().to_path_buf();
    let worker_app = app.clone();
    std::thread::spawn(move || {
        if download::ensure(&worker_app, &dir, spec).is_ok() {
            let _ = worker_app.emit("models-changed", ());
        }
    });
    let _ = app.emit("models-changed", ());
    Ok(())
}

#[tauri::command]
pub fn cancel_download(id: String) {
    download::cancel(&id);
}

#[tauri::command]
pub fn delete_model(app: AppHandle, state: State<'_, AppData>, id: String) -> Result<(), String> {
    let spec = models::find(&id).ok_or_else(|| format!("unknown model {id}"))?;
    if dictation::active_model() == Some(spec.id) || settings::get().model == spec.id {
        return Err("switch to another model before deleting this one".into());
    }
    if download::is_downloading(spec.id) {
        return Err("cancel the download first".into());
    }
    download::delete(state.dictation.models_dir(), spec)?;
    let _ = app.emit("models-changed", ());
    Ok(())
}

// ---- permissions ----

#[tauri::command]
pub fn check_permissions() -> perms::Permissions {
    perms::check()
}

#[tauri::command]
pub fn request_microphone() {
    perms::request_microphone();
}

#[tauri::command]
pub fn open_privacy_pane(pane: String) -> Result<(), String> {
    perms::open_settings_pane(&pane)
}

// ---- windows ----

/// Open Settings (optionally on a tab: "general", "model", "language",
/// "about").
#[tauri::command]
pub fn show_settings(app: AppHandle, tab: Option<String>) {
    open_settings_window(&app, tab.as_deref());
}

pub fn open_settings_window(app: &AppHandle, tab: Option<&str>) {
    panel::hide_panel(app);
    let Some(w) = app.get_webview_window(SETTINGS_WINDOW) else {
        return;
    };
    if let Some(tab) = tab {
        let _ = app.emit_to(SETTINGS_WINDOW, "settings-tab", tab);
    }
    let _ = w.unminimize();
    let _ = w.show();
    let _ = w.set_focus();
}

#[tauri::command]
pub fn hide_panel(app: AppHandle) {
    panel::hide_panel(&app);
}

/// Record button in the drop-down panel. The panel holds focus while open, so
/// close it and hand focus back first — the transcript must land in the app
/// the user was typing in.
#[tauri::command]
pub fn panel_dictate(app: AppHandle) {
    panel::return_focus(&app);
    std::thread::spawn(move || {
        // let macOS hand focus back before frontmost-app attribution runs
        std::thread::sleep(std::time::Duration::from_millis(200));
        app.state::<AppData>().dictation.toggle(&app);
    });
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub fn open_logs() -> Result<(), String> {
    let path = crate::system::log_path().ok_or("no home directory")?;
    let dir = path.parent().ok_or("bad log path")?;
    std::process::Command::new("/usr/bin/open")
        .arg(dir)
        .spawn()
        .map_err(|e| format!("open logs: {e}"))?;
    Ok(())
}

#[tauri::command]
pub fn open_project_page() -> Result<(), String> {
    std::process::Command::new("/usr/bin/open")
        .arg(PROJECT_URL)
        .spawn()
        .map_err(|e| format!("open {PROJECT_URL}: {e}"))?;
    Ok(())
}
