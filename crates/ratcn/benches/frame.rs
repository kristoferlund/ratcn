//! Baseline for the two costs a frame carries: declaring and painting the whole
//! component surface, and routing one click against the surface the last frame
//! left behind.

#[cfg(not(target_arch = "wasm32"))]
mod frame {
    use std::hint::black_box;

    use criterion::{Criterion, criterion_group};
    use ratatui::{Terminal, backend::TestBackend, layout::Rect};
    use ratcn::{
        Button, Theme,
        runtime::{
            Event, FocusState, Modifiers, MouseButton, MouseEvent, MouseKind, Ratcn, RenderCtx,
            ScopeOptions,
        },
    };

    const SCOPE_IDS: [&str; 4] = ["s0", "s1", "s2", "s3"];
    const BUTTON_IDS: [&str; 25] = [
        "b0", "b1", "b2", "b3", "b4", "b5", "b6", "b7", "b8", "b9", "b10", "b11", "b12", "b13",
        "b14", "b15", "b16", "b17", "b18", "b19", "b20", "b21", "b22", "b23", "b24",
    ];
    const SCOPE_WIDTH: u16 = 12;
    const WIDTH: u16 = 48;
    const HEIGHT: u16 = 25;

    /// A cell inside the thirteenth button of the second scope, so routing has
    /// to walk past most of the surface before it hits.
    const CLICK_COLUMN: u16 = SCOPE_WIDTH + 1;
    const CLICK_ROW: u16 = 12;

    struct State {
        focus: FocusState,
    }

    enum Msg {
        /// The runtime insists on a way to hand focus back; the benches route
        /// events but never apply the messages, so the payload stays unread.
        #[expect(dead_code, reason = "constructed by the focus binding, never applied")]
        Focus(FocusState),
        Press,
    }

    fn declare(ctx: &mut RenderCtx<'_, '_, State, Msg>) {
        for (index, scope_id) in (0u16..).zip(SCOPE_IDS) {
            let x = index * SCOPE_WIDTH;
            ctx.scope(
                scope_id,
                Rect::new(x, 0, SCOPE_WIDTH, HEIGHT),
                ScopeOptions::default(),
                |ctx| {
                    for (row, button_id) in (0u16..).zip(BUTTON_IDS) {
                        ctx.render_component(
                            button_id,
                            Button::new("Go").on_press(|| Msg::Press),
                            Rect::new(x, row, SCOPE_WIDTH, 1),
                        );
                    }
                },
            );
        }
    }

    fn surface() -> (Ratcn<State, Msg>, Terminal<TestBackend>, State, Theme) {
        (
            Ratcn::new().focus(|state: &State| &state.focus, Msg::Focus),
            Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("terminal"),
            State {
                focus: FocusState::intent(["s1", "b12"]),
            },
            Theme::default_dark(),
        )
    }

    fn mouse(kind: MouseKind) -> MouseEvent {
        MouseEvent {
            kind,
            column: CLICK_COLUMN,
            row: CLICK_ROW,
            modifiers: Modifiers::NONE,
        }
    }

    fn render(c: &mut Criterion) {
        let (mut ratcn, mut terminal, state, theme) = surface();
        c.bench_function("render", |b| {
            b.iter(|| {
                terminal
                    .draw(|frame| ratcn.render(frame, &state, &theme, declare))
                    .expect("draw");
            });
        });
    }

    fn route_click(c: &mut Criterion) {
        let (mut ratcn, mut terminal, state, theme) = surface();
        terminal
            .draw(|frame| ratcn.render(frame, &state, &theme, declare))
            .expect("draw");
        let press = mouse(MouseKind::Down(MouseButton::Left));
        let release = mouse(MouseKind::Up(MouseButton::Left));

        c.bench_function("route_click", |b| {
            b.iter(|| {
                black_box(ratcn.handle_event(Event::Mouse(press), &state));
                black_box(ratcn.handle_event(Event::Mouse(release), &state));
            });
        });
    }

    criterion_group!(benches, render, route_click);
}

#[cfg(not(target_arch = "wasm32"))]
criterion::criterion_main!(frame::benches);

#[cfg(target_arch = "wasm32")]
fn main() {}
