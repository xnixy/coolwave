//! The look Coolwave shares with Blurman: a plum shell, rose accent, and
//! rounded cards over a frosted window. `theme` holds the palette and frames,
//! `widgets` the small controls, and `window` the Windows 11 glass backdrop.

pub mod theme;
pub mod widgets;
#[cfg(windows)]
pub mod window;

pub use egui;
