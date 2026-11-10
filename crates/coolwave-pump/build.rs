//! Picks the window toolkit: egui on Windows (and anywhere with the `egui`
//! feature), GTK otherwise. Code asks with `cfg(egui_ui)`.

fn main() {
    println!("cargo::rustc-check-cfg=cfg(egui_ui)");
    let for_windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    if for_windows || std::env::var_os("CARGO_FEATURE_EGUI").is_some() {
        println!("cargo::rustc-cfg=egui_ui");
    }
}
