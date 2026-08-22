//! What a declaration pass builds: identity, paths, and the atomicity of a
//! pass that fails part way through.

use super::*;

struct ContextProbe {
    area: Rect,
}

impl Component<u8, ()> for ContextProbe {
    fn declare(&mut self, ctx: &mut DeclareCtx<'_, u8, ()>) {
        assert_eq!(ctx.area(), self.area);
        assert_eq!(*ctx.state(), 7);
    }

    fn is_focusable(&self) -> bool {
        true
    }
}

struct Composite;

impl Component<(), ()> for Composite {
    fn declare(&mut self, ctx: &mut DeclareCtx<'_, (), ()>) {
        let area = ctx.area();
        ctx.component(ChildId::Static("leaf"), Leaf, area);
    }

    fn scope_options(&self) -> ScopeOptions {
        ScopeOptions::default().tab_wrap(TabWrap::Wrap)
    }
}

struct PanickingLeaf;

impl Component<(), ()> for PanickingLeaf {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, (), ()>) {
        panic!("leaf render failed");
    }
}

struct PanickingScopeOptions;

impl Component<(), ()> for PanickingScopeOptions {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, (), ()>) {}

    fn scope_options(&self) -> ScopeOptions {
        panic!("scope options failed");
    }
}

struct PanickingResolve;

impl Component<(), ()> for PanickingResolve {
    fn prepare(&mut self, _state: &()) {
        panic!("declaration prop resolution failed");
    }

    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, (), ()>) {}
}

struct PanickingFocusable;

impl Component<(), ()> for PanickingFocusable {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, (), ()>) {}

    fn is_focusable(&self) -> bool {
        panic!("focusability failed");
    }
}

struct PanickingInteractionArea;

impl Component<(), ()> for PanickingInteractionArea {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, (), ()>) {}

    fn interaction_area(&self, _area: Rect) -> Rect {
        panic!("interaction area failed");
    }
}

struct EscapingInteractionArea;

impl Component<(), ()> for EscapingInteractionArea {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, (), ()>) {}

    fn interaction_area(&self, area: Rect) -> Rect {
        Rect::new(area.x, area.y, area.width.saturating_add(1), area.height)
    }
}

struct CatchingComposite;

impl Component<(), ()> for CatchingComposite {
    fn declare(&mut self, ctx: &mut DeclareCtx<'_, (), ()>) {
        let area = ctx.area();
        let caught = catch_unwind(AssertUnwindSafe(|| {
            ctx.component(ChildId::Static("panicking-child"), PanickingLeaf, area);
        }));
        assert!(caught.is_err());
        ctx.component(ChildId::Static("later-child"), Leaf, area);
    }
}

type PathLog = Rc<RefCell<Vec<Vec<ChildId>>>>;

/// Record the identity path the declaration pass currently has open.
fn record_declared_path(ctx: &DeclareCtx<'_, FocusTestState, FocusTestMsg>, log: &PathLog) {
    if let Some(path) = ctx.pass.current_path() {
        log.borrow_mut().push(path.to_vec());
    }
}

/// A focusable leaf that records where it was declared. The counterpart to
/// [`record_declared_path`] for nodes that are components rather than
/// scopes.
struct PathProbe(PathLog);

impl Component<FocusTestState, FocusTestMsg> for PathProbe {
    fn declare(&mut self, ctx: &mut DeclareCtx<'_, FocusTestState, FocusTestMsg>) {
        record_declared_path(ctx, &self.0);
    }

    fn is_focusable(&self) -> bool {
        true
    }
}

/// The two leaves at the bottom of a depth-four branch, plus the record
/// for the scope they were declared into.
fn declare_probe_cells(
    ctx: &mut DeclareCtx<'_, FocusTestState, FocusTestMsg>,
    top: u16,
    log: &PathLog,
) {
    record_declared_path(ctx, log);
    for (row, id) in [(top, "cell-1"), (top + 1, "cell-2")] {
        ctx.component(
            ChildId::from(id.to_owned()),
            PathProbe(Rc::clone(log)),
            Rect::new(0, row, 20, 1),
        );
    }
}

struct AreaAwareComposite {
    expected_area: Rect,
    minimum_width: u16,
    rendered: Arc<AtomicBool>,
}

impl Component<FocusTestState, FocusTestMsg> for AreaAwareComposite {
    fn declare(&mut self, ctx: &mut DeclareCtx<'_, FocusTestState, FocusTestMsg>) {
        assert_eq!(ctx.area(), self.expected_area);
        self.rendered.store(true, Ordering::SeqCst);
        ctx.component(ChildId::Static("child"), FocusLeaf::enabled(), ctx.area());
    }

    fn scope_options(&self) -> ScopeOptions {
        ScopeOptions::default()
    }

    fn interaction_area(&self, area: Rect) -> Rect {
        assert_eq!(area, self.expected_area);
        if area.width >= self.minimum_width {
            area
        } else {
            Rect::default()
        }
    }
}

fn hash(id: &ChildId) -> u64 {
    let mut hasher = DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}

#[test]
fn static_and_dynamic_ids_share_content_identity_and_allocation() {
    let shared: Arc<str> = Arc::from("row:42");
    let dynamic = ChildId::Dynamic(Arc::clone(&shared));
    let cloned = dynamic.clone();
    let static_id = ChildId::Static("row:42");

    assert_eq!(static_id, dynamic);
    assert_eq!(hash(&static_id), hash(&dynamic));
    assert_eq!(static_id.cmp(&dynamic), std::cmp::Ordering::Equal);
    let ChildId::Dynamic(cloned_shared) = cloned else {
        panic!("dynamic id changed representation");
    };
    assert!(Arc::ptr_eq(&shared, &cloned_shared));
}

#[test]
fn render_context_reports_each_declaration_area_and_state() {
    let state = 7;
    let scope_area = Rect::new(1, 0, 8, 3);
    let component_area = Rect::new(2, 1, 3, 1);
    let modal_area = Rect::new(0, 0, 10, 3);
    let mut driver = Driver::<u8, ()>::new(10, 3);

    let scope_contains_focus = Rc::new(RefCell::new(Vec::new()));
    let root_area = driver.area();
    driver.render(&state, |ctx| {
        assert_eq!(ctx.area(), root_area);
        assert_eq!(*ctx.state(), state);
        ctx.paint(|ctx| assert!(!ctx.contains_focus()));
        let scope_contains_focus = Rc::clone(&scope_contains_focus);
        ctx.scope(
            ChildId::Static("scope"),
            scope_area,
            ScopeOptions::default(),
            move |ctx| {
                assert_eq!(ctx.area(), scope_area);
                assert_eq!(*ctx.state(), state);
                ctx.paint(move |ctx| {
                    assert_eq!(ctx.area(), scope_area);
                    scope_contains_focus.borrow_mut().push(ctx.contains_focus());
                });
                ctx.component(
                    ChildId::Static("probe"),
                    ContextProbe {
                        area: component_area,
                    },
                    component_area,
                );
            },
        );
        ctx.modal(
            ChildId::Static("modal"),
            ContextProbe { area: modal_area },
            modal_area,
        );
    });
    // Paint runs once, after focus resolves: the modal takes it, so the
    // base scope does not contain it.
    assert_eq!(*scope_contains_focus.borrow(), [false]);
}

#[test]
fn composite_declaration_builds_paths_and_scope_options() {
    let mut driver = Driver::<(), ()>::new(10, 3);

    let area = driver.area();
    driver.render(&(), |ctx| {
        ctx.component(ChildId::Static("composite"), Composite, area);
    });

    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![
            vec![ChildId::Static("composite")],
            vec![ChildId::Static("composite"), ChildId::Static("leaf")],
        ]
    );
    assert_eq!(driver.ratcn.surface.roots, vec![0]);
    assert_eq!(driver.ratcn.surface.nodes[0].children, vec![1]);
    assert_eq!(driver.ratcn.surface.nodes[1].parent, Some(0));
    assert_eq!(
        driver.ratcn.surface.nodes[0].options.tab_wrap,
        TabWrap::Wrap
    );
    assert!(
        driver
            .ratcn
            .surface
            .nodes
            .iter()
            .all(|node| node.component.is_some())
    );
}

#[test]
fn duplicate_sibling_ids_panic_without_replacing_the_surface() {
    let mut driver = Driver::<(), ()>::new(10, 3);
    render_leaf(&mut driver, &ChildId::Static("previous"));

    let result = catch_unwind(AssertUnwindSafe(|| {
        let area = driver.area();
        driver.render(&(), |ctx| {
            ctx.component(ChildId::Static("duplicate"), Leaf, area);
            ctx.component(ChildId::Static("duplicate"), Leaf, area);
        });
    }));

    assert!(result.is_err());
    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![vec![ChildId::Static("previous")]]
    );
}

#[test]
fn same_child_id_in_distinct_scopes_builds_and_routes_distinct_paths() {
    let mut state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("left"), ChildId::Static("shared")]),
    };
    let mut driver = focus_driver(10, 3);

    let area = driver.area();
    driver.render(&state, |ctx| {
        for scope in ["left", "right"] {
            ctx.scope(
                ChildId::Static(scope),
                Rect::ZERO,
                ScopeOptions::default(),
                |ctx| {
                    ctx.component(ChildId::Static("shared"), FocusLeaf::enabled(), area);
                },
            );
        }
    });

    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(FocusTestMsg::Activated(vec![
            ChildId::Static("left"),
            ChildId::Static("shared"),
        ]))
    );
    state.focus = FocusState::intent([ChildId::Static("right"), ChildId::Static("shared")]);
    assert_eq!(
        driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
        EventResult::Emit(FocusTestMsg::Activated(vec![
            ChildId::Static("right"),
            ChildId::Static("shared"),
        ]))
    );
}

#[test]
fn declared_paths_match_the_declaration_for_a_depth_four_tree_with_layers_and_dynamic_ids() {
    // Two derivations of one identity have to agree: the cursor the
    // declaration pass carries down the tree, and the parent walk the
    // committed surface answers with afterwards. A layer boundary and two
    // branches reusing the same descendant ids are where they could
    // plausibly drift apart.
    let state = FocusTestState::default();
    let declared: PathLog = Rc::new(RefCell::new(Vec::new()));
    let mut driver = focus_driver(20, 6);

    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.scope(
            ChildId::Static("outer"),
            area,
            ScopeOptions::default(),
            |ctx| {
                record_declared_path(ctx, &declared);
                ctx.scope(
                    ChildId::from("row-1".to_owned()),
                    Rect::new(0, 0, 20, 3),
                    ScopeOptions::default(),
                    |ctx| {
                        record_declared_path(ctx, &declared);
                        let area = ctx.area();
                        ctx.modal_scope(
                            ChildId::Static("sheet"),
                            area,
                            ScopeOptions::default(),
                            |ctx| declare_probe_cells(ctx, 0, &declared),
                        );
                    },
                );
                // The same descendant ids again, under a different
                // row and outside the layer.
                ctx.scope(
                    ChildId::from("row-2".to_owned()),
                    Rect::new(0, 3, 20, 3),
                    ScopeOptions::default(),
                    |ctx| {
                        record_declared_path(ctx, &declared);
                        let area = ctx.area();
                        ctx.scope(
                            ChildId::Static("sheet"),
                            area,
                            ScopeOptions::default(),
                            |ctx| declare_probe_cells(ctx, 3, &declared),
                        );
                    },
                );
            },
        );
    });

    let path = |segments: &[&str]| {
        segments
            .iter()
            .map(|id| ChildId::from((*id).to_owned()))
            .collect::<Vec<_>>()
    };
    let expected = vec![
        path(&["outer"]),
        path(&["outer", "row-1"]),
        path(&["outer", "row-1", "sheet"]),
        path(&["outer", "row-1", "sheet", "cell-1"]),
        path(&["outer", "row-1", "sheet", "cell-2"]),
        path(&["outer", "row-2"]),
        path(&["outer", "row-2", "sheet"]),
        path(&["outer", "row-2", "sheet", "cell-1"]),
        path(&["outer", "row-2", "sheet", "cell-2"]),
    ];

    let recorded = declared.borrow().clone();
    assert_eq!(recorded, expected);
    assert_eq!(driver.ratcn.declared_paths(), expected);

    // Routing agrees with both: a press on the depth-four leaf reports the
    // same four segments back.
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 5, 1), &state),
        EventResult::Emit(FocusTestMsg::Focus(FocusState::intent(path(&[
            "outer", "row-1", "sheet", "cell-2"
        ]))))
    );
}

#[test]
fn declaration_panic_does_not_replace_the_previous_surface() {
    let mut driver = Driver::<(), ()>::new(10, 3);
    render_leaf(&mut driver, &ChildId::Static("stable"));

    let result = catch_unwind(AssertUnwindSafe(|| {
        let area = driver.area();
        driver.render(&(), |ctx| {
            ctx.component(ChildId::Static("staged"), Leaf, area);
            panic!("declaration failed");
        });
    }));

    assert!(result.is_err());
    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![vec![ChildId::Static("stable")]]
    );
}

#[test]
fn component_panic_does_not_replace_the_previous_surface() {
    let mut driver = Driver::<(), ()>::new(10, 3);
    render_leaf(&mut driver, &ChildId::Static("stable"));

    let result = catch_unwind(AssertUnwindSafe(|| {
        let area = driver.area();
        driver.render(&(), |ctx| {
            ctx.component(ChildId::Static("panicking"), PanickingLeaf, area);
        });
    }));

    assert!(result.is_err());
    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![vec![ChildId::Static("stable")]]
    );
}

#[test]
fn the_closure_declares_once_and_queued_paint_runs_once_on_the_frame() {
    let mut driver = Driver::<(), ()>::new(6, 1);
    let declared = Arc::new(AtomicUsize::new(0));
    let seen = Rc::new(RefCell::new(Vec::new()));

    let area = driver.area();
    driver.render(&(), |ctx| {
        declared.fetch_add(1, Ordering::SeqCst);
        let seen = Rc::clone(&seen);
        ctx.paint(move |ctx| {
            let before =
                ctx.with_buffer(|buf| buf.cell((0, 0)).expect("probe cell").symbol().to_owned());
            seen.borrow_mut().push(before);
            ctx.with_buffer(|buf| {
                buf.cell_mut((0, 0)).expect("probe cell").set_symbol("X");
            });
        });
        ctx.component(ChildId::Static("leaf"), Leaf, area);
    });

    // The closure ran once and the paint it queued ran once, against the
    // frame itself.
    assert_eq!(declared.load(Ordering::SeqCst), 1);
    assert_eq!(*seen.borrow(), [" "]);
    let buffer = driver.buffer();
    assert_eq!(buffer.cell((0, 0)).expect("painted cell").symbol(), "X");
}

#[test]
fn caught_component_panic_marks_the_whole_pass_as_failed() {
    let mut driver = Driver::<(), ()>::new(10, 3);
    render_leaf(&mut driver, &ChildId::Static("stable"));

    let result = catch_unwind(AssertUnwindSafe(|| {
        let area = driver.area();
        driver.render(&(), |ctx| {
            ctx.component(ChildId::Static("catching"), CatchingComposite, area);
        });
    }));

    assert!(result.is_err());
    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![vec![ChildId::Static("stable")]]
    );
}

#[test]
fn caught_duplicate_id_panic_marks_the_whole_pass_as_failed() {
    let mut driver = Driver::<(), ()>::new(10, 3);
    render_leaf(&mut driver, &ChildId::Static("stable"));

    let result = catch_unwind(AssertUnwindSafe(|| {
        let area = driver.area();
        driver.render(&(), |ctx| {
            ctx.component(ChildId::Static("duplicate"), Leaf, area);
            let caught = catch_unwind(AssertUnwindSafe(|| {
                ctx.component(ChildId::Static("duplicate"), Leaf, area);
            }));
            assert!(caught.is_err());
        });
    }));

    assert!(result.is_err());
    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![vec![ChildId::Static("stable")]]
    );
}

/// The same for the scope form: `scope` validates sibling ids exactly as
/// `component` does, so an app that catches the duplicate's panic and carries
/// on must still be denied the commit — a half-declared tree would route
/// against geometry the app believes it replaced.
#[test]
fn caught_duplicate_scope_id_panic_marks_the_whole_pass_as_failed() {
    let mut driver = Driver::<(), ()>::new(10, 3);
    render_leaf(&mut driver, &ChildId::Static("stable"));

    let result = catch_unwind(AssertUnwindSafe(|| {
        let area = driver.area();
        driver.render(&(), |ctx| {
            ctx.scope(
                ChildId::Static("duplicate"),
                area,
                ScopeOptions::default(),
                |_| {},
            );
            let caught = catch_unwind(AssertUnwindSafe(|| {
                ctx.scope(
                    ChildId::Static("duplicate"),
                    area,
                    ScopeOptions::default(),
                    |_| {},
                );
            }));
            assert!(caught.is_err());
        });
    }));

    assert!(result.is_err());
    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![vec![ChildId::Static("stable")]]
    );
}

#[test]
fn caught_scope_option_panic_marks_the_whole_pass_as_failed() {
    let mut driver = Driver::<(), ()>::new(10, 3);
    render_leaf(&mut driver, &ChildId::Static("stable"));

    let result = catch_unwind(AssertUnwindSafe(|| {
        let area = driver.area();
        driver.render(&(), |ctx| {
            let caught = catch_unwind(AssertUnwindSafe(|| {
                ctx.component(
                    ChildId::Static("panicking-options"),
                    PanickingScopeOptions,
                    area,
                );
            }));
            assert!(caught.is_err());
        });
    }));

    assert!(result.is_err());
    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![vec![ChildId::Static("stable")]]
    );
}

#[test]
fn caught_resolve_panic_marks_the_whole_pass_as_failed() {
    let mut driver = Driver::<(), ()>::new(10, 3);
    render_leaf(&mut driver, &ChildId::Static("stable"));

    let result = catch_unwind(AssertUnwindSafe(|| {
        let area = driver.area();
        driver.render(&(), |ctx| {
            let caught = catch_unwind(AssertUnwindSafe(|| {
                ctx.component(ChildId::Static("panicking-resolve"), PanickingResolve, area);
            }));
            assert!(caught.is_err());
        });
    }));

    assert!(result.is_err());
    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![vec![ChildId::Static("stable")]]
    );
}

#[test]
fn caught_focusability_panic_marks_the_whole_pass_as_failed() {
    let mut driver = Driver::<(), ()>::new(10, 3);
    render_leaf(&mut driver, &ChildId::Static("stable"));

    let result = catch_unwind(AssertUnwindSafe(|| {
        let area = driver.area();
        driver.render(&(), |ctx| {
            let caught = catch_unwind(AssertUnwindSafe(|| {
                ctx.component(
                    ChildId::Static("panicking-focusable"),
                    PanickingFocusable,
                    area,
                );
            }));
            assert!(caught.is_err());
        });
    }));

    assert!(result.is_err());
    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![vec![ChildId::Static("stable")]]
    );
}

#[test]
fn caught_interaction_area_panic_marks_the_whole_pass_as_failed() {
    let mut driver = Driver::<(), ()>::new(10, 3);
    render_leaf(&mut driver, &ChildId::Static("stable"));

    let result = catch_unwind(AssertUnwindSafe(|| {
        let area = driver.area();
        driver.render(&(), |ctx| {
            let caught = catch_unwind(AssertUnwindSafe(|| {
                ctx.component(
                    ChildId::Static("panicking-area"),
                    PanickingInteractionArea,
                    area,
                );
            }));
            assert!(caught.is_err());
        });
    }));

    assert!(result.is_err());
    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![vec![ChildId::Static("stable")]]
    );
}

#[test]
fn escaping_interaction_area_panics_without_replacing_the_surface() {
    let mut driver = Driver::<(), ()>::new(10, 3);
    render_leaf(&mut driver, &ChildId::Static("stable"));

    let result = catch_unwind(AssertUnwindSafe(|| {
        driver.render(&(), |ctx| {
            ctx.component(
                ChildId::Static("escaping-area"),
                EscapingInteractionArea,
                Rect::new(2, 1, 4, 1),
            );
        });
    }));

    assert!(result.is_err());
    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![vec![ChildId::Static("stable")]]
    );
}

#[test]
fn deferred_paint_finishes_before_surface_replacement() {
    let mut driver = Driver::<(), ()>::new(10, 3);
    let painted = Arc::new(AtomicBool::new(false));
    let deferred_painted = Arc::clone(&painted);

    let area = driver.area();
    driver.render(&(), |ctx| {
        ctx.component(ChildId::Static("next"), Leaf, area);
        let deferred_painted = Arc::clone(&deferred_painted);
        ctx.defer_paint(move |_| {
            deferred_painted.store(true, Ordering::SeqCst);
        });
    });

    assert!(painted.load(Ordering::SeqCst));
    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![vec![ChildId::Static("next")]]
    );
}

#[test]
fn deferred_paint_panic_does_not_replace_the_previous_surface() {
    let mut driver = Driver::<(), ()>::new(10, 3);
    render_leaf(&mut driver, &ChildId::Static("stable"));

    let result = catch_unwind(AssertUnwindSafe(|| {
        let area = driver.area();
        driver.render(&(), |ctx| {
            ctx.component(ChildId::Static("next"), Leaf, area);
            ctx.defer_paint(|_| panic!("deferred paint failed"));
        });
    }));

    assert!(result.is_err());
    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![vec![ChildId::Static("stable")]]
    );
}

/// Declared closed, prepared open: every pre-render answer reports what
/// `prepare` computed, never the value the builder was constructed with.
/// `is_focusable` stays at its default so the focus claim is attributable
/// to `scope_options` alone — the runtime ORs the two together.
struct PreparedClaims {
    open: bool,
}

impl Component<bool, ()> for PreparedClaims {
    fn prepare(&mut self, state: &bool) {
        self.open = *state;
    }

    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, bool, ()>) {}

    fn scope_options(&self) -> ScopeOptions {
        let options = ScopeOptions::default();
        if self.open {
            options.focusable()
        } else {
            options
        }
    }

    fn interaction_area(&self, area: Rect) -> Rect {
        // Non-empty when closed, so an unprepared area suppresses only the
        // hit-test assertion below and not the focus claim as well.
        if self.open {
            area
        } else {
            Rect::new(area.x, area.y, 1, 1)
        }
    }

    fn handle_event(
        &mut self,
        _event: &Event,
        _state: &bool,
        _ctx: &mut EventCtx<'_>,
    ) -> EventResult<()> {
        EventResult::Emit(())
    }
}

#[test]
fn prepare_runs_before_every_pre_render_answer_is_read() {
    let mut driver = Driver::<bool, ()>::new(8, 2);
    let area = Rect::new(0, 0, 8, 2);
    driver.render(&true, |ctx| {
        ctx.component(
            ChildId::Static("claims"),
            PreparedClaims { open: false },
            area,
        );
    });

    assert!(
        driver
            .ratcn
            .focus_path(&[ChildId::Static("claims")])
            .is_some(),
        "scope_options was read after prepare"
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 7, 1), &true),
        EventResult::Emit(()),
        "interaction_area was read after prepare, so the whole area hit-tests"
    );
}

#[test]
fn empty_interaction_area_keeps_paint_and_identity_but_excludes_its_subtree() {
    let rendered = Arc::new(AtomicBool::new(false));
    let state = FocusTestState {
        focus: FocusState::intent([ChildId::Static("area-aware"), ChildId::Static("child")]),
    };
    let mut driver = focus_driver(8, 2);

    for (width, usable) in [(1, false), (2, true)] {
        let area = Rect::new(0, 0, width, 1);
        rendered.store(false, Ordering::SeqCst);
        driver.render(&state, |ctx| {
            ctx.component(
                ChildId::Static("area-aware"),
                AreaAwareComposite {
                    expected_area: area,
                    minimum_width: 2,
                    rendered: Arc::clone(&rendered),
                },
                area,
            );
            ctx.component(
                ChildId::Static("visible"),
                FocusLeaf::enabled(),
                Rect::new(4, 0, 2, 1),
            );
        });
        assert!(rendered.load(Ordering::SeqCst));
        assert_eq!(
            driver
                .ratcn
                .focus_path(&[ChildId::Static("area-aware")])
                .is_some(),
            usable
        );
        let enter = driver.event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state);
        assert_eq!(
            matches!(enter, EventResult::Emit(FocusTestMsg::Activated(_))),
            usable
        );
        let down = driver.event(mouse(MouseKind::Down(MouseButton::Left), 0, 0), &state);
        assert_eq!(!matches!(down, EventResult::Ignored), usable);
        if !usable {
            assert_eq!(
                driver.event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
                EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([ChildId::Static(
                    "visible"
                )])))
            );
        }
    }

    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![
            vec![ChildId::Static("area-aware")],
            vec![ChildId::Static("area-aware"), ChildId::Static("child")],
            vec![ChildId::Static("visible")],
        ]
    );
}
