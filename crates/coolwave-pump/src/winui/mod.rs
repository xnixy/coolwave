//! The pump screen page of the WinUI window. What the buttons do lives in
//! `engine::Controller`; this draws its state and passes the clicks on.
//!
//! The controller outlives the window: Coolwave keeps it in [`PumpCore`], ticks it from its own
//! timer (also while the window is in the tray) and hands the window a [`PumpHandle`]. The page
//! itself, [`PumpPage`], is a component made again each time the window opens.

mod editor;
mod parts;
mod screen;
mod show;

use crate::engine::{demo, Controller};
use crate::layout::{self, Layout};
use crate::session::Setting;
use crate::settings::{Settings, Source};
use crate::stats::{self, Metric};
use crate::tray::Wake;
use coolwave_ui::*;
use editor::Editor;
use parts::Frames;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use windows_reactor::*;

/// How long a slider rests before what it changed is saved or sent to the pump.
const SETTLE: Duration = Duration::from_millis(600);
/// Environment variable that opens the page on a given tab, to work on it.
const TAB_ENV: &str = "COOLWAVE_UI_PUMP_TAB";
/// Pictures the file dialog offers.
const PICTURE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp", "mp4", "mov", "avi", "mkv", "webm"];

/// The chosen file's preview, as the pump worker made it.
#[derive(Clone)]
pub(crate) struct Preview {
    /// Changes with each new preview.
    pub id: u64,
    /// The JPEG itself, for an `Image`.
    pub jpeg: EncodedImage,
    /// Decoded, for cutting out the crop square.
    pub pixels: Rc<image::RgbImage>,
}

/// What lives as long as Coolwave does: the controller and the last preview.
pub struct PumpCore {
    pub(crate) core: Controller,
    pub(crate) preview: Option<Preview>,
    previews: u64,
}

impl PumpCore {
    /// Starts the pump screen worker and reads the pump.
    pub fn new(wake: Wake) -> Self {
        Self { core: Controller::new(wake), preview: None, previews: 0 }
    }

    /// The periodic work: finished jobs, a new preview, what the stats panel draws. Runs while
    /// the window is in the tray too. True when something the page shows changed.
    pub fn tick(&mut self, window_open: bool) -> bool {
        self.core.window_open = window_open;
        // Coolwave owns the tray; drain anything queued for the old one.
        while self.core.tray_rx.try_recv().is_ok() {}
        let finished = self.core.tick();
        finished | self.poll_preview()
    }

    /// A job or a preview is still running: tick again soon.
    pub fn waiting(&self) -> bool {
        self.core.waiting()
    }

    fn poll_preview(&mut self) -> bool {
        let Some(preview) = self.core.take_preview() else {
            return false;
        };
        match image::load_from_memory(&preview.jpeg) {
            Ok(decoded) => {
                self.previews += 1;
                self.preview = Some(Preview {
                    id: self.previews,
                    jpeg: EncodedImage::new(preview.jpeg),
                    pixels: Rc::new(decoded.to_rgb8()),
                });
            }
            Err(error) => self.core.status = format!("could not show a preview: {error}"),
        }
        true
    }

    /// The source picture's size: what the worker read, or the preview's own.
    pub(crate) fn source_size(&self) -> (u32, u32) {
        match (self.core.source_width, self.core.source_height, &self.preview) {
            (w, h, _) if w > 0 && h > 0 => (w, h),
            (_, _, Some(preview)) => (preview.pixels.width(), preview.pixels.height()),
            _ => (0, 0),
        }
    }
}

/// The core, compared by identity so it can be part of a component's input.
#[derive(Clone)]
pub struct PumpHandle(pub Rc<RefCell<PumpCore>>);

impl PumpHandle {
    pub fn new(core: PumpCore) -> Self {
        Self(Rc::new(RefCell::new(core)))
    }
}

impl PartialEq for PumpHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// The page's input. `revision` changes on every tick of Coolwave's timer, so the page draws
/// the controller's state again.
#[derive(Clone, PartialEq)]
pub struct PumpInput {
    pub pump: PumpHandle,
    pub revision: u64,
}

/// The sub-pages of the pump screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tab {
    Show,
    Widgets,
    Screen,
    Settings,
}

impl Tab {
    const ALL: [(Tab, &'static str); 4] =
        [(Tab::Show, "Show"), (Tab::Widgets, "Widgets"), (Tab::Screen, "Screen"), (Tab::Settings, "Settings")];

    /// The tab named in `COOLWAVE_UI_PUMP_TAB`.
    fn from_env() -> Option<Tab> {
        let name = std::env::var(TAB_ENV).ok()?.to_ascii_lowercase();
        Tab::ALL.iter().find(|(_, label)| label.to_ascii_lowercase() == name).map(|(tab, _)| *tab)
    }
}

/// Which layout is being edited.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Canvas {
    #[default]
    Dashboard,
    Picture,
}

/// What a file is picked for. Decides the dialog's filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Picture,
    Firmware,
}

/// When an update draws the screen again.
enum Redraw {
    /// If what it shows changed.
    IfChanged,
    /// Now, whatever changed.
    Now,
    /// A layout was edited: now, unless a frame was just drawn and this is not the `last` step.
    Edited { last: bool },
}

/// The page.
pub struct PumpPage {
    pub(crate) tab: Tab,
    /// The source whose controls the Show tab has open. What the pump shows is `settings.source`.
    pub(crate) source_tab: Source,
    pub(crate) editor: Editor,
    /// The live screens: the dashboard's and the one over the picture.
    pub(crate) frames: [Frames; 2],
    backdrop: Option<parts::Cached<layout::Background, image::RgbaImage>>,
    picture: Option<parts::Cached<(u64, (u32, u32, u32)), image::RgbImage>>,
    /// Where the pointer was in the crop view, while the square is dragged.
    pub(crate) crop_drag: Option<(f64, f64)>,
    /// A file dialog is open.
    pub(crate) picking: bool,
    _persist: Option<ComponentTimer>,
    _brightness: Option<ComponentTimer>,
    _settle: Option<ComponentTimer>,
    _render: Option<ComponentTimer>,
}

#[derive(Clone)]
pub enum Message {
    Tab(Option<String>),
    SourceTab(Source),
    Choose(Kind),
    Picked(Kind, Option<PathBuf>),
    CropPressed(f64, f64),
    CropMoved(f64, f64),
    CropReleased,
    Zoom(f64),
    Speed(f64),
    ResetCrop,
    StatsOnPicture(bool),
    EditWidgets(Canvas),
    ShowPicture,
    StopVideo,
    ShowDashboard,
    ShowAnimation,
    Persist,
    Refresh,
    Brightness(f64),
    ApplyBrightness,
    Rotation(Option<usize>),
    ScreenOn(bool),
    Standby(bool),
    Power(&'static str),
    FirmwareClosed(ContentDialogResult),
    ResumeLast(bool),
    SleepWhenLocked(bool),
    Fahrenheit(Option<usize>),
    Editor(editor::Message),
    /// The throttled redraw of a screen being edited.
    Render,
    /// A frame of a screen has been decoded and shows.
    FrameOpened(Canvas, u64),
}

impl Component for PumpPage {
    type Input = PumpInput;
    type Message = Message;

    fn create(input: &PumpInput, _context: &ComponentContext<Self>) -> Self {
        let source_tab = input.pump.0.borrow().core.settings.source;
        let mut page = Self {
            tab: Tab::from_env().unwrap_or(Tab::Show),
            source_tab,
            editor: Editor::new(),
            frames: Default::default(),
            backdrop: None,
            picture: None,
            crop_drag: None,
            picking: false,
            _persist: None,
            _brightness: None,
            _settle: None,
            _render: None,
        };
        page.draw_screen(&input.pump.0.borrow(), true);
        page
    }

    fn input_changed(&mut self, input: &PumpInput, _context: &ComponentContext<Self>) {
        // Readings change once a second; draw again when they are due.
        self.draw_screen(&input.pump.0.borrow(), false);
    }

    fn update(&mut self, message: Message, context: &ComponentContext<Self>) {
        let handle = context.input().pump.clone();
        let mut guard = handle.0.borrow_mut();
        let pump: &mut PumpCore = &mut guard;
        // Anything may change what a screen shows (a crop, a canvas, a tab): draw it if it did.
        let mut redraw = Redraw::IfChanged;
        let core = &mut pump.core;
        match message {
            Message::Tab(text) => {
                if let Some(tab) = Tab::ALL.iter().find(|(_, label)| Some(*label) == text.as_deref()) {
                    self.tab = tab.0;
                }
            }
            Message::SourceTab(source) => self.source_tab = source,
            Message::Choose(kind) => {
                if !self.picking {
                    self.picking = true;
                    // The dialog runs its own loop: on another thread, the window keeps drawing.
                    context.spawn_background(move |_| Message::Picked(kind, pick_file(kind)));
                }
            }
            Message::Picked(kind, path) => {
                self.picking = false;
                if let Some(path) = path {
                    match kind {
                        Kind::Picture => {
                            pump.preview = None;
                            pump.core.choose(path);
                        }
                        Kind::Firmware => pump.core.offer_firmware(&path),
                    }
                }
            }
            Message::CropPressed(x, y) => self.crop_drag = Some((x, y)),
            Message::CropMoved(x, y) => {
                if let Some((last_x, last_y)) = self.crop_drag {
                    self.crop_drag = Some((x, y));
                    let (width, height) = pump.source_size();
                    show::pan(&mut pump.core, (width, height), x - last_x, y - last_y);
                }
            }
            Message::CropReleased => {
                if self.crop_drag.take().is_some() {
                    core.persist();
                }
            }
            Message::Zoom(value) => {
                let zoom = Range::new(1.0, 4.0, 0.01).snap(value) as f32;
                if zoom != core.zoom {
                    core.zoom = zoom;
                    self._persist = Some(context.set_local_timeout(SETTLE, || Message::Persist));
                }
            }
            Message::Speed(value) => {
                let speed = Range::new(0.25, 2.0, 0.05).snap(value) as f32;
                if speed != core.speed {
                    core.set_speed(speed);
                    self._persist = Some(context.set_local_timeout(SETTLE, || Message::Persist));
                }
            }
            Message::ResetCrop => {
                core.reset_crop();
                core.persist();
            }
            Message::StatsOnPicture(on) => {
                if on != core.settings.stats_on_picture {
                    core.settings.stats_on_picture = on;
                    core.persist();
                }
            }
            Message::EditWidgets(canvas) => {
                self.tab = Tab::Widgets;
                if self.editor.canvas != canvas {
                    self.editor.switch_canvas(canvas);
                }
            }
            Message::ShowPicture => {
                if core.chosen.is_some() && !core.busy {
                    core.send_chosen();
                }
            }
            Message::StopVideo => core.stop_video(),
            Message::ShowDashboard => {
                if !core.busy {
                    core.show_dashboard();
                }
            }
            Message::ShowAnimation => {
                if !core.busy {
                    core.show_animation();
                }
            }
            Message::Persist => {
                self._persist = None;
                core.persist();
            }
            Message::Refresh => {
                if !core.busy {
                    core.refresh();
                }
            }
            Message::Brightness(value) => {
                let brightness = value.round().clamp(0.0, 100.0) as u8;
                if brightness != core.brightness {
                    core.brightness = brightness;
                    self._brightness = Some(context.set_local_timeout(SETTLE, || Message::ApplyBrightness));
                }
            }
            // Sent once the slider rests; while the pump is busy, once it is free.
            Message::ApplyBrightness => {
                if core.busy {
                    self._brightness = Some(context.set_local_timeout(SETTLE, || Message::ApplyBrightness));
                } else {
                    self._brightness = None;
                    core.apply_brightness();
                }
            }
            Message::Rotation(Some(index)) => {
                let wanted = screen::ROTATIONS.get(index).copied();
                let current = core.snapshot.as_ref().map(|shot| shot.rotation);
                if let (Some(wanted), Some(current)) = (wanted, current) {
                    if wanted != current && !core.busy {
                        core.apply_rotation(wanted);
                    }
                }
            }
            Message::Rotation(None) | Message::Fahrenheit(None) => {}
            Message::ScreenOn(on) => {
                if core.snapshot.as_ref().is_some_and(|shot| shot.screen_on != on) && !core.busy {
                    core.apply_setting(Setting::ScreenOn(on));
                }
            }
            Message::Standby(on) => {
                if core.snapshot.as_ref().is_some_and(|shot| shot.display_in_sleep != on) && !core.busy {
                    core.apply_setting(Setting::DisplayInSleep(on));
                }
            }
            Message::Power(event) => {
                if !core.busy {
                    core.apply_power(event);
                }
            }
            Message::FirmwareClosed(result) => {
                if core.firmware_offer.is_some() {
                    if matches!(result, ContentDialogResult::Primary) {
                        core.install_firmware();
                    } else {
                        core.cancel_firmware();
                    }
                }
            }
            Message::ResumeLast(on) => {
                if on != core.settings.resume_last {
                    core.settings.resume_last = on;
                    core.persist();
                }
            }
            Message::SleepWhenLocked(on) => {
                if on != core.settings.sleep_when_locked {
                    core.set_sleep_when_locked(on);
                }
            }
            Message::Fahrenheit(Some(index)) => {
                let fahrenheit = index == 1;
                if fahrenheit != core.settings.fahrenheit {
                    core.settings.fahrenheit = fahrenheit;
                    core.persist();
                }
            }
            Message::Editor(message) => {
                let outcome = self.editor.update(message, core);
                if outcome.settle_later {
                    self._settle = Some(context.set_local_timeout(SETTLE, || Message::Editor(editor::Message::Settle)));
                }
                if outcome.redraw {
                    redraw = Redraw::Edited { last: outcome.settled };
                }
            }
            Message::Render => {
                self._render = None;
                redraw = Redraw::Now;
            }
            Message::FrameOpened(canvas, id) => self.frames[canvas as usize].opened(id),
        }
        drop(guard);
        let pump = handle.0.borrow();
        match redraw {
            Redraw::IfChanged => self.draw_screen(&pump, false),
            Redraw::Now => self.draw_screen(&pump, true),
            // A drag gets a frame every so often, and the last one when it ends.
            Redraw::Edited { last } => {
                if !self.draw_screen_throttled(&pump, last) && self._render.is_none() {
                    self._render = Some(context.set_local_timeout(editor::DRAG_FRAME, || Message::Render));
                }
            }
        }
    }

    fn view(&self, input: &PumpInput, context: &mut ViewContext<Self>) -> View {
        let pump = input.pump.0.borrow();
        let core = &pump.core;
        let tabs = SelectorBar::new()
            .items(
                Tab::ALL
                    .iter()
                    .map(|(tab, label)| SelectorBarItem::new().text(*label).is_selected(self.tab == *tab).into())
                    .collect::<Vec<View>>(),
            )
            .on_selected_text_changed(
                context.callback(|text: Option<Rc<str>>| Message::Tab(text.map(|text| text.to_string()))),
            );
        let mut parts: Vec<View> = vec![
            page_header_with("Pump screen", "The 720 x 720 screen on the ROG Strix LC IV.", connection_pill(core)),
            tabs.into(),
        ];
        if core.asus_running {
            parts.push(
                banner(
                    InfoBarSeverity::Warning,
                    "Quit ROG STRIX LC & SLC IV Series. It is holding the pump's USB connection.",
                )
                .into(),
            );
        }
        parts.push(match self.tab {
            Tab::Show => self.show_tab(&pump, context),
            Tab::Widgets => self.widgets_tab(&pump, context),
            Tab::Screen => self.screen_tab(&pump, context),
            Tab::Settings => self.settings_tab(&pump, context),
        });
        parts.push(caption(&core.status).into());
        page_frame(StackPanel::new().spacing(16.0).children(parts)).content_dialog(firmware_dialog(core, context))
    }
}

impl PumpPage {
    /// The layout being edited on a canvas, as it is now.
    pub(crate) fn layout_of(settings: &Settings, canvas: Canvas) -> Layout {
        match canvas {
            Canvas::Dashboard => settings.dashboard_layout.clone(),
            Canvas::Picture => settings.picture_layout.clone(),
        }
        .unwrap_or_default()
    }

    /// The screen this page shows right now, if any: the editor's, or the dashboard preview.
    fn visible_canvas(&self) -> Option<Canvas> {
        match self.tab {
            Tab::Widgets => Some(self.editor.canvas),
            Tab::Show if self.source_tab == Source::Dashboard => Some(Canvas::Dashboard),
            _ => None,
        }
    }

    /// Draw the visible screen if what it shows changed, or when `due` or its readings are a
    /// second old.
    fn draw_screen(&mut self, pump: &PumpCore, due: bool) {
        let Some(canvas) = self.visible_canvas() else {
            return;
        };
        let key = self.frame_key(pump, canvas);
        let frames = &self.frames[canvas as usize];
        let stale = frames.key.as_ref() != Some(&key)
            || frames.drawn_at.is_none_or(|at| at.elapsed() >= crate::stats::INTERVAL);
        if due || stale {
            self.render(pump, canvas, key);
        }
    }

    /// Draw now unless the last frame was drawn very recently. False when it waits.
    fn draw_screen_throttled(&mut self, pump: &PumpCore, now: bool) -> bool {
        let Some(canvas) = self.visible_canvas() else {
            return true;
        };
        let recent = self.frames[canvas as usize].drawn_at.is_some_and(|at| at.elapsed() < editor::DRAG_FRAME);
        if recent && !now {
            return false;
        }
        let key = self.frame_key(pump, canvas);
        self.render(pump, canvas, key);
        true
    }

    fn frame_key(&self, pump: &PumpCore, canvas: Canvas) -> parts::FrameKey {
        let layout = Self::layout_of(&pump.core.settings, canvas);
        let base = match (canvas, &pump.preview) {
            (Canvas::Dashboard, _) => parts::BaseKey::Backdrop(layout.background),
            (Canvas::Picture, Some(preview)) => {
                let (width, height) = (preview.pixels.width(), preview.pixels.height());
                parts::BaseKey::Picture(preview.id, pump.core.crop().square(width, height))
            }
            (Canvas::Picture, None) => parts::BaseKey::Empty,
        };
        parts::FrameKey { widgets: layout.widgets, base }
    }

    /// The same drawing the pump gets: the background or the cropped picture, with the widgets.
    fn render(&mut self, pump: &PumpCore, canvas: Canvas, key: parts::FrameKey) {
        let readings = pump.core.stats.readings().unwrap_or_default();
        let picture = match &key.base {
            parts::BaseKey::Backdrop(background) => {
                let backdrop = parts::backdrop_of(&mut self.backdrop, *background);
                crate::panel::dashboard(&readings, &Layout { widgets: key.widgets.clone(), background: *background }, &backdrop)
            }
            parts::BaseKey::Picture(id, square) => {
                let wanted = (*id, *square);
                if self.picture.as_ref().is_none_or(|cached| cached.key != wanted) {
                    let Some(preview) = &pump.preview else { return };
                    self.picture = Some(parts::Cached {
                        key: wanted,
                        value: Rc::new(parts::crop_to_screen(&preview.pixels, *square)),
                    });
                }
                let mut picture = (*self.picture.as_ref().expect("made above").value).clone();
                crate::panel::compose(&mut picture, &crate::panel::overlay(&readings, &key.widgets));
                picture
            }
            parts::BaseKey::Empty => {
                let mut picture = parts::plain_screen(parts::EMPTY_SCREEN);
                crate::panel::compose(&mut picture, &crate::panel::overlay(&readings, &key.widgets));
                picture
            }
        };
        self.frames[canvas as usize].push(key, parts::encode(&picture));
    }

    /// The live screen of a canvas, filling its parent.
    pub(crate) fn screen_frames(&self, canvas: Canvas, context: &ViewContext<Self>) -> View {
        self.frames[canvas as usize].view(|id| context.message(Message::FrameOpened(canvas, id)))
    }
}

/// Whether the pump is reachable, at the right of the page header.
fn connection_pill(core: &Controller) -> View {
    let screen_off = core.snapshot.as_ref().is_some_and(|shot| !shot.screen_on);
    let (text, tone) = if demo() {
        ("Preview, no pump", Tone::Quiet)
    } else if core.asus_running {
        ("ASUS app is open", Tone::Caution)
    } else if screen_off {
        ("Screen is off", Tone::Caution)
    } else if core.snapshot.is_some() {
        ("Connected", Tone::Good)
    } else if core.busy {
        ("Connecting…", Tone::Caution)
    } else {
        ("Not connected", Tone::Quiet)
    };
    pill(text, tone)
}

/// Confirm a checked firmware package before it is sent.
fn firmware_dialog(core: &Controller, context: &ViewContext<PumpPage>) -> ContentDialog {
    let (title, lines) = match &core.firmware_offer {
        Some(package) => {
            let installed = core.snapshot.as_ref().map_or("unknown".to_string(), |shot| shot.firmware.clone());
            let same =
                crate::firmware::parse_version(&installed) == crate::firmware::parse_version(&package.version);
            let first = if same {
                format!("The pump already has {installed}. This installs it again.")
            } else {
                format!("The pump's firmware is {installed} now.")
            };
            (format!("Install firmware {}?", package.version), first)
        }
        None => ("Install firmware?".to_string(), String::new()),
    };
    ContentDialog::new()
        .title(title)
        .content(
            StackPanel::new().spacing(8.0).max_width(440.0).children((
                body(&lines),
                body(
                    "The screen goes dark while the pump installs it and restarts. That takes about half a \
                     minute, and Coolwave reconnects on its own.",
                ),
                body(
                    "Keep the computer awake and the cooler plugged in until the screen is back. Cooling is \
                     not affected: the pump runs from the motherboard.",
                ),
            )),
        )
        .primary_button_text("Install")
        .close_button_text("Cancel")
        .default_button(ContentDialogButton::Close)
        .is_open(core.firmware_offer.is_some())
        .on_closed(context.callback(Message::FirmwareClosed))
}

/// The system file dialog, on a thread of its own.
fn pick_file(kind: Kind) -> Option<PathBuf> {
    let dialog = match kind {
        Kind::Picture => rfd::FileDialog::new().add_filter("Photos and video", PICTURE_EXTENSIONS),
        Kind::Firmware => rfd::FileDialog::new().add_filter("ASUS firmware package", &["zip"]),
    };
    dialog.pick_file()
}

/// Readings in the order they first appear, for laying a screen out again.
pub(crate) fn metrics_of(widgets: &[layout::Widget]) -> Vec<Metric> {
    let mut metrics = Vec::new();
    for widget in widgets {
        if !metrics.contains(&widget.metric) {
            metrics.push(widget.metric);
        }
    }
    metrics
}

/// "CPU · Temperature".
pub(crate) fn metric_label(metric: Metric) -> String {
    format!("{} · {}", metric.group(), metric.name())
}

/// What a reading needs when this computer does not report it.
pub(crate) fn missing_reason(metric: Metric) -> &'static str {
    match metric.source() {
        stats::Source::Nvidia => "Needs an NVIDIA card.",
        _ => "Needs HWiNFO running with Shared Memory Support turned on.",
    }
}
