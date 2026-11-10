//! Picks the window toolkit: egui on Windows (and anywhere with the `egui`
//! feature), GTK otherwise. Code asks with `cfg(egui_ui)`. A Windows build
//! also gets its icon and version details embedded in the exe.

fn main() {
    println!("cargo::rustc-check-cfg=cfg(egui_ui)");
    let for_windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    if for_windows || std::env::var_os("CARGO_FEATURE_EGUI").is_some() {
        println!("cargo::rustc-cfg=egui_ui");
    }
    #[cfg(windows)]
    if for_windows {
        embed_exe_details();
    }
}

/// Only when building on Windows itself, where the resource compiler is at hand.
#[cfg(windows)]
fn embed_exe_details() {
    let icon = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/coolwave.ico");
    println!("cargo::rerun-if-changed={}", icon.display());
    let mut resource = winresource::WindowsResource::new();
    resource.set_icon(icon.to_str().expect("icon path"));
    resource.set("ProductName", "Coolwave");
    resource.set("FileDescription", "Coolwave");
    resource.set("OriginalFilename", "coolwave.exe");
    resource.set("LegalCopyright", "Copyright (c) 2026 xnixy. MIT license.");
    resource.compile().expect("embed the app icon");
}
