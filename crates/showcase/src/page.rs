//! The landing view: the site's front page, scrolling, with the `landing` demo
//! running at its natural size inside a preview window.
//!
//! Every block's height is decided by [`page`] alone, so the height the scroll
//! area is told and the rows the blocks are painted in cannot drift apart. The
//! rects come back relative to the top-left of the content, and the declaration
//! offsets them onto the content rect the runtime hands it.

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Margin, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Text},
    widgets::{Block, Paragraph},
};
use ratcn::{
    Button, ButtonSize, ScrollArea,
    geometry::wrapped_height,
    runtime::{DeclareCtx, KeyCode},
    text_width::wrap_to_width,
};
use tui_big_text::{BigText, PixelSize};

use crate::{AppState, Msg, chrome};

/// The scroll area's child id. Nothing focuses the page by name: focus landing
/// inside it would be revealed, and a landing page that opens scrolled to its
/// own middle reads as broken. Tab reaches it, and the wheel needs no focus.
const ID: &str = "page";

/// The site's 880-pixel hero column, at a 12-pixel cell.
const HERO_WIDTH: u16 = 74;
/// Its 620-pixel lede column, at the same cell.
const LEDE_WIDTH: u16 = 52;

const TOP_PADDING: u16 = 3;
/// Between the hero's blocks.
const GAP: u16 = 2;
/// Between the preview window and the caption that explains it.
const CAPTION_GAP: u16 = 1;
const BOTTOM_PADDING: u16 = 2;
/// Between the two hero buttons, and between them when they stack.
const BUTTON_GAP: u16 = 2;

const TITLE: &str = "The Foundation for your Terminal UI";
/// The heading, broken the way the site breaks it.
const TITLE_LINES: [&str; 3] = ["The Foundation", "for your", "Terminal UI"];
/// Rows one big glyph takes. A `tui-big-text` glyph is 8×8 pixels, so
/// `HalfHeight` spends 8 cells across and 4 down on one, and `Quadrant` 4 and
/// 4 — the same height. That is what keeps the hero from jumping when the
/// terminal crosses the breakpoint between them.
const GLYPH_ROWS: u16 = 4;
const BIG_TITLE_HEIGHT: u16 = TITLE_LINES.len() as u16 * GLYPH_ROWS;
/// Content widths the two big tiers need: the longest title line is 14 glyphs,
/// which is 112 cells at 8 across and 56 at 4.
const HALF_HEIGHT_WIDTH: u16 = 116;
const QUADRANT_WIDTH: u16 = 60;

const LEDE: &str = "A set of beautifully designed components that you can copy and paste into your Ratatui apps. Themeable. App-owned state. Open Source. Open Code.";

const GET_STARTED: &str = "Get Started with the CLI →";
const GET_STARTED_URL: &str = "https://ratcn.kristoferlund.se/docs/getting-started";
const GITHUB: &str = "GitHub";
const GITHUB_URL: &str = "https://github.com/kristoferlund/ratcn";

/// The site says WebAssembly here; in a terminal the same claim is the other
/// way round.
const CAPTION: &str = "Every component above is real Ratatui, running live in this terminal. The exact same code runs in your browser.";
const HINT: &str = "Click the preview to interact · esc to leave";

/// How the title is drawn at a given width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Big text at 8×4 cells a glyph.
    HalfHeight,
    /// Big text at 4×4 cells a glyph.
    Quadrant,
    /// Too narrow for either: a bold centred paragraph of the whole title.
    Plain,
}

/// Where every block of the page sits, relative to the top-left of the
/// content.
#[derive(Debug, Clone, Copy)]
pub struct Page {
    pub title: Rect,
    pub tier: Tier,
    pub lede: Rect,
    /// The two hero buttons, in the order they are declared.
    pub buttons: [Rect; 2],
    /// The preview window, its border included.
    pub window: Rect,
    /// The caption, with the interaction hint on the rows under it.
    pub caption: Rect,
    /// Rows the whole page occupies: what the scroll area is told.
    pub height: u16,
}

/// Lay the page out for a content column `width` cells across.
#[must_use]
pub fn page(width: u16) -> Page {
    let hero = HERO_WIDTH.min(width);
    let lede_width = text_width(width, LEDE_WIDTH);
    // Wider than the lede: at the lede's measure this orphans its last word,
    // and at the hero's it wraps to two lines and reads as the figure caption
    // it is.
    let caption_width = text_width(width, HERO_WIDTH);
    let mut y = TOP_PADDING;

    // The title is a heading and takes the whole column: that is what earns
    // the wide tier on a 120-column terminal. It needs no air of its own —
    // the big tiers are centred inside a width the ladder already guarantees
    // fits, and the plain one is a short centred line.
    let title = Rect::new(0, y, width, title_height(width));
    y += title.height + GAP;

    let lede = Rect::new(
        centered(width, lede_width),
        y,
        lede_width,
        wrapped_height(LEDE, lede_width),
    );
    y += lede.height + GAP;

    let (buttons, buttons_height) = button_rects(width, hero, y);
    y += buttons_height + GAP;

    // The window keeps the whole column: an edge beside the scrollbar reads
    // fine, and the demo inside wants every cell it can have.
    let window = Rect::new(0, y, width, demo_height(width) + 2);
    y += window.height + CAPTION_GAP;

    let caption = Rect::new(
        centered(width, caption_width),
        y,
        caption_width,
        wrapped_height(CAPTION, caption_width) + wrapped_height(HINT, caption_width),
    );
    y += caption.height + BOTTOM_PADDING;

    Page {
        title,
        tier: tier(width),
        lede,
        buttons,
        window,
        caption,
        height: y,
    }
}

/// Declare the page over `body`. `live` says the embedded demo has the input,
/// which is what the preview window's frame tells the user.
pub fn declare(ctx: &mut DeclareCtx<'_, AppState, Msg>, body: Rect, page: Page, live: bool) {
    // No guard on the cell cap the viewport asserts (262,144): the page is at
    // its tallest in the demo's one-column layout, around 215 rows at 45
    // columns, which is two orders of magnitude under it.
    let area = ScrollArea::new(page.height)
        .scroll(|state: &AppState| state.page_scroll, Msg::PageScrolled)
        .content(move |ctx| content(ctx, page, live));
    ctx.component(ID, area, body);
}

/// Paint the blocks and declare the two buttons, inside the scroll area's
/// logical content rect.
fn content(ctx: &mut DeclareCtx<'_, AppState, Msg>, page: Page, live: bool) {
    let origin = ctx.area();
    let theme = *ctx.theme;

    let title = place(page.title, origin);
    match page.tier {
        Tier::Plain => ctx.paint_widget(
            Paragraph::new(
                wrapped(TITLE, title.width).style(
                    Style::default()
                        .fg(theme.foreground)
                        .add_modifier(Modifier::BOLD),
                ),
            ),
            title,
        ),
        tier => ctx.paint_widget(big_title(tier, theme.foreground), title),
    }

    let lede = place(page.lede, origin);
    ctx.paint_widget(
        Paragraph::new(
            wrapped(LEDE, lede.width).style(Style::default().fg(theme.muted_foreground)),
        ),
        lede,
    );

    ctx.component(
        "get-started",
        Button::new(GET_STARTED)
            .size(ButtonSize::Large)
            .on_press(|| Msg::Open(GET_STARTED_URL)),
        place(page.buttons[0], origin),
    );
    ctx.component(
        "github",
        Button::new(GITHUB)
            .outline()
            .size(ButtonSize::Large)
            .on_press(|| Msg::Open(GITHUB_URL)),
        place(page.buttons[1], origin),
    );

    let muted = Style::default().fg(theme.muted_foreground);
    ctx.paint_widget(
        Block::bordered()
            // The window lights up while the demo inside has the input, the
            // way the Demos view's vertical rule does.
            .border_style(Style::default().fg(chrome::rule_color(&theme, live)))
            // The site's browser chrome, in the window's own frame.
            .title_top(Line::from(" ● ● ● ").style(muted))
            .title_top(Line::from(" cargo run -p landing ").style(muted).centered()),
        place(page.window, origin),
    );

    let caption = place(page.caption, origin);
    // The hint is measured like the caption above it rather than assumed to be
    // one row: at a narrow width it is a cell too long for the column, and a
    // reserved single row would truncate it.
    let [lines, hint] = caption.layout(&Layout::vertical([
        Constraint::Length(wrapped_height(CAPTION, caption.width)),
        Constraint::Fill(1),
    ]));
    ctx.paint_widget(
        Paragraph::new(wrapped(CAPTION, lines.width).style(muted)),
        lines,
    );
    ctx.paint_widget(Paragraph::new(wrapped(HINT, hint.width).style(muted)), hint);
}

/// Where the embedded region lands on screen, and which of its own rows that
/// is.
///
/// `column` is the region's screen column and width — nothing scrolls
/// horizontally — bounded vertically by the viewport it lives in. The region
/// occupies content rows `top..top + height`, and `offset` is the page's
/// scroll. [`None`] when none of it is showing.
#[must_use]
pub fn placement(column: Rect, top: u16, height: u16, offset: u16) -> Option<(Rect, u16)> {
    let top = i32::from(top) - i32::from(offset);
    let skipped = u16::try_from(-top).unwrap_or(0).min(height);
    let y = column.y + u16::try_from(top.max(0)).expect("a clamped row fits");
    let visible = height
        .saturating_sub(skipped)
        .min(column.bottom().saturating_sub(y.min(column.bottom())));
    (visible > 0).then(|| (Rect::new(column.x, y, column.width, visible), skipped))
}

/// Where a page key leaves the scroll offset, or [`None`] when it is not one
/// of them or the view is already there.
///
/// The landing page is the whole view, so these keys are the app's the way the
/// `landing` demo's own alt-chords are its. The scroll area answers them first
/// whenever focus is inside it; this is what happens when it is not. Nothing
/// here reaches into the area — the offset is the one the app already holds,
/// and `content - viewport` is all the travel there is, so a page that fits
/// its viewport does not move for any of them.
#[must_use]
pub fn scrolled(key: KeyCode, offset: u16, viewport: u16, content: u16) -> Option<u16> {
    let furthest = content.saturating_sub(viewport);
    let next = match key {
        KeyCode::PageDown => offset.saturating_add(viewport),
        KeyCode::PageUp => offset.saturating_sub(viewport),
        KeyCode::Home => 0,
        KeyCode::End => furthest,
        _ => return None,
    };
    let next = next.min(furthest);
    (next != offset).then_some(next)
}

/// Copy `target.height` rows of `source`, starting at its row `source_y`, into
/// `target`.
///
/// The embedded demo owns its own runtime over its own state, so it cannot be
/// declared inside the scroll area's viewport. It paints a canvas of exactly
/// its own size instead, and the rows the page is showing are copied out of it.
pub fn blit(destination: &mut Buffer, source: &Buffer, target: Rect, source_y: u16) {
    for row in 0..target.height {
        for column in 0..target.width {
            let Some(cell) = source.cell(Position::new(column, source_y + row)) else {
                continue;
            };
            destination[(target.x + column, target.y + row)] = cell.clone();
        }
    }
}

/// Rows the embedded demo needs inside a window `width` cells across.
fn demo_height(width: u16) -> u16 {
    landing::grid_height(width.saturating_sub(2))
}

/// How the title is drawn at `width`.
fn tier(width: u16) -> Tier {
    if width >= HALF_HEIGHT_WIDTH {
        Tier::HalfHeight
    } else if width >= QUADRANT_WIDTH {
        Tier::Quadrant
    } else {
        Tier::Plain
    }
}

fn title_height(width: u16) -> u16 {
    match tier(width) {
        Tier::HalfHeight | Tier::Quadrant => BIG_TITLE_HEIGHT,
        Tier::Plain => wrapped_height(TITLE, width),
    }
}

fn big_title(tier: Tier, color: Color) -> BigText<'static> {
    let pixel_size = match tier {
        Tier::HalfHeight => PixelSize::HalfHeight,
        // Nothing narrower than `Quadrant` is safe: the sextant, octant,
        // third- and quarter-height code points are too new to be in most
        // terminal fonts.
        Tier::Quadrant | Tier::Plain => PixelSize::Quadrant,
    };
    BigText::builder()
        .pixel_size(pixel_size)
        .lines(TITLE_LINES.map(Line::from).to_vec())
        .centered()
        .style(Style::default().fg(color))
        .build()
}

/// The two buttons side by side while the hero column holds both, stacked
/// otherwise, and the rows they take.
fn button_rects(width: u16, hero: u16, y: u16) -> ([Rect; 2], u16) {
    let row = ButtonSize::Large.height();
    let first = button_width(GET_STARTED);
    let second = button_width(GITHUB);
    let together = first + BUTTON_GAP + second;
    if together <= hero {
        let x = centered(width, together);
        return (
            [
                Rect::new(x, y, first, row),
                Rect::new(x + first + BUTTON_GAP, y, second, row),
            ],
            row,
        );
    }
    (
        [
            Rect::new(centered(width, first), y, first, row),
            Rect::new(centered(width, second), y + row + 1, second, row),
        ],
        2 * row + 1,
    )
}

/// Cells a hero button takes, from the component's own arithmetic.
fn button_width(label: &str) -> u16 {
    Button::<Msg>::new(label).width()
}

/// `text` wrapped to `width` by the same code [`wrapped_height`] measures it
/// with, so the paint cannot need a row the layout did not reserve.
fn wrapped(text: &'static str, width: u16) -> Text<'static> {
    Text::from(
        wrap_to_width(text, usize::from(width.max(1)))
            .into_iter()
            .map(|line| Line::from(line).centered())
            .collect::<Vec<_>>(),
    )
}

/// A content-relative rect, moved onto the content rect at `origin`.
fn place(rect: Rect, origin: Rect) -> Rect {
    Rect::new(
        origin.x + rect.x,
        origin.y + rect.y,
        rect.width,
        rect.height,
    )
}

/// The left edge that centers `width` cells in `total`.
const fn centered(total: u16, width: u16) -> u16 {
    total.saturating_sub(width) / 2
}

/// A text column at its own `maximum`, and never closer than a cell to either
/// edge of the content — so a narrow terminal does not run the last character
/// of a line straight into the scrollbar gutter.
const fn text_width(content: u16, maximum: u16) -> u16 {
    let fits = content.saturating_sub(2);
    if maximum < fits { maximum } else { fits }
}

/// The window's interior, in content coordinates.
#[must_use]
pub fn interior(window: Rect) -> Rect {
    window.inner(Margin::new(1, 1))
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;

    /// Rows of `buffer` that have anything painted on them.
    fn painted_rows(buffer: &Buffer) -> u16 {
        (0..buffer.area.height)
            .filter(|&row| {
                (0..buffer.area.width)
                    .any(|column| !buffer[(column, row)].symbol().trim().is_empty())
            })
            .count() as u16
    }

    /// The two big tiers have to agree on height, or the hero would jump as the
    /// terminal crossed the breakpoint between them. Measured from what the
    /// widget actually paints, not from the constant that predicts it.
    #[test]
    fn both_big_title_tiers_are_twelve_rows() {
        let tiers = [
            (HALF_HEIGHT_WIDTH, Tier::HalfHeight),
            (QUADRANT_WIDTH, Tier::Quadrant),
        ];
        for (width, expected) in tiers {
            assert_eq!(tier(width), expected, "at width {width}");
            assert_eq!(title_height(width), BIG_TITLE_HEIGHT, "at width {width}");

            let mut terminal = Terminal::new(TestBackend::new(width, 40)).expect("a test backend");
            let painted = terminal
                .draw(|frame| frame.render_widget(big_title(expected, Color::White), frame.area()))
                .expect("the test backend draws")
                .buffer;
            assert_eq!(
                painted_rows(painted),
                BIG_TITLE_HEIGHT,
                "the {expected:?} title does not fill the rows the layout reserves"
            );
        }
    }

    /// The height the scroll area is told is the height the blocks fill: a
    /// disagreement either clips the caption or leaves dead rows under it.
    #[test]
    fn the_page_ends_exactly_where_its_height_says() {
        for width in [41, 45, 59, 79, 115, 139] {
            let page = page(width);
            assert_eq!(
                page.title.y, TOP_PADDING,
                "at width {width} the page does not start below its top padding"
            );
            assert!(
                page.window.bottom() + CAPTION_GAP == page.caption.y,
                "at width {width} the caption does not follow the window"
            );
            assert_eq!(
                page.caption.bottom() + BOTTOM_PADDING,
                page.height,
                "at width {width} the last block does not end where the height says"
            );
        }
    }

    /// The page keys are the app's on this view, so their arithmetic is the
    /// app's to get right: neither end may be overshot, and a page that cannot
    /// scroll must not claim it did.
    #[test]
    fn page_keys_step_by_a_viewport_and_stop_at_both_ends() {
        // Forty rows of page in a ten-row viewport: thirty rows of travel.
        let (viewport, content) = (10, 40);
        let scroll = |key, offset| scrolled(key, offset, viewport, content);

        assert_eq!(
            scroll(KeyCode::PageDown, 0),
            Some(10),
            "a page down from the top"
        );
        assert_eq!(
            scroll(KeyCode::PageDown, 25),
            Some(30),
            "the last page down is a short one"
        );
        assert_eq!(
            scroll(KeyCode::PageDown, 30),
            None,
            "a page down at the bottom moves nothing"
        );
        assert_eq!(
            scroll(KeyCode::PageUp, 0),
            None,
            "and a page up at the top moves nothing"
        );
        assert_eq!(
            scroll(KeyCode::PageUp, 25),
            Some(15),
            "a page up from the middle"
        );
        assert_eq!(scroll(KeyCode::End, 0), Some(30), "End is the furthest row");
        assert_eq!(scroll(KeyCode::Home, 30), Some(0), "Home is the first");
        assert_eq!(
            scroll(KeyCode::Enter, 0),
            None,
            "and Enter is not a page key"
        );

        for key in [
            KeyCode::PageDown,
            KeyCode::PageUp,
            KeyCode::Home,
            KeyCode::End,
        ] {
            assert_eq!(
                scrolled(key, 0, 20, 8),
                None,
                "{key:?} on a page shorter than its viewport"
            );
        }
    }

    /// The demo region stands still on screen while the page scrolls under it,
    /// and the rows it shows are the rows scrolled to.
    #[test]
    fn placement_follows_the_scroll_and_stops_at_the_viewport() {
        // A viewport ten rows tall, starting at screen row 2.
        let column = Rect::new(4, 2, 30, 10);

        assert_eq!(
            placement(column, 0, 4, 0),
            Some((Rect::new(4, 2, 30, 4), 0)),
            "unscrolled, a region at the top of the content is shown whole"
        );
        assert_eq!(
            placement(column, 20, 20, 20),
            Some((Rect::new(4, 2, 30, 10), 0)),
            "scrolled to the region, it starts at the viewport top and fills it"
        );
        assert_eq!(
            placement(column, 20, 20, 45),
            None,
            "scrolled past the region entirely, none of it is showing"
        );
        assert_eq!(
            placement(column, 20, 4, 22),
            Some((Rect::new(4, 2, 30, 2), 2)),
            "a region shorter than the viewport, two of its rows scrolled off the top"
        );
        assert_eq!(
            placement(column, 20, 4, 8),
            None,
            "a region still below the viewport is not showing either"
        );
    }
}
