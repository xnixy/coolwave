//! Run one effect on every device for a few seconds: `cargo run --example demo -- rainbow 8`
use coolwave_core::effects::Effect;
use coolwave_core::engine::{default_layout, Config, Engine};
use std::time::{Duration, Instant};

fn main() {
    let mut args = std::env::args().skip(1);
    let name = args.next().unwrap_or_else(|| "rainbow".into());
    let secs: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(8);
    let effect = Effect::ALL
        .into_iter()
        .find(|e| e.name().to_lowercase().starts_with(&name.to_lowercase()))
        .unwrap_or(Effect::RainbowWave);
    let only = args.next().unwrap_or_else(|| "eurux,aura,gpu".into());
    let mut cfg = Config::default();
    cfg.params.effect = effect;
    cfg.eurux_enabled = only.contains("eurux");
    cfg.aura_enabled = only.contains("aura");
    cfg.gpu_enabled = only.contains("gpu");
    let engine = Engine::start(default_layout(), cfg);
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(secs) {
        std::thread::sleep(Duration::from_secs(2));
        let s = engine.status();
        println!("[{:>2}s] {} | eurux {:.0}fps {} | aura {:.0}fps {} | gpu {:.0}fps {}", t0.elapsed().as_secs(), effect.name(), s.eurux.fps, s.eurux.state, s.aura.fps, s.aura.state, s.gpu.fps, s.gpu.state);
    }
    println!("aura zones: {:?}", engine.status().aura_zones.iter().map(|z| (z.index, z.name.clone(), z.reported_leds)).collect::<Vec<_>>());
}
