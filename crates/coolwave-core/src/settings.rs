//! Persistent settings in `config.toml` under the user's config folder.

use crate::engine::{default_layout, Config, FanPort};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub lighting: Config,
    /// EURUX fan layout (port, name, fan count).
    pub fan_layout: Vec<FanPort>,
    /// Keep effects running after the window is closed.
    pub run_in_background: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { lighting: Config::default(), fan_layout: default_layout(), run_in_background: true }
    }
}

/// `$XDG_CONFIG_HOME/coolwave` on Linux, `%APPDATA%\\coolwave` on Windows.
pub fn config_dir() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.unwrap_or_else(|| PathBuf::from(".")).join("coolwave")
}

pub fn path() -> PathBuf {
    config_dir().join("config.toml")
}

impl Settings {
    /// Load settings; missing or unreadable files fall back to defaults.
    pub fn load() -> Self {
        std::fs::read_to_string(path()).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default()
    }

    /// Write atomically (temp file + rename).
    pub fn save(&self) -> io::Result<()> {
        let dir = config_dir();
        std::fs::create_dir_all(&dir)?;
        let text = toml::to_string_pretty(self).map_err(io::Error::other)?;
        let tmp = dir.join("config.toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut s = Settings::default();
        s.lighting.aura_zones.insert(3, crate::engine::ZoneCfg { enabled: false, leds: Some(42) });
        s.lighting.params.primary = crate::Rgb::new(1, 2, 3);
        let text = toml::to_string_pretty(&s).unwrap();
        let back: Settings = toml::from_str(&text).unwrap();
        assert_eq!(back.lighting.aura_zones.get(&3).unwrap().leds, Some(42));
        assert_eq!(back.lighting.params.primary, crate::Rgb::new(1, 2, 3));
        assert!(text.contains("#010203"));
    }
}
