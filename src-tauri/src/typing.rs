// typing.rs — insert the transcript at the system cursor.
//
// Two methods (Settings → Insert text):
//   - type: CGEvent keyboard events carrying a unicode payload (keycode 0),
//     so no keymap lookup is needed and any script types correctly; never
//     touches the clipboard
//   - paste: put the text on the clipboard, press ⌘V, then restore whatever
//     was on the clipboard before (every item and type, not just text)
// Both need the Accessibility permission.
//
// Safety rails, because synthetic keystrokes go wherever focus is:
//   - wait until the user has released every modifier first — typing while
//     ⌘/⌥/⌃ is physically held would turn letters into shortcuts (⌘A, ⌘W…)
//   - every posted event carries explicit modifier flags

use std::time::{Duration, Instant};

use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

// CGEventKeyboardSetUnicodeString truncates long payloads; stay well under.
const CHUNK_UTF16: usize = 20;
const CHUNK_PAUSE: Duration = Duration::from_millis(6);
const MODIFIER_WAIT: Duration = Duration::from_secs(3);
/// kVK_ANSI_V. Layouts that move V off this key (Dvorak) usually keep ⌘
/// shortcuts on QWERTY positions, which is what this relies on.
const KEY_V: u16 = 9;
/// Time the target app gets to read the clipboard before it is restored.
const PASTE_SETTLE: Duration = Duration::from_millis(400);

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn CGEventSourceFlagsState(state: i32) -> u64;
}

const HID_SYSTEM_STATE: i32 = 1; // kCGEventSourceStateHIDSystemState

pub fn accessibility_trusted() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
}

fn modifiers_held() -> bool {
    let mask = (CGEventFlags::CGEventFlagCommand
        | CGEventFlags::CGEventFlagAlternate
        | CGEventFlags::CGEventFlagControl
        | CGEventFlags::CGEventFlagShift)
        .bits();
    unsafe { CGEventSourceFlagsState(HID_SYSTEM_STATE) & mask != 0 }
}

/// Shared preconditions for both insert methods.
fn ready_to_insert() -> Result<(), String> {
    if !accessibility_trusted() {
        return Err("Accessibility permission is off — can't insert the text".into());
    }
    let deadline = Instant::now() + MODIFIER_WAIT;
    while modifiers_held() {
        if Instant::now() > deadline {
            return Err(
                "a modifier key is still held — skipped inserting to avoid triggering shortcuts"
                    .into(),
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

pub fn type_text(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }
    ready_to_insert()?;

    let src = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| "CGEventSource create failed".to_string())?;

    for chunk in utf16_chunks(text, CHUNK_UTF16) {
        let chunk = chunk.as_slice();
        let down = CGEvent::new_keyboard_event(src.clone(), 0, true)
            .map_err(|_| "CGEvent (down) create failed".to_string())?;
        down.set_flags(CGEventFlags::empty());
        down.set_string_from_utf16_unchecked(chunk);
        down.post(CGEventTapLocation::HID);

        let up = CGEvent::new_keyboard_event(src.clone(), 0, false)
            .map_err(|_| "CGEvent (up) create failed".to_string())?;
        up.set_flags(CGEventFlags::empty());
        up.post(CGEventTapLocation::HID);

        std::thread::sleep(CHUNK_PAUSE);
    }
    Ok(())
}

/// Paste via the clipboard, then put the user's clipboard back — unless
/// something else wrote to it in the meantime (then theirs wins).
pub fn paste_text(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }
    ready_to_insert()?;
    let saved = pasteboard::snapshot();
    let ours = pasteboard::write_transient(text)?;

    let src = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| "CGEventSource create failed".to_string())?;
    for down in [true, false] {
        let ev = CGEvent::new_keyboard_event(src.clone(), KEY_V, down)
            .map_err(|_| "CGEvent (⌘V) create failed".to_string())?;
        ev.set_flags(CGEventFlags::CGEventFlagCommand);
        ev.post(CGEventTapLocation::HID);
    }

    std::thread::sleep(PASTE_SETTLE);
    if pasteboard::change_count() == ours {
        pasteboard::restore(&saved);
    }
    Ok(())
}

/// Minimal NSPasteboard bridge: snapshot / write / restore.
#[cfg(target_os = "macos")]
mod pasteboard {
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    type Id = *mut Object;
    /// Every item on the clipboard as (UTI, bytes) pairs.
    pub type Snapshot = Vec<Vec<(String, Vec<u8>)>>;

    const PLAIN_TEXT: &str = "public.utf8-plain-text";
    /// nspasteboard.org convention: clipboard managers don't record these.
    const TRANSIENT: [&str; 2] = [
        "org.nspasteboard.TransientType",
        "org.nspasteboard.AutoGeneratedType",
    ];

    unsafe fn ns_string(s: &str) -> Id {
        let c = CString::new(s.replace('\0', "")).unwrap_or_default();
        msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()]
    }

    unsafe fn rust_string(ns: Id) -> Option<String> {
        if ns.is_null() {
            return None;
        }
        let p: *const c_char = msg_send![ns, UTF8String];
        (!p.is_null()).then(|| CStr::from_ptr(p).to_string_lossy().into_owned())
    }

    unsafe fn general() -> Id {
        msg_send![class!(NSPasteboard), generalPasteboard]
    }

    pub fn change_count() -> isize {
        objc::rc::autoreleasepool(|| unsafe { msg_send![general(), changeCount] })
    }

    pub fn snapshot() -> Snapshot {
        objc::rc::autoreleasepool(|| unsafe {
            let mut out = Vec::new();
            let items: Id = msg_send![general(), pasteboardItems];
            if items.is_null() {
                return out;
            }
            let n: usize = msg_send![items, count];
            for i in 0..n {
                let item: Id = msg_send![items, objectAtIndex: i];
                let types: Id = msg_send![item, types];
                let tn: usize = msg_send![types, count];
                let mut entries = Vec::with_capacity(tn);
                for j in 0..tn {
                    let t: Id = msg_send![types, objectAtIndex: j];
                    let data: Id = msg_send![item, dataForType: t];
                    let (Some(name), false) = (rust_string(t), data.is_null()) else {
                        continue;
                    };
                    let len: usize = msg_send![data, length];
                    let bytes: *const u8 = msg_send![data, bytes];
                    let copy = if len == 0 || bytes.is_null() {
                        Vec::new()
                    } else {
                        std::slice::from_raw_parts(bytes, len).to_vec()
                    };
                    entries.push((name, copy));
                }
                if !entries.is_empty() {
                    out.push(entries);
                }
            }
            out
        })
    }

    /// Replace the clipboard with `text` (marked transient). Returns the
    /// pasteboard change count right after our write.
    pub fn write_transient(text: &str) -> Result<isize, String> {
        objc::rc::autoreleasepool(|| unsafe {
            let pb = general();
            let _: isize = msg_send![pb, clearContents];
            let ok: bool = msg_send![pb, setString: ns_string(text) forType: ns_string(PLAIN_TEXT)];
            if !ok {
                return Err("couldn't put the text on the clipboard".to_string());
            }
            let empty: Id = msg_send![class!(NSData), data];
            for t in TRANSIENT {
                let _: bool = msg_send![pb, setData: empty forType: ns_string(t)];
            }
            Ok(msg_send![pb, changeCount])
        })
    }

    pub fn restore(saved: &Snapshot) {
        objc::rc::autoreleasepool(|| unsafe {
            let pb = general();
            let _: isize = msg_send![pb, clearContents];
            if saved.is_empty() {
                return;
            }
            let array: Id = msg_send![class!(NSMutableArray), arrayWithCapacity: saved.len()];
            for entries in saved {
                let item: Id = msg_send![class!(NSPasteboardItem), alloc];
                let item: Id = msg_send![item, init];
                for (uti, bytes) in entries {
                    let data: Id = msg_send![
                        class!(NSData),
                        dataWithBytes: bytes.as_ptr()
                        length: bytes.len()
                    ];
                    let _: bool = msg_send![item, setData: data forType: ns_string(uti)];
                }
                let _: () = msg_send![array, addObject: item];
                let _: () = msg_send![item, release]; // the array holds it now
            }
            let _: bool = msg_send![pb, writeObjects: array];
        })
    }
}

#[cfg(not(target_os = "macos"))]
mod pasteboard {
    pub type Snapshot = Vec<Vec<(String, Vec<u8>)>>;
    pub fn change_count() -> isize {
        0
    }
    pub fn snapshot() -> Snapshot {
        Vec::new()
    }
    pub fn write_transient(_text: &str) -> Result<isize, String> {
        Err("paste is only supported on macOS".into())
    }
    pub fn restore(_saved: &Snapshot) {}
}

/// Split into UTF-16 chunks of at most `max` units without ever cutting a
/// surrogate pair (emoji, rare CJK) in half.
fn utf16_chunks(text: &str, max: usize) -> Vec<Vec<u16>> {
    let mut chunks = Vec::new();
    let mut cur: Vec<u16> = Vec::with_capacity(max);
    let mut units = [0u16; 2];
    for ch in text.chars() {
        let enc = ch.encode_utf16(&mut units);
        if cur.len() + enc.len() > max && !cur.is_empty() {
            chunks.push(std::mem::take(&mut cur));
        }
        cur.extend_from_slice(enc);
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::{pasteboard, utf16_chunks};

    /// Touches the real clipboard (briefly) — run on purpose:
    ///   cargo test --manifest-path src-tauri/Cargo.toml clipboard -- --ignored
    #[test]
    #[ignore]
    fn clipboard_round_trip_restores_everything() {
        let before = pasteboard::snapshot();
        let ours = pasteboard::write_transient("sotto clipboard test ✓").unwrap();
        assert_eq!(pasteboard::change_count(), ours);
        let during = pasteboard::snapshot();
        assert!(during[0].iter().any(
            |(t, d)| t == "public.utf8-plain-text" && d == "sotto clipboard test ✓".as_bytes()
        ));
        assert!(during[0]
            .iter()
            .any(|(t, _)| t == "org.nspasteboard.TransientType"));
        pasteboard::restore(&before);
        assert_eq!(
            pasteboard::snapshot(),
            before,
            "clipboard not restored exactly"
        );
    }

    #[test]
    fn chunks_never_split_surrogates() {
        let text = "a😀".repeat(15); // 45 UTF-16 units
        let chunks = utf16_chunks(&text, 20);
        assert!(chunks.iter().all(|c| c.len() <= 20));
        let joined: Vec<u16> = chunks.concat();
        assert_eq!(String::from_utf16(&joined).unwrap(), text);
        for c in &chunks {
            assert!(
                String::from_utf16(c).is_ok(),
                "chunk split a surrogate pair"
            );
        }
    }
}
