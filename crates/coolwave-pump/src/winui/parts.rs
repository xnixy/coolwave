//! Pieces only the pump screen page needs: a labeled line, a switch row inside a card, the
//! screen's bezel and its live picture, and turning pictures into something an `Image` shows.

use coolwave_ui::*;
use image::{RgbImage, RgbaImage};
use std::rc::Rc;
use std::time::Instant;
use windows_reactor::*;

/// Width of the name at the start of a labeled line.
const LABEL_WIDTH: f64 = 96.0;
/// The panel's near-black, around the picture.
pub const BEZEL: Color = Color::rgb(8, 6, 10);
/// Where no picture is chosen yet.
pub const EMPTY_SCREEN: [u8; 3] = [22, 18, 26];
/// The crop square's outline: a pale rose that shows on any picture.
pub const CROP_OUTLINE: Color = Color::rgb(255, 228, 232);
/// Lines a dragged widget lines up with.
pub const GUIDE: Color = Color::rgb(255, 150, 170);

/// A name in the secondary color, then a control.
pub fn labeled(label: &str, control: impl Into<View>) -> View {
    Grid::new()
        .columns([GridLength::Pixel(LABEL_WIDTH), GridLength::STAR])
        .column_spacing(8.0)
        .children((
            muted(label).vertical_alignment(VerticalAlignment::Center),
            Border::new()
                .grid_column(1)
                .vertical_alignment(VerticalAlignment::Center)
                .horizontal_alignment(HorizontalAlignment::Left)
                .content(control),
        ))
        .into()
}

/// A switch with its title and words, for inside a card that has its own padding.
pub fn switch_line(on: bool, enabled: bool, heading: &str, words: &str, toggled: Callback<bool>) -> View {
    let mut text: Vec<View> = vec![body(heading).into()];
    if !words.is_empty() {
        text.push(caption(words).into());
    }
    Grid::new()
        .columns([GridLength::STAR, GridLength::Auto])
        .column_spacing(12.0)
        .margin(Thickness::new(0.0, 4.0, 0.0, 4.0))
        .children((
            StackPanel::new().vertical_alignment(VerticalAlignment::Center).children(text),
            Border::new()
                .grid_column(1)
                .vertical_alignment(VerticalAlignment::Center)
                .content(switch_enabled(on, enabled, heading, toggled)),
        ))
        .into()
}

/// A small heading inside a card, such as "Style" or "Color".
pub fn label(text: &str) -> View {
    caption(text).margin(Thickness::new(0.0, 6.0, 0.0, 0.0)).into()
}

/// Words in red, for a button that removes something.
pub fn danger(text: &str) -> View {
    TextBlock::new().text(text).foreground(ThemeBrush::SystemCritical).into()
}

/// A banner, with a button at its right when there is something to do about it.
pub fn banner_with(severity: InfoBarSeverity, message: &str, action: Option<View>) -> View {
    match action {
        None => banner(severity, message).into(),
        Some(action) => Grid::new()
            .columns([GridLength::STAR, GridLength::Auto])
            .column_spacing(12.0)
            .children((
                banner(severity, message),
                Border::new().grid_column(1).vertical_alignment(VerticalAlignment::Center).content(action),
            ))
            .into(),
    }
}

/// The panel's dark frame around a screen, so a preview reads as one.
pub fn bezel(content: impl Into<View>) -> Border {
    Border::new()
        .background(Brush::Solid(BEZEL))
        .border_brush(Brush::Solid(Color::argb(40, 255, 255, 255)))
        .border_thickness(1.0)
        .corner_radius(16.0)
        .padding(Thickness::new(6.0, 6.0, 6.0, 6.0))
        .content(content)
}

/// The live frames of one screen. A new frame goes on top of the last, which stays until the new
/// one has been decoded, so the picture never blinks.
#[derive(Default)]
pub struct Frames {
    /// Oldest first; at most two.
    pub shown: Vec<(u64, EncodedImage)>,
    next: u64,
    /// What the newest frame was drawn from.
    pub key: Option<FrameKey>,
    pub drawn_at: Option<Instant>,
}

/// What a frame depends on, apart from the readings.
#[derive(Clone, PartialEq)]
pub struct FrameKey {
    pub widgets: Vec<crate::layout::Widget>,
    pub base: BaseKey,
}

#[derive(Clone, PartialEq)]
pub enum BaseKey {
    Backdrop(crate::layout::Background),
    /// The preview's number and the crop square.
    Picture(u64, (u32, u32, u32)),
    Empty,
}

impl Frames {
    pub fn push(&mut self, key: FrameKey, image: EncodedImage) {
        self.next += 1;
        self.shown.push((self.next, image));
        while self.shown.len() > 2 {
            self.shown.remove(0);
        }
        self.key = Some(key);
        self.drawn_at = Some(Instant::now());
    }

    /// Frame `id` is on screen: the ones under it can go.
    pub fn opened(&mut self, id: u64) {
        self.shown.retain(|(shown, _)| *shown >= id);
    }

    /// The frames as images filling their parent, newest on top.
    pub fn view(&self, opened: impl Fn(u64) -> Callback<()>) -> View {
        Grid::new()
            .keyed_children(self.shown.iter().map(|(id, image)| {
                keyed(
                    id.to_string(),
                    Image::new().source_data(image.clone()).stretch(Stretch::Fill).on_opened(opened(*id)),
                )
            }))
            .into()
    }
}

/// An opaque picture as an encoded image. BMP: no compression to wait for, which matters while a
/// widget is dragged, and WinUI reads it as fast.
pub fn encode(picture: &RgbImage) -> EncodedImage {
    let mut bytes = Vec::with_capacity(picture.as_raw().len() + 64);
    let _ = picture.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Bmp);
    EncodedImage::new(bytes)
}

/// The part of a preview inside the crop square, at the pump's size.
pub fn crop_to_screen(pixels: &RgbImage, square: (u32, u32, u32)) -> RgbImage {
    let (side, x, y) = square;
    let side = side.max(1).min(pixels.width().saturating_sub(x)).min(pixels.height().saturating_sub(y)).max(1);
    let part = image::imageops::crop_imm(pixels, x, y, side, side).to_image();
    image::imageops::resize(&part, crate::panel::SIZE, crate::panel::SIZE, image::imageops::FilterType::Triangle)
}

/// A plain screen of one color.
pub fn plain_screen(rgb: [u8; 3]) -> RgbImage {
    RgbImage::from_pixel(crate::panel::SIZE, crate::panel::SIZE, image::Rgb(rgb))
}

/// A cached picture and what it was made from.
pub struct Cached<K, T> {
    pub key: K,
    pub value: Rc<T>,
}

/// The dashboard's background, made again only when it changes.
pub fn backdrop_of(
    cache: &mut Option<Cached<crate::layout::Background, RgbaImage>>,
    background: crate::layout::Background,
) -> Rc<RgbaImage> {
    if cache.as_ref().is_none_or(|cached| cached.key != background) {
        *cache = Some(Cached { key: background, value: Rc::new(crate::panel::backdrop(background)) });
    }
    cache.as_ref().expect("made above").value.clone()
}
