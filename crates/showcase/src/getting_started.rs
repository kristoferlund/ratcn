//! One Markdown document, rendered as a paragraph inside the page's ScrollArea.

use ratatui::{
    layout::Rect,
    style::Style,
    widgets::{Paragraph, Wrap},
};
use ratcn::{ScrollArea, runtime::DeclareCtx};

use crate::{
    AppState, Msg,
    page_geometry::{BOTTOM_PADDING, HERO_WIDTH, Scroll, TOP_PADDING, centered, place, text_width},
};

const CONTENT: &str = include_str!("../getting_started.md");

pub struct Layout {
    column: Rect,
    paragraph: Paragraph<'static>,
    pub scroll: Scroll,
}

pub fn layout(body: Rect, offset: u16) -> Layout {
    let available = body.width.saturating_sub(1);
    let width = text_width(available, HERO_WIDTH);
    let paragraph = Paragraph::new(tui_markdown::from_str(CONTENT)).wrap(Wrap { trim: false });
    // Use the paragraph's own wrapping for both its height and its paint.
    let height = paragraph.line_count(width) as u16;
    Layout {
        column: Rect::new(centered(available, width), TOP_PADDING, width, height),
        paragraph,
        scroll: Scroll::new(TOP_PADDING + height + BOTTOM_PADDING, body.height, offset),
    }
}

pub fn declare(ctx: &mut DeclareCtx<'_, AppState, Msg>, body: Rect, page: Layout) {
    let area = ScrollArea::new(page.scroll.content)
        .scroll(
            |state: &AppState| state.getting_started_scroll,
            Msg::GettingStartedScrolled,
        )
        .content(move |ctx| {
            ctx.paint_widget(
                page.paragraph
                    .style(Style::default().fg(ctx.theme.foreground)),
                place(page.column, ctx.area()),
            );
        });
    ctx.component("getting-started-page", area, body);
}
