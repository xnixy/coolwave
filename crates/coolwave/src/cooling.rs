use adw::prelude::*;
use coolwave_core::engine::{Engine, FanControl, FanPort};
use coolwave_core::hwmon::{self, PwmChannel};
use gtk::{gio, glib};
use std::cell::RefCell;
use std::rc::Rc;

const PUMP_HEADER: u32 = 7;
const PUMP_MIN_PCT: f64 = 40.0;

fn mode_name(m: u8) -> &'static str {
    match m {
        0 => "full speed",
        1 => "fixed",
        5 => "curve",
        2 => "thermal cruise",
        3 => "fan speed cruise",
        4 => "SmartFan III",
        _ => "other",
    }
}

fn pct(pwm: u8) -> f64 {
    (pwm as f64 * 100.0 / 255.0).round()
}

fn to_pwm(pct: f64) -> u32 {
    (pct.clamp(0.0, 100.0) * 255.0 / 100.0).round() as u32
}

/// Presets as (°C, %) for the 5 hardware curve points.
const PRESETS: &[(&str, [(f64, f64); 5])] = &[
    ("Silent", [(30.0, 20.0), (45.0, 25.0), (60.0, 40.0), (75.0, 65.0), (90.0, 100.0)]),
    ("Balanced", [(30.0, 30.0), (45.0, 40.0), (60.0, 55.0), (75.0, 80.0), (85.0, 100.0)]),
    ("Performance", [(30.0, 45.0), (45.0, 60.0), (60.0, 75.0), (70.0, 90.0), (80.0, 100.0)]),
];

struct Editor {
    channel: u32,
    temps: Vec<gtk::SpinButton>,
    pwms: Vec<gtk::SpinButton>,
    graph: gtk::DrawingArea,
    live: Rc<RefCell<(f32, f64)>>, // (source °C, current %)
}

impl Editor {
    fn points(&self) -> Vec<(f64, f64)> {
        self.temps.iter().zip(&self.pwms).map(|(t, p)| (t.value(), p.value())).collect()
    }

    fn set_points(&self, pts: &[(f64, f64)]) {
        let floor = if self.channel == PUMP_HEADER { PUMP_MIN_PCT } else { 0.0 };
        for ((t, p), (tv, pv)) in self.temps.iter().zip(&self.pwms).zip(pts) {
            t.set_value(*tv);
            p.set_value(pv.max(floor));
        }
        if let Some(last) = self.pwms.last() {
            last.set_value(100.0);
        }
        self.graph.queue_draw();
    }
}

fn draw_curve(cr: &gtk::cairo::Context, w: f64, h: f64, pts: &[(f64, f64)], live: (f32, f64)) {
    let (l, r, t, b) = (36.0, 12.0, 10.0, 24.0);
    let gw = (w - l - r).max(1.0);
    let gh = (h - t - b).max(1.0);
    let x = |c: f64| l + c.clamp(0.0, 100.0) / 100.0 * gw;
    let y = |p: f64| t + gh - p.clamp(0.0, 100.0) / 100.0 * gh;

    cr.set_source_rgba(0.5, 0.5, 0.5, 0.25);
    cr.set_line_width(1.0);
    for i in 0..=4 {
        let v = i as f64 * 25.0;
        cr.move_to(x(0.0), y(v));
        cr.line_to(x(100.0), y(v));
        cr.move_to(x(v), y(0.0));
        cr.line_to(x(v), y(100.0));
    }
    let _ = cr.stroke();

    cr.set_source_rgba(0.6, 0.6, 0.6, 0.9);
    cr.set_font_size(10.0);
    for i in 0..=4 {
        let v = i as f64 * 25.0;
        cr.move_to(2.0, y(v) + 3.0);
        let _ = cr.show_text(&format!("{v:.0}%"));
        cr.move_to(x(v) - 10.0, h - 6.0);
        let _ = cr.show_text(&format!("{v:.0}°C"));
    }

    if let Some(first) = pts.first() {
        cr.set_source_rgb(0.21, 0.52, 0.89);
        cr.set_line_width(2.5);
        cr.move_to(x(0.0), y(first.1));
        for &(c, p) in pts {
            cr.line_to(x(c), y(p));
        }
        cr.line_to(x(100.0), y(100.0));
        let _ = cr.stroke();
        for &(c, p) in pts {
            cr.arc(x(c), y(p), 4.5, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
        }
    }

    // live operating point
    cr.set_source_rgb(0.95, 0.45, 0.1);
    cr.arc(x(live.0 as f64), y(live.1), 5.0, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
}

fn run_fanctl(args: Vec<String>, toasts: adw::ToastOverlay, done_msg: String) {
    if !hwmon::fanctl_installed() {
        toasts.add_toast(adw::Toast::new("Fan helper not installed — run the Coolwave installer again (install.sh)"));
        return;
    }
    let mut argv: Vec<&std::ffi::OsStr> = vec![std::ffi::OsStr::new("pkexec"), std::ffi::OsStr::new(hwmon::FANCTL)];
    argv.extend(args.iter().map(std::ffi::OsStr::new));
    match gio::Subprocess::newv(&argv, gio::SubprocessFlags::STDERR_PIPE) {
        Ok(proc_) => {
            let p2 = proc_.clone();
            proc_.communicate_utf8_async(None, gio::Cancellable::NONE, move |res| {
                let proc_ = p2;
                let msg = match res {
                    Ok((_, err)) if proc_.is_successful() => {
                        let _ = err;
                        done_msg
                    }
                    Ok((_, err)) => format!("Failed: {}", err.map(|e| e.trim().to_string()).unwrap_or_default()),
                    Err(e) => format!("Failed: {e}"),
                };
                toasts.add_toast(adw::Toast::new(&msg));
            });
        }
        Err(e) => toasts.add_toast(adw::Toast::new(&format!("Failed to start helper: {e}"))),
    }
}

fn header_row(chip: &std::path::Path, ch: &PwmChannel, toasts: &adw::ToastOverlay) -> (adw::ExpanderRow, Rc<Editor>) {
    let title = if ch.index == PUMP_HEADER { format!("Header {} · AIO pump", ch.index) } else { format!("Header {}", ch.index) };
    let row = adw::ExpanderRow::builder().title(title).build();

    // graph
    let graph = gtk::DrawingArea::builder().content_height(180).margin_start(12).margin_end(12).margin_top(8).build();
    let floor = if ch.index == PUMP_HEADER { PUMP_MIN_PCT } else { 0.0 };
    let mut temps = Vec::new();
    let mut pwms = Vec::new();
    let points_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    points_box.set_margin_start(12);
    points_box.set_margin_end(12);
    points_box.set_homogeneous(true);
    for (i, p) in ch.curve.iter().enumerate() {
        let col = gtk::Box::new(gtk::Orientation::Vertical, 2);
        col.append(&gtk::Label::builder().label(format!("Point {}", i + 1)).css_classes(["caption", "dim-label"]).build());
        let t = gtk::SpinButton::with_range(0.0, 100.0, 1.0);
        t.set_value(p.temp_c as f64);
        let w = gtk::SpinButton::with_range(floor, 100.0, 1.0);
        w.set_value(pct(p.pwm).max(floor));
        if i + 1 == ch.curve.len() {
            w.set_value(100.0);
            w.set_sensitive(false);
        }
        col.append(&t);
        col.append(&gtk::Label::builder().label("°C").css_classes(["caption"]).build());
        col.append(&w);
        col.append(&gtk::Label::builder().label("%").css_classes(["caption"]).build());
        points_box.append(&col);
        temps.push(t);
        pwms.push(w);
    }
    let editor = Rc::new(Editor { channel: ch.index, temps, pwms, graph: graph.clone(), live: Rc::new(RefCell::new((0.0, pct(ch.pwm)))) });
    {
        let ed = editor.clone();
        graph.set_draw_func(move |_, cr, w, h| draw_curve(cr, w as f64, h as f64, &ed.points(), *ed.live.borrow()));
    }
    for s in editor.temps.iter().chain(&editor.pwms) {
        s.connect_value_changed(glib::clone!(#[weak] graph, move |_| graph.queue_draw()));
    }

    let graph_row = gtk::ListBoxRow::builder().activatable(false).child(&graph).build();
    row.add_row(&graph_row);
    let pts_row = gtk::ListBoxRow::builder().activatable(false).child(&points_box).build();
    row.add_row(&pts_row);

    // presets
    let presets = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    for (name, pts) in PRESETS {
        let b = gtk::Button::builder().label(*name).css_classes(["pill"]).build();
        let ed = editor.clone();
        let pts = pts.to_vec();
        b.connect_clicked(move |_| ed.set_points(&pts));
        presets.append(&b);
    }
    let presets_row = adw::ActionRow::builder().title("Presets").build();
    presets_row.add_suffix(&presets);
    row.add_row(&presets_row);

    // temperature source
    let sources = hwmon::chip_temps(chip);
    let labels: Vec<String> = sources.iter().map(|(i, l, _)| format!("{l} (temp{i})")).collect();
    let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let src_row = adw::ComboRow::builder().title("Follows temperature").model(&gtk::StringList::new(&label_refs)).build();
    if let Some(pos) = sources.iter().position(|(i, _, _)| Some(*i) == ch.temp_source) {
        src_row.set_selected(pos as u32);
    }
    row.add_row(&src_row);

    // actions
    let apply = gtk::Button::builder().label("Apply curve").css_classes(["suggested-action"]).valign(gtk::Align::Center).build();
    let full = gtk::Button::builder().label("Full speed").valign(gtk::Align::Center).build();
    let actions = adw::ActionRow::builder().title("Write to fan controller").subtitle("Stored in the chip — keeps working without Coolwave until reboot").build();
    actions.add_suffix(&full);
    actions.add_suffix(&apply);
    row.add_row(&actions);

    let idx = ch.index;
    apply.connect_clicked(glib::clone!(#[strong] editor, #[weak] toasts, #[strong] src_row, move |_| {
        let mut args = vec!["curve".to_string(), idx.to_string()];
        args.extend(editor.points().iter().map(|(t, p)| format!("{}:{}", t.round() as u32, to_pwm(*p))));
        run_fanctl(args, toasts.clone(), format!("Header {idx}: curve applied"));
        if let Some((src, _, _)) = sources.get(src_row.selected() as usize) {
            run_fanctl(vec!["source".into(), idx.to_string(), src.to_string()], toasts, format!("Header {idx}: temperature source set"));
        }
    }));
    full.connect_clicked(glib::clone!(#[weak] toasts, move |_| {
        run_fanctl(vec!["full".into(), idx.to_string()], toasts, format!("Header {idx}: full speed"));
    }));

    (row, editor)
}

/// EURUX fan ports: software-controlled from Coolwave (no sensor on the controller).
fn eurux_group(engine: &Rc<Engine>, layout: &[FanPort]) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("ROG EURUX fans")
        .description("Controlled by Coolwave (needs EURUX lighting enabled; keeps running in background mode)")
        .build();
    let sensors = hwmon::temperatures();
    let sensor_labels: Vec<String> = sensors.iter().map(|t| format!("{} · {}", t.chip, t.label)).collect();
    let sensor_refs: Vec<&str> = sensor_labels.iter().map(String::as_str).collect();
    let cfg = engine.config();
    let mut rpm_rows = Vec::new();

    for port in layout {
        let pidx = port.port;
        let exp = adw::ExpanderRow::builder().title(format!("{} ({} fan{})", port.name, port.fans, if port.fans == 1 { "" } else { "s" })).build();
        let mode = adw::ComboRow::builder().title("Mode").model(&gtk::StringList::new(&["Controller default", "Fixed speed", "Curve"])).build();
        let (fixed_row, fixed) = {
            let s = gtk::Scale::with_range(gtk::Orientation::Horizontal, 20.0, 100.0, 1.0);
            s.set_hexpand(true);
            s.set_width_request(240);
            s.set_valign(gtk::Align::Center);
            let r = adw::ActionRow::builder().title("Speed %").build();
            r.add_suffix(&s);
            (r, s)
        };
        let source = adw::ComboRow::builder().title("Follows temperature").model(&gtk::StringList::new(&sensor_refs)).build();
        let pts_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        pts_box.set_homogeneous(true);
        pts_box.set_margin_start(12);
        pts_box.set_margin_end(12);
        pts_box.set_margin_top(6);
        pts_box.set_margin_bottom(6);
        let default_pts = [(30.0, 30u8), (45.0, 40), (60.0, 55), (75.0, 80), (85.0, 100)];
        let mut temps = Vec::new();
        let mut pcts = Vec::new();
        for (i, (t, p)) in default_pts.iter().enumerate() {
            let col = gtk::Box::new(gtk::Orientation::Vertical, 2);
            col.append(&gtk::Label::builder().label(format!("Point {}", i + 1)).css_classes(["caption", "dim-label"]).build());
            let ts = gtk::SpinButton::with_range(0.0, 100.0, 1.0);
            ts.set_value(*t);
            let ps = gtk::SpinButton::with_range(20.0, 100.0, 1.0);
            ps.set_value(*p as f64);
            col.append(&ts);
            col.append(&gtk::Label::builder().label("°C").css_classes(["caption"]).build());
            col.append(&ps);
            col.append(&gtk::Label::builder().label("%").css_classes(["caption"]).build());
            pts_box.append(&col);
            temps.push(ts);
            pcts.push(ps);
        }
        let pts_row = gtk::ListBoxRow::builder().activatable(false).child(&pts_box).build();

        // restore saved state
        match cfg.eurux_fans.get(pidx) {
            Some(FanControl::Fixed { percent }) => {
                mode.set_selected(1);
                fixed.set_value(*percent as f64);
            }
            Some(FanControl::Curve { chip, label, points }) => {
                mode.set_selected(2);
                if let Some(i) = sensors.iter().position(|t| &t.chip == chip && &t.label == label) {
                    source.set_selected(i as u32);
                }
                for ((ts, ps), (t, p)) in temps.iter().zip(&pcts).zip(points) {
                    ts.set_value(*t as f64);
                    ps.set_value(*p as f64);
                }
            }
            _ => fixed.set_value(60.0),
        }
        let has_curve = cfg.eurux_fans.get(pidx).is_some_and(|f| matches!(f, FanControl::Curve { .. }));
        if let Some(i) = sensors.iter().position(|t| t.chip == "k10temp").filter(|_| !has_curve) {
            source.set_selected(i as u32);
        }

        let sensors_c = sensors.clone();
        let push = Rc::new(glib::clone!(#[strong] engine, #[strong] mode, #[strong] fixed, #[strong] source, #[strong] fixed_row, #[strong] pts_row, move || {
            let sel = mode.selected();
            fixed_row.set_visible(sel == 1);
            source.set_visible(sel == 2);
            pts_row.set_visible(sel == 2);
            let fc = match sel {
                1 => FanControl::Fixed { percent: fixed.value() as u8 },
                2 => {
                    let mut pts: Vec<(f32, u8)> = temps.iter().zip(&pcts).map(|(t, p)| (t.value() as f32, p.value() as u8)).collect();
                    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
                    let t = sensors_c.get(source.selected() as usize);
                    FanControl::Curve {
                        chip: t.map(|t| t.chip.clone()).unwrap_or_default(),
                        label: t.map(|t| t.label.clone()).unwrap_or_default(),
                        points: pts,
                    }
                }
                _ => FanControl::Default,
            };
            engine.update(|c| {
                if c.eurux_fans.len() <= pidx {
                    c.eurux_fans.resize(pidx + 1, FanControl::Default);
                }
                c.eurux_fans[pidx] = fc;
            });
        }));
        mode.connect_selected_notify(glib::clone!(#[strong] push, move |_| push()));
        fixed.connect_value_changed(glib::clone!(#[strong] push, move |_| push()));
        source.connect_selected_notify(glib::clone!(#[strong] push, move |_| push()));
        let mut c = pts_box.first_child();
        while let Some(col) = c {
            let mut w = col.first_child();
            while let Some(child) = w {
                if let Ok(spin) = child.clone().downcast::<gtk::SpinButton>() {
                    spin.connect_value_changed(glib::clone!(#[strong] push, move |_| push()));
                }
                w = child.next_sibling();
            }
            c = col.next_sibling();
        }
        // visibility only; don't overwrite saved config on startup
        fixed_row.set_visible(mode.selected() == 1);
        source.set_visible(mode.selected() == 2);
        pts_row.set_visible(mode.selected() == 2);

        exp.add_row(&mode);
        exp.add_row(&fixed_row);
        exp.add_row(&source);
        exp.add_row(&pts_row);
        group.add(&exp);
        rpm_rows.push((pidx, exp));
    }

    glib::timeout_add_seconds_local(1, glib::clone!(#[strong] engine, move || {
        let st = engine.status();
        for (pidx, row) in &rpm_rows {
            let rpm = st.eurux_rpm.get(*pidx).copied().unwrap_or(0);
            let duty = st.eurux_duty.get(*pidx).copied().flatten().map(|d| format!(" · {d}%")).unwrap_or_default();
            row.set_subtitle(&if st.eurux.state.starts_with("Connected") { format!("{rpm} RPM{duty}") } else { st.eurux.state.clone() });
        }
        glib::ControlFlow::Continue
    }));
    group
}

pub fn build(toasts: &adw::ToastOverlay, engine: &Rc<Engine>, layout: &[FanPort]) -> gtk::Widget {
    let page = adw::PreferencesPage::new();
    page.add(&eurux_group(engine, layout));
    let fans_group = adw::PreferencesGroup::builder().title("Motherboard fan headers").build();
    let temps_group = adw::PreferencesGroup::builder().title("Temperatures").build();
    page.add(&fans_group);
    page.add(&temps_group);

    let Some(chip) = hwmon::find_fan_chip() else {
        fans_group.set_description(Some("Fan controller driver not loaded (modprobe nct6775)"));
        return page.upcast();
    };
    if !hwmon::fanctl_installed() {
        fans_group.set_description(Some("Read-only until the fan helper is installed (run install.sh)"));
    }

    let rows: Vec<(u32, adw::ExpanderRow, Rc<Editor>)> = hwmon::pwm_channels(&chip)
        .into_iter()
        .filter(|ch| !ch.curve.is_empty())
        .map(|ch| {
            let (row, ed) = header_row(&chip, &ch, toasts);
            fans_group.add(&row);
            (ch.index, row, ed)
        })
        .collect();

    let temp_rows: Vec<adw::ActionRow> = hwmon::temperatures()
        .into_iter()
        .map(|t| {
            let row = adw::ActionRow::builder().title(format!("{} · {}", t.chip, t.label)).build();
            temps_group.add(&row);
            row
        })
        .collect();

    let refresh = move || {
        let temps = hwmon::chip_temps(&chip);
        for ch in hwmon::pwm_channels(&chip) {
            if let Some((_, row, ed)) = rows.iter().find(|(i, _, _)| *i == ch.index) {
                let rpm = ch.rpm.filter(|r| *r > 0).map(|r| format!("{r} RPM")).unwrap_or_else(|| "not connected".into());
                row.set_subtitle(&format!("{rpm} · {:.0}% · {}", pct(ch.pwm), mode_name(ch.mode)));
                let src_temp = temps.iter().find(|(i, _, _)| Some(*i) == ch.temp_source).map(|t| t.2).unwrap_or(0.0);
                *ed.live.borrow_mut() = (src_temp, pct(ch.pwm));
                ed.graph.queue_draw();
            }
        }
        for (row, t) in temp_rows.iter().zip(hwmon::temperatures()) {
            row.set_subtitle(&format!("{:.1} °C", t.celsius));
        }
    };
    refresh();
    glib::timeout_add_seconds_local(2, move || {
        refresh();
        glib::ControlFlow::Continue
    });

    page.upcast()
}
