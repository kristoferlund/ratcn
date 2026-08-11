//! Building blocks for composite components — components that declare
//! children of their own, the way [`Dialog`](crate::Dialog) does.
//!
//! Two pieces of bookkeeping recur in composites, and both are the same shape:
//! a value that has to be tracked through a lifecycle rather than simply held.
//!
//! - custom body closures are `FnOnce` and consumed by rendering, while the
//!   fact that a body exists must outlive the closure (event-time geometry
//!   asks) — [`BodySlot`] tracks that one body;
//! - standard children are prepared early, in the composite's
//!   [`prepare`](super::Component::prepare), so their measured sizes can
//!   drive the composite's layout, then rendered later —
//!   [`ChildSlots`] tracks them across that gap.
//!
//! Neither type is needed for ordinary leaf components; reach for them when
//! writing a composite whose children vary with app state.

use ratatui::layout::{Rect, Size};

use super::ChildId;
use super::component::{Component, MeasuredComponent, PreparedComponent, RenderCtx};

/// A composite's custom body closure, boxed for storage.
pub type BodyFn<S, M> = Box<dyn FnOnce(&mut RenderCtx<'_, '_, S, M>)>;

/// A custom body closure through its lifecycle: not configured, configured
/// but not yet painted, or already consumed by this declaration's render.
///
/// `Rendered` is not the same as `None`: the closure is `FnOnce` and gone
/// after painting, but event-time geometry may still need to know the
/// composite *has* a body — a dialog's box height depends on it while
/// handling drags.
#[derive(Default)]
pub enum BodySlot<S, M> {
    /// No body was configured.
    #[default]
    None,
    /// A body is configured and has not painted yet.
    Pending(BodyFn<S, M>),
    /// The body painted this declaration; the closure is gone, the fact
    /// that it existed remains.
    Rendered,
}

impl<S, M> std::fmt::Debug for BodySlot<S, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::None => "BodySlot::None",
            Self::Pending(_) => "BodySlot::Pending",
            Self::Rendered => "BodySlot::Rendered",
        })
    }
}

impl<S: 'static, M: 'static> BodySlot<S, M> {
    /// Store `f` as the pending body.
    pub fn set(&mut self, body: impl FnOnce(&mut RenderCtx<'_, '_, S, M>) + 'static) {
        *self = Self::Pending(Box::new(body));
    }
}

impl<S, M> BodySlot<S, M> {
    /// Whether a body was configured, regardless of whether it has painted.
    #[must_use]
    pub const fn is_configured(&self) -> bool {
        !matches!(self, Self::None)
    }

    /// Consume the pending closure, or `None` when no body was configured.
    ///
    /// # Panics
    ///
    /// Panics if the closure was already consumed — one declaration must
    /// render at most once.
    pub fn consume(&mut self) -> Option<BodyFn<S, M>> {
        match self {
            Self::None => None,
            Self::Rendered => panic!("composite body rendered more than once"),
            Self::Pending(_) => {
                let Self::Pending(render) = std::mem::replace(self, Self::Rendered) else {
                    unreachable!("matched Pending above");
                };
                Some(render)
            }
        }
    }
}

/// One child through its lifecycle: pending with the raw component, prepared
/// so its measured size can drive layout, then consumed by rendering.
///
/// The states are per *pass*, not per frame: the app closure rebuilds the
/// composite for each of the two passes, so every pass walks the whole
/// lifecycle from `Pending`.
enum ChildState<S, M> {
    /// Pushed, holding the raw component. Not yet declared to the runtime —
    /// that happens in [`ChildSlots::render_each`].
    Pending(Box<dyn Component<S, M>>),
    /// [`Component::prepare`] has run, so the component's scope options and
    /// focus claim are known and its measured size can drive layout.
    Prepared(PreparedComponent<S, M>),
    /// Declared and painted; the component is gone, its size remains.
    Rendered,
}

struct ChildSlot<S, M> {
    id: ChildId,
    state: ChildState<S, M>,
    /// Measured when pushed; still readable after the slot is consumed,
    /// because event-time geometry derives layout from child sizes.
    size: Size,
}

/// Standard children of a composite, held across the gap between early
/// preparation and late rendering.
///
/// The composite pushes measured children at build time, prepares them all in
/// its [`prepare`](super::Component::prepare) so their measured sizes can
/// drive layout, and renders them with [`render_each`](Self::render_each).
/// Each child is prepared exactly once, in insertion order — insertion order
/// is also focus traversal order.
pub struct ChildSlots<S, M> {
    children: Vec<ChildSlot<S, M>>,
}

impl<S, M> Default for ChildSlots<S, M> {
    fn default() -> Self {
        Self {
            children: Vec::new(),
        }
    }
}

impl<S, M> std::fmt::Debug for ChildSlots<S, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.children.iter().map(|child| &child.id))
            .finish()
    }
}

impl<S: 'static, M: 'static> ChildSlots<S, M> {
    /// Add a measured child. Ids share the composite's sibling namespace.
    pub fn push(
        &mut self,
        id: impl Into<ChildId>,
        component: impl MeasuredComponent<S, M> + 'static,
    ) {
        let size = component.measure();
        self.children.push(ChildSlot {
            id: id.into(),
            state: ChildState::Pending(Box::new(component)),
            size,
        });
    }

    /// Whether any children were pushed. `const` so composite builders can
    /// assert configuration conflicts in `const fn`s.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// How many children were pushed.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.children.len()
    }

    /// The measured size of every child, in insertion order. Available in all
    /// lifecycle states, so layout math can run before preparation and event
    /// geometry after rendering.
    pub fn sizes(&self) -> impl Iterator<Item = Size> + '_ {
        self.children.iter().map(|child| child.size)
    }

    /// Prepare every child against `state`, from the composite's
    /// [`prepare`](super::Component::prepare).
    ///
    /// # Panics
    ///
    /// Panics if called more than once on the same declaration.
    pub fn prepare(&mut self, state: &S) {
        for child in &mut self.children {
            let ChildState::Pending(component) =
                std::mem::replace(&mut child.state, ChildState::Rendered)
            else {
                panic!("composite child prepared more than once");
            };
            child.state = ChildState::Prepared(PreparedComponent::prepare(component, state));
        }
    }

    /// Consume every prepared child in insertion order. `place` maps a
    /// child's measured size to the area it should occupy; each child is then
    /// declared into `ctx` with its stored id, exactly as
    /// [`RenderCtx::render_component`] would, but reusing the preparation done
    /// by [`prepare`](Self::prepare).
    ///
    /// # Panics
    ///
    /// Panics if a child was not prepared, or was already rendered.
    pub fn render_each(
        &mut self,
        ctx: &mut RenderCtx<'_, '_, S, M>,
        mut place: impl FnMut(usize, Size) -> Rect,
    ) {
        for (index, child) in self.children.iter_mut().enumerate() {
            let ChildState::Prepared(prepared) =
                std::mem::replace(&mut child.state, ChildState::Rendered)
            else {
                panic!("composite child rendered before being prepared, or more than once");
            };
            let area = place(index, child.size);
            ctx.render_prepared_component(child.id.clone(), prepared, area);
        }
    }

    /// Whether every child has been rendered — for a composite's fail-loud
    /// end-of-render check.
    #[must_use]
    pub fn all_rendered(&self) -> bool {
        self.children
            .iter()
            .all(|child| matches!(child.state, ChildState::Rendered))
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;
    use crate::Button;

    #[test]
    fn body_slot_keeps_the_configured_fact_after_consumption() {
        let mut slot: BodySlot<(), ()> = BodySlot::default();
        assert!(!slot.is_configured());
        assert!(slot.consume().is_none());

        slot.set(|_| {});
        assert!(slot.is_configured());
        assert!(slot.consume().is_some());
        // The closure is gone, the fact that a body existed is not.
        assert!(slot.is_configured());

        let double = catch_unwind(AssertUnwindSafe(|| {
            slot.consume();
        }));
        assert!(double.is_err(), "a second consume must fail loud");
    }

    #[test]
    fn prepared_children_walk_the_declared_prepared_rendered_lifecycle() {
        let mut children: ChildSlots<(), ()> = ChildSlots::default();
        children.push("enabled", Button::new("OK"));
        children.push("disabled", Button::new("No").disabled(true));
        assert_eq!(children.len(), 2);

        children.prepare(&());

        let again = catch_unwind(AssertUnwindSafe(|| {
            children.prepare(&());
        }));
        assert!(again.is_err(), "a second prepare must fail loud");
    }
}
