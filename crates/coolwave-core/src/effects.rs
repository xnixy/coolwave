//! Software lighting effects. Every effect is a pure function of time and LED
//! position, so all devices stay in sync and the same effect works on any zone.

use crate::color::Rgb;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Effect {
    Off,
    Static,
    Breathing,
    Strobe,
    Spectrum,
    RainbowWave,
    ColorWave,
    Gradient,
    Comet,
    Twinkle,
    Fire,
    Temperature,
    // Zone-aware effects (use fan middle/left/right and strip direction)
    Reactor,
    Spin,
    Scanner,
    Aurora,
    Meteor,
    DualTone,
    Ripple,
}

impl Effect {
    pub const ALL: [Effect; 19] = [
        Effect::Aurora,
        Effect::Reactor,
        Effect::Spin,
        Effect::Scanner,
        Effect::Meteor,
        Effect::Ripple,
        Effect::DualTone,
        Effect::Static,
        Effect::Breathing,
        Effect::Strobe,
        Effect::Spectrum,
        Effect::RainbowWave,
        Effect::ColorWave,
        Effect::Gradient,
        Effect::Comet,
        Effect::Twinkle,
        Effect::Fire,
        Effect::Temperature,
        Effect::Off,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Effect::Off => "Off",
            Effect::Static => "Static",
            Effect::Breathing => "Breathing",
            Effect::Strobe => "Strobe",
            Effect::Spectrum => "Spectrum cycle",
            Effect::RainbowWave => "Rainbow wave",
            Effect::ColorWave => "Color wave",
            Effect::Gradient => "Gradient",
            Effect::Comet => "Comet",
            Effect::Twinkle => "Twinkle",
            Effect::Fire => "Fire",
            Effect::Temperature => "CPU temperature",
            Effect::Reactor => "Reactor",
            Effect::Spin => "Spin",
            Effect::Scanner => "Scanner",
            Effect::Aurora => "Aurora",
            Effect::Meteor => "Meteor shower",
            Effect::DualTone => "Dual tone",
            Effect::Ripple => "Ripple",
        }
    }

    /// Which user colors this effect draws with, as (primary, secondary).
    pub fn uses_colors(self) -> (bool, bool) {
        match self {
            Effect::Off | Effect::Spectrum | Effect::RainbowWave | Effect::Fire | Effect::Temperature | Effect::Aurora => (false, false),
            Effect::Static | Effect::Breathing | Effect::Strobe => (true, false),
            Effect::Ripple => (false, true),
            Effect::ColorWave
            | Effect::Gradient
            | Effect::Comet
            | Effect::Twinkle
            | Effect::Reactor
            | Effect::Spin
            | Effect::Scanner
            | Effect::Meteor
            | Effect::DualTone => (true, true),
        }
    }

    /// Effects whose output never changes over time (written once, not streamed).
    pub fn is_static(self) -> bool {
        matches!(self, Effect::Off | Effect::Static | Effect::Gradient)
    }
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Params {
    pub effect: Effect,
    pub primary: Rgb,
    pub secondary: Rgb,
    /// Cycles per second-ish multiplier, 0.1..=5.0.
    pub speed: f32,
    /// 0.0..=1.0
    pub brightness: f32,
    /// CPU temperature for the Temperature effect (°C), filled in live.
    #[serde(skip)]
    pub cpu_temp: f32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            effect: Effect::RainbowWave,
            primary: Rgb::new(0, 120, 255),
            secondary: Rgb::new(255, 0, 140),
            speed: 1.0,
            brightness: 1.0,
            cpu_temp: 40.0,
        }
    }
}

/// What a single LED physically is, so effects can treat fan zones and strips differently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneKind {
    FanLeft,
    FanMiddle,
    FanRight,
    /// An addressable strip: motherboard header, AIO ring, Polymo panel, GPU bar.
    Strip,
}

#[derive(Clone, Copy, Debug)]
pub struct Pixel {
    /// 0..1 along the whole setup (fans first, then motherboard zones, then GPU).
    pub pos: f32,
    /// Stable per-LED number for randomized effects.
    pub id: u32,
    pub kind: ZoneKind,
    /// Fan: global fan number; strip: zone number.
    pub group: u32,
    /// Fan: 0..1 position among all fans; strip: same as `local`.
    pub group_pos: f32,
    /// 0..1 position inside this strip (fans: 0 left, 0.5 middle, 1 right).
    pub local: f32,
}

impl Pixel {
    pub fn strip(pos: f32, id: u32) -> Self {
        Self { pos, id, kind: ZoneKind::Strip, group: 0, group_pos: pos, local: pos }
    }
}

fn pulse(phase: f32, sharp: i32) -> f32 {
    let k = 0.5 + 0.5 * (phase.fract() * std::f32::consts::TAU).cos();
    k.powi(sharp)
}

/// Color for one LED with full zone information. Zone-aware effects use `px.kind`;
/// the classic effects only use the global position.
pub fn render(p: &Params, t: f32, px: &Pixel) -> Rgb {
    let s = p.speed.clamp(0.05, 10.0);
    let fan = px.kind != ZoneKind::Strip;
    let c = match p.effect {
        // Core pulses first, the fan sides and the strip ends follow like a shockwave.
        Effect::Reactor => {
            let delay = match px.kind {
                ZoneKind::FanMiddle => 0.0,
                ZoneKind::FanLeft | ZoneKind::FanRight => 0.12,
                ZoneKind::Strip => (px.local - 0.5).abs() * 0.5,
            };
            let k = pulse(t * s * 0.8 - delay, 3);
            let base = if px.kind == ZoneKind::FanMiddle { p.primary } else { p.secondary };
            lerp(scale(base, 0.08), base, k)
        }
        // The three fan zones form one ring: primary steps left -> middle -> right over a
        // dimmed secondary, neighbouring fans are offset; strips run a three-segment
        // theater chase. Zones switch hard, since a fade would show a third, mixed color.
        Effect::Spin => {
            if fan {
                let slot = match px.kind {
                    ZoneKind::FanLeft => 0,
                    ZoneKind::FanMiddle => 1,
                    _ => 2,
                };
                let head = (t * s * 1.2 + px.group as f32 * 0.25).fract();
                if ((head * 3.0) as u32).min(2) == slot { p.primary } else { scale(p.secondary, 0.6) }
            } else {
                let seg = ((px.local * 24.0 - t * s * 6.0).floor() as i32).rem_euclid(3);
                if seg == 0 { p.primary } else { scale(p.secondary, 0.12) }
            }
        }
        // A bar sweeping back and forth with a fading tail (fans: left -> middle -> right).
        Effect::Scanner => {
            let x = if fan { px.pos } else { px.local };
            let ph = (t * s * 0.35).fract();
            let head = 1.0 - (2.0 * ph - 1.0).abs(); // triangle 0..1..0
            let d = (x - head).abs();
            let k = (1.0 - d / 0.18).max(0.0);
            lerp(scale(p.secondary, 0.05), p.primary, k * k)
        }
        // Slow flowing teal/green/violet bands; fan middles drift independently from sides.
        Effect::Aurora => {
            let x = match px.kind {
                ZoneKind::FanMiddle => px.group_pos * 0.6 + 0.3,
                ZoneKind::FanLeft => px.group_pos * 0.6,
                ZoneKind::FanRight => px.group_pos * 0.6 + 0.15,
                ZoneKind::Strip => px.local,
            };
            let w = t * s * 0.15;
            let n = (x * 5.0 + w).sin() * 0.5 + (x * 2.3 - w * 1.7).sin() * 0.35 + (x * 9.0 + w * 2.9).sin() * 0.15;
            let hue = 0.42 + 0.18 * n; // green .. teal .. violet
            let v = 0.45 + 0.55 * (0.5 + 0.5 * (x * 3.0 - w * 2.2).sin());
            hsv(hue, 0.85, v)
        }
        // Bright heads with long tails running along everything, randomly spaced.
        Effect::Meteor => {
            let mut best = 0.0f32;
            for m in 0..3u32 {
                let speed = 0.25 + 0.2 * hash(m, 7);
                let cycle = t * s * speed + hash(m, 13);
                let head = cycle.fract() * 1.4 - 0.2;
                let d = head - px.pos;
                if (0.0..0.22).contains(&d) {
                    best = best.max(1.0 - d / 0.22);
                }
            }
            let sparkle = if hash(px.id, (t * 20.0) as u32) > 0.97 { 0.3 } else { 0.0 };
            lerp(scale(p.secondary, 0.04), p.primary, (best * best + sparkle).min(1.0))
        }
        // Fan middle = primary, sides = secondary, strips blend; gentle breathing.
        Effect::DualTone => {
            let k = 0.75 + 0.25 * pulse(t * s * 0.3, 1);
            let c = match px.kind {
                ZoneKind::FanMiddle => p.primary,
                ZoneKind::FanLeft | ZoneKind::FanRight => p.secondary,
                ZoneKind::Strip => lerp(p.primary, p.secondary, px.local),
            };
            scale(c, k)
        }
        // Rings expanding from fan centres and from the middle of every strip.
        Effect::Ripple => {
            let r = match px.kind {
                ZoneKind::FanMiddle => 0.0,
                ZoneKind::FanLeft | ZoneKind::FanRight => 0.5,
                ZoneKind::Strip => (px.local - 0.5).abs() * 2.0,
            };
            let phase = t * s * 0.6 + px.group as f32 * 0.17 - r * 0.6;
            let k = pulse(phase, 4);
            lerp(scale(p.secondary, 0.1), hsv(phase * 0.05 + px.group as f32 * 0.08, 0.9, 1.0), k)
        }
        _ => return color_at(p, t, px.pos, px.id),
    };
    scale(c, p.brightness)
}

pub fn hsv(h: f32, s: f32, v: f32) -> Rgb {
    let h = h.rem_euclid(1.0) * 6.0;
    let i = h.floor();
    let f = h - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match i as u32 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    Rgb::new((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

pub fn lerp(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Rgb::new(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b))
}

pub fn scale(c: Rgb, k: f32) -> Rgb {
    let k = k.clamp(0.0, 1.0);
    Rgb::new((c.r as f32 * k) as u8, (c.g as f32 * k) as u8, (c.b as f32 * k) as u8)
}

fn hash(a: u32, b: u32) -> f32 {
    let mut x = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65535.0
}

/// Color for one LED.
/// `t`: seconds since the effect started; `pos`: 0..1 along the whole setup;
/// `id`: stable per-LED number (used for randomized effects).
pub fn color_at(p: &Params, t: f32, pos: f32, id: u32) -> Rgb {
    let s = p.speed.clamp(0.05, 10.0);
    let c = match p.effect {
        Effect::Off => Rgb::default(),
        Effect::Static => p.primary,
        Effect::Breathing => {
            let phase = (t * s * 0.5).fract();
            let k = 0.5 - 0.5 * (phase * std::f32::consts::TAU).cos();
            scale(p.primary, k * k)
        }
        Effect::Strobe => {
            if (t * s * 4.0).fract() < 0.15 {
                p.primary
            } else {
                Rgb::default()
            }
        }
        Effect::Spectrum => hsv(t * s * 0.1, 1.0, 1.0),
        Effect::RainbowWave => hsv(pos - t * s * 0.25, 1.0, 1.0),
        Effect::ColorWave => {
            let k = 0.5 + 0.5 * ((pos * 2.0 - t * s * 0.5) * std::f32::consts::TAU).sin();
            lerp(p.primary, p.secondary, k)
        }
        Effect::Gradient => lerp(p.primary, p.secondary, pos),
        Effect::Comet => {
            let head = (t * s * 0.4).fract();
            let d = (head - pos).rem_euclid(1.0);
            let tail = 0.25;
            if d < tail {
                scale(p.primary, 1.0 - d / tail)
            } else {
                scale(p.secondary, 0.08)
            }
        }
        Effect::Twinkle => {
            let slot = (t * s * 2.0) as u32;
            let r = hash(id, slot);
            let local = (t * s * 2.0).fract();
            if r > 0.85 {
                lerp(p.secondary, p.primary, 1.0 - (local * 2.0 - 1.0).abs())
            } else {
                scale(p.secondary, 0.25)
            }
        }
        Effect::Fire => {
            let n = hash(id, (t * s * 12.0) as u32);
            let flick = 0.55 + 0.45 * n;
            scale(lerp(Rgb::new(255, 30, 0), Rgb::new(255, 160, 20), n * 0.7), flick)
        }
        Effect::Temperature => {
            // 35 °C blue -> 60 °C green -> 85 °C red
            let k = ((p.cpu_temp - 35.0) / 50.0).clamp(0.0, 1.0);
            hsv(0.66 * (1.0 - k), 1.0, 1.0)
        }
        _ => return render(p, t, &Pixel::strip(pos, id)),
    };
    scale(c, p.brightness)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_and_brightness() {
        let mut p = Params { effect: Effect::Static, primary: Rgb::new(200, 100, 50), ..Default::default() };
        assert_eq!(color_at(&p, 3.0, 0.5, 7), Rgb::new(200, 100, 50));
        p.brightness = 0.5;
        assert_eq!(color_at(&p, 3.0, 0.5, 7), Rgb::new(100, 50, 25));
    }

    #[test]
    fn rainbow_moves() {
        let p = Params { effect: Effect::RainbowWave, ..Default::default() };
        assert_ne!(color_at(&p, 0.0, 0.0, 0), color_at(&p, 0.5, 0.0, 0));
    }

    #[test]
    fn dual_tone_uses_fan_zones() {
        let p = Params { effect: Effect::DualTone, primary: Rgb::new(255, 0, 0), secondary: Rgb::new(0, 0, 255), ..Default::default() };
        let mid = Pixel { kind: ZoneKind::FanMiddle, ..Pixel::strip(0.1, 1) };
        let side = Pixel { kind: ZoneKind::FanLeft, ..Pixel::strip(0.1, 2) };
        assert!(render(&p, 0.0, &mid).r > 0 && render(&p, 0.0, &mid).b == 0);
        assert!(render(&p, 0.0, &side).b > 0 && render(&p, 0.0, &side).r == 0);
    }

    #[test]
    fn spin_moves_primary_through_every_fan_zone() {
        let red = Rgb::new(255, 0, 0);
        let blue = Rgb::new(0, 0, 255);
        let p = Params { effect: Effect::Spin, primary: red, secondary: blue, ..Default::default() };
        let mut peaks = Vec::new();
        for k in [ZoneKind::FanLeft, ZoneKind::FanMiddle, ZoneKind::FanRight] {
            let px = Pixel { kind: k, ..Pixel::strip(0.1, 1) };
            let samples: Vec<Rgb> = (0..100).map(|i| render(&p, i as f32 / 120.0, &px)).collect();
            // Every zone reaches primary and falls back to secondary, and is never dark.
            assert!(samples.iter().any(|c| c.r > 240 && c.b < 20), "{k:?} never reaches primary");
            assert!(samples.iter().any(|c| c.r < 20 && c.b > 100), "{k:?} never reaches secondary");
            assert!(samples.iter().all(|c| c.r as u16 + c.b as u16 > 100), "{k:?} goes dark");
            assert!(samples.iter().all(|c| c.r == 0 || c.b == 0), "{k:?} mixes the two colors");
            peaks.push((0..100).max_by_key(|&i| samples[i].r).unwrap());
        }
        assert!(peaks[0] < peaks[1] && peaks[1] < peaks[2], "zones peak out of order: {peaks:?}");
    }

    #[test]
    fn uses_colors_matches_rendering() {
        let a = Rgb::new(255, 0, 0);
        let b = Rgb::new(0, 255, 0);
        let c = Rgb::new(0, 0, 255);
        let differs = |x: Params, y: Params| {
            (0..500).any(|i| {
                let t = i as f32 / 100.0;
                [ZoneKind::FanLeft, ZoneKind::FanMiddle, ZoneKind::FanRight, ZoneKind::Strip].into_iter().any(|kind| {
                    (0..20).any(|n| {
                        let px = Pixel { kind, local: n as f32 / 20.0, ..Pixel::strip(n as f32 / 20.0, n) };
                        render(&x, t, &px) != render(&y, t, &px)
                    })
                })
            })
        };
        for e in Effect::ALL {
            let base = Params { effect: e, primary: a, secondary: b, ..Default::default() };
            let (primary, secondary) = e.uses_colors();
            assert_eq!(differs(base, Params { primary: c, ..base }), primary, "{e:?} primary");
            assert_eq!(differs(base, Params { secondary: c, ..base }), secondary, "{e:?} secondary");
        }
    }

    #[test]
    fn every_effect_renders() {
        for e in Effect::ALL {
            let p = Params { effect: e, ..Default::default() };
            for k in [ZoneKind::FanLeft, ZoneKind::FanMiddle, ZoneKind::FanRight, ZoneKind::Strip] {
                let _ = render(&p, 1.3, &Pixel { kind: k, ..Pixel::strip(0.4, 3) });
            }
        }
    }
}
