//! Integration tests for the public render and event-routing entry points.

use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::{Buffer, Cell},
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::Block,
};
use ratcn::runtime::{
    ChildId, Component, DeclareCtx, Event, EventResult, FocusState, KeyCode, KeyEvent, PaintCtx,
    PopupOptions, Ratcn, ScopeOptions,
};
use ratcn::{Dialog, Theme};

#[derive(Default)]
struct State {
    focus: FocusState,
    marker: u8,
}

#[derive(Debug, Clone, PartialEq)]
enum Msg {
    Focus(FocusState),
    Activated,
}

struct Probe;

impl Component<State, Msg> for Probe {
    fn declare(&mut self, ctx: &mut DeclareCtx<'_, State, Msg>) {
        assert_eq!(ctx.state().marker, 7);
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, State>) {
        assert_eq!(ctx.state().marker, 7);
        let area = ctx.area();
        ctx.with_buffer(|buf| assert!(buf.area.width >= area.width));
    }

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &State,
        _ctx: &mut ratcn::runtime::EventCtx<'_>,
    ) -> EventResult<Msg> {
        if matches!(event, Event::Key(key) if key.code == KeyCode::Enter) {
            EventResult::Emit(Msg::Activated)
        } else {
            EventResult::Ignored
        }
    }

    fn scope_options(&self) -> ScopeOptions {
        ScopeOptions::default().focusable(true)
    }
}

#[test]
fn unified_render_apis_are_usable_from_an_external_crate() {
    let state = State {
        marker: 7,
        ..State::default()
    };
    let mut ratcn = Ratcn::new().focus(|state: &State| &state.focus, Msg::Focus);
    let mut terminal = Terminal::new(TestBackend::new(40, 10)).expect("terminal");
    let theme = Theme::default_dark();

    terminal
        .draw(|frame| {
            let area = frame.area();
            ratcn.render(frame, area, &state, &theme, |ctx| {
                ctx.scope(
                    ChildId::Static("view"),
                    area,
                    ScopeOptions::default(),
                    |ctx| ctx.component(ChildId::Static("probe"), Probe, ctx.area()),
                );
                ctx.modal(
                    ChildId::Static("dialog"),
                    Dialog::<State, Msg>::new().content(1, |ctx| {
                        ctx.component(ChildId::Static("probe"), Probe, ctx.area());
                    }),
                    area,
                );
            });
        })
        .expect("draw");

    assert_eq!(
        ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(Msg::Activated)
    );
}

fn layered_surface(ctx: &mut DeclareCtx<'_, State, Msg>) {
    let area = ctx.area();
    ctx.paint_widget(
        Block::new().style(Style::new().fg(Color::Yellow).bg(Color::Blue)),
        area,
    );
    ctx.modal_scope("modal", area, ScopeOptions::default(), |ctx| {
        ctx.viewport(Rect::new(area.x + 2, area.y + 2, 12, 2), 8, 3, |ctx| {
            let row = Rect::new(area.x + 2, area.y + 5, 12, 1);
            ctx.component("probe", Probe, row);
            ctx.paint_widget(Line::from("scrolled"), row);
            ctx.popup(
                "popup",
                Rect::new(area.x + 2, area.y + 7, 12, 1),
                PopupOptions::default(),
                |ctx| ctx.paint_widget(Line::from("popup"), ctx.area()),
            );
        });
        ctx.defer_paint(move |ctx| {
            ctx.widget(
                Line::from("modal"),
                Rect::new(area.x + 18, area.y + 8, 8, 1),
            );
        });
    });
    ctx.defer_paint(move |ctx| {
        ctx.widget(Line::from("root"), Rect::new(area.x, area.y, 4, 1));
    });
}

#[test]
fn offscreen_rendering_matches_frame_cells_and_retains_the_same_interactive_tree() {
    let state = State {
        marker: 7,
        ..State::default()
    };
    let theme = Theme::default_dark();
    let area = Rect::new(4, 2, 30, 12);
    let mut direct = Buffer::filled(Rect::new(0, 0, 40, 18), Cell::new("#"));
    let mut frame_runtime = Ratcn::new();
    let mut buffer_runtime = Ratcn::new();
    let mut terminal = Terminal::new(TestBackend::new(40, 18)).expect("terminal");

    terminal
        .draw(|frame| {
            *frame.buffer_mut() = direct.clone();
            frame_runtime.render(frame, area, &state, &theme, layered_surface);
            buffer_runtime.render_into(&mut direct, area, &state, &theme, layered_surface);
            assert_eq!(frame.buffer_mut(), &direct);
        })
        .expect("draw");

    // Both paths must actually composite, project the viewport, and flush each layer.
    assert_eq!(direct[(6, 4)].symbol(), "s");
    assert_eq!(direct[(6, 6)].symbol(), "p");
    assert_eq!(direct[(22, 10)].symbol(), "m");
    assert_eq!(direct[(4, 2)].symbol(), "r");
    assert_ne!(
        direct[(33, 3)].bg,
        Color::Blue,
        "the modal backdrop must dim"
    );
    assert_eq!(direct[(0, 0)].symbol(), "#");
    for runtime in [&mut frame_runtime, &mut buffer_runtime] {
        assert_eq!(
            runtime.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(Msg::Activated),
            "offscreen rendering still commits a surface that routes input"
        );
    }
}

#[test]
fn a_tall_offset_buffer_keeps_below_fold_content_without_clearing_untouched_cells() {
    let allocation = Rect::new(7, 11, 24, 200);
    let area = Rect::new(9, 13, 20, 196);
    let mut buffer = Buffer::filled(allocation, Cell::new("#"));
    let mut runtime = Ratcn::<(), ()>::new();
    runtime.render_into(&mut buffer, area, &(), &Theme::default_dark(), |ctx| {
        assert_eq!(ctx.area(), area);
        assert_eq!(ctx.frame_area(), area);
        ctx.paint_widget(Line::from("Top"), Rect::new(area.x, area.y, area.width, 1));
        ctx.paint_widget(
            Line::from("Below fold"),
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
        );
    });

    let bottom: String = (area.x..area.right())
        .map(|x| buffer[(x, area.bottom() - 1)].symbol())
        .collect();
    assert!(bottom.starts_with("Below fold"), "{bottom}");
    assert_eq!(buffer[(9, 13)].symbol(), "T");
    assert_eq!(buffer.area, allocation, "the caller owns the allocation");
    assert_eq!(
        buffer[(9, 100)].symbol(),
        "#",
        "rendering does not clear the page"
    );
    assert_eq!(buffer[(7, 11)].symbol(), "#");
}

#[test]
fn rendering_into_a_frame_buffer_releases_the_borrow_for_host_widgets() {
    let mut terminal = Terminal::new(TestBackend::new(16, 6)).expect("terminal");
    let mut runtime = Ratcn::<(), ()>::new();
    terminal
        .draw(|frame| {
            runtime.render_into(
                frame.buffer_mut(),
                Rect::new(3, 2, 8, 1),
                &(),
                &Theme::default_dark(),
                |ctx| ctx.paint_widget(Line::from("tree"), ctx.area()),
            );
            frame.render_widget(Line::from("host"), Rect::new(3, 3, 8, 1));
        })
        .expect("draw");

    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(3, 2)].symbol(), "t");
    assert_eq!(buffer[(3, 3)].symbol(), "h");
}
