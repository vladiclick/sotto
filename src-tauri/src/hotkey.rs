// hotkey.rs — the dictation key (Settings → Shortcut).
//
// A modifier-only key (right ⌘ by default; right ⌥, right ⌃ or fn) can't be
// registered as a global shortcut, so this runs a listen-only CGEventTap
// (flags-changed + key-down) on its own CFRunLoop thread. The tap needs the
// Accessibility permission the app already requires for typing; until it's
// granted the thread keeps retrying.
//
// Hold mode (push-to-talk): the mic opens on key-down so no first words are
// lost, but the press only counts once held alone for HOLD_DELAY; a quick
// tap or any shortcut chord (⌘C, ⌘Tab…) discards the audio. The hold check
// runs again on release from the measured time, so a late timer can never
// swallow a dictation.
// Toggle mode: a clean tap starts dictation, the next tap stops it.
// Esc cancels a recording in either mode.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use core_foundation::base::TCFType;
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::event::{
    CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    EventField,
};
use tauri::{AppHandle, Manager};

use crate::commands::AppData;
use crate::dictation::DictState;
use crate::settings::{self, Hotkey, Mode};

const HOLD_DELAY: Duration = Duration::from_millis(250);
/// Longest press that still counts as a tap in toggle mode.
const TAP_MAX: Duration = Duration::from_millis(500);
const TAP_RETRY: Duration = Duration::from_secs(2);
const ESC_KEYCODE: i64 = 53;

/// (virtual keycode, device-dependent "this key is down" flag bit)
fn trigger(h: Hotkey) -> (i64, u64) {
    match h {
        Hotkey::RightCommand => (54, 0x10),   // NX_DEVICERCMDKEYMASK
        Hotkey::RightOption => (61, 0x40),    // NX_DEVICERALTKEYMASK
        Hotkey::RightControl => (62, 0x2000), // NX_DEVICERCTLKEYMASK
        Hotkey::Fn => (63, 0x80_0000),        // kCGEventFlagMaskSecondaryFn
    }
}

static HELD: AtomicBool = AtomicBool::new(false);
static CHORD: AtomicBool = AtomicBool::new(false);
/// This press opened the mic. The key never touches a recording it didn't
/// start (e.g. one begun with the panel's mic button).
static OWNED: AtomicBool = AtomicBool::new(false);
static PRESS_GEN: AtomicU64 = AtomicU64::new(0);
static PRESSED_AT: Mutex<Option<Instant>> = Mutex::new(None);
/// The live tap's mach port, so the callback can re-enable a tap that macOS
/// switched off (it does that after a slow callback or secure input).
static TAP_PORT: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn CGEventTapEnable(tap: *mut std::ffi::c_void, enable: bool);
}

/// True while the key listener is installed, running and allowed to hear
/// keys.
pub fn active() -> bool {
    !TAP_PORT.load(Ordering::SeqCst).is_null() && crate::typing::accessibility_trusted()
}

fn on_press(app: &AppHandle) {
    HELD.store(true, Ordering::SeqCst);
    CHORD.store(false, Ordering::SeqCst);
    let gen = PRESS_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    *PRESSED_AT.lock().unwrap_or_else(|p| p.into_inner()) = Some(Instant::now());
    if settings::get().mode == Mode::Toggle {
        OWNED.store(false, Ordering::SeqCst); // toggle acts on release
        return;
    }
    if crate::panel::is_open() {
        // the panel has keyboard focus: the text would be typed into it
        crate::panel::return_focus(app);
    }
    let owned = app.state::<AppData>().dictation.ptt_press();
    OWNED.store(owned, Ordering::SeqCst);
    if !owned {
        log::info!(
            "dictation key pressed while {} — ignored",
            app.state::<AppData>().dictation.state().as_str()
        );
        return; // still loading, busy transcribing, or already recording
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(HOLD_DELAY);
        let same_press = PRESS_GEN.load(Ordering::SeqCst) == gen;
        if same_press && HELD.load(Ordering::SeqCst) && !CHORD.load(Ordering::SeqCst) {
            app.state::<AppData>().dictation.ptt_confirm(&app);
        }
    });
}

fn on_release(app: &AppHandle) {
    HELD.store(false, Ordering::SeqCst);
    let held_for = PRESSED_AT
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
        .map(|t| t.elapsed())
        .unwrap_or_default();
    let ms = held_for.as_millis();
    let dict = &app.state::<AppData>().dictation;
    if settings::get().mode == Mode::Toggle {
        if !CHORD.load(Ordering::SeqCst) && held_for <= TAP_MAX {
            log::info!("dictation key tapped ({ms} ms) — toggling");
            if crate::panel::is_open() {
                crate::panel::return_focus(app);
            }
            dict.toggle(app);
        }
        return;
    }
    if !OWNED.swap(false, Ordering::SeqCst) {
        return;
    }
    if CHORD.load(Ordering::SeqCst) {
        log::info!("dictation key released after {ms} ms — shortcut, audio discarded");
        dict.cancel(app);
    } else if held_for >= HOLD_DELAY {
        log::info!("dictation key released after {ms} ms — transcribing");
        dict.ptt_release(app, true);
    } else {
        log::info!("dictation key tapped ({ms} ms) — ignored");
        dict.ptt_release(app, false);
    }
}

fn on_chord(app: &AppHandle) {
    if !HELD.load(Ordering::SeqCst) || CHORD.swap(true, Ordering::SeqCst) {
        return;
    }
    if OWNED.load(Ordering::SeqCst) {
        app.state::<AppData>().dictation.cancel(app);
    }
}

/// Route one tapped event. Runs on the tap's run-loop thread: keep it fast.
fn handle_event(app: &AppHandle, etype: CGEventType, event: &CGEvent) {
    match etype {
        CGEventType::FlagsChanged => {
            let (keycode, down_bit) = trigger(settings::get().hotkey);
            let code = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
            if code == keycode {
                let down = event.get_flags().bits() & down_bit != 0;
                let held = HELD.load(Ordering::SeqCst);
                if down && !held {
                    on_press(app);
                } else if !down && held {
                    on_release(app);
                }
            } else {
                // another modifier joined: it's a shortcut chord
                on_chord(app);
            }
        }
        CGEventType::KeyDown => {
            let code = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
            let dict = &app.state::<AppData>().dictation;
            if code == ESC_KEYCODE && dict.state() == DictState::Recording {
                log::info!("Esc — recording cancelled");
                dict.cancel(app);
            } else {
                on_chord(app);
            }
        }
        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
            let port = TAP_PORT.load(Ordering::SeqCst);
            if !port.is_null() {
                log::warn!("hotkey: event tap was disabled by macOS, re-enabling");
                unsafe { CGEventTapEnable(port, true) };
            }
        }
        _ => {}
    }
}

/// Install the tap and run its run loop. Returns Err if the tap can't be
/// created (no Accessibility permission yet), Ok when the run loop ends.
fn run_tap(app: &AppHandle) -> Result<(), ()> {
    let cb_app = app.clone();
    let tap = CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::ListenOnly,
        vec![CGEventType::FlagsChanged, CGEventType::KeyDown],
        move |_proxy, etype, event| {
            handle_event(&cb_app, etype, event);
            None // listen-only: the event passes through untouched
        },
    )?;
    let source = tap.mach_port.create_runloop_source(0).map_err(|_| {
        log::error!("hotkey: runloop source creation failed");
    })?;
    let port = tap.mach_port.as_concrete_TypeRef() as *mut std::ffi::c_void;
    TAP_PORT.store(port, Ordering::SeqCst);
    unsafe { CFRunLoop::get_current().add_source(&source, kCFRunLoopCommonModes) };
    tap.enable();
    log::info!("hotkey: listening for the dictation key");
    CFRunLoop::run_current();
    // only returns if the tap's port dies
    TAP_PORT.store(std::ptr::null_mut(), Ordering::SeqCst);
    unsafe { CFRunLoop::get_current().remove_source(&source, kCFRunLoopCommonModes) };
    Ok(())
}

/// Spawn the event-tap thread. If the tap can't be created yet (no
/// Accessibility permission) it keeps retrying in the background, so the key
/// starts working within TAP_RETRY of the permission being granted.
pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("sotto-hotkey".into())
        .spawn(move || {
            crate::system::make_thread_interactive();
            let mut warned = false;
            loop {
                // macOS lets a listen-only tap be created without the
                // permission, but then never delivers it a key — and a tap
                // made before the permission stays deaf. Wait for it first.
                if !crate::typing::accessibility_trusted() {
                    if !warned {
                        log::info!("hotkey: waiting for the Accessibility permission");
                        warned = true;
                    }
                    std::thread::sleep(TAP_RETRY);
                    continue;
                }
                match run_tap(&app) {
                    Ok(()) => log::warn!("hotkey: event tap stopped, recreating"),
                    Err(()) if !warned => {
                        log::warn!(
                            "hotkey: no event tap yet (Accessibility not granted?) — retrying"
                        );
                        warned = true;
                    }
                    Err(()) => {}
                }
                std::thread::sleep(TAP_RETRY);
            }
        })
        .expect("spawn hotkey thread");
}
