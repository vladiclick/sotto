fn main() {
    // whisper.cpp's Metal backend uses `@available(macOS …)` checks, which
    // compile to calls into clang's runtime (`__isPlatformVersionAtLeast`)
    // whenever the deployment target (Tauri: 12.0) is older than the checked
    // version. rustc links with -nodefaultlibs, so add that runtime by hand.
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("xcrun")
            .args(["clang", "--print-runtime-dir"])
            .output()
            .expect("xcrun clang --print-runtime-dir (install Xcode command line tools)");
        let dir = String::from_utf8(out.stdout).expect("runtime dir is UTF-8");
        println!("cargo:rustc-link-search=native={}", dir.trim());
        println!("cargo:rustc-link-lib=static=clang_rt.osx");
    }
    tauri_build::build()
}
