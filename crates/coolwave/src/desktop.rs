//! Per-user desktop integration: app icon and launcher entry.

use std::path::PathBuf;

pub const ICON_NAME: &str = "io.github.xnixy.Coolwave";
const ICON_SVG: &[u8] = include_bytes!("../../../data/io.github.xnixy.Coolwave.svg");

/// PNG renders: GNOME Shell on this system has no SVG pixbuf loader, so ship bitmaps too.
pub const ICON_PNGS: &[(u32, &[u8])] = &[
    (16, include_bytes!("../../../data/icons/coolwave-16.png")),
    (22, include_bytes!("../../../data/icons/coolwave-22.png")),
    (24, include_bytes!("../../../data/icons/coolwave-24.png")),
    (32, include_bytes!("../../../data/icons/coolwave-32.png")),
    (48, include_bytes!("../../../data/icons/coolwave-48.png")),
    (64, include_bytes!("../../../data/icons/coolwave-64.png")),
    (128, include_bytes!("../../../data/icons/coolwave-128.png")),
    (256, include_bytes!("../../../data/icons/coolwave-256.png")),
    (512, include_bytes!("../../../data/icons/coolwave-512.png")),
];

fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Directory to pass as an icon theme search path (contains `hicolor/...`).
pub fn icon_theme_dir() -> PathBuf {
    data_home().join("icons")
}

fn write_if_changed(path: &PathBuf, data: &[u8]) -> std::io::Result<()> {
    if std::fs::read(path).ok().as_deref() == Some(data) {
        return Ok(());
    }
    std::fs::create_dir_all(path.parent().expect("has parent"))?;
    std::fs::write(path, data)
}

/// Install the icon and a launcher entry pointing at the running binary.
pub fn install() -> std::io::Result<()> {
    let hicolor = icon_theme_dir().join("hicolor");
    write_if_changed(&hicolor.join(format!("scalable/apps/{ICON_NAME}.svg")), ICON_SVG)?;
    for (size, png) in ICON_PNGS {
        write_if_changed(&hicolor.join(format!("{size}x{size}/apps/{ICON_NAME}.png")), png)?;
    }
    // Nudge icon caches that watch the theme directory's mtime.
    let _ = std::fs::File::open(&hicolor).and_then(|f| f.set_modified(std::time::SystemTime::now()));
    let exe = std::env::current_exe()?;
    let desktop = format!(
        "[Desktop Entry]\nType=Application\nName=Coolwave\nComment=Lighting and cooling control for ASUS ROG hardware\nExec=\"{}\"\nIcon={ICON_NAME}\nCategories=Settings;HardwareSettings;\nKeywords=RGB;lighting;fan;cooling;aura;\nStartupNotify=true\nStartupWMClass={ICON_NAME}\n",
        exe.display()
    );
    write_if_changed(&data_home().join(format!("applications/{ICON_NAME}.desktop")), desktop.as_bytes())
}

/// Tray icon pixmaps (ARGB32, network byte order) decoded from the PNG renders.
pub fn tray_pixmaps() -> Vec<ksni::Icon> {
    use gtk::gdk;
    use gtk::prelude::*;
    ICON_PNGS
        .iter()
        .filter(|(s, _)| [16, 22, 24, 32, 48, 64].contains(s))
        .filter_map(|(_, png)| {
            let tex = gdk::Texture::from_bytes(&gtk::glib::Bytes::from_static(png)).ok()?;
            let (w, h) = (tex.width(), tex.height());
            let stride = w as usize * 4;
            let mut buf = vec![0u8; stride * h as usize];
            tex.download(&mut buf, stride);
            // GDK gives premultiplied B,G,R,A (little-endian ARGB32); SNI wants straight A,R,G,B.
            let data = buf
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| {
                    let a = p[3] as u32;
                    let un = |c: u8| ((c as u32 * 255 + a / 2).checked_div(a).unwrap_or(0)).min(255) as u8;
                    [p[3], un(p[2]), un(p[1]), un(p[0])]
                })
                .collect();
            Some(ksni::Icon { width: w, height: h, data })
        })
        .collect()
}
