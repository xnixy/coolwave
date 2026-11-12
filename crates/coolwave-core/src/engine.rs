//! Lighting engine. Each device runs in its own thread (USB control transfers to
//! the Aura controller are slow, ~4.5 ms per 20 LEDs, and must not hold back the
//! fans or GPU). All threads share one clock and config, so effects stay in sync.

use crate::aura::Aura;
use crate::color::Rgb;
use crate::effects::{self, Effect, Params};
use crate::ene::Ene;
use crate::eurux::{Eurux, FanColors, PortStatic};
use crate::hwmon;
use std::collections::BTreeMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FanPort {
    pub port: usize,
    pub name: String,
    pub fans: usize,
}

pub fn default_layout() -> Vec<FanPort> {
    vec![
        FanPort { port: 0, name: "Bottom".into(), fans: 3 },
        FanPort { port: 1, name: "Rear".into(), fans: 1 },
        FanPort { port: 2, name: "Side".into(), fans: 2 },
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ZoneCfg {
    pub enabled: bool,
    /// LED count override; `None` uses the count reported by the controller.
    pub leds: Option<u16>,
}

impl Default for ZoneCfg {
    fn default() -> Self {
        Self { enabled: true, leds: None }
    }
}

/// How Coolwave drives one EURUX fan port.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum FanControl {
    /// Leave the controller alone.
    #[default]
    Default,
    Fixed { percent: u8 },
    /// Software curve: (°C, %) points following a hwmon temperature.
    Curve { chip: String, label: String, points: Vec<(f32, u8)> },
}

/// Linear interpolation over (°C, %) points; clamps outside the range.
pub fn curve_duty(points: &[(f32, u8)], temp: f32) -> Option<u8> {
    let first = points.first()?;
    if temp <= first.0 {
        return Some(first.1);
    }
    for w in points.windows(2) {
        let ((t0, p0), (t1, p1)) = (w[0], w[1]);
        if temp <= t1 {
            let k = if t1 > t0 { (temp - t0) / (t1 - t0) } else { 1.0 };
            return Some((p0 as f32 + (p1 as f32 - p0 as f32) * k).round() as u8);
        }
    }
    points.last().map(|p| p.1)
}

/// Built-in GPU animation run by the card itself (smooth, no CPU/I2C traffic).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GpuHardware {
    /// ENE mode number (see `ene::HARDWARE_MODES`).
    pub mode: u8,
    /// 0 = fastest .. 4 = slowest.
    pub speed: u8,
    /// 0 = forward, 1 = reverse.
    pub direction: u8,
}

/// A zone to blink white so the user can find it physically.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Identify {
    EuruxPort(usize),
    Aura(u8),
    Gpu,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Config {
    pub params: Params,
    pub eurux_enabled: bool,
    pub aura_enabled: bool,
    pub gpu_enabled: bool,
    /// Per-port, per-fan colors used instead of the effect when `Some` (Static only).
    pub eurux_custom: Option<Vec<Vec<FanColors>>>,
    /// Aura header settings keyed by header index.
    #[serde(with = "u8_keys")]
    pub aura_zones: BTreeMap<u8, ZoneCfg>,
    pub fps: u32,
    /// Use the GPU's own animation instead of the software effect.
    pub gpu_hardware: Option<GpuHardware>,
    /// Fan control per EURUX port (index = port 0..4).
    pub eurux_fans: Vec<FanControl>,
    /// Temporary "identify" blink (not saved).
    #[serde(skip)]
    pub identify: Option<(Identify, Instant)>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            params: Params::default(),
            eurux_enabled: true,
            aura_enabled: true,
            gpu_enabled: true,
            eurux_custom: None,
            aura_zones: BTreeMap::new(),
            fps: 30,
            gpu_hardware: None,
            eurux_fans: vec![FanControl::Default; crate::eurux::PORTS],
            identify: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct DeviceStatus {
    pub state: String,
    pub fps: f32,
}

#[derive(Clone, Debug, Default)]
pub struct AuraZoneInfo {
    pub index: u8,
    pub name: String,
    pub reported_leds: u16,
}

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub eurux: DeviceStatus,
    pub aura: DeviceStatus,
    pub gpu: DeviceStatus,
    pub aura_zones: Vec<AuraZoneInfo>,
    pub eurux_rpm: [u16; crate::eurux::PORTS],
    /// Duty last sent per port (None = left to the controller).
    pub eurux_duty: [Option<u8>; crate::eurux::PORTS],
}

struct Shared {
    config: Config,
    generation: u64,
    cpu_temp: f32,
    /// Virtual LED counts per device (eurux, aura, gpu) for spatial effects.
    leds: [usize; 3],
}

pub struct Engine {
    shared: Arc<Mutex<Shared>>,
    status: Arc<Mutex<Status>>,
    stop: Arc<AtomicBool>,
    handles: Vec<JoinHandle<()>>,
}

impl Engine {
    pub fn start(layout: Vec<FanPort>, config: Config) -> Self {
        let eurux_leds = layout.iter().map(|p| p.fans * 3).sum();
        let shared = Arc::new(Mutex::new(Shared { config, generation: 1, cpu_temp: 40.0, leds: [eurux_leds, 0, 0] }));
        let status = Arc::new(Mutex::new(Status::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let ctx = Ctx { shared: shared.clone(), status: status.clone(), stop: stop.clone(), clock: Instant::now() };

        let spawn = |name: &str, f: Box<dyn FnOnce() + Send>| {
            thread::Builder::new().name(name.into()).spawn(f).expect("spawn lighting thread")
        };
        let mut handles = Vec::new();
        {
            let (ctx, layout) = (ctx.clone(), layout.clone());
            handles.push(spawn("cw-eurux", Box::new(move || run_device(ctx, EuruxDev::new(layout)))));
        }
        {
            let ctx = ctx.clone();
            handles.push(spawn("cw-aura", Box::new(move || run_device(ctx, AuraDev::default()))));
        }
        {
            let ctx = ctx.clone();
            handles.push(spawn("cw-gpu", Box::new(move || run_device(ctx, GpuDev::default()))));
        }
        handles.push(spawn("cw-sensors", Box::new(move || run_sensors(ctx))));
        Self { shared, status, stop, handles }
    }

    pub fn update(&self, f: impl FnOnce(&mut Config)) {
        let mut s = self.shared.lock().unwrap();
        f(&mut s.config);
        s.generation += 1;
    }

    pub fn config(&self) -> Config {
        self.shared.lock().unwrap().config.clone()
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
    }
}

#[derive(Clone)]
struct Ctx {
    shared: Arc<Mutex<Shared>>,
    status: Arc<Mutex<Status>>,
    stop: Arc<AtomicBool>,
    clock: Instant,
}

/// Everything a device thread needs to render one frame.
struct Frame<'a> {
    cfg: &'a Config,
    params: Params,
    t: f32,
    /// Global LED offset of this device and total virtual LEDs, for spatial effects.
    offset: usize,
    total: usize,
}

impl Frame<'_> {
    /// While identifying: `Some(color)` overrides every LED of this device.
    fn identify(&self, zone: Identify) -> Option<Rgb> {
        let (target, until) = self.cfg.identify?;
        if Instant::now() >= until {
            return None;
        }
        let on = (self.t * 3.0).fract() < 0.5;
        Some(if target == zone && on { Rgb::new(255, 255, 255) } else { Rgb::default() })
    }

    fn identifying(&self) -> bool {
        self.cfg.identify.is_some_and(|(_, until)| Instant::now() < until)
    }

    /// One LED of a strip zone (`i` of `n`) whose first LED is at device-local `base`.
    fn strip(&self, base: usize, i: usize, n: usize, zone: u32) -> Rgb {
        let g = self.offset + base + i;
        let local = if n > 1 { i as f32 / (n - 1) as f32 } else { 0.5 };
        let px = effects::Pixel {
            pos: g as f32 / self.total.max(1) as f32,
            id: g as u32,
            kind: effects::ZoneKind::Strip,
            group: zone,
            group_pos: local,
            local,
        };
        effects::render(&self.params, self.t, &px)
    }

    /// One zone of a fan; `fan` is the global fan number out of `fans`.
    fn fan_zone(&self, base: usize, fan: usize, fans: usize, kind: effects::ZoneKind) -> Rgb {
        let (off, local) = match kind {
            effects::ZoneKind::FanLeft => (0, 0.0),
            effects::ZoneKind::FanMiddle => (1, 0.5),
            _ => (2, 1.0),
        };
        let g = self.offset + base + off;
        let px = effects::Pixel {
            pos: g as f32 / self.total.max(1) as f32,
            id: g as u32,
            kind,
            group: fan as u32,
            group_pos: if fans > 1 { fan as f32 / (fans - 1) as f32 } else { 0.5 },
            local,
        };
        effects::render(&self.params, self.t, &px)
    }
}

trait Device {
    /// Index into `Shared::leds` and the status slots.
    const SLOT: usize;
    fn enabled(cfg: &Config) -> bool;
    fn connected(&self) -> bool;
    fn connect(&mut self) -> io::Result<String>;
    fn disconnect(&mut self);
    fn leds(&self, cfg: &Config) -> usize;
    fn render(&mut self, f: &Frame) -> io::Result<()>;
    /// Extra status published after connecting (e.g. the zone list).
    fn publish(&self, _s: &mut Status) {}
    /// Periodic non-lighting work (fan control), called every loop.
    fn tick(&mut self, _cfg: &Config, _status: &Mutex<Status>) -> io::Result<()> {
        Ok(())
    }
}

fn status_slot(s: &mut Status, slot: usize) -> &mut DeviceStatus {
    match slot {
        0 => &mut s.eurux,
        1 => &mut s.aura,
        _ => &mut s.gpu,
    }
}

const RETRY: Duration = Duration::from_secs(5);
const STATIC_REFRESH: Duration = Duration::from_secs(10);

fn run_device<D: Device>(ctx: Ctx, mut dev: D) {
    let set_state = |msg: String| status_slot(&mut ctx.status.lock().unwrap(), D::SLOT).state = msg;
    let mut next_try = Instant::now();
    let mut last_gen = 0u64;
    let mut last_push = Instant::now() - STATIC_REFRESH;
    let mut fps_win = (Instant::now(), 0u32);

    while !ctx.stop.load(Ordering::Relaxed) {
        let (cfg, generation, cpu_temp, leds) = {
            let s = ctx.shared.lock().unwrap();
            (s.config.clone(), s.generation, s.cpu_temp, s.leds)
        };
        let tick = Instant::now();
        let frame_time = Duration::from_secs_f32(1.0 / cfg.fps.clamp(1, 60) as f32);

        if !D::enabled(&cfg) {
            if dev.connected() {
                dev.disconnect();
            }
            set_state("Disabled".into());
            thread::sleep(Duration::from_millis(200));
            continue;
        }
        if !dev.connected() {
            if Instant::now() >= next_try {
                next_try = Instant::now() + RETRY;
                match dev.connect() {
                    Ok(msg) => {
                        set_state(msg);
                        dev.publish(&mut ctx.status.lock().unwrap());
                        last_gen = 0;
                    }
                    Err(e) => set_state(format!("Unavailable: {e}")),
                }
            }
            if !dev.connected() {
                thread::sleep(Duration::from_millis(200));
                continue;
            }
        }

        if let Err(e) = dev.tick(&cfg, &ctx.status) {
            dev.disconnect();
            set_state(format!("Error: {e} (retrying)"));
            continue;
        }
        let my_leds = dev.leds(&cfg);
        if leds[D::SLOT] != my_leds {
            ctx.shared.lock().unwrap().leds[D::SLOT] = my_leds;
        }
        let identifying = cfg.identify.is_some_and(|(_, until)| Instant::now() < until + Duration::from_millis(200));
        let animated = !cfg.params.effect.is_static() || identifying;
        if animated || generation != last_gen || last_push.elapsed() >= STATIC_REFRESH {
            let mut params = cfg.params;
            params.cpu_temp = cpu_temp;
            let frame = Frame {
                cfg: &cfg,
                params,
                t: ctx.clock.elapsed().as_secs_f32(),
                offset: leds[..D::SLOT].iter().sum(),
                total: leds.iter().sum(),
            };
            if let Err(e) = dev.render(&frame) {
                dev.disconnect();
                set_state(format!("Error: {e} (retrying)"));
                continue;
            }
            last_gen = generation;
            last_push = Instant::now();
            fps_win.1 += 1;
        }
        if fps_win.0.elapsed() >= Duration::from_secs(1) {
            let fps = fps_win.1 as f32 / fps_win.0.elapsed().as_secs_f32();
            status_slot(&mut ctx.status.lock().unwrap(), D::SLOT).fps = fps;
            fps_win = (Instant::now(), 0);
        }
        thread::sleep(frame_time.saturating_sub(tick.elapsed()).max(Duration::from_millis(1)));
    }
}

fn run_sensors(ctx: Ctx) {
    while !ctx.stop.load(Ordering::Relaxed) {
        if let Some(celsius) = hwmon::cpu_temperature() {
            ctx.shared.lock().unwrap().cpu_temp = celsius;
        }
        for _ in 0..10 {
            if ctx.stop.load(Ordering::Relaxed) {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

// ---------------------------------------------------------------- EURUX

struct EuruxDev {
    layout: Vec<FanPort>,
    dev: Option<Eurux>,
    last: Vec<Vec<FanColors>>,
    next_fan_tick: Instant,
    last_duty: [Option<u8>; crate::eurux::PORTS],
    last_duty_sent: Instant,
}

impl EuruxDev {
    fn new(layout: Vec<FanPort>) -> Self {
        Self {
            layout,
            dev: None,
            last: Vec::new(),
            next_fan_tick: Instant::now(),
            last_duty: [None; crate::eurux::PORTS],
            last_duty_sent: Instant::now(),
        }
    }
}

impl Device for EuruxDev {
    const SLOT: usize = 0;
    fn enabled(cfg: &Config) -> bool {
        cfg.eurux_enabled
    }
    fn connected(&self) -> bool {
        self.dev.is_some()
    }
    fn connect(&mut self) -> io::Result<String> {
        let mut d = Eurux::open()?;
        for p in &self.layout {
            d.set_port_static(p.port, &PortStatic { fans: vec![FanColors::default(); p.fans], brightness_shift: 0 })?;
        }
        self.dev = Some(d);
        self.last = vec![Vec::new(); self.layout.len()];
        Ok("Connected".into())
    }
    fn disconnect(&mut self) {
        self.dev = None;
    }
    fn leds(&self, _cfg: &Config) -> usize {
        self.layout.iter().map(|p| p.fans * 3).sum()
    }
    fn tick(&mut self, cfg: &Config, status: &Mutex<Status>) -> io::Result<()> {
        if Instant::now() < self.next_fan_tick {
            return Ok(());
        }
        self.next_fan_tick = Instant::now() + Duration::from_secs(1);
        let dev = self.dev.as_mut().expect("connected");
        let rpm = dev.read_rpm()?;
        let mut duty = [None; crate::eurux::PORTS];
        for (port, d) in duty.iter_mut().enumerate() {
            *d = match cfg.eurux_fans.get(port) {
                Some(FanControl::Fixed { percent }) => Some(*percent),
                Some(FanControl::Curve { chip, label, points }) => {
                    // Unknown sensor: fail safe to full speed.
                    hwmon::temperature(chip, label).map_or(Some(100), |t| curve_duty(points, t))
                }
                _ => None,
            }
            .map(|v| v.clamp(crate::eurux::MIN_DUTY, 100));
        }
        if duty.iter().any(Option::is_some) && (duty != self.last_duty || self.last_duty_sent.elapsed() >= Duration::from_secs(10)) {
            dev.set_duty(duty)?;
            self.last_duty = duty;
            self.last_duty_sent = Instant::now();
        }
        let mut s = status.lock().unwrap();
        s.eurux_rpm = rpm;
        s.eurux_duty = duty;
        Ok(())
    }
    fn render(&mut self, f: &Frame) -> io::Result<()> {
        let dev = self.dev.as_mut().expect("connected");
        let mut base = 0usize;
        let total_fans: usize = self.layout.iter().map(|p| p.fans).sum();
        for (pi, port) in self.layout.iter().enumerate() {
            let fans: Vec<FanColors> = match (&f.cfg.eurux_custom, f.params.effect) {
                _ if f.identifying() => {
                    let c = f.identify(Identify::EuruxPort(port.port)).unwrap_or_default();
                    vec![FanColors::uniform(c); port.fans]
                }
                (Some(custom), Effect::Static) => custom
                    .get(pi)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|c| FanColors {
                        middle: effects::scale(c.middle, f.params.brightness),
                        left: effects::scale(c.left, f.params.brightness),
                        right: effects::scale(c.right, f.params.brightness),
                    })
                    .collect(),
                _ => (0..port.fans)
                    .map(|fan| {
                        let b = base + fan * 3;
                        let gf = b / 3; // global fan number
                        FanColors {
                            left: f.fan_zone(b, gf, total_fans, effects::ZoneKind::FanLeft),
                            middle: f.fan_zone(b, gf, total_fans, effects::ZoneKind::FanMiddle),
                            right: f.fan_zone(b, gf, total_fans, effects::ZoneKind::FanRight),
                        }
                    })
                    .collect(),
            };
            base += port.fans * 3;
            if self.last[pi] != fans {
                dev.set_port_colors(port.port, &fans)?;
                self.last[pi] = fans;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- Aura

/// Direct mode is not saved on the controller, so it falls back to its stored
/// effect whenever it resets (sleep, restart) while the USB handle stays valid
/// and writes keep succeeding. Switching it back makes the lights blink, so it
/// is only done when a reset is likely: this long after connecting or waking.
const AURA_SETTLE_SECS: [u64; 3] = [5, 20, 60];
/// The loop standing still this long, or the clock jumping, means the computer slept.
const AURA_WAKE_GAP: Duration = Duration::from_secs(5);

#[derive(Default)]
struct AuraDev {
    dev: Option<Aura>,
    last: BTreeMap<u8, Vec<Rgb>>,
    /// When the loop last ran, on both clocks (only one of them counts sleep, depending on the OS).
    seen: Option<(Instant, std::time::SystemTime)>,
    /// Times at which to switch back to direct mode.
    reassert: Vec<Instant>,
}

impl AuraDev {
    fn settle(now: Instant) -> impl Iterator<Item = Instant> {
        AURA_SETTLE_SECS.into_iter().map(move |s| now + Duration::from_secs(s))
    }
}

impl AuraDev {
    fn zones(&self, cfg: &Config) -> Vec<(u8, usize)> {
        let Some(d) = &self.dev else { return Vec::new() };
        d.headers
            .iter()
            .filter_map(|h| {
                let z = cfg.aura_zones.get(&h.index).copied().unwrap_or_default();
                z.enabled.then(|| (h.index, z.leds.unwrap_or(h.leds) as usize))
            })
            .collect()
    }
}

impl Device for AuraDev {
    const SLOT: usize = 1;
    fn enabled(cfg: &Config) -> bool {
        cfg.aura_enabled
    }
    fn connected(&self) -> bool {
        self.dev.is_some()
    }
    fn connect(&mut self) -> io::Result<String> {
        let mut d = Aura::open()?;
        d.enter_direct()?;
        let msg = format!("Connected ({})", d.firmware);
        self.dev = Some(d);
        self.last.clear();
        self.seen = None;
        self.reassert = Self::settle(Instant::now()).collect();
        Ok(msg)
    }
    fn disconnect(&mut self) {
        self.dev = None;
    }
    fn leds(&self, cfg: &Config) -> usize {
        self.zones(cfg).iter().map(|z| z.1).sum()
    }
    fn tick(&mut self, _cfg: &Config, _status: &Mutex<Status>) -> io::Result<()> {
        let (now, wall) = (Instant::now(), std::time::SystemTime::now());
        let woke = self.seen.is_some_and(|(then, then_wall)| {
            now.duration_since(then) > AURA_WAKE_GAP || wall.duration_since(then_wall).map_or(true, |d| d > AURA_WAKE_GAP)
        });
        self.seen = Some((now, wall));
        if woke {
            self.reassert = std::iter::once(now).chain(Self::settle(now)).collect();
        }
        Ok(())
    }
    fn render(&mut self, f: &Frame) -> io::Result<()> {
        let zones = self.zones(f.cfg);
        let dev = self.dev.as_mut().expect("connected");
        let now = Instant::now();
        if self.reassert.iter().any(|t| *t <= now) {
            self.reassert.retain(|t| *t > now);
            dev.enter_direct()?;
            // Resend every zone too: a reset controller has lost the colors it was showing.
            self.last.clear();
        }
        let mut base = 0usize;
        for (ch, n) in zones {
            let colors: Vec<Rgb> = match f.identify(Identify::Aura(ch)) {
                Some(c) => vec![c; n],
                None => (0..n).map(|i| f.strip(base, i, n, ch as u32)).collect(),
            };
            base += n;
            if self.last.get(&ch) != Some(&colors) {
                dev.set_direct(ch, &colors)?;
                self.last.insert(ch, colors);
            }
        }
        Ok(())
    }
    fn publish(&self, s: &mut Status) {
        if let Some(d) = &self.dev {
            s.aura_zones = d
                .headers
                .iter()
                .map(|h| AuraZoneInfo { index: h.index, name: h.name(), reported_leds: h.leds })
                .collect();
        }
    }
}

// ---------------------------------------------------------------- GPU

/// GPU frames at most this often: its I2C bus goes through the NVIDIA driver,
/// and keeping it busy makes the desktop (and mouse cursor) stutter.
const GPU_FRAME: Duration = Duration::from_millis(100);

#[derive(Default)]
struct GpuDev {
    dev: Option<Ene>,
    last_frame: Option<Instant>,
    /// Hardware mode currently programmed (to avoid rewriting it every frame).
    hw_active: Option<(GpuHardware, Rgb, Rgb)>,
}

impl Device for GpuDev {
    const SLOT: usize = 2;
    fn enabled(cfg: &Config) -> bool {
        cfg.gpu_enabled
    }
    fn connected(&self) -> bool {
        self.dev.is_some()
    }
    fn connect(&mut self) -> io::Result<String> {
        let mut d = Ene::find_gpu()?;
        d.enter_direct()?;
        let msg = format!("Connected ({}, {} LEDs, i2c-{})", d.firmware, d.led_count, d.bus_number);
        self.dev = Some(d);
        self.hw_active = None;
        Ok(msg)
    }
    fn disconnect(&mut self) {
        self.dev = None;
    }
    fn leds(&self, _cfg: &Config) -> usize {
        self.dev.as_ref().map(|d| d.led_count).unwrap_or(0)
    }
    fn render(&mut self, f: &Frame) -> io::Result<()> {
        let dev = self.dev.as_mut().expect("connected");
        if let (Some(hw), false) = (f.cfg.gpu_hardware, f.identifying()) {
            let key = (hw, f.params.primary, f.params.secondary);
            if self.hw_active != Some(key) {
                let n = dev.led_count;
                let colors: Vec<Rgb> = (0..n)
                    .map(|i| effects::lerp(f.params.primary, f.params.secondary, i as f32 / n.max(2) as f32 * 2.0 % 1.0))
                    .collect();
                dev.set_effect_colors(&colors)?;
                dev.set_hardware_mode(hw.mode, hw.speed, hw.direction)?;
                self.hw_active = Some(key);
            }
            return Ok(());
        }
        if self.hw_active.take().is_some() {
            dev.enter_direct()?;
        }
        if !f.identifying() && self.last_frame.is_some_and(|t| t.elapsed() < GPU_FRAME) {
            return Ok(());
        }
        self.last_frame = Some(Instant::now());
        let colors: Vec<Rgb> = match f.identify(Identify::Gpu) {
            Some(c) => vec![c; dev.led_count],
            None => (0..dev.led_count).map(|i| f.strip(0, i, dev.led_count, 100)).collect(),
        };
        dev.set_direct(&colors)
    }
}

/// TOML tables need string keys; store header indices as "0", "1", ...
mod u8_keys {
    use super::ZoneCfg;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    pub fn serialize<S: Serializer>(m: &BTreeMap<u8, ZoneCfg>, s: S) -> Result<S::Ok, S::Error> {
        m.iter().map(|(k, v)| (k.to_string(), *v)).collect::<BTreeMap<String, ZoneCfg>>().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<BTreeMap<u8, ZoneCfg>, D::Error> {
        let m = BTreeMap::<String, ZoneCfg>::deserialize(d)?;
        m.into_iter()
            .map(|(k, v)| k.parse::<u8>().map(|k| (k, v)).map_err(serde::de::Error::custom))
            .collect()
    }
}

impl Engine {
    /// Blink one zone white for `secs` seconds (everything else goes dark).
    pub fn identify(&self, zone: Identify, secs: u64) {
        self.update(|c| c.identify = Some((zone, Instant::now() + Duration::from_secs(secs))));
    }
}

#[cfg(test)]
mod tests {
    use super::curve_duty;

    #[test]
    fn interpolates_curve() {
        let pts = [(30.0, 30), (60.0, 60), (80.0, 100)];
        assert_eq!(curve_duty(&pts, 20.0), Some(30));
        assert_eq!(curve_duty(&pts, 45.0), Some(45));
        assert_eq!(curve_duty(&pts, 70.0), Some(80));
        assert_eq!(curve_duty(&pts, 95.0), Some(100));
        assert_eq!(curve_duty(&[], 50.0), None);
    }
}
