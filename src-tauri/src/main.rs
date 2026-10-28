// Sotto — private, on-device dictation in the macOS menu bar.
//
// Lives only in the menu bar: tray glyph + drop-down panel (ui/panel.html),
// a Settings window (ui/settings.html) and a recording pill above the Dock
// (ui/overlay.html, overlay.rs). No Dock icon. Hold the dictation key
// (hotkey.rs), speak, release: mic (audio.rs) → whisper.cpp on Metal
// (stt.rs) → inserted at the cursor (typing.rs). Usage stats stay local
// (stats.rs).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod commands;
mod dictation;
mod download;
mod frontmost;
mod hotkey;
mod models;
mod overlay;
mod panel;
mod perms;
mod settings;
mod sound;
mod stats;
mod stt;
mod system;
mod typing;

use std::sync::{Arc, Mutex};

use tauri::{Manager, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

use commands::AppData;
use stats::StatsDb;

fn main() {
    system::init_logging();
    log::info!("Sotto {} starting", env!("CARGO_PKG_VERSION"));
    system::disable_app_nap();

    tauri::Builder::default()
        // must be the first plugin: a second launch just exits
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            log::info!("Sotto is already running — opening Settings instead");
            commands::open_settings_window(app, None);
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None::<Vec<&str>>,
        ))
        .setup(|app| {
            // menu-bar only: no Dock icon, no app-switcher entry
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            settings::load(&data_dir);

            let db = StatsDb::open(&data_dir.join("stats.sqlite3"))
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            let db = Arc::new(Mutex::new(db));

            // first run: fire the native prompts; the panel walks the user
            // through anything still missing
            perms::request_microphone();
            perms::prompt_accessibility();
            let p = perms::check();
            log::info!(
                "permissions: microphone={} accessibility={}",
                serde_json::to_string(&p.microphone).unwrap_or_default(),
                p.accessibility
            );

            // tray + overlay exist before the pipeline can change state
            panel::build_tray(app.handle())?;
            overlay::init(app.handle());

            let models_dir = download::models_dir(&data_dir);
            log::info!("models: {}", models_dir.display());
            let dict = dictation::init(app.handle().clone(), db.clone(), models_dir);
            app.manage(AppData {
                db,
                dictation: dict,
            });

            hotkey::spawn(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // the drop-down panel behaves like a popover: click away = gone
            WindowEvent::Focused(false) if window.label() == panel::PANEL => {
                panel::hide_panel(window.app_handle());
            }
            // closing Settings just hides it; the app lives in the menu bar
            WindowEvent::CloseRequested { api, .. }
                if window.label() == commands::SETTINGS_WINDOW =>
            {
                api.prevent_close();
                let _ = window.hide();
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_dashboard,
            commands::get_dictation_state,
            commands::get_last_error,
            commands::get_settings,
            commands::set_settings,
            commands::get_languages,
            commands::list_models,
            commands::download_model,
            commands::cancel_download,
            commands::delete_model,
            commands::check_permissions,
            commands::request_microphone,
            commands::open_privacy_pane,
            commands::show_settings,
            commands::hide_panel,
            commands::panel_dictate,
            commands::quit_app,
            commands::open_logs,
            commands::open_project_page,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Sotto")
        .run(|_app, event| {
            // no windows open ≠ quit; only Quit in the panel / tray menu exits
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
