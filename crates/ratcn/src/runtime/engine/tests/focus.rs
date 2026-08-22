//! Where focus resolves to, how Tab and focus keys move it, and what happens
//! to a path whose target is not in the tree.

use super::*;

struct FocusComposite {
    parent_rendered: FocusRenderLog,
    child_rendered: FocusRenderLog,
}

impl Component<FocusTestState, FocusTestMsg> for FocusComposite {
    fn declare(&mut self, ctx: &mut DeclareCtx<'_, FocusTestState, FocusTestMsg>) {
        let area = ctx.area();
        ctx.component(
            ChildId::Static("child"),
            FocusLeaf::recording(Rc::clone(&self.child_rendered)),
            area,
        );
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, FocusTestState>) {
        self.parent_rendered
            .borrow_mut()
            .push((ctx.focused(), ctx.contains_focus()));
    }

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &FocusTestState,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<FocusTestMsg> {
        match event {
            Event::Key(key) if key.code == KeyCode::Char('p') => {
                EventResult::Emit(FocusTestMsg::Parent(ctx.path().to_vec()))
            }
            _ => EventResult::Ignored,
        }
    }

    fn scope_options(&self) -> ScopeOptions {
        ScopeOptions::default()
    }
}

struct EmptyComposite {
    rendered: FocusRenderLog,
    self_focusable: bool,
}

impl Component<FocusTestState, FocusTestMsg> for EmptyComposite {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, FocusTestState, FocusTestMsg>) {}

    fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, FocusTestState>) {
        self.rendered
            .borrow_mut()
            .push((ctx.focused(), ctx.contains_focus()));
    }

    fn scope_options(&self) -> ScopeOptions {
        let options = ScopeOptions::default();
        if self.self_focusable {
            options.focusable()
        } else {
            options
        }
    }
}

fn render_timing_button(
    driver: &mut Driver<ButtonTimingState, ButtonTimingMsg>,
    state: &ButtonTimingState,
    message: impl Fn() -> ButtonTimingMsg + 'static,
) {
    let message = Rc::new(message);
    let area = driver.area();
    driver.render(state, |ctx| {
        let message = Rc::clone(&message);
        ctx.component(
            ChildId::Static("save"),
            Button::new("Save")
                .disabled(state.saving)
                .on_press(move || message()),
            area,
        );
    });
}

#[test]
fn startup_focus_renders_and_routes_to_the_first_focusable_leaf() {
    let state = FocusTestState::default();
    let rendered = Rc::new(RefCell::new(Vec::new()));
    let first_rendered = Rc::clone(&rendered);
    let second_rendered = Rc::clone(&rendered);
    let mut driver = wrapping_focus_driver(10, 3);

    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.scope(
            ChildId::Static("pane"),
            Rect::ZERO,
            ScopeOptions::default(),
            |ctx| {
                ctx.component(
                    ChildId::Static("first"),
                    FocusLeaf::recording(Rc::clone(&first_rendered)),
                    area,
                );
                ctx.component(
                    ChildId::Static("second"),
                    FocusLeaf::recording(Rc::clone(&second_rendered)),
                    area,
                );
            },
        );
    });

    // Paint reports the resolved focus, once per leaf: startup focus
    // landed on the first, and that is what painted focused.
    assert_eq!(*rendered.borrow(), vec![(true, true), (false, false)]);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(FocusTestMsg::Activated(vec![
            ChildId::Static("pane"),
            ChildId::Static("first"),
        ]))
    );
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
        EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
            ChildId::Static("pane"),
            ChildId::Static("second"),
        ])))
    );
}

#[test]
fn startup_focus_skips_collapsed_candidates_and_routing_agrees() {
    let state = FocusTestState::default();
    let collapsed = Rc::new(RefCell::new(Vec::new()));
    let visible = Rc::new(RefCell::new(Vec::new()));
    let mut driver = focus_driver(10, 2);

    driver.render(&state, |ctx| {
        ctx.scope(
            ChildId::Static("group"),
            Rect::new(0, 0, 4, 1),
            ScopeOptions::default(),
            |ctx| {
                ctx.component(
                    ChildId::Static("collapsed"),
                    FocusLeaf::recording(Rc::clone(&collapsed)),
                    Rect::new(0, 0, 0, 1),
                );
            },
        );
        ctx.component(
            ChildId::Static("visible"),
            FocusLeaf::recording(Rc::clone(&visible)),
            Rect::new(5, 0, 4, 1),
        );
    });

    // Focus resolves between the passes against actual geometry, so the
    // zero-area candidate is never targeted: startup focus lands on the
    // visible leaf, paints there, and routes there — render and routing
    // agree because both come from the same resolution.
    assert_eq!(*collapsed.borrow(), [(false, false)]);
    assert_eq!(*visible.borrow(), [(true, true)]);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(FocusTestMsg::Activated(vec![ChildId::Static("visible")])),
        "routing targets the leaf that actually painted focused"
    );
}

#[test]
fn empty_focus_renders_and_routes_to_the_active_modal() {
    let state = FocusTestState::default();
    let base = Rc::new(RefCell::new(Vec::new()));
    let modal = Rc::new(RefCell::new(Vec::new()));
    let mut driver = focus_driver(10, 3);

    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(
            ChildId::Static("base"),
            FocusLeaf::recording(Rc::clone(&base)),
            area,
        );
        ctx.modal(
            ChildId::Static("modal"),
            FocusLeaf::recording(Rc::clone(&modal)),
            area,
        );
    });

    // Startup focus resolves once, against the complete tree — the modal
    // is already known, so only the modal paints focused.
    assert_eq!(*base.borrow(), [(false, false)]);
    assert_eq!(*modal.borrow(), [(true, true)]);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(FocusTestMsg::Activated(vec![ChildId::Static("modal")]))
    );
}

#[test]
fn composite_reports_focus_within_and_receives_bubbled_events() {
    let state = FocusTestState::default();
    let parent_rendered = Rc::new(RefCell::new(Vec::new()));
    let child_rendered = Rc::new(RefCell::new(Vec::new()));
    let mut driver = wrapping_focus_driver(10, 3);

    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(
            ChildId::Static("composite"),
            FocusComposite {
                parent_rendered: Rc::clone(&parent_rendered),
                child_rendered: Rc::clone(&child_rendered),
            },
            area,
        );
    });

    assert_eq!(*parent_rendered.borrow(), vec![(false, true)]);
    assert_eq!(*child_rendered.borrow(), vec![(true, true)]);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Char('p'))), &state),
        EventResult::Emit(FocusTestMsg::Parent(vec![ChildId::Static("composite")]))
    );
}

#[test]
fn empty_composite_does_not_claim_sibling_focus_but_can_focus_itself() {
    let state = FocusTestState::default();
    let empty_rendered = Rc::new(RefCell::new(Vec::new()));
    let leaf_rendered = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Driver::<FocusTestState, FocusTestMsg>::new(10, 3);

    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(
            ChildId::Static("empty"),
            EmptyComposite {
                rendered: Rc::clone(&empty_rendered),
                self_focusable: false,
            },
            area,
        );
        ctx.component(
            ChildId::Static("leaf"),
            FocusLeaf::recording(Rc::clone(&leaf_rendered)),
            area,
        );
    });

    assert_eq!(*empty_rendered.borrow(), vec![(false, false)]);
    assert_eq!(*leaf_rendered.borrow(), vec![(true, true)]);

    empty_rendered.borrow_mut().clear();
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(
            ChildId::Static("empty"),
            EmptyComposite {
                rendered: Rc::clone(&empty_rendered),
                self_focusable: true,
            },
            area,
        );
    });
    assert_eq!(*empty_rendered.borrow(), vec![(true, true)]);
}

#[test]
fn tab_and_backtab_traverse_siblings_and_honor_nested_escape_and_wrap() {
    let mut state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("left"), ChildId::Static("a2")]),
    };
    let mut driver = wrapping_focus_driver(10, 3);

    let render =
        |driver: &mut Driver<FocusTestState, FocusTestMsg>, state: &FocusTestState, left_wrap| {
            let area = driver.area();
            driver.render(state, |ctx| {
                ctx.component(ChildId::Static("before"), FocusLeaf::enabled(), area);
                ctx.scope(
                    ChildId::Static("left"),
                    Rect::ZERO,
                    ScopeOptions::default().tab_wrap(left_wrap),
                    |ctx| {
                        ctx.component(ChildId::Static("a1"), FocusLeaf::enabled(), area);
                        ctx.component(ChildId::Static("a2"), FocusLeaf::enabled(), area);
                    },
                );
                ctx.scope(
                    ChildId::Static("right"),
                    Rect::ZERO,
                    ScopeOptions::default(),
                    |ctx| {
                        ctx.component(ChildId::Static("b1"), FocusLeaf::enabled(), area);
                    },
                );
            });
        };

    render(&mut driver, &state, TabWrap::Escape);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::BackTab)), &state),
        EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
            ChildId::Static("left"),
            ChildId::Static("a1"),
        ])))
    );
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
        EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
            ChildId::Static("right"),
            ChildId::Static("b1"),
        ])))
    );
    state.focus = FocusState::intent([ChildId::Static("left"), ChildId::Static("a1")]);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::BackTab)), &state),
        EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([ChildId::Static(
            "before"
        ),])))
    );

    render(&mut driver, &state, TabWrap::Wrap);
    state.focus = FocusState::intent([ChildId::Static("left"), ChildId::Static("a2")]);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
        EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
            ChildId::Static("left"),
            ChildId::Static("a1"),
        ])))
    );
    state.focus = FocusState::intent([ChildId::Static("left"), ChildId::Static("a1")]);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::BackTab)), &state),
        EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
            ChildId::Static("left"),
            ChildId::Static("a2"),
        ])))
    );
}

#[test]
fn backtab_accepts_shift_but_ignores_ctrl_and_alt() {
    let state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("second")]),
    };
    let mut driver = focus_driver(10, 2);
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component("first", FocusLeaf::enabled(), area);
        ctx.component("second", FocusLeaf::enabled(), area);
    });

    let backtab = |modifiers| {
        Event::Key(KeyEvent {
            code: KeyCode::BackTab,
            modifiers,
        })
    };
    assert_eq!(
        driver.event(
            backtab(Modifiers {
                shift: true,
                ..Modifiers::NONE
            }),
            &state,
        ),
        EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([ChildId::Static(
            "first"
        ),])))
    );
    for modifiers in [
        Modifiers {
            ctrl: true,
            shift: true,
            ..Modifiers::NONE
        },
        Modifiers {
            alt: true,
            shift: true,
            ..Modifiers::NONE
        },
    ] {
        assert_eq!(
            driver.event(backtab(modifiers), &state),
            EventResult::Ignored
        );
    }
}

#[test]
fn reordering_preserves_identity_and_changes_tab_order() {
    let b = ChildId::Dynamic(Arc::from("b"));
    let mut state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("items"), b.clone()]),
    };
    let mut driver = wrapping_focus_driver(10, 3);

    let render = |driver: &mut Driver<FocusTestState, FocusTestMsg>,
                  state: &FocusTestState,
                  ids: [ChildId; 3]| {
        let area = driver.area();
        driver.render(state, |ctx| {
            ctx.scope(
                ChildId::Static("items"),
                Rect::ZERO,
                ScopeOptions::default(),
                |ctx| {
                    for id in &ids {
                        ctx.component(id.clone(), FocusLeaf::enabled(), area);
                    }
                },
            );
        });
    };

    render(
        &mut driver,
        &state,
        [
            ChildId::Dynamic(Arc::from("a")),
            b.clone(),
            ChildId::Dynamic(Arc::from("c")),
        ],
    );
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
        EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
            ChildId::Static("items"),
            ChildId::Dynamic(Arc::from("c")),
        ])))
    );

    render(
        &mut driver,
        &state,
        [
            ChildId::Dynamic(Arc::from("c")),
            b.clone(),
            ChildId::Dynamic(Arc::from("a")),
        ],
    );
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(FocusTestMsg::Activated(vec![ChildId::Static("items"), b,]))
    );
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
        EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
            ChildId::Static("items"),
            ChildId::Dynamic(Arc::from("a")),
        ])))
    );
    state.focus = FocusState::default();
}

#[test]
fn absent_and_partial_focus_park_then_recover_at_scope_edges() {
    let state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("items"), ChildId::Static("missing")]),
    };
    let mut driver = wrapping_focus_driver(10, 3);

    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.scope(
            ChildId::Static("items"),
            Rect::ZERO,
            ScopeOptions::default(),
            |ctx| {
                ctx.component(ChildId::Static("first"), FocusLeaf::enabled(), area);
                ctx.component(ChildId::Static("last"), FocusLeaf::enabled(), area);
            },
        );
    });

    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Ignored
    );
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
        EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
            ChildId::Static("items"),
            ChildId::Static("first"),
        ])))
    );
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::BackTab)), &state),
        EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
            ChildId::Static("items"),
            ChildId::Static("last"),
        ])))
    );
}

#[test]
fn parked_future_tree_intent_resolves_when_target_reappears() {
    let state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("items"), ChildId::Static("target")]),
    };
    let mut driver = focus_driver(10, 3);

    driver.render(&state, |ctx| {
        ctx.scope(
            ChildId::Static("items"),
            Rect::ZERO,
            ScopeOptions::default(),
            |_| {},
        );
    });
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Ignored
    );

    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.scope(
            ChildId::Static("items"),
            Rect::ZERO,
            ScopeOptions::default(),
            |ctx| {
                ctx.component(ChildId::Static("target"), FocusLeaf::enabled(), area);
            },
        );
    });
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(FocusTestMsg::Activated(vec![
            ChildId::Static("items"),
            ChildId::Static("target"),
        ]))
    );
}

#[test]
fn absent_focus_escapes_an_empty_scope_but_wrap_traps_it() {
    let state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("left"), ChildId::Static("removed")]),
    };
    let mut driver = wrapping_focus_driver(10, 3);

    let render = |driver: &mut Driver<FocusTestState, FocusTestMsg>, left_wrap| {
        let area = driver.area();
        driver.render(&state, |ctx| {
            ctx.scope(
                ChildId::Static("left"),
                Rect::ZERO,
                ScopeOptions::default().tab_wrap(left_wrap),
                |_| {},
            );
            ctx.scope(
                ChildId::Static("right"),
                Rect::ZERO,
                ScopeOptions::default(),
                |ctx| {
                    ctx.component(ChildId::Static("b1"), FocusLeaf::enabled(), area);
                },
            );
        });
    };

    render(&mut driver, TabWrap::Escape);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
        EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
            ChildId::Static("right"),
            ChildId::Static("b1"),
        ])))
    );

    render(&mut driver, TabWrap::Wrap);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
        EventResult::Consumed
    );
}

#[test]
fn scope_only_intent_descends_to_the_first_enabled_leaf() {
    let mut state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("pane")]),
    };
    let mut driver = wrapping_focus_driver(10, 3);

    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.scope(
            ChildId::Static("pane"),
            Rect::ZERO,
            ScopeOptions::default(),
            |ctx| {
                ctx.component(ChildId::Static("disabled"), FocusLeaf::disabled(), area);
                ctx.component(ChildId::Static("enabled"), FocusLeaf::enabled(), area);
            },
        );
    });

    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(FocusTestMsg::Activated(vec![
            ChildId::Static("pane"),
            ChildId::Static("enabled"),
        ]))
    );

    state.focus = FocusState::intent([ChildId::Static("pane"), ChildId::Static("disabled")]);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Ignored
    );
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
        EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
            ChildId::Static("pane"),
            ChildId::Static("enabled"),
        ])))
    );
}

#[test]
fn focus_keys_resolve_relative_to_the_bubbling_scope() {
    let state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("pane"), ChildId::Static("first")]),
    };
    let mut driver = wrapping_focus_driver(10, 3);

    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.scope(
            ChildId::Static("pane"),
            Rect::ZERO,
            ScopeOptions::default().focus_key('x', [ChildId::Static("second")]),
            |ctx| {
                ctx.component(ChildId::Static("first"), FocusLeaf::enabled(), area);
                ctx.component(ChildId::Static("second"), FocusLeaf::enabled(), area);
            },
        );
    });

    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Char('x'))), &state),
        EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
            ChildId::Static("pane"),
            ChildId::Static("second"),
        ])))
    );

    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.scope(
            ChildId::Static("pane"),
            Rect::ZERO,
            ScopeOptions::default().focus_key('x', [ChildId::Static("second")]),
            |ctx| {
                ctx.component(
                    ChildId::Static("first"),
                    FocusLeaf::consuming_focus_key(),
                    area,
                );
                ctx.component(ChildId::Static("second"), FocusLeaf::enabled(), area);
            },
        );
    });
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Char('x'))), &state),
        EventResult::Consumed
    );
}

#[test]
fn focus_keys_normalize_chars_but_match_ctrl_and_alt_exactly() {
    let mut state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("first")]),
    };
    let mut driver = Driver::with(
        Ratcn::new()
            .focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus)
            .focus_key('c', [ChildId::Static("second")])
            .focus_key(
                KeyChord::from('m').ctrl().alt(),
                [ChildId::Static("second")],
            ),
        10,
        2,
    );
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(ChildId::Static("first"), FocusLeaf::enabled(), area);
        ctx.component(ChildId::Static("second"), FocusLeaf::enabled(), area);
    });

    let key = |code, ctrl, alt, shift| {
        Event::Key(KeyEvent {
            code,
            modifiers: Modifiers { ctrl, alt, shift },
        })
    };
    let second = FocusTestMsg::Focus(FocusState::intent([ChildId::Static("second")]));
    assert_eq!(
        driver.event(key(KeyCode::Char('C'), false, false, true), &state),
        EventResult::Emit(second)
    );
    assert_eq!(
        driver.event(key(KeyCode::Char('c'), true, false, false), &state),
        EventResult::Ignored
    );
    for (ctrl, alt) in [(false, false), (true, false), (false, true)] {
        assert_eq!(
            driver.event(key(KeyCode::Char('m'), ctrl, alt, false), &state),
            EventResult::Ignored
        );
    }
    assert!(matches!(
        driver.event(key(KeyCode::Char('m'), true, true, false), &state),
        EventResult::Emit(FocusTestMsg::Focus(_))
    ));

    state.focus = FocusState::intent([ChildId::Static("second")]);
    assert_eq!(
        driver.event(key(KeyCode::Char('C'), false, false, true), &state),
        EventResult::Consumed,
        "an already-satisfied focus shortcut must not emit redundant state"
    );
}

#[test]
fn invalid_inner_focus_key_falls_back_to_the_outer_binding() {
    let state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("pane"), ChildId::Static("first")]),
    };
    let mut driver = Driver::with(
        Ratcn::new()
            .focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus)
            .focus_key('x', [ChildId::Static("outside")]),
        10,
        3,
    );

    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.scope(
            ChildId::Static("pane"),
            Rect::ZERO,
            ScopeOptions::default().focus_key('x', [ChildId::Static("missing")]),
            |ctx| {
                ctx.component(ChildId::Static("first"), FocusLeaf::enabled(), area);
            },
        );
        ctx.component(ChildId::Static("outside"), FocusLeaf::enabled(), area);
    });

    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Char('x'))), &state),
        EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([ChildId::Static(
            "outside"
        ),])))
    );
}

#[test]
fn events_before_the_first_render_are_ignored() {
    let state = FocusTestState::default();
    let mut driver = focus_driver(10, 3);

    assert!(!driver.ratcn.has_rendered());

    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
        EventResult::Ignored
    );

    let failed = catch_unwind(AssertUnwindSafe(|| {
        driver.render(&state, |_| panic!("first render failed"));
    }));
    assert!(failed.is_err());
    assert!(!driver.ratcn.has_rendered());

    driver.render(&state, |_| {});

    assert!(driver.ratcn.has_rendered());
}

#[test]
fn semantic_modal_before_the_first_render_still_ignores_events() {
    let mut state = ModalTestState::default();
    state
        .modals
        .open(ChildId::Static("dialog"), &mut state.focus)
        .expect("open modal");
    let mut driver: Driver<ModalTestState, ModalTestMsg> = Driver::with(
        Ratcn::new().modals(|state: &ModalTestState| &state.modals),
        5,
        2,
    );

    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
        EventResult::Ignored
    );
}

#[test]
fn button_events_use_last_rendered_disabledness_until_redraw() {
    let mut state = ButtonTimingState::default();
    let mut driver = Driver::with(
        Ratcn::new().focus(
            |state: &ButtonTimingState| &state.focus,
            ButtonTimingMsg::Focus,
        ),
        10,
        3,
    );
    let enter = Event::Key(KeyEvent::new(KeyCode::Enter));

    render_timing_button(&mut driver, &state, || ButtonTimingMsg::Save);
    let EventResult::Emit(first) = driver.event(enter.clone(), &state) else {
        panic!("rendered enabled button did not emit");
    };
    assert!(update_button_timing(&mut state, first));

    let EventResult::Emit(second) = driver.event(enter.clone(), &state) else {
        panic!("old enabled declaration did not handle the second event");
    };
    assert!(!update_button_timing(&mut state, second));
    assert_eq!(state.accepted_saves, 1);

    render_timing_button(&mut driver, &state, || ButtonTimingMsg::Save);
    assert_eq!(driver.event(enter.clone(), &state), EventResult::Ignored);

    state.saving = false;
    assert_eq!(driver.event(enter, &state), EventResult::Ignored);
}

#[test]
fn failed_render_keeps_the_previous_button_declaration_interactive() {
    let mut state = ButtonTimingState::default();
    let mut driver = Driver::with(
        Ratcn::new().focus(
            |state: &ButtonTimingState| &state.focus,
            ButtonTimingMsg::Focus,
        ),
        10,
        3,
    );
    render_timing_button(&mut driver, &state, || ButtonTimingMsg::Save);
    state.saving = true;

    let result = catch_unwind(AssertUnwindSafe(|| {
        let area = driver.area();
        driver.render(&state, |ctx| {
            ctx.component(
                ChildId::Static("save"),
                Button::new("Replacement")
                    .disabled(true)
                    .on_press(|| ButtonTimingMsg::Replacement),
                area,
            );
            panic!("failed after staging replacement button");
        });
    }));

    assert!(result.is_err());
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(ButtonTimingMsg::Save)
    );
}

struct HoverFocusComposite;

impl Component<HoverFocusState, HoverFocusMsg> for HoverFocusComposite {
    fn declare(&mut self, ctx: &mut DeclareCtx<'_, HoverFocusState, HoverFocusMsg>) {
        let area = ctx.area();
        ctx.component(ChildId::Static("leaf"), HoverFocusLeaf::enabled(), area);
    }

    fn scope_options(&self) -> ScopeOptions {
        ScopeOptions::default()
    }
}

#[test]
fn focusable_decorative_scope_receives_mouse_focus_and_hover_context() {
    let rendered = Rc::new(RefCell::new(Vec::new()));
    let state = HoverFocusState {
        focus: FocusState::intent([ChildId::Static("other")]),
    };
    let mut driver = Driver::with(
        Ratcn::new().focus(|state: &HoverFocusState| &state.focus, HoverFocusMsg::Focus),
        8,
        2,
    );
    let render = |driver: &mut Driver<HoverFocusState, HoverFocusMsg>, state: &HoverFocusState| {
        driver.render(state, |ctx| {
            ctx.component(
                ChildId::Static("other"),
                HoverFocusLeaf::enabled(),
                Rect::new(0, 0, 2, 2),
            );
            let rendered = Rc::clone(&rendered);
            ctx.scope(
                ChildId::Static("decoration"),
                Rect::new(3, 0, 5, 2),
                ScopeOptions::default().focusable(),
                move |ctx| {
                    ctx.paint(move |ctx| {
                        rendered
                            .borrow_mut()
                            .push((ctx.hovered(), ctx.contains_hover()));
                    });
                },
            );
        });
    };

    render(&mut driver, &state);
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 4, 0), &state),
        EventResult::Consumed,
        "the decorative scope is a hover target, and the frame is now stale"
    );
    render(&mut driver, &state);
    // Frame one: not hovered. Frame two: the pointer rests on the scope,
    // and paint sees it.
    assert_eq!(*rendered.borrow(), [(false, false), (true, true)]);
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 4, 0), &state),
        EventResult::Emit(HoverFocusMsg::Focus(FocusState::intent([ChildId::Static(
            "decoration"
        )])))
    );
}

/// A focusable leaf that queues a paint thunk from its own declaration, so
/// a test can see which node a thunk's flags are read from.
struct ThunkProbe(FocusRenderLog);

impl Component<FocusTestState, FocusTestMsg> for ThunkProbe {
    fn declare(&mut self, ctx: &mut DeclareCtx<'_, FocusTestState, FocusTestMsg>) {
        let log = Rc::clone(&self.0);
        ctx.paint(move |ctx| {
            log.borrow_mut().push((ctx.focused(), ctx.contains_focus()));
        });
    }

    fn is_focusable(&self) -> bool {
        true
    }
}

/// Paint queued from a declaration belongs to *that* declaration — not to
/// the root, and not to the outermost scope it happens to sit inside.
///
/// The scope's thunk carries the `focus-within` signal a container paints
/// its border from, and the leaf's carries `focused`. Recording both is
/// what makes the attribution visible: the two disagree only because each
/// thunk is read from its own node.
#[test]
fn a_scope_thunk_reports_focus_within_its_subtree() {
    let state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("pane"), ChildId::Static("leaf")]),
    };
    let scope_flags = Rc::new(RefCell::new(Vec::new()));
    let leaf_flags = Rc::new(RefCell::new(Vec::new()));
    let mut driver = focus_driver(6, 1);

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
                        .push((ctx.focused(), ctx.contains_focus()));
                });
                ctx.component(ChildId::Static("leaf"), ThunkProbe(leaf_flags), area);
            },
        );
    });

    assert_eq!(
        *scope_flags.borrow(),
        [(false, true)],
        "the scope contains focus without being the focused leaf"
    );
    assert_eq!(
        *leaf_flags.borrow(),
        [(true, true)],
        "the leaf's own thunk is read from the leaf"
    );
}

#[test]
fn focus_path_validates_latest_surface_focusability_and_scope_descent() {
    let state = HoverFocusState::default();
    let dynamic = ChildId::Dynamic(Arc::from("dynamic"));
    let mut driver = Driver::<HoverFocusState, HoverFocusMsg>::new(8, 2);
    assert!(
        driver
            .ratcn
            .focus_path(&[ChildId::Static("pane")])
            .is_none()
    );
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.scope(
            ChildId::Static("pane"),
            Rect::ZERO,
            ScopeOptions::default(),
            |ctx| {
                ctx.component(
                    ChildId::Static("disabled"),
                    HoverFocusLeaf::disabled(),
                    area,
                );
                ctx.component(ChildId::Static("enabled"), HoverFocusLeaf::enabled(), area);
            },
        );
        ctx.component(dynamic.clone(), HoverFocusLeaf::enabled(), area);
    });

    assert_eq!(
        driver.ratcn.focus_path(&[ChildId::Static("pane")]),
        Some(FocusState::intent([
            ChildId::Static("pane"),
            ChildId::Static("enabled")
        ]))
    );
    assert!(
        driver
            .ratcn
            .focus_path(&[ChildId::Static("pane"), ChildId::Static("disabled")])
            .is_none()
    );
    assert_eq!(
        driver.ratcn.focus_path(std::slice::from_ref(&dynamic)),
        Some(FocusState::intent([dynamic.clone()]))
    );
    assert!(
        driver
            .ratcn
            .focus_path(&[ChildId::Static("missing")])
            .is_none()
    );

    driver.render(&state, |_| {});
    assert!(
        driver
            .ratcn
            .focus_path(std::slice::from_ref(&dynamic))
            .is_none()
    );
}

#[test]
fn collapsed_components_are_excluded_and_recover_when_geometry_reappears() {
    let mut state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("width-zero")]),
    };
    let mut driver = focus_driver(8, 2);
    let render = |driver: &mut Driver<FocusTestState, FocusTestMsg>, recovered| {
        driver.render(&state, |ctx| {
            ctx.component(
                ChildId::Static("width-zero"),
                FocusLeaf::enabled(),
                if recovered {
                    Rect::new(0, 0, 1, 1)
                } else {
                    Rect::new(0, 0, 0, 1)
                },
            );
            ctx.component(
                ChildId::Static("height-zero"),
                FocusLeaf::enabled(),
                Rect::new(1, 0, 1, 0),
            );
            ctx.component(
                ChildId::Static("visible"),
                FocusLeaf::enabled(),
                Rect::new(2, 0, 1, 1),
            );
            ctx.scope(
                ChildId::Static("group"),
                Rect::ZERO,
                ScopeOptions::default(),
                |ctx| {
                    ctx.component(
                        ChildId::Static("child"),
                        FocusLeaf::enabled(),
                        Rect::new(4, 0, 1, 1),
                    );
                },
            );
        });
    };

    render(&mut driver, false);
    for id in ["width-zero", "height-zero"] {
        assert!(driver.ratcn.focus_path(&[ChildId::Static(id)]).is_none());
    }
    assert_eq!(
        driver.ratcn.focus_path(&[ChildId::Static("group")]),
        Some(FocusState::intent([
            ChildId::Static("group"),
            ChildId::Static("child")
        ]))
    );
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Ignored,
        "a parked collapsed target must not activate"
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 0, 0), &state),
        EventResult::Ignored,
        "collapsed geometry must not be a mouse target"
    );
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
        EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([ChildId::Static(
            "visible"
        )])))
    );

    render(&mut driver, true);
    assert_eq!(
        driver.ratcn.focus_path(&[ChildId::Static("width-zero")]),
        Some(FocusState::intent([ChildId::Static("width-zero")]))
    );
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(FocusTestMsg::Activated(vec![ChildId::Static("width-zero")]))
    );
    state.focus = FocusState::intent([ChildId::Static("visible")]);
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 0, 0), &state),
        EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([ChildId::Static(
            "width-zero"
        )])))
    );
}

#[test]
fn zero_area_focusable_scope_groups_descendants_but_cannot_hold_focus_itself() {
    let state = FocusTestState::default();
    let mut driver = focus_driver(8, 2);

    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.scope(
            ChildId::Static("empty"),
            Rect::ZERO,
            ScopeOptions::default().focusable(),
            |_| {},
        );
        ctx.component(ChildId::Static("visible"), FocusLeaf::enabled(), area);
    });

    assert!(
        driver
            .ratcn
            .focus_path(&[ChildId::Static("empty")])
            .is_none()
    );
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(FocusTestMsg::Activated(vec![ChildId::Static("visible")]))
    );
}

#[test]
fn focus_path_rejects_inactive_layers_and_descends_in_active_modal() {
    let state = HoverFocusState::default();
    let mut driver = Driver::<HoverFocusState, HoverFocusMsg>::new(8, 2);
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(ChildId::Static("base"), HoverFocusLeaf::enabled(), area);
        ctx.modal(ChildId::Static("lower"), HoverFocusComposite, area);
        ctx.modal(ChildId::Static("top"), HoverFocusComposite, area);
    });

    for inactive in [ChildId::Static("base"), ChildId::Static("lower")] {
        assert!(driver.ratcn.focus_path(&[inactive]).is_none());
    }
    assert_eq!(
        driver.ratcn.focus_path(&[ChildId::Static("top")]),
        Some(FocusState::intent([
            ChildId::Static("top"),
            ChildId::Static("leaf")
        ]))
    );
}
