//! What the pointer is on, and how a redraw or a motion moves it.

use super::*;

#[derive(Default)]
struct ModalPointerState {
    focus: FocusState,
    modals: ModalState,
}

struct ModalHoverLeaf {
    rendered: FocusRenderLog,
}

impl Component<ModalPointerState, PointerMsg> for ModalHoverLeaf {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, ModalPointerState, PointerMsg>) {}

    fn paint(&mut self, ctx: &mut PaintCtx<'_, ModalPointerState>) {
        self.rendered
            .borrow_mut()
            .push((ctx.hovered(), ctx.contains_hover()));
    }
}

struct ModalPointerLeaf;

impl Component<ModalPointerState, PointerMsg> for ModalPointerLeaf {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, ModalPointerState, PointerMsg>) {}
}

struct EmittingHoverLeaf;

impl Component<PointerState, PointerMsg> for EmittingHoverLeaf {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, PointerState, PointerMsg>) {}

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &PointerState,
        _ctx: &mut EventCtx<'_>,
    ) -> EventResult<PointerMsg> {
        if matches!(
            event,
            Event::Mouse(MouseEvent {
                kind: MouseKind::Moved,
                ..
            })
        ) {
            EventResult::Emit(PointerMsg::Routed("move", MouseKind::Moved, 1))
        } else {
            EventResult::Ignored
        }
    }
}

/// Claims the pointer on its press and reports the hover flags it paints
/// with, so a test can watch what a frame does to a capturing node's
/// hover.
struct CapturingHoverLeaf {
    rendered: FocusRenderLog,
}

impl Component<PointerState, PointerMsg> for CapturingHoverLeaf {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, PointerState, PointerMsg>) {}

    fn paint(&mut self, ctx: &mut PaintCtx<'_, PointerState>) {
        self.rendered
            .borrow_mut()
            .push((ctx.hovered(), ctx.contains_hover()));
    }

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &PointerState,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<PointerMsg> {
        match event {
            Event::Mouse(mouse) if mouse.kind == MouseKind::Down(MouseButton::Left) => {
                ctx.capture_pointer(MouseButton::Left);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}

#[test]
fn root_then_nested_hover_focus_attract_on_successive_moves() {
    let mut state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("left"), ChildId::Static("first")]),
    };
    let mut driver = Driver::with(
        Ratcn::new()
            .focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus)
            .hover_focus(),
        12,
        2,
    );
    driver.render(&state, |ctx| {
        for (scope, x) in [("left", 0), ("right", 6)] {
            ctx.scope(
                ChildId::Static(scope),
                Rect::new(x, 0, 6, 2),
                ScopeOptions::default().hover_focus(),
                |ctx| {
                    ctx.component(
                        ChildId::Static("first"),
                        FocusLeaf::enabled(),
                        Rect::new(x, 0, 3, 2),
                    );
                    ctx.component(
                        ChildId::Static("second"),
                        FocusLeaf::enabled(),
                        Rect::new(x + 3, 0, 3, 2),
                    );
                },
            );
        }
    });

    let EventResult::Emit(FocusTestMsg::Focus(root_focus)) =
        driver.event(mouse(MouseKind::Moved, 10, 0), &state)
    else {
        panic!("root boundary did not attract focus");
    };
    assert_eq!(
        root_focus.path(),
        &[ChildId::Static("right"), ChildId::Static("first")]
    );
    // The same motion did both: hover is the runtime's own, so it lands
    // whole while the one message the event may carry is the focus change.
    assert_eq!(
        driver.ratcn.hover_path(),
        [ChildId::Static("right"), ChildId::Static("second")]
    );
    state.focus = root_focus;

    let EventResult::Emit(FocusTestMsg::Focus(nested_focus)) =
        driver.event(mouse(MouseKind::Moved, 10, 0), &state)
    else {
        panic!("nested boundary did not attract focus after the root");
    };
    assert_eq!(
        nested_focus.path(),
        &[ChildId::Static("right"), ChildId::Static("second")]
    );
    state.focus = nested_focus;

    // Both boundaries satisfied and hover already there: the motion still
    // reports the redraw signal, but it asks for nothing.
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 10, 0), &state),
        EventResult::Consumed
    );
}

#[test]
fn hover_focus_is_off_by_default_and_skips_disabled_targets_and_empty_space() {
    let state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("enabled")]),
    };
    let mut default = Driver::with(
        Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus),
        12,
        2,
    );
    let mut hover_focus = Driver::with(
        Ratcn::new()
            .focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus)
            .hover_focus(),
        12,
        2,
    );
    let render = |driver: &mut Driver<FocusTestState, FocusTestMsg>| {
        driver.render(&state, |ctx| {
            ctx.component(
                ChildId::Static("enabled"),
                FocusLeaf::enabled(),
                Rect::new(0, 0, 3, 2),
            );
            ctx.component(
                ChildId::Static("disabled"),
                FocusLeaf::disabled(),
                Rect::new(4, 0, 3, 2),
            );
        });
    };

    render(&mut default);
    assert_eq!(
        default.event(mouse(MouseKind::Moved, 5, 0), &state),
        EventResult::Consumed,
        "without `hover_focus` a motion moves nothing but hover"
    );
    assert_eq!(default.ratcn.hover_path(), [ChildId::Static("disabled")]);

    render(&mut hover_focus);
    assert_eq!(
        hover_focus.event(mouse(MouseKind::Moved, 5, 0), &state),
        EventResult::Consumed,
        "a disabled target hovers without attracting focus"
    );
    assert_eq!(
        hover_focus.ratcn.hover_path(),
        [ChildId::Static("disabled")]
    );
    assert_eq!(
        hover_focus.event(mouse(MouseKind::Moved, 10, 0), &state),
        EventResult::Consumed,
        "empty space attracts no focus either"
    );
    assert!(
        hover_focus.ratcn.hover_path().is_empty(),
        "and hover returns to nothing over empty space"
    );
}

/// The motion contract: with a surface to route against, a motion is
/// never `Ignored`. Crossing into a component, drifting within one, and
/// leaving for empty space all report `Consumed` — the host redraws on
/// anything but `Ignored`, and every motion is news to a frame that may
/// paint from the pointer position itself.
#[test]
fn hover_crosses_consumes_same_target_and_clears_over_empty_space() {
    let state = PointerState;
    let mut driver = Driver::<PointerState, PointerMsg>::new(10, 2);
    driver.render(&state, |ctx| {
        for (id, x) in [("left", 0), ("right", 5)] {
            ctx.component(
                ChildId::Static(id),
                HoverLeaf {
                    consume_move: false,
                    rendered: None,
                },
                Rect::new(x, 0, 4, 2),
            );
        }
    });

    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 1, 0), &state),
        EventResult::Consumed,
        "the first crossing moved hover"
    );
    assert_eq!(
        driver.ratcn.hover_path(),
        [ChildId::Static("left")],
        "the crossing put hover on the target it entered"
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 2, 1), &state),
        EventResult::Consumed,
        "motion within one target moves no hover, and is still the redraw signal"
    );
    assert_eq!(
        driver.ratcn.hover_path(),
        [ChildId::Static("left")],
        "drifting inside `left` leaves hover where it was"
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 6, 0), &state),
        EventResult::Consumed,
        "crossing to the other target"
    );
    assert_eq!(driver.ratcn.hover_path(), [ChildId::Static("right")]);
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 4, 0), &state),
        EventResult::Consumed,
        "leaving for empty space is a change like any other"
    );
    assert!(
        driver.ratcn.hover_path().is_empty(),
        "over empty space the pointer is on nothing"
    );
}

/// `pointer_within` answers for the declaration that asks, not for the
/// pointer in general: true on what the pointer is on and on everything
/// enclosing it, false on a sibling. The root closure has no declaration
/// of its own, so it asks whether anything is hovered at all.
#[test]
fn pointer_within_answers_for_the_asking_declaration() {
    let state = PointerState;
    let log: Rc<RefCell<Vec<(&'static str, bool)>>> = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Driver::<PointerState, PointerMsg>::new(10, 2);
    let render = |driver: &mut Driver<PointerState, PointerMsg>| {
        driver.render(&state, |ctx| {
            log.borrow_mut().push(("root", ctx.pointer_within()));
            for (id, x) in [("left", 0), ("right", 5)] {
                let log = Rc::clone(&log);
                let area = Rect::new(x, 0, 5, 2);
                ctx.scope(
                    ChildId::Static(id),
                    area,
                    ScopeOptions::default(),
                    move |ctx| {
                        log.borrow_mut().push((id, ctx.pointer_within()));
                        ctx.component(
                            ChildId::Static("leaf"),
                            HoverLeaf {
                                consume_move: false,
                                rendered: None,
                            },
                            area,
                        );
                    },
                );
            }
        });
    };

    render(&mut driver);
    assert_eq!(
        *log.borrow(),
        [("root", false), ("left", false), ("right", false)],
        "nothing is hovered before the pointer has been anywhere"
    );

    driver.event(mouse(MouseKind::Moved, 1, 0), &state);
    log.borrow_mut().clear();
    render(&mut driver);
    assert_eq!(
        *log.borrow(),
        [("root", true), ("left", true), ("right", false)],
        "the pointer is on `left`'s leaf, so only that subtree contains it"
    );
}

/// Hover freezes for the length of a claimed gesture. A component that
/// captured the pointer owns it, and the geometry it drags moves under a
/// pointer that is by definition on it, so neither the drag events nor the
/// frames they produce may retarget hover. The release hands it back.
#[test]
fn a_captured_gesture_freezes_hover_until_it_ends() {
    let state = PointerState;
    let mut driver = Driver::<PointerState, PointerMsg>::new(20, 4);
    let surface = [
        ("left", "left", Rect::new(0, 0, 4, 2)),
        ("right", "right", Rect::new(10, 0, 4, 2)),
    ];

    render_drag_surface(&mut driver, &state, &surface);
    driver.event(mouse(MouseKind::Moved, 1, 1), &state);
    assert_eq!(driver.ratcn.hover_path(), [ChildId::Static("left")]);

    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
    assert!(
        driver
            .ratcn
            .gestures
            .capture_path(MouseButton::Left)
            .is_some()
    );
    driver.event(mouse(MouseKind::Moved, 11, 1), &state);
    assert_eq!(
        driver.ratcn.hover_path(),
        [ChildId::Static("left")],
        "the drag belongs to the component that claimed it"
    );
    render_drag_surface(&mut driver, &state, &surface);
    assert_eq!(
        driver.ratcn.hover_path(),
        [ChildId::Static("left")],
        "and a redraw mid-gesture does not retarget it either"
    );

    driver.event(mouse(MouseKind::Up(MouseButton::Left), 11, 1), &state);
    render_drag_surface(&mut driver, &state, &surface);
    assert_eq!(
        driver.ratcn.hover_path(),
        [ChildId::Static("right")],
        "the release ends the claim, and the next frame answers for the pointer again"
    );
}

/// The freeze is one rule for every gesture, not just claimed ones. A
/// press that captured nothing still owns the pointer until it is
/// released — motion under a held button normalizes to `Drag` and never
/// writes hover — so a redraw that moves geometry out from under that
/// pointer must not retarget hover either. The release hands it back.
#[test]
fn a_held_press_freezes_hover_against_a_moving_redraw() {
    let state = PointerState;
    let mut driver = Driver::<PointerState, PointerMsg>::new(10, 2);
    let render = |driver: &mut Driver<PointerState, PointerMsg>, x| {
        driver.render(&state, |ctx| {
            ctx.component(
                ChildId::Static("target"),
                HoverLeaf {
                    consume_move: false,
                    rendered: None,
                },
                Rect::new(x, 0, 2, 1),
            );
        });
    };

    render(&mut driver, 0);
    driver.event(mouse(MouseKind::Moved, 1, 0), &state);
    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state);
    assert!(
        driver
            .ratcn
            .gestures
            .capture_path(MouseButton::Left)
            .is_none(),
        "nothing claimed this gesture — the freeze is not about captures"
    );

    render(&mut driver, 5);
    assert_eq!(
        driver.ratcn.hover_path(),
        [ChildId::Static("target")],
        "the press owns the pointer, so the redraw does not retarget hover"
    );

    driver.event(mouse(MouseKind::Up(MouseButton::Left), 1, 0), &state);
    render(&mut driver, 5);
    assert!(
        driver.ratcn.hover_path().is_empty(),
        "the release ends the gesture, and the pointer is over empty space"
    );
}

/// A freeze lasts only as long as its target could still be under the
/// pointer. The frame that opens a modal cancels the gesture beneath it,
/// and that same frame must paint the captured node unhovered — a
/// gesture whose target is covered has no claim on hover left.
#[test]
fn a_frame_that_cancels_a_gesture_unhovers_its_target() {
    let rendered = Rc::new(RefCell::new(Vec::new()));
    let state = PointerState;
    let mut driver = Driver::<PointerState, PointerMsg>::new(10, 2);
    let render = |driver: &mut Driver<PointerState, PointerMsg>, modal| {
        let area = driver.area();
        driver.render(&state, |ctx| {
            ctx.component(
                ChildId::Static("grip"),
                CapturingHoverLeaf {
                    rendered: Rc::clone(&rendered),
                },
                Rect::new(0, 0, 4, 2),
            );
            if modal {
                ctx.modal(ChildId::Static("modal"), RouteLeaf("modal"), area);
            }
        });
    };

    render(&mut driver, false);
    driver.event(mouse(MouseKind::Moved, 1, 0), &state);
    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state);
    assert!(
        driver
            .ratcn
            .gestures
            .capture_path(MouseButton::Left)
            .is_some()
    );
    driver.event(mouse(MouseKind::Drag(MouseButton::Left), 8, 1), &state);
    render(&mut driver, false);
    assert_eq!(
        rendered.borrow().last(),
        Some(&(true, true)),
        "the drag keeps hover on the node that claimed it"
    );

    render(&mut driver, true);
    assert_eq!(
        rendered.borrow().last(),
        Some(&(false, false)),
        "the modal cancels the gesture and covers its target, on this frame"
    );
    assert_ne!(driver.ratcn.hover_path(), [ChildId::Static("grip")]);
}

#[test]
fn pointer_exit_clears_hover() {
    let state = PointerState;
    let mut driver = Driver::<PointerState, PointerMsg>::new(4, 2);
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(
            ChildId::Static("target"),
            HoverLeaf {
                consume_move: false,
                rendered: None,
            },
            area,
        );
    });

    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 1, 0), &state),
        EventResult::Consumed
    );
    assert_eq!(driver.ratcn.hover_path(), [ChildId::Static("target")]);
    assert_eq!(
        driver.event(mouse(MouseKind::Exited, 1, 0), &state),
        EventResult::Consumed
    );
    assert!(
        driver.ratcn.hover_path().is_empty(),
        "a pointer that left the grid is on nothing"
    );
}

/// An exit that arrives while the modal stacks disagree still empties
/// hover, and no later commit brings it back: the pointer is gone, so
/// there is nothing for the recompute to find it on.
#[test]
fn pointer_exit_during_modal_mismatch_keeps_hover_empty() {
    let rendered = Rc::new(RefCell::new(Vec::new()));
    let mut state = ModalPointerState::default();
    let mut driver = Driver::with(
        Ratcn::new().modals(|state: &ModalPointerState| &state.modals),
        5,
        2,
    );
    let render = |driver: &mut Driver<ModalPointerState, PointerMsg>, state: &ModalPointerState| {
        let area = driver.area();
        driver.render(state, |ctx| {
            ctx.component(
                ChildId::Static("base"),
                ModalHoverLeaf {
                    rendered: Rc::clone(&rendered),
                },
                area,
            );
            if state.modals.is_open("modal") {
                ctx.modal(ChildId::Static("modal"), ModalPointerLeaf, area);
            }
        });
    };

    render(&mut driver, &state);
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 1, 0), &state),
        EventResult::Consumed
    );
    render(&mut driver, &state);
    assert_eq!(
        rendered.borrow().last(),
        Some(&(true, true)),
        "the pointer is on the base"
    );

    state
        .modals
        .open("modal", &mut state.focus)
        .expect("open modal");
    assert_eq!(
        driver.event(mouse(MouseKind::Exited, 1, 0), &state),
        EventResult::Consumed
    );
    render(&mut driver, &state);
    let _ = state.modals.close(&mut state.focus);
    render(&mut driver, &state);
    render(&mut driver, &state);
    assert_eq!(
        rendered.borrow().last(),
        Some(&(false, false)),
        "the pointer left, and closing the modal does not put it back"
    );

    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 1, 0), &state),
        EventResult::Consumed
    );
    render(&mut driver, &state);
    assert_eq!(
        rendered.borrow().last(),
        Some(&(true, true)),
        "the pointer coming back is what restores hover"
    );
}

/// A redraw that slides the target out from under a pointer that never
/// moved: the commit re-answers the hit test, so the very next frame
/// paints unhovered without any event being involved.
#[test]
fn redraw_moves_hover_when_the_target_moves_away_from_the_pointer() {
    let rendered = Rc::new(RefCell::new(Vec::new()));
    let state = PointerState;
    let mut driver = Driver::<PointerState, PointerMsg>::new(10, 2);
    let render = |driver: &mut Driver<PointerState, PointerMsg>, state: &PointerState, x| {
        driver.render(state, |ctx| {
            ctx.component(
                ChildId::Static("target"),
                HoverLeaf {
                    consume_move: false,
                    rendered: Some(Rc::clone(&rendered)),
                },
                Rect::new(x, 0, 2, 1),
            );
        });
    };

    render(&mut driver, &state, 0);
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 1, 0), &state),
        EventResult::Consumed,
        "entering the target"
    );
    render(&mut driver, &state, 0);
    render(&mut driver, &state, 5);

    assert_eq!(
        *rendered.borrow(),
        [(false, false), (true, true), (false, false)],
        "the frame that moves the target is already the frame that unhovers it"
    );
    assert!(
        driver.ratcn.hover_path().is_empty(),
        "the correction needed no motion"
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 1, 0), &state),
        EventResult::Consumed,
        "a later motion is still the redraw signal, with nothing left to correct"
    );
    assert!(driver.ratcn.hover_path().is_empty());
}

/// The hover change a crossing causes does not swallow the motion: the
/// same event moves hover *and* goes on to the component under it.
#[test]
fn crossing_motion_moves_hover_and_still_reaches_a_consuming_component() {
    let state = PointerState;
    let mut driver = Driver::<PointerState, PointerMsg>::new(5, 2);
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(
            ChildId::Static("consumer"),
            HoverLeaf {
                consume_move: true,
                rendered: None,
            },
            area,
        );
    });

    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 0, 0), &state),
        EventResult::Consumed,
        "the crossing motion reached the consuming component"
    );
    assert_eq!(
        driver.ratcn.hover_path(),
        [ChildId::Static("consumer")],
        "the crossing motion moved hover"
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 1, 0), &state),
        EventResult::Consumed,
        "same-path motion must reach the consuming component"
    );
}

#[test]
fn crossing_motion_moves_hover_and_still_reaches_an_emitting_component() {
    let state = PointerState;
    let mut driver = Driver::<PointerState, PointerMsg>::new(5, 2);
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.scope(
            ChildId::Static("panel"),
            area,
            ScopeOptions::default(),
            |ctx| {
                ctx.component(ChildId::Static("emitter"), EmittingHoverLeaf, area);
            },
        );
    });

    // The message the component returns is the event's answer; hover
    // landing alongside it costs nothing, because it needs no message.
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 0, 0), &state),
        EventResult::Emit(PointerMsg::Routed("move", MouseKind::Moved, 1))
    );
    assert_eq!(
        driver.ratcn.hover_path(),
        [ChildId::Static("panel"), ChildId::Static("emitter")]
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 1, 0), &state),
        EventResult::Emit(PointerMsg::Routed("move", MouseKind::Moved, 1))
    );
}

/// What a redraw does to hover with the pointer sitting still: a failed
/// pass changes nothing (the previous surface is still what the pointer is
/// on), a pass that drops the target unhovers, and one that declares it
/// again under the same pointer hovers it again — all without an event.
#[test]
fn removed_target_unhovers_and_a_redeclared_one_hovers_again_under_the_pointer() {
    let rendered = Rc::new(RefCell::new(Vec::new()));
    let state = PointerState;
    let mut driver = Driver::<PointerState, PointerMsg>::new(5, 2);
    let render_target = |driver: &mut Driver<PointerState, PointerMsg>| {
        let area = driver.area();
        driver.render(&state, |ctx| {
            ctx.component(
                ChildId::Static("target"),
                HoverLeaf {
                    consume_move: false,
                    rendered: Some(Rc::clone(&rendered)),
                },
                area,
            );
        });
    };

    render_target(&mut driver);
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 4, 1), &state),
        EventResult::Consumed
    );
    render_target(&mut driver);

    let failed_removal = catch_unwind(AssertUnwindSafe(|| {
        driver.render(&state, |_| panic!("failed removal"));
    }));
    assert!(failed_removal.is_err());
    assert_eq!(
        driver.ratcn.hover_path(),
        [ChildId::Static("target")],
        "a rejected pass leaves the surface the pointer is on in place"
    );
    render_target(&mut driver);

    driver.render(&state, |_| {});
    assert!(
        driver.ratcn.hover_path().is_empty(),
        "the target is gone, so the pointer is on nothing"
    );
    render_target(&mut driver);

    // One entry per successful render; the failed pass painted nothing and
    // so recorded nothing. The last is the redeclared target, hovered
    // again by the commit that declared it — no event was involved.
    assert_eq!(
        *rendered.borrow(),
        vec![(false, false), (true, true), (true, true), (true, true)]
    );
    assert_eq!(driver.ratcn.hover_path(), [ChildId::Static("target")]);
}

/// Removal settles at the commit, so no later event carries a correction:
/// the first motion after it has nothing to change and says so.
#[test]
fn motion_after_a_removal_has_nothing_left_to_correct() {
    let state = PointerState;
    let mut driver = Driver::<PointerState, PointerMsg>::new(5, 2);

    let render_target = |driver: &mut Driver<PointerState, PointerMsg>, state: &PointerState| {
        driver.render(state, |ctx| {
            ctx.component(
                ChildId::Static("target"),
                HoverLeaf {
                    consume_move: false,
                    rendered: None,
                },
                Rect::new(0, 0, 2, 1),
            );
        });
    };

    render_target(&mut driver, &state);
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 1, 0), &state),
        EventResult::Consumed,
        "entering the target"
    );
    driver.render(&state, |_| {});
    assert!(
        driver.ratcn.hover_path().is_empty(),
        "the removal emptied hover at the commit"
    );

    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 4, 1), &state),
        EventResult::Consumed,
        "the motion is the redraw signal, but it carries no correction"
    );
    render_target(&mut driver, &state);
    assert!(
        driver.ratcn.hover_path().is_empty(),
        "and the pointer is no longer over the redeclared target"
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 1, 0), &state),
        EventResult::Consumed
    );
    assert_eq!(driver.ratcn.hover_path(), [ChildId::Static("target")]);
}

/// The two hover flags mean different things, and a container relies on
/// the difference: `hovered` is "the pointer is on *me*", `contains_hover`
/// is "the pointer is somewhere inside me".
///
/// They agree everywhere except on an ancestor of the hovered leaf, which
/// is why the ancestor has to be watched alongside the leaf — either flag
/// collapsing into the other is invisible from the leaf alone.
#[test]
fn hovered_and_contains_hover_are_distinct_at_the_leaf_and_its_ancestor() {
    let state = PointerState;
    let scope_flags = Rc::new(RefCell::new(Vec::new()));
    let leaf_flags = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Driver::<PointerState, PointerMsg>::new(6, 1);
    let render = |driver: &mut Driver<PointerState, PointerMsg>| {
        let area = driver.area();
        driver.render(&state, |ctx| {
            let scope_flags = Rc::clone(&scope_flags);
            let leaf_flags = Rc::clone(&leaf_flags);
            ctx.scope(
                ChildId::Static("pane"),
                area,
                ScopeOptions::default(),
                move |ctx| {
                    ctx.paint(move |ctx| {
                        scope_flags
                            .borrow_mut()
                            .push((ctx.hovered(), ctx.contains_hover()));
                    });
                    ctx.component(
                        ChildId::Static("leaf"),
                        HoverLeaf {
                            consume_move: false,
                            rendered: Some(leaf_flags),
                        },
                        area,
                    );
                },
            );
        });
    };

    render(&mut driver);
    driver.event(mouse(MouseKind::Moved, 1, 0), &state);
    leaf_flags.borrow_mut().clear();
    scope_flags.borrow_mut().clear();
    render(&mut driver);

    assert_eq!(
        *leaf_flags.borrow(),
        [(true, true)],
        "the pointer is on the leaf, so both halves hold there"
    );
    assert_eq!(
        *scope_flags.borrow(),
        [(false, true)],
        "the scope contains the pointer without being under it"
    );
}

/// A backend in button-event mode reports no motion before a press, so the
/// press is the first word on where the pointer is: hover follows it to the
/// pressed node, and the gesture then holds it there.
#[test]
fn a_press_with_no_motion_before_it_moves_hover_to_the_pressed_node() {
    let state = PointerState;
    let mut driver = Driver::<PointerState, PointerMsg>::new(10, 1);
    let render = |driver: &mut Driver<PointerState, PointerMsg>| {
        driver.render(&state, |ctx| {
            ctx.component(ChildId::Static("a"), RouteLeaf("a"), Rect::new(0, 0, 5, 1));
            ctx.component(ChildId::Static("b"), RouteLeaf("b"), Rect::new(5, 0, 5, 1));
        });
    };
    render(&mut driver);
    driver.event(mouse(MouseKind::Moved, 1, 0), &state);
    assert_eq!(driver.ratcn.hover_path(), [ChildId::Static("a")]);

    driver.event(mouse(MouseKind::Down(MouseButton::Left), 7, 0), &state);
    assert_eq!(driver.ratcn.hover_path(), [ChildId::Static("b")]);
    render(&mut driver);
    assert_eq!(
        driver.ratcn.hover_path(),
        [ChildId::Static("b")],
        "the held press freezes hover on what it pressed, not on what was hovered before"
    );
}
