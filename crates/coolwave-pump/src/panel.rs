//! The stats widgets: drawn on this computer and sent to the pump as part of
//! the picture, the way the ASUS app does it. Either over the user's photo or
//! video, or as a full dashboard on its own background.

use crate::layout::{Backdrop, Background, Layout, Style, Widget};
use crate::stats::{self, Metric, Readings, Sampler};
use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use image::{Rgb, RgbImage, Rgba, RgbaImage};
use coolwave_core::engine::{live_effect, EffectFrame};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

pub const SIZE: u32 = 720;

const WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);
const MUTED: Rgba<u8> = Rgba([206, 201, 213, 255]);
/// Under every line of text, so it holds up on a bright picture or effect.
const TEXT_SHADOW: Rgba<u8> = Rgba([0, 0, 0, 120]);

const TRACK: Rgba<u8> = Rgba([255, 255, 255, 38]);
/// Behind a widget with a backdrop.
const SHADE: Rgba<u8> = Rgba([8, 6, 12, 190]);
/// Dials open at the bottom: they start at the lower left and sweep 240°.
const DIAL_START: f32 = 150.0;
const DIAL_SWEEP: f32 = 240.0;
/// A segment that is not lit: a ghost of its shape, so the whole scale stays visible.
const GHOST: Rgba<u8> = Rgba([255, 255, 255, 60]);
/// The plate under a tile or gauge. Nearly opaque, so a bright picture behind it cannot muddy it.
const PLATE: [u8; 3] = [6, 6, 10];
const PLATE_EDGE: Rgba<u8> = Rgba([255, 255, 255, 44]);

fn fonts() -> &'static (FontRef<'static>, FontRef<'static>) {
    static FONTS: OnceLock<(FontRef<'static>, FontRef<'static>)> = OnceLock::new();
    FONTS.get_or_init(|| {
        let bold = FontRef::try_from_slice(include_bytes!("../assets/fonts/Rajdhani-Bold.ttf"))
            .expect("bundled bold font");
        let semi = FontRef::try_from_slice(include_bytes!("../assets/fonts/Rajdhani-SemiBold.ttf"))
            .expect("bundled semibold font");
        (bold, semi)
    })
}

/// Widgets on a transparent canvas, to lay over a photo or video.
pub fn overlay(readings: &Readings, widgets: &[Widget]) -> RgbaImage {
    let mut canvas = RgbaImage::new(SIZE, SIZE);
    draw_widgets(&mut canvas, readings, widgets);
    canvas
}

/// A full-screen dashboard: the background with the widgets on it.
pub fn dashboard(readings: &Readings, layout: &Layout, background: &RgbaImage) -> RgbImage {
    let mut canvas = background.clone();
    draw_widgets(&mut canvas, readings, &layout.widgets);
    flatten(&canvas)
}

/// Later widgets are drawn over earlier ones.
pub fn draw_widgets(canvas: &mut RgbaImage, readings: &Readings, widgets: &[Widget]) {
    for widget in widgets {
        match widget.style {
            Style::Text => text_widget(canvas, readings, widget),
            Style::Tile => tile_widget(canvas, readings, widget),
            Style::Ring => ring_widget(canvas, readings, widget),
            Style::Bar => bar_widget(canvas, readings, widget),
        }
    }
}

fn color_of(widget: &Widget) -> Rgba<u8> {
    let [r, g, b] = widget.color;
    Rgba([r, g, b, 255])
}

/// The value's color. The clock reads best in plain white and grey.
fn value_color(metric: Metric) -> Rgba<u8> {
    if metric == Metric::Date {
        MUTED
    } else {
        WHITE
    }
}

/// The name over a large value, both centered.
fn text_widget(canvas: &mut RgbaImage, readings: &Readings, widget: &Widget) {
    let (bold, semi) = fonts();
    let (x0, y0, w, h) = (widget.x, widget.y, widget.w, widget.h);
    if widget.backdrop {
        let radius = (h * 0.22).min(24.0);
        fill_rounded(canvas, x0, y0, x0 + w, y0 + h, radius, SHADE);
    }
    let pad = (w * 0.06).min(16.0);
    let inner = w - pad * 2.0;
    let center = x0 + w / 2.0;
    let value = readings.text(widget.metric);
    if widget.caption {
        let caption_size = h * 0.2;
        let value_size = h * 0.44;
        let gap = h * 0.08;
        let block = caption_size * 0.7 + gap + value_size * 0.7;
        let caption_baseline = y0 + (h - block) / 2.0 + caption_size * 0.7;
        text(
            canvas,
            semi,
            caption_size,
            center,
            caption_baseline,
            widget.metric.label(),
            color_of(widget),
            Align::Center,
            inner,
        );
        text(
            canvas,
            bold,
            value_size,
            center,
            caption_baseline + gap + value_size * 0.7,
            &value,
            value_color(widget.metric),
            Align::Center,
            inner,
        );
    } else {
        let size = h * 0.72;
        let font = if widget.metric == Metric::Date {
            semi
        } else {
            bold
        };
        text(
            canvas,
            font,
            size,
            center,
            y0 + h / 2.0 + size * 0.35,
            &value,
            value_color(widget.metric),
            Align::Center,
            inner,
        );
    }
}

/// What kind of reading a widget shows. Each kind has its own instrument, so
/// readings can be told apart by shape before reading a word.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    /// How busy a processor is: a segmented dial with a redline.
    Load,
    /// A temperature: a strip of blocks from blue to red.
    Heat,
    /// A clock speed: a rising staircase.
    Clock,
    /// Watts: a half-moon band.
    Power,
    /// How full memory or a disk is: a grid of cells.
    Capacity,
    /// Bytes per second: a histogram of the last minute.
    Rate,
    /// A fan or the pump: a turning rotor.
    Rotor,
    /// The clock and date have no instrument.
    Plain,
}

fn family(metric: Metric) -> Family {
    use Metric::*;
    match metric {
        CpuUsage | GpuUsage => Family::Load,
        CpuTemp | GpuTemp | CoolantTemp => Family::Heat,
        CpuClock | GpuClock => Family::Clock,
        CpuPower | GpuPower => Family::Power,
        MemoryUsage | MemoryUsed | GpuMemory | DiskUsage => Family::Capacity,
        DiskRead | DiskWrite | NetDown | NetUp => Family::Rate,
        PumpSpeed | FanSpeed | GpuFan => Family::Rotor,
        Time | Date => Family::Plain,
    }
}

/// The bottom and top of a reading's instrument. Readings without a real
/// maximum get a usual one, raised to the highest value of the last minute.
fn span(readings: &Readings, metric: Metric) -> Option<(f64, f64)> {
    use Metric::*;
    let value = readings.get(metric)?;
    let peak = readings.past(metric).iter().copied().fold(value.now, f64::max);
    Some(match metric {
        // The bottom fifth of a temperature scale is never reached, so it starts at 20 °C.
        CpuTemp | GpuTemp => (20.0, 100.0),
        CoolantTemp => (20.0, 60.0),
        CpuClock => (0.0, peak.max(6000.0)),
        GpuClock => (0.0, peak.max(3200.0)),
        CpuPower => (0.0, peak.max(250.0)),
        GpuPower => (0.0, peak.max(600.0)),
        // A floor keeps idle noise from filling the whole graph.
        DiskRead | DiskWrite => (0.0, (peak * 1.25).max(1_048_576.0)),
        NetDown | NetUp => (0.0, (peak * 1.25).max(65_536.0)),
        PumpSpeed | FanSpeed => (0.0, peak.max(3000.0)),
        _ => (0.0, value.max.filter(|max| *max > 0.0)?),
    })
}

/// How far along its instrument a reading is now, and at its highest in the last minute. Both 0 to 1.
fn level(readings: &Readings, metric: Metric) -> Option<(f32, f32)> {
    let (low, high) = span(readings, metric)?;
    let now = readings.get(metric)?.now;
    let peak = readings.past(metric).iter().copied().fold(now, f64::max);
    let at = |value: f64| ((value - low) / (high - low)).clamp(0.0, 1.0) as f32;
    Some((at(now), at(peak)))
}

fn mix(from: [u8; 3], to: [u8; 3], amount: f32) -> [u8; 3] {
    let amount = amount.clamp(0.0, 1.0);
    std::array::from_fn(|channel| (f32::from(from[channel]) + (f32::from(to[channel]) - f32::from(from[channel])) * amount).round() as u8)
}

fn with_alpha(color: [u8; 3], alpha: u8) -> Rgba<u8> {
    Rgba([color[0], color[1], color[2], alpha])
}

/// Captions and units: the widget's color, lightened so it reads as text.
fn tint_of(widget: &Widget) -> Rgba<u8> {
    with_alpha(mix(widget.color, [255, 255, 255], 0.45), 255)
}

/// The warning color for a redline. Red, unless the widget is red already.
fn hot_for(accent: [u8; 3]) -> [u8; 3] {
    let [r, g, b] = accent;
    if r > 200 && g < 120 && b < 130 {
        [255, 214, 90]
    } else {
        [255, 48, 56]
    }
}

/// The color of a temperature scale at a point from 0 (cold) to 1 (hot).
fn heat(at: f32) -> [u8; 3] {
    const STOPS: [(f32, [u8; 3]); 5] = [
        (0.0, [40, 200, 255]),
        (0.45, [90, 230, 120]),
        (0.65, [255, 190, 40]),
        (0.8, [255, 110, 30]),
        (1.0, [255, 40, 50]),
    ];
    let at = at.clamp(0.0, 1.0);
    for pair in STOPS.windows(2) {
        let ((from, cold), (to, hot)) = (pair[0], pair[1]);
        if at <= to {
            return mix(cold, hot, (at - from) / (to - from));
        }
    }
    STOPS[4].1
}

/// How many of `count` segments are lit. Anything above nothing lights the first one.
fn lit_of(at: f32, count: usize) -> usize {
    if at <= 0.005 {
        0
    } else {
        ((at * count as f32).round() as usize).clamp(1, count)
    }
}

/// The color of segment `index` of `count` on a scale that is `lit` segments
/// full: lit segments are solid, unlit ones a ghost of the same shape.
fn segment_color(fam: Family, accent: [u8; 3], index: usize, count: usize, lit: usize) -> Rgba<u8> {
    let on = index < lit;
    let place = (index as f32 + 0.5) / count as f32;
    match fam {
        // The whole temperature scale stays visible, so the unlit end shows how far there is to go.
        Family::Heat => with_alpha(heat(place), if on { 255 } else { 62 }),
        Family::Load if place > 0.85 => with_alpha(hot_for(accent), if on { 255 } else { 100 }),
        _ if !on => GHOST,
        // The newest lit segment is brighter, like the tip of a needle.
        _ if index + 1 == lit => with_alpha(mix(accent, [255, 255, 255], 0.5), 255),
        _ => with_alpha(accent, 255),
    }
}

/// The plate under a tile: near-black, with the top left and bottom right
/// corners cut off, a faint edge, and brackets in the widget's color at the cuts.
fn plate(canvas: &mut RgbaImage, x0: f32, y0: f32, x1: f32, y1: f32, solid: bool, accent: [u8; 3]) {
    let (w, h) = (x1 - x0, y1 - y0);
    let cut = (0.16 * w.min(h)).clamp(8.0, 22.0);
    let fill = with_alpha(PLATE, if solid { 232 } else { 130 });
    let bracket = with_alpha(accent, 255);
    for py in (y0.floor().max(0.0) as i32)..(y1.ceil().min(SIZE as f32) as i32) {
        for px in (x0.floor().max(0.0) as i32)..(x1.ceil().min(SIZE as f32) as i32) {
            let (fx, fy) = (px as f32 + 0.5, py as f32 + 0.5);
            // Distances outside each edge; the largest is the distance outside the plate.
            let top = (y0 - fy).max((cut - (fx - x0) - (fy - y0)) * std::f32::consts::FRAC_1_SQRT_2);
            let bottom = (fy - y1).max((cut - (x1 - fx) - (y1 - fy)) * std::f32::consts::FRAC_1_SQRT_2);
            let outside = (x0 - fx).max(fx - x1).max(top).max(bottom);
            let inside = (0.5 - outside).clamp(0.0, 1.0);
            if inside <= 0.0 {
                continue;
            }
            blend(canvas, px, py, fill, inside);
            if (top > -3.5 && fx < x0 + w * 0.3) || (bottom > -3.5 && fx > x1 - w * 0.3) {
                blend(canvas, px, py, bracket, inside);
            } else if outside > -2.5 {
                blend(canvas, px, py, PLATE_EDGE, inside * (outside + 2.5).clamp(0.0, 1.0));
            }
        }
    }
}

/// A band around a circle, cut into `count` segments with gaps of `gap` degrees.
/// Angles are in degrees, clockwise from three o'clock. `color` is asked per segment.
#[allow(clippy::too_many_arguments)]
fn seg_arc(
    canvas: &mut RgbaImage,
    cx: f32,
    cy: f32,
    outer: f32,
    thickness: f32,
    start: f32,
    sweep: f32,
    count: usize,
    gap: f32,
    color: impl Fn(usize) -> Rgba<u8>,
) {
    if count == 0 || sweep <= 0.0 || outer <= 0.0 {
        return;
    }
    let inner = (outer - thickness).max(0.0);
    let step = sweep / count as f32;
    for py in ((cy - outer - 1.0).floor().max(0.0) as i32)..((cy + outer + 1.0).ceil().min(SIZE as f32) as i32) {
        for px in ((cx - outer - 1.0).floor().max(0.0) as i32)..((cx + outer + 1.0).ceil().min(SIZE as f32) as i32) {
            let (dx, dy) = (px as f32 + 0.5 - cx, py as f32 + 0.5 - cy);
            let square = dx * dx + dy * dy;
            if square > (outer + 1.0) * (outer + 1.0) || square < (inner - 1.0).max(0.0).powi(2) {
                continue;
            }
            let distance = square.sqrt();
            let across = (outer - distance + 0.5).clamp(0.0, 1.0) * (distance - inner + 0.5).clamp(0.0, 1.0);
            if across <= 0.0 {
                continue;
            }
            let along = (dy.atan2(dx).to_degrees() - start).rem_euclid(360.0);
            if along >= sweep {
                continue;
            }
            let index = ((along / step) as usize).min(count - 1);
            let within = along - index as f32 * step;
            // How far inside the lit part of the segment, in pixels along the band.
            let edge = (within - gap / 2.0).min(step - gap / 2.0 - within) * distance.to_radians();
            let coverage = across * (edge + 0.5).clamp(0.0, 1.0);
            if coverage > 0.0 {
                blend(canvas, px, py, color(index), coverage);
            }
        }
    }
}

/// A row of `count` blocks with `gap` pixels between them. `lean` pushes the
/// top of every block that many pixels to the right, making parallelograms.
#[allow(clippy::too_many_arguments)]
fn seg_strip(
    canvas: &mut RgbaImage,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    count: usize,
    gap: f32,
    lean: f32,
    color: impl Fn(usize) -> Rgba<u8>,
) {
    let (width, height) = (x1 - x0 - lean, y1 - y0);
    if count == 0 || width <= 0.0 || height <= 0.0 {
        return;
    }
    let pitch = (width + gap) / count as f32;
    for py in (y0.floor().max(0.0) as i32)..(y1.ceil().min(SIZE as f32) as i32) {
        let fy = py as f32 + 0.5;
        let rows = (fy - y0 + 0.5).clamp(0.0, 1.0).min((y1 - fy + 0.5).clamp(0.0, 1.0));
        let shift = lean * (y1 - fy) / height;
        for px in (x0.floor().max(0.0) as i32)..(x1.ceil().min(SIZE as f32) as i32) {
            let along = px as f32 + 0.5 - x0 - shift;
            if along < -0.5 {
                continue;
            }
            let index = (along.max(0.0) / pitch) as usize;
            if index >= count {
                continue;
            }
            let within = along - index as f32 * pitch;
            let columns = (within + 0.5).clamp(0.0, 1.0).min((pitch - gap - within + 0.5).clamp(0.0, 1.0));
            if rows * columns > 0.0 {
                blend(canvas, px, py, color(index), rows * columns);
            }
        }
    }
}

fn fill_rect(canvas: &mut RgbaImage, x0: f32, y0: f32, x1: f32, y1: f32, color: Rgba<u8>) {
    seg_strip(canvas, x0, y0, x1, y1, 1, 0.0, 0.0, |_| color);
}

/// The value with its unit after it, smaller and in the widget's tint. Shrunk
/// together to fit `width`. `right` ends the line at `x` instead of starting it there.
#[allow(clippy::too_many_arguments)]
fn value_line(
    canvas: &mut RgbaImage,
    readings: &Readings,
    widget: &Widget,
    x: f32,
    baseline: f32,
    size: f32,
    width: f32,
    right: bool,
    color: Rgba<u8>,
) {
    let (bold, semi) = fonts();
    let (value, unit) = readings.parts(widget.metric);
    let natural = |size: f32| {
        let unit_width = if unit.is_empty() { 0.0 } else { size * 0.08 + measure(semi, size * 0.42, &unit) };
        measure(bold, size, &value) + unit_width
    };
    let size = if natural(size) > width && width > 0.0 { size * width / natural(size) } else { size };
    if size < 8.0 {
        return;
    }
    let value_width = measure(bold, size, &value);
    let start = if right { x - natural(size) } else { x };
    text(canvas, bold, size, start, baseline, &value, color, Align::Left, value_width + 2.0);
    if !unit.is_empty() {
        text(canvas, semi, size * 0.42, start + value_width + size * 0.08, baseline, &unit, tint_of(widget), Align::Left, width);
    }
}

/// The name over the value, filling a region from its top left corner.
#[allow(clippy::too_many_arguments)]
fn value_block(canvas: &mut RgbaImage, readings: &Readings, widget: &Widget, x: f32, y: f32, w: f32, h: f32, color: Rgba<u8>) {
    let (_, semi) = fonts();
    let caption_size = (h * 0.26).clamp(14.0, 30.0);
    // A caption that would not fit over the value is left out, not squeezed.
    if widget.caption && h >= 54.0 {
        text(canvas, semi, caption_size, x, y + caption_size * 0.78, widget.metric.label(), tint_of(widget), Align::Left, w);
        let size = (h - caption_size) * 0.92;
        value_line(canvas, readings, widget, x, y + h - size * 0.04, size, w, false, color);
    } else {
        let size = h * 1.1;
        value_line(canvas, readings, widget, x, y + h / 2.0 + size * 0.34, size, w, false, color);
    }
}

/// A rectangle inside a widget.
#[derive(Clone, Copy)]
struct Area {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

/// A tile: a plate with the name and value, and the instrument that suits the reading.
fn tile_widget(canvas: &mut RgbaImage, readings: &Readings, widget: &Widget) {
    let (x0, y0, w, h) = (widget.x, widget.y, widget.w, widget.h);
    plate(canvas, x0, y0, x0 + w, y0 + h, widget.backdrop, widget.color);
    let pad = (0.11 * h.min(w)).clamp(6.0, 18.0);
    let inner = Area { x: x0 + pad, y: y0 + pad, w: w - pad * 2.0, h: h - pad * 2.0 };
    let fam = family(widget.metric);
    let (at, peak) = level(readings, widget.metric).unwrap_or((0.0, 0.0));
    let mut color = value_color(widget.metric);
    match fam {
        Family::Load if at >= 0.85 => color = with_alpha(hot_for(widget.color), 255),
        Family::Capacity if at >= 0.9 => color = with_alpha(hot_for(widget.color), 255),
        Family::Heat if readings.has(widget.metric) => color = with_alpha(mix(heat(at), [255, 255, 255], 0.35), 255),
        _ => {}
    }
    // Too small for an instrument: the value alone.
    if fam == Family::Plain || w.min(h) < 80.0 {
        value_block(canvas, readings, widget, inner.x, inner.y, inner.w, inner.h, color);
        return;
    }
    let wide = w >= 1.6 * h;
    // Where the instrument goes, and where that leaves the name and value.
    let (instrument, words) = match (fam, wide) {
        // Round instruments sit on the left, as tall as the tile.
        (Family::Load | Family::Power | Family::Rotor, true) => {
            let side = inner.h.min(inner.w * 0.45);
            (
                Area { x: inner.x, y: inner.y, w: side, h: inner.h },
                Area { x: inner.x + side + pad, y: inner.y, w: inner.w - side - pad, h: inner.h },
            )
        }
        (Family::Clock | Family::Capacity, true) => (
            Area { x: inner.x + inner.w * 0.58, y: inner.y, w: inner.w * 0.42, h: inner.h },
            Area { x: inner.x, y: inner.y, w: inner.w * 0.54, h: inner.h },
        ),
        (Family::Heat, true) => (
            Area { x: inner.x, y: inner.y + inner.h * 0.8, w: inner.w, h: inner.h * 0.2 },
            Area { x: inner.x, y: inner.y, w: inner.w, h: inner.h * 0.72 },
        ),
        (Family::Rate, _) => (
            Area { x: inner.x, y: inner.y + inner.h * 0.52, w: inner.w, h: inner.h * 0.48 },
            Area { x: inner.x, y: inner.y, w: inner.w, h: inner.h * 0.44 },
        ),
        // Not wide: the instrument runs along the bottom.
        _ => (
            Area { x: inner.x, y: inner.y + inner.h * 0.62, w: inner.w, h: inner.h * 0.38 },
            Area { x: inner.x, y: inner.y, w: inner.w, h: inner.h * 0.54 },
        ),
    };
    if fam == Family::Rate {
        rate_words(canvas, readings, widget, words, color);
    } else {
        value_block(canvas, readings, widget, words.x, words.y, words.w, words.h, color);
    }
    let accent = widget.color;
    match (fam, wide) {
        (Family::Load, true) => {
            // The dial opens at the bottom, so it is one and a half radii tall.
            let radius = (instrument.h / 1.5).min(instrument.w / 2.0);
            let (cx, cy) = (instrument.x + instrument.w / 2.0, instrument.y + (instrument.h - radius * 1.5) / 2.0 + radius);
            dial(canvas, cx, cy, radius, radius * 0.32, 16, 3.0, fam, accent, at, peak);
        }
        (Family::Power, true) => {
            let radius = instrument.h.min(instrument.w / 2.0);
            let (cx, cy) = (instrument.x + instrument.w / 2.0, instrument.y + (instrument.h + radius) / 2.0);
            seg_arc(canvas, cx, cy, radius, radius * 0.4, 180.0, 180.0, 1, 0.0, |_| GHOST);
            seg_arc(canvas, cx, cy, radius, radius * 0.4, 180.0, 180.0 * at, 1, 0.0, |_| with_alpha(accent, 255));
            if peak > 0.0 {
                seg_arc(canvas, cx, cy, radius, radius * 0.4, 180.0 + 180.0 * peak - 1.5, 3.0, 1, 0.0, |_| WHITE);
            }
        }
        (Family::Rotor, true) => {
            let radius = instrument.h.min(instrument.w) / 2.0;
            rotor(canvas, instrument.x + instrument.w / 2.0, instrument.y + instrument.h / 2.0, radius, widget.metric, readings, at, accent);
        }
        (Family::Clock, _) => ladder(canvas, instrument, accent, at),
        (Family::Capacity, _) => cells(canvas, instrument, accent, at),
        (Family::Rate, _) => scope(canvas, instrument, readings, widget.metric, accent),
        // A pump or case fan has no scale to fill a strip with.
        (Family::Rotor, false) if widget.metric != Metric::GpuFan => {}
        _ => strip(canvas, instrument, fam, accent, at, peak),
    }
}

/// A dial of lit segments that opens at the bottom, with a white mark at the highest point of the last minute.
#[allow(clippy::too_many_arguments)]
fn dial(canvas: &mut RgbaImage, cx: f32, cy: f32, radius: f32, thickness: f32, count: usize, gap: f32, fam: Family, accent: [u8; 3], at: f32, peak: f32) {
    let lit = lit_of(at, count);
    seg_arc(canvas, cx, cy, radius, thickness, DIAL_START, DIAL_SWEEP, count, gap, |index| segment_color(fam, accent, index, count, lit));
    if peak > 0.0 {
        let mark = (170.0 / radius).clamp(1.5, 5.0);
        let angle = DIAL_START + (DIAL_SWEEP - mark) * peak;
        seg_arc(canvas, cx, cy, radius, thickness, angle, mark, 1, 0.0, |_| WHITE);
    }
}

/// A strip of slanted blocks along the bottom of a tile.
fn strip(canvas: &mut RgbaImage, area: Area, fam: Family, accent: [u8; 3], at: f32, peak: f32) {
    let count = ((area.w / 20.0) as usize).clamp(6, 14);
    let height = area.h.min(30.0);
    let (top, bottom) = (area.y + area.h - height, area.y + area.h);
    let lean = height * 0.35;
    let lit = lit_of(at, count);
    seg_strip(canvas, area.x, top, area.x + area.w, bottom, count, 4.0, lean, |index| segment_color(fam, accent, index, count, lit));
    if peak > 0.0 {
        let x = area.x + (area.w - lean - 3.0) * peak;
        seg_strip(canvas, x, top, x + 3.0 + lean, bottom, 1, 0.0, lean, |_| WHITE);
    }
}

/// A staircase of bars; the top step lights white.
fn ladder(canvas: &mut RgbaImage, area: Area, accent: [u8; 3], at: f32) {
    const STEPS: usize = 8;
    let lit = lit_of(at, STEPS);
    let gap = 4.0;
    let pitch = (area.w + gap) / STEPS as f32;
    let tall = area.h.min(area.w * 0.9);
    for step in 0..STEPS {
        let height = tall * (0.25 + 0.75 * step as f32 / (STEPS - 1) as f32);
        let x = area.x + pitch * step as f32;
        let color = match step {
            _ if step >= lit => GHOST,
            _ if step + 1 == STEPS => WHITE,
            _ => with_alpha(accent, 255),
        };
        fill_rect(canvas, x, area.y + area.h - height, x + pitch - gap, area.y + area.h, color);
    }
}

/// A grid of cells that fills from the bottom left, row by row, like a tank.
fn cells(canvas: &mut RgbaImage, area: Area, accent: [u8; 3], at: f32) {
    const COLUMNS: usize = 8;
    const ROWS: usize = 4;
    let pitch = (area.w / COLUMNS as f32).min(area.h / ROWS as f32);
    let gap = (pitch * 0.22).max(2.0);
    let (left, top) = (area.x + area.w - pitch * COLUMNS as f32 + gap, area.y + (area.h - pitch * ROWS as f32 + gap) / 2.0);
    let full = at * (COLUMNS * ROWS) as f32;
    let lit = if at >= 0.9 { hot_for(accent) } else { accent };
    for row in 0..ROWS {
        for column in 0..COLUMNS {
            let index = ((ROWS - 1 - row) * COLUMNS + column) as f32;
            // The cell being filled shows how far along it is.
            let share = (full - index).clamp(0.0, 1.0);
            let (x, y) = (left + pitch * column as f32, top + pitch * row as f32);
            fill_rect(canvas, x, y, x + pitch - gap, y + pitch - gap, GHOST);
            if share > 0.0 {
                fill_rect(canvas, x, y, x + pitch - gap, y + pitch - gap, with_alpha(lit, (255.0 * share) as u8));
            }
        }
    }
}

/// The last minute as columns, newest on the right and white.
fn scope(canvas: &mut RgbaImage, area: Area, readings: &Readings, metric: Metric, accent: [u8; 3]) {
    let columns = ((area.w / 9.0) as usize).clamp(8, 30);
    let pitch = area.w / columns as f32;
    let past = readings.past(metric);
    let (_, high) = span(readings, metric).unwrap_or((0.0, 1.0));
    // Each column is the higher of two seconds, so a short burst is not lost.
    let per = 2;
    for column in 0..columns {
        let age = (columns - 1 - column) * per;
        let value = (0..per)
            .filter_map(|offset| past.len().checked_sub(1 + age + offset).map(|index| past[index]))
            .fold(0.0, f64::max);
        let share = (value / high).clamp(0.0, 1.0) as f32;
        let x = area.x + pitch * column as f32;
        let (right, bottom) = (x + (pitch * 0.68).max(3.0), area.y + area.h);
        fill_rect(canvas, x, bottom - 3.0, right, bottom, GHOST);
        if share > 0.0 {
            let color = if column + 1 == columns { WHITE } else { with_alpha(accent, 255) };
            fill_rect(canvas, x, bottom - (area.h * share).max(3.0), right, bottom, color);
        }
    }
}

/// An arrow for the direction, the name, and the value on the right.
fn rate_words(canvas: &mut RgbaImage, readings: &Readings, widget: &Widget, area: Area, color: Rgba<u8>) {
    let (_, semi) = fonts();
    let caption_size = (area.h * 0.55).clamp(14.0, 28.0);
    let arrow = caption_size * 0.62;
    let down = matches!(widget.metric, Metric::NetDown | Metric::DiskWrite);
    let middle = area.y + area.h / 2.0;
    for py in (middle - arrow / 2.0).floor() as i32..(middle + arrow / 2.0).ceil() as i32 {
        let along = ((py as f32 + 0.5 - (middle - arrow / 2.0)) / arrow).clamp(0.0, 1.0);
        let half = arrow / 2.0 * if down { 1.0 - along } else { along };
        let cx = area.x + arrow / 2.0;
        fill_rect(canvas, cx - half, py as f32, cx + half, py as f32 + 1.0, with_alpha(widget.color, 255));
    }
    let mut left = area.x + arrow + 8.0;
    if widget.caption {
        let label = widget.metric.label();
        let room = area.w * 0.45 - arrow - 8.0;
        text(canvas, semi, caption_size, left, middle + caption_size * 0.33, label, tint_of(widget), Align::Left, room);
        left += measure(semi, caption_size, label).min(room.max(0.0)) + 10.0;
    }
    let size = area.h * 1.15;
    value_line(canvas, readings, widget, area.x + area.w, middle + size * 0.34, size, area.x + area.w - left, true, color);
}

/// A fan seen from the front: a ring with blades that turn faster the faster it runs.
#[allow(clippy::too_many_arguments)]
fn rotor(canvas: &mut RgbaImage, cx: f32, cy: f32, radius: f32, metric: Metric, readings: &Readings, at: f32, accent: [u8; 3]) {
    // The pump has a few fat blades, a fan more and thinner ones.
    let blades = if metric == Metric::PumpSpeed || radius < 30.0 { 3 } else { 5 };
    let speed = if readings.has(metric) { at.clamp(0.15, 1.0) } else { 0.0 };
    // Turned by the clock, so it needs no state. Slow enough that ten frames a second still read as turning.
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |elapsed| (elapsed.as_millis() % 3_600_000) as f32 / 1000.0);
    let turn = (seconds * speed * 200.0).rem_euclid(360.0);
    let ring = (radius * 0.08).max(3.0);
    seg_arc(canvas, cx, cy, radius, ring, 0.0, 360.0, 1, 0.0, |_| with_alpha(accent, 255));
    let reach = radius - ring - 3.0;
    let hub = radius * 0.26;
    let step = 360.0 / blades as f32;
    seg_arc(canvas, cx, cy, reach, reach - hub, turn, 360.0, blades, step * 0.5, |_| with_alpha(mix(accent, [255, 255, 255], 0.25), 255));
    fill_rounded(canvas, cx - hub, cy - hub, cx + hub, cy + hub, hub, WHITE);
}

/// A round gauge: a dial of segments around the value, with the name in the opening at the bottom.
fn ring_widget(canvas: &mut RgbaImage, readings: &Readings, widget: &Widget) {
    let (bold, semi) = fonts();
    let side = widget.w.min(widget.h);
    let (cx, cy) = widget.center();
    if widget.backdrop {
        fill_rounded(
            canvas,
            cx - side / 2.0,
            cy - side / 2.0,
            cx + side / 2.0,
            cy + side / 2.0,
            side / 2.0,
            with_alpha(PLATE, 232),
        );
    }
    let radius = side / 2.0 - side * 0.04;
    let thickness = radius * 0.22;
    let fam = family(widget.metric);
    let (at, peak) = level(readings, widget.metric).unwrap_or((0.0, 0.0));
    dial(canvas, cx, cy, radius, thickness, 24, 2.5, fam, widget.color, at, peak);
    // A solid ring inside the dial, where a real gauge has its scale.
    let scale = radius - thickness - (radius * 0.07).max(4.0);
    seg_arc(canvas, cx, cy, scale, 3.0, DIAL_START, DIAL_SWEEP, 1, 0.0, |_| with_alpha(widget.color, 130));
    let value_size = side * 0.26;
    let mut color = value_color(widget.metric);
    if fam == Family::Load && at >= 0.85 {
        color = with_alpha(hot_for(widget.color), 255);
    } else if fam == Family::Heat && readings.has(widget.metric) {
        color = with_alpha(mix(heat(at), [255, 255, 255], 0.35), 255);
    }
    let (value, unit) = readings.parts(widget.metric);
    let room = scale * 1.5;
    let natural = measure(bold, value_size, &value) + if unit.is_empty() { 0.0 } else { value_size * 0.08 + measure(semi, value_size * 0.42, &unit) };
    let size = if natural > room { value_size * room / natural } else { value_size };
    let width = natural * size / value_size;
    value_line(canvas, readings, widget, cx - width / 2.0, cy + size * 0.3, size, room, false, color);
    if widget.caption {
        text(
            canvas,
            semi,
            side * 0.11,
            cx,
            cy + radius * 0.74,
            widget.metric.label(),
            tint_of(widget),
            Align::Center,
            radius,
        );
    }
}

/// The name on the left and the value on the right, over a bar the width of the widget.
fn bar_widget(canvas: &mut RgbaImage, readings: &Readings, widget: &Widget) {
    let (bold, semi) = fonts();
    let (x0, y0, w, h) = (widget.x, widget.y, widget.w, widget.h);
    if widget.backdrop {
        fill_rounded(canvas, x0, y0, x0 + w, y0 + h, (h * 0.2).min(20.0), SHADE);
    }
    let pad = if widget.backdrop {
        (h * 0.16).min(18.0)
    } else {
        0.0
    };
    let inner = w - pad * 2.0;
    let thickness = (h * 0.13).max(4.0);
    let value_size = h * 0.42;
    let baseline = y0 + pad + value_size * 0.75;
    let value = readings.text(widget.metric);
    let value_width = if widget.caption { inner * 0.55 } else { inner };
    text(
        canvas,
        bold,
        value_size,
        x0 + w - pad,
        baseline,
        &value,
        value_color(widget.metric),
        Align::Right,
        value_width,
    );
    if widget.caption {
        let used = measure(bold, value_size, &value).min(value_width);
        text(
            canvas,
            semi,
            h * 0.3,
            x0 + pad,
            baseline,
            widget.metric.label(),
            MUTED,
            Align::Left,
            (inner - used - 12.0).max(10.0),
        );
    }
    bar(
        canvas,
        x0 + pad,
        y0 + h - pad - thickness,
        inner,
        thickness,
        readings.fraction(widget.metric).unwrap_or(0.0),
        color_of(widget),
    );
}

/// The dashboard's background, before any widgets.
pub fn backdrop(background: Background) -> RgbaImage {
    backdrop_from(&glow_mask(background.style), background.color)
}

/// How strongly the glow shows at each pixel, row by row. The slow part of a
/// backdrop, and the same for every color.
fn glow_mask(style: Backdrop) -> Vec<f32> {
    let mut mask = Vec::with_capacity((SIZE * SIZE) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (x, y) = (x as f32, y as f32);
            mask.push(match style {
                Backdrop::Glow => falloff(x - 560.0, y - 600.0, 620.0, 1.6),
                Backdrop::Spotlight => falloff(x - 360.0, y - 360.0, 520.0, 1.8) * 0.8,
                Backdrop::Plain => 0.0,
            });
        }
    }
    mask
}

fn backdrop_from(mask: &[f32], color: [u8; 3]) -> RgbaImage {
    let [r, g, b] = color.map(f32::from);
    let mut image = RgbaImage::new(SIZE, SIZE);
    for (pixel, glow) in image.pixels_mut().zip(mask) {
        let channel = |base: f32, color: f32| (base + color * 0.3 * glow).round().min(255.0) as u8;
        *pixel = Rgba([channel(10.0, r), channel(8.0, g), channel(13.0, b), 255]);
    }
    image
}

/// How often the dashboard is redrawn while it plays a moving lighting effect.
pub const EFFECT_FRAME: Duration = Duration::from_millis(100);
/// The effect is worked out on a grid this many cells wide and then smoothed
/// up to the screen, which is far cheaper than one color per pixel.
const FIELD: usize = 240;
/// Randomized effects (twinkle, fire) get one number per block of this many cells, so their spots are large enough to see.
const FIELD_BLOCK: usize = 12;

/// The lighting effect as a full-screen background.
fn effect_backdrop(effect: &EffectFrame) -> RgbaImage {
    let mut cells = Vec::with_capacity(FIELD * FIELD);
    for row in 0..FIELD {
        for column in 0..FIELD {
            let (u, v) = ((column as f32 + 0.5) / FIELD as f32, (row as f32 + 0.5) / FIELD as f32);
            let block = (row / FIELD_BLOCK * FIELD + column / FIELD_BLOCK) as u32;
            let color = effect.screen(u, v, block);
            cells.push([f32::from(color.r), f32::from(color.g), f32::from(color.b)]);
        }
    }
    // For each screen row or column: the two cells it lies between, and how far along.
    let size = SIZE as usize;
    let spots: Vec<(usize, usize, f32)> = (0..size)
        .map(|pixel| {
            let at = ((pixel as f32 + 0.5) * FIELD as f32 / size as f32 - 0.5).clamp(0.0, FIELD as f32 - 1.0);
            let low = at.floor() as usize;
            (low, (low + 1).min(FIELD - 1), at - low as f32)
        })
        .collect();
    let mut image = RgbaImage::new(SIZE, SIZE);
    for (row, line) in image.chunks_exact_mut(size * 4).enumerate() {
        let (top, bottom, down) = spots[row];
        for (pixel, (left, right, across)) in line.chunks_exact_mut(4).zip(&spots) {
            for channel in 0..3 {
                let upper = cells[top * FIELD + left][channel] * (1.0 - across) + cells[top * FIELD + right][channel] * across;
                let lower = cells[bottom * FIELD + left][channel] * (1.0 - across) + cells[bottom * FIELD + right][channel] * across;
                pixel[channel] = (upper * (1.0 - down) + lower * down).round().min(255.0) as u8;
            }
            pixel[3] = 255;
        }
    }
    image
}

/// A full-screen dashboard on the lighting effect.
fn dashboard_on_effect(readings: &Readings, layout: &Layout, effect: &EffectFrame) -> RgbImage {
    let mut canvas = effect_backdrop(effect);
    // Every gauge, bar and tile gets its dark backdrop, so it stays readable on bright colors.
    // Plain text keeps the effect behind it: its shadow is enough, and a slab would hide most of the screen.
    let mut widgets = layout.widgets.clone();
    for widget in &mut widgets {
        widget.backdrop |= widget.style != Style::Text;
    }
    draw_widgets(&mut canvas, readings, &widgets);
    flatten(&canvas)
}

fn falloff(dx: f32, dy: f32, reach: f32, power: f32) -> f32 {
    (1.0 - (dx * dx + dy * dy).sqrt() / reach)
        .clamp(0.0, 1.0)
        .powf(power)
}

fn bar(
    canvas: &mut RgbaImage,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    fraction: f64,
    color: Rgba<u8>,
) {
    let radius = height / 2.0;
    fill_rounded(canvas, x, y, x + width, y + height, radius, TRACK);
    let filled = width * fraction as f32;
    if filled >= 1.0 {
        fill_rounded(
            canvas,
            x,
            y,
            x + filled.max(height),
            y + height,
            radius,
            color,
        );
    }
}


#[derive(Clone, Copy)]
enum Align {
    Left,
    Center,
    Right,
}

/// Draw one line of text with its baseline at `baseline`. Text wider than
/// `max_width` is shrunk to fit.
#[allow(clippy::too_many_arguments)]
fn text(
    canvas: &mut RgbaImage,
    font: &FontRef<'static>,
    size: f32,
    x: f32,
    baseline: f32,
    line: &str,
    color: Rgba<u8>,
    align: Align,
    max_width: f32,
) {
    if size < 1.0 || max_width <= 0.0 {
        return;
    }
    let mut size = size;
    let mut width = measure(font, size, line);
    if width > max_width && width > 0.0 {
        size *= max_width / width;
        width = max_width;
    }
    let scaled = font.as_scaled(PxScale::from(size));
    let mut pen = match align {
        Align::Left => x,
        Align::Center => x - width / 2.0,
        Align::Right => x - width,
    };
    let mut previous = None;
    for character in line.chars() {
        let id = scaled.glyph_id(character);
        if let Some(previous) = previous {
            pen += scaled.kern(previous, id);
        }
        let glyph = id.with_scale_and_position(size, ab_glyph::point(pen, baseline));
        pen += scaled.h_advance(id);
        previous = Some(id);
        let Some(outline) = font.outline_glyph(glyph) else {
            continue;
        };
        let bounds = outline.px_bounds();
        let drop = (size * 0.04).round().clamp(1.0, 4.0) as i32;
        outline.draw(|gx, gy, coverage| {
            let px = bounds.min.x as i32 + gx as i32;
            let py = bounds.min.y as i32 + gy as i32;
            blend(canvas, px, py + drop, TEXT_SHADOW, coverage);
        });
        outline.draw(|gx, gy, coverage| {
            let px = bounds.min.x as i32 + gx as i32;
            let py = bounds.min.y as i32 + gy as i32;
            blend(canvas, px, py, color, coverage);
        });
    }
}

fn measure(font: &FontRef<'static>, size: f32, line: &str) -> f32 {
    let scaled = font.as_scaled(PxScale::from(size));
    let mut width = 0.0;
    let mut previous = None;
    for character in line.chars() {
        let id = scaled.glyph_id(character);
        if let Some(previous) = previous {
            width += scaled.kern(previous, id);
        }
        width += scaled.h_advance(id);
        previous = Some(id);
    }
    width
}

/// Anti-aliased rounded rectangle.
fn fill_rounded(
    canvas: &mut RgbaImage,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    radius: f32,
    color: Rgba<u8>,
) {
    let radius = radius.min((x1 - x0) / 2.0).min((y1 - y0) / 2.0).max(0.0);
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (half_w, half_h) = ((x1 - x0) / 2.0 - radius, (y1 - y0) / 2.0 - radius);
    for py in (y0.floor().max(0.0) as i32)..(y1.ceil().min(SIZE as f32) as i32) {
        for px in (x0.floor().max(0.0) as i32)..(x1.ceil().min(SIZE as f32) as i32) {
            let qx = ((px as f32 + 0.5 - cx).abs() - half_w).max(0.0);
            let qy = ((py as f32 + 0.5 - cy).abs() - half_h).max(0.0);
            let distance = (qx * qx + qy * qy).sqrt() - radius;
            let coverage = (0.5 - distance).clamp(0.0, 1.0);
            if coverage > 0.0 {
                blend(canvas, px, py, color, coverage);
            }
        }
    }
}

/// Source-over blend in straight alpha, so a transparent canvas stays usable as an overlay.
fn blend(canvas: &mut RgbaImage, x: i32, y: i32, color: Rgba<u8>, coverage: f32) {
    if x < 0 || y < 0 || x >= SIZE as i32 || y >= SIZE as i32 {
        return;
    }
    let pixel = canvas.get_pixel_mut(x as u32, y as u32);
    let alpha = color[3] as f32 / 255.0 * coverage.clamp(0.0, 1.0);
    let below = pixel[3] as f32 / 255.0;
    let out = alpha + below * (1.0 - alpha);
    if out <= 0.0 {
        return;
    }
    for channel in 0..3 {
        let mixed =
            (color[channel] as f32 * alpha + pixel[channel] as f32 * below * (1.0 - alpha)) / out;
        pixel[channel] = mixed.round() as u8;
    }
    pixel[3] = (out * 255.0).round() as u8;
}

fn flatten(canvas: &RgbaImage) -> RgbImage {
    RgbImage::from_fn(SIZE, SIZE, |x, y| {
        let pixel = canvas.get_pixel(x, y);
        Rgb([pixel[0], pixel[1], pixel[2]])
    })
}

/// Lay `overlay` over a decoded picture.
pub fn compose(picture: &mut RgbImage, overlay: &RgbaImage) {
    for (pixel, top) in picture.pixels_mut().zip(overlay.pixels()) {
        let alpha = u32::from(top[3]);
        if alpha == 0 {
            continue;
        }
        for channel in 0..3 {
            let mixed =
                (u32::from(top[channel]) * alpha + u32::from(pixel[channel]) * (255 - alpha) + 127)
                    / 255;
            pixel[channel] = mixed as u8;
        }
    }
}

pub fn encode(picture: &RgbImage) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 88)
        .encode_image(picture)
        .ok()?;
    Some(out)
}

/// What the panel should show. Set by the window, read by the panel thread.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    /// Widgets over the user's photo or video, while that is on the pump.
    pub picture: Option<Vec<Widget>>,
    /// The dashboard, while it is on the pump.
    pub dashboard: Option<Layout>,
    /// The window is open and shows live readings.
    pub window_open: bool,
    pub fahrenheit: bool,
}

#[derive(Clone, Default)]
pub struct Rendered {
    /// Changes whenever a new panel is drawn.
    pub version: u64,
    pub overlay: Option<Arc<RgbaImage>>,
    pub dashboard: Option<Arc<Vec<u8>>>,
}

/// Shared between the window, the panel thread, and the screen worker.
#[derive(Default)]
pub struct Shared {
    config: Mutex<Config>,
    rendered: Mutex<Rendered>,
    readings: Mutex<Option<Readings>>,
    /// The dashboard plays a moving lighting effect, so it is redrawn every `EFFECT_FRAME`.
    animated: AtomicBool,
}

impl Shared {
    pub fn configure(&self, config: Config) {
        *self
            .config
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = config;
    }

    pub fn config(&self) -> Config {
        self.config
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    fn draws_on_picture(&self) -> bool {
        self.config
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .picture
            .is_some()
    }

    pub fn rendered(&self) -> Rendered {
        self.rendered
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    /// How long the screen should hold a dashboard frame, when that is shorter than usual.
    pub fn dashboard_frame(&self) -> Option<Duration> {
        self.animated.load(Ordering::Relaxed).then_some(EFFECT_FRAME)
    }

    pub fn readings(&self) -> Option<Readings> {
        self.readings
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }
}

/// Sample and draw once a second while anything needs it. A dashboard that
/// plays a moving lighting effect is drawn every `EFFECT_FRAME` instead, from
/// the same readings until the next sample.
pub fn spawn(shared: Arc<Shared>) {
    thread::spawn(move || {
        let mut sampler = Sampler::default();
        let mut version = 0u64;
        let mut glow: Option<(Backdrop, Vec<f32>)> = None;
        let mut background: Option<(Background, RgbaImage)> = None;
        let mut sampled: Option<(Instant, Readings)> = None;
        loop {
            let began = Instant::now();
            let config = shared.config();
            let on_picture = config
                .picture
                .as_ref()
                .is_some_and(|widgets| !widgets.is_empty());
            let drawing = on_picture || config.dashboard.is_some();
            let effect = config
                .dashboard
                .as_ref()
                .filter(|layout| layout.background.follow_effect)
                .and_then(|_| live_effect())
                .and_then(|view| view.frame());
            let animated = effect.as_ref().is_some_and(EffectFrame::animated);
            shared.animated.store(animated, Ordering::Relaxed);
            if drawing || config.window_open {
                // A little early counts as due, so sleeping a whole interval never skips a sample.
                let due = sampled.as_ref().is_none_or(|(at, _)| {
                    at.elapsed() + Duration::from_millis(50) >= stats::INTERVAL
                });
                if due {
                    sampled = Some((began, sampler.sample()));
                }
                let (_, readings) = sampled.as_mut().expect("sampled above");
                readings.fahrenheit = config.fahrenheit;
                let readings = readings.clone();
                if drawing && (due || animated) {
                    version += 1;
                    let overlay = if due {
                        config
                            .picture
                            .as_ref()
                            .filter(|widgets| !widgets.is_empty())
                            .map(|widgets| Arc::new(overlay(&readings, widgets)))
                    } else {
                        shared.rendered().overlay
                    };
                    let dashboard = config.dashboard.as_ref().and_then(|layout| {
                        if let Some(effect) = &effect {
                            return encode(&dashboard_on_effect(&readings, layout, effect)).map(Arc::new);
                        }
                        let style = layout.background.style;
                        if glow.as_ref().is_none_or(|(drawn, _)| *drawn != style) {
                            glow = Some((style, glow_mask(style)));
                        }
                        let (_, mask) = glow.as_ref()?;
                        if background
                            .as_ref()
                            .is_none_or(|(drawn, _)| *drawn != layout.background)
                        {
                            background =
                                Some((layout.background, backdrop_from(mask, layout.background.color)));
                        }
                        let (_, image) = background.as_ref()?;
                        encode(&dashboard(&readings, layout, image)).map(Arc::new)
                    });
                    *shared
                        .rendered
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner()) = Rendered {
                        version,
                        overlay,
                        dashboard,
                    };
                }
                *shared
                    .readings
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner()) = Some(readings);
            }
            if animated {
                thread::sleep(EFFECT_FRAME.saturating_sub(began.elapsed()).max(Duration::from_millis(10)));
            } else {
                thread::sleep(stats::INTERVAL);
            }
        }
    });
}

/// Turns the frames the worker is about to send into frames with the panel on
/// them. Keeps the last result, so a held still is only redrawn once a second.
#[derive(Default)]
pub struct Composer {
    last: Option<(FrameKey, Arc<Vec<u8>>)>,
}

/// The frame's address and length, and the panel version drawn on it.
type FrameKey = (usize, usize, u64);

impl Composer {
    /// `None` means send `base` unchanged.
    pub fn frame(&mut self, shared: &Shared, base: &[u8], dashboard: bool) -> Option<Arc<Vec<u8>>> {
        let rendered = shared.rendered();
        if dashboard {
            return rendered.dashboard;
        }
        let overlay = rendered.overlay?;
        if !shared.draws_on_picture() {
            return None;
        }
        let key = (base.as_ptr() as usize, base.len(), rendered.version);
        if let Some((last_key, jpeg)) = &self.last {
            if *last_key == key {
                return Some(Arc::clone(jpeg));
            }
        }
        let mut picture = image::load_from_memory(base).ok()?.to_rgb8();
        if picture.dimensions() != (SIZE, SIZE) {
            return None;
        }
        compose(&mut picture, &overlay);
        let jpeg = Arc::new(encode(&picture)?);
        self.last = Some((key, Arc::clone(&jpeg)));
        Some(jpeg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{self, Widget};

    fn sample() -> Readings {
        let mut readings = Readings::default();
        readings.set(Metric::CpuUsage, 37.0, Some(100.0));
        readings.set(Metric::GpuTemp, 58.0, None);
        readings.set(Metric::GpuUsage, 64.0, Some(100.0));
        readings.set(Metric::MemoryUsage, 18.0, Some(100.0));
        readings.set(Metric::CpuClock, 4300.0, None);
        readings.set(Metric::NetDown, 2_400_000.0, None);
        readings.set(Metric::PumpSpeed, 2400.0, None);
        readings.set(Metric::CpuPower, 95.0, None);
        readings.set(Metric::CpuTemp, 84.0, None);
        readings.set(Metric::GpuFan, 45.0, Some(100.0));
        // A minute of made-up history, so graphs have something to draw.
        for (index, metric) in [Metric::CpuUsage, Metric::GpuTemp, Metric::GpuUsage, Metric::MemoryUsage, Metric::CpuClock, Metric::NetDown]
            .into_iter()
            .enumerate()
        {
            let now = readings.get(metric).expect("set above").now;
            let past = (0..60)
                .map(|second| {
                    let wave = ((second as f64 * 0.21 + index as f64).sin() + (second as f64 * 0.67).cos() * 0.5) * 0.25;
                    (now * (1.0 + wave * (60 - second) as f64 / 60.0)).max(0.0)
                })
                .collect();
            readings.set_history(metric, past);
        }
        readings.set(
            Metric::MemoryUsed,
            21.0 * 1_073_741_824.0,
            Some(64.0 * 1_073_741_824.0),
        );
        readings
    }

    #[test]
    fn a_dashboard_following_the_effect_plays_it_as_the_background() {
        use coolwave_core::effects::Effect;
        use coolwave_core::engine::{Config, Engine};
        let mut config = Config { eurux_enabled: false, aura_enabled: false, gpu_enabled: false, ..Config::default() };
        config.params.effect = Effect::Static;
        let primary = config.params.primary;
        let engine = Engine::start(Vec::new(), config);
        let frame = engine.effect_view().frame().expect("lights are on");
        let image = effect_backdrop(&frame);
        for (x, y) in [(0, 0), (360, 360), (719, 719), (13, 700)] {
            assert_eq!(image.get_pixel(x, y).0, [primary.r, primary.g, primary.b, 255]);
        }

        // Every effect fills the whole screen without a seam, and one frame fits well inside `EFFECT_FRAME`.
        let mut layout = layout::arrange_dashboard(&stats::ALL[..8], Background::default());
        if let Some(saved) = std::env::var_os("COOLWAVE_EFFECT_LAYOUT") {
            layout = serde_json::from_slice(&std::fs::read(saved).expect("reads")).expect("a layout");
        }
        let folder = std::env::var_os("COOLWAVE_EFFECT_SHOTS").map(std::path::PathBuf::from);
        for effect in Effect::ALL {
            if effect == Effect::Off {
                continue;
            }
            engine.update(|config| config.params.effect = effect);
            let frame = engine.effect_view().frame().expect("lights are on");
            let began = Instant::now();
            let picture = dashboard_on_effect(&sample(), &layout, &frame);
            let jpeg = encode(&picture).expect("encodes");
            println!("{}: {:?}, {} bytes", effect.name(), began.elapsed(), jpeg.len());
            if let Some(folder) = &folder {
                picture.save(folder.join(format!("{}.png", effect.name()))).expect("saves");
            }
        }
    }

    #[test]
    fn widgets_only_paint_inside_their_box() {
        for style in Style::ALL {
            for backdrop in [false, true] {
                let mut widget = Widget::new(Metric::CpuUsage, style, 360.0, 360.0);
                widget.backdrop = backdrop;
                let image = overlay(&sample(), &[widget.clone()]);
                let mut painted = 0;
                for (x, y, pixel) in image.enumerate_pixels() {
                    if pixel[3] == 0 {
                        continue;
                    }
                    painted += 1;
                    let (x, y) = (x as f32, y as f32);
                    assert!(
                        x >= widget.x - 2.0
                            && x <= widget.x + widget.w + 2.0
                            && y >= widget.y - 2.0
                            && y <= widget.y + widget.h + 2.0,
                        "{style:?} painted outside its box at {x},{y}"
                    );
                }
                assert!(painted > 0, "{style:?} drew nothing");
            }
        }
    }

    #[test]
    fn a_gauge_fills_with_the_reading() {
        let mut widget = Widget::new(Metric::CpuUsage, Style::Ring, 360.0, 360.0);
        widget.caption = false;
        let image = overlay(&sample(), &[widget.clone()]);
        let color = Rgba([widget.color[0], widget.color[1], widget.color[2], 255]);
        let outer = widget.w / 2.0 - widget.w * 0.04;
        let radius = outer - outer * 0.11;
        // 37% lights 9 of the 24 segments. Sample the middle of one that is lit and one that is not.
        let at = |segment: f32| {
            let angle = (DIAL_START + DIAL_SWEEP * (segment + 0.5) / 24.0).to_radians();
            let (cx, cy) = widget.center();
            *image.get_pixel(
                (cx + radius * angle.cos()) as u32,
                (cy + radius * angle.sin()) as u32,
            )
        };
        assert_eq!(at(2.0), color, "filled part");
        assert_ne!(at(15.0), color, "empty part");
    }

    #[test]
    fn overlay_leaves_the_rest_of_the_picture_clear() {
        let layout = layout::arrange_band(&[Metric::CpuUsage, Metric::GpuTemp], false);
        let overlay = overlay(&sample(), &layout.widgets);
        assert_eq!(overlay.get_pixel(360, 100)[3], 0, "top is untouched");
        assert!(overlay.get_pixel(250, 690)[3] > 0, "band is at the bottom");
    }

    #[test]
    fn dashboard_draws_every_arrangement() {
        for count in 0..=stats::ALL.len() {
            let layout = layout::arrange_dashboard(&stats::ALL[..count], Background::default());
            let image = dashboard(&sample(), &layout, &backdrop(layout.background));
            assert_eq!(image.dimensions(), (SIZE, SIZE));
        }
    }

    #[test]
    fn plain_backdrop_has_no_glow() {
        let plain = backdrop(Background {
            style: Backdrop::Plain,
            ..Background::default()
        });
        assert_eq!(plain.get_pixel(560, 600), plain.get_pixel(0, 0));
        let glow = backdrop(Background::default());
        assert!(glow.get_pixel(560, 600)[0] > glow.get_pixel(0, 0)[0]);
    }

    #[test]
    fn compose_keeps_pixels_under_clear_overlay() {
        let mut picture = RgbImage::from_pixel(SIZE, SIZE, Rgb([10, 200, 30]));
        let mut overlay = RgbaImage::new(SIZE, SIZE);
        overlay.put_pixel(5, 5, Rgba([255, 255, 255, 255]));
        compose(&mut picture, &overlay);
        assert_eq!(picture.get_pixel(0, 0), &Rgb([10, 200, 30]));
        assert_eq!(picture.get_pixel(5, 5), &Rgb([255, 255, 255]));
    }
}
