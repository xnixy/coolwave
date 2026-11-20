//! The Lighting page: the effect, its look, and the devices that play it.

use super::window::{Device, FanZone, MainWindow, Message, color_of_rgb, rgb_of};
use coolwave_core::Rgb;
use coolwave_core::effects::Effect;
use coolwave_core::ene::HARDWARE_MODES;
use coolwave_core::engine::{AuraZoneInfo, DeviceStatus, FanPort, GpuHardware, Identify, ZoneCfg};
use coolwave_core::eurux::FanColors;
use coolwave_ui::*;
use windows_reactor::*;

/// Colors for every fan of every port, in layout order.
pub type FanColorGrid = Vec<Vec<FanColors>>;

const IDENTIFY_SECONDS: u64 = 5;
const SYNCED_LABEL: &str = "Synced with the effect";
/// The GPU's own animation speeds, slowest first, as the card numbers them.
const ANIMATION_SPEEDS: [(u8, &str); 5] = [(4, "Slowest"), (3, "Slow"), (2, "Normal"), (1, "Fast"), (0, "Fastest")];
const DEFAULT_ANIMATION_SPEED: u8 = 2;
const SPEED: Range = Range::new(0.1, 5.0, 0.1);
const BRIGHTNESS: Range = Range::new(0.0, 100.0, 1.0);
const MIN_LEDS: u16 = 1;
const MAX_LEDS: u16 = 500;

/// A grid with one entry per fan in `layout`, keeping what `old` already has.
pub fn fitted(old: Option<&FanColorGrid>, layout: &[FanPort], fill: Rgb) -> FanColorGrid {
    layout
        .iter()
        .enumerate()
        .map(|(port, p)| {
            (0..p.fans).map(|fan| old.and_then(|grid| grid.get(port)?.get(fan).copied()).unwrap_or(FanColors::uniform(fill))).collect()
        })
        .collect()
}

fn state_text(status: &DeviceStatus) -> String {
    if status.state.starts_with("Connected") && status.fps > 0.0 {
        format!("{} · {:.0} fps", status.state, status.fps)
    } else {
        status.state.clone()
    }
}

impl MainWindow {
    pub(super) fn lighting_update(&mut self, message: Message) {
        let mut core = self.core_mut();
        match message {
            Message::Effect(effect) => core.set_effect(effect),
            Message::Primary(color) => core.engine.update(|config| config.params.primary = rgb_of(color)),
            Message::Secondary(color) => core.engine.update(|config| config.params.secondary = rgb_of(color)),
            Message::Speed(value) => core.engine.update(|config| config.params.speed = SPEED.snap(value) as f32),
            Message::Brightness(value) => core.engine.update(|config| config.params.brightness = (BRIGHTNESS.snap(value) / 100.0) as f32),
            Message::Device(device, on) => core.engine.update(|config| match device {
                Device::Eurux => config.eurux_enabled = on,
                Device::Aura => config.aura_enabled = on,
                Device::Gpu => config.gpu_enabled = on,
            }),
            Message::Identify(zone) => core.engine.identify(zone, IDENTIFY_SECONDS),
            Message::GpuAnimation(Some(index)) => {
                let current = core.engine.config().gpu_hardware;
                let choice = index.checked_sub(1).and_then(|mode| HARDWARE_MODES.get(mode)).map(|(mode, _)| GpuHardware {
                    mode: *mode,
                    speed: current.map_or(DEFAULT_ANIMATION_SPEED, |hw| hw.speed),
                    direction: 0,
                });
                if choice != current {
                    core.engine.update(|config| config.gpu_hardware = choice);
                }
            }
            Message::GpuSpeed(Some(index)) => {
                if let (Some(mut hw), Some((speed, _))) = (core.engine.config().gpu_hardware, ANIMATION_SPEEDS.get(index)) {
                    if hw.speed != *speed {
                        hw.speed = *speed;
                        core.engine.update(|config| config.gpu_hardware = Some(hw));
                    }
                }
            }
            Message::ZoneOn(index, on) => {
                let saved = zone_cfg(&core.engine.config().aura_zones, index);
                if saved.enabled != on {
                    core.engine.update(|config| {
                        config.aura_zones.insert(index, ZoneCfg { enabled: on, ..saved });
                    });
                }
            }
            Message::ZoneLeds(index, Some(value)) if value.is_finite() => {
                let leds = (value.round() as i64).clamp(MIN_LEDS as i64, MAX_LEDS as i64) as u16;
                let saved = zone_cfg(&core.engine.config().aura_zones, index);
                if saved.leds != Some(leds) {
                    core.engine.update(|config| {
                        config.aura_zones.insert(index, ZoneCfg { leds: Some(leds), ..saved });
                    });
                }
            }
            Message::FanColorsOn(on) => {
                let config = core.engine.config();
                if on == config.eurux_custom.is_some() {
                    return;
                }
                let custom = if on {
                    Some(fitted(self.remembered_fan_colors.as_ref(), &core.settings.fan_layout, config.params.primary))
                } else {
                    None
                };
                drop(core);
                if !on {
                    self.remembered_fan_colors = config.eurux_custom.clone();
                }
                self.core_mut().engine.update(|config| config.eurux_custom = custom);
            }
            Message::FanColor(port, fan, zone, color) => {
                let config = core.engine.config();
                let Some(grid) = config.eurux_custom.as_ref() else { return };
                let mut grid = fitted(Some(grid), &core.settings.fan_layout, config.params.primary);
                if let Some(colors) = grid.get_mut(port).and_then(|fans| fans.get_mut(fan)) {
                    let slot = match zone {
                        FanZone::Left => &mut colors.left,
                        FanZone::Middle => &mut colors.middle,
                        FanZone::Right => &mut colors.right,
                    };
                    *slot = rgb_of(color);
                    core.engine.update(|config| config.eurux_custom = Some(grid));
                }
            }
            _ => {}
        }
    }

    pub(super) fn lighting_page(&self, context: &ViewContext<Self>) -> View {
        page_frame(StackPanel::new().spacing(22.0).max_width(1100.0).children((
            page_header("Lighting", "One effect, played in sync on every device you turn on."),
            columns(1.0, 1.0, self.effect_card(context), self.look_card(context)),
            self.devices_section(context),
            self.zones_section(context),
            self.fan_colors_section(context),
        )))
    }

    /// Every effect as a button. The current one is filled.
    fn effect_card(&self, context: &ViewContext<Self>) -> View {
        let current = self.core().engine.config().params.effect;
        let buttons: Vec<View> = Effect::ALL
            .into_iter()
            .map(|effect| {
                let mut button = Button::new()
                    .horizontal_alignment(HorizontalAlignment::Stretch)
                    .vertical_alignment(VerticalAlignment::Stretch)
                    .margin(Thickness::new(0.0, 0.0, 6.0, 6.0))
                    .automation_name(effect.name())
                    .on_click(context.message(Message::Effect(effect)))
                    .content(TextBlock::new().text(effect.name()).text_trimming(TextTrimming::CharacterEllipsis));
                if effect == current {
                    button = button.style(ButtonStyle::Accent);
                }
                button.into()
            })
            .collect();
        padded_card(StackPanel::new().spacing(10.0).children((
            strong("Effect"),
            VariableSizedWrapGrid::new().orientation(Orientation::Horizontal).item_width(124.0).item_height(42.0).children(buttons),
        )))
        .into()
    }

    /// Colors an effect uses, then speed and brightness.
    fn look_card(&self, context: &ViewContext<Self>) -> View {
        let config = self.core().engine.config();
        let params = config.params;
        // The GPU's built-in animations take both colors whatever the effect is.
        let gpu_animation = config.gpu_enabled && config.gpu_hardware.is_some();
        let (primary, secondary) = params.effect.uses_colors();
        let (primary, secondary) = (primary || gpu_animation, secondary || gpu_animation);
        let colors: View = if primary || secondary {
            let mut buttons: Vec<View> = Vec::new();
            if primary {
                buttons.push(color_button(color_of_rgb(params.primary), "First color", context.callback(Message::Primary)));
            }
            if secondary {
                buttons.push(color_button(color_of_rgb(params.secondary), "Second color", context.callback(Message::Secondary)));
            }
            let label = match (primary, secondary) {
                (true, true) => "Colors",
                (true, false) => "Color",
                _ => "Background",
            };
            labeled(label, line(8.0, buttons))
        } else {
            labeled("Colors", muted("This effect chooses its own colors."))
        };
        let speed = params.speed as f64;
        let brightness = (params.brightness as f64 * 100.0).round();
        padded_card(StackPanel::new().spacing(12.0).children((
            strong("Look"),
            colors,
            labeled(
                "Speed",
                slider(speed, SPEED, format!("{speed:.1}x"), "Speed", !params.effect.is_static(), context.callback(Message::Speed)),
            ),
            labeled(
                "Brightness",
                slider(brightness, BRIGHTNESS, format!("{brightness:.0}%"), "Brightness", true, context.callback(Message::Brightness)),
            ),
        )))
        .into()
    }

    /// The three devices: a switch, what each is doing now, and the GPU's own animations.
    fn devices_section(&self, context: &ViewContext<Self>) -> View {
        let core = self.core();
        let status = core.engine.status();
        let config = core.engine.config();
        let device = |title: &str, on: bool, status: &DeviceStatus, which: Device, identify: Option<Identify>| -> View {
            let mut controls: Vec<View> = Vec::new();
            if let Some(zone) = identify {
                controls.push(identify_button(context, zone));
            }
            controls.push(switch(on, title, context.callback(move |on| Message::Device(which, on))));
            row(title, &state_text(status), line(12.0, controls))
        };
        let mut items = vec![
            device("ROG EURUX fans", config.eurux_enabled, &status.eurux, Device::Eurux, None),
            device("Motherboard, AIO and Polymo panel", config.aura_enabled, &status.aura, Device::Aura, None),
            device("ROG Astral RTX 5090", config.gpu_enabled, &status.gpu, Device::Gpu, Some(Identify::Gpu)),
        ];
        let current = config.gpu_hardware;
        let mut modes: Vec<&str> = vec![SYNCED_LABEL];
        modes.extend(HARDWARE_MODES.iter().map(|(_, name)| *name));
        let selected = match current {
            None => 0,
            Some(hw) => HARDWARE_MODES.iter().position(|(mode, _)| *mode == hw.mode).map_or(0, |index| index + 1),
        };
        items.push(row(
            "GPU animation",
            "Built-in animations run on the card itself, which is smoothest.",
            ComboBox::new()
                .automation_name("GPU animation")
                .min_width(220.0)
                .items_source(modes)
                .selected_index(Some(selected))
                .on_selection_changed(context.callback(Message::GpuAnimation)),
        ));
        if let Some(hw) = current {
            items.push(row(
                "Animation speed",
                "How fast the card's own animation runs.",
                ComboBox::new()
                    .automation_name("Animation speed")
                    .min_width(220.0)
                    .items_source(ANIMATION_SPEEDS.iter().map(|(_, name)| *name))
                    .selected_index(ANIMATION_SPEEDS.iter().position(|(speed, _)| *speed == hw.speed))
                    .on_selection_changed(context.callback(Message::GpuSpeed)),
            ));
        }
        section("Devices", items)
    }

    /// The Aura motherboard headers: which are in use and how many LEDs each has.
    fn zones_section(&self, context: &ViewContext<Self>) -> View {
        let core = self.core();
        let zones = core.engine.status().aura_zones;
        let saved = core.engine.config().aura_zones;
        let mut items: Vec<View> = Vec::new();
        if zones.is_empty() {
            items.push(row("No headers yet", "The headers appear once the motherboard controller is connected.", nothing()));
        }
        for zone in &zones {
            items.push(zone_row(context, zone, zone_cfg(&saved, zone.index)));
        }
        section_with(
            "Motherboard headers",
            "Turn off headers with nothing connected and set the real LED count. Fewer LEDs make smoother effects.",
            items,
        )
    }

    /// Fixed colors for each fan, used instead of the effect while it is Static.
    fn fan_colors_section(&self, context: &ViewContext<Self>) -> View {
        let core = self.core();
        let config = core.engine.config();
        let layout = &core.settings.fan_layout;
        let mut items = vec![row(
            "Use per-fan colors",
            "Replaces the effect on the fans while the effect is Static.",
            switch(config.eurux_custom.is_some(), "Use per-fan colors", context.callback(Message::FanColorsOn)),
        )];
        if let Some(grid) = config.eurux_custom.as_ref() {
            let grid = fitted(Some(grid), layout, config.params.primary);
            for (index, (port, fans)) in layout.iter().zip(grid.iter()).enumerate() {
                items.push(port_colors(context, index, port, fans));
            }
        }
        section("Per-fan colors", items)
    }
}

/// A zone's saved settings, or the defaults.
fn zone_cfg(saved: &std::collections::BTreeMap<u8, ZoneCfg>, index: u8) -> ZoneCfg {
    saved.get(&index).copied().unwrap_or_default()
}

fn zone_row(context: &ViewContext<MainWindow>, zone: &AuraZoneInfo, saved: ZoneCfg) -> View {
    let index = zone.index;
    let leds = saved.leds.unwrap_or(zone.reported_leds).clamp(MIN_LEDS, MAX_LEDS);
    let detail = format!("Header {} · controller reports {} LEDs", index + 1, zone.reported_leds);
    row(
        &zone.name,
        &detail,
        line(
            12.0,
            (
                NumberBox::new()
                    .automation_name(format!("LEDs on {}", zone.name))
                    .header("LEDs")
                    .width(130.0)
                    .minimum(MIN_LEDS as f64)
                    .maximum(MAX_LEDS as f64)
                    .small_change(1.0)
                    .spin_button_placement_mode(NumberBoxSpinButtonPlacementMode::Compact)
                    .value(Some(leds as f64))
                    .on_value_changed(context.callback(move |value| Message::ZoneLeds(index, value))),
                Border::new().vertical_alignment(VerticalAlignment::Bottom).content(identify_button(context, Identify::Aura(index))),
                Border::new().vertical_alignment(VerticalAlignment::Bottom).content(switch(
                    saved.enabled,
                    &zone.name,
                    context.callback(move |on| Message::ZoneOn(index, on)),
                )),
            ),
        ),
    )
}

fn port_colors(context: &ViewContext<MainWindow>, index: usize, port: &FanPort, fans: &[FanColors]) -> View {
    let title = format!("{} · port {} · {} fan{}", port.name, port.port + 1, port.fans, if port.fans == 1 { "" } else { "s" });
    let mut lines: Vec<View> = fans
        .iter()
        .enumerate()
        .map(|(fan, colors)| {
            let zone = |zone: FanZone, rgb: Rgb, name: &str| -> View {
                StackPanel::new()
                    .spacing(2.0)
                    .children((
                        color_button(color_of_rgb(rgb), &format!("Fan {} {}", fan + 1, name.to_lowercase()), context.callback(move |color| Message::FanColor(index, fan, zone, color))),
                        caption(name).horizontal_alignment(HorizontalAlignment::Center),
                    ))
                    .into()
            };
            Grid::new()
                .columns([GridLength::Pixel(64.0), GridLength::Auto])
                .children((
                    muted(&format!("Fan {}", fan + 1)).vertical_alignment(VerticalAlignment::Center),
                    line(10.0, (zone(FanZone::Left, colors.left, "Left"), zone(FanZone::Middle, colors.middle, "Middle"), zone(FanZone::Right, colors.right, "Right")))
                        .grid_column(1),
                ))
                .into()
        })
        .collect();
    lines.push(Border::new().horizontal_alignment(HorizontalAlignment::Left).content(identify_button(context, Identify::EuruxPort(port.port))).into());
    Border::new()
        .padding(Thickness::new(12.0, 8.0, 12.0, 8.0))
        .content(
            Expander::new()
                .horizontal_alignment(HorizontalAlignment::Stretch)
                .horizontal_content_alignment(HorizontalAlignment::Left)
                .header(TextBlock::new().text(title))
                .content(StackPanel::new().spacing(10.0).children(lines)),
        )
        .into()
}

/// A small button that blinks one zone white so it can be found in the case.
fn identify_button(context: &ViewContext<MainWindow>, zone: Identify) -> View {
    Button::new()
        .content("Identify")
        .automation_name("Identify")
        .on_click(context.message(Message::Identify(zone)))
        .tooltip(format!("Blink this zone white for {IDENTIFY_SECONDS} seconds"))
}

/// A heading and a line under it over a card of rows.
pub(super) fn section_with(heading: &str, words: &str, items: Vec<View>) -> View {
    StackPanel::new()
        .spacing(8.0)
        .children((StackPanel::new().children((strong(heading), caption(words))), card(Thickness::new(0.0, 0.0, 0.0, 0.0), rows(items))))
        .into()
}
