// frontmost.rs — which app owns the cursor when dictation starts.
//
// NSWorkspace.frontmostApplication via objc msg_send. Captured on the main
// thread at hotkey time (before recording), so the transcript is attributed
// to the app the user was actually dictating into.

#[cfg(target_os = "macos")]
pub fn frontmost_app() -> String {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    use std::ffi::CStr;
    use std::os::raw::c_char;

    objc::rc::autoreleasepool(|| unsafe {
        let ws: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
        if ws.is_null() {
            return "Unknown".into();
        }
        let app: *mut Object = msg_send![ws, frontmostApplication];
        if app.is_null() {
            return "Unknown".into();
        }
        let name: *mut Object = msg_send![app, localizedName];
        if name.is_null() {
            return "Unknown".into();
        }
        let utf8: *const c_char = msg_send![name, UTF8String];
        if utf8.is_null() {
            return "Unknown".into();
        }
        CStr::from_ptr(utf8).to_string_lossy().into_owned()
    })
}

#[cfg(not(target_os = "macos"))]
pub fn frontmost_app() -> String {
    "Unknown".into()
}

/// Map an app name onto a usage category key (see stats::CATEGORIES).
pub fn categorize(app: &str) -> &'static str {
    let a = app.to_ascii_lowercase();
    const AI: [&str; 8] = [
        "chatgpt",
        "claude",
        "cursor",
        "perplexity",
        "copilot",
        "gemini",
        "ollama",
        "lm studio",
    ];
    const WORK: [&str; 6] = ["slack", "teams", "zoom", "linear", "jira", "discord"];
    const PERSONAL: [&str; 5] = ["messages", "whatsapp", "telegram", "signal", "imessage"];
    const DOCS: [&str; 8] = [
        "notion", "obsidian", "word", "pages", "textedit", "docs", "craft", "bear",
    ];
    const EMAIL: [&str; 5] = ["mail", "outlook", "spark", "superhuman", "airmail"];

    let hit = |list: &[&str]| list.iter().any(|k| a.contains(k));
    if hit(&AI) {
        "ai"
    } else if hit(&WORK) {
        "work"
    } else if hit(&PERSONAL) {
        "personal"
    } else if hit(&DOCS) {
        "docs"
    } else if hit(&EMAIL) {
        "email"
    } else {
        "other"
    }
}
