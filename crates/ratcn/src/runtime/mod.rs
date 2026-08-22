//! The interaction runtime: focus, hover, mouse routing, and the typed
//! messages a component sends back to your app.
//!
//! Ratatui widgets only draw. Anything interactive needs something that knows
//! which component is focused, what the pointer is over, and where an event
//! should go. That is what [`Ratcn`] adds, without taking over the app's loop.
//!
//! # The two call sites
//!
//! The runtime enters an app in exactly two places, and both can be removed
//! again:
//!
//! - [`Ratcn::render`] — run once per frame. Its closure *declares* the
//!   components that exist right now and paints them.
//! - [`Ratcn::handle_event`] — hand it one backend event; it returns an
//!   [`EventResult`] which is either `Ignored`, `Consumed`, or `Emit(msg)`.
//!
//! Everything else — the event loop, the update function, theme switching,
//! app-level hotkeys — stays the app's.
//!
//! # Declarations and the retained surface
//!
//! Each `render` is a fresh **declaration pass**: the closure builds components
//! from current state, so there is no persistent widget tree to keep in sync.
//! When a pass completes successfully the runtime keeps the result — the
//! **retained surface** — holding the component instances, their identity
//! paths, their painted geometry, and the interaction props they were given.
//!
//! `handle_event` routes against the retained surface rather than re-declaring,
//! which is why events arriving before the first successful render are ignored:
//! there is nothing yet to route through. Events reach the focused or
//! hit-tested leaf and bubble upward if unhandled, and anything the app must
//! act on comes back as a returned message, never as a mutation.
//!
//! # Declaring and painting are two walks
//!
//! [`Ratcn::render`] runs the closure once, and that run paints nothing: it
//! builds the tree and queues the paint each declaration owes. Focus resolves
//! against the finished tree, and only then does the queue run — which is why
//! [`PaintCtx`] carries the interaction flags and [`DeclareCtx`] does not.
//! Declaring once means the closure may have side effects and may move what it
//! captures into the components it declares.
//!
//! # Who owns what
//!
//! The app owns semantic state: domain data, component values, focus, and the
//! optional [`ModalState`]. The runtime owns what a render derives and what
//! the pointer does — the retained surface above, hover, mouse-gesture
//! bookkeeping, and short-lived per-component values stashed via
//! [`EventCtx::transient`]. Neither side may become a second source of truth
//! for what the other owns.
//!
//! Focus and hover fall on opposite sides of that line, and whose question
//! each answers is the reason. Focus is a decision an app can make on its
//! own — open a dialog, focus its first field — so it lives in app state and
//! moves by message. Hover is a fact about where the mouse physically is,
//! which no app decides, so the runtime keeps it and rewrites it whenever the
//! pointer or the surface moves. Apps read it structurally through
//! [`DeclareCtx::pointer_within`] and paint from [`PaintCtx::hovered`] and
//! [`PaintCtx::contains_hover`].
//!
//! This module is the public namespace for both using the runtime and writing
//! components against it.

use std::{cmp::Ordering, fmt, hash::Hash, sync::Arc};

mod component;
mod drag;
mod engine;
mod event;
mod focus;
mod modal;

/// The name a component is declared under, and half of how it is identified.
///
/// A component's full identity is the *path* of ids through its enclosing
/// scopes, not the id alone. So an id only has to be unique among its
/// siblings, and `"save"` can appear under several different parents.
///
/// Ids must also be **stable across frames** for the same logical component.
/// Every frame re-declares the surface from scratch, and identity is how the
/// runtime recognises that this frame's button is last frame's focused button.
/// An id derived from something that shifts — a loop index over a reorderable
/// list, say — moves focus and hover around with it.
///
/// Use [`Static`](Self::Static) for ids known at compile time and
/// [`Dynamic`](Self::Dynamic) for ids built from data, converting once when the
/// item enters your model and cloning the stored id on each declaration.
#[derive(Clone)]
pub enum ChildId {
    /// A compile-time structural id. Supports allocation-free constants.
    Static(&'static str),
    /// A runtime id whose immutable string allocation is shared across paths
    /// and declaration trees.
    Dynamic(Arc<str>),
}

impl ChildId {
    /// The string content used for identity, diagnostics, ordering, and hashing.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Static(id) => id,
            Self::Dynamic(id) => id,
        }
    }
}

impl From<&'static str> for ChildId {
    fn from(id: &'static str) -> Self {
        Self::Static(id)
    }
}

/// Builds a [`ChildId::Dynamic`] from an owned runtime string. Convert once
/// when the logical child enters the model and clone the stored id afterwards.
impl From<String> for ChildId {
    fn from(id: String) -> Self {
        Self::Dynamic(Arc::from(id))
    }
}

impl From<Arc<str>> for ChildId {
    fn from(id: Arc<str>) -> Self {
        Self::Dynamic(id)
    }
}

impl From<&ChildId> for ChildId {
    fn from(id: &ChildId) -> Self {
        id.clone()
    }
}

impl PartialEq for ChildId {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for ChildId {}

impl PartialOrd for ChildId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ChildId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl Hash for ChildId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl PartialEq<&str> for ChildId {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<ChildId> for &str {
    fn eq(&self, other: &ChildId) -> bool {
        *self == other.as_str()
    }
}

impl AsRef<str> for ChildId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for ChildId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(f)
    }
}

impl fmt::Display for ChildId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(f)
    }
}

pub use component::{
    Component, DeclareCtx, EventCtx, MeasuredComponent, PaintCtx, PopupOptions, ScopeOptions, Step,
};
pub use drag::{CellOffset, DragOptions, DragPhase, clamp_offset, offset_rect};
pub use engine::Ratcn;
#[cfg(all(target_arch = "wasm32", feature = "ratzilla"))]
pub use event::BrowserEventError;
pub use event::{
    Event, EventResult, KeyChord, KeyCode, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseKind,
    ScrollDirection, Unsupported,
};
pub use focus::{FocusState, TabWrap};
pub use modal::{ModalOpenError, ModalState};
