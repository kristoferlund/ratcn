//! The landing view: the site's front page, scrolling, with the `landing` demo
//! running at its natural size inside a preview window.
//!
//! [`layout`] is the view's one measurement. It subtracts the scroll area's
//! gutter, places every block, clamps the offset to the travel the content has,
//! and works out where the demo lands — so the height the area is told, the
//! rows the blocks paint in, and the rows the demo is blitted from all come
//! from the same arithmetic and cannot drift apart.

use ratatui::{
    layout::{Constraint, Margin, Rect, Size},
    style::{Color, Modifier, Style},
    text::{Line, Text},
    widgets::{Block, Paragraph},
};
use ratcn::{
    Button, ButtonSize, ScrollArea,
    geometry::wrapped_height,
    runtime::{DeclareCtx, FocusState},
};
use tui_big_text::{BigText, PixelSize};

use crate::{
    AppState, Msg, View, chrome,
    page_geometry::{
        BOTTOM_PADDING, HERO_WIDTH, Scroll, TOP_PADDING, centered, place, text_width, wrapped,
    },
};

/// The scroll area's child id. Nothing focuses the page by name: focus landing
/// inside it would be revealed, and a landing page that opens scrolled to its
/// own middle reads as broken. Tab reaches it, and the wheel needs no focus.
const ID: &str = "page";

/// The hero buttons' child ids, in declaration order. [`Layout::reveal`]
/// matches focus against them, so the ids and the rects stay one list.
const BUTTON_IDS: [&str; 2] = ["getting-started", "demos"];

/// The site's 620-pixel lede column, at the hero column's 12-pixel cell.
const LEDE_WIDTH: u16 = 52;

/// Between the hero's blocks.
const GAP: u16 = 2;
/// Between the preview window and the caption that explains it.
const CAPTION_GAP: u16 = 1;
/// Between the two hero buttons, and between them when they stack.
const BUTTON_GAP: u16 = 2;

/// What the preview window spends on itself: one row and column of border, and
/// a column of air inside it on each side. Without the air the demo's own tile
/// borders sit flush against the frame at every width where its grid exactly
/// fills the interior, and the two read as one doubled border.
const WINDOW_MARGIN: Margin = Margin {
    horizontal: 2,
    vertical: 1,
};

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

/// The hero buttons are the header's two links again, under the labels the
/// header names: one destination, one name.
const GET_STARTED: &str = chrome::labels::GETTING_STARTED;
const DEMOS: &str = chrome::labels::DEMOS;

/// The site says WebAssembly here; in a terminal the same claim is the other
/// way round.
const CAPTION: &str = "Every component above is real Ratatui, running live in this terminal. The exact same code runs in your browser.";
/// Short enough to fit the caption column at the narrowest terminal the chrome
/// lays out in, so it never wraps and strands a word.
const HINT: &str = "Click to interact · esc to leave";

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

/// The landing page, measured for one frame.
///
/// The block rects are relative to the top-left of the scroll area's content;
/// the declaration offsets them onto the content rect the runtime hands it.
/// Everything else is what the view needs around that: how far the page
/// scrolls, where it is scrolled to, and where the embedded demo goes.
#[derive(Debug, Clone, Copy)]
pub struct Layout {
    title: Rect,
    tier: Tier,
    lede: Rect,
    buttons: [Rect; 2],
    /// The preview window, its border included.
    window: Rect,
    /// The caption, with the interaction hint on the rows under it.
    caption: Rect,
    /// How far the page scrolls and where it is scrolled to.
    pub scroll: Scroll,
    /// The size the embedded demo's canvas has to be.
    pub canvas: Size,
    /// Where the demo's rows land on screen, and which of its own rows the
    /// first of them is. [`None`] when none of it is showing.
    pub embed: Option<(Rect, u16)>,
}

/// Measure the page for a body of `body`, scrolled to `offset`.
#[must_use]
pub fn layout(body: Rect, offset: u16) -> Layout {
    // The scroll area always keeps a gutter column for its scrollbar.
    let width = body.width.saturating_sub(1);
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
    let window = Rect::new(0, y, width, demo_height(width) + 2 * WINDOW_MARGIN.vertical);
    y += window.height + CAPTION_GAP;

    let caption = Rect::new(
        centered(width, caption_width),
        y,
        caption_width,
        wrapped_height(CAPTION, caption_width) + wrapped_height(HINT, caption_width),
    );
    y += caption.height + BOTTOM_PADDING;

    let interior = window.inner(WINDOW_MARGIN);
    let scroll = Scroll::new(y, body.height, offset);
    // Nothing scrolls horizontally, so the demo keeps the window's own columns
    // and only the rows move under it.
    let column = Rect::new(body.x + interior.x, body.y, interior.width, body.height);
    Layout {
        title,
        tier: tier(width),
        lede,
        buttons,
        window,
        caption,
        scroll,
        canvas: Size::new(interior.width, interior.height),
        embed: placement(column, interior.y, interior.height, scroll.offset),
    }
}

impl Layout {
    /// The offset that brings the hero button `focus` names fully into view,
    /// or [`None`] when it names neither or the page already shows it.
    ///
    /// The scroll area reveals a clipped descendant by taking a hold it never
    /// reports back, which would leave the page drawn at one offset and the
    /// demo blitted from another. Doing the reveal here keeps the offset the
    /// app holds the only one there is: by the next render the button is
    /// already in view, so the area finds nothing to move and takes no hold.
    #[must_use]
    pub fn reveal(&self, focus: &FocusState) -> Option<u16> {
        let button = BUTTON_IDS
            .iter()
            .zip(self.buttons)
            .find_map(|(id, rect)| focus.contains_path([ID, id]).then_some(rect))?;
        // The area's own rule, and minimal in both directions like it.
        let next = if button.y < self.scroll.offset {
            button.y
        } else if button.bottom() > self.scroll.offset.saturating_add(self.scroll.viewport) {
            button.bottom().saturating_sub(self.scroll.viewport)
        } else {
            self.scroll.offset
        };
        self.scroll.moved_to(next)
    }
}

/// Declare the page over `body`. `live` says the embedded demo has the input,
/// which is what the preview window's frame tells the user.
pub fn declare(ctx: &mut DeclareCtx<'_, AppState, Msg>, body: Rect, page: Layout, live: bool) {
    let area = ScrollArea::new(page.scroll.content)
        .scroll(
            |state: &AppState| state.landing_scroll,
            Msg::LandingScrolled,
        )
        .content(move |ctx| content(ctx, page, live));
    ctx.component(ID, area, body);
}

/// Paint the blocks and declare the two buttons, inside the scroll area's
/// logical content rect.
fn content(ctx: &mut DeclareCtx<'_, AppState, Msg>, page: Layout, live: bool) {
    let origin = ctx.area();
    let theme = *ctx.theme;

    let title = place(page.title, origin);
    match page.tier {
        Tier::Plain => ctx.paint_widget(
            Paragraph::new(
                hero_text(TITLE, title.width).style(
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
            hero_text(LEDE, lede.width).style(Style::default().fg(theme.muted_foreground)),
        ),
        lede,
    );

    ctx.component(
        BUTTON_IDS[0],
        Button::new(GET_STARTED)
            .size(ButtonSize::Large)
            .on_press(|| Msg::Navigate(View::GettingStarted)),
        place(page.buttons[0], origin),
    );
    ctx.component(
        BUTTON_IDS[1],
        Button::new(DEMOS)
            .outline()
            .size(ButtonSize::Large)
            .on_press(|| Msg::Navigate(View::Demos)),
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
    // one row, so a width that makes it wrap gets the row it needs.
    let [lines, hint] = caption.layout(&ratatui::layout::Layout::vertical([
        Constraint::Length(wrapped_height(CAPTION, caption.width)),
        Constraint::Fill(1),
    ]));
    ctx.paint_widget(
        Paragraph::new(hero_text(CAPTION, lines.width).style(muted)),
        lines,
    );
    ctx.paint_widget(
        Paragraph::new(hero_text(HINT, hint.width).style(muted)),
        hint,
    );
}

/// Where the embedded region lands on screen, and which of its own rows that
/// is.
///
/// `column` is the region's screen column and width, bounded vertically by the
/// viewport it lives in. The region occupies content rows `top..top + height`,
/// and `offset` is the page's scroll. [`None`] when none of it is showing.
fn placement(column: Rect, top: u16, height: u16, offset: u16) -> Option<(Rect, u16)> {
    let top = i32::from(top) - i32::from(offset);
    let skipped = u16::try_from(-top).unwrap_or(0).min(height);
    let y = column.y + u16::try_from(top.max(0)).expect("a clamped row fits");
    let visible = height
        .saturating_sub(skipped)
        .min(column.bottom().saturating_sub(y.min(column.bottom())));
    (visible > 0).then(|| (Rect::new(column.x, y, column.width, visible), skipped))
}

/// Rows the embedded demo needs inside a window `width` cells across.
fn demo_height(width: u16) -> u16 {
    landing::grid_height(width.saturating_sub(2 * WINDOW_MARGIN.horizontal))
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
    let second = button_width(DEMOS);
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

/// The shared wrap, centered: every text block on this page is a centred
/// column, and the alignment is the only thing the landing page adds.
fn hero_text(text: &'static str, width: u16) -> Text<'static> {
    wrapped(text, width).centered()
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

    use super::*;

    /// The cap [`DeclareCtx::viewport`] asserts on a viewport's logical
    /// content, in cells.
    const VIEWPORT_CELLS: u32 = 262_144;

    /// A layout with the scrolling numbers a test wants, and the blocks a real
    /// measurement gives.
    fn scrolling(offset: u16, viewport: u16, content: u16) -> Layout {
        Layout {
            scroll: Scroll {
                content,
                viewport,
                offset,
            },
            ..layout(Rect::new(0, 0, 80, viewport), offset)
        }
    }

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
    /// disagreement either clips the caption or leaves dead rows under it. And
    /// a viewport asserts on its logical content in *cells*, so the page has to
    /// stay under that too.
    #[test]
    fn the_page_ends_exactly_where_its_height_says() {
        // 61 is where the page is tallest: the widest terminal that still
        // gives the demo a one-column grid, with the title already at a big
        // tier.
        for width in [42, 46, 60, 61, 80, 116, 140, 320] {
            let page = layout(Rect::new(0, 0, width, 24), 0);
            assert_eq!(
                page.title.y, TOP_PADDING,
                "at width {width} the page does not start below its top padding"
            );
            assert_eq!(
                page.window.bottom() + CAPTION_GAP,
                page.caption.y,
                "at width {width} the caption does not follow the window"
            );
            assert_eq!(
                page.caption.bottom() + BOTTOM_PADDING,
                page.scroll.content,
                "at width {width} the last block does not end where the height says"
            );
            let cells = u32::from(width) * u32::from(page.scroll.content);
            assert!(
                cells <= VIEWPORT_CELLS,
                "at width {width} the page is {cells} cells, past the viewport's cap"
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

    /// The app reveals its own hero buttons so the scroll area never takes a
    /// hold it does not report — which would draw the page and the demo at two
    /// different offsets. Minimal in both directions, like the area's own.
    #[test]
    fn a_focused_hero_button_is_revealed_by_the_smallest_scroll_that_shows_it() {
        // A ten-row viewport over forty rows, with a three-row button at rows
        // 20..23.
        let button = Rect::new(0, 20, 30, 3);
        let page = |offset| Layout {
            buttons: [button, Rect::new(0, 34, 10, 3)],
            ..scrolling(offset, 10, 40)
        };
        let focus = |id| FocusState::intent([ID, id]);

        assert_eq!(
            page(0).reveal(&focus(BUTTON_IDS[0])),
            Some(13),
            "a button below the viewport comes to its bottom edge, no further"
        );
        assert_eq!(
            page(25).reveal(&focus(BUTTON_IDS[0])),
            Some(20),
            "a button above it comes to its top edge"
        );
        assert_eq!(
            page(15).reveal(&focus(BUTTON_IDS[0])),
            None,
            "a button already in view moves nothing"
        );
        assert_eq!(
            page(0).reveal(&focus(BUTTON_IDS[1])),
            Some(27),
            "the second button comes to the bottom edge on its own account"
        );
        assert_eq!(
            Layout {
                buttons: [button, Rect::new(0, 38, 10, 3)],
                ..scrolling(0, 10, 40)
            }
            .reveal(&focus(BUTTON_IDS[1])),
            Some(30),
            "and a reveal never asks for more scroll than the page has"
        );
        assert_eq!(
            page(0).reveal(&FocusState::intent(["home"])),
            None,
            "focus on the chrome is not the page's to reveal"
        );
        assert_eq!(
            page(0).reveal(&FocusState::default()),
            None,
            "and neither is unresolved focus"
        );
    }
}
