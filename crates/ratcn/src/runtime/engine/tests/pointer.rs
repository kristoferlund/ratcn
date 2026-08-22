//! Pointer gestures: capture, transient state, and the clicks and drags the
//! runtime synthesizes from presses and releases.

use super::*;

#[derive(Clone, Copy)]
enum DownBehavior {
    CaptureAndIgnore,
    Consume,
    Emit,
}

struct DownFocusLeaf(DownBehavior);

impl Component<FocusTestState, FocusTestMsg> for DownFocusLeaf {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, FocusTestState, FocusTestMsg>) {}

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &FocusTestState,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<FocusTestMsg> {
        if !matches!(
            event,
            Event::Mouse(MouseEvent {
                kind: MouseKind::Down(MouseButton::Left),
                ..
            })
        ) {
            return EventResult::Ignored;
        }
        match self.0 {
            DownBehavior::CaptureAndIgnore => {
                ctx.capture_pointer(MouseButton::Left);
                EventResult::Ignored
            }
            DownBehavior::Consume => EventResult::Consumed,
            DownBehavior::Emit => EventResult::Emit(FocusTestMsg::Activated(ctx.path().to_vec())),
        }
    }

    fn scope_options(&self) -> ScopeOptions {
        ScopeOptions::default().focusable(true)
    }
}

#[derive(Clone, Copy)]
struct LifecycleDrag {
    offset: CellOffset,
    can_start: bool,
}

impl Component<PointerState, PointerMsg> for LifecycleDrag {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, PointerState, PointerMsg>) {}

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &PointerState,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<PointerMsg> {
        let Event::Mouse(mouse) = event else {
            return EventResult::Ignored;
        };
        match ctx.drag(
            mouse,
            DragOptions::new(self.offset).start_if(self.can_start),
        ) {
            DragPhase::Ignored => EventResult::Ignored,
            phase => EventResult::Emit(PointerMsg::Drag(phase)),
        }
    }
}

struct StringTransient;

impl Component<PointerState, PointerMsg> for StringTransient {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, PointerState, PointerMsg>) {}

    fn handle_event(
        &mut self,
        _event: &Event,
        _state: &PointerState,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<PointerMsg> {
        ctx.transient::<String>().push('x');
        EventResult::Consumed
    }
}

struct NumberTransient;

impl Component<PointerState, PointerMsg> for NumberTransient {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, PointerState, PointerMsg>) {}

    fn handle_event(
        &mut self,
        _event: &Event,
        _state: &PointerState,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<PointerMsg> {
        *ctx.transient::<usize>() += 1;
        EventResult::Consumed
    }
}

struct TransientProbe;

impl Component<PointerState, PointerMsg> for TransientProbe {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, PointerState, PointerMsg>) {}

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &PointerState,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<PointerMsg> {
        if !matches!(
            event,
            Event::Mouse(MouseEvent {
                kind: MouseKind::Down(_),
                ..
            })
        ) {
            return EventResult::Ignored;
        }
        let value = ctx.transient::<usize>();
        *value += 1;
        EventResult::Emit(PointerMsg::Transient(*value))
    }
}

#[derive(Default)]
struct CleanupTransient {
    dropped: Option<Arc<AtomicBool>>,
}

impl Drop for CleanupTransient {
    fn drop(&mut self) {
        if let Some(dropped) = &self.dropped {
            dropped.store(true, Ordering::SeqCst);
        }
    }
}

struct CleanupComponent {
    transient_dropped: Arc<AtomicBool>,
    component_dropped: Arc<AtomicBool>,
    /// Set by the instance that handled the event. Structure-pass
    /// instances are ephemeral — constructed, rendered paint-suppressed,
    /// and dropped without ever seeing an event — so only the retained,
    /// event-handling instance carries the cleanup-ordering assertion.
    armed: bool,
}

impl Drop for CleanupComponent {
    fn drop(&mut self) {
        if self.armed {
            assert!(
                self.transient_dropped.load(Ordering::SeqCst),
                "transient cleanup must finish before the previous component drops"
            );
            self.component_dropped.store(true, Ordering::SeqCst);
        }
    }
}

impl Component<PointerState, PointerMsg> for CleanupComponent {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, PointerState, PointerMsg>) {}

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &PointerState,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<PointerMsg> {
        if matches!(
            event,
            Event::Mouse(MouseEvent {
                kind: MouseKind::Down(MouseButton::Left),
                ..
            })
        ) {
            ctx.capture_pointer(MouseButton::Left);
            ctx.transient::<CleanupTransient>().dropped = Some(Arc::clone(&self.transient_dropped));
            self.armed = true;
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }
}

struct RecordingPointer {
    name: &'static str,
    events: Rc<RefCell<Vec<(&'static str, MouseKind)>>>,
    capture: bool,
}

impl Component<PointerState, PointerMsg> for RecordingPointer {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, PointerState, PointerMsg>) {}

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &PointerState,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<PointerMsg> {
        let Event::Mouse(mouse) = event else {
            return EventResult::Ignored;
        };
        if self.capture
            && let MouseKind::Down(button) = mouse.kind
        {
            ctx.capture_pointer(button);
        }
        self.events.borrow_mut().push((self.name, mouse.kind));
        EventResult::Consumed
    }
}

fn render_lifecycle_drag(
    driver: &mut Driver<PointerState, PointerMsg>,
    state: &PointerState,
    component: Option<LifecycleDrag>,
    area: Rect,
) {
    driver.render(state, |ctx| {
        if let Some(component) = component {
            ctx.component(ChildId::Static("drag"), component, area);
        }
    });
}

#[test]
fn drag_helper_stays_captured_across_rebuild_and_ends_outside() {
    let state = PointerState;
    let mut driver = Driver::new(20, 4);
    render_lifecycle_drag(
        &mut driver,
        &state,
        Some(LifecycleDrag {
            offset: CellOffset::new(3, -1),
            can_start: true,
        }),
        Rect::new(0, 0, 4, 2),
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state),
        EventResult::Emit(PointerMsg::Drag(DragPhase::Down))
    );

    render_lifecycle_drag(
        &mut driver,
        &state,
        Some(LifecycleDrag {
            offset: CellOffset::default(),
            can_start: false,
        }),
        Rect::new(10, 0, 4, 2),
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 19, 3), &state),
        EventResult::Emit(PointerMsg::Drag(DragPhase::Moved {
            offset: CellOffset::new(21, 1),
            position: Position::new(19, 3),
        }))
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Up(MouseButton::Left), 19, 3), &state),
        EventResult::Emit(PointerMsg::Drag(DragPhase::Ended {
            position: Position::new(19, 3),
            moved: true,
        }))
    );
    assert!(driver.ratcn.transients.is_empty());
    assert!(driver.ratcn.capture_path(MouseButton::Left).is_none());
    assert_eq!(
        driver.event(mouse(MouseKind::Drag(MouseButton::Left), 19, 3), &state),
        EventResult::Ignored
    );
}

#[test]
fn drag_helper_path_removal_cleans_transient_and_suppresses_capture() {
    let state = PointerState;
    let mut driver = Driver::new(20, 4);
    render_lifecycle_drag(
        &mut driver,
        &state,
        Some(LifecycleDrag {
            offset: CellOffset::default(),
            can_start: true,
        }),
        Rect::new(0, 0, 4, 2),
    );
    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);

    render_lifecycle_drag(&mut driver, &state, None, Rect::ZERO);
    assert!(driver.ratcn.transients.is_empty());
    assert!(driver.ratcn.capture_path(MouseButton::Left).is_none());
    assert!(driver.ratcn.is_suppressed(MouseButton::Left));
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 19, 3), &state),
        EventResult::Consumed
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Up(MouseButton::Left), 19, 3), &state),
        EventResult::Consumed
    );
    assert!(!driver.ratcn.is_suppressed(MouseButton::Left));
}

/// One button's gesture says nothing about another's. A left press that
/// claimed a component does not route the right button's release to it,
/// and a left gesture called off by a redraw does not swallow the right
/// button's events — each button is tracked, cancelled, and ended alone.
#[test]
fn one_buttons_gesture_never_routes_or_suppresses_another() {
    let state = PointerState;
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Driver::<PointerState, PointerMsg>::new(12, 2);
    let render = |driver: &mut Driver<PointerState, PointerMsg>, grabber| {
        driver.render(&state, |ctx| {
            if grabber {
                ctx.component(
                    ChildId::Static("grabber"),
                    RecordingPointer {
                        name: "grabber",
                        events: Rc::clone(&events),
                        capture: true,
                    },
                    Rect::new(0, 0, 5, 2),
                );
            }
            ctx.component(
                ChildId::Static("other"),
                RecordingPointer {
                    name: "other",
                    events: Rc::clone(&events),
                    capture: false,
                },
                Rect::new(6, 0, 6, 2),
            );
        });
    };

    render(&mut driver, true);
    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state);
    assert_eq!(
        driver.ratcn.capture_path(MouseButton::Left),
        Some([ChildId::Static("grabber")].as_slice())
    );
    assert!(
        driver.ratcn.capture_path(MouseButton::Right).is_none(),
        "the right button claimed nothing"
    );

    // The right button's release is routed by its own gesture, not by the
    // left button's claim.
    events.borrow_mut().clear();
    driver.event(mouse(MouseKind::Down(MouseButton::Right), 8, 0), &state);
    driver.event(mouse(MouseKind::Up(MouseButton::Right), 8, 0), &state);
    assert!(
        events.borrow().iter().all(|&(name, _)| name == "other"),
        "the right button's gesture belongs to what it hit"
    );

    // The left gesture's component disappears: that button is called off,
    // and only that button.
    render(&mut driver, false);
    assert!(driver.ratcn.is_suppressed(MouseButton::Left));
    assert!(!driver.ratcn.is_suppressed(MouseButton::Right));

    events.borrow_mut().clear();
    driver.event(mouse(MouseKind::Down(MouseButton::Right), 8, 0), &state);
    driver.event(mouse(MouseKind::Up(MouseButton::Right), 8, 0), &state);
    assert!(
        !events.borrow().is_empty(),
        "a suppressed left gesture must not swallow the right button"
    );

    events.borrow_mut().clear();
    assert_eq!(
        driver.event(mouse(MouseKind::Up(MouseButton::Left), 1, 0), &state),
        EventResult::Consumed,
        "the called-off left release is swallowed"
    );
    assert!(events.borrow().is_empty());
    assert!(
        !driver.ratcn.is_suppressed(MouseButton::Left),
        "and that release ends the gesture"
    );
}

#[test]
fn capture_and_transient_follow_identity_through_replacement_and_reorder() {
    let state = PointerState;
    let mut driver = Driver::new(20, 4);
    render_drag_surface(
        &mut driver,
        &state,
        &[
            ("drag", "old", Rect::new(0, 0, 4, 2)),
            ("other", "other", Rect::new(5, 0, 4, 2)),
        ],
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state),
        EventResult::Consumed
    );

    render_drag_surface(
        &mut driver,
        &state,
        &[
            ("other", "other", Rect::new(5, 0, 4, 2)),
            ("drag", "replacement", Rect::new(10, 0, 4, 2)),
        ],
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 19, 3), &state),
        EventResult::Emit(PointerMsg::Routed(
            "replacement",
            MouseKind::Drag(MouseButton::Left),
            2,
        ))
    );
}

#[test]
fn raw_release_returns_its_first_emitted_normalized_event() {
    let state = PointerState;
    let mut driver = Driver::new(20, 4);
    render_drag_surface(
        &mut driver,
        &state,
        &[("drag", "drag", Rect::new(0, 0, 4, 2))],
    );
    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
    driver.event(mouse(MouseKind::Moved, 19, 3), &state);

    assert_eq!(
        driver.event(mouse(MouseKind::Up(MouseButton::Left), 19, 3), &state),
        EventResult::Emit(PointerMsg::Routed(
            "drag",
            MouseKind::Up(MouseButton::Left),
            3,
        ))
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Drag(MouseButton::Left), 19, 3), &state),
        EventResult::Ignored
    );
}

#[test]
fn pointer_exit_cancels_capture_and_stale_press_before_reentry() {
    let state = PointerState;
    let mut driver = Driver::new(20, 4);
    render_drag_surface(
        &mut driver,
        &state,
        &[("drag", "drag", Rect::new(0, 0, 4, 2))],
    );
    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
    assert!(driver.ratcn.capture_path(MouseButton::Left).is_some());
    assert!(
        driver
            .ratcn
            .gestures
            .iter()
            .any(|gesture| gesture.press.is_some())
    );

    assert_eq!(
        driver.event(mouse(MouseKind::Exited, 1, 1), &state),
        EventResult::Consumed
    );
    assert!(driver.ratcn.gestures.is_empty());

    // Re-entry is plain motion: it moves hover (which the exit emptied)
    // and nothing else — a surviving press would have made it a drag, and
    // the component would have emitted.
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 1, 1), &state),
        EventResult::Consumed
    );
}

#[test]
fn disappearing_capture_is_suppressed_through_reappearance_until_release() {
    let state = PointerState;
    let mut driver = Driver::new(20, 4);
    render_drag_surface(
        &mut driver,
        &state,
        &[("drag", "before", Rect::new(0, 0, 4, 2))],
    );
    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
    render_drag_surface(&mut driver, &state, &[]);
    render_drag_surface(
        &mut driver,
        &state,
        &[("drag", "after", Rect::new(0, 0, 4, 2))],
    );

    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 1, 1), &state),
        EventResult::Consumed
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Up(MouseButton::Left), 1, 1), &state),
        EventResult::Consumed
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state),
        EventResult::Consumed
    );
}

#[test]
fn deferred_paint_failure_preserves_capture_transient_and_previous_component() {
    let state = PointerState;
    let mut driver = Driver::new(20, 4);
    render_drag_surface(
        &mut driver,
        &state,
        &[("drag", "stable", Rect::new(0, 0, 4, 2))],
    );
    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
    let result = catch_unwind(AssertUnwindSafe(|| {
        let area = driver.area();
        driver.render(&state, |ctx| {
            ctx.component(
                ChildId::Static("drag"),
                Draggable {
                    name: "replacement",
                },
                area,
            );
            ctx.defer_paint(|_| panic!("deferred paint failed"));
        });
    }));
    assert!(result.is_err());

    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 19, 3), &state),
        EventResult::Emit(PointerMsg::Routed(
            "stable",
            MouseKind::Drag(MouseButton::Left),
            2,
        ))
    );
}

#[test]
fn incompatible_transient_reuse_reports_path_and_types() {
    let state = PointerState;
    let mut driver = Driver::new(5, 2);
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(ChildId::Static("typed"), StringTransient, area);
    });
    driver.event(mouse(MouseKind::Moved, 0, 0), &state);
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(ChildId::Static("typed"), NumberTransient, area);
    });

    let panic = catch_unwind(AssertUnwindSafe(|| {
        driver.event(mouse(MouseKind::Moved, 0, 0), &state);
    }));
    let payload = panic.expect_err("incompatible transient type must panic");
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .expect("string panic");
    assert!(message.contains("typed"));
    assert!(message.contains("alloc::string::String"));
    assert!(message.contains("usize"));
}

#[test]
fn successful_path_removal_drops_its_transient_state() {
    let state = PointerState;
    let mut driver = Driver::new(5, 2);
    let render_probe = |driver: &mut Driver<PointerState, PointerMsg>, present| {
        let area = driver.area();
        driver.render(&state, |ctx| {
            if present {
                ctx.component(ChildId::Static("probe"), TransientProbe, area);
            }
        });
    };

    render_probe(&mut driver, true);
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 0, 0), &state),
        EventResult::Emit(PointerMsg::Transient(1))
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 0, 0), &state),
        EventResult::Emit(PointerMsg::Transient(2))
    );
    render_probe(&mut driver, false);
    render_probe(&mut driver, true);
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 0, 0), &state),
        EventResult::Emit(PointerMsg::Transient(1))
    );
}

#[test]
fn capture_and_transient_cleanup_finish_before_previous_component_drop() {
    let state = PointerState;
    let transient_dropped = Arc::new(AtomicBool::new(false));
    let component_dropped = Arc::new(AtomicBool::new(false));
    let mut driver = Driver::new(5, 2);

    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(
            ChildId::Static("cleanup"),
            CleanupComponent {
                transient_dropped: Arc::clone(&transient_dropped),
                component_dropped: Arc::clone(&component_dropped),
                armed: false,
            },
            area,
        );
    });
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 0, 0), &state,),
        EventResult::Consumed
    );

    driver.render(&state, |_| {});

    assert!(transient_dropped.load(Ordering::SeqCst));
    assert!(component_dropped.load(Ordering::SeqCst));
    assert!(driver.ratcn.capture_path(MouseButton::Left).is_none());
    assert!(driver.ratcn.is_suppressed(MouseButton::Left));
}

#[test]
fn reverse_paint_order_routes_overlap_to_the_topmost_component() {
    let state = PointerState;
    let mut driver = Driver::new(5, 2);
    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(ChildId::Static("bottom"), RouteLeaf("bottom"), area);
        ctx.component(ChildId::Static("top"), RouteLeaf("top"), area);
    });

    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 0, 0), &state),
        EventResult::Emit(PointerMsg::Routed(
            "top",
            MouseKind::Down(MouseButton::Left),
            0,
        ))
    );
}

#[test]
fn successful_redraw_removes_click_target_without_retargeting_its_old_geometry() {
    let state = PointerState;
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Driver::new(10, 2);
    driver.render(&state, |ctx| {
        ctx.component(
            ChildId::Static("removed"),
            RecordingPointer {
                name: "removed",
                events: Rc::clone(&events),
                capture: false,
            },
            Rect::new(0, 0, 4, 2),
        );
    });
    driver.render(&state, |ctx| {
        ctx.component(
            ChildId::Static("other"),
            RecordingPointer {
                name: "other",
                events: Rc::clone(&events),
                capture: false,
            },
            Rect::new(6, 0, 4, 2),
        );
    });

    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state),
        EventResult::Ignored
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Up(MouseButton::Left), 1, 1), &state),
        EventResult::Ignored
    );
    assert!(events.borrow().is_empty());

    driver.event(mouse(MouseKind::Down(MouseButton::Left), 7, 1), &state);
    driver.event(mouse(MouseKind::Up(MouseButton::Left), 7, 1), &state);
    assert_eq!(
        *events.borrow(),
        [
            ("other", MouseKind::Down(MouseButton::Left)),
            ("other", MouseKind::Up(MouseButton::Left)),
            ("other", MouseKind::Click(MouseButton::Left)),
        ]
    );
}

#[test]
fn uncaptured_click_does_not_retarget_after_successful_redraw() {
    let state = PointerState;
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Driver::new(5, 2);
    let render = |driver: &mut Driver<PointerState, PointerMsg>, id, name| {
        let area = driver.area();
        driver.render(&state, |ctx| {
            ctx.component(
                ChildId::Static(id),
                RecordingPointer {
                    name,
                    events: Rc::clone(&events),
                    capture: false,
                },
                area,
            );
        });
    };

    render(&mut driver, "before", "before");
    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
    render(&mut driver, "after", "after");
    assert_eq!(
        driver.event(mouse(MouseKind::Up(MouseButton::Left), 1, 1), &state),
        EventResult::Consumed
    );
    assert_eq!(
        *events.borrow(),
        [
            ("before", MouseKind::Down(MouseButton::Left)),
            ("after", MouseKind::Up(MouseButton::Left)),
        ]
    );

    events.borrow_mut().clear();
    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
    driver.event(mouse(MouseKind::Up(MouseButton::Left), 1, 1), &state);
    assert_eq!(
        *events.borrow(),
        [
            ("after", MouseKind::Down(MouseButton::Left)),
            ("after", MouseKind::Up(MouseButton::Left)),
            ("after", MouseKind::Click(MouseButton::Left)),
        ]
    );
}

#[test]
fn release_after_successful_rebuild_clicks_the_same_stable_identity_once() {
    let state = PointerState;
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Driver::new(5, 2);
    let render = |driver: &mut Driver<PointerState, PointerMsg>, id, name| {
        let area = driver.area();
        driver.render(&state, |ctx| {
            ctx.component(
                ChildId::Static(id),
                RecordingPointer {
                    name,
                    events: Rc::clone(&events),
                    capture: true,
                },
                area,
            );
        });
    };

    render(&mut driver, "stable", "before");
    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
    render(&mut driver, "stable", "replacement");
    driver.event(mouse(MouseKind::Up(MouseButton::Left), 1, 1), &state);

    assert_eq!(
        *events.borrow(),
        [
            ("before", MouseKind::Down(MouseButton::Left)),
            ("replacement", MouseKind::Up(MouseButton::Left)),
            ("replacement", MouseKind::Click(MouseButton::Left)),
        ]
    );

    events.borrow_mut().clear();
    render(&mut driver, "stable", "before");
    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
    render(&mut driver, "different", "replacement");
    assert_eq!(
        driver.event(mouse(MouseKind::Up(MouseButton::Left), 1, 1), &state),
        EventResult::Consumed
    );
    assert_eq!(
        *events.borrow(),
        [("before", MouseKind::Down(MouseButton::Left))]
    );
}

/// Two neighbours with a gap between them, for the press-drift cases: a
/// click has to survive movement inside one of them and has to die when
/// the pointer leaves for the other.
fn render_neighbours(
    driver: &mut Driver<PointerState, PointerMsg>,
    events: &Rc<RefCell<Vec<(&'static str, MouseKind)>>>,
    capture: bool,
) {
    let state = PointerState;
    driver.render(&state, |ctx| {
        ctx.component(
            ChildId::Static("left"),
            RecordingPointer {
                name: "left",
                events: Rc::clone(events),
                capture,
            },
            Rect::new(0, 0, 4, 2),
        );
        ctx.component(
            ChildId::Static("right"),
            RecordingPointer {
                name: "right",
                events: Rc::clone(events),
                capture,
            },
            Rect::new(6, 0, 4, 2),
        );
    });
}

#[test]
fn a_press_that_drifts_inside_one_component_still_clicks_it() {
    // The pointer moved a column while held, which is enough to emit
    // `Drag` — but the release is still on the component the press hit and
    // nobody claimed the gesture, so the click stands. A cell-exact rule
    // would silently drop this press, which is the ordinary way a real
    // mouse behaves.
    let state = PointerState;
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Driver::new(10, 2);
    render_neighbours(&mut driver, &events, false);

    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
    driver.event(mouse(MouseKind::Moved, 2, 1), &state);
    driver.event(mouse(MouseKind::Up(MouseButton::Left), 2, 1), &state);

    assert_eq!(
        *events.borrow(),
        [
            ("left", MouseKind::Down(MouseButton::Left)),
            ("left", MouseKind::Drag(MouseButton::Left)),
            ("left", MouseKind::Up(MouseButton::Left)),
            ("left", MouseKind::Click(MouseButton::Left)),
        ]
    );
}

#[test]
fn a_press_released_on_another_component_clicks_neither() {
    let state = PointerState;
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Driver::new(10, 2);
    render_neighbours(&mut driver, &events, false);

    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
    driver.event(mouse(MouseKind::Moved, 7, 1), &state);
    driver.event(mouse(MouseKind::Up(MouseButton::Left), 7, 1), &state);

    let log = events.borrow();
    assert!(
        !log.iter()
            .any(|(_, kind)| matches!(kind, MouseKind::Click(_))),
        "a release off the press target must not click anything: {log:?}"
    );
    assert!(
        log.contains(&("right", MouseKind::DragEnd(MouseButton::Left))),
        "the gesture ends as a drag instead: {log:?}"
    );
}

/// Empty space is where a press can land like any other, and a release
/// somewhere else is no more a click for having started on nothing.
#[test]
fn a_press_that_started_on_empty_space_clicks_nothing_else() {
    let state = PointerState;
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Driver::new(10, 2);
    render_neighbours(&mut driver, &events, false);

    // Column 5 is the gap between the two components.
    driver.event(mouse(MouseKind::Down(MouseButton::Left), 5, 1), &state);
    driver.event(mouse(MouseKind::Up(MouseButton::Left), 1, 1), &state);

    let log = events.borrow();
    assert!(
        !log.iter()
            .any(|(_, kind)| matches!(kind, MouseKind::Click(_))),
        "the press landed on nothing, so this release clicks nothing: {log:?}"
    );
}

#[test]
fn a_claimed_gesture_that_moved_ends_as_a_drag_not_a_click() {
    // Same drift as `a_press_that_drifts_inside_one_component_still_clicks_it`,
    // but the component claimed the pointer on `Down`. Claiming declares
    // the movement meaningful, so this is a drag.
    let state = PointerState;
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Driver::new(10, 2);
    render_neighbours(&mut driver, &events, true);

    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
    driver.event(mouse(MouseKind::Moved, 2, 1), &state);
    driver.event(mouse(MouseKind::Up(MouseButton::Left), 2, 1), &state);

    assert_eq!(
        *events.borrow(),
        [
            ("left", MouseKind::Down(MouseButton::Left)),
            ("left", MouseKind::Drag(MouseButton::Left)),
            ("left", MouseKind::Up(MouseButton::Left)),
            ("left", MouseKind::DragEnd(MouseButton::Left)),
        ]
    );
}

#[test]
fn a_claimed_press_that_never_moved_is_still_a_click() {
    // The other half of the capture rule: claiming the pointer must not
    // cost a component its plain clicks, or nothing can both drag and be
    // clicked.
    let state = PointerState;
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Driver::new(10, 2);
    render_neighbours(&mut driver, &events, true);

    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
    driver.event(mouse(MouseKind::Up(MouseButton::Left), 1, 1), &state);

    assert_eq!(
        *events.borrow(),
        [
            ("left", MouseKind::Down(MouseButton::Left)),
            ("left", MouseKind::Up(MouseButton::Left)),
            ("left", MouseKind::Click(MouseButton::Left)),
        ]
    );
}

/// Motion that has not left the pressed cell is not yet a drag, so
/// nothing is synthesized from it — but the runtime still answers for it,
/// because the host has to redraw whether or not the pointer crossed a
/// cell boundary.
#[test]
fn motion_inside_the_pressed_cell_synthesizes_nothing() {
    let state = PointerState;
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Driver::new(10, 2);
    render_neighbours(&mut driver, &events, false);

    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 1, 1), &state),
        EventResult::Consumed
    );
    driver.event(mouse(MouseKind::Up(MouseButton::Left), 1, 1), &state);

    assert_eq!(
        *events.borrow(),
        [
            ("left", MouseKind::Down(MouseButton::Left)),
            ("left", MouseKind::Up(MouseButton::Left)),
            ("left", MouseKind::Click(MouseButton::Left)),
        ]
    );
}

/// A release somewhere else ends a gesture that never moved as a plain
/// `Up`: there was no drag to end, and it landed on nothing it could
/// click.
#[test]
fn a_still_press_released_elsewhere_is_only_an_up() {
    let state = PointerState;
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Driver::new(10, 2);
    render_neighbours(&mut driver, &events, false);

    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
    driver.event(mouse(MouseKind::Up(MouseButton::Left), 7, 1), &state);

    assert_eq!(
        *events.borrow(),
        [
            ("left", MouseKind::Down(MouseButton::Left)),
            ("right", MouseKind::Up(MouseButton::Left)),
        ]
    );
}

/// crossterm reports `Drag` itself, where other backends report motion
/// under a held button. It marks the press as moved just as synthesized
/// motion does, so the release still ends as a drag.
#[test]
fn a_backend_reported_drag_still_ends_as_a_drag() {
    let state = PointerState;
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Driver::new(10, 2);
    render_neighbours(&mut driver, &events, false);

    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
    driver.event(mouse(MouseKind::Drag(MouseButton::Left), 7, 1), &state);
    driver.event(mouse(MouseKind::Up(MouseButton::Left), 7, 1), &state);

    assert_eq!(
        *events.borrow(),
        [
            ("left", MouseKind::Down(MouseButton::Left)),
            ("right", MouseKind::Drag(MouseButton::Left)),
            ("right", MouseKind::Up(MouseButton::Left)),
            ("right", MouseKind::DragEnd(MouseButton::Left)),
        ]
    );
}

/// Motion belongs to the button pressed most recently, and the release of
/// that button hands it back to one still held — so a second button
/// pressed mid-drag does not strand the first.
#[test]
fn motion_belongs_to_the_latest_press_and_returns_to_the_one_still_held() {
    let state = PointerState;
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Driver::new(10, 2);
    render_neighbours(&mut driver, &events, false);

    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
    driver.event(mouse(MouseKind::Down(MouseButton::Right), 7, 1), &state);
    events.borrow_mut().clear();

    driver.event(mouse(MouseKind::Moved, 8, 1), &state);
    driver.event(mouse(MouseKind::Up(MouseButton::Right), 8, 1), &state);
    driver.event(mouse(MouseKind::Moved, 2, 1), &state);

    assert_eq!(
        *events.borrow(),
        [
            ("right", MouseKind::Drag(MouseButton::Right)),
            ("right", MouseKind::Up(MouseButton::Right)),
            ("right", MouseKind::Click(MouseButton::Right)),
            ("left", MouseKind::Drag(MouseButton::Left)),
        ]
    );
}

/// The other release order: the button pressed *first* goes up while a
/// later one is still held. Its own gesture ends, and motion stays with
/// the button still down.
#[test]
fn releasing_the_earlier_press_leaves_motion_with_the_later_one() {
    let state = PointerState;
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Driver::new(10, 2);
    render_neighbours(&mut driver, &events, false);

    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
    driver.event(mouse(MouseKind::Down(MouseButton::Right), 7, 1), &state);
    events.borrow_mut().clear();

    driver.event(mouse(MouseKind::Up(MouseButton::Left), 1, 1), &state);
    driver.event(mouse(MouseKind::Moved, 8, 1), &state);

    assert_eq!(
        *events.borrow(),
        [
            ("left", MouseKind::Up(MouseButton::Left)),
            ("left", MouseKind::Click(MouseButton::Left)),
            ("right", MouseKind::Drag(MouseButton::Right)),
        ]
    );
}

#[test]
fn area_scope_hit_prefers_descendant_then_falls_back_to_scope() {
    let state = FocusTestState::default();
    let mut driver = Driver::<FocusTestState, FocusTestMsg>::new(8, 2);
    driver.render(&state, |ctx| {
        ctx.scope(
            ChildId::Static("scope"),
            Rect::new(0, 0, 8, 2),
            ScopeOptions::default(),
            |ctx| {
                ctx.component(
                    ChildId::Static("child"),
                    FocusLeaf::enabled(),
                    Rect::new(0, 0, 3, 2),
                );
            },
        );
    });

    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 1, 0), &state),
        EventResult::Consumed
    );
    assert_eq!(
        driver.ratcn.hover_path(),
        [ChildId::Static("scope"), ChildId::Static("child")],
        "the descendant wins the enclosing scope hit"
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 6, 0), &state),
        EventResult::Consumed
    );
    assert_eq!(
        driver.ratcn.hover_path(),
        [ChildId::Static("scope")],
        "off the child, the scope answers for its own area"
    );
}

#[test]
fn raw_button_press_focuses_then_synthesized_click_emits() {
    let mut state = ButtonTimingState {
        focus: FocusState::intent([ChildId::Static("first")]),
        ..ButtonTimingState::default()
    };
    let mut driver = Driver::with(
        Ratcn::new().focus(
            |state: &ButtonTimingState| &state.focus,
            ButtonTimingMsg::Focus,
        ),
        20,
        2,
    );
    driver.render(&state, |ctx| {
        ctx.component(
            ChildId::Static("first"),
            Button::new("First").on_press(|| ButtonTimingMsg::Replacement),
            Rect::new(0, 0, 8, 2),
        );
        ctx.component(
            ChildId::Static("second"),
            Button::new("Second").on_press(|| ButtonTimingMsg::Save),
            Rect::new(10, 0, 8, 2),
        );
    });

    for button in [MouseButton::Right, MouseButton::Middle] {
        assert_eq!(
            driver.event(mouse(MouseKind::Down(button), 11, 0), &state),
            EventResult::Ignored
        );
        assert_eq!(
            driver.event(mouse(MouseKind::Up(button), 11, 0), &state),
            EventResult::Ignored
        );
        assert_eq!(state.focus.path(), &[ChildId::Static("first")]);
    }

    let EventResult::Emit(focus) =
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 11, 0), &state)
    else {
        panic!("button down did not request focus");
    };
    assert!(update_button_timing(&mut state, focus));
    assert_eq!(state.focus.path(), &[ChildId::Static("second")]);
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 11, 0), &state),
        EventResult::Consumed
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Up(MouseButton::Left), 11, 0), &state),
        EventResult::Emit(ButtonTimingMsg::Save)
    );
}

#[test]
fn primary_down_result_controls_focus_fallback_after_capture_and_routing() {
    let state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("first")]),
    };
    let mut driver = focus_driver(16, 2);
    driver.render(&state, |ctx| {
        ctx.component(
            ChildId::Static("first"),
            FocusLeaf::enabled(),
            Rect::new(0, 0, 3, 1),
        );
        for (id, x, behavior) in [
            ("ignored", 4, DownBehavior::CaptureAndIgnore),
            ("consumed", 8, DownBehavior::Consume),
            ("emitted", 12, DownBehavior::Emit),
        ] {
            ctx.component(
                ChildId::Static(id),
                DownFocusLeaf(behavior),
                Rect::new(x, 0, 3, 1),
            );
        }
    });

    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 4, 0), &state),
        EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([ChildId::Static(
            "ignored"
        )])))
    );
    assert_eq!(
        driver.ratcn.capture_path(MouseButton::Left),
        Some([ChildId::Static("ignored")].as_slice())
    );
    driver.event(mouse(MouseKind::Exited, 4, 0), &state);
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 8, 0), &state),
        EventResult::Consumed
    );
    driver.event(mouse(MouseKind::Exited, 8, 0), &state);
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 12, 0), &state),
        EventResult::Emit(FocusTestMsg::Activated(vec![ChildId::Static("emitted")]))
    );
}

#[test]
fn mouse_before_the_first_render_is_ignored_without_arming_the_tracker() {
    let state = PointerState;
    let mut driver = Driver::new(5, 2);
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 0, 0), &state),
        EventResult::Ignored
    );
    render_drag_surface(
        &mut driver,
        &state,
        &[("drag", "drag", Rect::new(0, 0, 5, 2))],
    );
    // Plain motion, not a drag: the press before the first render armed
    // nothing, so this only moves hover, which is what `Consumed` reports.
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 0, 0), &state),
        EventResult::Consumed
    );
}
