//! Baseline for the costs a frame carries: declaring and painting the whole
//! component surface, routing one click against the surface the last frame left
//! behind, drawing a long list through a viewport that shows a handful of its
//! rows, and wrapping a paragraph of prose to the width of a box.

#[cfg(not(target_arch = "wasm32"))]
mod frame {
    use std::hint::black_box;

    use criterion::{Criterion, criterion_group};
    use ratatui::{Terminal, backend::TestBackend, layout::Rect};
    use ratcn::{
        Button, Dialog, List, ListItem, Theme,
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

    /// A thousand items against a fifteen-row viewport: the frame paints about
    /// one percent of the list, so anything the declaration does per item shows
    /// up here and nowhere else.
    const LIST_LEN: usize = 1000;
    const LIST_HEIGHT: u16 = 15;
    /// Far enough down that the painted window starts nowhere near the first
    /// item, so a windowed declaration cannot pass by starting at zero.
    const LIST_CURSOR: usize = 500;

    /// Prose that wraps into a dozen rows, wrapped twice per frame: the box
    /// measures the description to size itself, then paints it at that size.
    /// Every component that wraps or truncates text goes through the same
    /// measurement, so its cost per cell shows up here.
    const DESCRIPTION: &str = "Deleting this workspace removes every board, card, and attachment \
        it holds, for everyone who can see it. Members lose access the moment the deletion lands, \
        and nothing here can be restored afterwards, so make sure the exports finished first.";

    struct State {
        focus: FocusState,
        cursor: Option<usize>,
    }

    enum Msg {
        /// The runtime insists on a way to hand focus back; the benches route
        /// events but never apply the messages, so the payload stays unread.
        #[expect(dead_code, reason = "constructed by the focus binding, never applied")]
        Focus(FocusState),
        #[expect(
            dead_code,
            reason = "constructed by the item-focus binding, never applied"
        )]
        Cursor(usize, usize),
        Press,
    }

    fn declare(ctx: &mut RenderCtx<'_, State, Msg>) {
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
                cursor: Some(LIST_CURSOR),
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

    fn render_list_1000(c: &mut Criterion) {
        let (mut ratcn, _, state, theme) = surface();
        let mut terminal = Terminal::new(TestBackend::new(WIDTH, LIST_HEIGHT)).expect("terminal");
        let items: Vec<ListItem<usize>> = (0..LIST_LEN)
            .map(|index| ListItem::new(index, format!("Item {index}")))
            .collect();

        c.bench_function("render_list_1000", |b| {
            b.iter(|| {
                terminal
                    .draw(|frame| {
                        let area = frame.area();
                        ratcn.render(frame, &state, &theme, |ctx| {
                            ctx.render_component(
                                "list",
                                List::new(items.clone())
                                    .item_focus(|state: &State| state.cursor, Msg::Cursor),
                                area,
                            );
                        });
                    })
                    .expect("draw");
            });
        });
    }

    fn render_dialog_wrapped(c: &mut Criterion) {
        let (mut ratcn, mut terminal, state, theme) = surface();

        c.bench_function("render_dialog_wrapped", |b| {
            b.iter(|| {
                terminal
                    .draw(|frame| {
                        let area = frame.area();
                        ratcn.render(frame, &state, &theme, |ctx| {
                            ctx.render_component(
                                "dialog",
                                Dialog::new()
                                    .title("Delete workspace")
                                    .description(DESCRIPTION),
                                area,
                            );
                        });
                    })
                    .expect("draw");
            });
        });
    }

    criterion_group!(
        benches,
        render,
        route_click,
        render_list_1000,
        render_dialog_wrapped
    );
}

#[cfg(not(target_arch = "wasm32"))]
criterion::criterion_main!(frame::benches);

#[cfg(target_arch = "wasm32")]
fn main() {}
