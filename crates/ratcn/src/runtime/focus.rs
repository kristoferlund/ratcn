//! Keyboard focus: which component holds it, and how it moves.
//!
//! [`FocusState`] is the path of the focused component, and it lives in app
//! state. Hand it to the runtime with [`Ratcn::focus`](super::Ratcn::focus),
//! which pairs the accessor that reads it with the message that replaces it;
//! the runtime then moves it as Tab, Shift+Tab, and pointer presses land.
//! [`TabWrap`] decides what a focus scope does at its ends.

use super::ChildId;

/// Which component currently has keyboard focus, stored as a path.
///
/// Focus is a path of [`ChildId`](super::ChildId)s rather than a single id,
/// because an id is only unique among its siblings. The path starts at the root
/// declaration's focused child and ends at the focused leaf, so
/// `["settings", "save"]` and `["editor", "save"]` name different buttons.
///
/// This value lives in *your* state, not in the runtime. Wire it up with
/// [`Ratcn::focus`](super::Ratcn::focus), which bundles the accessor that reads
/// it together with the message constructor used to replace it. The runtime
/// never writes app state: a focus change comes back to you as a message
/// carrying a new snapshot, and your update function stores it.
///
/// # The default (empty) path
///
/// `FocusState::default()` is an empty path, meaning "no explicit choice yet —
/// use the default focus". Apps never have to pre-resolve startup focus. During
/// a successful declaration the runtime descends to the first focusable
/// candidate and focuses its first participating leaf.
///
/// Layout is responsive, so that candidate can turn out to have no focusable
/// leaf once its parent has painted. The surface then keeps the parent path it
/// painted, and event routing reuses that same path, so render and routing
/// agree on where focus is.
///
/// Events that arrive before the first successful render are ignored, so an
/// unresolved empty path never receives input.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FocusState {
    pub(crate) path: Vec<ChildId>,
}

/// The empty path, borrowable: what a runtime with no focus binding reads.
pub(crate) static UNRESOLVED: FocusState = FocusState { path: Vec::new() };

impl FocusState {
    /// Build a focus path directly, without checking that it exists.
    ///
    /// The constructor for a path already in hand: an app jumping to a pane
    /// after a hotkey, the runtime handing back a leaf it walked to. The path
    /// is resolved against the surface later, at render and routing time, so
    /// nothing is validated here. A path naming no declared component leaves
    /// focus parked on it; the runtime never silently retargets to a nearby
    /// control.
    ///
    /// Stopping the path at a container is fine and usual — the runtime
    /// descends from there to that container's first focusable leaf.
    #[must_use]
    pub fn intent(path: impl IntoIterator<Item = impl Into<ChildId>>) -> Self {
        Self {
            path: path.into_iter().map(Into::into).collect(),
        }
    }

    /// True if `path` is a prefix of the focus path — the focused leaf is at or
    /// inside that subtree.
    ///
    /// This is the app-side equivalent of the
    /// [`PaintCtx::contains_focus`](super::PaintCtx::contains_focus) flag a
    /// component sees, useful for things like accenting a pane whose contents
    /// hold focus.
    #[must_use]
    pub fn contains_path(&self, path: impl IntoIterator<Item = impl Into<ChildId>>) -> bool {
        let mut ids = self.path.iter();
        path.into_iter().all(|id| ids.next() == Some(&id.into()))
    }

    /// The focus path, root declaration's child first, leaf last.
    #[must_use]
    pub fn path(&self) -> &[ChildId] {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::FocusState;

    #[test]
    fn contains_path_matches_prefixes_of_the_path() {
        let focus = FocusState::intent(["left", "save"]);
        assert!(focus.contains_path(["left"]));
        assert!(focus.contains_path(["left", "save"]));
        assert!(!focus.contains_path(["right"]));
        assert!(!focus.contains_path(["save"]));
        assert!(!focus.contains_path(["left", "save", "extra"]));
    }

    // A single-segment path is still a full path: it identifies a leaf declared
    // directly under the root, not "that leaf ID anywhere".
    #[test]
    fn path_queries_on_a_single_segment_path() {
        let focus = FocusState::intent(["save"]);
        assert!(focus.contains_path(["save"]));
        assert!(!focus.contains_path(["left", "save"]));
    }

    // The default (empty) path matches no component; the empty query path is
    // exactly the default and a prefix of every path.
    #[test]
    fn path_queries_on_and_with_the_empty_path() {
        let empty: [&str; 0] = [];
        let unresolved = FocusState::default();
        assert!(unresolved.contains_path(empty));
        assert!(!unresolved.contains_path(["save"]));

        let focus = FocusState::intent(["left", "save"]);
        assert!(focus.contains_path(empty));
    }

    // Static and dynamic ids share string identity, so a runtime-built path
    // matches a static query path.
    #[test]
    fn path_queries_compare_static_and_dynamic_ids_by_content() {
        let focus = FocusState::intent([String::from("left"), String::from("save")]);
        assert!(focus.contains_path(["left", "save"]));
        assert!(!focus.contains_path(["right", "save"]));
    }
}

/// What a focus scope does when Tab moves past its last focusable child (or
/// `BackTab` past its first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabWrap {
    /// Return the event unhandled so the parent scope advances. Whole-app Tab
    /// traversal emerges from this; the default for nested scopes.
    #[default]
    Escape,
    /// Cycle within this scope, never escaping to a sibling — a hard tab
    /// boundary. With nothing to move to (e.g. a focusable container with no
    /// focusable children) Tab stays put. Typical for roots, dialogs, and panes
    /// that should trap Tab.
    Wrap,
}
