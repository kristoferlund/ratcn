//! Integration tests for the public render and event-routing entry points.

use ratatui::{Terminal, backend::TestBackend};
use ratcn::runtime::{
    ChildId, Component, DeclareCtx, Event, EventResult, FocusState, KeyCode, KeyEvent, PaintCtx,
    Ratcn, ScopeOptions,
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
            ratcn.render(frame, &state, &theme, |ctx| {
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
