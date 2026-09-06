//! The Getting started view: a scrolling page of prose and code.
//!
//! The page is a list of [`Block`]s and one walk over it, [`placed`], which
//! answers where each block goes and how tall the page is. The layout, the
//! paint, and the tests all read that same walk, so the height the scroll area
//! is told and the rows the blocks land on cannot drift apart.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
};
use ratcn::{
    ScrollArea, Theme,
    geometry::wrapped_height,
    runtime::DeclareCtx,
    text_width::{display_width_u16, truncate_to_width},
};

use crate::{
    AppState, Msg, code,
    page_geometry::{
        BOTTOM_PADDING, HERO_WIDTH, Scroll, TOP_PADDING, centered, place, text_width, wrapped,
    },
};

/// The scroll area's child id. Not `getting-started`: the header link that
/// leads here already has that name, and both are declared in the root scope.
const ID: &str = "getting-started-page";

/// The blank row between blocks.
const GAP: u16 = 1;
/// Rows of air above a heading, on top of the [`GAP`] every block gets. A
/// heading belongs to what follows it, so it sits nearer that than the block
/// it closes off — without this the headings scan as ordinary sentences.
const HEADING_LEAD: u16 = 1;
/// Code is set in from the prose, the way a fenced block is on the site.
const CODE_INDENT: u16 = 2;
/// What marks a code line the column was too narrow to show whole.
const ELLIPSIS: &str = "…";

const TITLE: &str = "Getting started";
const LEDE: &str = "ratcn is a component library for Ratatui: components you copy, theme, and own in your source, plus a small runtime for focus, hover, and events. It never takes over your app loop.";
const PREVIEW: &str = "Preview release. The API is unstable — pin an exact version and expect to edit when you upgrade. Twelve components today: Button, List, Select, Tabs, Dialog, ScrollArea, Checkbox, Cycle, Tooltip, ToasterWidget, BarChartWidget, ProgressWidget.";

const INSTALL: &str = "Install";
const INSTALL_INTRO: &str = "Requires Rust 1.88. The cargo-ratcn CLI sets terminal projects up and copies components into them.";
const INSTALL_SHELL: &str = include_str!("../snippets/install.sh");
const INIT_DOES: &str = "init adds ratcn with its termina feature, a compatible ratatui, ratcn.toml, and src/components/mod.rs. Over Cargo's untouched default main.rs, and only on a terminal, it also offers a starter: keep it, a minimal app, or a demo app with a button and a Hello World toast. Source you wrote yourself is never replaced.";
const INIT_RUN: &str = "Take the demo app, run cargo run, and a ratcn app is on screen.";

const COPY: &str = "Copy a component";
const COPY_INTRO: &str = "Every component module is self-contained, so you can own one:";
const COPY_SHELL: &str = include_str!("../snippets/add.sh");
const COPY_DOES: &str = "add copies the source from the exact ratcn package your project resolved, registers the module, and never replaces a file unless you pass --force. Import crate::components::dialog::Dialog to use your copy.";

const CHARGE: &str = "Your app stays in charge";
const CHARGE_INTRO: &str = "The runtime enters your loop at exactly two call sites. Remove them and the rest of the loop is untouched.";
/// The two call sites, in the order they happen.
///
/// Read from files rather than written here, because the test module compiles
/// the same files with `include!` — see `the_snippets_on_the_page_compile`. The
/// bytes on the page and the bytes the compiler checks are therefore one thing,
/// which a parallel copy kept by hand would not be.
///
/// Two bytes diverge, and only these two: `include!` takes a single *expression*
/// and nothing more, so `render.rs` cannot carry the `;` that ends the statement
/// at every real call site. The file stores the expression — and no trailing
/// newline, so this lands the semicolon on the last line — and the page shows it
/// with the semicolon a reader would type. Displayed text is compiled text plus
/// `";\n"`: the semicolon a reader needs, and the newline that ends the line it
/// sits on. The `match` below needs no semicolon and gets none.
const RENDER_SNIPPET: &str = concat!(include_str!("../snippets/render.rs"), ";\n");
const EVENT_SNIPPET: &str = include_str!("../snippets/handle_event.rs");
const CHARGE_DOES: &str = "render declares what is on screen this frame; handle_event answers Emit, Consumed, or Ignored, and Ignored leaves the key to your own shortcuts. Components read your state and never write it; update is the only writer.";
const CHARGE_WIDGETS: &str = "Or skip the runtime: most components have a paint-only half — ButtonWidget, ListWidget and the rest are plain Ratatui widgets that take a theme and some bools — and where a component has one, it paints through that same widget.";

const NEXT: &str = "Next";
const NEXT_BODY: &str = "Demos, in the header, runs every demo in the repository. The rest of the documentation is at ratcn.kristoferlund.se, including Crossterm and browser builds through ratzilla.";

/// One block of the page, in the order it is laid out and painted.
enum Block {
    /// A bold heading, with extra air above it.
    Heading(&'static str),
    /// A paragraph in the body color.
    Body(&'static str),
    /// A paragraph in the muted color.
    Muted(&'static str),
    /// A shell transcript, in the body color: code, but nothing to color.
    Shell(&'static str),
    /// A Rust snippet, colored by [`code::highlight`].
    Rust(&'static str),
}

impl Block {
    /// Columns this block is inset from the prose column.
    const fn indent(&self) -> u16 {
        match self {
            Self::Shell(_) | Self::Rust(_) => CODE_INDENT,
            _ => 0,
        }
    }

    /// Rows of air above it, beyond the blank row every block gets.
    const fn lead(&self) -> u16 {
        match self {
            Self::Heading(_) => HEADING_LEAD,
            _ => 0,
        }
    }

    /// Rows it takes at `width`.
    ///
    /// Code is measured by counting its lines, because code is not wrapped: a
    /// snippet too wide for the column is clipped and marked. A wrapped line of
    /// code reads worse than a cut one, and the prose around each snippet
    /// carries the meaning.
    fn height(&self, width: u16) -> u16 {
        match self {
            Self::Heading(text) | Self::Body(text) | Self::Muted(text) => {
                wrapped_height(text, width)
            }
            Self::Shell(source) | Self::Rust(source) => lines(source),
        }
    }
}

/// The page.
const BLOCKS: &[Block] = &[
    Block::Heading(TITLE),
    Block::Body(LEDE),
    Block::Muted(PREVIEW),
    Block::Heading(INSTALL),
    Block::Body(INSTALL_INTRO),
    Block::Shell(INSTALL_SHELL),
    Block::Body(INIT_DOES),
    Block::Body(INIT_RUN),
    Block::Heading(COPY),
    Block::Body(COPY_INTRO),
    Block::Shell(COPY_SHELL),
    Block::Body(COPY_DOES),
    Block::Heading(CHARGE),
    Block::Body(CHARGE_INTRO),
    Block::Rust(RENDER_SNIPPET),
    Block::Rust(EVENT_SNIPPET),
    Block::Body(CHARGE_DOES),
    Block::Body(CHARGE_WIDGETS),
    Block::Heading(NEXT),
    Block::Body(NEXT_BODY),
];

/// The Getting started page, measured for one frame.
#[derive(Debug, Clone, Copy)]
pub struct Layout {
    /// The prose column's left edge inside the scroll area's content.
    column_x: u16,
    /// Its width.
    column_width: u16,
    /// How far the page scrolls and where it is scrolled to.
    pub scroll: Scroll,
}

/// Measure the page for a body of `body`, scrolled to `offset`.
#[must_use]
pub fn layout(body: Rect, offset: u16) -> Layout {
    // The scroll area always keeps a gutter column for its scrollbar, and the
    // prose keeps the landing page's measure and its cell of air beside it.
    let available = body.width.saturating_sub(1);
    let column_width = text_width(available, HERO_WIDTH);
    let column_x = centered(available, column_width);
    let (_, content) = placed(column_x, column_width);
    Layout {
        column_x,
        column_width,
        scroll: Scroll::new(content, body.height, offset),
    }
}

/// Where every block goes inside the scroll area's content, and the rows the
/// page occupies.
fn placed(column_x: u16, column_width: u16) -> (Vec<Rect>, u16) {
    let mut rects = Vec::with_capacity(BLOCKS.len());
    let mut y = TOP_PADDING;
    for block in BLOCKS {
        let indent = block.indent();
        let width = column_width.saturating_sub(indent);
        // The first block's lead would be air on top of the top padding.
        if !rects.is_empty() {
            y += block.lead();
        }
        let height = block.height(width);
        rects.push(Rect::new(column_x + indent, y, width, height));
        y += height + GAP;
    }
    (rects, y.saturating_sub(GAP) + BOTTOM_PADDING)
}

/// Lines a snippet paints, which is what [`code::highlight`] hands back for a
/// Rust one: a trailing newline is not a trailing empty line.
fn lines(source: &str) -> u16 {
    source.lines().count() as u16
}

/// Declare the page over `body`.
pub fn declare(ctx: &mut DeclareCtx<'_, AppState, Msg>, body: Rect, page: Layout) {
    let area = ScrollArea::new(page.scroll.content)
        .scroll(
            |state: &AppState| state.getting_started_scroll,
            Msg::GettingStartedScrolled,
        )
        .content(move |ctx| content(ctx, page));
    ctx.component(ID, area, body);
}

/// Paint the blocks inside the scroll area's logical content rect.
///
/// Code paints straight on the page background — no well and no tint behind
/// it. ratcn's accents are fill colors and miss the text floor on
/// `theme.field`, which is exactly why [`code::highlight`] maps onto
/// `background`.
fn content(ctx: &mut DeclareCtx<'_, AppState, Msg>, page: Layout) {
    let origin = ctx.area();
    let theme = *ctx.theme;
    let heading = Style::default()
        .fg(theme.foreground)
        .add_modifier(Modifier::BOLD);
    let body = Style::default().fg(theme.foreground);
    let muted = Style::default().fg(theme.muted_foreground);

    let (rects, _) = placed(page.column_x, page.column_width);
    for (block, rect) in BLOCKS.iter().zip(rects) {
        let rect = place(rect, origin);
        // No `Paragraph::wrap` anywhere here: prose arrives already wrapped by
        // the code that measured it, and code is meant to be clipped.
        let paragraph = match block {
            Block::Heading(text) => Paragraph::new(wrapped(text, rect.width)).style(heading),
            Block::Body(text) => Paragraph::new(wrapped(text, rect.width)).style(body),
            Block::Muted(text) => Paragraph::new(wrapped(text, rect.width)).style(muted),
            Block::Shell(source) => {
                Paragraph::new(clipped(plain(source, &theme), rect.width, &theme)).style(body)
            }
            Block::Rust(source) => {
                Paragraph::new(clipped(code::highlight(source, &theme), rect.width, &theme))
            }
        };
        ctx.paint_widget(paragraph, rect);
    }
}

/// A snippet as its own lines, uncolored.
fn plain(source: &'static str, theme: &Theme) -> Vec<Line<'static>> {
    source
        .lines()
        .map(|line| Line::styled(line, Style::default().fg(theme.foreground)))
        .collect()
}

/// Every line cut to `width`, with a marker in the last cell of any line that
/// lost something.
///
/// Without the marker a clipped snippet does not read as a cropped view of
/// valid Rust — it reads as invalid Rust. At 47 columns the opening `{` of the
/// first snippet falls off while the `});` two lines below still closes the
/// block, and nothing on screen says which of the two is the lie.
fn clipped<'a>(lines: Vec<Line<'a>>, width: u16, theme: &Theme) -> Text<'a> {
    let marker = display_width_u16(ELLIPSIS);
    Text::from(
        lines
            .into_iter()
            .map(|line| {
                if line.width() as u16 <= width {
                    return line;
                }
                let keep = usize::from(width.saturating_sub(marker));
                let mut spans: Vec<Span<'a>> = Vec::new();
                let mut used = 0usize;
                for span in line.spans {
                    let room = keep.saturating_sub(used);
                    if room == 0 {
                        break;
                    }
                    let text = truncate_to_width(&span.content, room);
                    used += display_width_u16(text) as usize;
                    let style = span.style;
                    spans.push(Span::styled(text.to_owned(), style));
                }
                spans.push(Span::styled(
                    ELLIPSIS,
                    Style::default().fg(theme.muted_foreground),
                ));
                Line::from(spans).style(line.style)
            })
            .collect::<Vec<_>>(),
    )
}

#[cfg(test)]
mod tests {
    use ratatui::Frame;
    use ratcn::{
        Button, Theme,
        runtime::{Event, EventResult, Ratcn},
    };

    use super::*;
    use crate::snapshot;

    /// The page's Rust, compiled.
    ///
    /// `include!` pulls in the same files [`RENDER_SNIPPET`] and
    /// [`EVENT_SNIPPET`] put on the page, so what a visitor reads and what the
    /// compiler checks are one set of bytes rather than two that agree today.
    /// Change `Ratcn::render`'s signature and this fails to build, instead of
    /// the front page of the SSH site serving Rust that does not compile as the
    /// first code anyone reads.
    ///
    /// The body is never run — it would want a real `Frame` and a real event —
    /// and there would be nothing to assert if it were: compiling *is* the
    /// assertion. Naming `snippets` at the end is what keeps it from being dead
    /// code, which is cheaper than an `#[expect(dead_code)]` that would itself
    /// fail the build the day the function stopped being dead.
    #[test]
    fn the_snippets_on_the_page_compile() {
        fn snippets(
            ratcn: &mut Ratcn<HarnessState, HarnessMsg>,
            frame: &mut Frame,
            // Owned, like the state and theme a reader's own loop holds — a
            // harness that passed references would make the snippets' `&state`
            // and `&theme` redundant, and clippy would ask the page to drop the
            // borrows that every real caller needs.
            mut state: HarnessState,
            theme: Theme,
            area: Rect,
            event: Event,
        ) {
            // The names the snippets use, which any app of the reader's would
            // supply for itself.
            use HarnessMsg as Msg;

            include!("../snippets/render.rs");
            include!("../snippets/handle_event.rs");
        }

        let _ = snippets;
    }

    /// The app state a snippet writes through, in the shape the page describes:
    /// `update` is the only writer.
    #[derive(Default)]
    struct HarnessState;

    impl HarnessState {
        fn update(&mut self, _msg: HarnessMsg) {}
    }

    /// Carries `Hello`, because the first snippet names it.
    enum HarnessMsg {
        Hello,
    }

    /// The height the scroll area is told is the height the blocks fill: a
    /// disagreement either clips the last paragraph or leaves dead rows under
    /// it.
    #[test]
    fn the_page_ends_exactly_where_its_height_says() {
        for width in [42, 46, 62, 80, 100, 120, 320] {
            let page = layout(Rect::new(0, 0, width, 24), 0);
            let (rects, content) = placed(page.column_x, page.column_width);
            let first = rects.first().expect("the page has blocks");
            let last = rects.last().expect("the page has blocks");

            assert_eq!(
                first.y, TOP_PADDING,
                "at width {width} the page does not start below its top padding"
            );
            for (pair, block) in rects.windows(2).zip(&BLOCKS[1..]) {
                assert_eq!(
                    pair[0].bottom() + GAP + block.lead(),
                    pair[1].y,
                    "at width {width} two blocks are not the air apart that they owe"
                );
            }
            assert_eq!(
                last.bottom() + BOTTOM_PADDING,
                content,
                "at width {width} the last block does not end where the height says"
            );
            assert_eq!(
                content, page.scroll.content,
                "at width {width} the layout reports a height its blocks do not fill"
            );
        }
    }

    /// The prose keeps a cell of air off the scrollbar gutter, and never grows
    /// past the landing page's hero column.
    #[test]
    fn the_prose_column_is_the_hero_column_with_air_beside_the_gutter() {
        let column = |width| {
            let page = layout(Rect::new(0, 0, width, 24), 0);
            (page.column_x, page.column_width)
        };

        assert_eq!(column(46).1, 46 - 1 - 2, "gutter, then a cell either side");
        assert_eq!(column(320).1, HERO_WIDTH, "capped at the hero column");
        assert_eq!(
            column(320).0,
            (320 - 1 - HERO_WIDTH) / 2,
            "and centred in what is left"
        );
    }

    /// A snippet's measured height is the height it paints, or the block below
    /// it would land on top of its last line.
    #[test]
    fn a_colored_snippet_is_as_tall_as_the_layout_reserved() {
        let theme = Theme::default_dark();
        for source in [RENDER_SNIPPET, EVENT_SNIPPET] {
            assert_eq!(
                code::highlight(source, &theme).len() as u16,
                lines(source),
                "the coloring and the measurement disagree on the line count"
            );
        }
    }

    /// A clipped line has to say it was clipped, and has to still fit: an
    /// unmarked cut reads as invalid Rust, and a marker that overflows would
    /// push the cut cell back onto the scrollbar gutter.
    #[test]
    fn a_line_too_wide_for_its_column_is_cut_and_marked() {
        let theme = Theme::default_dark();
        let lines = code::highlight(RENDER_SNIPPET, &theme);
        let widest = lines
            .iter()
            .map(|line| line.width() as u16)
            .max()
            .expect("the snippet has lines");

        for width in 8..=widest {
            let text = clipped(lines.clone(), width, &theme);
            for (row, line) in text.lines.iter().enumerate() {
                assert!(
                    line.width() as u16 <= width,
                    "at width {width} row {row} paints {} cells",
                    line.width()
                );
                let cut = lines[row].width() as u16 > width;
                assert_eq!(
                    line.spans
                        .last()
                        .is_some_and(|span| span.content == ELLIPSIS),
                    cut,
                    "at width {width} row {row} is marked when it was not cut, or not when it was"
                );
            }
        }

        let whole = clipped(lines.clone(), widest, &theme);
        assert_eq!(
            whole.lines.iter().map(Line::width).sum::<usize>(),
            lines.iter().map(Line::width).sum::<usize>(),
            "a column wide enough for every line takes nothing away"
        );
    }

    snapshot::snapshot_test! {
        /// The snippets this page ships are the first Rust most visitors read,
        /// so an edit that breaks their coloring has to fail here rather than
        /// ship.
        name: the_rust_snippets_tokenize_the_same_way_they_did,
        fixture: "src/getting_started_snapshot.txt",
        expected: include_str!("getting_started_snapshot.txt"),
        actual: snapshot::of([RENDER_SNIPPET, EVENT_SNIPPET]),
    }
}
