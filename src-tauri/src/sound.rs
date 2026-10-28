// sound.rs — quiet start/stop cues (Settings → Sounds).
//
// Uses sounds that ship with macOS, played softly: a "tink" when the mic goes
// live and a "pop" when it stops and transcription starts. A cancelled
// recording makes no sound.

use crate::dictation::DictState;

const VOLUME: f32 = 0.35;

/// Play the cue for a state change, if any and if sounds are enabled.
pub fn transition(from: DictState, to: DictState) {
    let name = match (from, to) {
        (DictState::Recording, DictState::Recording) => return,
        (_, DictState::Recording) => "Tink",
        (DictState::Recording, DictState::Processing) => "Pop",
        _ => return,
    };
    if crate::settings::get().sounds {
        play(name);
    }
}

#[cfg(target_os = "macos")]
fn play(name: &'static str) {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    // NSSound is played from a short-lived thread with its own autorelease
    // pool; it mixes asynchronously, so this returns immediately.
    std::thread::spawn(move || {
        objc::rc::autoreleasepool(|| unsafe {
            let ns_name: *mut Object = msg_send![
                class!(NSString),
                stringWithUTF8String: [name, "\0"].concat().as_ptr()
            ];
            let sound: *mut Object = msg_send![class!(NSSound), soundNamed: ns_name];
            if sound.is_null() {
                return;
            }
            let _: () = msg_send![sound, setVolume: VOLUME];
            let _: bool = msg_send![sound, play];
            // keep the thread (and pool) alive until the short cue finishes
            std::thread::sleep(std::time::Duration::from_millis(600));
        });
    });
}

#[cfg(not(target_os = "macos"))]
fn play(_name: &'static str) {}
