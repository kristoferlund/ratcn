use super::ChildId;

/// App-owned path from the root declaration's hovered child to the component
/// under the pointer.
///
/// The pointer twin of [`FocusState`](super::FocusState), and deliberately
/// separate from it: hovering must **not** move focus (typing in an input keeps
/// working while the mouse drifts over a button). Wired through
/// [`Ratcn::hover`](super::Ratcn::hover); changes arrive as messages carrying a
/// new snapshot, exactly like focus.
///
/// `HoverState::default()` (an empty path) means "nothing is hovered". Ratcn
/// validates stored hover against the last successful surface and last known
/// pointer position; pointer motion emits a replacement snapshot after
/// hit-testing that retained geometry.
///
/// Unlike focus, hover returns to an empty path over empty space.
///
/// The stored path may temporarily name a component that was removed or covered
/// by a modal. Rendering and event routing resolve effective hover against the
/// latest successful surface, so use this value as interaction state rather
/// than as proof that a component currently exists or is eligible.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HoverState {
    pub(crate) path: Vec<ChildId>,
}

impl HoverState {
    /// Build a hover path directly, without checking that it exists.
    ///
    /// The runtime constructs these from hit-testing, so an app rarely needs
    /// to. Reach for it to seed or clear hover from your update function, and
    /// in tests that assert against a known path. Nothing is validated here:
    /// as with [`FocusState::intent`](super::FocusState::intent), the path is
    /// resolved against the surface later, and one naming no declared
    /// component simply reports as not hovered.
    #[must_use]
    pub fn intent(path: impl IntoIterator<Item = impl Into<ChildId>>) -> Self {
        Self {
            path: path.into_iter().map(Into::into).collect(),
        }
    }

    /// True if `path` is exactly the hover path, root declaration's child
    /// first, leaf last. Identity is the full path — a bare ID is ambiguous
    /// when different subtrees reuse it.
    #[must_use]
    pub fn is_path(&self, path: impl IntoIterator<Item = impl Into<ChildId>>) -> bool {
        let mut ids = self.path.iter();
        path.into_iter().all(|id| ids.next() == Some(&id.into())) && ids.next().is_none()
    }

    /// True if `path` is a prefix of the hover path — the hovered leaf is at or
    /// inside that subtree.
    ///
    /// This is the app-side equivalent of the
    /// [`RenderCtx::contains_hover`](super::RenderCtx::contains_hover) flag a
    /// component sees.
    #[must_use]
    pub fn contains_path(&self, path: impl IntoIterator<Item = impl Into<ChildId>>) -> bool {
        let mut ids = self.path.iter();
        path.into_iter().all(|id| ids.next() == Some(&id.into()))
    }

    /// The hover path, root declaration's child first, hovered leaf last.
    #[must_use]
    pub fn path(&self) -> &[ChildId] {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::HoverState;

    // Mirror of the focus-path queries: a bare leaf ID reused across subtrees
    // is ambiguous, so only full paths and their prefixes match.
    #[test]
    fn path_queries_match_full_paths_and_prefixes_only() {
        let hover = HoverState::intent(["left", "save"]);
        assert!(hover.is_path(["left", "save"]));
        assert!(!hover.is_path(["right", "save"]));
        assert!(!hover.is_path(["save"]));
        assert!(!hover.is_path(["left"]));
        assert!(!hover.is_path(["left", "save", "extra"]));
        assert!(hover.contains_path(["left"]));
        assert!(hover.contains_path(["left", "save"]));
        assert!(!hover.contains_path(["right"]));
        assert!(!hover.contains_path(["save"]));
        assert!(!hover.contains_path(["left", "save", "extra"]));
    }

    // The default (empty) path is "nothing hovered": no component matches, the
    // empty query path is exactly it and a prefix of every path.
    #[test]
    fn path_queries_on_and_with_the_empty_path() {
        let empty: [&str; 0] = [];
        let unhovered = HoverState::default();
        assert!(unhovered.is_path(empty));
        assert!(unhovered.contains_path(empty));
        assert!(!unhovered.is_path(["save"]));
        assert!(!unhovered.contains_path(["save"]));

        let hover = HoverState::intent(["left", "save"]);
        assert!(!hover.is_path(empty));
        assert!(hover.contains_path(empty));
    }
}
