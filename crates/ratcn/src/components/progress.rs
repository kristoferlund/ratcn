//! A slim horizontal bar showing how far a task has come.
//!
//! ```text
//! Uploading notes.md                    56%
//! █████▋
//! ```
//!
//! This is a themed, opinionated take on ratatui's [`Gauge`]: the gauge keeps
//! the drawing — including the fractional block that lets the fill move in
//! steps finer than one cell — and this widget adds what an application
//! progress bar wants that a raw gauge does not carry:
//!
//! - theme colors, from a [`Theme`] via [`themed`](Self::themed) or an
//!   explicit [`ProgressStyle`] — the theme's primary as the fill on the
//!   inset well the other control surfaces use, the way the bar chart paints
//! - an optional [`label`](ProgressWidget::label) above the bar, left-aligned
//! - an optional [`show_value`](ProgressWidget::show_value) percentage,
//!   right-aligned on that same row, the way shadcn/ui composes
//!   `ProgressLabel` and `ProgressValue`
//!
//! The ratio is clamped into `0.0..=1.0` on the way in, so a denominator that
//! briefly misbehaves cannot smear the bar off its track.
//!
//! A progress bar takes no focus and handles no events, so there is no
//! interactive half.
//!
//! **Usable in any ratatui app.** Nothing here depends on
//! [`Ratcn`](crate::runtime::Ratcn) or the component layer — it is an ordinary
//! [`Widget`], so `frame.render_widget(...)` is all it needs.
//!
//! # Examples
//!
//! ```
//! use ratcn::ProgressWidget;
//!
//! // Just the bar.
//! # let _ =
//! ProgressWidget::new(0.33);
//!
//! // Label and percentage above the bar.
//! # let _ =
//! ProgressWidget::new(0.56)
//!     .label("Uploading notes.md")
//!     .show_value(true);
//! ```

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::Span,
    widgets::{Gauge, Widget},
};

use crate::{Theme, text_width};

/// Colors for a [`ProgressWidget`].
///
/// A bar has no interaction states, so unlike the interactive components'
/// style structs there is one color per role rather than one per role per
/// state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressStyle {
    /// The fill painted from the left — the part of the task done so far.
    pub fill: Color,
    /// The well the fill moves through — the whole extent of the task.
    pub track: Color,
    /// The [`label`](ProgressWidget::label) above the bar.
    pub label: Color,
    /// The [`show_value`](ProgressWidget::show_value) percentage.
    pub value: Color,
}

impl ProgressStyle {
    /// A neutral style using plain ANSI colors, for painting without a
    /// [`Theme`]. Prefer [`from_theme`](Self::from_theme) when one is
    /// available.
    #[must_use]
    pub const fn fallback() -> Self {
        Self {
            fill: Color::Cyan,
            track: Color::DarkGray,
            label: Color::Gray,
            value: Color::Gray,
        }
    }

    /// Derive bar colors from `theme`: the primary as the fill, on the inset
    /// well the chart backdrop uses, with the label read as secondary text
    /// and the percentage as ordinary text.
    #[must_use]
    pub const fn from_theme(theme: &Theme) -> Self {
        Self {
            fill: theme.primary,
            track: theme.field,
            label: theme.muted_foreground,
            value: theme.foreground,
        }
    }
}

/// A progress bar — the fill's share of the track is the work done.
///
/// One instantiation is one bar. The ratio arrives through
/// [`new`](Self::new); [`label`](Self::label) and
/// [`show_value`](Self::show_value) compose the header above the track; and
/// [`themed`](Self::themed) or [`style`](Self::style) choose the colors.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressWidget<'a> {
    /// The fraction done, already clamped into `0.0..=1.0`.
    ratio: f64,
    label: Option<&'a str>,
    show_value: bool,
    style: ProgressStyle,
}

impl<'a> ProgressWidget<'a> {
    /// A bare bar filled by `ratio`, where `0.0` is empty and `1.0` is full.
    ///
    /// Values outside `0.0..=1.0` are clamped: infinities pin to the nearer
    /// end, and a NaN that slipped out of a division counts as empty rather
    /// than printing itself as a percentage.
    #[must_use]
    pub fn new(ratio: f64) -> Self {
        Self {
            ratio: clamp_ratio(ratio),
            label: None,
            show_value: false,
            style: ProgressStyle::fallback(),
        }
    }

    /// Label the bar — the task's name, above the fill, left-aligned. Pair it
    /// with [`show_value`](Self::show_value) to complete the composition:
    /// name on the left, percentage on the right, bar underneath.
    #[must_use]
    pub const fn label(mut self, label: &'a str) -> Self {
        self.label = Some(label);
        self
    }

    /// Print the percentage right-aligned above the bar. Rounded to the
    /// nearest whole percent, `0%` through `100%`. The percentage and the
    /// fill round independently — ratatui's gauge rounds its last cell in
    /// eighths — so at a width's rounding edge the number can briefly sit one
    /// eighth of a cell away from the bar it describes.
    #[must_use]
    pub const fn show_value(mut self, show_value: bool) -> Self {
        self.show_value = show_value;
        self
    }

    /// Take colors from `theme`.
    #[must_use]
    pub const fn themed(mut self, theme: &Theme) -> Self {
        self.style = ProgressStyle::from_theme(theme);
        self
    }

    /// Use these exact colors, ignoring any theme.
    #[must_use]
    pub const fn style(mut self, style: ProgressStyle) -> Self {
        self.style = style;
        self
    }

    /// Rows the bar asks a layout for: two when a label or the percentage
    /// shows, one otherwise — an empty label still shows, since you asked
    /// for the row. Given less, the bar keeps the rows there are and
    /// the header is the first thing dropped; given more, the extra rows are
    /// left alone — a progress bar is one row of track, not a block meter.
    #[must_use]
    pub const fn height(&self) -> u16 {
        if self.has_header() { 2 } else { 1 }
    }

    const fn has_header(&self) -> bool {
        self.label.is_some() || self.show_value
    }
}

/// Any ratio becomes one a gauge can draw: finite values clamp into
/// `0.0..=1.0`, and infinities run to the nearer end while NaN lands empty.
fn clamp_ratio(ratio: f64) -> f64 {
    if ratio.is_nan() {
        0.0
    } else {
        ratio.clamp(0.0, 1.0)
    }
}

impl Widget for ProgressWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        // The header sits directly above the track, and only when there are
        // rows for both; squeezed to one row, the bar keeps the space.
        let (header_area, bar_area) = if self.has_header() && area.height >= 2 {
            (
                Some(Rect { height: 1, ..area }),
                Rect {
                    y: area.y + 1,
                    height: area.height - 1,
                    ..area
                },
            )
        } else {
            (None, area)
        };

        // The bar is exactly one row tall, wherever inside the area that row
        // was promised to be.
        let bar_area = crate::geometry::fixed_height(bar_area, 1);

        if let Some(header_area) = header_area {
            self.render_header(header_area, buf);
        }
        if !bar_area.is_empty() {
            Gauge::default()
                .ratio(self.ratio)
                .label("")
                .use_unicode(true)
                .gauge_style(Style::default().fg(self.style.fill).bg(self.style.track))
                .render(bar_area, buf);
            self.restore_the_label_slot(bar_area, buf);
        }
    }
}

impl ProgressWidget<'_> {
    /// Ratatui reserves the cell under its own centered label even when the
    /// label is empty: whenever the fill crosses the gauge's middle column,
    /// that one cell comes back blanked with the fill and track colors
    /// swapped. The percentage lives up in the header here, so the cell is
    /// restored to the full block its neighbors carry. The exact slot and
    /// fill math are pinned by the tests — an upstream change breaks loudly
    /// here rather than punching silent holes in every app's bars.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the ratio is clamped into 0.0..=1.0, so the fill never exceeds the area's width and always fits a u16"
    )]
    fn restore_the_label_slot(&self, bar_area: Rect, buf: &mut Buffer) {
        let filled_end = bar_area.x + (f64::from(bar_area.width) * self.ratio).floor() as u16;
        let label_slot = bar_area.x + bar_area.width / 2;
        if filled_end > label_slot {
            buf[(label_slot, bar_area.y)]
                .set_symbol(ratatui::symbols::block::FULL)
                .set_fg(self.style.fill)
                .set_bg(self.style.track);
        }
    }
    /// The header row: label flush left, percentage flush right, each in its
    /// own color. On a row too narrow for both, the percentage holds its
    /// place and the label takes what remains.
    fn render_header(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the ratio is clamped into 0.0..=1.0, so the percentage rounds into 0..=100 and always fits a u16"
        )]
        let value_text = format!("{}%", (self.ratio * 100.0).round() as u16);
        // Only a shown percentage may hold room at the right edge; a
        // label-only header gets the whole row.
        let value_width = if self.show_value {
            text_width::display_width_u16(&value_text).min(area.width)
        } else {
            0
        };
        let value_x = area.right() - value_width;

        if self.show_value {
            Span::raw(value_text.as_str())
                .style(Style::default().fg(self.style.value))
                .render(Rect::new(value_x, area.y, value_width, 1), buf);
        }

        if let Some(label) = self.label {
            let room = value_x - area.x;
            if room > 0 {
                Span::raw(text_width::truncate_to_width(label, usize::from(room)))
                    .style(Style::default().fg(self.style.label))
                    .render(Rect::new(area.x, area.y, room, 1), buf);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::symbols::block;

    const AREA: Rect = Rect::new(0, 0, 10, 2);

    fn symbols_of(buffer: &Buffer, y: u16) -> String {
        (0..AREA.width)
            .map(|x| buffer.cell((x, y)).expect("cell").symbol())
            .collect()
    }

    #[test]
    fn the_fill_takes_its_share_of_the_track() {
        let mut buffer = Buffer::empty(AREA);
        ProgressWidget::new(0.5).render(AREA, &mut buffer);

        assert_eq!(symbols_of(&buffer, 0), "█████     ");
        for x in 0..5u16 {
            assert_eq!(buffer.cell((x, 0)).expect("cell").fg, Color::Cyan);
        }
        // Unfilled cells keep the track behind them.
        assert_eq!(buffer.cell((7, 0)).expect("cell").bg, Color::DarkGray);
    }

    /// The fractional block lets the bar move in eighths of a cell: 55% of a
    /// ten-cell track is five full cells and a half-block sixth.
    #[test]
    fn the_boundary_cell_holds_a_partial_block() {
        let mut buffer = Buffer::empty(AREA);
        ProgressWidget::new(0.55).render(AREA, &mut buffer);

        assert_eq!(symbols_of(&buffer, 0), "█████▌    ");
        assert_eq!(
            buffer.cell((5, 0)).expect("cell").fg,
            Color::Cyan,
            "the partial block paints in the fill color"
        );
    }

    #[test]
    fn a_full_bar_leaves_no_partial_cell_and_an_empty_one_paints_only_track() {
        let mut full = Buffer::empty(AREA);
        ProgressWidget::new(1.0).render(AREA, &mut full);
        assert_eq!(symbols_of(&full, 0), "██████████");

        let mut empty = Buffer::empty(AREA);
        ProgressWidget::new(0.0).render(AREA, &mut empty);
        assert_eq!(symbols_of(&empty, 0), "          ");
        assert_eq!(empty.cell((3, 0)).expect("cell").bg, Color::DarkGray);
    }

    /// A fill that crosses the middle column must not lose the cell ratatui
    /// reserves for its own centered label — the regression the label-slot
    /// restore exists for. At 70% of ten cells the hole would sit inside the
    /// filled run; this pins it shut.
    #[test]
    fn a_fill_crossing_the_middle_leaves_no_hole() {
        let mut buffer = Buffer::empty(AREA);
        ProgressWidget::new(0.7).render(AREA, &mut buffer);
        assert_eq!(symbols_of(&buffer, 0), "███████   ");
    }

    /// Out-of-range ratios pin to the ends instead of smearing past them.
    #[test]
    fn ratios_clamp_to_the_ends() {
        let pinned_empty = {
            let mut buffer = Buffer::empty(AREA);
            ProgressWidget::new(-3.0).render(AREA, &mut buffer);
            symbols_of(&buffer, 0)
        };
        let pinned_full = {
            let mut buffer = Buffer::empty(AREA);
            ProgressWidget::new(7.0).render(AREA, &mut buffer);
            symbols_of(&buffer, 0)
        };
        assert_eq!(pinned_empty, "          ");
        assert_eq!(pinned_full, "██████████");
    }

    /// NaN must not print itself as a percentage or paint nonsense; it reads
    /// as nothing done yet.
    #[test]
    fn a_nan_ratio_reads_as_zero_percent() {
        let mut buffer = Buffer::empty(AREA);
        ProgressWidget::new(f64::NAN)
            .label("Broken")
            .show_value(true)
            .render(AREA, &mut buffer);

        let bar = symbols_of(&buffer, 1);
        assert_eq!(bar, "          ", "NaN must not fill anything");
        let header: String = (0..AREA.width)
            .map(|x| buffer.cell((x, 0)).expect("cell").symbol())
            .collect();
        assert!(header.ends_with("0%"), "{header:?}");
        assert!(!header.contains("NaN"), "{header:?}");
    }

    #[test]
    fn the_header_carries_the_label_left_and_the_value_right() {
        let theme = Theme::default_dark();
        let mut buffer = Buffer::empty(AREA);
        ProgressWidget::new(0.56)
            .label("Upload")
            .show_value(true)
            .themed(&theme)
            .render(AREA, &mut buffer);

        let header: Vec<char> = (0..AREA.width)
            .map(|x| {
                buffer
                    .cell((x, 0))
                    .expect("cell")
                    .symbol()
                    .chars()
                    .next()
                    .expect("one char")
            })
            .collect();
        assert_eq!(&header[0..6], ['U', 'p', 'l', 'o', 'a', 'd'], "{header:?}");
        assert_eq!(&header[7..10], ['5', '6', '%'], "{header:?}");
        assert_eq!(
            buffer.cell((0, 0)).expect("cell").fg,
            theme.muted_foreground,
            "the label reads as secondary text"
        );
        assert_eq!(
            buffer.cell((9, 0)).expect("cell").fg,
            theme.foreground,
            "the percentage is ordinary text"
        );
        // And the bar sits underneath, not beside.
        assert!(symbols_of(&buffer, 1).starts_with("█████"));
    }

    /// Two thirds rounds to 67%, not a truncated 66%.
    #[test]
    fn the_percentage_rounds_to_nearest() {
        let mut buffer = Buffer::empty(AREA);
        ProgressWidget::new(2.0 / 3.0)
            .show_value(true)
            .render(AREA, &mut buffer);

        let header: String = (0..AREA.width)
            .map(|x| buffer.cell((x, 0)).expect("cell").symbol())
            .collect();
        assert!(header.ends_with("67%"), "{header:?}");
    }

    /// One row cannot hold a header and a bar; the bar wins, and the widget
    /// says so through `height()` beforehand.
    #[test]
    fn a_single_row_area_keeps_the_bar_and_drops_the_header() {
        let area = Rect::new(0, 0, 10, 1);
        let mut buffer = Buffer::empty(area);
        ProgressWidget::new(0.5)
            .label("Upload")
            .show_value(true)
            .render(area, &mut buffer);

        let labeled = ProgressWidget::new(0.5).label("Upload");
        assert_eq!(labeled.height(), 2);
        assert_eq!(ProgressWidget::new(0.5).height(), 1);

        assert_eq!(symbols_of(&buffer, 0), "█████     ");
        assert!(
            !symbols_of(&buffer, 0).contains('%'),
            "the percentage must not paint over the track"
        );
    }

    /// Extra rows below the bar are left alone — this is a slim bar, not a
    /// block meter that grows to fill whatever it is handed.
    #[test]
    fn a_tall_area_leaves_the_rows_below_the_bar_alone() {
        let area = Rect::new(0, 0, 10, 4);
        let mut buffer = Buffer::empty(area);
        ProgressWidget::new(0.5)
            .themed(&Theme::default_dark())
            .render(area, &mut buffer);

        for y in 1..4u16 {
            for x in 0..10u16 {
                let cell = buffer.cell((x, y)).expect("cell");
                assert_eq!(cell.symbol(), " ", "row {y} column {x} painted");
                assert_eq!(cell.bg, Color::Reset, "row {y} column {x} tracked over");
            }
        }
    }

    /// A label too long for the row truncates before it would push the
    /// percentage off its right edge.
    #[test]
    fn a_long_label_yields_room_for_the_value() {
        let mut buffer = Buffer::empty(AREA);
        ProgressWidget::new(0.56)
            .label("Uploading notes.md and everything else")
            .show_value(true)
            .render(AREA, &mut buffer);

        let header: String = (0..AREA.width)
            .map(|x| buffer.cell((x, 0)).expect("cell").symbol())
            .collect();
        assert!(header.starts_with("Uploadi"), "{header:?}");
        assert!(header.ends_with("56%"), "{header:?}");
    }

    /// A label alone gets the whole row: no columns may sit empty for a
    /// percentage nobody asked to show.
    #[test]
    fn a_label_without_a_value_spans_the_whole_row() {
        let mut buffer = Buffer::empty(AREA);
        ProgressWidget::new(0.56)
            .label("ABCDEFGHIJKLMNOP")
            .render(AREA, &mut buffer);

        let header: String = (0..AREA.width)
            .map(|x| buffer.cell((x, 0)).expect("cell").symbol())
            .collect();
        assert_eq!(header, "ABCDEFGHIJ", "{header:?}");
    }

    /// The themed style lands where the docs say: primary fill on the field
    /// well, secondary text above.
    #[test]
    fn theme_colors_land_in_their_roles() {
        let theme = Theme::default_dark();
        let mut buffer = Buffer::empty(AREA);
        ProgressWidget::new(0.5)
            .label("Upload")
            .themed(&theme)
            .render(AREA, &mut buffer);

        assert_eq!(
            buffer.cell((0, 0)).expect("label cell").fg,
            theme.muted_foreground
        );
        assert_eq!(buffer.cell((0, 1)).expect("fill cell").fg, theme.primary);
        assert_eq!(buffer.cell((9, 1)).expect("track cell").bg, theme.field);
    }

    /// The fallback must read without a theme: the fill may not vanish into
    /// its own track.
    #[test]
    fn the_fallback_fill_is_visible_against_its_track() {
        assert_ne!(
            ProgressStyle::fallback().fill,
            ProgressStyle::fallback().track
        );

        let mut buffer = Buffer::empty(AREA);
        ProgressWidget::new(0.5).render(AREA, &mut buffer);
        let fill_cell = buffer.cell((0, 0)).expect("cell");
        assert_ne!(
            fill_cell.fg, fill_cell.bg,
            "the fill vanished into the track"
        );
    }

    /// An empty or zero-height area paints nothing and panics nowhere — the
    /// gauge under this widget would handle it, but the header math runs first.
    #[test]
    fn degenerate_areas_paint_nothing() {
        for area in [Rect::ZERO, Rect::new(0, 0, 0, 3), Rect::new(4, 4, 10, 0)] {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 4));
            ProgressWidget::new(0.5)
                .label("Upload")
                .show_value(true)
                .render(area, &mut buffer);
            assert_eq!(
                buffer,
                Buffer::empty(Rect::new(0, 0, 12, 4)),
                "area {area:?} painted"
            );
        }
    }

    /// The partial glyph comes from ratatui's block set — pin the exact one
    /// so a symbol-set change upstream is a conscious update, not a silent
    /// reflow of every progress bar in every app.
    #[test]
    fn the_partial_glyph_is_ratatuis_half_block() {
        let mut buffer = Buffer::empty(AREA);
        ProgressWidget::new(0.55).render(AREA, &mut buffer);
        assert_eq!(buffer.cell((5, 0)).expect("cell").symbol(), block::HALF);
    }
}
