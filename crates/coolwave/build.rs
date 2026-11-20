//! A Windows build gets its icon and version details embedded in the exe, and the Windows App
//! Runtime its WinUI window needs staged beside it.

fn main() {
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_exe_details();
        // The runtime's files go beside coolwave.exe and a manifest naming its classes is
        // embedded, so the window runs with nothing installed. Downloaded once, from NuGet.
        windows_reactor_setup::as_self_contained();
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
