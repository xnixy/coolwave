//! The pieces every page of the Windows window is made of, in Blurman's look: WinUI's type ramp,
//! cards with a hairline, and Windows Settings-style rows (title and words on the left, the
//! control on the right). Both the app's pages and the pump screen's use them.

#![cfg(windows)]

pub use windows_reactor;
use windows_reactor::*;

/// WinUI's display face, for page titles.
const DISPLAY_FONT: &str = "Segoe UI Variable Display";

/// A page: scrolls, with the boards' margins, no wider than reads well.
pub fn page_frame(content: impl Into<View>) -> View {
    ScrollViewer::new()
        .content(Border::new().padding(Thickness::new(32.0, 4.0, 32.0, 28.0)).content(content))
        .into()
}

/// A page's title and the line under it.
pub fn page_header(heading: &str, words: &str) -> View {
    StackPanel::new().children((title(heading), caption(words))).into()
}

/// A page's title and line, with something at the right (a status pill).
pub fn page_header_with(heading: &str, words: &str, right: impl Into<View>) -> View {
    Grid::new()
        .columns([GridLength::STAR, GridLength::Auto])
        .column_spacing(16.0)
        .children((
            page_header(heading, words),
            Border::new().grid_column(1).vertical_alignment(VerticalAlignment::Center).content(right),
        ))
        .into()
}

/// A page title: WinUI's Title style.
pub fn title(text: &str) -> TextBlock {
    TextBlock::new().text(text).font_family(DISPLAY_FONT).font_size(28.0).font_weight(FontWeight::SEMI_BOLD)
}

/// WinUI's Subtitle style.
pub fn subtitle(text: &str) -> TextBlock {
    TextBlock::new()
        .text(text)
        .font_family(DISPLAY_FONT)
        .font_size(20.0)
        .font_weight(FontWeight::SEMI_BOLD)
        .text_trimming(TextTrimming::CharacterEllipsis)
}

/// WinUI's BodyStrong style: section headings and card titles.
pub fn strong(text: &str) -> TextBlock {
    TextBlock::new().text(text).font_weight(FontWeight::SEMI_BOLD).text_wrapping(TextWrapping::Wrap)
}

/// Plain body text that wraps.
pub fn body(text: &str) -> TextBlock {
    TextBlock::new().text(text).text_wrapping(TextWrapping::Wrap)
}

/// WinUI's Caption style in the secondary text color.
pub fn caption(text: &str) -> TextBlock {
    TextBlock::new().text(text).font_size(12.0).foreground(ThemeBrush::SecondaryText).text_wrapping(TextWrapping::Wrap)
}

/// Body text in the secondary color.
pub fn muted(text: &str) -> TextBlock {
    TextBlock::new().text(text).foreground(ThemeBrush::SecondaryText).text_wrapping(TextWrapping::Wrap)
}

/// Words in the caution color, for a problem a row has.
pub fn warning(text: &str) -> TextBlock {
    TextBlock::new().text(text).font_size(12.0).foreground(ThemeBrush::SystemCaution).text_wrapping(TextWrapping::Wrap)
}

/// A Segoe Fluent Icons glyph.
pub fn glyph(code: &str, size: f64) -> FontIcon {
    FontIcon::new().glyph(code).font_size(size)
}

/// A button's words with an icon in front.
pub fn with_glyph(code: &str, text: &str) -> View {
    StackPanel::new()
        .orientation(Orientation::Horizontal)
        .spacing(8.0)
        .children((glyph(code, 14.0).vertical_alignment(VerticalAlignment::Center), TextBlock::new().text(text)))
        .into()
}

/// A card: the boards' fill, hairline and corners.
pub fn card(padding: Thickness, content: impl Into<View>) -> Border {
    Border::new()
        .background(ThemeBrush::CardBackground)
        .border_brush(ThemeBrush::CardStroke)
        .border_thickness(1.0)
        .corner_radius(7.0)
        .padding(padding)
        .content(content)
}

/// A card with the usual room inside.
pub fn padded_card(content: impl Into<View>) -> Border {
    card(Thickness::new(16.0, 14.0, 16.0, 16.0), content)
}

/// A heading over a card of rows.
pub fn section(heading: &str, items: Vec<View>) -> View {
    StackPanel::new().spacing(8.0).children((strong(heading), card(Thickness::new(0.0, 0.0, 0.0, 0.0), rows(items)))).into()
}

/// Rows, with a hairline between each.
pub fn rows(items: Vec<View>) -> View {
    let mut children: Vec<View> = Vec::new();
    for (index, item) in items.into_iter().enumerate() {
        if index > 0 {
            children.push(hairline());
        }
        children.push(item);
    }
    StackPanel::new().children(children).into()
}

/// The line between two rows.
pub fn hairline() -> View {
    Border::new().height(1.0).background(ThemeBrush::CardStroke).into()
}

/// A row: its title and what it says under that on the left, its control on the right.
pub fn row(heading: &str, words: &str, control: impl Into<View>) -> View {
    row_with(heading, words, Border::new().into(), control.into())
}

/// A row with something more under its words (a problem, a hint).
pub fn row_with(heading: &str, words: &str, under: View, control: View) -> View {
    let mut text: Vec<View> = vec![TextBlock::new().text(heading).text_wrapping(TextWrapping::Wrap).into()];
    if !words.is_empty() {
        text.push(caption(words).into());
    }
    text.push(under);
    Border::new()
        .padding(Thickness::new(16.0, 10.0, 16.0, 10.0))
        .min_height(62.0)
        .content(
            Grid::new().columns([GridLength::STAR, GridLength::Auto]).column_spacing(16.0).children((
                StackPanel::new().vertical_alignment(VerticalAlignment::Center).children(text),
                Border::new().grid_column(1).vertical_alignment(VerticalAlignment::Center).content(control),
            )),
        )
        .into()
}

/// A row whose control is too wide to sit beside its words, like a row of choices: the control
/// goes under them, so the words keep the row's whole width.
pub fn row_under(heading: &str, words: &str, control: impl Into<View>) -> View {
    let mut text: Vec<View> = vec![TextBlock::new().text(heading).text_wrapping(TextWrapping::Wrap).into()];
    if !words.is_empty() {
        text.push(caption(words).into());
    }
    Border::new()
        .padding(Thickness::new(16.0, 10.0, 16.0, 12.0))
        .min_height(62.0)
        .content(StackPanel::new().spacing(8.0).children((
            StackPanel::new().children(text),
            Border::new().horizontal_alignment(HorizontalAlignment::Left).content(control.into()),
        )))
        .into()
}

/// Several controls side by side.
pub fn line(spacing: f64, items: impl IntoViews) -> StackPanel {
    StackPanel::new().orientation(Orientation::Horizontal).spacing(spacing).children(items)
}

/// A switch without WinUI's On and Off words beside it: the row says what it is.
pub fn switch(on: bool, label: &str, toggled: Callback<bool>) -> View {
    ToggleSwitch::new()
        .is_on(on)
        .on_content("")
        .off_content("")
        .min_width(0.0)
        .automation_name(label)
        .on_toggled(toggled)
        .into()
}

/// A switch that is greyed out while `enabled` is false.
pub fn switch_enabled(on: bool, enabled: bool, label: &str, toggled: Callback<bool>) -> View {
    ToggleSwitch::new()
        .is_on(on)
        .is_enabled(enabled)
        .on_content("")
        .off_content("")
        .min_width(0.0)
        .automation_name(label)
        .on_toggled(toggled)
        .into()
}

/// What a slider covers, how it steps, and how its value reads after it.
#[derive(Clone, Copy)]
pub struct Range {
    pub min: f64,
    pub max: f64,
    pub step: f64,
}

impl Range {
    pub const fn new(min: f64, max: f64, step: f64) -> Self {
        Self { min, max, step }
    }

    /// A slider's value snapped to the step and kept in range.
    pub fn snap(&self, value: f64) -> f64 {
        let stepped = if self.step > 0.0 { ((value - self.min) / self.step).round() * self.step + self.min } else { value };
        stepped.clamp(self.min, self.max)
    }
}

/// A slider that fills its column with its value written after it. However narrow the column,
/// the value keeps its room. Made for a column of known width (beside a label, as in
/// `labeled`), not for a horizontal stack, where it would have no width to fill.
pub fn slider(value: f64, range: Range, text: String, label: &str, enabled: bool, changed: Callback<f64>) -> View {
    Grid::new()
        .columns([GridLength::STAR, GridLength::Pixel(VALUE_WIDTH)])
        .column_spacing(12.0)
        .children((
            Slider::new()
                .horizontal_alignment(HorizontalAlignment::Stretch)
                .min_width(MIN_SLIDER_WIDTH)
                .minimum(range.min)
                .maximum(range.max)
                .step_frequency(range.step)
                .value(value)
                .is_enabled(enabled)
                .automation_name(label)
                .vertical_alignment(VerticalAlignment::Center)
                .on_value_changed(changed),
            TextBlock::new()
                .grid_column(1)
                .text(text)
                .text_alignment(TextAlignment::Right)
                .foreground(ThemeBrush::SecondaryText)
                .vertical_alignment(VerticalAlignment::Center),
        ))
        .into()
}

/// The room a slider's value gets after it.
const VALUE_WIDTH: f64 = 52.0;
/// Narrower than this a slider is hard to set.
const MIN_SLIDER_WIDTH: f64 = 100.0;

/// A name on the left, the same width on every line, and what it names beside it.
pub fn labeled(label: &str, content: impl Into<View>) -> View {
    Grid::new()
        .columns([GridLength::Pixel(104.0), GridLength::STAR])
        .children((muted(label).vertical_alignment(VerticalAlignment::Center), Border::new().grid_column(1).content(content)))
        .into()
}

/// How a status pill is colored.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Good,
    Caution,
    Quiet,
}

/// A rounded label with a colored dot, such as "Connected" or "On the pump".
pub fn pill(text: &str, tone: Tone) -> View {
    let brush = match tone {
        Tone::Good => ThemeBrush::SystemSuccess,
        Tone::Caution => ThemeBrush::SystemCaution,
        Tone::Quiet => ThemeBrush::SecondaryText,
    };
    Border::new()
        .border_brush(brush)
        .border_thickness(1.0)
        .corner_radius(11.0)
        .padding(Thickness::new(9.0, 2.0, 10.0, 3.0))
        .vertical_alignment(VerticalAlignment::Center)
        .horizontal_alignment(HorizontalAlignment::Left)
        .content(line(
            6.0,
            (
                Ellipse::new().width(7.0).height(7.0).fill(brush).vertical_alignment(VerticalAlignment::Center),
                TextBlock::new().text(text).font_size(12.0).foreground(brush).vertical_alignment(VerticalAlignment::Center),
            ),
        ))
        .into()
}

/// A round dot of one color.
pub fn color_dot(color: Color, size: f64) -> Border {
    Border::new()
        .width(size)
        .height(size)
        .corner_radius(size / 2.0)
        .background(Brush::Solid(color))
        .border_brush(ThemeBrush::CardStroke)
        .border_thickness(1.0)
}

/// A button showing a color that opens a picker for it. The picker reports every change.
pub fn color_button(color: Color, label: &str, changed: Callback<Color>) -> View {
    Button::new()
        .min_width(0.0)
        .automation_name(label)
        .content(color_dot(color, 20.0))
        .flyout_with(Flyout::rich(
            ColorPicker::new()
                .color(color)
                .is_alpha_enabled(false)
                .is_hex_input_visible(true)
                .on_color_changed(changed),
        ))
        .tooltip(label)
}

/// A row of color dots to pick from. The current one has a ring.
pub fn swatches(current: [u8; 3], palette: &[[u8; 3]], label: &str, picked: impl Fn([u8; 3]) -> Callback<()>) -> View {
    let dots: Vec<View> = palette
        .iter()
        .map(|&rgb| {
            let color = Color::rgb(rgb[0], rgb[1], rgb[2]);
            let chosen = rgb == current;
            Button::new()
                .style(ButtonStyle::Subtle)
                .min_width(0.0)
                .automation_name(format!("{label} #{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]))
                .on_click(picked(rgb))
                .content(
                    Border::new()
                        .width(28.0)
                        .height(28.0)
                        .corner_radius(14.0)
                        .border_brush(if chosen { Brush::Theme(ThemeBrush::PrimaryText) } else { Brush::Solid(Color::transparent()) })
                        .border_thickness(2.0)
                        .padding(Thickness::new(3.0, 3.0, 3.0, 3.0))
                        .content(Border::new().corner_radius(9.0).background(Brush::Solid(color))),
                )
                .into()
        })
        .collect();
    VariableSizedWrapGrid::new().orientation(Orientation::Horizontal).item_width(48.0).item_height(42.0).children(dots).into()
}

/// Two cards side by side, the left one taking `left` parts of the width and the right `right`.
pub fn columns(left: f64, right: f64, first: impl Into<View>, second: impl Into<View>) -> View {
    Grid::new()
        .columns([GridLength::Star(left), GridLength::Star(right)])
        .column_spacing(12.0)
        .children((
            Border::new().vertical_alignment(VerticalAlignment::Top).content(first),
            Border::new().grid_column(1).vertical_alignment(VerticalAlignment::Top).content(second),
        ))
        .into()
}

/// A banner across a page: something to know or to fix.
pub fn banner(severity: InfoBarSeverity, message: &str) -> InfoBar {
    InfoBar::new().message(message).severity(severity).is_open(true).is_closable(false)
}

/// The engine's colors and WinUI's.
pub fn color_of(rgb: [u8; 3]) -> Color {
    Color::rgb(rgb[0], rgb[1], rgb[2])
}

/// Nothing, where a view is wanted.
pub fn nothing() -> View {
    Border::new().into()
}
