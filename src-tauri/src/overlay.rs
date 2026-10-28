// overlay.rs — the recording pill at the bottom-center of the screen.
//
// A small transparent, click-through window (label "overlay",
// ui/overlay.html) that appears just above the Dock while you dictate: live
// voice bars while recording, a shimmer while transcribing, gone when idle.
//
// It must never take focus — the transcript is typed into whatever app has
// focus — so the window is non-focusable, ignores the mouse, and is shown on
// the screen under the cursor without activating Sotto. It joins every
// Space and floats over full-screen apps.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition};

use crate::dictation::DictState;

pub const OVERLAY: &str = "overlay";
/// Gap between the top of the Dock (bottom of the work area) and the pill.
const BOTTOM_GAP: f64 = 10.0;
/// NSStatusWindowLevel: above normal and floating windows.
const STATUS_WINDOW_LEVEL: isize = 25;
/// canJoinAllSpaces | stationary | ignoresCycle | fullScreenAuxiliary
const COLLECTION_BEHAVIOR: usize = (1 << 0) | (1 << 4) | (1 << 6) | (1 << 8);
/// Let the pill's CSS fade-out finish before the window goes away.
const FADE_OUT: Duration = Duration::from_millis(200);

/// Bumped on every show; a pending delayed hide only runs if no show
/// happened since it was scheduled.
static SHOW_GEN: AtomicU64 = AtomicU64::new(0);

/// One-time window setup. Must run on the main thread (Tauri `setup`).
pub fn init(app: &AppHandle) {
    let Some(w) = app.get_webview_window(OVERLAY) else {
        log::error!("overlay window missing from tauri.conf.json");
        return;
    };
    let _ = w.set_ignore_cursor_events(true);
    #[cfg(target_os = "macos")]
    if let Ok(ns) = w.ns_window() {
        use objc::runtime::Object;
        use objc::{msg_send, sel, sel_impl};
        let ns = ns as *mut Object;
        unsafe {
            let _: () = msg_send![ns, setLevel: STATUS_WINDOW_LEVEL];
            let _: () = msg_send![ns, setCollectionBehavior: COLLECTION_BEHAVIOR];
        }
    }
}

/// Mirror a dictation state change: show while recording/transcribing.
pub fn sync(app: &AppHandle, s: DictState) {
    let Some(w) = app.get_webview_window(OVERLAY) else {
        return;
    };
    let wanted = crate::settings::get().overlay;
    match s {
        DictState::Recording | DictState::Processing if wanted => {
            SHOW_GEN.fetch_add(1, Ordering::SeqCst);
            if !w.is_visible().unwrap_or(false) {
                place_on_cursor_screen(app, &w);
                let _ = w.show();
            }
        }
        _ => {
            let gen = SHOW_GEN.load(Ordering::SeqCst);
            std::thread::spawn(move || {
                std::thread::sleep(FADE_OUT);
                if SHOW_GEN.load(Ordering::SeqCst) == gen {
                    let _ = w.hide();
                }
            });
        }
    }
}

/// Live input level (RMS) for the voice bars.
pub fn level(app: &AppHandle, rms: f32) {
    let _ = app.emit_to(OVERLAY, "mic-level", rms);
}

/// Bottom-center of the work area (above the Dock) of the screen the mouse
/// is on, so the pill shows up where the user is looking.
fn place_on_cursor_screen(app: &AppHandle, w: &tauri::WebviewWindow) {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let (Some(m), Ok(size)) = (monitor, w.outer_size()) else {
        return;
    };
    let area = m.work_area();
    let x = area.position.x as f64 + (area.size.width as f64 - size.width as f64) / 2.0;
    let y = area.position.y as f64 + area.size.height as f64
        - size.height as f64
        - BOTTOM_GAP * m.scale_factor();
    let _ = w.set_position(PhysicalPosition::new(x, y));
}
