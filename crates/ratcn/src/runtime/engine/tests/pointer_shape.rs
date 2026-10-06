//! The pointer shape a frame paints: asked for by the hovered declaration,
//! read by the host after rendering.

use super::*;

/// Asks for `shape` every time it paints, hovered or not, and declares
/// `child` over its lower row when given one. It captures the pointer on a
/// press, so a drag keeps arriving here.
struct Shaped {
    shape: PointerShape,
    child: Option<PointerShape>,
}

impl Shaped {
    const fn leaf(shape: PointerShape) -> Self {
        Self { shape, child: None }
    }
}

impl Component<(), ()> for Shaped {
    fn declare(&mut self, ctx: &mut DeclareCtx<'_, (), ()>) {
        if let Some(shape) = self.child {
            let area = ctx.area();
            let row = Rect::new(area.x, area.bottom() - 1, area.width, 1);
            ctx.component("child", Self::leaf(shape), row);
        }
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, ()>) {
        ctx.set_pointer_shape(self.shape);
    }

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &(),
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<()> {
        match event {
            Event::Mouse(mouse) if mouse.kind == MouseKind::Down(MouseButton::Left) => {
                ctx.capture_pointer(MouseButton::Left);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}

/// A parent and the child on its lower row, both asking unconditionally.
fn nested(ctx: &mut DeclareCtx<'_, (), ()>) {
    let parent = Shaped {
        shape: PointerShape::Grab,
        child: Some(PointerShape::Pointer),
    };
    ctx.component("parent", parent, Rect::new(0, 0, 10, 2));
}

/// The parent paints first and asks too, but only the hovered declaration is
/// heard: the child under the pointer decides, and over the parent's own row
/// the parent does.
#[test]
fn the_deepest_hovered_declaration_decides_the_shape() {
    let mut driver = Driver::<(), ()>::new(10, 3);
    driver.render(&(), nested);

    driver.event(mouse(MouseKind::Moved, 1, 1), &());
    driver.render(&(), nested);
    assert_eq!(driver.ratcn.pointer_shape(), PointerShape::Pointer);

    driver.event(mouse(MouseKind::Moved, 1, 0), &());
    driver.render(&(), nested);
    assert_eq!(driver.ratcn.pointer_shape(), PointerShape::Grab);
}

/// Paint that asks while nothing is hovered is not heard, so the frame shows
/// the terminal's own pointer: before the first motion, over empty space, and
/// after the pointer leaves.
#[test]
fn nothing_hovered_shows_the_default_pointer() {
    let mut driver = Driver::<(), ()>::new(10, 3);
    driver.render(&(), nested);
    assert_eq!(driver.ratcn.pointer_shape(), PointerShape::Default);

    driver.event(mouse(MouseKind::Moved, 1, 1), &());
    driver.render(&(), nested);
    driver.event(mouse(MouseKind::Moved, 1, 2), &());
    driver.render(&(), nested);
    assert_eq!(
        driver.ratcn.pointer_shape(),
        PointerShape::Default,
        "a shape is the frame's, not a setting that outlives the hover"
    );
}

/// A modal blocks hover on what it covers, so a base control under the
/// pointer cannot show its hand through the modal.
#[test]
fn a_modal_blocks_the_shapes_beneath_it() {
    let mut driver = Driver::<(), ()>::new(10, 3);
    let declare = |ctx: &mut DeclareCtx<'_, (), ()>| {
        ctx.component(
            "base",
            Shaped::leaf(PointerShape::Pointer),
            Rect::new(0, 0, 10, 3),
        );
        ctx.modal(
            ChildId::Static("dialog"),
            Shaped::leaf(PointerShape::Text),
            Rect::new(0, 0, 4, 1),
        );
    };
    driver.render(&(), declare);

    driver.event(mouse(MouseKind::Moved, 6, 2), &());
    driver.render(&(), declare);
    assert_eq!(driver.ratcn.pointer_shape(), PointerShape::Default);

    driver.event(mouse(MouseKind::Moved, 1, 0), &());
    driver.render(&(), declare);
    assert_eq!(driver.ratcn.pointer_shape(), PointerShape::Text);
}

/// Releasing a press dragged off its button unfreezes hover onto empty space.
/// Nothing handles that release, but it still asks for a frame, and that frame
/// shows the default pointer without waiting for the pointer to move again.
#[test]
fn the_last_release_asks_for_the_frame_that_drops_a_stale_shape() {
    let mut driver = Driver::<(), ()>::new(10, 3);
    let declare = |ctx: &mut DeclareCtx<'_, (), ()>| {
        let button = crate::Button::new("Go").on_press(|| ());
        ctx.component("go", button, Rect::new(0, 0, 4, 1));
    };
    driver.render(&(), declare);
    driver.event(mouse(MouseKind::Moved, 1, 0), &());
    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &());
    driver.event(mouse(MouseKind::Moved, 8, 2), &());
    driver.render(&(), declare);
    assert_eq!(
        driver.ratcn.pointer_shape(),
        PointerShape::Pointer,
        "the held press keeps hover on the button"
    );

    assert_eq!(
        driver.event(mouse(MouseKind::Up(MouseButton::Left), 8, 2), &()),
        EventResult::Consumed
    );
    driver.render(&(), declare);
    assert_eq!(driver.ratcn.pointer_shape(), PointerShape::Default);
}

/// A drag that leaves the component it pressed keeps that component hovered,
/// so its shape stays with the pointer rather than changing under the drag.
#[test]
fn a_captured_drag_keeps_its_components_shape() {
    let mut driver = Driver::<(), ()>::new(10, 3);
    let declare = |ctx: &mut DeclareCtx<'_, (), ()>| {
        ctx.component(
            "handle",
            Shaped::leaf(PointerShape::Grabbing),
            Rect::new(0, 0, 3, 1),
        );
        ctx.component(
            "other",
            Shaped::leaf(PointerShape::Pointer),
            Rect::new(0, 2, 10, 1),
        );
    };
    driver.render(&(), declare);

    driver.event(mouse(MouseKind::Moved, 1, 0), &());
    driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &());
    driver.event(mouse(MouseKind::Drag(MouseButton::Left), 5, 2), &());
    driver.render(&(), declare);
    assert_eq!(driver.ratcn.pointer_shape(), PointerShape::Grabbing);

    driver.event(mouse(MouseKind::Up(MouseButton::Left), 5, 2), &());
    driver.event(mouse(MouseKind::Moved, 5, 2), &());
    driver.render(&(), declare);
    assert_eq!(driver.ratcn.pointer_shape(), PointerShape::Pointer);
}
