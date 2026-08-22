//! Popup and hint layers: what they cover, what dismisses them, and where
//! their events go.

use super::*;

fn render_popup_over_leaf(
    driver: &mut Driver<PointerState, PointerMsg>,
    state: &PointerState,
    with_dismiss: bool,
) {
    let area = driver.area();
    driver.render(state, |ctx| {
        ctx.component(ChildId::Static("under"), RouteLeaf("under"), area);
        let options = if with_dismiss {
            PopupOptions::default().on_dismiss(|| PointerMsg::Dismissed)
        } else {
            PopupOptions::default()
        };
        // The popup covers the left half; its content is passive
        // paint, so presses inside it reach nothing interactive.
        ctx.popup(
            ChildId::Static("panel"),
            Rect::new(0, 0, 5, 2),
            options,
            |_| {},
        );
    });
}

#[test]
fn popup_occludes_its_footprint_and_leaves_the_rest_clickable() {
    let state = PointerState;
    let mut driver = Driver::<PointerState, PointerMsg>::new(10, 2);
    render_popup_over_leaf(&mut driver, &state, false);

    // Inside the popup's footprint, the occluded leaf must never see the
    // press: the popup's content ignored it, so it is consumed at the
    // popup boundary rather than falling through.
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state),
        EventResult::Consumed
    );
    // Outside the footprint, the leaf is visibly there and stays live.
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 7, 0), &state),
        EventResult::Emit(PointerMsg::Routed(
            "under",
            MouseKind::Down(MouseButton::Left),
            0
        ))
    );
}

/// Modal policy is about the modal's subtree, not about declaration
/// order: a popup declared after the modal but outside it is still
/// covered, so presses on it are consumed rather than routed.
#[test]
fn a_popup_declared_after_a_modal_is_still_covered_by_it() {
    let state = PointerState;
    let mut driver = Driver::<PointerState, PointerMsg>::new(10, 4);
    driver.render(&state, |ctx| {
        ctx.modal(
            ChildId::Static("dlg"),
            RouteLeaf("dlg"),
            Rect::new(0, 2, 10, 2),
        );
        // Declared later, so it takes a higher layer number — but
        // it is a sibling of the modal, not inside it.
        ctx.popup(
            ChildId::Static("panel"),
            Rect::new(0, 0, 5, 1),
            PopupOptions::default(),
            |ctx| {
                ctx.component(
                    ChildId::Static("pi"),
                    RouteLeaf("pi"),
                    Rect::new(0, 0, 5, 1),
                );
            },
        );
    });

    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state),
        EventResult::Consumed,
        "the modal covers it, so the press must not reach the popup's content"
    );
}

/// A hint layer paints above everything and takes nothing: a press over it
/// reaches the control underneath, and its content cannot be focused *even
/// when that content claims it is focusable*. The claim has to be refused
/// by the layer policy rather than by what happens to be declared inside,
/// so the content here is deliberately focusable.
#[test]
fn a_hint_layer_is_inert_to_the_pointer_and_to_focus() {
    struct FocusableLeaf;

    impl Component<PointerState, PointerMsg> for FocusableLeaf {
        fn declare(&mut self, _ctx: &mut DeclareCtx<'_, PointerState, PointerMsg>) {}

        fn scope_options(&self) -> ScopeOptions {
            ScopeOptions::default().focusable(true)
        }
    }

    let state = PointerState;
    let mut driver = Driver::<PointerState, PointerMsg>::new(10, 3);
    driver.render(&state, |ctx| {
        ctx.component(
            ChildId::Static("button"),
            RouteLeaf("button"),
            Rect::new(0, 0, 6, 1),
        );
        // Covers the button completely.
        ctx.hint(
            ChildId::Static("tip"),
            Rect::new(0, 0, 6, 1),
            ScopeOptions::default(),
            |ctx| {
                ctx.component(
                    ChildId::Static("text"),
                    FocusableLeaf,
                    Rect::new(0, 0, 6, 1),
                );
            },
        );
    });

    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state),
        EventResult::Emit(PointerMsg::Routed(
            "button",
            MouseKind::Down(MouseButton::Left),
            0
        )),
        "the press passes through the hint to the control it describes"
    );
    assert_eq!(
        driver
            .ratcn
            .focus_path(&[ChildId::Static("tip"), ChildId::Static("text")]),
        None,
        "a focusable component inside a hint is still not a focus target"
    );
}

/// Sibling popups each dismiss when a press lands outside them — including
/// a press that lands inside the other one. "Outside" is a containment
/// question, not a comparison of layer numbers.
#[test]
fn a_press_inside_one_popup_dismisses_its_sibling() {
    let state = PointerState;
    let mut driver = Driver::<PointerState, PointerMsg>::new(12, 4);
    driver.render(&state, |ctx| {
        ctx.popup(
            ChildId::Static("first"),
            Rect::new(0, 0, 5, 1),
            PopupOptions::default().on_dismiss(|| PointerMsg::Routed("first", MouseKind::Moved, 0)),
            |_| {},
        );
        ctx.popup(
            ChildId::Static("second"),
            Rect::new(6, 2, 5, 1),
            PopupOptions::default()
                .on_dismiss(|| PointerMsg::Routed("second", MouseKind::Moved, 0)),
            |_| {},
        );
    });

    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 7, 2), &state),
        EventResult::Emit(PointerMsg::Routed("first", MouseKind::Moved, 0)),
        "the press is inside `second` and outside `first`, so `first` dismisses"
    );
}

/// With popups nested inside one another, a press outside dismisses the
/// innermost — the one on top — not the one it is nested in.
#[test]
fn an_outside_press_dismisses_the_innermost_nested_popup() {
    let state = PointerState;
    let mut driver = Driver::<PointerState, PointerMsg>::new(10, 4);
    driver.render(&state, |ctx| {
        ctx.popup(
            ChildId::Static("outer"),
            Rect::new(0, 0, 5, 2),
            PopupOptions::default().on_dismiss(|| PointerMsg::Routed("outer", MouseKind::Moved, 0)),
            |ctx| {
                ctx.popup(
                    ChildId::Static("inner"),
                    Rect::new(0, 0, 3, 1),
                    PopupOptions::default()
                        .on_dismiss(|| PointerMsg::Routed("inner", MouseKind::Moved, 0)),
                    |_| {},
                );
            },
        );
    });

    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 9, 3), &state),
        EventResult::Emit(PointerMsg::Routed("inner", MouseKind::Moved, 0)),
        "the innermost popup is the topmost, so it is what a press outside dismisses"
    );
}

#[test]
fn outside_press_emits_the_dismiss_hook_only_when_routing_stayed_silent() {
    let state = PointerState;
    let mut driver = Driver::<PointerState, PointerMsg>::new(10, 4);
    driver.render(&state, |ctx| {
        ctx.component(
            ChildId::Static("button"),
            RouteLeaf("button"),
            Rect::new(6, 0, 4, 1),
        );
        ctx.popup(
            ChildId::Static("panel"),
            Rect::new(0, 0, 5, 2),
            PopupOptions::default().on_dismiss(|| PointerMsg::Dismissed),
            |_| {},
        );
    });

    // A press on inert space outside the popup: nothing routed, so the
    // dismiss hook speaks.
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 8, 3), &state),
        EventResult::Emit(PointerMsg::Dismissed)
    );
    // A press on a control outside the popup: the control's own message
    // wins — the app treats it as the dismissal signal, and the click
    // that follows activates as usual.
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 7, 0), &state),
        EventResult::Emit(PointerMsg::Routed(
            "button",
            MouseKind::Down(MouseButton::Left),
            0
        ))
    );
    // A press inside the popup dismisses nothing.
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state),
        EventResult::Consumed
    );
}

struct PopupHost;

impl Component<FocusTestState, FocusTestMsg> for PopupHost {
    fn declare(&mut self, ctx: &mut DeclareCtx<'_, FocusTestState, FocusTestMsg>) {
        let area = ctx.area();
        ctx.popup(
            ChildId::Static("panel"),
            area,
            PopupOptions::default(),
            |ctx| {
                let area = ctx.area();
                ctx.component(ChildId::Static("item"), FocusLeaf::enabled(), area);
            },
        );
    }

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &FocusTestState,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<FocusTestMsg> {
        if matches!(event, Event::Key(key) if key.code == KeyCode::Esc) {
            EventResult::Emit(FocusTestMsg::Parent(ctx.path().to_vec()))
        } else {
            EventResult::Ignored
        }
    }
}

#[test]
fn keys_bubble_through_the_popup_root_to_the_declaring_component() {
    // Focus sits on the popup's item; Esc is not handled inside the
    // panel, crosses the popup root, and reaches the component that
    // opened the popup — the Select-closes-on-Esc pattern.
    let state = FocusTestState {
        focus: FocusState::intent([
            ChildId::Static("host"),
            ChildId::Static("panel"),
            ChildId::Static("item"),
        ]),
    };
    let mut driver = focus_driver(10, 3);
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(ChildId::Static("host"), PopupHost, area);
    });

    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(FocusTestMsg::Activated(vec![
            ChildId::Static("host"),
            ChildId::Static("panel"),
            ChildId::Static("item"),
        ]))
    );
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Esc)), &state),
        EventResult::Emit(FocusTestMsg::Parent(vec![ChildId::Static("host")]))
    );
}

#[test]
fn popup_inside_a_modal_sits_above_it_and_routes() {
    let state = PointerState;
    let mut driver = Driver::<PointerState, PointerMsg>::new(10, 2);
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(ChildId::Static("base"), RouteLeaf("base"), area);
        ctx.modal_scope(
            ChildId::Static("sheet"),
            area,
            ScopeOptions::default(),
            move |ctx| {
                ctx.component(
                    ChildId::Static("field"),
                    RouteLeaf("field"),
                    Rect::new(5, 0, 5, 2),
                );
                ctx.popup(
                    ChildId::Static("panel"),
                    Rect::new(0, 0, 5, 2),
                    PopupOptions::default(),
                    |ctx| {
                        ctx.component(
                            ChildId::Static("option"),
                            RouteLeaf("option"),
                            Rect::new(0, 0, 5, 2),
                        );
                    },
                );
            },
        );
    });

    // The popup, declared inside the modal, is above the modal floor and
    // receives its own hits.
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state),
        EventResult::Emit(PointerMsg::Routed(
            "option",
            MouseKind::Down(MouseButton::Left),
            0
        ))
    );
    // The modal's own content stays interactive beside the popup.
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 7, 0), &state),
        EventResult::Emit(PointerMsg::Routed(
            "field",
            MouseKind::Down(MouseButton::Left),
            0
        ))
    );
}

#[test]
fn popup_paint_composites_above_later_declared_base_siblings() {
    let state = PointerState;
    let mut driver = Driver::<PointerState, PointerMsg>::new(4, 1);
    let area = driver.area();
    driver.render(&state, |ctx| {
        // The popup paints first in declaration order…
        ctx.popup(
            ChildId::Static("panel"),
            Rect::new(0, 0, 2, 1),
            PopupOptions::default(),
            |ctx| {
                ctx.paint(|ctx| {
                    ctx.widget(ratatui::text::Line::from("PP"), Rect::new(0, 0, 2, 1));
                });
            },
        );
        // …and a base sibling paints over the same cells after —
        // yet the popup composites on top.
        ctx.paint(move |ctx| {
            ctx.widget(ratatui::text::Line::from("BBBB"), area);
        });
    });

    let buffer = driver.buffer();
    assert_eq!(buffer.cell((0, 0)).expect("cell").symbol(), "P");
    assert_eq!(buffer.cell((1, 0)).expect("cell").symbol(), "P");
    assert_eq!(buffer.cell((2, 0)).expect("cell").symbol(), "B");
}

struct ClickLeaf(&'static str);

impl Component<PointerState, PointerMsg> for ClickLeaf {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, PointerState, PointerMsg>) {}

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &PointerState,
        _ctx: &mut EventCtx<'_>,
    ) -> EventResult<PointerMsg> {
        match event {
            Event::Mouse(mouse) if matches!(mouse.kind, MouseKind::Click(_)) => {
                EventResult::Emit(PointerMsg::Routed(self.0, mouse.kind, 0))
            }
            _ => EventResult::Ignored,
        }
    }
}

#[test]
fn one_physical_click_dismisses_the_popup_and_presses_the_button() {
    // The full click-through sequence: the press dismisses (Down), the
    // app closes the popup and redraws, and the release's Click still
    // presses the control the press landed on — the press target
    // survives the popup-closing redraw.
    let state = PointerState;
    let mut driver = Driver::<PointerState, PointerMsg>::new(10, 2);
    let render = |driver: &mut Driver<PointerState, PointerMsg>, popup_open: bool| {
        driver.render(&PointerState, |ctx| {
            ctx.component(
                ChildId::Static("button"),
                ClickLeaf("button"),
                Rect::new(6, 0, 4, 1),
            );
            if popup_open {
                ctx.popup(
                    ChildId::Static("panel"),
                    Rect::new(0, 0, 5, 2),
                    PopupOptions::default().on_dismiss(|| PointerMsg::Dismissed),
                    |_| {},
                );
            }
        });
    };

    render(&mut driver, true);
    // Press on the visible button: the button ignores Down, so the
    // dismiss hook speaks for the press.
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 7, 0), &state),
        EventResult::Emit(PointerMsg::Dismissed)
    );
    // The app closes the popup and redraws.
    render(&mut driver, false);
    // The release completes the same physical click on the button.
    assert_eq!(
        driver.event(mouse(MouseKind::Up(MouseButton::Left), 7, 0), &state),
        EventResult::Emit(PointerMsg::Routed(
            "button",
            MouseKind::Click(MouseButton::Left),
            0
        ))
    );
}
