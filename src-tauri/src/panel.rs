// panel.rs — menu-bar tray glyph + the drop-down "popover" panel.
//
// Left click on the tray glyph toggles a borderless, vibrancy-backed window
// (label "panel", ui/panel.html) anchored right under it, like Control
// Center; it hides itself as soon as it loses focus. Right click opens a
// small native menu (Settings…, Quit). The glyph is Sotto's mark — a
// decrescendo hairpin fading into a text cursor — drawn at runtime: tinted
// like any menu-bar icon when idle, vermilion while recording.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::image::Image;
use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, PhysicalPosition, Rect, WebviewWindow};

use crate::dictation::DictState;
use crate::settings::{self, Hotkey, Mode};

pub const TRAY_ID: &str = "sotto-tray";
pub const PANEL: &str = "panel";

/// Gap between the bottom of the menu bar and the top of the panel.
const GAP_PX: f64 = 6.0;

/// Glyph canvas: 36×36 px = 18 pt @2x, the menu-bar icon size.
const GLYPH: usize = 36;
/// (x0, y0, x1, y1, stroke width) segments of the mark, in canvas pixels:
/// the two hairpin strokes and the cursor.
const GLYPH_STROKES: [(f32, f32, f32, f32, f32); 3] = [
    (4.0, 13.4, 22.0, 18.0, 2.3),
    (4.0, 22.6, 22.0, 18.0, 2.3),
    (29.5, 8.7, 29.5, 27.3, 3.4),
];

/// Distance from p to segment a–b.
fn segment_distance(px: f32, py: f32, (ax, ay, bx, by): (f32, f32, f32, f32)) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    let t = (((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    ((px - ax - t * dx).powi(2) + (py - ay - t * dy).powi(2)).sqrt()
}

/// The mark in `rgba`, antialiased (round-capped strokes as capsule SDFs).
fn glyph_image(rgba: [u8; 4]) -> Image<'static> {
    let mut px = vec![0u8; GLYPH * GLYPH * 4];
    for y in 0..GLYPH {
        for x in 0..GLYPH {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let cover = GLYPH_STROKES
                .iter()
                .map(|&(x0, y0, x1, y1, w)| {
                    let d = segment_distance(fx, fy, (x0, y0, x1, y1)) - w / 2.0;
                    (0.5 - d).clamp(0.0, 1.0)
                })
                .fold(0.0f32, f32::max);
            let o = (y * GLYPH + x) * 4;
            px[o..o + 3].copy_from_slice(&rgba[..3]);
            px[o + 3] = (cover * rgba[3] as f32) as u8;
        }
    }
    Image::new_owned(px, GLYPH as u32, GLYPH as u32)
}

/// Glyph colour and whether macOS should treat it as a template image
/// (tinted to match a light or dark menu bar).
fn state_glyph(s: DictState) -> ([u8; 4], bool) {
    match s {
        DictState::Idle => ([0, 0, 0, 255], true),
        DictState::Loading => ([0, 0, 0, 110], true),
        DictState::Recording => ([255, 90, 31, 255], false), // signal vermilion
        DictState::Processing => ([255, 90, 31, 150], false),
        DictState::Failed => ([200, 70, 60, 200], false),
    }
}

pub fn hotkey_label(h: Hotkey) -> &'static str {
    match h {
        Hotkey::RightCommand => "right ⌘",
        Hotkey::RightOption => "right ⌥",
        Hotkey::RightControl => "right ⌃",
        Hotkey::Fn => "fn",
    }
}

/// Restyle the tray glyph for a dictation state. Safe to call from any thread.
pub fn update_tray(app: &AppHandle, s: DictState) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let (rgba, template) = state_glyph(s);
        let _ = tray.set_icon(Some(glyph_image(rgba)));
        let _ = tray.set_icon_as_template(template);
        let cfg = settings::get();
        let key = hotkey_label(cfg.hotkey);
        let tip = match (s, cfg.mode) {
            (DictState::Recording, Mode::Hold) => {
                format!("Sotto — listening… release {key} to stop")
            }
            (DictState::Recording, Mode::Toggle) => format!("Sotto — listening… tap {key} to stop"),
            (DictState::Processing, _) => "Sotto — transcribing…".into(),
            (DictState::Loading, _) => "Sotto — loading the voice model…".into(),
            (DictState::Failed, _) => "Sotto — dictation unavailable".into(),
            (DictState::Idle, Mode::Hold) => format!("Sotto — hold {key} to dictate"),
            (DictState::Idle, Mode::Toggle) => format!("Sotto — tap {key} to dictate"),
        };
        let _ = tray.set_tooltip(Some(tip));
    }
}

/// Tracked here so the hotkey thread can check it without a round trip to
/// the main thread on every key press.
static PANEL_OPEN: AtomicBool = AtomicBool::new(false);

pub fn is_open() -> bool {
    PANEL_OPEN.load(Ordering::SeqCst)
}

pub fn hide_panel(app: &AppHandle) {
    PANEL_OPEN.store(false, Ordering::SeqCst);
    if let Some(p) = app.get_webview_window(PANEL) {
        let _ = p.hide();
    }
}

/// Close the panel and give keyboard focus back to the app the user was
/// working in — that's where the transcript gets typed. Sotto is an
/// accessory app, so hiding its only window does not do that by itself:
/// hide the app (macOS activates the previous one), then un-hide it without
/// activation so the recording overlay can still appear.
pub fn return_focus(app: &AppHandle) {
    hide_panel(app);
    let _ = app.hide();
    let _ = app.run_on_main_thread(|| {
        #[cfg(target_os = "macos")]
        unsafe {
            use objc::runtime::Object;
            use objc::{class, msg_send, sel, sel_impl};
            let ns_app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![ns_app, unhideWithoutActivation];
        }
    });
}

/// Center the panel horizontally under the tray icon, clamped to the monitor.
fn position_under(panel: &WebviewWindow, icon: Rect) {
    let scale = panel.scale_factor().unwrap_or(2.0);
    let icon_pos = icon.position.to_physical::<f64>(scale);
    let icon_size = icon.size.to_physical::<f64>(scale);
    let width = panel
        .outer_size()
        .map(|s| s.width as f64)
        .unwrap_or(320.0 * scale);

    let mut x = icon_pos.x + icon_size.width / 2.0 - width / 2.0;
    let y = icon_pos.y + icon_size.height + GAP_PX * scale;

    if let Ok(Some(mon)) = panel.current_monitor() {
        let left = mon.position().x as f64;
        let right = left + mon.size().width as f64;
        let margin = 8.0 * scale;
        x = x.clamp(left + margin, (right - width - margin).max(left + margin));
    }
    let _ = panel.set_position(PhysicalPosition::new(x, y));
}

fn toggle_panel(app: &AppHandle, icon: Rect) {
    let Some(panel) = app.get_webview_window(PANEL) else {
        return;
    };
    if panel.is_visible().unwrap_or(false) {
        hide_panel(app);
        return;
    }
    position_under(&panel, icon);
    PANEL_OPEN.store(true, Ordering::SeqCst);
    let _ = panel.show();
    let _ = panel.set_focus();
}

pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let settings_item = MenuItemBuilder::with_id("settings", "Settings…")
        .accelerator("Cmd+,")
        .build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit Sotto")
        .accelerator("Cmd+Q")
        .build(app)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&settings_item, &separator, &quit])
        .build()?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(glyph_image(state_glyph(DictState::Loading).0))
        .icon_as_template(true)
        .tooltip("Sotto — loading the voice model…")
        .menu(&menu)
        // left click opens the panel; the native menu stays on right click
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| match ev.id().as_ref() {
            "settings" => crate::commands::open_settings_window(app, None),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                toggle_panel(tray.app_handle(), rect);
            }
        })
        .build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_is_the_mark() {
        let img = glyph_image([0, 0, 0, 255]);
        let (w, h) = (img.width() as usize, img.height() as usize);
        let px = img.rgba();
        let alpha = |x: usize, y: usize| px[(y * w + x) * 4 + 3];
        // mirror-symmetric top to bottom
        for y in 0..h / 2 {
            for x in 0..w {
                let (t, b) = (alpha(x, y) as i32, alpha(x, h - 1 - y) as i32);
                assert!((t - b).abs() <= 2, "asymmetric at ({x},{y}): {t} vs {b}");
            }
        }
        // the cursor is a solid, tall stroke; the hairpin tip is solid
        let cursor_rows = (0..h).filter(|&y| alpha(29, y) > 200).count();
        assert!(cursor_rows >= 18, "cursor {cursor_rows} px tall");
        assert_eq!(alpha(21, 18), 255, "hairpin tip");
        // open mouth: nothing between the two hairpin strokes at the left
        assert_eq!(alpha(5, 18), 0);
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(w - 1, h - 1), 0);
        if let Ok(path) = std::env::var("SOTTO_DUMP_GLYPH") {
            std::fs::write(path, px).unwrap();
        }
    }
}
