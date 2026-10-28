// perms.rs — macOS privacy-permission handling (Microphone + Accessibility).
//
// - Microphone: AVCaptureDevice authorization status; `request_microphone`
//   triggers the native TCC prompt when the state is still undetermined.
// - Accessibility (typing the transcript + the right-⌘ listener):
//   AXIsProcessTrusted via typing.rs; `prompt_accessibility` registers the
//   app in the Accessibility list and shows the system prompt.
// - `open_settings` deep-links straight to the right System Settings pane so
//   the panel can send the user to the exact checkbox.

use serde::Serialize;

#[derive(Serialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MicStatus {
    Granted,
    Denied,
    Undetermined,
}

#[derive(Serialize, Clone)]
pub struct Permissions {
    pub microphone: MicStatus,
    pub accessibility: bool,
    /// The right-⌘ listener is running (needs Accessibility; it starts by
    /// itself within a couple of seconds of the permission being granted).
    pub hotkey: bool,
}

#[cfg(target_os = "macos")]
#[link(name = "AVFoundation", kind = "framework")]
extern "C" {}

#[cfg(target_os = "macos")]
fn av_media_type_audio() -> *mut objc::runtime::Object {
    use objc::{class, msg_send, sel, sel_impl};
    // AVMediaTypeAudio == @"soun"
    unsafe { msg_send![class!(NSString), stringWithUTF8String: c"soun".as_ptr()] }
}

#[cfg(target_os = "macos")]
pub fn microphone_status() -> MicStatus {
    use objc::{class, msg_send, sel, sel_impl};
    objc::rc::autoreleasepool(|| {
        let status: i64 = unsafe {
            msg_send![
                class!(AVCaptureDevice),
                authorizationStatusForMediaType: av_media_type_audio()
            ]
        };
        match status {
            3 => MicStatus::Granted,      // AVAuthorizationStatusAuthorized
            1 | 2 => MicStatus::Denied,   // Restricted | Denied
            _ => MicStatus::Undetermined, // NotDetermined
        }
    })
}

/// Fire the native "app would like to access the microphone" prompt. No-op
/// when the decision was already made (macOS only prompts once).
#[cfg(target_os = "macos")]
pub fn request_microphone() {
    use block::ConcreteBlock;
    use objc::{class, msg_send, sel, sel_impl};
    if microphone_status() != MicStatus::Undetermined {
        return;
    }
    let handler = ConcreteBlock::new(move |granted: bool| {
        log::info!("microphone permission prompt answered: granted={granted}");
    })
    .copy();
    objc::rc::autoreleasepool(|| unsafe {
        let () = msg_send![
            class!(AVCaptureDevice),
            requestAccessForMediaType: av_media_type_audio()
            completionHandler: &*handler
        ];
    });
}

#[cfg(not(target_os = "macos"))]
pub fn microphone_status() -> MicStatus {
    MicStatus::Granted
}

#[cfg(not(target_os = "macos"))]
pub fn request_microphone() {}

pub fn check() -> Permissions {
    Permissions {
        microphone: microphone_status(),
        accessibility: crate::typing::accessibility_trusted(),
        hotkey: crate::hotkey::active(),
    }
}

/// Ask macOS to add Sotto to the Accessibility list and show its "would
/// like to control this computer" prompt, so the user only flips a switch
/// instead of hunting for the app with "+". No-op once trusted.
#[cfg(target_os = "macos")]
pub fn prompt_accessibility() {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::{CFString, CFStringRef};

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        static kAXTrustedCheckOptionPrompt: CFStringRef;
        fn AXIsProcessTrustedWithOptions(
            options: core_foundation::dictionary::CFDictionaryRef,
        ) -> u8;
    }

    if crate::typing::accessibility_trusted() {
        return;
    }
    unsafe {
        let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let opts = CFDictionary::from_CFType_pairs(&[(key, CFBoolean::true_value())]);
        AXIsProcessTrustedWithOptions(opts.as_concrete_TypeRef());
    }
}

#[cfg(not(target_os = "macos"))]
pub fn prompt_accessibility() {}

/// Deep-link to the exact System Settings privacy pane.
pub fn open_settings_pane(pane: &str) -> Result<(), String> {
    let anchor = match pane {
        "microphone" => "Privacy_Microphone",
        "accessibility" => "Privacy_Accessibility",
        _ => return Err(format!("unknown settings pane: {pane}")),
    };
    let url = format!("x-apple.systempreferences:com.apple.preference.security?{anchor}");
    std::process::Command::new("/usr/bin/open")
        .arg(url)
        .spawn()
        .map_err(|e| format!("open System Settings: {e}"))?;
    Ok(())
}
