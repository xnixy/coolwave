use adw::prelude::*;
use coolwave_core::effects::Effect;
use coolwave_core::engine::{Engine, FanPort, GpuHardware, Identify, ZoneCfg};
use coolwave_core::ene::HARDWARE_MODES;
use coolwave_core::eurux::FanColors;
use coolwave_core::Rgb;
use gtk::{gdk, glib};
use std::cell::RefCell;
use std::rc::Rc;

fn to_rgb(c: &gdk::RGBA) -> Rgb {
    let f = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgb::new(f(c.red()), f(c.green()), f(c.blue()))
}

fn to_rgba(c: Rgb) -> gdk::RGBA {
    gdk::RGBA::new(c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0, 1.0)
}

fn color_button(c: Rgb) -> gtk::ColorDialogButton {
    let b = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::builder().with_alpha(false).build()));
    b.set_rgba(&to_rgba(c));
    b.set_valign(gtk::Align::Center);
    b
}

fn identify_button(engine: &Rc<Engine>, zone: Identify) -> gtk::Button {
    let b = gtk::Button::builder()
        .icon_name("find-location-symbolic")
        .tooltip_text("Identify: blink this zone white for 5 seconds")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    let engine = engine.clone();
    b.connect_clicked(move |_| engine.identify(zone, 5));
    b
}

fn scale_row(title: &str, subtitle: &str, min: f64, max: f64, step: f64, value: f64) -> (adw::ActionRow, gtk::Scale) {
    let s = gtk::Scale::with_range(gtk::Orientation::Horizontal, min, max, step);
    s.set_value(value);
    s.set_hexpand(true);
    s.set_width_request(260);
    s.set_valign(gtk::Align::Center);
    let row = adw::ActionRow::builder().title(title).subtitle(subtitle).build();
    row.add_suffix(&s);
    (row, s)
}

struct FanButtons {
    left: gtk::ColorDialogButton,
    middle: gtk::ColorDialogButton,
    right: gtk::ColorDialogButton,
}

impl FanButtons {
    fn colors(&self) -> FanColors {
        FanColors { left: to_rgb(&self.left.rgba()), middle: to_rgb(&self.middle.rgba()), right: to_rgb(&self.right.rgba()) }
    }
}

pub fn build(engine: Rc<Engine>, layout: &[FanPort]) -> gtk::Widget {
    let page = adw::PreferencesPage::new();
    let cfg = engine.config();

    // ---------------------------------------------------------- Effect
    let fx = adw::PreferencesGroup::builder().title("Effect").description("Applied to every enabled device, in sync").build();
    let names: Vec<&str> = Effect::ALL.iter().map(|e| e.name()).collect();
    let effect_row = adw::ComboRow::builder().title("Effect").model(&gtk::StringList::new(&names)).build();
    effect_row.set_selected(Effect::ALL.iter().position(|e| *e == cfg.params.effect).unwrap_or(0) as u32);
    let primary = color_button(cfg.params.primary);
    let secondary = color_button(cfg.params.secondary);
    let colors_row = adw::ActionRow::builder().title("Colors").subtitle("Primary · Secondary").build();
    colors_row.add_suffix(&primary);
    colors_row.add_suffix(&secondary);
    let (speed_row, speed) = scale_row("Speed", "", 0.1, 5.0, 0.1, cfg.params.speed as f64);
    let (bright_row, brightness) = scale_row("Brightness", "", 0.0, 100.0, 1.0, (cfg.params.brightness * 100.0) as f64);
    fx.add(&effect_row);
    fx.add(&colors_row);
    fx.add(&speed_row);
    fx.add(&bright_row);
    page.add(&fx);

    effect_row.connect_selected_notify(glib::clone!(#[strong] engine, move |r| {
        let e = Effect::ALL[(r.selected() as usize).min(Effect::ALL.len() - 1)];
        engine.update(|c| c.params.effect = e);
    }));
    primary.connect_rgba_notify(glib::clone!(#[strong] engine, move |b| {
        let c = to_rgb(&b.rgba());
        engine.update(|cfg| cfg.params.primary = c);
    }));
    secondary.connect_rgba_notify(glib::clone!(#[strong] engine, move |b| {
        let c = to_rgb(&b.rgba());
        engine.update(|cfg| cfg.params.secondary = c);
    }));
    speed.connect_value_changed(glib::clone!(#[strong] engine, move |s| {
        let v = s.value() as f32;
        engine.update(|c| c.params.speed = v);
    }));
    brightness.connect_value_changed(glib::clone!(#[strong] engine, move |s| {
        let v = (s.value() / 100.0) as f32;
        engine.update(|c| c.params.brightness = v);
    }));

    // ---------------------------------------------------------- Devices
    let devices = adw::PreferencesGroup::builder().title("Devices").build();
    let eurux_row = adw::SwitchRow::builder().title("ROG EURUX fans").active(cfg.eurux_enabled).build();
    let aura_row = adw::SwitchRow::builder().title("Motherboard, AIO and Polymo panel").active(cfg.aura_enabled).build();
    let gpu_row = adw::SwitchRow::builder().title("ROG Astral RTX 5090").active(cfg.gpu_enabled).build();
    gpu_row.add_suffix(&identify_button(&engine, Identify::Gpu));
    for r in [&eurux_row, &aura_row, &gpu_row] {
        devices.add(r);
    }
    // GPU: synced software effect or the card's own animation
    let mut gpu_names = vec!["Synced with Coolwave effect"];
    gpu_names.extend(HARDWARE_MODES.iter().map(|(_, n)| *n));
    let gpu_mode = adw::ComboRow::builder()
        .title("GPU animation")
        .subtitle("Built-in animations run on the card itself (smoothest)")
        .model(&gtk::StringList::new(&gpu_names))
        .build();
    let gpu_speed = adw::ComboRow::builder()
        .title("GPU animation speed")
        .model(&gtk::StringList::new(&["Fastest", "Fast", "Normal", "Slow", "Slowest"]))
        .selected(2)
        .build();
    if let Some(hw) = cfg.gpu_hardware {
        if let Some(i) = HARDWARE_MODES.iter().position(|(m, _)| *m == hw.mode) {
            gpu_mode.set_selected(i as u32 + 1);
        }
        gpu_speed.set_selected(hw.speed.min(4) as u32);
    }
    gpu_speed.set_visible(gpu_mode.selected() > 0);
    devices.add(&gpu_mode);
    devices.add(&gpu_speed);
    let push_gpu = Rc::new(glib::clone!(#[strong] engine, #[strong] gpu_mode, #[strong] gpu_speed, move || {
        let sel = gpu_mode.selected() as usize;
        gpu_speed.set_visible(sel > 0);
        let hw = (sel > 0).then(|| GpuHardware { mode: HARDWARE_MODES[sel - 1].0, speed: gpu_speed.selected() as u8, direction: 0 });
        engine.update(|c| c.gpu_hardware = hw);
    }));
    gpu_mode.connect_selected_notify(glib::clone!(#[strong] push_gpu, move |_| push_gpu()));
    gpu_speed.connect_selected_notify(glib::clone!(#[strong] push_gpu, move |_| push_gpu()));
    page.add(&devices);
    eurux_row.connect_active_notify(glib::clone!(#[strong] engine, move |r| {
        let on = r.is_active();
        engine.update(|c| c.eurux_enabled = on);
    }));
    aura_row.connect_active_notify(glib::clone!(#[strong] engine, move |r| {
        let on = r.is_active();
        engine.update(|c| c.aura_enabled = on);
    }));
    gpu_row.connect_active_notify(glib::clone!(#[strong] engine, move |r| {
        let on = r.is_active();
        engine.update(|c| c.gpu_enabled = on);
    }));

    // ---------------------------------------------------------- Motherboard zones (filled once the controller reports them)
    let zones = adw::PreferencesGroup::builder()
        .title("Motherboard headers")
        .description("Turn off headers with nothing connected and set real LED counts — fewer LEDs means smoother effects")
        .build();
    page.add(&zones);
    let zones_built = Rc::new(RefCell::new(false));

    // ---------------------------------------------------------- EURUX per-fan colors
    let custom = adw::PreferencesGroup::builder()
        .title("Per-fan colors")
        .description("Used instead of the effect when the effect is Static")
        .build();
    let custom_switch = adw::SwitchRow::builder().title("Use per-fan colors").active(cfg.eurux_custom.is_some()).build();
    custom.add(&custom_switch);
    let mut ports: Vec<Vec<FanButtons>> = Vec::new();
    for (pi, port) in layout.iter().enumerate() {
        let exp = adw::ExpanderRow::builder()
            .title(&port.name)
            .subtitle(format!("Port {} · {} fan{}", port.port + 1, port.fans, if port.fans == 1 { "" } else { "s" }))
            .build();
        exp.add_suffix(&identify_button(&engine, Identify::EuruxPort(port.port)));
        let mut fans = Vec::new();
        for f in 0..port.fans {
            let row = adw::ActionRow::builder().title(format!("Fan {}", f + 1)).subtitle("Left · Middle · Right").build();
            let saved = cfg.eurux_custom.as_ref().and_then(|c| c.get(pi)?.get(f).copied()).unwrap_or(FanColors::uniform(cfg.params.primary));
            let fb = FanButtons { left: color_button(saved.left), middle: color_button(saved.middle), right: color_button(saved.right) };
            row.add_suffix(&fb.left);
            row.add_suffix(&fb.middle);
            row.add_suffix(&fb.right);
            exp.add_row(&row);
            fans.push(fb);
        }
        custom.add(&exp);
        ports.push(fans);
    }
    page.add(&custom);
    let ports = Rc::new(ports);
    let push_custom = Rc::new(glib::clone!(#[strong] engine, #[strong] ports, #[strong] custom_switch, move || {
        let value = custom_switch
            .is_active()
            .then(|| ports.iter().map(|fans| fans.iter().map(FanButtons::colors).collect()).collect());
        engine.update(|c| c.eurux_custom = value);
    }));
    custom_switch.connect_active_notify(glib::clone!(#[strong] push_custom, move |_| push_custom()));
    for fans in ports.iter() {
        for fb in fans {
            for b in [&fb.left, &fb.middle, &fb.right] {
                b.connect_rgba_notify(glib::clone!(#[strong] push_custom, move |_| push_custom()));
            }
        }
    }

    // ---------------------------------------------------------- live status
    glib::timeout_add_seconds_local(1, glib::clone!(#[strong] engine, move || {
        let st = engine.status();
        let fmt = |d: &coolwave_core::engine::DeviceStatus| {
            if d.state.starts_with("Connected") && d.fps > 0.0 { format!("{} · {:.0} fps", d.state, d.fps) } else { d.state.clone() }
        };
        eurux_row.set_subtitle(&fmt(&st.eurux));
        aura_row.set_subtitle(&fmt(&st.aura));
        gpu_row.set_subtitle(&fmt(&st.gpu));

        if !*zones_built.borrow() && !st.aura_zones.is_empty() {
            *zones_built.borrow_mut() = true;
            for z in st.aura_zones {
                let row = adw::ActionRow::builder().title(&z.name).subtitle(format!("Header {} · controller reports {} LEDs", z.index + 1, z.reported_leds)).build();
                let saved = engine.config().aura_zones.get(&z.index).copied().unwrap_or_default();
                let count = gtk::SpinButton::with_range(1.0, 500.0, 1.0);
                count.set_value(saved.leds.unwrap_or(z.reported_leds) as f64);
                count.set_valign(gtk::Align::Center);
                let on = gtk::Switch::builder().active(saved.enabled).valign(gtk::Align::Center).build();
                row.add_suffix(&identify_button(&engine, Identify::Aura(z.index)));
                row.add_suffix(&count);
                row.add_suffix(&on);
                let idx = z.index;
                let apply = Rc::new(glib::clone!(#[strong] engine, #[strong] count, #[strong] on, move || {
                    let zc = ZoneCfg { enabled: on.is_active(), leds: Some(count.value() as u16) };
                    engine.update(|c| {
                        c.aura_zones.insert(idx, zc);
                    });
                }));
                count.connect_value_changed(glib::clone!(#[strong] apply, move |_| apply()));
                on.connect_active_notify(glib::clone!(#[strong] apply, move |_| apply()));
                zones.add(&row);
            }
        }
        glib::ControlFlow::Continue
    }));

    page.upcast()
}
