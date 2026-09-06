//! The Getting started view: a scrolling page of prose and code.
//!
//! The page is a list of [`Block`]s and one walk over it, [`placed`], which
//! answers where each block goes and how tall the page is. The layout, the
//! paint, and the tests all read that same walk, so the height the scroll area
//! is told and the rows the blocks land on cannot drift apart.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Text},
    widgets::Paragraph,
};
use ratcn::{ScrollArea, geometry::wrapped_height, runtime::DeclareCtx, text_width::wrap_to_width};

use crate::{AppState, Msg, code, page, scroll::Scroll};

/// The scroll area's child id. Not `getting-started`: the header link that
/// leads here already has that name, and both are declared in the root scope.
const ID: &str = "getting-started-page";

const TOP_PADDING: u16 = 2;
const BOTTOM_PADDING: u16 = 2;
/// The blank row between blocks.
const GAP: u16 = 1;
/// Code is set in from the prose, the way a fenced block is on the site.
const CODE_INDENT: u16 = 2;

const TITLE: &str = "Getting started";
const LEDE: &str = "ratcn is a component library for Ratatui: components you copy, theme, and own in your source, plus a small runtime for focus, hover, and events. It never takes over your app loop.";
const PREVIEW: &str = "Preview release. The API is unstable — pin an exact version and expect to edit when you upgrade. Twelve components today: Button, List, Select, Tabs, Dialog, ScrollArea, Checkbox, Cycle, Tooltip, Toast, BarChart, Progress.";

const INSTALL: &str = "Install";
const INSTALL_INTRO: &str = "Requires Rust 1.88. The cargo-ratcn CLI sets terminal projects up and copies components into them.";
const INSTALL_SHELL: &str =
    "cargo install cargo-ratcn\ncargo new my-app\ncd my-app\ncargo ratcn init\n";
const INIT_DOES: &str = "init adds ratcn with its termina feature and a compatible ratatui, writes ratcn.toml, and creates src/components/mod.rs. On Cargo's untouched default main.rs it also offers a starter: keep it, a minimal app, or a demo app with a button and a Hello World toast. Source you wrote yourself is never replaced.";
const INIT_RUN: &str = "Take the demo app, run cargo run, and a ratcn app is on screen.";

const COPY: &str = "Copy a component";
const COPY_INTRO: &str = "Every component module is self-contained, so you can own one outright:";
const COPY_SHELL: &str = "cargo ratcn add --list\ncargo ratcn add dialog\n";
const COPY_DOES: &str = "add copies the source from the exact ratcn package your project resolved, registers the module, and never replaces a file unless you pass --force. Import crate::components::dialog::Dialog to use your copy.";

const CHARGE: &str = "Your app stays in charge";
const CHARGE_INTRO: &str = "The runtime enters your loop at exactly two call sites. Remove them and the rest of the loop is untouched.";
/// The two call sites, in the order they happen. Compile-checked against this
/// repository's own API, and snapshotted so a change to either the API or the
/// coloring is read by a human before it ships.
const RENDER_SNIPPET: &str = "ratcn.render(frame, &state, &theme, |ctx| {
    let button = Button::new(\"Hello\")
        .on_press(|| Msg::Hello);
    ctx.component(\"hello\", button, area);
});
";
const EVENT_SNIPPET: &str = "let result = ratcn.handle_event(event, &state);
if let EventResult::Emit(msg) = result {
    state.update(msg);
}
";
const CHARGE_DOES: &str = "render declares what is on screen this frame; handle_event answers Emit, Consumed, or Ignored, and Ignored leaves the key to your own shortcuts. Components read your state and never write it; update is the only writer.";
const CHARGE_WIDGETS: &str = "Or skip the runtime entirely: ButtonWidget, ListWidget and the rest are plain Ratatui widgets that only paint, and the components paint through those very same widgets.";

const NEXT: &str = "Next";
const NEXT_BODY: &str = "Demos, in the header, runs every example in the repository. Crossterm, browser builds through ratzilla, and the rest of the documentation are at ratcn.kristoferlund.se.";

/// One block of the page, in the order it is laid out and painted.
enum Block {
    /// A bold heading.
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
    /// The prose column inside the scroll area's content: its left edge and
    /// width. The rows come from [`placed`].
    column: Rect,
    /// How far the page scrolls and where it is scrolled to.
    pub scroll: Scroll,
}

/// Measure the page for a body of `body`, scrolled to `offset`.
#[must_use]
pub fn layout(body: Rect, offset: u16) -> Layout {
    // The scroll area always keeps a gutter column for its scrollbar, and the
    // prose keeps the landing page's measure and its cell of air beside it.
    let available = body.width.saturating_sub(1);
    let width = page::text_width(available, page::HERO_WIDTH);
    let column = Rect::new(page::centered(available, width), 0, width, 0);
    let (_, content) = placed(column);
    Layout {
        column,
        scroll: Scroll::new(content, body.height, offset),
    }
}

/// Where every block goes inside the scroll area's content, and the rows the
/// page occupies.
///
/// `column` gives the prose its left edge and width; only those two of its
/// fields are read.
fn placed(column: Rect) -> (Vec<Rect>, u16) {
    let mut rects = Vec::with_capacity(BLOCKS.len());
    let mut y = TOP_PADDING;
    for block in BLOCKS {
        let indent = match block {
            Block::Shell(_) | Block::Rust(_) => CODE_INDENT,
            _ => 0,
        };
        let width = column.width.saturating_sub(indent);
        let height = height(block, width);
        rects.push(Rect::new(column.x + indent, y, width, height));
        y += height + GAP;
    }
    (rects, y.saturating_sub(GAP) + BOTTOM_PADDING)
}

/// Rows a block takes at `width`.
///
/// Code is measured by counting its lines, because code is not wrapped: a
/// snippet too wide for the column is clipped. A wrapped line of code reads
/// worse than a cut one, and the prose around each snippet carries the meaning.
fn height(block: &Block, width: u16) -> u16 {
    match block {
        Block::Heading(text) | Block::Body(text) | Block::Muted(text) => {
            wrapped_height(text, width)
        }
        Block::Shell(source) | Block::Rust(source) => lines(source),
    }
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

    let (rects, _) = placed(page.column);
    for (block, rect) in BLOCKS.iter().zip(rects) {
        let rect = page::place(rect, origin);
        // No `Paragraph::wrap` anywhere here: prose arrives already wrapped by
        // the code that measured it, and code is meant to be clipped.
        let paragraph = match block {
            Block::Heading(text) => Paragraph::new(wrapped(text, rect.width)).style(heading),
            Block::Body(text) => Paragraph::new(wrapped(text, rect.width)).style(body),
            Block::Muted(text) => Paragraph::new(wrapped(text, rect.width)).style(muted),
            Block::Shell(source) => Paragraph::new(plain(source)).style(body),
            Block::Rust(source) => Paragraph::new(code::highlight(source, &theme)),
        };
        ctx.paint_widget(paragraph, rect);
    }
}

/// `text` wrapped to `width` by the same code [`wrapped_height`] measures it
/// with, so the paint cannot need a row the layout did not reserve.
fn wrapped(text: &'static str, width: u16) -> Text<'static> {
    Text::from(
        wrap_to_width(text, usize::from(width.max(1)))
            .into_iter()
            .map(Line::from)
            .collect::<Vec<_>>(),
    )
}

/// A snippet as its own lines, uncolored.
fn plain(source: &'static str) -> Text<'static> {
    Text::from(source.lines().map(Line::raw).collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The height the scroll area is told is the height the blocks fill: a
    /// disagreement either clips the last paragraph or leaves dead rows under
    /// it.
    #[test]
    fn the_page_ends_exactly_where_its_height_says() {
        for width in [42, 46, 62, 80, 100, 120, 320] {
            let page = layout(Rect::new(0, 0, width, 24), 0);
            let (rects, content) = placed(page.column);
            let first = rects.first().expect("the page has blocks");
            let last = rects.last().expect("the page has blocks");

            assert_eq!(
                first.y, TOP_PADDING,
                "at width {width} the page does not start below its top padding"
            );
            for pair in rects.windows(2) {
                assert_eq!(
                    pair[0].bottom() + GAP,
                    pair[1].y,
                    "at width {width} two blocks are not one blank row apart"
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
        let column = |width| layout(Rect::new(0, 0, width, 24), 0).column;

        let narrow = column(46);
        assert_eq!(narrow.width, 46 - 1 - 2, "gutter, then a cell either side");
        let wide = column(320);
        assert_eq!(wide.width, page::HERO_WIDTH, "capped at the hero column");
        assert_eq!(
            wide.x,
            (320 - 1 - page::HERO_WIDTH) / 2,
            "and centred in what is left"
        );
    }

    /// Where the snapshot lives, relative to the crate root `cargo test` runs
    /// tests from.
    const SNAPSHOT: &str = "src/getting_started_snapshot.txt";

    /// The one command that repairs this fixture, named here so the next
    /// person reads the fix rather than deriving it.
    const REGENERATE: &str = "UPDATE_CODE_SNAPSHOT=1 cargo test -p showcase the_rust_snippets_tokenize_the_same_way_they_did";

    /// One `Kind:text` per line, whitespace dropped — the same shape
    /// `code.rs`'s own snapshot takes.
    fn snapshot() -> String {
        [RENDER_SNIPPET, EVENT_SNIPPET]
            .iter()
            .flat_map(|source| {
                code::tokens(source)
                    .into_iter()
                    .filter(|(_, text)| !text.trim().is_empty())
                    .map(|(kind, text)| format!("{kind:?}:{}\n", text.replace('\n', "\\n")))
            })
            .collect()
    }

    /// The snippets this page ships are the first Rust most visitors read, so
    /// an edit that breaks their coloring has to fail here rather than ship.
    ///
    /// Setting `UPDATE_CODE_SNAPSHOT` rewrites the fixture instead of comparing
    /// against it — see [`REGENERATE`] for the whole command. Rewriting is a
    /// deliberate act: the point of the test is that the diff gets read.
    #[test]
    fn the_rust_snippets_tokenize_the_same_way_they_did() {
        let actual = snapshot();
        if std::env::var_os("UPDATE_CODE_SNAPSHOT").is_some() {
            std::fs::write(SNAPSHOT, &actual).expect("the snapshot is writable");
            return;
        }

        let expected = include_str!("getting_started_snapshot.txt");
        assert_eq!(
            expected, actual,
            "the snippets' token stream changed; if that was intended, regenerate with:\n  \
             {REGENERATE}"
        );
    }

    /// A snippet's measured height is the height it paints, or the block below
    /// it would land on top of its last line.
    #[test]
    fn a_colored_snippet_is_as_tall_as_the_layout_reserved() {
        let theme = ratcn::Theme::default_dark();
        for source in [RENDER_SNIPPET, EVENT_SNIPPET] {
            assert_eq!(
                code::highlight(source, &theme).len() as u16,
                lines(source),
                "the coloring and the measurement disagree on the line count"
            );
        }
    }
}
