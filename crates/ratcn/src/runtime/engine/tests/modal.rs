//! The modal stack: what it absorbs, where focus goes inside it, and what a
//! mismatch with the app's stack does.

use super::*;

struct ModalRoute(&'static str);

impl Component<ModalTestState, ModalTestMsg> for ModalRoute {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, ModalTestState, ModalTestMsg>) {}

    fn handle_event(
        &mut self,
        _event: &Event,
        _state: &ModalTestState,
        _ctx: &mut EventCtx<'_>,
    ) -> EventResult<ModalTestMsg> {
        EventResult::Emit(ModalTestMsg::Routed(self.0))
    }

    fn is_focusable(&self) -> bool {
        true
    }
}

struct ModalFocusRoute {
    rendered: FocusRenderLog,
}

impl Component<ModalTestState, ModalTestMsg> for ModalFocusRoute {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, ModalTestState, ModalTestMsg>) {}

    fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, ModalTestState>) {
        self.rendered
            .borrow_mut()
            .push((ctx.focused, ctx.contains_focus));
    }

    fn handle_event(
        &mut self,
        _event: &Event,
        _state: &ModalTestState,
        _ctx: &mut EventCtx<'_>,
    ) -> EventResult<ModalTestMsg> {
        EventResult::Emit(ModalTestMsg::Routed("dialog"))
    }

    fn is_focusable(&self) -> bool {
        true
    }
}

struct FocusModal;

impl Component<FocusTestState, FocusTestMsg> for FocusModal {
    fn declare(&mut self, ctx: &mut DeclareCtx<'_, FocusTestState, FocusTestMsg>) {
        let area = ctx.area();
        ctx.component(ChildId::Static("leaf"), FocusLeaf::enabled(), area);
    }

    fn scope_options(&self) -> ScopeOptions {
        ScopeOptions::default().tab_wrap(TabWrap::Wrap)
    }
}

struct EscapeFocusModal;

impl Component<FocusTestState, FocusTestMsg> for EscapeFocusModal {
    fn declare(&mut self, ctx: &mut DeclareCtx<'_, FocusTestState, FocusTestMsg>) {
        let area = ctx.area();
        ctx.component(ChildId::Static("first"), FocusLeaf::enabled(), area);
        ctx.component(ChildId::Static("second"), FocusLeaf::enabled(), area);
    }

    fn scope_options(&self) -> ScopeOptions {
        ScopeOptions::default().tab_wrap(TabWrap::Escape)
    }
}

struct PanickingFocusComponent;

impl Component<FocusTestState, FocusTestMsg> for PanickingFocusComponent {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, FocusTestState, FocusTestMsg>) {
        panic!("modal render failed");
    }
}

struct RecordingFocusModal {
    rendered: Rc<RefCell<Vec<(bool, bool)>>>,
}

impl Component<FocusTestState, FocusTestMsg> for RecordingFocusModal {
    fn declare(&mut self, ctx: &mut DeclareCtx<'_, FocusTestState, FocusTestMsg>) {
        let area = ctx.area();
        ctx.component(
            ChildId::Static("leaf"),
            FocusLeaf::recording(Rc::clone(&self.rendered)),
            area,
        );
    }

    fn scope_options(&self) -> ScopeOptions {
        ScopeOptions::default()
    }
}

#[test]
fn later_modal_focus_intent_marks_only_its_descendant_focused() {
    let state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("top")]),
    };
    let lower = Rc::new(RefCell::new(Vec::new()));
    let top = Rc::new(RefCell::new(Vec::new()));
    let mut driver = focus_driver(5, 2);
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.modal(
            ChildId::Static("lower"),
            RecordingFocusModal {
                rendered: Rc::clone(&lower),
            },
            area,
        );
        ctx.modal(
            ChildId::Static("top"),
            RecordingFocusModal {
                rendered: Rc::clone(&top),
            },
            area,
        );
    });

    assert_eq!(*lower.borrow(), [(false, false)]);
    assert_eq!(*top.borrow(), [(true, true)]);
}

#[test]
fn top_modal_alone_receives_and_absorbs_keyboard_input() {
    let state = FocusTestState::default();
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut driver = focus_driver(5, 2);
    let area = driver.area();
    driver.render(&state, |ctx| {
        for (id, name) in [("lower", "lower"), ("top", "top")] {
            ctx.modal(
                ChildId::Static(id),
                LoggingComponent {
                    name,
                    log: Rc::clone(&log),
                    focusable: true,
                },
                area,
            );
        }
    });

    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(FocusTestMsg::Activated(vec![ChildId::Static("top")]))
    );
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Char('z'))), &state),
        EventResult::Emit(FocusTestMsg::Activated(vec![ChildId::Static("top")]))
    );
}

#[test]
fn tab_from_base_focus_enters_and_cannot_escape_the_active_modal() {
    let state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("base")]),
    };
    let mut driver = focus_driver(5, 2);
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(ChildId::Static("base"), FocusLeaf::enabled(), area);
        ctx.modal(ChildId::Static("dialog"), FocusModal, area);
    });

    let expected = FocusState::intent([ChildId::Static("dialog"), ChildId::Static("leaf")]);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(FocusTestMsg::Activated(expected.path().to_vec()))
    );
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
        EventResult::Consumed
    );
}

#[test]
fn app_restores_exact_base_focus_and_can_restore_an_absent_parked_path() {
    let mut state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("base")]),
    };
    let mut driver = focus_driver(5, 2);
    let saved = state.focus.clone();
    state.focus = FocusState::intent([ChildId::Static("dialog")]);
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(ChildId::Static("base"), FocusLeaf::enabled(), area);
        ctx.modal(ChildId::Static("dialog"), FocusModal, area);
    });

    state.focus = saved;
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(ChildId::Static("base"), FocusLeaf::enabled(), area);
    });
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(FocusTestMsg::Activated(vec![ChildId::Static("base")]))
    );

    state.focus = FocusState::intent([ChildId::Static("temporarily-absent")]);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Ignored
    );
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
        EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([ChildId::Static(
            "base"
        )])))
    );
}

#[test]
fn app_owned_focus_selects_each_edge_of_a_nested_modal_stack() {
    let mut state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("top")]),
    };
    let mut driver = focus_driver(5, 2);
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.modal(ChildId::Static("lower"), FocusModal, area);
        ctx.modal(ChildId::Static("top"), FocusModal, area);
    });
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(FocusTestMsg::Activated(vec![
            ChildId::Static("top"),
            ChildId::Static("leaf"),
        ]))
    );

    state.focus = FocusState::intent([ChildId::Static("lower")]);
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.modal(ChildId::Static("lower"), FocusModal, area);
    });
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(FocusTestMsg::Activated(vec![
            ChildId::Static("lower"),
            ChildId::Static("leaf"),
        ]))
    );
}

#[test]
fn app_owned_nested_modal_history_restores_each_exact_focus_path() {
    let base = FocusState::intent([ChildId::Static("base"), ChildId::Static("base-child")]);
    let lower = FocusState::intent([ChildId::Static("lower"), ChildId::Static("leaf")]);
    let top = FocusState::intent([ChildId::Static("top"), ChildId::Static("leaf")]);
    let mut state = FocusTestState {
        focus: base.clone(),
    };
    let mut focus_history = Vec::new();
    let mut driver = focus_driver(5, 2);
    let render = |driver: &mut Driver<FocusTestState, FocusTestMsg>,
                  state: &FocusTestState,
                  lower_open,
                  top_open| {
        let area = driver.area();
        driver.render(state, |ctx| {
            ctx.scope(
                ChildId::Static("base"),
                Rect::ZERO,
                ScopeOptions::default(),
                |ctx| {
                    ctx.component(ChildId::Static("base-child"), FocusLeaf::enabled(), area);
                },
            );
            if lower_open {
                ctx.modal(ChildId::Static("lower"), FocusModal, area);
            }
            if top_open {
                ctx.modal(ChildId::Static("top"), FocusModal, area);
            }
        });
    };

    focus_history.push(state.focus.clone());
    state.focus = lower.clone();
    render(&mut driver, &state, true, false);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(FocusTestMsg::Activated(lower.path().to_vec()))
    );

    focus_history.push(state.focus.clone());
    state.focus = top.clone();
    render(&mut driver, &state, true, true);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(FocusTestMsg::Activated(top.path().to_vec()))
    );

    state.focus = focus_history.pop().expect("lower modal focus history");
    assert_eq!(state.focus, lower);
    render(&mut driver, &state, true, false);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(FocusTestMsg::Activated(lower.path().to_vec()))
    );

    state.focus = focus_history.pop().expect("base focus history");
    assert_eq!(state.focus, base);
    render(&mut driver, &state, false, false);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(FocusTestMsg::Activated(base.path().to_vec()))
    );
    assert!(focus_history.is_empty());
}

#[test]
fn modal_binding_parks_absent_focus_while_fallback_still_routes() {
    // Focus parked on a path this surface never declared stays parked —
    // absent paths are never silently retargeted, bound modal or not.
    // Interaction is not lost: keys nothing owns fall back to the modal
    // root, and paint shows no false focus.
    let mut state = ModalTestState::default();
    state
        .modals
        .open("dialog", &mut state.focus)
        .expect("open dialog");
    state.focus = FocusState::intent([ChildId::Static("gone")]);
    let rendered = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Driver::with(
        Ratcn::new()
            .focus(|state: &ModalTestState| &state.focus, |_| unreachable!())
            .modals(|state: &ModalTestState| &state.modals),
        5,
        2,
    );

    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.modal(
            ChildId::Static("dialog"),
            ModalFocusRoute {
                rendered: Rc::clone(&rendered),
            },
            area,
        );
    });

    assert_eq!(*rendered.borrow(), [(false, false)]);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(ModalTestMsg::Routed("dialog"))
    );
}

#[test]
fn modal_binding_suppresses_opening_and_closing_gaps_then_routes_after_sync() {
    let mut state = ModalTestState::default();
    let mut driver = Driver::with(
        Ratcn::new()
            .focus(|state: &ModalTestState| &state.focus, |_| unreachable!())
            .modals(|state: &ModalTestState| &state.modals),
        5,
        2,
    );
    let render = |driver: &mut Driver<ModalTestState, ModalTestMsg>, state: &ModalTestState| {
        let area = driver.area();
        driver.render(state, |ctx| {
            ctx.component(ChildId::Static("base"), ModalRoute("base"), area);
            if state.modals.is_open("dialog") {
                ctx.modal(ChildId::Static("dialog"), ModalRoute("dialog"), area);
            }
        });
    };

    state
        .modals
        .open("dialog", &mut state.focus)
        .expect("open before first render");
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Ignored,
        "there is no retained surface to protect before the first render"
    );
    let _ = state.modals.close(&mut state.focus);
    render(&mut driver, &state);
    state
        .modals
        .open("dialog", &mut state.focus)
        .expect("open dialog");
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Consumed,
        "opening gap must not reach the retained base"
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 0, 0), &state),
        EventResult::Consumed
    );

    render(&mut driver, &state);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(ModalTestMsg::Routed("dialog"))
    );

    let _ = state.modals.close(&mut state.focus);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Consumed,
        "closing gap must not reach the retained modal"
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Up(MouseButton::Left), 0, 0), &state),
        EventResult::Consumed
    );

    render(&mut driver, &state);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(ModalTestMsg::Routed("base"))
    );
}

#[test]
fn modal_binding_mismatch_preserves_the_previous_surface_atomically() {
    let mut state = ModalTestState::default();
    let mut driver = Driver::with(
        Ratcn::new()
            .focus(|state: &ModalTestState| &state.focus, |_| unreachable!())
            .modals(|state: &ModalTestState| &state.modals),
        5,
        2,
    );
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(ChildId::Static("base"), ModalRoute("base"), area);
    });

    state
        .modals
        .open("expected", &mut state.focus)
        .expect("open expected modal");
    let failed = catch_unwind(AssertUnwindSafe(|| {
        let area = driver.area();
        driver.render(&state, |ctx| {
            ctx.modal(ChildId::Static("wrong"), ModalRoute("wrong"), area);
        });
    }));

    assert!(failed.is_err());
    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![vec![ChildId::Static("base")]]
    );
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Consumed
    );

    let _ = state.modals.close(&mut state.focus);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(ModalTestMsg::Routed("base"))
    );
}

#[test]
fn modal_scope_confines_events_and_focuses_its_children() {
    #[derive(Debug, Default)]
    struct State {
        focus: FocusState,
    }

    #[derive(Debug, Clone, PartialEq)]
    enum Msg {
        Focus(FocusState),
        Base,
        Ok,
    }

    let state = State::default();
    let mut driver = Driver::with(
        Ratcn::new().focus(|state: &State| &state.focus, Msg::Focus),
        20,
        6,
    );
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(
            ChildId::Static("base"),
            crate::Button::new("Base").on_press(|| Msg::Base),
            Rect::new(0, 0, 10, 1),
        );
        ctx.modal_scope(
            ChildId::Static("sheet"),
            area,
            ScopeOptions::default(),
            |ctx| {
                ctx.component(
                    ChildId::Static("ok"),
                    crate::Button::new("OK").on_press(|| Msg::Ok),
                    Rect::new(2, 3, 6, 1),
                );
            },
        );
    });

    // Startup focus descends into the modal scope, not the base layer.
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(Msg::Ok)
    );

    // A click where the base button sits is absorbed by the modal layer.
    let click = |kind| {
        Event::Mouse(MouseEvent {
            kind,
            column: 1,
            row: 0,
            modifiers: Modifiers::NONE,
        })
    };
    assert_eq!(
        driver.event(click(MouseKind::Down(MouseButton::Left)), &state),
        EventResult::Consumed
    );
    assert_eq!(
        driver.event(click(MouseKind::Up(MouseButton::Left)), &state),
        EventResult::Consumed
    );

    // A key nothing inside handles is absorbed, not leaked to the base UI.
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Esc)), &state),
        EventResult::Consumed
    );
}

fn render_bound_nested_modal(
    driver: &mut Driver<ModalTestState, ModalTestMsg>,
    state: &ModalTestState,
    rendered: &FocusRenderLog,
) {
    let area = driver.area();
    driver.render(state, |ctx| {
        let rendered = Rc::clone(rendered);
        ctx.scope(
            ChildId::Static("pane"),
            area,
            ScopeOptions::default(),
            move |ctx| {
                ctx.modal_scope(
                    ChildId::Static("sheet"),
                    area,
                    ScopeOptions::default(),
                    move |ctx| {
                        let area = ctx.area();
                        ctx.component(
                            ChildId::Static("inner"),
                            ModalFocusLeaf {
                                rendered: Rc::clone(&rendered),
                            },
                            area,
                        );
                    },
                );
            },
        );
    });
}

#[test]
fn bound_nested_modal_keeps_valid_in_modal_focus() {
    // With `Ratcn::modals` bound and the modal declared from a nested
    // scope, both focus shapes the app can hold must resolve into the
    // modal: the bare-id intent `ModalState::open` records, and a full
    // in-modal path from a later focus message. Alignment keys on the
    // declared root — never on a root-level path shape.
    let mut state = ModalTestState::default();
    let mut focus = state.focus.clone();
    state
        .modals
        .open(ChildId::Static("sheet"), &mut focus)
        .expect("open modal");
    state.focus = focus;
    let rendered = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Driver::with(
        Ratcn::new()
            .focus(|state: &ModalTestState| &state.focus, ModalTestMsg::Focus)
            .modals(|state: &ModalTestState| &state.modals),
        20,
        6,
    );

    // Frame one: the `[sheet]` open intent descends into the nested
    // modal's first focusable leaf.
    render_bound_nested_modal(&mut driver, &state, &rendered);
    assert_eq!(*rendered.borrow(), [(true, true)]);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(ModalTestMsg::Routed("inner"))
    );

    // Frame two: a stored full in-modal path survives alignment intact.
    state.focus = FocusState::intent([
        ChildId::Static("pane"),
        ChildId::Static("sheet"),
        ChildId::Static("inner"),
    ]);
    rendered.borrow_mut().clear();
    render_bound_nested_modal(&mut driver, &state, &rendered);
    assert_eq!(*rendered.borrow(), [(true, true)]);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(ModalTestMsg::Routed("inner"))
    );
}

struct ModalFocusLeaf {
    rendered: FocusRenderLog,
}

impl Component<ModalTestState, ModalTestMsg> for ModalFocusLeaf {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, ModalTestState, ModalTestMsg>) {}

    fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, ModalTestState>) {
        self.rendered
            .borrow_mut()
            .push((ctx.focused, ctx.contains_focus));
    }

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &ModalTestState,
        _ctx: &mut EventCtx<'_>,
    ) -> EventResult<ModalTestMsg> {
        if matches!(event, Event::Key(key) if key.code == KeyCode::Enter) {
            EventResult::Emit(ModalTestMsg::Routed("inner"))
        } else {
            EventResult::Ignored
        }
    }

    fn is_focusable(&self) -> bool {
        true
    }
}

#[test]
fn nested_modal_scope_behaves_like_a_root_declared_one() {
    // A modal declared from inside a component subtree anchors its
    // identity there but carries full modal policy: focus resolves into
    // it, keys nothing inside handles are consumed at its root, and the
    // base layer stops receiving events.
    let state = FocusTestState::default();
    let rendered = Rc::new(RefCell::new(Vec::new()));
    let mut driver = focus_driver(20, 6);

    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(ChildId::Static("base"), FocusLeaf::enabled(), area);
        let rendered = Rc::clone(&rendered);
        ctx.scope(
            ChildId::Static("pane"),
            area,
            ScopeOptions::default(),
            move |ctx| {
                ctx.modal_scope(
                    ChildId::Static("sheet"),
                    area,
                    ScopeOptions::default(),
                    move |ctx| {
                        let area = ctx.area();
                        ctx.component(
                            ChildId::Static("inner"),
                            FocusLeaf::recording(rendered),
                            area,
                        );
                    },
                );
            },
        );
    });

    // Startup focus resolves into the nested modal, full path intact.
    assert_eq!(*rendered.borrow(), [(true, true)]);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(FocusTestMsg::Activated(vec![
            ChildId::Static("pane"),
            ChildId::Static("sheet"),
            ChildId::Static("inner"),
        ]))
    );
    // A key nothing inside handles is consumed at the modal boundary, not
    // delivered to the base layer or the declaring scope.
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Char('x'))), &state),
        EventResult::Consumed
    );
}

#[test]
fn layer_guard_rejects_a_stored_path_sharing_the_layer_roots_id_under_another_parent() {
    // The focus-holding layer is rooted at `right/sheet`. The stored path
    // runs through `left/sheet`: same depth, same last segment, different
    // branch. Deciding membership on anything less than the whole ancestor
    // chain would call it "already inside the layer" and leave focus on the
    // branch the modal covers.
    let state = FocusTestState {
        focus: FocusState::intent([
            ChildId::Static("left"),
            ChildId::Static("sheet"),
            ChildId::Static("item"),
        ]),
    };
    let left = Rc::new(RefCell::new(Vec::new()));
    let right = Rc::new(RefCell::new(Vec::new()));
    let mut driver = focus_driver(20, 4);

    driver.render(&state, |ctx| {
        let left = Rc::clone(&left);
        ctx.scope(
            ChildId::Static("left"),
            Rect::new(0, 0, 20, 2),
            ScopeOptions::default(),
            move |ctx| {
                let area = ctx.area();
                ctx.scope(
                    ChildId::Static("sheet"),
                    area,
                    ScopeOptions::default(),
                    move |ctx| {
                        ctx.component(ChildId::Static("item"), FocusLeaf::recording(left), area);
                    },
                );
            },
        );
        let right = Rc::clone(&right);
        ctx.scope(
            ChildId::Static("right"),
            Rect::new(0, 2, 20, 2),
            ScopeOptions::default(),
            move |ctx| {
                let area = ctx.area();
                ctx.modal_scope(
                    ChildId::Static("sheet"),
                    area,
                    ScopeOptions::default(),
                    move |ctx| {
                        ctx.component(ChildId::Static("item"), FocusLeaf::recording(right), area);
                    },
                );
            },
        );
    });

    // Paint sees the resolved focus, not the raw app snapshot it started
    // from: focus moved off the covered branch and into the layer.
    assert_eq!(*left.borrow(), [(false, false)]);
    assert_eq!(*right.borrow(), [(true, true)]);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(FocusTestMsg::Activated(vec![
            ChildId::Static("right"),
            ChildId::Static("sheet"),
            ChildId::Static("item"),
        ]))
    );
    // Tab is trapped at the layer root, so it never reaches `left`.
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
        EventResult::Consumed
    );
}

#[test]
fn failed_modal_pass_preserves_the_previous_stack() {
    let state = FocusTestState::default();
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Driver::<FocusTestState, FocusTestMsg>::new(5, 2);
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.modal(
            ChildId::Static("stable"),
            LoggingComponent {
                name: "stable",
                log: Rc::clone(&log),
                focusable: false,
            },
            area,
        );
    });
    let failed = catch_unwind(AssertUnwindSafe(|| {
        let area = driver.area();
        driver.render(&state, |ctx| {
            ctx.modal(
                ChildId::Static("replacement"),
                PanickingFocusComponent,
                area,
            );
        });
    }));

    assert!(failed.is_err());
    assert!(driver.ratcn.modal_is_open());
    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![vec![ChildId::Static("stable")]]
    );
}

#[test]
fn modal_is_the_component_root() {
    let state = FocusTestState {
        focus: FocusState::default(),
    };
    let mut driver = focus_driver(5, 2);
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.modal(ChildId::Static("dialog"), FocusModal, area);
    });

    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![
            vec![ChildId::Static("dialog")],
            vec![ChildId::Static("dialog"), ChildId::Static("leaf"),],
        ]
    );
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(FocusTestMsg::Activated(vec![
            ChildId::Static("dialog"),
            ChildId::Static("leaf"),
        ]))
    );
}

#[test]
fn zero_area_modal_is_retained_but_excluded_from_keyboard_fallback() {
    let state = FocusTestState::default();
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut driver = focus_driver(5, 2);
    driver.render(&state, |ctx| {
        ctx.modal(
            ChildId::Static("dialog"),
            LoggingComponent {
                name: "dialog",
                log: Rc::clone(&log),
                focusable: false,
            },
            Rect::ZERO,
        );
    });

    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Consumed
    );
    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![vec![ChildId::Static("dialog")]]
    );
    assert_eq!(*log.borrow(), ["dialog"]);
}

#[test]
fn modal_wraps_focus_outside_the_component_boundary() {
    let state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("dialog"), ChildId::Static("second")]),
    };
    let mut driver = focus_driver(5, 2);
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.modal(ChildId::Static("dialog"), EscapeFocusModal, area);
    });

    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
        EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
            ChildId::Static("dialog"),
            ChildId::Static("first"),
        ])))
    );
}

#[test]
fn caught_modal_boundary_failure_is_sticky_and_atomic() {
    let mut driver = Driver::<(), ()>::new(5, 2);
    render_leaf(&mut driver, &ChildId::Static("stable"));
    let failed = catch_unwind(AssertUnwindSafe(|| {
        let area = driver.area();
        driver.render(&(), |ctx| {
            ctx.defer_paint(|_| panic!("base overlay failed"));
            let caught = catch_unwind(AssertUnwindSafe(|| {
                ctx.modal(ChildId::Static("modal"), Leaf, area);
            }));
            assert!(caught.is_err());
        });
    }));

    assert!(failed.is_err());
    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![vec![ChildId::Static("stable")]]
    );
}

#[test]
fn caught_lower_modal_overlay_flush_failure_preserves_retained_interaction() {
    let state = PointerState;
    let mut driver = Driver::new(10, 2);
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.modal(
            ChildId::Static("stable"),
            Draggable { name: "stable" },
            area,
        );
    });
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state),
        EventResult::Consumed
    );

    let failed = catch_unwind(AssertUnwindSafe(|| {
        let area = driver.area();
        driver.render(&state, |ctx| {
            ctx.modal(
                ChildId::Static("replacement"),
                Draggable {
                    name: "replacement",
                },
                area,
            );
            ctx.defer_paint(|_| panic!("lower modal overlay failed"));
            let caught = catch_unwind(AssertUnwindSafe(|| {
                ctx.modal(ChildId::Static("top"), RouteLeaf("top"), area);
            }));
            assert!(caught.is_err());
        });
    }));

    assert!(failed.is_err());
    assert!(driver.ratcn.modal_is_open());
    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![vec![ChildId::Static("stable")]]
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 9, 1), &state),
        EventResult::Emit(PointerMsg::Routed(
            "stable",
            MouseKind::Drag(MouseButton::Left),
            2,
        ))
    );
}

#[test]
fn modal_transition_cancels_base_capture_through_release() {
    let state = PointerState;
    let mut driver = Driver::new(10, 2);
    render_drag_surface(
        &mut driver,
        &state,
        &[("drag", "base", Rect::new(0, 0, 5, 2))],
    );
    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(ChildId::Static("drag"), Draggable { name: "base" }, area);
        ctx.modal(ChildId::Static("modal"), RouteLeaf("modal"), area);
    });

    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 9, 1), &state),
        EventResult::Consumed
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Up(MouseButton::Left), 9, 1), &state),
        EventResult::Consumed
    );
    render_drag_surface(
        &mut driver,
        &state,
        &[("drag", "base", Rect::new(0, 0, 5, 2))],
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Drag(MouseButton::Left), 9, 1), &state),
        EventResult::Ignored
    );
}

/// A modal opening and closing over a pointer that never moves. Covering
/// takes hover off what is beneath and uncovering gives it back, both on
/// the frame that does it — the commit re-answers the hit test, and a
/// modal is just another thing that changes the answer.
#[test]
fn a_modal_covering_and_uncovering_moves_hover_without_motion() {
    let rendered = Rc::new(RefCell::new(Vec::new()));
    let state = PointerState;
    let mut driver = Driver::<PointerState, PointerMsg>::new(5, 2);
    let render = |driver: &mut Driver<PointerState, PointerMsg>, modal| {
        let area = driver.area();
        driver.render(&state, |ctx| {
            ctx.component(
                ChildId::Static("base"),
                HoverLeaf {
                    consume_move: false,
                    rendered: Some(Rc::clone(&rendered)),
                },
                area,
            );
            if modal {
                ctx.modal(ChildId::Static("modal"), RouteLeaf("modal"), area);
            }
        });
    };

    render(&mut driver, false);
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 1, 1), &state),
        EventResult::Consumed
    );
    render(&mut driver, false);
    assert_eq!(rendered.borrow().last(), Some(&(true, true)));

    render(&mut driver, true);
    assert_eq!(
        rendered.borrow().last(),
        Some(&(false, false)),
        "the modal is what the pointer is on now"
    );
    assert_eq!(driver.ratcn.hover_path(), [ChildId::Static("modal")]);

    render(&mut driver, false);
    assert_eq!(
        rendered.borrow().last(),
        Some(&(true, true)),
        "and closing it hands the pointer back, on the same frame"
    );
    assert_eq!(driver.ratcn.hover_path(), [ChildId::Static("base")]);
}

#[test]
fn passive_overlay_never_becomes_a_hit_target() {
    let state = PointerState;
    let mut driver = Driver::new(5, 2);
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(ChildId::Static("base"), RouteLeaf("base"), area);
        ctx.defer_paint(|ctx| {
            ctx.with_buffer(|buf| {
                buf[(0, 0)].set_symbol("overlay");
            });
        });
    });

    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 0, 0), &state),
        EventResult::Emit(PointerMsg::Routed(
            "base",
            MouseKind::Down(MouseButton::Left),
            0,
        ))
    );
}

#[test]
fn duplicate_modal_root_ids_fail_before_entering_their_layer() {
    for ids in [&["a", "a"][..], &["a", "b", "a"][..]] {
        let pending_overlay = Arc::new(AtomicBool::new(false));
        let mut driver = Driver::<(), ()>::new(5, 2);
        render_leaf(&mut driver, &ChildId::Static("stable"));
        let failed = catch_unwind(AssertUnwindSafe(|| {
            let area = driver.area();
            driver.render(&(), |ctx| {
                for (position, id) in ids.iter().enumerate() {
                    ctx.modal(ChildId::Static(id), Leaf, area);
                    if position + 1 == ids.len() - 1 {
                        let painted = Arc::clone(&pending_overlay);
                        ctx.defer_paint(move |_| {
                            painted.store(true, Ordering::SeqCst);
                        });
                    }
                }
            });
        }));
        assert!(failed.is_err());
        assert!(!pending_overlay.load(Ordering::SeqCst));
        assert_eq!(
            driver.ratcn.declared_paths(),
            vec![vec![ChildId::Static("stable")]]
        );
    }
}

/// A layer root carries its kind from the moment it exists, so a modal
/// declared *inside* another modal is checked against the one enclosing it.
#[test]
fn a_duplicate_modal_id_nested_inside_a_modal_fails() {
    let mut driver = Driver::<(), ()>::new(5, 2);
    render_leaf(&mut driver, &ChildId::Static("stable"));
    let failed = catch_unwind(AssertUnwindSafe(|| {
        let area = driver.area();
        driver.render(&(), |ctx| {
            ctx.modal_scope(
                ChildId::Static("same"),
                area,
                ScopeOptions::default(),
                |ctx| {
                    ctx.modal(ChildId::Static("same"), Leaf, area);
                },
            );
        });
    }));

    assert!(failed.is_err());
    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![vec![ChildId::Static("stable")]]
    );
}

#[test]
fn base_and_modal_root_id_collision_fails_before_base_overlay_flush() {
    let painted = Arc::new(AtomicBool::new(false));
    let mut driver = Driver::<(), ()>::new(5, 2);
    render_leaf(&mut driver, &ChildId::Static("stable"));
    let failed = catch_unwind(AssertUnwindSafe(|| {
        let area = driver.area();
        driver.render(&(), |ctx| {
            ctx.component(ChildId::Static("same"), Leaf, area);
            let deferred = Arc::clone(&painted);
            ctx.defer_paint(move |_| deferred.store(true, Ordering::SeqCst));
            ctx.modal(ChildId::Static("same"), Leaf, area);
        });
    }));
    assert!(failed.is_err());
    assert!(!painted.load(Ordering::SeqCst));
    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![vec![ChildId::Static("stable")]]
    );
}

/// A parked, undeclared path recovers through Tab with an explicit focus
/// message into the modal, mirroring parked recovery at base-layer scope
/// edges; the modal's Escape wrap never leaks Tab below its layer. The
/// dialog is a focus target because it wires `on_dismiss` — that binding
/// is what makes a dialog itself focusable. Empty focus already resolves
/// to the modal's only focusable node, so its wrap absorbs Tab without a
/// message.
#[test]
fn modal_tab_recovers_parked_focus_and_absorbs_wrapped_resolved_focus() {
    let mut state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("parked")]),
    };
    let mut driver = focus_driver(10, 3);
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.modal(
            ChildId::Static("modal"),
            Dialog::new()
                .tab_wrap(TabWrap::Escape)
                .on_dismiss(|| FocusTestMsg::Activated(vec![])),
            area,
        );
    });

    let recovered = FocusState::intent([ChildId::Static("modal")]);
    for code in [KeyCode::Tab, KeyCode::BackTab] {
        assert_eq!(
            driver.event(Event::Key(KeyEvent::new(code)), &state),
            EventResult::Emit(FocusTestMsg::Focus(recovered.clone()))
        );
    }
    state.focus = FocusState::default();
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
        EventResult::Consumed
    );
}

/// A modal declared inside another modal's subtree — a dialog opening its
/// own confirmation — is the top one. Tab stays trapped in it, and keys
/// reach it rather than the modal it covers.
#[test]
fn a_modal_nested_inside_another_is_the_top_of_the_stack() {
    let state = FocusTestState {
        focus: FocusState::intent([
            ChildId::Static("outer"),
            ChildId::Static("inner"),
            ChildId::Static("leaf"),
        ]),
    };
    let mut driver = focus_driver(10, 4);
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.modal_scope(
            ChildId::Static("outer"),
            area,
            ScopeOptions::default().tab_wrap(TabWrap::Wrap),
            |ctx| {
                ctx.component(ChildId::Static("outerleaf"), FocusLeaf::enabled(), area);
                // `Escape` on purpose: the modal boundary, not the
                // scope's own wrap, must be what traps Tab.
                ctx.modal_scope(
                    ChildId::Static("inner"),
                    area,
                    ScopeOptions::default(),
                    |ctx| {
                        ctx.component(ChildId::Static("leaf"), FocusLeaf::enabled(), area);
                    },
                );
            },
        );
    });

    // Tab must not leak into the outer modal's own content.
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
        EventResult::Consumed,
        "the inner modal has one focusable leaf, so Tab wraps onto it"
    );
    // And the key reaches the inner modal's leaf, not the outer one's.
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(FocusTestMsg::Activated(vec![
            ChildId::Static("outer"),
            ChildId::Static("inner"),
            ChildId::Static("leaf"),
        ]))
    );
}

/// With `Ratcn::modals` bound, a nested modal validates against the stack
/// in the order the app opened it: outer first, then the one it opened.
#[test]
fn a_nested_modal_matches_the_app_stack_in_open_order() {
    let mut state = ModalTestState::default();
    state
        .modals
        .open(ChildId::Static("outer"), &mut state.focus)
        .expect("open outer");
    state
        .modals
        .open(ChildId::Static("inner"), &mut state.focus)
        .expect("open inner");
    let mut driver: Driver<ModalTestState, ModalTestMsg> = Driver::with(
        Ratcn::new().modals(|state: &ModalTestState| &state.modals),
        10,
        4,
    );

    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.modal_scope(
            ChildId::Static("outer"),
            area,
            ScopeOptions::default(),
            |ctx| {
                ctx.modal_scope(
                    ChildId::Static("inner"),
                    area,
                    ScopeOptions::default(),
                    |_| {},
                );
            },
        );
    });
}

/// Focus parked outside an open modal, inside a scope that wraps Tab:
/// traversal must still reach the modal. Consulting the covered scope's
/// wrap would swallow Tab forever and strand the user.
#[test]
fn tab_reaches_an_open_modal_from_a_parked_path_in_a_wrapping_scope() {
    for wrap in [TabWrap::Escape, TabWrap::Wrap] {
        let state = FocusTestState {
            // A leaf that no longer exists — e.g. a list row removed in
            // the same frame the modal opened.
            focus: FocusState::intent([ChildId::Static("pane"), ChildId::Static("gone")]),
        };
        let mut driver = focus_driver(20, 6);
        let area = driver.area();
        driver.render(&state, |ctx| {
            ctx.scope(
                ChildId::Static("pane"),
                Rect::new(0, 0, 20, 2),
                ScopeOptions::default().tab_wrap(wrap),
                |ctx| {
                    ctx.component(
                        ChildId::Static("inside"),
                        FocusLeaf::enabled(),
                        Rect::new(0, 0, 10, 1),
                    );
                },
            );
            ctx.modal(ChildId::Static("dlg"), FocusModal, area);
        });

        assert_eq!(
            driver.event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
            EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
                ChildId::Static("dlg"),
                ChildId::Static("leaf"),
            ]))),
            "Tab must enter the modal whatever the covered scope's wrap is ({wrap:?})"
        );
    }
}

#[test]
fn a_zero_area_modal_still_absorbs_keys() {
    let state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("outside")]),
    };
    let mut driver = focus_driver(20, 4);
    driver.render(&state, |ctx| {
        ctx.component(
            ChildId::Static("outside"),
            Button::new("Outside")
                .on_press(|| FocusTestMsg::Activated(vec![ChildId::Static("outside")])),
            Rect::new(0, 0, 12, 1),
        );
        // A collapsed modal takes part in nothing, but it is still
        // open: keys must not reach what it covers.
        ctx.modal(ChildId::Static("modal"), FocusLeaf::enabled(), Rect::ZERO);
    });

    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Consumed
    );
}

/// A root-level focus key cannot pull focus out of an open modal: the
/// target sits below the modal floor, so it is not focusable and the
/// binding is skipped rather than firing.
#[test]
fn a_root_focus_key_cannot_escape_an_open_modal() {
    let state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("modal")]),
    };
    let mut driver = Driver::with(
        Ratcn::new()
            .focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus)
            .focus_key(KeyChord::from('1').alt(), [ChildId::Static("outside")]),
        20,
        6,
    );
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(
            ChildId::Static("outside"),
            Button::new("Outside")
                .on_press(|| FocusTestMsg::Activated(vec![ChildId::Static("outside")])),
            Rect::new(0, 0, 12, 1),
        );
        ctx.modal(
            ChildId::Static("modal"),
            Dialog::new().on_dismiss(|| FocusTestMsg::Activated(vec![])),
            area,
        );
    });

    assert_eq!(
        driver.event(
            Event::Key(KeyEvent {
                code: KeyCode::Char('1'),
                modifiers: Modifiers {
                    alt: true,
                    ..Modifiers::NONE
                },
            }),
            &state,
        ),
        EventResult::Consumed,
        "the binding names a target the modal covers, so it does not fire"
    );
}

#[test]
fn a_modal_with_no_focusable_content_still_absorbs_keys() {
    let state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("outside")]),
    };
    let mut driver = focus_driver(30, 10);
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(
            ChildId::Static("outside"),
            Button::new("Outside")
                .on_press(|| FocusTestMsg::Activated(vec![ChildId::Static("outside")])),
            Rect::new(0, 0, 12, 1),
        );
        // No `on_dismiss`, no actions: nothing inside is focusable.
        ctx.modal(
            ChildId::Static("modal"),
            Dialog::new().title("Notice").description("Wait."),
            area,
        );
    });

    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Consumed,
        "the modal layer absorbs the key; the button beneath must not press"
    );
}

/// A modal dialog without `on_dismiss` is never itself a focus target:
/// focus resolves to its focusable descendants and Tab cycles them
/// without parking on the dialog root, whose unhandled keys the modal
/// layer still absorbs.
#[test]
fn handlerless_modal_dialog_never_becomes_the_focus_target() {
    let mut state = FocusTestState {
        focus: FocusState::default(),
    };
    let mut driver = focus_driver(30, 10);
    let draw = |driver: &mut Driver<FocusTestState, FocusTestMsg>, state: &FocusTestState| {
        let area = driver.area();
        driver.render(state, |ctx| {
            ctx.modal(
                ChildId::Static("modal"),
                Dialog::new()
                    .action(
                        ChildId::Static("ok"),
                        Button::new("OK")
                            .on_press(|| FocusTestMsg::Activated(vec![ChildId::Static("ok")])),
                    )
                    .action(
                        ChildId::Static("cancel"),
                        Button::new("Cancel")
                            .on_press(|| FocusTestMsg::Activated(vec![ChildId::Static("cancel")])),
                    ),
                area,
            );
        });
    };
    draw(&mut driver, &state);

    // Empty focus resolves to the first action; Tab cycles the actions
    // and never emits a path ending at the dialog root itself.
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
        EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
            ChildId::Static("modal"),
            ChildId::Static("cancel"),
        ])))
    );
    state.focus = FocusState::intent([ChildId::Static("modal"), ChildId::Static("cancel")]);
    draw(&mut driver, &state);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
        EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
            ChildId::Static("modal"),
            ChildId::Static("ok"),
        ]))),
        "the default Wrap cycles among the actions, never onto the dialog"
    );

    // Without `on_dismiss`, Esc emits nothing — the modal absorbs it
    // rather than letting it reach the base layer.
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Esc)), &state),
        EventResult::Consumed
    );
}

#[test]
fn top_modal_push_and_pop_cancel_capture_through_release() {
    let state = PointerState;
    let mut driver = Driver::new(10, 2);
    let render = |driver: &mut Driver<PointerState, PointerMsg>, top: bool| {
        let area = driver.area();
        driver.render(&state, |ctx| {
            ctx.modal(ChildId::Static("lower"), Draggable { name: "lower" }, area);
            if top {
                ctx.modal(ChildId::Static("top"), Draggable { name: "top" }, area);
            }
        });
    };

    render(&mut driver, false);
    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
    render(&mut driver, true);
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 9, 1), &state),
        EventResult::Consumed
    );
    driver.event(mouse(MouseKind::Up(MouseButton::Left), 9, 1), &state);

    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
    render(&mut driver, false);
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 9, 1), &state),
        EventResult::Consumed
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Up(MouseButton::Left), 9, 1), &state),
        EventResult::Consumed
    );
}

#[test]
fn same_top_modal_identity_retains_capture_when_lower_stack_changes() {
    let state = PointerState;
    let mut driver = Driver::new(10, 2);
    let render = |driver: &mut Driver<PointerState, PointerMsg>, lower, top_name| {
        let area = driver.area();
        driver.render(&state, |ctx| {
            ctx.modal(ChildId::Static(lower), RouteLeaf("lower"), area);
            ctx.modal(ChildId::Static("top"), Draggable { name: top_name }, area);
        });
    };

    render(&mut driver, "lower-a", "before");
    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
    render(&mut driver, "lower-b", "after");
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 9, 1), &state),
        EventResult::Emit(PointerMsg::Routed(
            "after",
            MouseKind::Drag(MouseButton::Left),
            2,
        ))
    );
}

#[test]
fn same_top_modal_identity_retains_hover_when_lower_stack_changes() {
    let rendered = Rc::new(RefCell::new(Vec::new()));
    let state = PointerState;
    let mut driver = Driver::<PointerState, PointerMsg>::new(10, 2);
    let render = |driver: &mut Driver<PointerState, PointerMsg>, state: &PointerState, lower| {
        let area = driver.area();
        driver.render(state, |ctx| {
            ctx.modal(ChildId::Static(lower), RouteLeaf("lower"), area);
            ctx.modal(
                ChildId::Static("top"),
                HoverLeaf {
                    consume_move: false,
                    rendered: Some(Rc::clone(&rendered)),
                },
                area,
            );
        });
    };

    render(&mut driver, &state, "lower-a");
    driver.event(mouse(MouseKind::Moved, 1, 1), &state);
    render(&mut driver, &state, "lower-a");
    render(&mut driver, &state, "lower-b");

    // The pointer never left the top modal, and swapping the stack beneath
    // it does not move what the pointer is on.
    assert_eq!(rendered.borrow().last(), Some(&(true, true)));
    assert_eq!(driver.ratcn.hover_path(), [ChildId::Static("top")]);
}

#[test]
fn modal_transition_cancels_uncaptured_click_through_release() {
    let state = PointerState;
    let mut driver = Driver::new(10, 2);
    let render = |driver: &mut Driver<PointerState, PointerMsg>, modal| {
        let area = driver.area();
        driver.render(&state, |ctx| {
            ctx.component(
                ChildId::Static("base"),
                Button::new("Base").on_press(|| {
                    PointerMsg::Routed("base", MouseKind::Click(MouseButton::Left), 0)
                }),
                area,
            );
            if modal {
                ctx.modal(
                    ChildId::Static("modal"),
                    Button::new("Modal").on_press(|| {
                        PointerMsg::Routed("modal", MouseKind::Click(MouseButton::Left), 0)
                    }),
                    area,
                );
            }
        });
    };

    render(&mut driver, false);
    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state);
    render(&mut driver, true);
    assert_eq!(
        driver.event(mouse(MouseKind::Up(MouseButton::Left), 1, 0), &state),
        EventResult::Consumed
    );

    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state);
    assert_eq!(
        driver.event(mouse(MouseKind::Up(MouseButton::Left), 1, 0), &state),
        EventResult::Emit(PointerMsg::Routed(
            "modal",
            MouseKind::Click(MouseButton::Left),
            0,
        ))
    );
}

#[test]
fn repeated_descendant_ids_render_focus_only_on_the_complete_path() {
    let state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("left"), ChildId::Static("shared")]),
    };
    let left = Rc::new(RefCell::new(Vec::new()));
    let right = Rc::new(RefCell::new(Vec::new()));
    let mut driver = focus_driver(10, 2);

    let area = driver.area();
    driver.render(&state, |ctx| {
        for (id, rendered) in [("left", &left), ("right", &right)] {
            let rendered = Rc::clone(rendered);
            ctx.scope(
                ChildId::Static(id),
                area,
                ScopeOptions::default(),
                move |ctx| {
                    ctx.component(
                        ChildId::Static("shared"),
                        FocusLeaf::recording(rendered),
                        area,
                    );
                },
            );
        }
    });

    assert_eq!(*left.borrow(), [(true, true)]);
    assert_eq!(*right.borrow(), [(false, false)]);
}

#[test]
fn repeated_descendant_ids_render_hover_only_on_the_complete_path() {
    let state = PointerState;
    let left = Rc::new(RefCell::new(Vec::new()));
    let right = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Driver::<PointerState, PointerMsg>::new(10, 2);
    let render = |driver: &mut Driver<PointerState, PointerMsg>| {
        driver.render(&state, |ctx| {
            for (id, x, rendered) in [("left", 0, &left), ("right", 5, &right)] {
                let rendered = Rc::clone(rendered);
                let area = Rect::new(x, 0, 5, 2);
                ctx.scope(
                    ChildId::Static(id),
                    area,
                    ScopeOptions::default(),
                    move |ctx| {
                        ctx.component(
                            ChildId::Static("shared"),
                            HoverLeaf {
                                consume_move: false,
                                rendered: Some(rendered),
                            },
                            area,
                        );
                    },
                );
            }
        });
    };

    render(&mut driver);
    driver.event(mouse(MouseKind::Moved, 1, 0), &state);
    left.borrow_mut().clear();
    right.borrow_mut().clear();
    render(&mut driver);

    assert_eq!(
        driver.ratcn.hover_path(),
        [ChildId::Static("left"), ChildId::Static("shared")]
    );
    assert_eq!(*left.borrow(), [(true, true)]);
    assert_eq!(
        *right.borrow(),
        [(false, false)],
        "the same leaf id under another parent is a different component"
    );
}

#[test]
fn modal_mismatch_release_clears_the_pre_transition_press() {
    let mut state = ModalTestState::default();
    let mut driver = Driver::with(
        Ratcn::new()
            .focus(|state: &ModalTestState| &state.focus, |_| unreachable!())
            .modals(|state: &ModalTestState| &state.modals),
        10,
        2,
    );
    let render = |driver: &mut Driver<ModalTestState, ModalTestMsg>, state: &ModalTestState| {
        let area = driver.area();
        driver.render(state, |ctx| {
            ctx.component(
                ChildId::Static("base"),
                Button::new("Base").on_press(|| ModalTestMsg::Routed("base")),
                area,
            );
            if state.modals.is_open("dialog") {
                ctx.modal(
                    ChildId::Static("dialog"),
                    Button::new("Dialog").on_press(|| ModalTestMsg::Routed("dialog")),
                    area,
                );
            }
        });
    };

    render(&mut driver, &state);
    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state);
    state
        .modals
        .open("dialog", &mut state.focus)
        .expect("open dialog");
    assert_eq!(
        driver.event(mouse(MouseKind::Up(MouseButton::Left), 1, 0), &state),
        EventResult::Consumed
    );

    render(&mut driver, &state);
    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state);
    assert_eq!(
        driver.event(mouse(MouseKind::Up(MouseButton::Left), 1, 0), &state),
        EventResult::Emit(ModalTestMsg::Routed("dialog"))
    );

    state.modals.close(&mut state.focus).expect("close dialog");
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state),
        EventResult::Consumed
    );
    state
        .modals
        .open("dialog", &mut state.focus)
        .expect("reopen before redraw");
    assert_eq!(
        driver.event(mouse(MouseKind::Up(MouseButton::Left), 1, 0), &state),
        EventResult::Consumed
    );
}
