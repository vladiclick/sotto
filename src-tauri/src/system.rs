// system.rs — process-level macOS settings Sotto depends on.
//
// - App Nap: a menu-bar app with no visible window is a prime App Nap
//   target; napping delays its timers and lowers its threads' priority, which
//   made push-to-talk miss presses unless the panel had just been opened.
//   An NSProcessInfo activity keeps the process un-napped for its lifetime
//   (idle system sleep and display sleep stay allowed).
// - Thread QoS: the hotkey and audio threads are latency-sensitive.
// - Logging: everything goes to ~/Library/Logs/Sotto/sotto.log, since an
//   app launched from Finder has no visible stderr.

use std::fs::{self, OpenOptions};
use std::path::PathBuf;

/// NSActivityUserInitiatedAllowingIdleSystemSleep
const ACTIVITY_USER_INITIATED_ALLOW_SLEEP: u64 = 0x00FF_FFFF & !(1 << 20);
const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;

#[cfg(target_os = "macos")]
pub fn disable_app_nap() {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    objc::rc::autoreleasepool(|| unsafe {
        let info: *mut Object = msg_send![class!(NSProcessInfo), processInfo];
        let reason: *mut Object = msg_send![
            class!(NSString),
            stringWithUTF8String: c"Push-to-talk dictation must react instantly".as_ptr()
        ];
        let token: *mut Object = msg_send![
            info,
            beginActivityWithOptions: ACTIVITY_USER_INITIATED_ALLOW_SLEEP
            reason: reason
        ];
        // keep the activity alive for the whole process lifetime
        let _: *mut Object = msg_send![token, retain];
    });
}

#[cfg(not(target_os = "macos"))]
pub fn disable_app_nap() {}

/// Mark the calling thread as user-interactive (highest non-realtime QoS).
#[cfg(target_os = "macos")]
pub fn make_thread_interactive() {
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0);
    }
}

#[cfg(not(target_os = "macos"))]
pub fn make_thread_interactive() {}

pub fn log_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Logs/Sotto/sotto.log"))
}

/// Log to ~/Library/Logs/Sotto/sotto.log (rotated at 2 MB), falling back
/// to stderr. `RUST_LOG` still overrides the default level.
pub fn init_logging() {
    let mut builder = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("sotto=info,warn"),
    );
    if let Some(path) = log_path() {
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if fs::metadata(&path)
            .map(|m| m.len() > MAX_LOG_BYTES)
            .unwrap_or(false)
        {
            let _ = fs::rename(&path, path.with_extension("log.1"));
        }
        if let Ok(file) = OpenOptions::new().create(true).append(true).open(&path) {
            builder.target(env_logger::Target::Pipe(Box::new(file)));
        }
    }
    builder.init();
}
