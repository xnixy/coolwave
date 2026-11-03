//! Cycle through effects on all devices: `cargo run --release --example showcase -- [secs_per_effect]`
use coolwave_core::effects::Effect;
use coolwave_core::engine::{default_layout, Config, Engine, GpuHardware};
use coolwave_core::Rgb;
use std::time::Duration;

fn main() {
    let secs: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(10);
    let engine = Engine::start(default_layout(), Config::default());
    // (effect, speed, primary, secondary, gpu hardware mode)
    let show: &[(Effect, f32, Rgb, Rgb, Option<u8>)] = &[
        (Effect::Aurora, 1.0, Rgb::new(0, 255, 160), Rgb::new(120, 0, 255), None),
        (Effect::Reactor, 1.0, Rgb::new(0, 160, 255), Rgb::new(255, 0, 120), None),
        (Effect::Spin, 1.0, Rgb::new(255, 60, 0), Rgb::new(255, 200, 0), None),
        (Effect::Scanner, 1.0, Rgb::new(255, 0, 0), Rgb::new(40, 0, 0), None),
        (Effect::Meteor, 1.0, Rgb::new(180, 220, 255), Rgb::new(0, 20, 80), None),
        (Effect::Ripple, 1.0, Rgb::new(255, 255, 255), Rgb::new(0, 0, 40), None),
        (Effect::DualTone, 1.0, Rgb::new(255, 0, 80), Rgb::new(0, 120, 255), None),
        (Effect::Spin, 1.4, Rgb::new(0, 255, 200), Rgb::new(0, 60, 255), Some(11)), // GPU: spectrum wave
        (Effect::Reactor, 1.2, Rgb::new(255, 0, 0), Rgb::new(255, 120, 0), Some(12)), // GPU: chase rainbow pulse
        (Effect::Aurora, 1.6, Rgb::new(0, 255, 160), Rgb::new(120, 0, 255), None),
    ];
    for (effect, speed, a, b, gpu) in show {
        engine.update(|c| {
            c.params.effect = *effect;
            c.params.speed = *speed;
            c.params.primary = *a;
            c.params.secondary = *b;
            c.gpu_hardware = gpu.map(|mode| GpuHardware { mode, speed: 1, direction: 0 });
        });
        let gpu_txt = gpu.map(|m| format!(" + GPU hardware mode {m}")).unwrap_or_default();
        println!("▶ {}{}", effect.name(), gpu_txt);
        std::thread::sleep(Duration::from_secs(secs));
    }
    std::thread::sleep(Duration::from_millis(300));
}
