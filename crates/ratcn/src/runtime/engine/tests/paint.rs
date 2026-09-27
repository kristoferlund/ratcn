//! The order the paint queue replays in, and what a rejected pass leaves on
//! the screen.

use super::*;

/// Every layer paints after the whole base declaration, in the order the
/// layers were declared: base content declared after a layer still paints
/// beneath it.
#[test]
fn layers_paint_after_the_base_declaration_in_declaration_order() {
    let state = FocusTestState::default();
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut driver = Driver::<FocusTestState, FocusTestMsg>::new(5, 2);
    let area = driver.area();
    let logging = |name| LoggingComponent {
        name,
        log: Rc::clone(&log),
        focusable: false,
    };
    driver.render(&state, |ctx| {
        ctx.component(ChildId::Static("base"), logging("base"), area);
        ctx.modal(ChildId::Static("lower"), logging("lower"), area);
        ctx.component(ChildId::Static("after"), logging("after"), area);
        ctx.modal(ChildId::Static("top"), logging("top"), area);
    });

    assert_eq!(*log.borrow(), ["base", "after", "lower", "top"]);
    assert!(driver.ratcn.modal_is_open());
}

/// A composite that fills its own area, and a child that draws one cell of
/// it.
struct BackdropParent;

impl Component<(), ()> for BackdropParent {
    fn declare(&mut self, ctx: &mut DeclareCtx<'_, (), ()>) {
        let area = ctx.area();
        ctx.component(ChildId::Static("glyph"), GlyphLeaf("C"), area);
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, ()>) {
        let area = ctx.area();
        ctx.widget(
            ratatui::text::Line::from("#".repeat(area.width as usize)),
            area,
        );
    }
}

/// A leaf whose whole behavior is putting one identifiable glyph in the
/// top-left cell of its area, so a test can name what reached the screen.
struct GlyphLeaf(&'static str);
impl<S, M> Component<S, M> for GlyphLeaf {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, S, M>) {}

    fn paint(&mut self, ctx: &mut PaintCtx<'_, S>) {
        let area = ctx.area();
        ctx.widget(
            ratatui::text::Line::from(self.0),
            Rect {
                width: 1,
                height: 1,
                ..area
            },
        );
    }
}

/// The paint-before-children contract, kept by queue position rather than
/// by each component's care: a composite is queued where it opens, so its
/// backdrop is drawn before anything it declares inside itself and the
/// child's glyph survives on top.
#[test]
fn a_components_own_paint_lands_beneath_its_descendants_paint() {
    let mut driver = Driver::<(), ()>::new(3, 1);
    let area = driver.area();
    driver.render(&(), |ctx| {
        ctx.component(ChildId::Static("parent"), BackdropParent, area);
    });

    let buffer = driver.buffer();
    assert_eq!(buffer.cell((0, 0)).expect("cell").symbol(), "C");
    assert_eq!(buffer.cell((1, 0)).expect("cell").symbol(), "#");
}

/// A leaf that fills its whole area with one glyph, so a test can read
/// which writes landed under it and which over it.
struct FillLeaf(&'static str);
impl<S, M> Component<S, M> for FillLeaf {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, S, M>) {}

    fn paint(&mut self, ctx: &mut PaintCtx<'_, S>) {
        let area = ctx.area();
        ctx.widget(
            ratatui::text::Line::from(self.0.repeat(area.width as usize)),
            area,
        );
    }
}

/// [`DeclareCtx::paint_widget`] is one op queued where it is called, the
/// same position [`DeclareCtx::paint`] takes: a component declared between
/// two of them paints over the first and under the second, so the
/// shorthand carries the declaration-order z-order rather than collapsing
/// to the ends of the frame.
#[test]
fn paint_widget_queues_at_the_position_it_is_called_from() {
    let mut driver = Driver::<(), ()>::new(5, 1);
    let area = driver.area();
    driver.render(&(), |ctx| {
        ctx.paint_widget(ratatui::text::Line::from("aaaaa"), area);
        ctx.component(
            ChildId::Static("leaf"),
            FillLeaf("#"),
            Rect::new(1, 0, 3, 1),
        );
        ctx.paint_widget(ratatui::text::Line::from("zz"), Rect::new(2, 0, 2, 1));
    });

    let buffer = driver.buffer();
    let row: Vec<&str> = (0..5)
        .map(|x| buffer.cell((x, 0)).expect("cell").symbol())
        .collect();
    assert_eq!(
        row,
        ["a", "#", "z", "z", "a"],
        "the component covers the paint_widget declared before it and is covered by the one declared after it"
    );
}

/// A pass the runtime rejects is rejected before it draws: declaration
/// and both checks finish while the frame is still only a description of
/// itself. So the cells already on screen survive a bad frame, exactly as
/// the retained surface does.
///
/// Declared modal roots that disagree with the app's own stack are the
/// rejection the runtime can only answer once declaration has ended,
/// which is what makes them the case worth pinning: the whole queue is
/// built before anything decides it will never run.
#[test]
fn a_pass_rejected_by_the_modal_stack_never_touches_the_screen() {
    let state = ModalTestState::default();
    let mut driver = Driver::with(
        Ratcn::<ModalTestState, ModalTestMsg>::new().modals(|state: &ModalTestState| &state.modals),
        3,
        1,
    );

    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(ChildId::Static("first"), GlyphLeaf("A"), area);
    });

    let theme = Theme::default_dark();
    let Driver { terminal, ratcn } = &mut driver;
    terminal
        .draw(|frame| {
            let area = frame.area();
            frame.render_widget(ratatui::text::Line::from("A"), area);
            // Nothing is open in the app's stack, so this modal root is a
            // declaration the runtime refuses to retain.
            let rejected = catch_unwind(AssertUnwindSafe(|| {
                ratcn.render(frame, area, &state, &theme, |ctx| {
                    ctx.modal(ChildId::Static("sheet"), GlyphLeaf("B"), area);
                });
            }));
            assert!(rejected.is_err());
        })
        .expect("draw");

    assert_eq!(
        driver.cell(0, 0).symbol(),
        "A",
        "a pass rejected by the modal stack must not have painted"
    );
    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![vec![ChildId::Static("first")]]
    );
}

/// The rejection above, watched from the base layer rather than from the
/// modal that caused it.
///
/// A modal paints only after the whole base declaration, so a
/// modal-mismatched pass could paint its *base* content and still leave
/// the modal's own invisible. That is the leak the check's position has
/// to prevent, and it is only visible from a base-layer declaration
/// sharing the cell the last good frame owns.
#[test]
fn a_rejected_pass_never_paints_its_base_layer_either() {
    let state = ModalTestState::default();
    let mut driver = Driver::with(
        Ratcn::<ModalTestState, ModalTestMsg>::new().modals(|state: &ModalTestState| &state.modals),
        3,
        1,
    );

    let area = driver.area();
    driver.render(&state, |ctx| {
        ctx.component(ChildId::Static("first"), GlyphLeaf("A"), area);
    });

    let theme = Theme::default_dark();
    let Driver { terminal, ratcn } = &mut driver;
    terminal
        .draw(|frame| {
            let area = frame.area();
            frame.render_widget(ratatui::text::Line::from("A"), area);
            let rejected = catch_unwind(AssertUnwindSafe(|| {
                ratcn.render(frame, area, &state, &theme, |ctx| {
                    // Base-layer content, which paints straight onto the
                    // frame, declared alongside the modal root the app's
                    // empty stack refuses.
                    ctx.component(ChildId::Static("base"), GlyphLeaf("B"), area);
                    ctx.modal(ChildId::Static("sheet"), GlyphLeaf("C"), area);
                });
            }));
            assert!(rejected.is_err());
        })
        .expect("draw");

    assert_eq!(
        driver.cell(0, 0).symbol(),
        "A",
        "a rejected pass must not have painted its base layer"
    );
    assert_eq!(
        driver.ratcn.declared_paths(),
        vec![vec![ChildId::Static("first")]]
    );
}

/// Layers are transparent: a layer paints only the cells its content writes,
/// and what it leaves alone keeps whatever was painted beneath it. A layer
/// that means to occlude paints its own background first.
#[test]
fn a_layer_leaves_the_cells_its_content_does_not_write_to_what_is_beneath() {
    for (occlude, expected) in [(false, "P##"), (true, "P  ")] {
        let mut driver = Driver::<(), ()>::new(3, 1);
        driver.render(&(), |ctx| {
            ctx.paint_widget(ratatui::text::Line::from("###"), Rect::new(0, 0, 3, 1));
            ctx.popup(
                ChildId::Static("panel"),
                Rect::new(0, 0, 3, 1),
                PopupOptions::default(),
                |ctx| {
                    let area = ctx.area();
                    if occlude {
                        ctx.paint_widget(ratatui::widgets::Clear, area);
                    }
                    ctx.paint_widget(ratatui::text::Line::from("P"), area);
                },
            );
        });

        assert_eq!(driver.row(0), expected, "occluding: {occlude}");
    }
}

/// A layer's paint reaches the frame whichever write form did the writing,
/// and a raw write touches only what it writes: the rest of the layer's
/// footprint keeps what is beneath it.
#[test]
fn layer_paint_written_through_with_buffer_lands_only_where_it_writes() {
    let mut driver = Driver::<(), ()>::new(3, 1);
    driver.render(&(), |ctx| {
        ctx.paint_widget(ratatui::text::Line::from("###"), Rect::new(0, 0, 3, 1));
        ctx.popup(
            ChildId::Static("panel"),
            Rect::new(0, 0, 3, 1),
            PopupOptions::default(),
            |ctx| {
                ctx.paint(|ctx| {
                    ctx.with_buffer(Rect::new(0, 0, 1, 1), |area, buf| {
                        buf[(area.x, area.y)].set_symbol("Z");
                    });
                });
            },
        );
    });

    assert_eq!(driver.row(0), "Z##");
}
