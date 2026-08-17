use std::{cell::Cell, collections::HashMap, fmt, rc::Rc};

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Position, Rect},
};

use crate::Theme;
use crate::backdrop::dim_background;

use super::{
    ChildId, Component, Event, EventCtx, EventResult, FocusState, KeyCode, KeyEvent, ModalState,
    MouseButton, MouseEvent, MouseKind, MouseTracker, PaintCtx, Painter, RenderCtx, ScopeOptions,
    Step, TabWrap,
    component::{InteractionFlags, PaintTarget, TransientMap},
};

struct FocusBinding<State, Msg> {
    read: Box<dyn Fn(&State) -> &FocusState>,
    on_change: Box<dyn Fn(FocusState) -> Msg>,
}

struct ModalBinding<State> {
    read: Box<dyn Fn(&State) -> &ModalState>,
}

enum FocusAdvance {
    Move(FocusState),
    Consumed,
    Ignored,
}

/// What kind of layer a node roots, when it roots one.
///
/// A layer is a subtree painted above everything declared outside it. Every
/// kind shares the mechanism — a tag, a canvas, compositing order — and they
/// differ only in policy: a modal dims what is beneath it, consumes events
/// outside itself, and takes focus; a popup does none of that and instead
/// observes outside presses through its dismiss hook; a hint takes no input at
/// all. The differences live in [`LayerKind::policy`] and nowhere else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayerKind {
    Modal,
    Popup,
    Hint,
}

/// What a layer does to interaction, as data rather than as branches.
///
/// A layer is one mechanism — a tagged subtree with its own canvas. This is
/// the only thing that differs between kinds, so adding a kind means adding a
/// row to [`LayerKind::policy`] and nothing else. The base layer everything
/// else is declared into has [`LayerPolicy::base`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "a policy table: each flag is one independent layer behavior"
)]
struct LayerPolicy {
    /// Dim what is beneath before this layer composites.
    dims: bool,
    /// Interaction belongs to this layer alone: events landing outside it are
    /// consumed rather than routed. Distinct from pointer capture, which is a
    /// component's claim on one gesture.
    exclusive: bool,
    /// Focus resolves into this layer, and Tab is trapped at its root.
    holds_focus: bool,
    /// Keys stop at this layer's root instead of bubbling to whatever
    /// declared it.
    traps_keys: bool,
    /// The pointer can hit this layer at all. A layer that cannot is inert
    /// decoration: presses fall through to whatever is beneath it.
    hit_testable: bool,
    /// Anything inside this layer may hold focus. A layer that does not allow
    /// it is skipped by Tab and by every explicit focus request, whatever its
    /// contents claim through [`Component::is_focusable`].
    allows_focus: bool,
    /// A press outside this layer emits its dismiss hook.
    dismiss_on_outside_press: bool,
}

impl LayerPolicy {
    /// What everything declared outside any layer gets: no policy at all,
    /// except that it can be clicked.
    const fn base() -> Self {
        Self {
            dims: false,
            exclusive: false,
            holds_focus: false,
            traps_keys: false,
            hit_testable: true,
            allows_focus: true,
            dismiss_on_outside_press: false,
        }
    }
}

impl LayerKind {
    /// The whole difference between the layer kinds, in one table.
    const fn policy(self) -> LayerPolicy {
        match self {
            // Takes over: dims, claims interaction, holds focus, swallows keys.
            Self::Modal => LayerPolicy {
                dims: true,
                exclusive: true,
                holds_focus: true,
                traps_keys: true,
                ..LayerPolicy::base()
            },
            // Occludes its own footprint and dismisses on an outside press,
            // but never steals focus and lets keys reach its declarer.
            Self::Popup => LayerPolicy {
                dismiss_on_outside_press: true,
                ..LayerPolicy::base()
            },
            // Says something and takes nothing: not a pointer target, so a
            // press goes to whatever it covers, and not a focus target, so Tab
            // passes it by even if what is inside claims to be focusable.
            Self::Hint => LayerPolicy {
                hit_testable: false,
                allows_focus: false,
                ..LayerPolicy::base()
            },
        }
    }
}

pub(crate) struct Node<State, Msg> {
    /// This node's own segment of its identity path. The full path is the ids
    /// of the parent chain and this one, and is derived on demand through
    /// [`Surface::path_of`] rather than stored: building the tree is the
    /// hottest thing a frame does, and a stored path costs an allocation per
    /// node.
    id: ChildId,
    parent: Option<usize>,
    children: Vec<usize>,
    area: Rect,
    options: ScopeOptions,
    is_scope: bool,
    self_focusable: bool,
    focuses_on_click: bool,
    component: Option<Box<dyn Component<State, Msg>>>,
    layer: usize,
    /// `Some` exactly on layer roots, naming the kind this subtree is.
    layer_kind: Option<LayerKind>,
    /// Set on dismissible layer roots: the message emitted when a press lands
    /// outside (see `Ratcn::handle_mouse`).
    on_dismiss: Option<Box<dyn Fn() -> Msg>>,
}

impl<State, Msg> fmt::Debug for Node<State, Msg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Node")
            .field("id", &self.id)
            .field("parent", &self.parent)
            .field("children", &self.children)
            .field("area", &self.area)
            .field("options", &self.options)
            .field("is_scope", &self.is_scope)
            .field("self_focusable", &self.self_focusable)
            .field("focuses_on_click", &self.focuses_on_click)
            .field("component", &self.component.is_some())
            .field("layer", &self.layer)
            .field("layer_kind", &self.layer_kind)
            .field("on_dismiss", &self.on_dismiss.is_some())
            .finish()
    }
}

pub(crate) struct Surface<State, Msg> {
    nodes: Vec<Node<State, Msg>>,
    roots: Vec<usize>,
    /// Every layer root, in declaration order. A layer declared inside
    /// another lands after it, so the last entry is always the innermost.
    layer_roots: Vec<usize>,
    /// The policy of each layer, indexed by the layer number nodes carry.
    /// Index 0 is the base layer.
    layer_policies: Vec<LayerPolicy>,
}

impl<State, Msg> Default for Surface<State, Msg> {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            roots: Vec::new(),
            layer_roots: Vec::new(),
            layer_policies: vec![LayerPolicy::base()],
        }
    }
}

impl<State, Msg> fmt::Debug for Surface<State, Msg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Surface")
            .field("nodes", &self.nodes.len())
            .field("roots", &self.roots.len())
            .field("layer_roots", &self.layer_roots.len())
            .field("layer_policies", &self.layer_policies.len())
            .finish()
    }
}

impl<State, Msg> Surface<State, Msg> {
    /// The identity path of `index`, outermost first.
    ///
    /// Building it allocates, so it belongs to the consumers that need a path
    /// as a value — panic messages, the paths handed to components, the
    /// runtime's own hover path and app-held focus. Structural questions have
    /// non-allocating answers instead: [`Self::is_ancestor_or_self`] for
    /// containment, [`Self::path_is_prefix_of`] for testing a node against a
    /// stored path.
    fn path_of(&self, index: usize) -> Vec<ChildId> {
        let mut path = Vec::with_capacity(self.depth(index));
        let mut current = Some(index);
        while let Some(node) = current {
            path.push(self.nodes[node].id.clone());
            current = self.nodes[node].parent;
        }
        path.reverse();
        path
    }

    /// How many ids `index`'s identity path has.
    fn depth(&self, index: usize) -> usize {
        let mut depth = 0;
        let mut current = Some(index);
        while let Some(node) = current {
            depth += 1;
            current = self.nodes[node].parent;
        }
        depth
    }

    /// Whether `index`'s identity path is exactly `path`.
    ///
    /// It walks the parent chain against `path` back to front, so neither
    /// side is materialized. [`Self::path_is_prefix_of`] is the same walk
    /// with the prefix trimmed to the node's own depth first.
    fn path_is(&self, index: usize, path: &[ChildId]) -> bool {
        let mut current = Some(index);
        let mut rest = path;
        while let Some(node) = current {
            let Some((last, head)) = rest.split_last() else {
                return false;
            };
            if self.nodes[node].id != *last {
                return false;
            }
            current = self.nodes[node].parent;
            rest = head;
        }
        rest.is_empty()
    }

    /// Whether `index`'s identity path is `path` or a prefix of it — the
    /// question `path.starts_with(node_path)` asks, without building either.
    fn path_is_prefix_of(&self, index: usize, path: &[ChildId]) -> bool {
        let depth = self.depth(index);
        depth <= path.len() && self.path_is(index, &path[..depth])
    }

    /// Where `index` sits in this frame's focus and hover — the four flags
    /// [`PaintCtx`] reports.
    ///
    /// Answered here, against the finished tree, because that is the only
    /// place it can be: `focus` is the path [`Self::resolve_focus`] chose
    /// once every declaration was in, so no flag exists until declaring is
    /// over. Both pairs are the same two questions — is this the named node,
    /// and does the named path run through it — asked without materializing a
    /// path on either side.
    fn interaction_flags(
        &self,
        index: usize,
        focus: &FocusState,
        hover: &[ChildId],
    ) -> InteractionFlags {
        let (focused, contains_focus) = self.path_match(index, focus.path());
        let (hovered, contains_hover) = self.path_match(index, hover);
        InteractionFlags {
            focused,
            contains_focus,
            hovered,
            contains_hover,
        }
    }

    /// Whether `index`'s identity path *is* `path`, and whether it is a
    /// prefix of it — the leaf question and the within question, as a pair.
    ///
    /// Asked together because the second answers the first: a path can only
    /// equal `path` by being a prefix of it that runs the whole length. One
    /// depth walk and one comparison serve both, where asking separately
    /// walks the parent chain three times.
    fn path_match(&self, index: usize, path: &[ChildId]) -> (bool, bool) {
        let depth = self.depth(index);
        if depth > path.len() {
            return (false, false);
        }
        let within = self.path_is(index, &path[..depth]);
        (within && depth == path.len(), within)
    }

    /// Whether `node` is `ancestor` or one of its descendants.
    ///
    /// Identity paths are unique, so walking parent indices answers this
    /// exactly as comparing paths would, and without touching an id.
    fn is_ancestor_or_self(&self, ancestor: usize, node: usize) -> bool {
        let mut current = Some(node);
        while let Some(index) = current {
            if index == ancestor {
                return true;
            }
            current = self.nodes[index].parent;
        }
        false
    }

    fn has_hit_geometry(&self, index: usize) -> bool {
        let area = self.nodes[index].area;
        area.width > 0 && area.height > 0
    }

    fn participates(&self, index: usize) -> bool {
        let node = &self.nodes[index];
        (node.is_scope || self.has_hit_geometry(index))
            && node.parent.is_none_or(|parent| self.participates(parent))
    }

    fn children(&self, parent: Option<usize>) -> &[usize] {
        parent.map_or(self.roots.as_slice(), |index| {
            self.nodes[index].children.as_slice()
        })
    }

    /// Whether this node is inside the exclusive layer, and so can still be
    /// interacted with. With no exclusive layer open, everything can.
    ///
    /// Membership is containment in the tree, not layer number: a layer
    /// declared after the exclusive one takes a higher layer number but is not
    /// thereby inside it. Layer numbers order paint; this orders interaction,
    /// and only the ancestor chain can answer it. Checked on
    /// interaction targets (hit, focus leaves), not on ancestors: a nested
    /// layer root's ancestors provide identity and structure, not interaction.
    fn interactive(&self, index: usize) -> bool {
        self.exclusive_root()
            .is_none_or(|root| self.inside(index, root))
    }

    /// The policy of the layer `layer` names. Unknown numbers cannot occur —
    /// every layer registers its policy as it opens — but fall back to the
    /// base policy rather than panicking on a paint-only concern.
    fn policy(&self, layer: usize) -> LayerPolicy {
        self.layer_policies
            .get(layer)
            .copied()
            .unwrap_or_else(LayerPolicy::base)
    }

    /// The innermost open layer root whose policy satisfies `wants`.
    ///
    /// `layer_roots` is in declaration order and nesting appends, so scanning
    /// backwards finds the innermost first — the one on top.
    fn top_layer_root(&self, wants: impl Fn(LayerPolicy) -> bool) -> Option<usize> {
        self.layer_roots
            .iter()
            .rev()
            .copied()
            .find(|&root| wants(self.layer_kind_policy(root)))
    }

    /// The policy of the layer rooted at `root`.
    fn layer_kind_policy(&self, root: usize) -> LayerPolicy {
        self.nodes[root]
            .layer_kind
            .map_or_else(LayerPolicy::base, LayerKind::policy)
    }

    /// The layer that has taken interaction over, if one is open: everything
    /// outside it is inert.
    fn exclusive_root(&self) -> Option<usize> {
        self.top_layer_root(|policy| policy.exclusive)
    }

    /// The layer that owns focus, if one is open.
    fn focus_root(&self) -> Option<usize> {
        self.top_layer_root(|policy| policy.holds_focus)
    }

    /// Every modal root, outermost first — what `Ratcn::modals` validates the
    /// app's stack against.
    fn modal_roots(&self) -> impl Iterator<Item = usize> + '_ {
        self.layer_roots
            .iter()
            .copied()
            .filter(|&root| self.nodes[root].layer_kind == Some(LayerKind::Modal))
    }

    /// Whether `index` is `root` or one of its descendants.
    ///
    /// The one containment test. Layer numbers order paint and must never be
    /// used to answer this: a layer declared after another takes a higher
    /// number without being inside it.
    fn inside(&self, index: usize, root: usize) -> bool {
        self.is_ancestor_or_self(root, index)
    }

    /// The roots focus traversal works across: the topmost focus-holding layer
    /// root alone while one is open (Tab is trapped inside it), all roots
    /// otherwise.
    fn traversal_roots(&self) -> Vec<usize> {
        self.focus_root()
            .map_or_else(|| self.roots.clone(), |root| vec![root])
    }

    /// Whether some node's identity path is exactly `path`. The empty path
    /// names no node: every declaration has at least its own id.
    fn contains_declared_path(&self, path: &[ChildId]) -> bool {
        !path.is_empty() && self.nodes_along_path(path).len() == path.len()
    }

    /// The node indices along `path`, outermost first, stopping at the first
    /// segment this surface does not declare. A result shorter than `path` is
    /// how callers detect a path that no longer resolves.
    fn nodes_along_path(&self, path: &[ChildId]) -> Vec<usize> {
        let mut parent = None;
        let mut matched = Vec::new();
        for id in path {
            let Some(index) = self
                .children(parent)
                .iter()
                .copied()
                .find(|&index| self.nodes[index].id == *id)
            else {
                break;
            };
            matched.push(index);
            parent = Some(index);
        }
        matched
    }

    /// Whether `path` names something the pointer could be on: declared,
    /// participating, with hit geometry, and not shut out by an exclusive
    /// layer. The question [`Ratcn::resolve_hover`] asks of a frozen path.
    fn contains_hit_path(&self, path: &[ChildId]) -> bool {
        let matched = self.nodes_along_path(path);
        matched.len() == path.len()
            && matched.last().is_some_and(|&index| {
                self.participates(index) && self.has_hit_geometry(index) && self.interactive(index)
            })
    }

    fn contains_participating_path(&self, path: &[ChildId]) -> bool {
        let matched = self.nodes_along_path(path);
        matched.len() == path.len()
            && matched
                .last()
                .is_some_and(|&index| self.participates(index))
    }

    /// The topmost interactive node under `point`, across all layers at or
    /// above the floor: highest layer wins, then latest declaration within it.
    /// This is target *selection* — an event never falls through geometry to a
    /// lower layer; it routes to this one node and bubbles up its ancestors.
    fn hit_index(&self, point: Position) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (index, node) in self.nodes.iter().enumerate() {
            if self.policy(node.layer).hit_testable
                && self.interactive(index)
                && self.participates(index)
                && self.has_hit_geometry(index)
                && node.area.contains(point)
            {
                let key = (node.layer, index);
                if best.is_none_or(|best| key > best) {
                    best = Some(key);
                }
            }
        }
        best.map(|(_, index)| index)
    }

    fn hit_path(&self, point: Position) -> Option<Vec<ChildId>> {
        self.hit_index(point).map(|index| self.path_of(index))
    }

    fn is_layer_root(&self, index: usize) -> bool {
        self.nodes[index].layer_kind.is_some()
    }

    /// The ancestor chain a mouse event at `path` bubbles through: from the
    /// innermost enclosing layer root down to the hit node. A layer confines
    /// its pointer events — they are consumed at its root rather than
    /// delivered to the occluded content beneath or to the component that
    /// declared the layer. The bool reports whether such a boundary exists.
    fn mouse_bubble_chain(&self, path: &[ChildId]) -> (Vec<usize>, bool) {
        let mut matched = self.nodes_along_path(path);
        if let Some(position) = matched.iter().rposition(|&index| self.is_layer_root(index)) {
            matched.drain(..position);
            (matched, true)
        } else {
            (matched, false)
        }
    }

    /// Whether this node can hold focus itself, right now.
    ///
    /// The effective answer, not the claim: `Node::self_focusable` is only
    /// what the declaration asked for, and it is the last of five conditions
    /// checked here. The node must also still be part of the tree, have
    /// hit geometry, sit inside the exclusive layer if one is open, and belong
    /// to a layer that allows focus at all.
    ///
    /// See [`Self::focusable`] for the same question about a node *or any of
    /// its descendants*.
    fn takes_focus(&self, index: usize) -> bool {
        self.participates(index)
            && self.has_hit_geometry(index)
            && self.interactive(index)
            && self.policy(self.nodes[index].layer).allows_focus
            && self.nodes[index].self_focusable
    }

    /// Whether focus can land anywhere in this subtree — on the node itself
    /// or on any descendant. Traversal uses this to decide whether a container
    /// is worth descending into; [`Self::takes_focus`] answers for the node
    /// alone.
    fn focusable(&self, index: usize) -> bool {
        self.participates(index)
            && (self.takes_focus(index)
                || self.nodes[index]
                    .children
                    .iter()
                    .any(|&child| self.focusable(child)))
    }

    /// The first focusable index among `candidates`, scanning declaration order
    /// forward or reverse per `direction`.
    fn find_focusable(&self, candidates: &[usize], direction: Step) -> Option<usize> {
        let mut iter = candidates.iter().copied();
        match direction {
            Step::Forward => iter.find(|&index| self.focusable(index)),
            Step::Backward => iter.rfind(|&index| self.focusable(index)),
        }
    }

    fn edge_child(&self, parent: Option<usize>, direction: Step) -> Option<usize> {
        let candidates = parent.map_or_else(
            || self.traversal_roots(),
            |index| self.nodes[index].children.clone(),
        );
        self.find_focusable(&candidates, direction)
    }

    fn extend_to_edge(&self, index: usize, direction: Step, path: &mut Vec<ChildId>) -> bool {
        path.push(self.nodes[index].id.clone());
        if let Some(child) = self.edge_child(Some(index), direction) {
            return self.extend_to_edge(child, direction, path);
        }
        if self.takes_focus(index) {
            true
        } else {
            path.pop();
            false
        }
    }

    fn edge_focus(&self, parent: Option<usize>, direction: Step) -> Option<FocusState> {
        let index = self.edge_child(parent, direction)?;
        self.descend_focus(index, direction)
    }

    /// The focus path produced by descending from `index` to its first
    /// focusable leaf, seeded with the node's own ancestor prefix — correct
    /// whether the node is a tree root or a nested layer root.
    fn descend_focus(&self, index: usize, direction: Step) -> Option<FocusState> {
        let mut path = self.nodes[index]
            .parent
            .map_or_else(Vec::new, |parent| self.path_of(parent));
        self.extend_to_edge(index, direction, &mut path)
            .then(|| FocusState::intent(path))
    }

    /// Resolve an app-held focus path against this surface's actual structure
    /// and geometry: the path that is painted as focused and that events route
    /// to.
    ///
    /// The one focus-resolution function. Rendering calls it between
    /// declaring the tree and painting it; event routing calls it against the
    /// retained surface. Both sides resolving through the same function over
    /// the same tree is what guarantees render and routing agree — there is no
    /// second resolution to drift from.
    ///
    /// - An empty path resolves to the first participating focus candidate,
    ///   descended to its first focusable leaf (startup focus).
    /// - A path naming a container descends to the container's first focusable
    ///   leaf.
    /// - A path this surface did not declare, or that is no longer focusable,
    ///   stays as it is — parked, never silently retargeted.
    /// - With a focus-holding layer open, a declared path outside it resolves
    ///   into that layer; an absent path stays parked even then, so render and
    ///   routing agree on it.
    fn resolve_focus(&self, stored: &FocusState) -> FocusState {
        if let Some(root) = self.focus_root()
            && !self.path_is_prefix_of(root, stored.path())
        {
            // `ModalState::open` records its focus intent as the modal's
            // bare id; that intent resolves into the modal wherever the
            // modal was declared.
            let open_intent = stored.path() == [self.nodes[root].id.clone()];
            // Beyond that, the layer steals focus from an empty path and
            // from paths it occludes — but an absent path stays parked,
            // so render and routing keep agreeing on it.
            if !open_intent
                && !stored.path().is_empty()
                && !self.contains_declared_path(stored.path())
            {
                return stored.clone();
            }
            return self.descend_focus(root, Step::Forward).unwrap_or_else(|| {
                if stored.path().is_empty() {
                    FocusState::default()
                } else {
                    stored.clone()
                }
            });
        }
        if stored.path().is_empty() {
            return self.edge_focus(None, Step::Forward).unwrap_or_default();
        }
        let matched = self.nodes_along_path(stored.path());
        if matched.len() != stored.path().len() {
            return stored.clone();
        }
        let Some(&target) = matched.last() else {
            return stored.clone();
        };
        let Some(child) = self.edge_child(Some(target), Step::Forward) else {
            return stored.clone();
        };
        let mut path = stored.path().to_vec();
        self.extend_to_edge(child, Step::Forward, &mut path);
        FocusState::intent(path)
    }

    fn explicit_focus(&self, path: &[ChildId]) -> Option<FocusState> {
        let matched = self.nodes_along_path(path);
        if matched.len() != path.len() {
            return None;
        }
        let &target = matched.last()?;
        if !matched.iter().all(|&index| self.focusable(index)) {
            return None;
        }
        let mut focus = path.to_vec();
        if let Some(child) = self.edge_child(Some(target), Step::Forward) {
            self.extend_to_edge(child, Step::Forward, &mut focus);
        } else if !self.takes_focus(target) {
            return None;
        }
        Some(FocusState::intent(focus))
    }

    fn hover_focus(
        &self,
        path: &[ChildId],
        focus: &FocusState,
        root_options: &ScopeOptions,
    ) -> Option<FocusState> {
        let matched = self.nodes_along_path(path);
        if matched.len() != path.len() {
            return None;
        }

        // `matched` resolved `path` whole, so the node at position `n` is the
        // node named by `path[..=n]` — the child's own identity path, already
        // materialized by the caller.
        let boundaries = std::iter::once(root_options)
            .chain(matched.iter().map(|&parent| &self.nodes[parent].options));
        boundaries
            .zip(matched.iter().copied())
            .enumerate()
            .find_map(|(position, (options, child))| {
                let child_path = &path[..=position];
                (options.hover_focus
                    && !focus.path().starts_with(child_path)
                    && self.focusable(child))
                .then(|| self.explicit_focus(child_path))
                .flatten()
            })
    }

    fn next_focus(
        &self,
        focus: &FocusState,
        direction: Step,
        root_options: &ScopeOptions,
    ) -> FocusAdvance {
        // A path parked outside the focus-holding layer belongs to nothing
        // traversal may use: the scope holding it is covered, so consulting
        // its `tab_wrap` would let a wrapping pane swallow Tab forever with
        // the layer unreachable. Start from that layer's own edge instead.
        if let Some(root) = self.focus_root()
            && !self.path_is_prefix_of(root, focus.path())
        {
            return self
                .edge_focus(None, direction)
                .map_or(FocusAdvance::Consumed, FocusAdvance::Move);
        }
        let matched = self.nodes_along_path(focus.path());
        if matched.len() != focus.path().len() {
            let parent = matched.last().copied();
            if let Some(next) = self.edge_focus(parent, direction) {
                return FocusAdvance::Move(next);
            }
            let options = parent.map_or(root_options, |index| &self.nodes[index].options);
            if options.tab_wrap == TabWrap::Wrap {
                return FocusAdvance::Consumed;
            }
            let Some(current) = parent else {
                return FocusAdvance::Ignored;
            };
            return self.next_from_scope(current, focus, direction, root_options);
        }

        let Some(current) = matched.last().copied() else {
            return self
                .edge_focus(None, direction)
                .map_or(FocusAdvance::Ignored, FocusAdvance::Move);
        };
        self.next_from_scope(current, focus, direction, root_options)
    }

    fn next_from_scope(
        &self,
        mut current: usize,
        focus: &FocusState,
        direction: Step,
        root_options: &ScopeOptions,
    ) -> FocusAdvance {
        loop {
            let parent = self.nodes[current].parent;
            let siblings = self.children(parent);
            let position = siblings
                .iter()
                .position(|&index| index == current)
                .expect("focused node is registered under its parent");
            let remaining = match direction {
                Step::Forward => &siblings[position + 1..],
                Step::Backward => &siblings[..position],
            };
            let next = self.find_focusable(remaining, direction);
            if let Some(next) = next {
                let mut path = parent.map_or_else(Vec::new, |index| self.path_of(index));
                self.extend_to_edge(next, direction, &mut path);
                return FocusAdvance::Move(FocusState::intent(path));
            }

            // A focus-holding layer root traps Tab regardless of where it sits
            // in the tree; otherwise the enclosing scope decides.
            let tab_wrap = if self.focus_root() == Some(current) {
                TabWrap::Wrap
            } else {
                parent.map_or(root_options.tab_wrap, |index| {
                    self.nodes[index].options.tab_wrap
                })
            };
            if tab_wrap == TabWrap::Wrap {
                let Some(next) = self.edge_child(parent, direction) else {
                    return FocusAdvance::Consumed;
                };
                let mut path = parent.map_or_else(Vec::new, |index| self.path_of(index));
                self.extend_to_edge(next, direction, &mut path);
                let next = FocusState::intent(path);
                return if next == *focus {
                    FocusAdvance::Consumed
                } else {
                    FocusAdvance::Move(next)
                };
            }
            let Some(parent) = parent else {
                return FocusAdvance::Ignored;
            };
            current = parent;
        }
    }
}

type DeferredPaint<State> = Box<dyn FnOnce(&mut Painter<'_, '_>, &State)>;

type PaintThunk<State> = Box<dyn FnOnce(&mut PaintCtx<'_, '_, State>)>;

/// One entry of the frame's paint queue: what to draw, and where it lands.
///
/// Declaring and drawing are separate walks. The declaration walk queues
/// these in the order it reaches them and draws nothing; [`RenderPass::replay_paint`]
/// runs the queue afterwards, when the tree is complete and focus has
/// resolved. Order in the queue is therefore the paint order, and a
/// component is queued where it opens, so its own paint precedes its
/// descendants'.
struct QueuedPaint<State> {
    /// Index into `RenderPass::canvases`, or `None` to paint on the frame.
    /// Fixed where the op was queued, so an op belongs to the layer that was
    /// open at its declaration whatever is open at replay.
    canvas: Option<usize>,
    op: PaintOp<State>,
}

/// Every op names the node whose position it paints at, because that node is
/// what its interaction flags are read from once focus has resolved. Only the
/// area is captured: it is a declaration fact, settled where the op was
/// queued, while the flags are not facts yet.
enum PaintOp<State> {
    /// Call [`Component::paint`] on the component installed at this node.
    Node { index: usize, area: Rect },
    /// Run a closure queued through [`RenderCtx::paint`]. `node` is the
    /// declaration it was reached from, or `None` at the root, which has no
    /// identity and therefore no flags.
    Thunk {
        node: Option<usize>,
        area: Rect,
        paint: PaintThunk<State>,
    },
    /// Run the deferred thunks registered in `layer`, at the point that
    /// layer's declaration closed — which is what keeps [`RenderCtx::defer_paint`]
    /// above its layer's content.
    FlushDeferred { layer: usize },
}

impl<State> PaintOp<State> {
    /// The declaration this op paints at, and so the one its interaction
    /// flags come from. `None` for the root and for deferred paint, neither
    /// of which has an identity.
    const fn node(&self) -> Option<usize> {
        match self {
            Self::Node { index, .. } => Some(*index),
            Self::Thunk { node, .. } => *node,
            Self::FlushDeferred { .. } => None,
        }
    }
}

/// The two things replay can hand a [`PaintCtx`] to, once the op that named
/// them has been resolved against the pass. Exists so both are given the
/// context by one piece of code rather than two.
enum PaintBody<'a, State, Msg> {
    Component(&'a mut dyn Component<State, Msg>),
    Thunk(PaintThunk<State>),
}

/// One layer's private paint surface.
///
/// A layer subtree is declared inline, wherever its owner lives in the tree,
/// but must paint *above* everything declared outside it — including siblings
/// declared later. So its widgets paint into this canvas instead of the
/// frame, and the canvases composite onto the frame in discovery order once
/// the whole pass has painted. `painted` records the areas the layer's
/// widgets declared; only those rects blit — so a modal declared over the
/// full screen composites just the box it painted — and each rect composites
/// opaquely, unwritten cells included.
pub(crate) struct LayerCanvas {
    /// The kind, not its policy: policy is derived wherever it is read, so
    /// this stays one fact rather than a copy that could go stale.
    kind: LayerKind,
    area: Rect,
    pub(crate) buffer: Buffer,
    painted: Vec<Rect>,
}

impl LayerCanvas {
    fn new(kind: LayerKind, area: Rect) -> Self {
        Self {
            kind,
            area,
            buffer: Buffer::empty(area),
            painted: Vec::new(),
        }
    }

    /// Record that `area` was painted, clipped to the canvas.
    pub(crate) fn mark_painted(&mut self, area: Rect) {
        let clipped = area.intersection(self.area);
        if clipped.width > 0 && clipped.height > 0 {
            self.painted.push(clipped);
        }
    }
}

/// The declaration environment: everything a declaration needs that is not
/// specific to the node being declared.
///
/// One value travels down the declaration call chain instead of seven
/// positional parameters, and it is the only thing a [`RenderPass`] method
/// needs besides the node's own identity and options. `area` rides along
/// because it is the member that changes per declaration — whoever declares a
/// child chooses where it goes — while the rest is constant for the whole
/// pass.
pub(crate) struct DeclarationEnv<'a, State> {
    /// The whole terminal frame. Declaring never paints, so the frame itself
    /// stays with [`Ratcn::render`] until the replay; its area is the only part
    /// a declaration can act on, and it is constant for the pass.
    pub(crate) frame_area: Rect,
    pub(crate) area: Rect,
    pub(crate) state: &'a State,
    pub(crate) theme: &'a Theme,
    pub(crate) transients: Option<&'a mut TransientMap>,
    pub(crate) depth: usize,
}

impl<'a, State> DeclarationEnv<'a, State> {
    /// The environment for a root declaration: the app's own closure, covering
    /// the whole frame at depth zero.
    fn root(
        frame_area: Rect,
        state: &'a State,
        theme: &'a Theme,
        transients: &'a mut TransientMap,
    ) -> Self {
        Self {
            frame_area,
            area: frame_area,
            state,
            theme,
            transients: Some(transients),
            depth: 0,
        }
    }

    /// The same environment reborrowed for the declarations *inside* the node
    /// just opened: one level deeper, over `area`.
    ///
    /// Depth counts declaration nesting rather than tree nesting, which is why
    /// it advances here — at the point a node starts parenting others — and
    /// not in [`RenderPass::begin_node`].
    fn nested(&mut self, area: Rect) -> DeclarationEnv<'_, State> {
        DeclarationEnv {
            frame_area: self.frame_area,
            area,
            state: self.state,
            theme: self.theme,
            transients: self.transients.as_deref_mut(),
            depth: self.depth + 1,
        }
    }
}

/// What a declaration adds to the tree, and what it is allowed to do there.
///
/// These three travel together because they are decided together. Keeping
/// them in a named pair also stops the two signatures that carry them from
/// drifting out of step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NodeRole {
    /// A scope rather than a component: it sits *behind* its descendants when
    /// hit-testing, so a press only reaches it when nothing inside was hit.
    is_scope: bool,
    /// This node can hold focus itself, rather than only parenting things that
    /// can.
    self_focusable: bool,
    /// Focus arrives on the `Click` rather than on the `Down`.
    focuses_on_click: bool,
}

impl NodeRole {
    /// A scope. It never focuses on click: that policy belongs to controls,
    /// and a scope is a grouping, not a control.
    fn scope(self_focusable: bool) -> Self {
        Self {
            is_scope: true,
            self_focusable,
            focuses_on_click: false,
        }
    }

    /// A component, which decides both of its own flags.
    fn component(self_focusable: bool, focuses_on_click: bool) -> Self {
        Self {
            is_scope: false,
            self_focusable,
            focuses_on_click,
        }
    }
}

pub(crate) struct RenderPass<State, Msg> {
    surface: Surface<State, Msg>,
    parent_stack: Vec<usize>,
    /// The identity path of the open declaration chain, maintained in step
    /// with `parent_stack` by [`Self::enter_node`] and [`Self::leave_node`].
    /// One cursor for the whole pass: the alternative is rebuilding a path
    /// from parent links every time a declaration asks for one, which is the
    /// hot case this pass has to keep cheap.
    path_cursor: Vec<ChildId>,
    /// Deferred paint thunks, each tagged with the layer it was registered
    /// in: a layer's thunks flush into its canvas when the layer ends, the
    /// base layer's flush onto the frame after every canvas has composited,
    /// which is what makes root-level `defer_paint` the topmost slot.
    deferred: Vec<(usize, DeferredPaint<State>)>,
    /// Every paint this frame owes, in the order the declaration walk reached
    /// it, replayed by [`Self::replay_paint`] once the walk is over.
    paint_queue: Vec<QueuedPaint<State>>,
    /// Where the pointer is, and what it rests on, as the runtime knew both
    /// when this pass started. Hover is pre-frame data — it was resolved
    /// against the last committed surface — so unlike focus it can be read
    /// while declaring, by [`RenderCtx::pointer_within`] and by
    /// [`RenderCtx::hover_position`]. [`Self::replay_paint`] derives the paint
    /// flags from the same path.
    hover_position: Option<Position>,
    hover_path: Vec<ChildId>,
    /// Set when any declaration region unwinds — see [`Self::guarded`]. A
    /// poisoned pass can keep declaring (a component may have caught the
    /// panic) but can never commit.
    failed: Rc<Cell<bool>>,
    /// Layer ids handed out so far this pass. 0 is the base layer; each layer
    /// declaration takes the next id, so discovery order is stacking order.
    layers_declared: usize,
    /// The layer currently being declared into, innermost last. Empty means
    /// the base layer.
    layer_stack: Vec<usize>,
    /// One canvas per declared layer, in discovery order.
    canvases: Vec<LayerCanvas>,
    /// Indices into `canvases` for the layers currently open, innermost last.
    canvas_stack: Vec<usize>,
}

/// Poisons the pass when dropped by unwinding. Drop runs while a panic
/// unwinds even when an ancestor catches it, which is what keeps a
/// declaration panic caught by component code from committing a half-built
/// surface.
struct PoisonOnUnwind {
    failed: Rc<Cell<bool>>,
    armed: bool,
}

impl Drop for PoisonOnUnwind {
    fn drop(&mut self) {
        if self.armed {
            self.failed.set(true);
        }
    }
}

impl<State, Msg> RenderPass<State, Msg> {
    fn new() -> Self {
        Self {
            surface: Surface::default(),
            parent_stack: Vec::new(),
            path_cursor: Vec::new(),
            deferred: Vec::new(),
            paint_queue: Vec::new(),
            hover_position: None,
            hover_path: Vec::new(),
            failed: Rc::new(Cell::new(false)),
            layers_declared: 0,
            layer_stack: Vec::new(),
            canvases: Vec::new(),
            canvas_stack: Vec::new(),
        }
    }

    /// The layer currently being declared into; 0 is the base layer.
    fn current_layer(&self) -> usize {
        self.layer_stack.last().copied().unwrap_or(0)
    }

    /// The identity path of the declaration currently being declared into —
    /// the key [`RenderCtx::transient`] reads the transient store with.
    pub(crate) fn current_path(&self) -> Option<&[ChildId]> {
        (!self.path_cursor.is_empty()).then_some(self.path_cursor.as_slice())
    }

    /// Whether the hovered path runs through the declaration currently open.
    /// Empty at the root declaration, which owns no path: the question is then
    /// whether anything at all is hovered.
    pub(crate) fn pointer_within_current(&self) -> bool {
        !self.hover_path.is_empty()
            && self
                .hover_path
                .starts_with(self.current_path().unwrap_or_default())
    }

    /// Open `index` as the parent of everything declared until the matching
    /// [`Self::leave_node`], extending the path cursor by its id.
    ///
    /// The two stacks move together and only here, so
    /// `path_cursor[i] == nodes[parent_stack[i]].id` holds unconditionally and
    /// the cursor is the open chain's path by construction. That survives a
    /// nested declaration panic a component catches: neither stack pops while
    /// unwinding, so both stay equally deep.
    fn enter_node(&mut self, index: usize) {
        self.path_cursor.push(self.surface.nodes[index].id.clone());
        self.parent_stack.push(index);
    }

    /// Close the innermost open declaration.
    fn leave_node(&mut self) {
        self.parent_stack.pop();
        self.path_cursor.pop();
    }

    /// The canvas the ops queued right now belong to, or `None` for the
    /// frame.
    fn active_canvas(&self) -> Option<usize> {
        self.canvas_stack.last().copied()
    }

    /// Queue an op for the layer currently being declared into.
    fn queue(&mut self, op: PaintOp<State>) {
        let canvas = self.active_canvas();
        self.paint_queue.push(QueuedPaint { canvas, op });
    }

    /// Queue a closure registered through [`RenderCtx::paint`], tagged with
    /// the declaration it was reached from so replay can read that node's
    /// flags.
    pub(crate) fn queue_thunk(
        &mut self,
        area: Rect,
        paint: impl FnOnce(&mut PaintCtx<'_, '_, State>) + 'static,
    ) {
        let node = self.parent_stack.last().copied();
        self.queue(PaintOp::Thunk {
            node,
            area,
            paint: Box::new(paint),
        });
    }

    /// Queue the just-opened node's own paint. `area` is its paint
    /// allocation, which [`Component::interaction_area`] may have narrowed
    /// for the node itself but never for what it draws.
    fn queue_node(&mut self, index: usize, area: Rect) {
        self.queue(PaintOp::Node { index, area });
    }

    /// Open a layer, declare its root subtree through `declare_root`, and
    /// close it.
    ///
    /// The single place the layer lifecycle is written. The root index is
    /// recorded *before* the subtree declares, so a layer declared inside this
    /// one lands after it and `layer_roots` reads outermost-first — which is
    /// what makes `top_layer_root` find the innermost.
    fn layer(&mut self, kind: LayerKind, area: Rect, declare_root: impl FnOnce(&mut Self, usize)) {
        self.begin_layer(kind, area);
        let index = self.surface.nodes.len();
        self.surface.layer_roots.push(index);
        declare_root(self, index);
        self.surface.nodes[index].layer_kind = Some(kind);
        self.end_layer();
    }

    /// Open a layer: subsequent declarations carry its tag, and their paint
    /// lands on a fresh canvas composited after the frame.
    fn begin_layer(&mut self, kind: LayerKind, area: Rect) {
        self.layers_declared += 1;
        self.layer_stack.push(self.layers_declared);
        // Registered by layer number so any node can ask what its layer does,
        // not just the root.
        debug_assert_eq!(self.surface.layer_policies.len(), self.layers_declared);
        self.surface.layer_policies.push(kind.policy());
        self.canvases.push(LayerCanvas::new(kind, area));
        self.canvas_stack.push(self.canvases.len() - 1);
    }

    /// Close the innermost layer, queueing its deferred paint behind
    /// everything the layer declared.
    fn end_layer(&mut self) {
        let layer = self
            .layer_stack
            .pop()
            .expect("end_layer closes a layer begin_layer opened");
        // Queued rather than run here: the layer's own content has only been
        // queued so far, and deferred paint has to land on top of it.
        self.queue(PaintOp::FlushDeferred { layer });
        self.canvas_stack
            .pop()
            .expect("a declared layer opened a canvas");
    }

    /// Run the deferred thunks registered in `layer` into its canvas.
    fn flush_deferred_for(
        &mut self,
        layer: usize,
        theme: &Theme,
        state: &State,
        canvas_index: usize,
    ) {
        self.guarded(|pass| {
            let mut thunks = Vec::new();
            let mut index = 0;
            while index < pass.deferred.len() {
                if pass.deferred[index].0 == layer {
                    thunks.push(pass.deferred.remove(index).1);
                } else {
                    index += 1;
                }
            }
            for thunk in thunks {
                let mut painter = Painter {
                    target: PaintTarget::Canvas(&mut pass.canvases[canvas_index]),
                    theme,
                };
                thunk(&mut painter, state);
            }
        });
    }

    /// Run `f` as one declaration region: if it unwinds — a panicking
    /// component, or the runtime's own validation — the pass is poisoned and
    /// can never commit, no matter who catches the panic. The single
    /// mechanism behind "a failed pass never replaces the surface"; every
    /// entry point that runs user code or validates a declaration goes
    /// through here.
    pub(crate) fn guarded<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let mut poison = PoisonOnUnwind {
            failed: Rc::clone(&self.failed),
            armed: true,
        };
        let result = f(self);
        poison.armed = false;
        result
    }

    fn begin_node(
        &mut self,
        id: ChildId,
        area: Rect,
        options: ScopeOptions,
        role: NodeRole,
    ) -> usize {
        let NodeRole {
            is_scope,
            self_focusable,
            focuses_on_click,
        } = role;
        let parent = self.parent_stack.last().copied();
        let siblings = parent.map_or(self.surface.roots.as_slice(), |index| {
            self.surface.nodes[index].children.as_slice()
        });
        assert!(
            !siblings
                .iter()
                .any(|&index| self.surface.nodes[index].id == id),
            "duplicate child id `{id}` in one declaration scope"
        );
        let index = self.surface.nodes.len();
        let layer = self.current_layer();
        self.surface.nodes.push(Node {
            id,
            parent,
            children: Vec::new(),
            area,
            options,
            is_scope,
            self_focusable,
            focuses_on_click,
            component: None,
            layer,
            layer_kind: None,
            on_dismiss: None,
        });
        if let Some(parent) = parent {
            self.surface.nodes[parent].children.push(index);
        } else {
            self.surface.roots.push(index);
        }
        index
    }

    pub(crate) fn render_component(
        &mut self,
        id: ChildId,
        component: impl Component<State, Msg> + 'static,
        mut env: DeclarationEnv<'_, State>,
    ) {
        let state = env.state;
        self.guarded(|pass| {
            let mut component: Box<dyn Component<State, Msg>> = Box::new(component);
            // Every claim the runtime needs before descendants exist is read
            // here, in this order: focus for the whole frame is decided in one
            // pass, so none of it may depend on what painting produces.
            component.prepare(state);
            let options = component.scope_options();
            let self_focusable = component.is_focusable(state);
            let focuses_on_click = component.focuses_on_click(state);
            let role = NodeRole::component(options.focusable || self_focusable, focuses_on_click);
            let area = env.area;
            let interaction_area = component.interaction_area(area);
            assert!(
                interaction_area.width == 0
                    || interaction_area.height == 0
                    || (interaction_area.x >= area.x
                        && interaction_area.y >= area.y
                        && interaction_area.right() <= area.right()
                        && interaction_area.bottom() <= area.bottom()),
                "Component::interaction_area returned {interaction_area:?}, which is not fully contained in paint area {area:?}"
            );
            // The node hit-tests against `interaction_area`, but its children
            // are declared over the full paint `area`: a component may narrow
            // what it responds to without narrowing where it draws.
            let index = pass.begin_node(id, interaction_area, options, role);
            pass.enter_node(index);
            // Queued before the subtree declares, so the component's own
            // paint replays ahead of its descendants' — the paint-before-
            // children contract, kept by position in the queue rather than by
            // each component's care.
            pass.queue_node(index, area);
            pass.declare(env.nested(area), |ctx| component.render(ctx));
            pass.leave_node();
            pass.surface.nodes[index].component = Some(component);
        });
    }

    /// Validate that no other modal root this pass carries the same id — the
    /// app-owned [`ModalState`] stack is id-keyed, so two modal roots with one
    /// id would make its validation ambiguous.
    fn assert_unique_modal_id(&self, id: &ChildId) {
        assert!(
            !self
                .surface
                .modal_roots()
                .any(|index| &self.surface.nodes[index].id == id),
            "duplicate modal root id `{id}`"
        );
    }

    pub(crate) fn modal(
        &mut self,
        id: ChildId,
        component: impl Component<State, Msg> + 'static,
        env: DeclarationEnv<'_, State>,
    ) {
        self.guarded(|pass| {
            pass.assert_unique_modal_id(&id);
            let area = env.area;
            pass.layer(LayerKind::Modal, area, |pass, _| {
                pass.render_component(id, component, env);
            });
        });
    }

    /// The scope form of [`modal`](Self::modal): same layer lifecycle, but the
    /// root is an app-declared scope rather than a component.
    pub(crate) fn modal_scope(
        &mut self,
        id: ChildId,
        options: ScopeOptions,
        env: DeclarationEnv<'_, State>,
        declare: impl FnOnce(&mut RenderCtx<'_, State, Msg>),
    ) {
        self.guarded(|pass| {
            pass.assert_unique_modal_id(&id);
            let area = env.area;
            pass.layer(LayerKind::Modal, area, |pass, _| {
                pass.scope(id, options, env, declare);
            });
        });
    }

    /// Declare a layer whose root is an app-declared scope rather than a
    /// component: the popup and hint form of [`layer`](Self::layer).
    ///
    /// `on_dismiss` belongs to the caller rather than to the kind, because
    /// only a kind whose policy has `dismiss_on_outside_press` can ever fire
    /// one — hints pass `None` instead of carrying a hook that never runs.
    pub(crate) fn layer_scope(
        &mut self,
        id: ChildId,
        kind: LayerKind,
        options: ScopeOptions,
        on_dismiss: Option<Box<dyn Fn() -> Msg>>,
        env: DeclarationEnv<'_, State>,
        declare: impl FnOnce(&mut RenderCtx<'_, State, Msg>),
    ) {
        debug_assert!(
            on_dismiss.is_none() || kind.policy().dismiss_on_outside_press,
            "a dismiss hook on a layer kind that never dismisses"
        );
        self.guarded(|pass| {
            let area = env.area;
            pass.layer(kind, area, |pass, index| {
                pass.scope(id, options, env, declare);
                pass.surface.nodes[index].on_dismiss = on_dismiss;
            });
        });
    }

    pub(crate) fn scope(
        &mut self,
        id: ChildId,
        options: ScopeOptions,
        mut env: DeclarationEnv<'_, State>,
        declare: impl FnOnce(&mut RenderCtx<'_, State, Msg>),
    ) {
        self.guarded(|pass| {
            let role = NodeRole::scope(options.focusable);
            let area = env.area;
            let index = pass.begin_node(id, area, options, role);
            pass.enter_node(index);
            pass.declare(env.nested(area), declare);
            pass.leave_node();
        });
    }

    /// Run one declaration closure over the current parent node. The single
    /// construction site for declaration [`RenderCtx`]s: root, scope, modal, and
    /// component render all pass through here.
    fn declare(
        &mut self,
        env: DeclarationEnv<'_, State>,
        declare: impl FnOnce(&mut RenderCtx<'_, State, Msg>),
    ) {
        let DeclarationEnv {
            frame_area,
            area,
            state,
            theme,
            transients,
            depth,
        } = env;
        let hover_position = self.hover_position;
        self.guarded(|pass| {
            let mut ctx = RenderCtx {
                frame_area,
                area,
                theme,
                hover_position,
                transients,
                depth,
                pass,
                state,
            };
            declare(&mut ctx);
        });
    }

    pub(crate) fn defer_paint(
        &mut self,
        paint: impl FnOnce(&mut Painter<'_, '_>, &State) + 'static,
    ) {
        self.deferred.push((self.current_layer(), Box::new(paint)));
    }

    /// Draw the frame: run every queued op in declaration order, each onto
    /// the surface its declaration belonged to.
    ///
    /// This is the whole of the frame's painting apart from compositing.
    /// Running it after the walk rather than during it is what lets every
    /// paint read the finished tree and the focus resolved over it — `focus`
    /// here is that resolved path, not the app's stored one, and the flags
    /// each op paints with are derived from it now because there was nothing
    /// to derive them from earlier. The hover half needs no resolving: it is
    /// the runtime's own path, fixed for the whole frame.
    ///
    /// Only a pass that has already passed every check gets here, so the
    /// queue is either run whole or not at all: an already-poisoned pass draws
    /// nothing rather than putting a frame on screen that no event will route
    /// against. Each op is then guarded exactly as deferred paint is — a
    /// panicking paint poisons the pass, and the cells it wrote before
    /// panicking are the one thing that cannot be taken back.
    fn replay_paint(
        &mut self,
        frame: &mut Frame,
        state: &State,
        theme: &Theme,
        focus: &FocusState,
    ) {
        // Dead as things stand — `assert_valid` rejects a poisoned pass before
        // this is reached — and kept as the second half of the guarantee: it
        // is what would still hold if replay were ever moved ahead of the
        // checks.
        if self.failed.get() {
            return;
        }
        for queued in std::mem::take(&mut self.paint_queue) {
            let QueuedPaint { canvas, op } = queued;
            let frame = &mut *frame;
            self.guarded(|pass| {
                // Read before the component borrow below, which needs the
                // surface mutably. The root declaration has no node, and so
                // no flags.
                let flags = op.node().map_or_else(InteractionFlags::default, |index| {
                    pass.surface
                        .interaction_flags(index, focus, &pass.hover_path)
                });
                let (area, paint) = match op {
                    PaintOp::FlushDeferred { layer } => {
                        let index = canvas.expect("a layer's flush names that layer's canvas");
                        pass.flush_deferred_for(layer, theme, state, index);
                        return;
                    }
                    PaintOp::Node { index, area } => {
                        // `assert_valid`'s completeness check ran before
                        // replay, so every node here has its component.
                        let component = pass.surface.nodes[index]
                            .component
                            .as_deref_mut()
                            .expect("a checked pass installed every node's component");
                        (area, PaintBody::Component(component))
                    }
                    PaintOp::Thunk { area, paint, .. } => (area, PaintBody::Thunk(paint)),
                };
                let target = match canvas {
                    Some(index) => PaintTarget::Canvas(&mut pass.canvases[index]),
                    None => PaintTarget::Frame(frame),
                };
                let mut ctx = PaintCtx {
                    target,
                    theme,
                    area,
                    focused: flags.focused,
                    contains_focus: flags.contains_focus,
                    hovered: flags.hovered,
                    contains_hover: flags.contains_hover,
                    hover_position: pass.hover_position,
                    state,
                };
                match paint {
                    PaintBody::Component(component) => component.paint(&mut ctx),
                    PaintBody::Thunk(paint) => paint(&mut ctx),
                }
            });
        }
    }

    /// Finish the frame's painting: composite every layer canvas over
    /// the frame in discovery order — a modal dims what is beneath it first —
    /// then flush the base declaration's deferred thunks on top of the
    /// result, making root-level [`RenderCtx::defer_paint`] the topmost
    /// decoration slot (toast stacks, drag ghosts).
    fn finish_frame(&mut self, frame: &mut Frame, state: &State, theme: &Theme) {
        for canvas in &self.canvases {
            if canvas.kind.policy().dims {
                dim_background(frame.buffer_mut(), canvas.area, theme.background);
            }
            let frame_area = frame.area();
            let buffer = frame.buffer_mut();
            for &rect in &canvas.painted {
                let rect = rect.intersection(frame_area);
                for y in rect.y..rect.bottom() {
                    for x in rect.x..rect.right() {
                        if let (Some(target), Some(source)) =
                            (buffer.cell_mut((x, y)), canvas.buffer.cell((x, y)))
                        {
                            *target = source.clone();
                        }
                    }
                }
            }
        }
        self.guarded(|pass| {
            let mut painter = Painter {
                target: PaintTarget::Frame(frame),
                theme,
            };
            for (_, thunk) in pass.deferred.drain(..) {
                thunk(&mut painter, state);
            }
        });
    }

    fn assert_valid(&self) {
        assert!(
            !self.failed.get(),
            "cannot commit a failed declaration pass"
        );
        assert!(
            self.parent_stack.is_empty() && self.layer_stack.is_empty(),
            "cannot commit a declaration pass with unclosed components or layers"
        );
        assert!(
            self.path_cursor.is_empty(),
            "cannot commit a declaration pass with an unclosed path cursor"
        );
        assert!(
            self.surface
                .nodes
                .iter()
                .all(|node| node.is_scope || node.component.is_some()),
            "cannot commit a declaration pass with incomplete components"
        );
    }

    fn finish(self) -> Surface<State, Msg> {
        self.assert_valid();
        self.surface
    }
}

/// The interaction runtime: keep one of these next to your app state and call
/// it from two places in your loop.
///
/// It exists because ratatui widgets only draw. Something has to remember which
/// component is focused, what the pointer is over, and where an event should be
/// delivered. `Ratcn` does that and nothing else — it does not own the loop,
/// the app state, or the update function.
///
/// # Using it
///
/// Build one with [`new`](Ratcn::new) and the builder methods, which wire it to
/// the parts of app state it needs to read (focus, modals) and set root
/// traversal policy. Then, per frame:
///
/// - [`render`](Ratcn::render) declares and paints the components that exist
///   right now.
/// - [`handle_event`](Ratcn::handle_event) routes one backend event and returns
///   an [`EventResult`], which the app matches on to apply messages.
///
/// # Why the retained surface sits between them
///
/// A successful `render` retains that pass's surface — component instances,
/// identity paths, painted geometry, declared props, focus scopes, modal
/// layers. `handle_event` routes against that retained surface instead of re-declaring,
/// which is what lets event handling be a cheap, ordinary function call.
///
/// Two consequences follow, and both are deliberate:
///
/// - **Nothing routes before the first successful render**, because there is no
///   retained surface yet. Such events are ignored.
/// - **The retained surface can be one frame behind app state.** After a message is
///   applied and before the redraw, routing and declared props still come from
///   the previous pass, while components read the current state passed to
///   `handle_event`. Binding [`modals`](Ratcn::modals) closes this gap for the
///   one case where a stale layer would be wrong: events are consumed rather
///   than routed while the app's modal stack and the surface's disagree.
///
/// Replacement is atomic, and so is the frame. A pass that panics or fails
/// validation leaves the previous surface in charge *and* the previous frame
/// on screen: declaring does not draw, and every reason to reject a pass is
/// known before the first cell is written. The one thing that cannot be taken
/// back is a panic thrown by painting itself, after the pass had already been
/// accepted.
pub struct Ratcn<State, Msg> {
    surface: Surface<State, Msg>,
    has_rendered: bool,
    focus_binding: Option<FocusBinding<State, Msg>>,
    modal_binding: Option<ModalBinding<State>>,
    root_options: ScopeOptions,
    /// Raw events in, normalized ones out: which buttons are physically held,
    /// and whether each has moved since its press.
    mouse_tracker: MouseTracker,
    /// What is being tracked for each button whose gesture is under way. One
    /// entry per button, added by its press and removed by its release.
    gestures: HashMap<MouseButton, ButtonGesture>,
    transients: TransientMap,
    /// Where the pointer physically is, from the last mouse event. `None`
    /// until the first one, and again once the pointer leaves the terminal.
    pointer: Option<Position>,
    /// The identity path of whatever the pointer rests on, empty over empty
    /// space. Derived from `pointer` and the retained surface, and rewritten
    /// wherever either changes — pointer motion, and every commit.
    hover: Vec<ChildId>,
}

/// Everything the runtime tracks for one mouse button between its press and
/// its release, as one state rather than as facts scattered across parallel
/// maps.
///
/// A button with no entry has no gesture: nothing to route to, nothing to
/// judge a release against, nothing to swallow. The two states an entry can
/// be in are exclusive by construction — a gesture that has been called off
/// keeps no claim and no target, because nothing may act on either again.
#[derive(Debug)]
enum ButtonGesture {
    /// A live gesture.
    Active {
        /// The path that claimed it with
        /// [`EventCtx::capture_pointer`](super::EventCtx::capture_pointer).
        /// It outranks geometry for every event until the release.
        capture: Option<Vec<ChildId>>,
        /// What the press landed on, recorded once the press had been
        /// delivered and `None` until then. A release is a click when it lands
        /// on this again.
        press_target: Option<PressTarget>,
    },
    /// The gesture has been called off — its target stopped being declared, or
    /// a modal transition moved the ground under it — but the button is still
    /// held. Every event for it is swallowed until the release that ends it.
    Suppressed,
}

impl Default for ButtonGesture {
    /// A gesture that has just begun: nothing claimed yet, no press recorded
    /// yet.
    fn default() -> Self {
        Self::Active {
            capture: None,
            press_target: None,
        }
    }
}

impl ButtonGesture {
    /// The path that claimed this gesture, if one did.
    fn capture(&self) -> Option<&[ChildId]> {
        match self {
            Self::Active { capture, .. } => capture.as_deref(),
            Self::Suppressed => None,
        }
    }

    /// Whether a release with `current` under it lands where the press landed
    /// — the click test, with "empty space" comparing equal to itself.
    fn releases_on_target(&self, current: Option<&[ChildId]>) -> bool {
        match self {
            Self::Active { press_target, .. } => press_target
                .as_ref()
                .is_some_and(|pressed| pressed.holds(current)),
            Self::Suppressed => false,
        }
    }
}

/// Where a press landed, as the thing its release is judged against.
///
/// Empty space is a target like any other: a press and a release that both hit
/// nothing are still the same place, and so still a click — on nothing.
#[derive(Debug)]
enum PressTarget {
    /// The press landed on this identity path.
    Path(Vec<ChildId>),
    /// The press landed where nothing is declared.
    Nothing,
}

impl PressTarget {
    /// What a press that hit `path`, or nothing, landed on.
    fn at(path: Option<Vec<ChildId>>) -> Self {
        path.map_or(Self::Nothing, Self::Path)
    }

    /// Whether a release with `current` under it lands here.
    fn holds(&self, current: Option<&[ChildId]>) -> bool {
        match self {
            Self::Path(path) => current == Some(path.as_slice()),
            Self::Nothing => current.is_none(),
        }
    }
}

impl<State, Msg> fmt::Debug for Ratcn<State, Msg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ratcn")
            .field("surface", &self.surface)
            .field("has_rendered", &self.has_rendered)
            .field("focus_binding", &self.focus_binding.is_some())
            .field("modal_binding", &self.modal_binding.is_some())
            .field("root_options", &self.root_options)
            .field("mouse_tracker", &self.mouse_tracker)
            .field("gestures", &self.gestures)
            .field("transients", &self.transients.len())
            .field("pointer", &self.pointer)
            .field("hover", &self.hover)
            .finish()
    }
}

impl<State, Msg> Default for Ratcn<State, Msg> {
    fn default() -> Self {
        Self {
            surface: Surface::default(),
            has_rendered: false,
            focus_binding: None,
            modal_binding: None,
            root_options: ScopeOptions::default(),
            mouse_tracker: MouseTracker::new(),
            gestures: HashMap::new(),
            transients: HashMap::new(),
            pointer: None,
            hover: Vec::new(),
        }
    }
}

impl<State, Msg> Ratcn<State, Msg> {
    /// Create the runtime, ready to be wired to your app's state.
    ///
    /// Build one and keep it for the life of the app, next to your state —
    /// never inside the draw loop. Between frames it holds the retained surface
    /// [`handle_event`](Ratcn::handle_event) routes against, along with the
    /// bookkeeping that spans several events: which button is mid-drag, which
    /// component captured the pointer, and each component's
    /// [`transient`](super::EventCtx::transient) values. Rebuild it every frame
    /// and all of that is thrown away, so drags fall apart and any event
    /// arriving before that frame's render is ignored.
    ///
    /// A fresh runtime is unwired. The builder methods connect it to your
    /// state — [`focus`](Ratcn::focus), [`modals`](Ratcn::modals) — and set
    /// root traversal policy through
    /// [`tab_wrap`](Ratcn::tab_wrap), [`hover_focus`](Ratcn::hover_focus), and
    /// [`focus_key`](Ratcn::focus_key). Skip [`focus`](Ratcn::focus) and there
    /// is nowhere to store a focus change, so focus resolves to the first
    /// focusable leaf and stays there.
    ///
    /// [`handle_event`](Ratcn::handle_event) returns
    /// [`EventResult::Ignored`] until the first successful
    /// [`render`](Ratcn::render), since there is no retained surface to route through
    /// yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether at least one declaration pass has completed successfully.
    ///
    /// Ask this before handing the runtime an event you would otherwise have to
    /// swallow: a browser paste listener, say, can decline the paste and let the
    /// page keep it rather than dropping it on a runtime with nothing to route
    /// through. Events sent before this is `true` are ignored.
    ///
    /// A failed first render leaves it `false`. Once it is `true` it stays true —
    /// a later failed render keeps the previous surface rather than clearing it.
    #[must_use]
    pub const fn has_rendered(&self) -> bool {
        self.has_rendered
    }

    /// Tell the runtime where focus lives in app state, and how to ask for a
    /// change to it.
    ///
    /// `read` returns the current [`FocusState`] and is called during render and
    /// event routing. `on_change` wraps a new focus path into one of your messages,
    /// which the runtime returns as [`EventResult::Emit`] whenever focus should
    /// move — Tab, a focus hotkey, a click on a control. Applying it is your
    /// update function's job; the runtime never writes app state itself.
    ///
    /// The two halves are one call because they must agree. A reader and a
    /// writer pointing at different fields would leave focus permanently stuck.
    ///
    /// Without this binding focus still resolves to the first focusable leaf,
    /// but it can never move: there is nowhere to store a change, so Tab and
    /// friends come back as [`EventResult::Consumed`] and nothing happens.
    #[must_use]
    pub fn focus(
        mut self,
        read: impl Fn(&State) -> &FocusState + 'static,
        on_change: impl Fn(FocusState) -> Msg + 'static,
    ) -> Self {
        self.focus_binding = Some(FocusBinding {
            read: Box::new(read),
            on_change: Box::new(on_change),
        });
        self
    }

    /// Tell the runtime which modals the app considers open.
    ///
    /// Read-only, unlike [`focus`](Ratcn::focus):
    /// opening and closing modals is entirely the app's decision, made through
    /// [`ModalState::open`] and [`ModalState::close`]. The runtime only needs to
    /// know the answer.
    ///
    /// Binding it buys two things:
    ///
    /// - **No events land on the wrong layer.** Between the message that opens
    ///   or closes a modal and the redraw that declares it, the retained
    ///   retained surface still describes the old layer. While the two disagree, events
    ///   are consumed instead of routed.
    /// - **Focus is correct on the modal's first frame.** Knowing the top modal
    ///   before declaration starts lets focus paint and event routing agree from
    ///   the start of the frame, rather than a lower layer painting focus and
    ///   the modal claiming it a frame later. A focus path outside the top modal
    ///   is pulled to that modal's root; a path already inside it is left
    ///   exactly as it is, parked or not.
    ///
    /// In exchange, every successful render must declare exactly these ids, in
    /// stack order, with [`RenderCtx::modal`] — a mismatch panics rather than
    /// silently diverging.
    ///
    /// Apps using modals should bind this. Without it, [`RenderCtx::modal`]
    /// still layers and routes correctly, but neither guarantee above applies.
    #[must_use]
    pub fn modals(mut self, read: impl Fn(&State) -> &ModalState + 'static) -> Self {
        self.modal_binding = Some(ModalBinding {
            read: Box::new(read),
        });
        self
    }

    /// Set what Tab does at the end of the outermost scope.
    ///
    /// The root closure declares into an implicit scope that has no component of
    /// its own; this configures that scope. [`TabWrap::Wrap`] is the usual
    /// choice for an app, so Tab cycles through the whole UI instead of falling
    /// off the end. See [`ScopeOptions::tab_wrap`] for the same setting on a
    /// nested scope.
    #[must_use]
    pub fn tab_wrap(mut self, tab_wrap: TabWrap) -> Self {
        self.root_options.tab_wrap = tab_wrap;
        self
    }

    /// Make pointer motion move focus between the root's direct children.
    ///
    /// The root-scope version of [`ScopeOptions::hover_focus`], for layouts
    /// where moving the mouse onto a top-level pane should focus it. Off by
    /// default, so hover normally leaves focus alone.
    ///
    /// This is nearly always the right place for the setting: the mouse picks
    /// the pane, and the keyboard then works inside it. Motion *within* a pane
    /// leaves focus alone, because only the root's direct children are
    /// boundaries. Putting it on the pane instead makes every drift between
    /// two controls inside that pane move focus.
    ///
    /// The motion that enters a pane does both things at once: hover is the
    /// runtime's own, written before the focus change is emitted, so the frame
    /// that first paints the new pane focused already paints the new target
    /// hovered. Only the focus half needs a message.
    #[must_use]
    pub fn hover_focus(mut self) -> Self {
        self.root_options.hover_focus = true;
        self
    }

    /// Bind an app-wide key chord that jumps focus to `path`.
    ///
    /// The root-scope version of [`ScopeOptions::focus_key`], and the usual home
    /// for pane hotkeys like `Alt+1`. Because the root scope is outermost, these
    /// are checked last: a binding on an inner scope wins for the same chord.
    #[must_use]
    pub fn focus_key(
        mut self,
        chord: impl Into<super::KeyChord>,
        path: impl IntoIterator<Item = impl Into<ChildId>>,
    ) -> Self {
        self.root_options = self.root_options.focus_key(chord, path);
        self
    }

    /// The app-held focus, as stored. Modal alignment is not applied here —
    /// [`Surface::resolve_focus`] aligns against the *declared* modal roots,
    /// which is nesting-proof, and the semantic/declared distinction is
    /// already covered: at render the just-declared roots are validated
    /// against the bound [`ModalState`], and during the one-frame mismatch
    /// gap events are consumed before routing.
    fn stored_focus(&self, state: &State) -> FocusState {
        self.focus_binding
            .as_ref()
            .map_or_else(FocusState::default, |binding| (binding.read)(state).clone())
    }

    /// Declare and paint one frame, then keep it as the surface events route
    /// against.
    ///
    /// Call this once per frame, from inside ratatui's `Terminal::draw`. The
    /// `declare` closure is the whole UI for this frame: build components from
    /// `state`, place them with
    /// [`render_component`](RenderCtx::render_component), group them with
    /// [`scope`](RenderCtx::scope), queue whatever else you want drawn with
    /// [`paint`](RenderCtx::paint). Nothing is retained between frames, so
    /// there is no widget tree to keep in sync — what you declare is what
    /// exists.
    ///
    /// The pass has to finish completely before it counts: declaration,
    /// component paint, runtime validation, and deferred paint all have to
    /// succeed. Only then does the new surface replace the old one, and it
    /// happens in one step. A pass that panics or fails validation leaves the
    /// previous surface handling events, so a bad frame degrades interaction to
    /// "one frame stale" rather than breaking it — and it leaves the previous
    /// *frame* too. Declaration, validation, and the modal-stack check all
    /// finish before the first cell is written, so a rejected pass never
    /// reaches the screen. Only a panic thrown by painting itself, once the
    /// pass has been accepted, can leave cells behind.
    ///
    /// # Declaring, then drawing
    ///
    /// The closure runs once, and nothing draws while it does. Declaration
    /// records what exists and where; [`Component::paint`] and the closures
    /// [`RenderCtx::paint`] queues are replayed afterwards, in the order the
    /// declaration reached them. Focus and hover resolve in between, against
    /// the finished tree, so every interaction flag a paint reads is derived
    /// from a tree that is already complete — which is the whole reason the
    /// two walks are separate, and why [`RenderCtx`] has no flags to offer.
    ///
    /// One consequence is worth stating plainly: **structure may not depend
    /// on the interaction flags**, because there are none to depend on while
    /// declaring. Which components exist, their ids, and their areas may
    /// depend on anything in `state`, app-held focus included — and on
    /// [`RenderCtx::pointer_within`], which reports hover as it stood when
    /// the pass began rather than as this frame will resolve it.
    ///
    /// # Ordering within the pass
    ///
    /// Declaration order is meaningful. It sets Tab order, it sets paint
    /// order — a component draws before its own descendants — and it sets
    /// hit-testing order, with later declarations on top — within one layer.
    /// [`modal`](RenderCtx::modal), [`popup`](RenderCtx::popup), and
    /// [`hint`](RenderCtx::hint) layers are exempt from paint order: each
    /// paints into its own canvas, and canvases composite over the frame in
    /// the order the layers were declared, so base content declared *after* a
    /// layer still paints beneath it. Layers may therefore be declared from
    /// anywhere in the tree, whenever their owner declares.
    ///
    /// # Panics
    ///
    /// Panics if the closure or a component panics, if runtime validation of
    /// the declaration fails (duplicate sibling ids, an interaction area
    /// outside its component's paint area, a duplicate modal root id), or if
    /// [`modals`](Ratcn::modals) is bound and the declared modal ids do not
    /// exactly match the app's stack. All of these fire before the retained
    /// surface is replaced.
    pub fn render(
        &mut self,
        frame: &mut Frame,
        state: &State,
        theme: &Theme,
        declare: impl FnOnce(&mut RenderCtx<'_, State, Msg>),
    ) {
        let focus_snapshot = self.stored_focus(state);

        // Declare. Nothing is drawn and no *focus* flag is read: the walk
        // builds the tree and queues the paint it owes. Hover is the one
        // interaction fact that predates the pass, so the declaration may ask
        // for it — see [`RenderCtx::pointer_within`].
        let mut pass = RenderPass::new();
        pass.hover_position = self.pointer;
        pass.hover_path.clone_from(&self.hover);
        pass.declare(
            DeclarationEnv::root(frame.area(), state, theme, &mut self.transients),
            declare,
        );
        // Every reason to reject a pass is known once declaration ends, and
        // nothing has painted yet — so the checks run first and a rejected
        // pass never reaches the screen at all. They also establish what
        // `resolve_focus` needs: a complete tree.
        pass.assert_valid();
        self.assert_modal_stack(&pass.surface, state);
        // Focus resolves once, over that tree, and only then does anything
        // learn where it landed. Hover re-answers its own question against the
        // same tree — the pointer has not moved, but what is under it may
        // have — so paint reports this frame's hover rather than the one the
        // declaration was built from.
        let resolved_focus = pass.surface.resolve_focus(&focus_snapshot);
        let resolved_hover = self.resolve_hover(&pass.surface);
        pass.hover_path.clone_from(&resolved_hover);
        pass.replay_paint(frame, state, theme, &resolved_focus);
        pass.finish_frame(frame, state, theme);
        let next = pass.finish();
        self.commit_surface(next, resolved_hover);
    }

    /// What the pointer is on, answered against `surface`.
    ///
    /// Every way a redraw can strand hover is this one question: a modal that
    /// now covers the old target, geometry that moved out from under the
    /// pointer, a node that stopped being declared — each changes the answer,
    /// and nothing else needs saying. The pointer has not moved, so no event
    /// is involved and nothing is emitted.
    ///
    /// The exception is a gesture in flight, which owns the pointer: hover
    /// stays on whatever the gesture started on, so the geometry a drag moves
    /// does not chase the pointer that is dragging it. The freeze holds the
    /// *path*, not the geometry — a frozen target that is redeclared
    /// elsewhere paints hovered wherever it now is — and it lasts only while
    /// that path is still something the pointer could be on at all. A modal
    /// that covers it or a redraw that drops it ends the freeze on the frame
    /// that does it, even though the gesture itself may run on.
    fn resolve_hover(&self, surface: &Surface<State, Msg>) -> Vec<ChildId> {
        if self.gesture_in_flight() && surface.contains_hit_path(&self.hover) {
            return self.hover.clone();
        }
        self.pointer
            .and_then(|position| surface.hit_path(position))
            .unwrap_or_default()
    }

    /// Whether a pointer gesture is under way: a claimed capture, or any
    /// button still held. Both freeze hover, and they have to agree — the
    /// event path stops writing hover the moment a button goes down, because
    /// motion under a held button normalizes to `Drag` rather than `Moved`,
    /// so a redraw path that kept following the pointer would be the only
    /// thing moving hover mid-gesture.
    fn gesture_in_flight(&self) -> bool {
        self.any_capture() || self.mouse_tracker.has_pressed_button()
    }

    /// Whether any button's gesture has been claimed.
    fn any_capture(&self) -> bool {
        self.gestures
            .values()
            .any(|gesture| gesture.capture().is_some())
    }

    /// The path that claimed `button`'s gesture, if one did.
    fn capture_path(&self, button: MouseButton) -> Option<&[ChildId]> {
        self.gestures.get(&button)?.capture()
    }

    /// Whether `button`'s events are being swallowed.
    fn is_suppressed(&self, button: MouseButton) -> bool {
        matches!(self.gestures.get(&button), Some(ButtonGesture::Suppressed))
    }

    /// Call off `button`'s gesture, keeping it suppressed until its release.
    fn suppress(&mut self, button: MouseButton) {
        self.gestures.insert(button, ButtonGesture::Suppressed);
    }

    /// The claim and the press target of `button`'s live gesture, to write
    /// into — starting a gesture if the button has none yet.
    ///
    /// `None` means the gesture has been called off, and a caller that finds
    /// it so writes nothing: a suppressed gesture takes no new claim and no
    /// new press target, because nothing will ever act on either again. Both
    /// writers may pass over that quietly rather than guard against it,
    /// because the suppression stopped the event upstream — every event for a
    /// suppressed button is swallowed by
    /// [`deliver_mouse`](Self::deliver_mouse) before a component can claim it
    /// or a press can land, so there is no fact to record in the first place.
    fn active_gesture(
        &mut self,
        button: MouseButton,
    ) -> Option<(&mut Option<Vec<ChildId>>, &mut Option<PressTarget>)> {
        match self.gestures.entry(button).or_default() {
            ButtonGesture::Active {
                capture,
                press_target,
            } => Some((capture, press_target)),
            ButtonGesture::Suppressed => None,
        }
    }

    /// Every declared modal root must match the app-owned [`ModalState`], in
    /// order, before a surface paints or is committed. The stack is what event
    /// routing compares against to detect the stale window between an app
    /// opening or closing a modal and the redraw that shows it, so a surface
    /// that disagrees with it would make that check meaningless.
    ///
    /// It reads declaration facts alone — which nodes root a modal layer, and
    /// their ids — so it can be answered the moment declaration ends, before
    /// anything is drawn.
    fn assert_modal_stack(&self, surface: &Surface<State, Msg>, state: &State) {
        let Some(binding) = &self.modal_binding else {
            return;
        };
        let semantic = (binding.read)(state).ids();
        let declared = surface.modal_roots().map(|index| &surface.nodes[index].id);
        assert!(
            semantic.iter().eq(declared),
            "declared modal roots do not match app-owned modal ids: expected {semantic:?}"
        );
    }

    /// Publish `next` as the retained interaction surface, and carry the
    /// cross-frame pointer and transient bookkeeping onto it.
    ///
    /// Everything here outlives a single frame and so has to be reconciled
    /// when the ground moves: a captured gesture's component may no longer be
    /// declared, transients are keyed by paths this surface may not contain,
    /// and the pointer may now rest on something else entirely — see
    /// [`resolve_hover`](Self::resolve_hover). A modal opening or
    /// closing is the disruptive case and gets its own treatment — every
    /// tracked gesture is abandoned rather than re-checked, because the layer
    /// that appeared or vanished changes what the pointer was ever over.
    fn commit_surface(&mut self, next: Surface<State, Msg>, hover: Vec<ChildId>) {
        let active_modal_changed = self
            .surface
            .modal_roots()
            .last()
            .map(|index| &self.surface.nodes[index].id)
            != next.modal_roots().last().map(|index| &next.nodes[index].id);

        let previous = std::mem::replace(&mut self.surface, next);
        self.has_rendered = true;

        if active_modal_changed {
            self.cancel_pointer_gestures();
        } else {
            // A capture whose component this surface no longer declares cannot
            // receive the rest of its gesture, so the gesture is suppressed
            // instead of silently retargeting to whatever now sits there.
            let Self {
                gestures, surface, ..
            } = self;
            for gesture in gestures.values_mut() {
                if let Some(path) = gesture.capture()
                    && !surface.contains_participating_path(path)
                {
                    *gesture = ButtonGesture::Suppressed;
                }
            }
        }
        self.transients
            .retain(|path, _| self.surface.contains_declared_path(path));
        // The hover this frame painted, published with the surface it was
        // resolved against — a pass that never got here leaves the previous
        // one in charge, exactly as it leaves the previous surface.
        self.hover = hover;
        // Explicit: dropping the previous surface drops the component
        // instances it retained, and that must happen after everything above
        // has finished reading the new one.
        drop(previous);
    }

    /// Route one event through the last successful retained surface.
    ///
    /// The retained surface is the component tree captured by the most recent
    /// successful [`render`](Ratcn::render): its identity paths, resolved props,
    /// hit geometry, and component instances. Events are matched against that
    /// retained surface; this method never re-runs the declaration closure.
    ///
    /// Routing depends on the kind of event:
    ///
    /// - Keyboard events go to the focused component. The focus path is read
    ///   from the app-owned [`FocusState`] and then resolved against the
    ///   retained surface, so a path that surface never painted resolves to the
    ///   path that was actually painted.
    /// - Mouse events go to whatever the retained hit geometry places under the
    ///   pointer.
    /// - In both cases an event the target ignores bubbles toward the root, so
    ///   an ancestor scope can handle what a leaf did not.
    ///
    /// One raw mouse event can normalize into several — a button release
    /// becomes `Up` and then `Click`. Those route in order until one emits a
    /// message, because this method returns at most one
    /// [`EventResult::Emit`]. Returning `Consumed` for the first does not
    /// suppress the follow-up.
    ///
    /// # Current state, possibly stale surface
    ///
    /// `state` is the app's current semantic state, but the surface can be one
    /// frame behind it: after an emitted message is applied and before the next
    /// redraw, this call still uses the previous declaration's props, geometry,
    /// and component instances, while focus and other semantic reads see the
    /// new `state`. The next successful render publishes the updated
    /// declaration.
    ///
    /// Modals are where that one-frame lag would matter, since an event could
    /// otherwise land on a layer the app considers closed. So with
    /// [`modals`](Ratcn::modals) bound, an event is consumed without routing
    /// whenever the semantic modal stack disagrees with the retained modal
    /// roots.
    ///
    /// Events are ignored entirely before the first successful render, and when
    /// the backend event does not convert into an [`Event`].
    pub fn handle_event(&mut self, event: impl TryInto<Event>, state: &State) -> EventResult<Msg> {
        let Ok(event) = event.try_into() else {
            return EventResult::Ignored;
        };
        if !self.has_rendered {
            return EventResult::Ignored;
        }
        if !self.modal_stack_matches(state) {
            if let Event::Mouse(raw) = event {
                self.consume_mouse_without_routing(raw);
            }
            return EventResult::Consumed;
        }

        let result = match event {
            Event::Mouse(raw) => self.handle_mouse(raw, state),
            ref event => self.route_key(event, state),
        };
        // An open modal is a floor under the whole surface: nothing it covers
        // may report an event as unhandled, or the app would act on input the
        // modal was meant to block.
        if matches!(result, EventResult::Ignored) && self.modal_is_open() {
            EventResult::Consumed
        } else {
            result
        }
    }

    /// Route one non-pointer event, and answer for it when nothing in the
    /// surface does.
    ///
    /// The keyboard twin of [`route_mouse`](Self::route_mouse), and the same
    /// cascade: build a chain, dispatch through it, then fall back. Only the
    /// chain differs — keys descend the focus path rather than a hit test.
    ///
    /// The fallbacks are traversal and jumps, and they are exclusive. Tab and
    /// `BackTab` move focus one step; any other key may match a
    /// [`focus_key`](Ratcn::focus_key) binding and jump to a named path. A key
    /// that is traversal never consults the bindings, so a binding cannot
    /// shadow Tab.
    fn route_key(&mut self, event: &Event, state: &State) -> EventResult<Msg> {
        if self.surface.nodes.is_empty() {
            return EventResult::Ignored;
        }
        let stored_focus = self.stored_focus(state);
        let focus = self.surface.resolve_focus(&stored_focus);
        let chain = self.key_bubble_chain(&focus);

        let routed = self.dispatch_chain(&chain, event, state, &mut None, None);
        if !matches!(routed, EventResult::Ignored) {
            return routed;
        }

        let Event::Key(key) = event else {
            return EventResult::Ignored;
        };
        match traversal_direction(key) {
            Some(direction) => {
                match self
                    .surface
                    .next_focus(&focus, direction, &self.root_options)
                {
                    FocusAdvance::Move(next) => self.focus_result(next),
                    FocusAdvance::Consumed => EventResult::Consumed,
                    FocusAdvance::Ignored => EventResult::Ignored,
                }
            }
            None => self
                .focus_key_jump(key, &chain, &focus)
                .unwrap_or(EventResult::Ignored),
        }
    }

    /// The chain a key bubbles through: the focused leaf up to the root, cut
    /// at the topmost key-trapping layer.
    ///
    /// Keys never cross such a layer outward. Bubbling stops at its root,
    /// which doubles as the layer-wide fallback for keys nothing inside
    /// handled. Layers that do not trap keys — popups, hints — are absent
    /// here, so an unhandled Esc still reaches whatever declared them.
    fn key_bubble_chain(&self, focus: &FocusState) -> Vec<usize> {
        let mut matched = self.surface.nodes_along_path(focus.path());
        let Some(trapping_root) = self.surface.top_layer_root(|policy| policy.traps_keys) else {
            return matched;
        };
        match matched.iter().position(|&index| index == trapping_root) {
            Some(position) => {
                matched.drain(..position);
                matched
            }
            // Focus is parked outside the layer: either it has nothing
            // focusable to take the path over, or the stored path is absent
            // from this surface. The chain becomes that root alone — keeping
            // the outside chain would offer the key to the covered component
            // first, since the chain is walked deepest-first.
            None => vec![trapping_root],
        }
    }

    /// The focus jump a [`focus_key`](Ratcn::focus_key) binding asks for, or
    /// `None` when no binding in scope matches this key.
    ///
    /// Bindings are searched innermost scope first and the root's own last, so
    /// a scope can rebind a chord its ancestor also uses. A binding whose path
    /// no longer resolves is skipped rather than swallowing the key, which
    /// keeps a hotkey for a pane that is not currently declared inert instead
    /// of dead.
    fn focus_key_jump(
        &self,
        key: &KeyEvent,
        chain: &[usize],
        focus: &FocusState,
    ) -> Option<EventResult<Msg>> {
        for scope in chain.iter().rev().copied().map(Some).chain([None]) {
            let options = scope.map_or(&self.root_options, |index| {
                &self.surface.nodes[index].options
            });
            for binding in &options.focus_keys {
                if !binding.chord.matches(key) {
                    continue;
                }
                let mut path = scope.map_or_else(Vec::new, |index| self.surface.path_of(index));
                path.extend(binding.path.iter().cloned());
                let Some(next) = self.surface.explicit_focus(&path) else {
                    continue;
                };
                return Some(if next == *focus {
                    EventResult::Consumed
                } else {
                    self.focus_result(next)
                });
            }
        }
        None
    }

    /// Resolve a focus path against the retained surface, or `None` if it is not
    /// focusable right now.
    ///
    /// Use this when the app wants to move focus somewhere and needs to know
    /// whether that is actually possible — say, focusing a field only if it is
    /// currently on screen and enabled. A path ending at a container resolves to
    /// its first focusable leaf, so the returned [`FocusState`] is always a real
    /// target.
    ///
    /// This is the counterpart to [`FocusState::intent`], which never validates
    /// anything and is what you want for a path that should park until the
    /// component appears. Prefer `intent` for "focus this when it exists" and
    /// this for "focus this only if it exists".
    ///
    /// `None` means: nothing has rendered yet, some id in the path is missing or
    /// not focusable, or the path is on a layer below the open modal.
    #[must_use]
    pub fn focus_path(&self, path: &[ChildId]) -> Option<FocusState> {
        self.has_rendered
            .then(|| self.surface.explicit_focus(path))
            .flatten()
    }

    /// Whether the last successful render declared a modal.
    ///
    /// This reflects what was *painted*, which is not always what the app
    /// believes: right after a message opens a modal, this is still false until
    /// the redraw. For app logic, read your own [`ModalState`] instead; this is
    /// for asking what the retained surface looks like.
    #[must_use]
    pub fn modal_is_open(&self) -> bool {
        self.surface.modal_roots().next().is_some()
    }

    fn modal_stack_matches(&self, state: &State) -> bool {
        self.modal_binding.as_ref().is_none_or(|binding| {
            let semantic = (binding.read)(state).ids();
            let retained = self
                .surface
                .modal_roots()
                .map(|index| &self.surface.nodes[index].id);
            semantic.iter().eq(retained)
        })
    }

    /// Handle one *raw* mouse event: pointer bookkeeping, gesture synthesis,
    /// then delivery of everything that synthesis produced.
    ///
    /// Backends report `Down`, `Up`, and `Moved`; components consume `Click`,
    /// `Drag`, and `DragEnd`. [`MouseTracker`] bridges the two, so one raw
    /// event can expand into several normalized ones — a release becomes `Up`
    /// and then `Click` or `DragEnd`. Each is delivered in turn, but only until
    /// one emits: the app sees at most one message per raw event, the same
    /// contract the keyboard path keeps.
    ///
    /// A pointer gesture spans several raw events, and its lifetime is
    /// bracketed here and nowhere else:
    /// [`observe_pointer`](Self::observe_pointer) opens it,
    /// [`record_press_target`](Self::record_press_target) stores what the press
    /// landed on for its release to be judged against, and
    /// [`end_gesture`](Self::end_gesture) closes it. What they maintain is one
    /// [`ButtonGesture`] per button, and it exists only between a press and its
    /// release.
    fn handle_mouse(&mut self, raw: MouseEvent, state: &State) -> EventResult<Msg> {
        if raw.kind == MouseKind::Exited {
            return self.handle_pointer_exit();
        }
        let (pressed, released) = self.observe_pointer(raw);
        // Motion the tracker swallowed because the press has not left its cell
        // yet: nothing to deliver, but the app must not read it as unhandled.
        let held_motion = raw.kind == MouseKind::Moved && self.mouse_tracker.has_pressed_button();
        let events = self
            .mouse_tracker
            .feed(raw, self.releases_on_press_target(raw));
        let mut result = if held_motion && events.is_empty() {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        };
        for mouse in events {
            let next = self.deliver_mouse(mouse, state);
            // After delivery, never before: a component claims the pointer
            // while handling its `Down`, and that claim is what the press
            // target records.
            if let Some(button) = pressed {
                self.record_press_target(button, mouse);
            }
            match next {
                // A message ends the batch: any normalized event synthesized
                // after this one is dropped. The pairs that matter do not
                // collide — a press ignores and its `Click` emits — but a
                // component that emits on `Up` would swallow that `Click`.
                EventResult::Emit(_) => {
                    result = next;
                    break;
                }
                EventResult::Consumed => result = next,
                EventResult::Ignored => {}
            }
        }
        if let Some(button) = released {
            self.end_gesture(button);
        }
        result
    }

    /// The pointer left the backend's grid: abandon every tracked gesture and
    /// clear hover. Nothing routes, so this always counts as handled.
    fn handle_pointer_exit(&mut self) -> EventResult<Msg> {
        self.reset_pointer_gestures();
        self.pointer_gone();
        EventResult::Consumed
    }

    /// The pointer is no longer anywhere on the grid, so it is on nothing.
    fn pointer_gone(&mut self) {
        self.pointer = None;
        self.hover.clear();
    }

    /// Deliver one normalized event, unless its gesture is suppressed, and give
    /// a press that landed outside a popup the chance to dismiss it.
    ///
    /// Dismissal is observed, not consumed: the press already routed to
    /// whatever it hit. The hook fires only when routing produced no message of
    /// its own — a press landing on a focusable control emits a focus change
    /// instead, and the app closes the popup during that update.
    fn deliver_mouse(&mut self, mouse: MouseEvent, state: &State) -> EventResult<Msg> {
        if mouse_button(mouse.kind).is_some_and(|button| self.is_suppressed(button)) {
            return EventResult::Consumed;
        }
        let routed = self.route_mouse(mouse, state);
        match (mouse.kind, &routed) {
            (MouseKind::Down(_), EventResult::Ignored | EventResult::Consumed) => self
                .popup_dismissal(Position::new(mouse.column, mouse.row))
                .map_or(routed, EventResult::Emit),
            _ => routed,
        }
    }

    /// Store what this press landed on, for
    /// [`releases_on_press_target`](Self::releases_on_press_target) to judge
    /// its release against. A capture outranks geometry: a component that
    /// claimed the gesture owns it wherever the pointer then goes.
    fn record_press_target(&mut self, button: MouseButton, mouse: MouseEvent) {
        let target = self
            .capture_path(button)
            .map(<[ChildId]>::to_vec)
            .or_else(|| {
                self.surface
                    .hit_path(Position::new(mouse.column, mouse.row))
            });
        if let Some((_, press_target)) = self.active_gesture(button) {
            *press_target = Some(PressTarget::at(target));
        }
    }

    /// Forget everything tracked for `button`: its gesture ended with this
    /// release.
    fn end_gesture(&mut self, button: MouseButton) {
        self.gestures.remove(&button);
    }

    /// Does this raw release complete a click? The whole definition lives here,
    /// because it is the half [`MouseTracker`] cannot answer: it knows where
    /// the pointer went, not what is under it or who claimed it.
    ///
    /// A click is a release on the component the press hit. The pointer may
    /// have moved in between — drifting a column while pressing a button is
    /// still a click, and it is only the hit path that has to match. That path
    /// comparison also catches the reverse case, where neither the pointer nor
    /// the press moved but a redraw slid a different component under it.
    ///
    /// One thing disqualifies an otherwise matching release: a gesture that a
    /// component claimed with [`EventCtx::capture_pointer`] *and* then dragged.
    /// Claiming it declares the movement meaningful, so it ends as
    /// [`DragEnd`](MouseKind::DragEnd). A claim that never moved is still a
    /// click, which is what lets one component both drag and be clicked.
    fn releases_on_press_target(&self, raw: MouseEvent) -> bool {
        let MouseKind::Up(button) = raw.kind else {
            return false;
        };
        let Some(gesture) = self.gestures.get(&button) else {
            return false;
        };
        if gesture.capture().is_some() && self.mouse_tracker.press_moved(button) {
            return false;
        }
        let current = self.surface.hit_path(Position::new(raw.column, raw.row));
        gesture.releases_on_target(current.as_deref())
    }

    /// Call off every gesture under way: each claimed one, and each button
    /// still physically held. They stay suppressed until their releases.
    fn cancel_pointer_gestures(&mut self) {
        for button in self.mouse_tracker.pressed_buttons() {
            self.suppress(button);
        }
        for gesture in self.gestures.values_mut() {
            if gesture.capture().is_some() {
                *gesture = ButtonGesture::Suppressed;
            }
        }
    }

    fn reset_pointer_gestures(&mut self) {
        self.mouse_tracker.clear();
        self.gestures.clear();
    }

    /// The stale-modal-window half of mouse handling: while the semantic modal
    /// stack disagrees with the retained one, events must not route, but the
    /// cross-event pointer bookkeeping still has to advance exactly as
    /// [`handle_mouse`](Self::handle_mouse) would advance it — the shared
    /// [`observe_pointer`](Self::observe_pointer) step plus a feed of the
    /// tracker — or gestures desynchronize across the gap. Presses that start
    /// inside the gap are suppressed until their release.
    fn consume_mouse_without_routing(&mut self, raw: MouseEvent) {
        if raw.kind == MouseKind::Exited {
            self.pointer_gone();
            self.reset_pointer_gestures();
            return;
        }
        let (pressed, released) = self.observe_pointer(raw);
        self.cancel_pointer_gestures();
        // Nothing routes across this gap, so the synthesized follow-up is
        // discarded either way; `false` keeps the tracker from consulting a
        // surface the app has already moved past.
        let _ = self.mouse_tracker.feed(raw, false);
        if let Some(button) = pressed {
            self.suppress(button);
        }
        if let Some(button) = released {
            self.end_gesture(button);
        }
    }

    /// The pointer bookkeeping every non-exited mouse event performs, shared
    /// by the routing path and the stale-modal-window consume path so the two
    /// cannot drift: where the pointer is, and which button this event
    /// presses or releases.
    fn observe_pointer(&mut self, raw: MouseEvent) -> (Option<MouseButton>, Option<MouseButton>) {
        self.pointer = Some(Position::new(raw.column, raw.row));
        let pressed = match raw.kind {
            MouseKind::Down(button) => Some(button),
            _ => None,
        };
        let released = match raw.kind {
            MouseKind::Up(button) => Some(button),
            _ => None,
        };
        (pressed, released)
    }

    /// Offer `event` to each component in `chain`, deepest first, stopping at
    /// the first that does not ignore it.
    ///
    /// The one dispatch loop. Keys and pointer events build different chains —
    /// keys from the focus path, the pointer from what it hit — but bubble
    /// through them identically, so this is where "unhandled events bubble up"
    /// is actually implemented. `capture` receives a component's
    /// [`EventCtx::capture_pointer`] claim; the key path passes `&mut None`
    /// because there is no gesture to own.
    fn dispatch_chain(
        &mut self,
        chain: &[usize],
        event: &Event,
        state: &State,
        capture: &mut Option<Vec<ChildId>>,
        capture_button: Option<MouseButton>,
    ) -> EventResult<Msg> {
        for &index in chain.iter().rev() {
            if !self.surface.participates(index) {
                continue;
            }
            let path = self.surface.path_of(index);
            let area = self.surface.nodes[index].area;
            let Some(component) = self.surface.nodes[index].component.as_mut() else {
                continue;
            };
            let mut ctx = EventCtx::at(&path, area, &mut self.transients, capture, capture_button);
            let result = component.handle_event(event, state, &mut ctx);
            if !matches!(result, EventResult::Ignored) {
                return result;
            }
        }
        EventResult::Ignored
    }

    /// Route one *normalized* mouse event through the retained surface, and
    /// answer for it when nothing in the surface does.
    ///
    /// This is a cascade, and its order is the policy. The pointer's target
    /// resolves first, and motion writes hover from it before anything else
    /// sees the event: hover is the runtime's own value, so there is nothing
    /// to route and nobody to ask. Hover-focus, which does need a message,
    /// takes the motion next. What is left goes to the component under the
    /// pointer and bubbles to its ancestors.
    ///
    /// Only when none of them handled it do the fallbacks run: a primary press
    /// moves focus, and an event confined to a layer is consumed at that
    /// layer's boundary instead of escaping to what lies beneath. An event that
    /// survives the whole cascade is unhandled — except a motion, which is
    /// always at least [`Consumed`](EventResult::Consumed) once a surface
    /// exists. Motion changes what the next frame should look like whether or
    /// not it changed hover: paint may read the pointer position itself
    /// through [`PaintCtx::hover_position`], and a host that redraws on any
    /// result but [`Ignored`](EventResult::Ignored) needs that signal for
    /// motion *within* one component as much as for crossing between two.
    fn route_mouse(&mut self, mouse: MouseEvent, state: &State) -> EventResult<Msg> {
        let path = self.pointer_target(mouse);
        let moved = mouse.kind == MouseKind::Moved;
        if moved {
            self.set_hover(path.as_deref().unwrap_or_default());
        }

        if moved
            && let Some(path) = path.as_deref()
            && let Some(staged) = self.stage_hover_focus(path, state)
        {
            return staged;
        }

        let Some(path) = path else {
            // Nothing under the pointer. Motion is still this frame's news;
            // anything else lands on nobody.
            return if moved {
                EventResult::Consumed
            } else {
                EventResult::Ignored
            };
        };

        let (chain, layer_confined) = self.surface.mouse_bubble_chain(&path);
        let routed = self.dispatch_pointer(&chain, mouse, state);
        if !matches!(routed, EventResult::Ignored) {
            return routed;
        }
        if moved {
            return EventResult::Consumed;
        }
        if let Some(focused) = self.focus_on_press(&chain, mouse, state) {
            return focused;
        }
        // An event the hit layer's content ignored is consumed at the layer
        // boundary: it must never read as unhandled to lower layers or to
        // whatever declared the layer.
        if layer_confined {
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }

    /// What this event is aimed at: the component that captured the gesture if
    /// one did, otherwise whatever geometry puts under the pointer.
    ///
    /// Only the events that continue a gesture consult the capture map. A
    /// `Down` starts one, so it always hit-tests; a `Moved` belongs to no
    /// gesture at all.
    fn pointer_target(&self, mouse: MouseEvent) -> Option<Vec<ChildId>> {
        let captured = match mouse.kind {
            MouseKind::Drag(button)
            | MouseKind::Up(button)
            | MouseKind::Click(button)
            | MouseKind::DragEnd(button) => self.capture_path(button).map(<[ChildId]>::to_vec),
            _ => None,
        };
        captured.or_else(|| {
            self.surface
                .hit_path(Position::new(mouse.column, mouse.row))
        })
    }

    /// Offer the event to the hit component and its ancestors, and record a
    /// capture if one of them claims the gesture. Only a `Down` may claim.
    fn dispatch_pointer(
        &mut self,
        chain: &[usize],
        mouse: MouseEvent,
        state: &State,
    ) -> EventResult<Msg> {
        let capture_button = match mouse.kind {
            MouseKind::Down(button) => Some(button),
            _ => None,
        };
        let mut capture = None;
        let result = self.dispatch_chain(
            chain,
            &Event::Mouse(mouse),
            state,
            &mut capture,
            capture_button,
        );
        if let (Some(button), Some(path)) = (capture_button, capture)
            && let Some((claim, _)) = self.active_gesture(button)
        {
            *claim = Some(path);
        }
        result
    }

    /// The focus change a primary press produces when no component handled it,
    /// or `None` when this event is not such a press or nothing along the chain
    /// can take focus.
    ///
    /// Whether a component focuses on the `Down` or on the `Click` is its own
    /// choice, so both kinds arrive here and each skips the nodes that wanted
    /// the other. The search runs over the bubble chain rather than the whole
    /// surface, which keeps focus-on-press inside the hit layer: a press in a
    /// popup panel must not focus the component that declared the popup.
    fn focus_on_press(
        &mut self,
        chain: &[usize],
        mouse: MouseEvent,
        state: &State,
    ) -> Option<EventResult<Msg>> {
        if !matches!(
            mouse.kind,
            MouseKind::Down(MouseButton::Left) | MouseKind::Click(MouseButton::Left)
        ) {
            return None;
        }
        let target = chain.iter().rev().copied().find(|&index| {
            let focuses = match mouse.kind {
                MouseKind::Down(_) => !self.surface.nodes[index].focuses_on_click,
                MouseKind::Click(_) => self.surface.nodes[index].focuses_on_click,
                _ => false,
            };
            focuses && self.surface.focusable(index)
        })?;

        let stored = self.stored_focus(state);
        let current = self.surface.resolve_focus(&stored);
        // Focus lands on a leaf, so a focusable container hands off to its
        // first focusable descendant.
        let mut path = self.surface.path_of(target);
        if let Some(child) = self.surface.edge_child(Some(target), Step::Forward) {
            self.surface.extend_to_edge(child, Step::Forward, &mut path);
        }
        let focus = FocusState::intent(path);
        Some(if focus == current {
            EventResult::Consumed
        } else {
            self.focus_result(focus)
        })
    }

    /// The dismiss message of the topmost popup the press at `point` landed
    /// outside of, if any. "Outside" is containment, not depth: the press hit
    /// nothing, or hit something that is not inside the popup's subtree.
    /// Popups an open modal covers are inert and never dismiss.
    fn popup_dismissal(&self, point: Position) -> Option<Msg> {
        let target = self.surface.hit_index(point);
        // Innermost first, and keep looking: the layer the press landed
        // inside is not dismissed, but one it landed outside of still is.
        let top = self
            .surface
            .layer_roots
            .iter()
            .rev()
            .copied()
            .find(|&root| {
                self.surface
                    .layer_kind_policy(root)
                    .dismiss_on_outside_press
                    && self.surface.interactive(root)
                    && self.surface.participates(root)
                    && target.is_none_or(|hit| !self.surface.inside(hit, root))
            })?;
        self.surface.nodes[top].on_dismiss.as_ref().map(|f| f())
    }

    /// The focus change a motion onto `path` produces when it crosses a
    /// [`hover_focus`](ScopeOptions::hover_focus) boundary, and `None`
    /// otherwise. Hover itself is already written by the time this runs, so
    /// the one message a motion can carry is this one.
    fn stage_hover_focus(&mut self, path: &[ChildId], state: &State) -> Option<EventResult<Msg>> {
        let stored = self.stored_focus(state);
        let focus = self.surface.resolve_focus(&stored);
        let next = self.surface.hover_focus(path, &focus, &self.root_options)?;
        Some(self.focus_result(next))
    }

    /// Point hover at `path`. The event-time writer; a commit writes it from
    /// [`resolve_hover`](Self::resolve_hover) instead. Only motion reaches
    /// here — every other pointer event records the position and leaves the
    /// next resolution to answer with it.
    fn set_hover(&mut self, path: &[ChildId]) {
        if self.hover != path {
            self.hover = path.to_vec();
        }
    }

    fn focus_result(&self, focus: FocusState) -> EventResult<Msg> {
        self.focus_binding
            .as_ref()
            .map_or(EventResult::Consumed, |binding| {
                EventResult::Emit((binding.on_change)(focus))
            })
    }

    #[cfg(test)]
    fn hover_path(&self) -> &[ChildId] {
        &self.hover
    }

    #[cfg(test)]
    fn declared_paths(&self) -> Vec<Vec<ChildId>> {
        (0..self.surface.nodes.len())
            .map(|index| self.surface.path_of(index))
            .collect()
    }
}

/// Which way this key moves focus, or `None` if it is not a traversal key.
///
/// Tab must be unmodified: Ctrl+Tab and friends belong to the app or the
/// terminal. `BackTab` already implies Shift, so only Ctrl and Alt disqualify
/// it.
fn traversal_direction(key: &KeyEvent) -> Option<Step> {
    match key.code {
        KeyCode::Tab if !key.modifiers.any() => Some(Step::Forward),
        KeyCode::BackTab if !key.modifiers.ctrl && !key.modifiers.alt => Some(Step::Backward),
        _ => None,
    }
}

fn mouse_button(kind: MouseKind) -> Option<MouseButton> {
    match kind {
        MouseKind::Down(button)
        | MouseKind::Up(button)
        | MouseKind::Click(button)
        | MouseKind::Drag(button)
        | MouseKind::DragEnd(button) => Some(button),
        MouseKind::Moved | MouseKind::Exited | MouseKind::Scroll(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::hash_map::DefaultHasher,
        hash::{Hash, Hasher},
        panic::{AssertUnwindSafe, catch_unwind},
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
    };

    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::runtime::PopupOptions;
    use crate::{
        Button, Dialog,
        runtime::{CellOffset, DragOptions, DragPhase, KeyChord, Modifiers},
    };

    struct Leaf;

    impl Component<(), ()> for Leaf {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, (), ()>) {}

        fn handle_event(
            &mut self,
            _event: &Event,
            _state: &(),
            _ctx: &mut EventCtx<'_>,
        ) -> EventResult<()> {
            EventResult::Ignored
        }
    }

    struct ContextProbe {
        area: Rect,
    }

    impl Component<u8, ()> for ContextProbe {
        fn render(&mut self, ctx: &mut RenderCtx<'_, u8, ()>) {
            assert_eq!(ctx.area(), self.area);
            assert_eq!(*ctx.state(), 7);
        }

        fn is_focusable(&self, _state: &u8) -> bool {
            true
        }
    }

    struct Composite;

    impl Component<(), ()> for Composite {
        fn render(&mut self, ctx: &mut RenderCtx<'_, (), ()>) {
            let area = ctx.area();
            ctx.render_component(ChildId::Static("leaf"), Leaf, area);
        }

        fn scope_options(&self) -> ScopeOptions {
            ScopeOptions::default().tab_wrap(TabWrap::Wrap)
        }
    }

    struct PanickingLeaf;

    impl Component<(), ()> for PanickingLeaf {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, (), ()>) {
            panic!("leaf render failed");
        }
    }

    struct PanickingScopeOptions;

    impl Component<(), ()> for PanickingScopeOptions {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, (), ()>) {}

        fn scope_options(&self) -> ScopeOptions {
            panic!("scope options failed");
        }
    }

    struct PanickingResolve;

    impl Component<(), ()> for PanickingResolve {
        fn prepare(&mut self, _state: &()) {
            panic!("declaration prop resolution failed");
        }

        fn render(&mut self, _ctx: &mut RenderCtx<'_, (), ()>) {}
    }

    struct PanickingFocusable;

    impl Component<(), ()> for PanickingFocusable {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, (), ()>) {}

        fn is_focusable(&self, _state: &()) -> bool {
            panic!("focusability failed");
        }
    }

    struct PanickingInteractionArea;

    impl Component<(), ()> for PanickingInteractionArea {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, (), ()>) {}

        fn interaction_area(&self, _area: Rect) -> Rect {
            panic!("interaction area failed");
        }
    }

    struct EscapingInteractionArea;

    impl Component<(), ()> for EscapingInteractionArea {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, (), ()>) {}

        fn interaction_area(&self, area: Rect) -> Rect {
            Rect::new(area.x, area.y, area.width.saturating_add(1), area.height)
        }
    }

    struct CatchingComposite;

    impl Component<(), ()> for CatchingComposite {
        fn render(&mut self, ctx: &mut RenderCtx<'_, (), ()>) {
            let area = ctx.area();
            let caught = catch_unwind(AssertUnwindSafe(|| {
                ctx.render_component(ChildId::Static("panicking-child"), PanickingLeaf, area);
            }));
            assert!(caught.is_err());
            ctx.render_component(ChildId::Static("later-child"), Leaf, area);
        }
    }

    #[derive(Default)]
    struct FocusTestState {
        focus: FocusState,
    }

    #[derive(Default)]
    struct ModalTestState {
        focus: FocusState,
        modals: ModalState,
    }

    #[derive(Debug, Clone, PartialEq)]
    enum ModalTestMsg {
        Routed(&'static str),
        Focus(FocusState),
    }

    struct ModalRoute(&'static str);

    impl Component<ModalTestState, ModalTestMsg> for ModalRoute {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, ModalTestState, ModalTestMsg>) {}

        fn handle_event(
            &mut self,
            _event: &Event,
            _state: &ModalTestState,
            _ctx: &mut EventCtx<'_>,
        ) -> EventResult<ModalTestMsg> {
            EventResult::Emit(ModalTestMsg::Routed(self.0))
        }

        fn is_focusable(&self, _state: &ModalTestState) -> bool {
            true
        }
    }

    struct ModalFocusRoute {
        rendered: FocusRenderLog,
    }

    impl Component<ModalTestState, ModalTestMsg> for ModalFocusRoute {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, ModalTestState, ModalTestMsg>) {}

        fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, ModalTestState>) {
            self.rendered
                .lock()
                .expect("modal focus render log")
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

        fn is_focusable(&self, _state: &ModalTestState) -> bool {
            true
        }
    }

    #[derive(Default)]
    struct ButtonTimingState {
        focus: FocusState,
        saving: bool,
        accepted_saves: usize,
    }

    #[derive(Debug, Clone, PartialEq)]
    enum ButtonTimingMsg {
        Focus(FocusState),
        Save,
        Replacement,
    }

    fn update_button_timing(state: &mut ButtonTimingState, msg: ButtonTimingMsg) -> bool {
        match msg {
            ButtonTimingMsg::Focus(focus) => {
                state.focus = focus;
                true
            }
            ButtonTimingMsg::Save if !state.saving => {
                state.saving = true;
                state.accepted_saves += 1;
                true
            }
            ButtonTimingMsg::Save | ButtonTimingMsg::Replacement => false,
        }
    }

    #[derive(Debug, PartialEq)]
    enum FocusTestMsg {
        Focus(FocusState),
        Activated(Vec<ChildId>),
        Parent(Vec<ChildId>),
    }

    type FocusRenderLog = Arc<Mutex<Vec<(bool, bool)>>>;

    struct FocusLeaf {
        enabled: bool,
        consume_focus_key: bool,
        rendered: Option<FocusRenderLog>,
    }

    impl FocusLeaf {
        fn enabled() -> Self {
            Self {
                enabled: true,
                consume_focus_key: false,
                rendered: None,
            }
        }

        fn disabled() -> Self {
            Self {
                enabled: false,
                consume_focus_key: false,
                rendered: None,
            }
        }

        fn recording(rendered: FocusRenderLog) -> Self {
            Self {
                enabled: true,
                consume_focus_key: false,
                rendered: Some(rendered),
            }
        }

        fn consuming_focus_key() -> Self {
            Self {
                enabled: true,
                consume_focus_key: true,
                rendered: None,
            }
        }
    }

    impl Component<FocusTestState, FocusTestMsg> for FocusLeaf {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, FocusTestState, FocusTestMsg>) {}

        fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, FocusTestState>) {
            if let Some(rendered) = &self.rendered {
                rendered
                    .lock()
                    .expect("render log")
                    .push((ctx.focused, ctx.contains_focus));
            }
        }

        fn handle_event(
            &mut self,
            event: &Event,
            _state: &FocusTestState,
            ctx: &mut EventCtx<'_>,
        ) -> EventResult<FocusTestMsg> {
            if !self.enabled {
                return EventResult::Ignored;
            }
            match event {
                Event::Key(key) if self.consume_focus_key && key.code == KeyCode::Char('x') => {
                    EventResult::Consumed
                }
                Event::Key(key) if key.code == KeyCode::Enter => {
                    EventResult::Emit(FocusTestMsg::Activated(ctx.path().to_vec()))
                }
                _ => EventResult::Ignored,
            }
        }

        fn is_focusable(&self, _state: &FocusTestState) -> bool {
            self.enabled
        }
    }

    struct ClickFocusLeaf;

    impl Component<FocusTestState, FocusTestMsg> for ClickFocusLeaf {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, FocusTestState, FocusTestMsg>) {}

        fn is_focusable(&self, _state: &FocusTestState) -> bool {
            true
        }

        fn focuses_on_click(&self, _state: &FocusTestState) -> bool {
            true
        }
    }

    type PathLog = Arc<Mutex<Vec<Vec<ChildId>>>>;

    /// Record the identity path the declaration pass currently has open.
    fn record_declared_path(ctx: &mut RenderCtx<'_, FocusTestState, FocusTestMsg>, log: &PathLog) {
        if let Some(path) = ctx.pass.current_path() {
            log.lock().expect("path log").push(path.to_vec());
        }
    }

    /// A focusable leaf that records where it was declared. The counterpart to
    /// [`record_declared_path`] for nodes that are components rather than
    /// scopes.
    struct PathProbe(PathLog);

    impl Component<FocusTestState, FocusTestMsg> for PathProbe {
        fn render(&mut self, ctx: &mut RenderCtx<'_, FocusTestState, FocusTestMsg>) {
            record_declared_path(ctx, &self.0);
        }

        fn is_focusable(&self, _state: &FocusTestState) -> bool {
            true
        }
    }

    /// The two leaves at the bottom of a depth-four branch, plus the record
    /// for the scope they were declared into.
    fn declare_probe_cells(
        ctx: &mut RenderCtx<'_, FocusTestState, FocusTestMsg>,
        top: u16,
        log: &PathLog,
    ) {
        record_declared_path(ctx, log);
        for (row, id) in [(top, "cell-1"), (top + 1, "cell-2")] {
            ctx.render_component(
                ChildId::from(id.to_owned()),
                PathProbe(Arc::clone(log)),
                Rect::new(0, row, 20, 1),
            );
        }
    }

    #[derive(Clone, Copy)]
    enum DownBehavior {
        CaptureAndIgnore,
        Consume,
        Emit,
    }

    struct DownFocusLeaf(DownBehavior);

    impl Component<FocusTestState, FocusTestMsg> for DownFocusLeaf {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, FocusTestState, FocusTestMsg>) {}

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
                DownBehavior::Emit => {
                    EventResult::Emit(FocusTestMsg::Activated(ctx.path().to_vec()))
                }
            }
        }

        fn is_focusable(&self, _state: &FocusTestState) -> bool {
            true
        }
    }

    struct AreaAwareComposite {
        expected_area: Rect,
        minimum_width: u16,
        rendered: Arc<AtomicBool>,
    }

    impl Component<FocusTestState, FocusTestMsg> for AreaAwareComposite {
        fn render(&mut self, ctx: &mut RenderCtx<'_, FocusTestState, FocusTestMsg>) {
            assert_eq!(ctx.area(), self.expected_area);
            self.rendered.store(true, Ordering::SeqCst);
            ctx.render_component(ChildId::Static("child"), FocusLeaf::enabled(), ctx.area());
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

    struct FocusComposite {
        parent_rendered: FocusRenderLog,
        child_rendered: FocusRenderLog,
    }

    struct EmptyComposite {
        rendered: FocusRenderLog,
        self_focusable: bool,
    }

    impl Component<FocusTestState, FocusTestMsg> for EmptyComposite {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, FocusTestState, FocusTestMsg>) {}

        fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, FocusTestState>) {
            self.rendered
                .lock()
                .expect("empty composite render log")
                .push((ctx.focused, ctx.contains_focus));
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

    impl Component<FocusTestState, FocusTestMsg> for FocusComposite {
        fn render(&mut self, ctx: &mut RenderCtx<'_, FocusTestState, FocusTestMsg>) {
            let area = ctx.area();
            ctx.render_component(
                ChildId::Static("child"),
                FocusLeaf::recording(Arc::clone(&self.child_rendered)),
                area,
            );
        }

        fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, FocusTestState>) {
            self.parent_rendered
                .lock()
                .expect("parent render log")
                .push((ctx.focused, ctx.contains_focus));
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

    fn render_leaf(ratcn: &mut Ratcn<(), ()>, terminal: &mut Terminal<TestBackend>, id: &ChildId) {
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &(), &theme, |ctx| {
                    ctx.render_component(id.clone(), Leaf, area);
                });
            })
            .expect("draw");
    }

    fn render_timing_button(
        ratcn: &mut Ratcn<ButtonTimingState, ButtonTimingMsg>,
        terminal: &mut Terminal<TestBackend>,
        state: &ButtonTimingState,
        theme: &Theme,
        message: impl Fn() -> ButtonTimingMsg + 'static,
    ) {
        let message = Rc::new(message);
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, state, theme, |ctx| {
                    let message = Rc::clone(&message);
                    ctx.render_component(
                        ChildId::Static("save"),
                        Button::new("Save")
                            .disabled(state.saving)
                            .on_press(move || message()),
                        area,
                    );
                });
            })
            .expect("draw");
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
        let mut ratcn = Ratcn::<u8, ()>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        let theme = Theme::default_dark();

        let scope_contains_focus = Arc::new(Mutex::new(Vec::new()));
        terminal
            .draw(|frame| {
                let root_area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    assert_eq!(ctx.area(), root_area);
                    assert_eq!(*ctx.state(), state);
                    ctx.paint(|ctx| assert!(!ctx.contains_focus));
                    let scope_contains_focus = Arc::clone(&scope_contains_focus);
                    ctx.scope(
                        ChildId::Static("scope"),
                        scope_area,
                        ScopeOptions::default(),
                        move |ctx| {
                            assert_eq!(ctx.area(), scope_area);
                            assert_eq!(*ctx.state(), state);
                            ctx.paint(move |ctx| {
                                assert_eq!(ctx.area(), scope_area);
                                scope_contains_focus
                                    .lock()
                                    .expect("scope flag log")
                                    .push(ctx.contains_focus);
                            });
                            ctx.render_component(
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
            })
            .expect("draw");
        // Paint runs once, after focus resolves: the modal takes it, so the
        // base scope does not contain it.
        assert_eq!(
            *scope_contains_focus.lock().expect("scope flag log"),
            [false]
        );
    }

    #[test]
    fn composite_declaration_builds_paths_and_scope_options() {
        let mut ratcn = Ratcn::<(), ()>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &(), &theme, |ctx| {
                    ctx.render_component(ChildId::Static("composite"), Composite, area);
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.declared_paths(),
            vec![
                vec![ChildId::Static("composite")],
                vec![ChildId::Static("composite"), ChildId::Static("leaf")],
            ]
        );
        assert_eq!(ratcn.surface.roots, vec![0]);
        assert_eq!(ratcn.surface.nodes[0].children, vec![1]);
        assert_eq!(ratcn.surface.nodes[1].parent, Some(0));
        assert_eq!(ratcn.surface.nodes[0].options.tab_wrap, TabWrap::Wrap);
        assert!(
            ratcn
                .surface
                .nodes
                .iter()
                .all(|node| node.component.is_some())
        );
    }

    #[test]
    fn duplicate_sibling_ids_panic_without_replacing_the_surface() {
        let mut ratcn = Ratcn::<(), ()>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        render_leaf(&mut ratcn, &mut terminal, &ChildId::Static("previous"));
        let theme = Theme::default_dark();

        let result = catch_unwind(AssertUnwindSafe(|| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &(), &theme, |ctx| {
                        ctx.render_component(ChildId::Static("duplicate"), Leaf, area);
                        ctx.render_component(ChildId::Static("duplicate"), Leaf, area);
                    });
                })
                .expect("draw");
        }));

        assert!(result.is_err());
        assert_eq!(
            ratcn.declared_paths(),
            vec![vec![ChildId::Static("previous")]]
        );
    }

    #[test]
    fn same_child_id_in_distinct_scopes_builds_and_routes_distinct_paths() {
        let mut state = FocusTestState {
            focus: FocusState::intent([ChildId::Static("left"), ChildId::Static("shared")]),
        };
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    for scope in ["left", "right"] {
                        ctx.scope(
                            ChildId::Static(scope),
                            Rect::ZERO,
                            ScopeOptions::default(),
                            |ctx| {
                                ctx.render_component(
                                    ChildId::Static("shared"),
                                    FocusLeaf::enabled(),
                                    area,
                                );
                            },
                        );
                    }
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(FocusTestMsg::Activated(vec![
                ChildId::Static("left"),
                ChildId::Static("shared"),
            ]))
        );
        state.focus = FocusState::intent([ChildId::Static("right"), ChildId::Static("shared")]);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
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
        let declared: PathLog = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(20, 6)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
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
            })
            .expect("draw");

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

        let recorded = declared.lock().expect("path log").clone();
        assert_eq!(recorded, expected);
        assert_eq!(ratcn.declared_paths(), expected);

        // Routing agrees with both: a press on the depth-four leaf reports the
        // same four segments back.
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 5, 1), &state),
            EventResult::Emit(FocusTestMsg::Focus(FocusState::intent(path(&[
                "outer", "row-1", "sheet", "cell-2"
            ]))))
        );
    }

    #[test]
    fn declaration_panic_does_not_replace_the_previous_surface() {
        let mut ratcn = Ratcn::<(), ()>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        render_leaf(&mut ratcn, &mut terminal, &ChildId::Static("stable"));
        let theme = Theme::default_dark();

        let result = catch_unwind(AssertUnwindSafe(|| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &(), &theme, |ctx| {
                        ctx.render_component(ChildId::Static("staged"), Leaf, area);
                        panic!("declaration failed");
                    });
                })
                .expect("draw");
        }));

        assert!(result.is_err());
        assert_eq!(
            ratcn.declared_paths(),
            vec![vec![ChildId::Static("stable")]]
        );
    }

    #[test]
    fn component_panic_does_not_replace_the_previous_surface() {
        let mut ratcn = Ratcn::<(), ()>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        render_leaf(&mut ratcn, &mut terminal, &ChildId::Static("stable"));
        let theme = Theme::default_dark();

        let result = catch_unwind(AssertUnwindSafe(|| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &(), &theme, |ctx| {
                        ctx.render_component(ChildId::Static("panicking"), PanickingLeaf, area);
                    });
                })
                .expect("draw");
        }));

        assert!(result.is_err());
        assert_eq!(
            ratcn.declared_paths(),
            vec![vec![ChildId::Static("stable")]]
        );
    }

    #[test]
    fn the_closure_declares_once_and_queued_paint_runs_once_on_the_frame() {
        let mut ratcn = Ratcn::<(), ()>::new();
        let mut terminal = Terminal::new(TestBackend::new(6, 1)).expect("terminal");
        let theme = Theme::default_dark();
        let declared = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(Mutex::new(Vec::new()));

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &(), &theme, |ctx| {
                    declared.fetch_add(1, Ordering::SeqCst);
                    let seen = Arc::clone(&seen);
                    ctx.paint(move |ctx| {
                        let before = ctx.with_buffer(|buf| {
                            buf.cell((0, 0)).expect("probe cell").symbol().to_owned()
                        });
                        seen.lock().expect("probe log").push(before);
                        ctx.with_buffer(|buf| {
                            buf.cell_mut((0, 0)).expect("probe cell").set_symbol("X");
                        });
                    });
                    ctx.render_component(ChildId::Static("leaf"), Leaf, area);
                });
            })
            .expect("draw");

        // The closure ran once and the paint it queued ran once, against the
        // frame itself.
        assert_eq!(declared.load(Ordering::SeqCst), 1);
        assert_eq!(*seen.lock().expect("probe log"), [" "]);
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((0, 0)).expect("painted cell").symbol(), "X");
    }

    #[test]
    fn caught_component_panic_marks_the_whole_pass_as_failed() {
        let mut ratcn = Ratcn::<(), ()>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        render_leaf(&mut ratcn, &mut terminal, &ChildId::Static("stable"));
        let theme = Theme::default_dark();

        let result = catch_unwind(AssertUnwindSafe(|| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &(), &theme, |ctx| {
                        ctx.render_component(ChildId::Static("catching"), CatchingComposite, area);
                    });
                })
                .expect("draw");
        }));

        assert!(result.is_err());
        assert_eq!(
            ratcn.declared_paths(),
            vec![vec![ChildId::Static("stable")]]
        );
    }

    #[test]
    fn caught_duplicate_id_panic_marks_the_whole_pass_as_failed() {
        let mut ratcn = Ratcn::<(), ()>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        render_leaf(&mut ratcn, &mut terminal, &ChildId::Static("stable"));
        let theme = Theme::default_dark();

        let result = catch_unwind(AssertUnwindSafe(|| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &(), &theme, |ctx| {
                        ctx.render_component(ChildId::Static("duplicate"), Leaf, area);
                        let caught = catch_unwind(AssertUnwindSafe(|| {
                            ctx.render_component(ChildId::Static("duplicate"), Leaf, area);
                        }));
                        assert!(caught.is_err());
                    });
                })
                .expect("draw");
        }));

        assert!(result.is_err());
        assert_eq!(
            ratcn.declared_paths(),
            vec![vec![ChildId::Static("stable")]]
        );
    }

    #[test]
    fn caught_scope_option_panic_marks_the_whole_pass_as_failed() {
        let mut ratcn = Ratcn::<(), ()>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        render_leaf(&mut ratcn, &mut terminal, &ChildId::Static("stable"));
        let theme = Theme::default_dark();

        let result = catch_unwind(AssertUnwindSafe(|| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &(), &theme, |ctx| {
                        let caught = catch_unwind(AssertUnwindSafe(|| {
                            ctx.render_component(
                                ChildId::Static("panicking-options"),
                                PanickingScopeOptions,
                                area,
                            );
                        }));
                        assert!(caught.is_err());
                    });
                })
                .expect("draw");
        }));

        assert!(result.is_err());
        assert_eq!(
            ratcn.declared_paths(),
            vec![vec![ChildId::Static("stable")]]
        );
    }

    #[test]
    fn caught_resolve_panic_marks_the_whole_pass_as_failed() {
        let mut ratcn = Ratcn::<(), ()>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        render_leaf(&mut ratcn, &mut terminal, &ChildId::Static("stable"));
        let theme = Theme::default_dark();

        let result = catch_unwind(AssertUnwindSafe(|| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &(), &theme, |ctx| {
                        let caught = catch_unwind(AssertUnwindSafe(|| {
                            ctx.render_component(
                                ChildId::Static("panicking-resolve"),
                                PanickingResolve,
                                area,
                            );
                        }));
                        assert!(caught.is_err());
                    });
                })
                .expect("draw");
        }));

        assert!(result.is_err());
        assert_eq!(
            ratcn.declared_paths(),
            vec![vec![ChildId::Static("stable")]]
        );
    }

    #[test]
    fn caught_focusability_panic_marks_the_whole_pass_as_failed() {
        let mut ratcn = Ratcn::<(), ()>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        render_leaf(&mut ratcn, &mut terminal, &ChildId::Static("stable"));
        let theme = Theme::default_dark();

        let result = catch_unwind(AssertUnwindSafe(|| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &(), &theme, |ctx| {
                        let caught = catch_unwind(AssertUnwindSafe(|| {
                            ctx.render_component(
                                ChildId::Static("panicking-focusable"),
                                PanickingFocusable,
                                area,
                            );
                        }));
                        assert!(caught.is_err());
                    });
                })
                .expect("draw");
        }));

        assert!(result.is_err());
        assert_eq!(
            ratcn.declared_paths(),
            vec![vec![ChildId::Static("stable")]]
        );
    }

    #[test]
    fn caught_interaction_area_panic_marks_the_whole_pass_as_failed() {
        let mut ratcn = Ratcn::<(), ()>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        render_leaf(&mut ratcn, &mut terminal, &ChildId::Static("stable"));
        let theme = Theme::default_dark();

        let result = catch_unwind(AssertUnwindSafe(|| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &(), &theme, |ctx| {
                        let caught = catch_unwind(AssertUnwindSafe(|| {
                            ctx.render_component(
                                ChildId::Static("panicking-area"),
                                PanickingInteractionArea,
                                area,
                            );
                        }));
                        assert!(caught.is_err());
                    });
                })
                .expect("draw");
        }));

        assert!(result.is_err());
        assert_eq!(
            ratcn.declared_paths(),
            vec![vec![ChildId::Static("stable")]]
        );
    }

    #[test]
    fn escaping_interaction_area_panics_without_replacing_the_surface() {
        let mut ratcn = Ratcn::<(), ()>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        render_leaf(&mut ratcn, &mut terminal, &ChildId::Static("stable"));
        let theme = Theme::default_dark();

        let result = catch_unwind(AssertUnwindSafe(|| {
            terminal
                .draw(|frame| {
                    ratcn.render(frame, &(), &theme, |ctx| {
                        ctx.render_component(
                            ChildId::Static("escaping-area"),
                            EscapingInteractionArea,
                            Rect::new(2, 1, 4, 1),
                        );
                    });
                })
                .expect("draw");
        }));

        assert!(result.is_err());
        assert_eq!(
            ratcn.declared_paths(),
            vec![vec![ChildId::Static("stable")]]
        );
    }

    #[test]
    fn deferred_paint_finishes_before_surface_replacement() {
        let mut ratcn = Ratcn::<(), ()>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        let theme = Theme::default_dark();
        let painted = Arc::new(AtomicBool::new(false));
        let deferred_painted = Arc::clone(&painted);

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &(), &theme, |ctx| {
                    ctx.render_component(ChildId::Static("next"), Leaf, area);
                    let deferred_painted = Arc::clone(&deferred_painted);
                    ctx.defer_paint(move |_, ()| {
                        deferred_painted.store(true, Ordering::SeqCst);
                    });
                });
            })
            .expect("draw");

        assert!(painted.load(Ordering::SeqCst));
        assert_eq!(ratcn.declared_paths(), vec![vec![ChildId::Static("next")]]);
    }

    #[test]
    fn deferred_paint_panic_does_not_replace_the_previous_surface() {
        let mut ratcn = Ratcn::<(), ()>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        render_leaf(&mut ratcn, &mut terminal, &ChildId::Static("stable"));
        let theme = Theme::default_dark();

        let result = catch_unwind(AssertUnwindSafe(|| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &(), &theme, |ctx| {
                        ctx.render_component(ChildId::Static("next"), Leaf, area);
                        ctx.defer_paint(|_, ()| panic!("deferred paint failed"));
                    });
                })
                .expect("draw");
        }));

        assert!(result.is_err());
        assert_eq!(
            ratcn.declared_paths(),
            vec![vec![ChildId::Static("stable")]]
        );
    }

    #[test]
    fn startup_focus_renders_and_routes_to_the_first_focusable_leaf() {
        let state = FocusTestState::default();
        let rendered = Arc::new(Mutex::new(Vec::new()));
        let first_rendered = Arc::clone(&rendered);
        let second_rendered = Arc::clone(&rendered);
        let mut ratcn = Ratcn::new()
            .focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus)
            .tab_wrap(TabWrap::Wrap);
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.scope(
                        ChildId::Static("pane"),
                        Rect::ZERO,
                        ScopeOptions::default(),
                        |ctx| {
                            ctx.render_component(
                                ChildId::Static("first"),
                                FocusLeaf::recording(Arc::clone(&first_rendered)),
                                area,
                            );
                            ctx.render_component(
                                ChildId::Static("second"),
                                FocusLeaf::recording(Arc::clone(&second_rendered)),
                                area,
                            );
                        },
                    );
                });
            })
            .expect("draw");

        // Paint reports the resolved focus, once per leaf: startup focus
        // landed on the first, and that is what painted focused.
        assert_eq!(
            *rendered.lock().expect("render log"),
            vec![(true, true), (false, false)]
        );
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(FocusTestMsg::Activated(vec![
                ChildId::Static("pane"),
                ChildId::Static("first"),
            ]))
        );
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
            EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
                ChildId::Static("pane"),
                ChildId::Static("second"),
            ])))
        );
    }

    #[test]
    fn startup_focus_skips_collapsed_candidates_and_routing_agrees() {
        let state = FocusTestState::default();
        let collapsed = Arc::new(Mutex::new(Vec::new()));
        let visible = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.scope(
                        ChildId::Static("group"),
                        Rect::new(0, 0, 4, 1),
                        ScopeOptions::default(),
                        |ctx| {
                            ctx.render_component(
                                ChildId::Static("collapsed"),
                                FocusLeaf::recording(Arc::clone(&collapsed)),
                                Rect::new(0, 0, 0, 1),
                            );
                        },
                    );
                    ctx.render_component(
                        ChildId::Static("visible"),
                        FocusLeaf::recording(Arc::clone(&visible)),
                        Rect::new(5, 0, 4, 1),
                    );
                });
            })
            .expect("draw");

        // Focus resolves between the passes against actual geometry, so the
        // zero-area candidate is never targeted: startup focus lands on the
        // visible leaf, paints there, and routes there — render and routing
        // agree because both come from the same resolution.
        assert_eq!(
            *collapsed.lock().expect("collapsed render log"),
            [(false, false)]
        );
        assert_eq!(*visible.lock().expect("visible render log"), [(true, true)]);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(FocusTestMsg::Activated(vec![ChildId::Static("visible")])),
            "routing targets the leaf that actually painted focused"
        );
    }

    #[test]
    fn empty_focus_renders_and_routes_to_the_active_modal() {
        let state = FocusTestState::default();
        let base = Arc::new(Mutex::new(Vec::new()));
        let modal = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("base"),
                        FocusLeaf::recording(Arc::clone(&base)),
                        area,
                    );
                    ctx.modal(
                        ChildId::Static("modal"),
                        FocusLeaf::recording(Arc::clone(&modal)),
                        area,
                    );
                });
            })
            .expect("draw");

        // Startup focus resolves once, against the complete tree — the modal
        // is already known, so only the modal paints focused.
        assert_eq!(*base.lock().expect("base focus log"), [(false, false)]);
        assert_eq!(*modal.lock().expect("modal focus log"), [(true, true)]);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(FocusTestMsg::Activated(vec![ChildId::Static("modal")]))
        );
    }

    #[test]
    fn composite_reports_focus_within_and_receives_bubbled_events() {
        let state = FocusTestState::default();
        let parent_rendered = Arc::new(Mutex::new(Vec::new()));
        let child_rendered = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn = Ratcn::new()
            .focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus)
            .tab_wrap(TabWrap::Wrap);
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("composite"),
                        FocusComposite {
                            parent_rendered: Arc::clone(&parent_rendered),
                            child_rendered: Arc::clone(&child_rendered),
                        },
                        area,
                    );
                });
            })
            .expect("draw");

        assert_eq!(
            *parent_rendered.lock().expect("parent render log"),
            vec![(false, true)]
        );
        assert_eq!(
            *child_rendered.lock().expect("child render log"),
            vec![(true, true)]
        );
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Char('p'))), &state),
            EventResult::Emit(FocusTestMsg::Parent(vec![ChildId::Static("composite")]))
        );
    }

    #[test]
    fn empty_composite_does_not_claim_sibling_focus_but_can_focus_itself() {
        let state = FocusTestState::default();
        let empty_rendered = Arc::new(Mutex::new(Vec::new()));
        let leaf_rendered = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn = Ratcn::<FocusTestState, FocusTestMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("empty"),
                        EmptyComposite {
                            rendered: Arc::clone(&empty_rendered),
                            self_focusable: false,
                        },
                        area,
                    );
                    ctx.render_component(
                        ChildId::Static("leaf"),
                        FocusLeaf::recording(Arc::clone(&leaf_rendered)),
                        area,
                    );
                });
            })
            .expect("draw");

        assert_eq!(
            *empty_rendered.lock().expect("empty composite render log"),
            vec![(false, false)]
        );
        assert_eq!(
            *leaf_rendered.lock().expect("leaf render log"),
            vec![(true, true)]
        );

        empty_rendered
            .lock()
            .expect("empty composite render log")
            .clear();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("empty"),
                        EmptyComposite {
                            rendered: Arc::clone(&empty_rendered),
                            self_focusable: true,
                        },
                        area,
                    );
                });
            })
            .expect("draw");
        assert_eq!(
            *empty_rendered.lock().expect("empty composite render log"),
            vec![(true, true)]
        );
    }

    #[test]
    fn tab_and_backtab_traverse_siblings_and_honor_nested_escape_and_wrap() {
        let mut state = FocusTestState {
            focus: FocusState::intent([ChildId::Static("left"), ChildId::Static("a2")]),
        };
        let mut ratcn = Ratcn::new()
            .focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus)
            .tab_wrap(TabWrap::Wrap);
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        let theme = Theme::default_dark();

        let render = |ratcn: &mut Ratcn<FocusTestState, FocusTestMsg>,
                      terminal: &mut Terminal<TestBackend>,
                      state: &FocusTestState,
                      left_wrap| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, state, &theme, |ctx| {
                        ctx.render_component(ChildId::Static("before"), FocusLeaf::enabled(), area);
                        ctx.scope(
                            ChildId::Static("left"),
                            Rect::ZERO,
                            ScopeOptions::default().tab_wrap(left_wrap),
                            |ctx| {
                                ctx.render_component(
                                    ChildId::Static("a1"),
                                    FocusLeaf::enabled(),
                                    area,
                                );
                                ctx.render_component(
                                    ChildId::Static("a2"),
                                    FocusLeaf::enabled(),
                                    area,
                                );
                            },
                        );
                        ctx.scope(
                            ChildId::Static("right"),
                            Rect::ZERO,
                            ScopeOptions::default(),
                            |ctx| {
                                ctx.render_component(
                                    ChildId::Static("b1"),
                                    FocusLeaf::enabled(),
                                    area,
                                );
                            },
                        );
                    });
                })
                .expect("draw");
        };

        render(&mut ratcn, &mut terminal, &state, TabWrap::Escape);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::BackTab)), &state),
            EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
                ChildId::Static("left"),
                ChildId::Static("a1"),
            ])))
        );
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
            EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
                ChildId::Static("right"),
                ChildId::Static("b1"),
            ])))
        );
        state.focus = FocusState::intent([ChildId::Static("left"), ChildId::Static("a1")]);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::BackTab)), &state),
            EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([ChildId::Static(
                "before"
            ),])))
        );

        render(&mut ratcn, &mut terminal, &state, TabWrap::Wrap);
        state.focus = FocusState::intent([ChildId::Static("left"), ChildId::Static("a2")]);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
            EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
                ChildId::Static("left"),
                ChildId::Static("a1"),
            ])))
        );
        state.focus = FocusState::intent([ChildId::Static("left"), ChildId::Static("a1")]);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::BackTab)), &state),
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
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component("first", FocusLeaf::enabled(), area);
                    ctx.render_component("second", FocusLeaf::enabled(), area);
                });
            })
            .expect("draw");

        let backtab = |modifiers| {
            Event::Key(KeyEvent {
                code: KeyCode::BackTab,
                modifiers,
            })
        };
        assert_eq!(
            ratcn.handle_event(
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
                ratcn.handle_event(backtab(modifiers), &state),
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
        let mut ratcn = Ratcn::new()
            .focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus)
            .tab_wrap(TabWrap::Wrap);
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        let theme = Theme::default_dark();

        let render = |ratcn: &mut Ratcn<FocusTestState, FocusTestMsg>,
                      terminal: &mut Terminal<TestBackend>,
                      state: &FocusTestState,
                      ids: [ChildId; 3]| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, state, &theme, |ctx| {
                        ctx.scope(
                            ChildId::Static("items"),
                            Rect::ZERO,
                            ScopeOptions::default(),
                            |ctx| {
                                for id in &ids {
                                    ctx.render_component(id.clone(), FocusLeaf::enabled(), area);
                                }
                            },
                        );
                    });
                })
                .expect("draw");
        };

        render(
            &mut ratcn,
            &mut terminal,
            &state,
            [
                ChildId::Dynamic(Arc::from("a")),
                b.clone(),
                ChildId::Dynamic(Arc::from("c")),
            ],
        );
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
            EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
                ChildId::Static("items"),
                ChildId::Dynamic(Arc::from("c")),
            ])))
        );

        render(
            &mut ratcn,
            &mut terminal,
            &state,
            [
                ChildId::Dynamic(Arc::from("c")),
                b.clone(),
                ChildId::Dynamic(Arc::from("a")),
            ],
        );
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(FocusTestMsg::Activated(vec![ChildId::Static("items"), b,]))
        );
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
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
        let mut ratcn = Ratcn::new()
            .focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus)
            .tab_wrap(TabWrap::Wrap);
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.scope(
                        ChildId::Static("items"),
                        Rect::ZERO,
                        ScopeOptions::default(),
                        |ctx| {
                            ctx.render_component(
                                ChildId::Static("first"),
                                FocusLeaf::enabled(),
                                area,
                            );
                            ctx.render_component(
                                ChildId::Static("last"),
                                FocusLeaf::enabled(),
                                area,
                            );
                        },
                    );
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Ignored
        );
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
            EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
                ChildId::Static("items"),
                ChildId::Static("first"),
            ])))
        );
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::BackTab)), &state),
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
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.scope(
                        ChildId::Static("items"),
                        Rect::ZERO,
                        ScopeOptions::default(),
                        |_| {},
                    );
                });
            })
            .expect("draw");
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Ignored
        );

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.scope(
                        ChildId::Static("items"),
                        Rect::ZERO,
                        ScopeOptions::default(),
                        |ctx| {
                            ctx.render_component(
                                ChildId::Static("target"),
                                FocusLeaf::enabled(),
                                area,
                            );
                        },
                    );
                });
            })
            .expect("draw");
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
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
        let mut ratcn = Ratcn::new()
            .focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus)
            .tab_wrap(TabWrap::Wrap);
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        let theme = Theme::default_dark();

        let render = |ratcn: &mut Ratcn<FocusTestState, FocusTestMsg>,
                      terminal: &mut Terminal<TestBackend>,
                      left_wrap| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &state, &theme, |ctx| {
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
                                ctx.render_component(
                                    ChildId::Static("b1"),
                                    FocusLeaf::enabled(),
                                    area,
                                );
                            },
                        );
                    });
                })
                .expect("draw");
        };

        render(&mut ratcn, &mut terminal, TabWrap::Escape);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
            EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
                ChildId::Static("right"),
                ChildId::Static("b1"),
            ])))
        );

        render(&mut ratcn, &mut terminal, TabWrap::Wrap);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
            EventResult::Consumed
        );
    }

    #[test]
    fn scope_only_intent_descends_to_the_first_enabled_leaf() {
        let mut state = FocusTestState {
            focus: FocusState::intent([ChildId::Static("pane")]),
        };
        let mut ratcn = Ratcn::new()
            .focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus)
            .tab_wrap(TabWrap::Wrap);
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.scope(
                        ChildId::Static("pane"),
                        Rect::ZERO,
                        ScopeOptions::default(),
                        |ctx| {
                            ctx.render_component(
                                ChildId::Static("disabled"),
                                FocusLeaf::disabled(),
                                area,
                            );
                            ctx.render_component(
                                ChildId::Static("enabled"),
                                FocusLeaf::enabled(),
                                area,
                            );
                        },
                    );
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(FocusTestMsg::Activated(vec![
                ChildId::Static("pane"),
                ChildId::Static("enabled"),
            ]))
        );

        state.focus = FocusState::intent([ChildId::Static("pane"), ChildId::Static("disabled")]);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Ignored
        );
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
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
        let mut ratcn = Ratcn::new()
            .focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus)
            .tab_wrap(TabWrap::Wrap);
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.scope(
                        ChildId::Static("pane"),
                        Rect::ZERO,
                        ScopeOptions::default().focus_key('x', [ChildId::Static("second")]),
                        |ctx| {
                            ctx.render_component(
                                ChildId::Static("first"),
                                FocusLeaf::enabled(),
                                area,
                            );
                            ctx.render_component(
                                ChildId::Static("second"),
                                FocusLeaf::enabled(),
                                area,
                            );
                        },
                    );
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Char('x'))), &state),
            EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
                ChildId::Static("pane"),
                ChildId::Static("second"),
            ])))
        );

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.scope(
                        ChildId::Static("pane"),
                        Rect::ZERO,
                        ScopeOptions::default().focus_key('x', [ChildId::Static("second")]),
                        |ctx| {
                            ctx.render_component(
                                ChildId::Static("first"),
                                FocusLeaf::consuming_focus_key(),
                                area,
                            );
                            ctx.render_component(
                                ChildId::Static("second"),
                                FocusLeaf::enabled(),
                                area,
                            );
                        },
                    );
                });
            })
            .expect("draw");
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Char('x'))), &state),
            EventResult::Consumed
        );
    }

    #[test]
    fn focus_keys_normalize_chars_but_match_ctrl_and_alt_exactly() {
        let mut state = FocusTestState {
            focus: FocusState::intent([ChildId::Static("first")]),
        };
        let mut ratcn = Ratcn::new()
            .focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus)
            .focus_key('c', [ChildId::Static("second")])
            .focus_key(
                KeyChord::from('m').ctrl().alt(),
                [ChildId::Static("second")],
            );
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(ChildId::Static("first"), FocusLeaf::enabled(), area);
                    ctx.render_component(ChildId::Static("second"), FocusLeaf::enabled(), area);
                });
            })
            .expect("draw");

        let key = |code, ctrl, alt, shift| {
            Event::Key(KeyEvent {
                code,
                modifiers: Modifiers { ctrl, alt, shift },
            })
        };
        let second = FocusTestMsg::Focus(FocusState::intent([ChildId::Static("second")]));
        assert_eq!(
            ratcn.handle_event(key(KeyCode::Char('C'), false, false, true), &state),
            EventResult::Emit(second)
        );
        assert_eq!(
            ratcn.handle_event(key(KeyCode::Char('c'), true, false, false), &state),
            EventResult::Ignored
        );
        for (ctrl, alt) in [(false, false), (true, false), (false, true)] {
            assert_eq!(
                ratcn.handle_event(key(KeyCode::Char('m'), ctrl, alt, false), &state),
                EventResult::Ignored
            );
        }
        assert!(matches!(
            ratcn.handle_event(key(KeyCode::Char('m'), true, true, false), &state),
            EventResult::Emit(FocusTestMsg::Focus(_))
        ));

        state.focus = FocusState::intent([ChildId::Static("second")]);
        assert_eq!(
            ratcn.handle_event(key(KeyCode::Char('C'), false, false, true), &state),
            EventResult::Consumed,
            "an already-satisfied focus shortcut must not emit redundant state"
        );
    }

    #[test]
    fn invalid_inner_focus_key_falls_back_to_the_outer_binding() {
        let state = FocusTestState {
            focus: FocusState::intent([ChildId::Static("pane"), ChildId::Static("first")]),
        };
        let mut ratcn = Ratcn::new()
            .focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus)
            .focus_key('x', [ChildId::Static("outside")]);
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.scope(
                        ChildId::Static("pane"),
                        Rect::ZERO,
                        ScopeOptions::default().focus_key('x', [ChildId::Static("missing")]),
                        |ctx| {
                            ctx.render_component(
                                ChildId::Static("first"),
                                FocusLeaf::enabled(),
                                area,
                            );
                        },
                    );
                    ctx.render_component(ChildId::Static("outside"), FocusLeaf::enabled(), area);
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Char('x'))), &state),
            EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([ChildId::Static(
                "outside"
            ),])))
        );
    }

    #[test]
    fn events_before_the_first_render_are_ignored() {
        let state = FocusTestState::default();
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);

        assert!(!ratcn.has_rendered());

        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
            EventResult::Ignored
        );

        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        let theme = Theme::default_dark();
        let failed = catch_unwind(AssertUnwindSafe(|| {
            terminal
                .draw(|frame| {
                    ratcn.render(frame, &state, &theme, |_| panic!("first render failed"));
                })
                .expect("failed draw");
        }));
        assert!(failed.is_err());
        assert!(!ratcn.has_rendered());

        terminal
            .draw(|frame| ratcn.render(frame, &state, &theme, |_| {}))
            .expect("draw");

        assert!(ratcn.has_rendered());
    }

    #[test]
    fn semantic_modal_before_the_first_render_still_ignores_events() {
        let mut state = ModalTestState::default();
        state
            .modals
            .open(ChildId::Static("dialog"), &mut state.focus)
            .expect("open modal");
        let mut ratcn: Ratcn<ModalTestState, ModalTestMsg> =
            Ratcn::new().modals(|state: &ModalTestState| &state.modals);

        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
            EventResult::Ignored
        );
    }

    #[test]
    fn button_events_use_last_rendered_disabledness_until_redraw() {
        let mut state = ButtonTimingState::default();
        let mut ratcn = Ratcn::new().focus(
            |state: &ButtonTimingState| &state.focus,
            ButtonTimingMsg::Focus,
        );
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        let theme = Theme::default_dark();
        let enter = Event::Key(KeyEvent::new(KeyCode::Enter));

        render_timing_button(&mut ratcn, &mut terminal, &state, &theme, || {
            ButtonTimingMsg::Save
        });
        let EventResult::Emit(first) = ratcn.handle_event(enter.clone(), &state) else {
            panic!("rendered enabled button did not emit");
        };
        assert!(update_button_timing(&mut state, first));

        let EventResult::Emit(second) = ratcn.handle_event(enter.clone(), &state) else {
            panic!("old enabled declaration did not handle the second event");
        };
        assert!(!update_button_timing(&mut state, second));
        assert_eq!(state.accepted_saves, 1);

        render_timing_button(&mut ratcn, &mut terminal, &state, &theme, || {
            ButtonTimingMsg::Save
        });
        assert_eq!(
            ratcn.handle_event(enter.clone(), &state),
            EventResult::Ignored
        );

        state.saving = false;
        assert_eq!(ratcn.handle_event(enter, &state), EventResult::Ignored);
    }

    #[test]
    fn failed_render_keeps_the_previous_button_declaration_interactive() {
        let mut state = ButtonTimingState::default();
        let mut ratcn = Ratcn::new().focus(
            |state: &ButtonTimingState| &state.focus,
            ButtonTimingMsg::Focus,
        );
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        let theme = Theme::default_dark();
        render_timing_button(&mut ratcn, &mut terminal, &state, &theme, || {
            ButtonTimingMsg::Save
        });
        state.saving = true;

        let result = catch_unwind(AssertUnwindSafe(|| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.render_component(
                            ChildId::Static("save"),
                            Button::new("Replacement")
                                .disabled(true)
                                .on_press(|| ButtonTimingMsg::Replacement),
                            area,
                        );
                        panic!("failed after staging replacement button");
                    });
                })
                .expect("draw");
        }));

        assert!(result.is_err());
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(ButtonTimingMsg::Save)
        );
    }

    /// The pointer tests need no app state at all: hover is the runtime's.
    #[derive(Debug, Default)]
    struct PointerState;

    #[derive(Default)]
    struct ModalPointerState {
        focus: FocusState,
        modals: ModalState,
    }

    #[derive(Debug, Clone, PartialEq)]
    enum PointerMsg {
        Routed(&'static str, MouseKind, usize),
        Transient(usize),
        Drag(DragPhase),
        Dismissed,
    }

    #[derive(Debug, Default)]
    struct HoverFocusState {
        focus: FocusState,
    }

    #[derive(Debug, Clone, PartialEq)]
    enum HoverFocusMsg {
        Focus(FocusState),
    }

    struct HoverFocusLeaf {
        enabled: bool,
    }

    impl HoverFocusLeaf {
        fn enabled() -> Self {
            Self { enabled: true }
        }

        fn disabled() -> Self {
            Self { enabled: false }
        }
    }

    impl Component<HoverFocusState, HoverFocusMsg> for HoverFocusLeaf {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, HoverFocusState, HoverFocusMsg>) {}

        fn is_focusable(&self, _state: &HoverFocusState) -> bool {
            self.enabled
        }
    }

    struct HoverFocusComposite;

    impl Component<HoverFocusState, HoverFocusMsg> for HoverFocusComposite {
        fn render(&mut self, ctx: &mut RenderCtx<'_, HoverFocusState, HoverFocusMsg>) {
            let area = ctx.area();
            ctx.render_component(ChildId::Static("leaf"), HoverFocusLeaf::enabled(), area);
        }

        fn scope_options(&self) -> ScopeOptions {
            ScopeOptions::default()
        }
    }

    #[derive(Default)]
    struct DragTransient {
        events: usize,
    }

    struct Draggable {
        name: &'static str,
    }

    #[derive(Clone, Copy)]
    struct LifecycleDrag {
        offset: CellOffset,
        can_start: bool,
    }

    impl Component<PointerState, PointerMsg> for LifecycleDrag {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, PointerState, PointerMsg>) {}

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

    impl Component<PointerState, PointerMsg> for Draggable {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, PointerState, PointerMsg>) {}

        fn handle_event(
            &mut self,
            event: &Event,
            _state: &PointerState,
            ctx: &mut EventCtx<'_>,
        ) -> EventResult<PointerMsg> {
            let Event::Mouse(mouse) = event else {
                return EventResult::Ignored;
            };
            match mouse.kind {
                MouseKind::Down(MouseButton::Left) => {
                    ctx.capture_pointer(MouseButton::Left);
                    ctx.transient::<DragTransient>().events += 1;
                    EventResult::Consumed
                }
                MouseKind::Drag(MouseButton::Left)
                | MouseKind::Up(MouseButton::Left)
                | MouseKind::Click(MouseButton::Left)
                | MouseKind::DragEnd(MouseButton::Left) => {
                    let transient = ctx.transient::<DragTransient>();
                    transient.events += 1;
                    EventResult::Emit(PointerMsg::Routed(self.name, mouse.kind, transient.events))
                }
                _ => EventResult::Ignored,
            }
        }
    }

    struct HoverLeaf {
        consume_move: bool,
        rendered: Option<HoverRenderLog>,
    }

    type HoverRenderLog = Arc<Mutex<Vec<(bool, bool)>>>;

    struct ModalHoverLeaf {
        rendered: HoverRenderLog,
    }

    impl Component<ModalPointerState, PointerMsg> for ModalHoverLeaf {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, ModalPointerState, PointerMsg>) {}

        fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, ModalPointerState>) {
            self.rendered
                .lock()
                .expect("modal hover render log")
                .push((ctx.hovered, ctx.contains_hover));
        }
    }

    struct ModalPointerLeaf;

    impl Component<ModalPointerState, PointerMsg> for ModalPointerLeaf {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, ModalPointerState, PointerMsg>) {}
    }

    impl Component<PointerState, PointerMsg> for HoverLeaf {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, PointerState, PointerMsg>) {}

        fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, PointerState>) {
            if let Some(rendered) = &self.rendered {
                rendered
                    .lock()
                    .expect("hover render log")
                    .push((ctx.hovered, ctx.contains_hover));
            }
        }

        fn handle_event(
            &mut self,
            event: &Event,
            _state: &PointerState,
            _ctx: &mut EventCtx<'_>,
        ) -> EventResult<PointerMsg> {
            if self.consume_move
                && matches!(
                    event,
                    Event::Mouse(MouseEvent {
                        kind: MouseKind::Moved,
                        ..
                    })
                )
            {
                EventResult::Consumed
            } else {
                EventResult::Ignored
            }
        }
    }

    struct EmittingHoverLeaf;

    impl Component<PointerState, PointerMsg> for EmittingHoverLeaf {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, PointerState, PointerMsg>) {}

        fn handle_event(
            &mut self,
            event: &Event,
            _state: &PointerState,
            _ctx: &mut EventCtx<'_>,
        ) -> EventResult<PointerMsg> {
            if matches!(
                event,
                Event::Mouse(MouseEvent {
                    kind: MouseKind::Moved,
                    ..
                })
            ) {
                EventResult::Emit(PointerMsg::Routed("move", MouseKind::Moved, 1))
            } else {
                EventResult::Ignored
            }
        }
    }

    /// Claims the pointer on its press and reports the hover flags it paints
    /// with, so a test can watch what a frame does to a capturing node's
    /// hover.
    struct CapturingHoverLeaf {
        rendered: HoverRenderLog,
    }

    impl Component<PointerState, PointerMsg> for CapturingHoverLeaf {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, PointerState, PointerMsg>) {}

        fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, PointerState>) {
            self.rendered
                .lock()
                .expect("capturing hover log")
                .push((ctx.hovered, ctx.contains_hover));
        }

        fn handle_event(
            &mut self,
            event: &Event,
            _state: &PointerState,
            ctx: &mut EventCtx<'_>,
        ) -> EventResult<PointerMsg> {
            match event {
                Event::Mouse(mouse) if mouse.kind == MouseKind::Down(MouseButton::Left) => {
                    ctx.capture_pointer(MouseButton::Left);
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            }
        }
    }

    struct StringTransient;

    impl Component<PointerState, PointerMsg> for StringTransient {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, PointerState, PointerMsg>) {}

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
        fn render(&mut self, _ctx: &mut RenderCtx<'_, PointerState, PointerMsg>) {}

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
        fn render(&mut self, _ctx: &mut RenderCtx<'_, PointerState, PointerMsg>) {}

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
        fn render(&mut self, _ctx: &mut RenderCtx<'_, PointerState, PointerMsg>) {}

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
                ctx.transient::<CleanupTransient>().dropped =
                    Some(Arc::clone(&self.transient_dropped));
                self.armed = true;
                EventResult::Consumed
            } else {
                EventResult::Ignored
            }
        }
    }

    struct RouteLeaf(&'static str);

    impl Component<PointerState, PointerMsg> for RouteLeaf {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, PointerState, PointerMsg>) {}

        fn handle_event(
            &mut self,
            event: &Event,
            _state: &PointerState,
            _ctx: &mut EventCtx<'_>,
        ) -> EventResult<PointerMsg> {
            match event {
                Event::Mouse(mouse) => EventResult::Emit(PointerMsg::Routed(self.0, mouse.kind, 0)),
                _ => EventResult::Ignored,
            }
        }
    }

    struct RecordingPointer {
        name: &'static str,
        events: Arc<Mutex<Vec<(&'static str, MouseKind)>>>,
        capture: bool,
    }

    impl Component<PointerState, PointerMsg> for RecordingPointer {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, PointerState, PointerMsg>) {}

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
            self.events
                .lock()
                .expect("pointer event log")
                .push((self.name, mouse.kind));
            EventResult::Consumed
        }
    }

    fn mouse(kind: MouseKind, column: u16, row: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: super::super::Modifiers::NONE,
        })
    }

    fn render_drag_surface(
        ratcn: &mut Ratcn<PointerState, PointerMsg>,
        terminal: &mut Terminal<TestBackend>,
        state: &PointerState,
        ids: &[(&'static str, &'static str, Rect)],
    ) {
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                ratcn.render(frame, state, &theme, |ctx| {
                    for &(id, name, area) in ids {
                        ctx.render_component(ChildId::Static(id), Draggable { name }, area);
                    }
                });
            })
            .expect("draw");
    }

    fn render_lifecycle_drag(
        ratcn: &mut Ratcn<PointerState, PointerMsg>,
        terminal: &mut Terminal<TestBackend>,
        state: &PointerState,
        component: Option<LifecycleDrag>,
        area: Rect,
    ) {
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                ratcn.render(frame, state, &theme, |ctx| {
                    if let Some(component) = component {
                        ctx.render_component(ChildId::Static("drag"), component, area);
                    }
                });
            })
            .expect("draw");
    }

    #[test]
    fn drag_helper_stays_captured_across_rebuild_and_ends_outside() {
        let state = PointerState;
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).expect("terminal");
        render_lifecycle_drag(
            &mut ratcn,
            &mut terminal,
            &state,
            Some(LifecycleDrag {
                offset: CellOffset::new(3, -1),
                can_start: true,
            }),
            Rect::new(0, 0, 4, 2),
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state),
            EventResult::Emit(PointerMsg::Drag(DragPhase::Down))
        );

        render_lifecycle_drag(
            &mut ratcn,
            &mut terminal,
            &state,
            Some(LifecycleDrag {
                offset: CellOffset::default(),
                can_start: false,
            }),
            Rect::new(10, 0, 4, 2),
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 19, 3), &state),
            EventResult::Emit(PointerMsg::Drag(DragPhase::Moved {
                offset: CellOffset::new(21, 1),
                position: Position::new(19, 3),
            }))
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 19, 3), &state),
            EventResult::Emit(PointerMsg::Drag(DragPhase::Ended {
                position: Position::new(19, 3),
                moved: true,
            }))
        );
        assert!(ratcn.transients.is_empty());
        assert!(ratcn.capture_path(MouseButton::Left).is_none());
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Drag(MouseButton::Left), 19, 3), &state),
            EventResult::Ignored
        );
    }

    #[test]
    fn drag_helper_path_removal_cleans_transient_and_suppresses_capture() {
        let state = PointerState;
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).expect("terminal");
        render_lifecycle_drag(
            &mut ratcn,
            &mut terminal,
            &state,
            Some(LifecycleDrag {
                offset: CellOffset::default(),
                can_start: true,
            }),
            Rect::new(0, 0, 4, 2),
        );
        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);

        render_lifecycle_drag(&mut ratcn, &mut terminal, &state, None, Rect::ZERO);
        assert!(ratcn.transients.is_empty());
        assert!(ratcn.capture_path(MouseButton::Left).is_none());
        assert!(ratcn.is_suppressed(MouseButton::Left));
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 19, 3), &state),
            EventResult::Consumed
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 19, 3), &state),
            EventResult::Consumed
        );
        assert!(!ratcn.is_suppressed(MouseButton::Left));
    }

    /// One button's gesture says nothing about another's. A left press that
    /// claimed a component does not route the right button's release to it,
    /// and a left gesture called off by a redraw does not swallow the right
    /// button's events — each button is tracked, cancelled, and ended alone.
    #[test]
    fn one_buttons_gesture_never_routes_or_suppresses_another() {
        let state = PointerState;
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(12, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let mut render = |ratcn: &mut Ratcn<PointerState, PointerMsg>, grabber| {
            terminal
                .draw(|frame| {
                    ratcn.render(frame, &state, &theme, |ctx| {
                        if grabber {
                            ctx.render_component(
                                ChildId::Static("grabber"),
                                RecordingPointer {
                                    name: "grabber",
                                    events: Arc::clone(&events),
                                    capture: true,
                                },
                                Rect::new(0, 0, 5, 2),
                            );
                        }
                        ctx.render_component(
                            ChildId::Static("other"),
                            RecordingPointer {
                                name: "other",
                                events: Arc::clone(&events),
                                capture: false,
                            },
                            Rect::new(6, 0, 6, 2),
                        );
                    });
                })
                .expect("draw");
        };

        render(&mut ratcn, true);
        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state);
        assert_eq!(
            ratcn.capture_path(MouseButton::Left),
            Some([ChildId::Static("grabber")].as_slice())
        );
        assert!(
            ratcn.capture_path(MouseButton::Right).is_none(),
            "the right button claimed nothing"
        );

        // The right button's release is routed by its own gesture, not by the
        // left button's claim.
        events.lock().expect("pointer event log").clear();
        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Right), 8, 0), &state);
        ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Right), 8, 0), &state);
        assert!(
            events
                .lock()
                .expect("pointer event log")
                .iter()
                .all(|&(name, _)| name == "other"),
            "the right button's gesture belongs to what it hit"
        );

        // The left gesture's component disappears: that button is called off,
        // and only that button.
        render(&mut ratcn, false);
        assert!(ratcn.is_suppressed(MouseButton::Left));
        assert!(!ratcn.is_suppressed(MouseButton::Right));

        events.lock().expect("pointer event log").clear();
        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Right), 8, 0), &state);
        ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Right), 8, 0), &state);
        assert!(
            !events.lock().expect("pointer event log").is_empty(),
            "a suppressed left gesture must not swallow the right button"
        );

        events.lock().expect("pointer event log").clear();
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 1, 0), &state),
            EventResult::Consumed,
            "the called-off left release is swallowed"
        );
        assert!(events.lock().expect("pointer event log").is_empty());
        assert!(
            !ratcn.is_suppressed(MouseButton::Left),
            "and that release ends the gesture"
        );
    }

    #[test]
    fn capture_and_transient_follow_identity_through_replacement_and_reorder() {
        let state = PointerState;
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).expect("terminal");
        render_drag_surface(
            &mut ratcn,
            &mut terminal,
            &state,
            &[
                ("drag", "old", Rect::new(0, 0, 4, 2)),
                ("other", "other", Rect::new(5, 0, 4, 2)),
            ],
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state),
            EventResult::Consumed
        );

        render_drag_surface(
            &mut ratcn,
            &mut terminal,
            &state,
            &[
                ("other", "other", Rect::new(5, 0, 4, 2)),
                ("drag", "replacement", Rect::new(10, 0, 4, 2)),
            ],
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 19, 3), &state),
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
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).expect("terminal");
        render_drag_surface(
            &mut ratcn,
            &mut terminal,
            &state,
            &[("drag", "drag", Rect::new(0, 0, 4, 2))],
        );
        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
        ratcn.handle_event(mouse(MouseKind::Moved, 19, 3), &state);

        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 19, 3), &state),
            EventResult::Emit(PointerMsg::Routed(
                "drag",
                MouseKind::Up(MouseButton::Left),
                3,
            ))
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Drag(MouseButton::Left), 19, 3), &state),
            EventResult::Ignored
        );
    }

    #[test]
    fn pointer_exit_cancels_capture_and_stale_press_before_reentry() {
        let state = PointerState;
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).expect("terminal");
        render_drag_surface(
            &mut ratcn,
            &mut terminal,
            &state,
            &[("drag", "drag", Rect::new(0, 0, 4, 2))],
        );
        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
        assert!(ratcn.capture_path(MouseButton::Left).is_some());
        assert_eq!(ratcn.mouse_tracker.pressed_buttons(), [MouseButton::Left]);

        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Exited, 1, 1), &state),
            EventResult::Consumed
        );
        assert!(ratcn.mouse_tracker.pressed_buttons().is_empty());
        assert!(ratcn.gestures.is_empty());

        // Re-entry is plain motion: it moves hover (which the exit emptied)
        // and nothing else — a surviving press would have made it a drag, and
        // the component would have emitted.
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 1, 1), &state),
            EventResult::Consumed
        );
    }

    #[test]
    fn disappearing_capture_is_suppressed_through_reappearance_until_release() {
        let state = PointerState;
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).expect("terminal");
        render_drag_surface(
            &mut ratcn,
            &mut terminal,
            &state,
            &[("drag", "before", Rect::new(0, 0, 4, 2))],
        );
        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
        render_drag_surface(&mut ratcn, &mut terminal, &state, &[]);
        render_drag_surface(
            &mut ratcn,
            &mut terminal,
            &state,
            &[("drag", "after", Rect::new(0, 0, 4, 2))],
        );

        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 1, 1), &state),
            EventResult::Consumed
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 1, 1), &state),
            EventResult::Consumed
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state),
            EventResult::Consumed
        );
    }

    #[test]
    fn deferred_paint_failure_preserves_capture_transient_and_previous_component() {
        let state = PointerState;
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).expect("terminal");
        render_drag_surface(
            &mut ratcn,
            &mut terminal,
            &state,
            &[("drag", "stable", Rect::new(0, 0, 4, 2))],
        );
        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
        let theme = Theme::default_dark();
        let result = catch_unwind(AssertUnwindSafe(|| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.render_component(
                            ChildId::Static("drag"),
                            Draggable {
                                name: "replacement",
                            },
                            area,
                        );
                        ctx.defer_paint(|_, _| panic!("deferred paint failed"));
                    });
                })
                .expect("draw");
        }));
        assert!(result.is_err());

        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 19, 3), &state),
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
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(ChildId::Static("typed"), StringTransient, area);
                });
            })
            .expect("draw");
        ratcn.handle_event(mouse(MouseKind::Moved, 0, 0), &state);
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(ChildId::Static("typed"), NumberTransient, area);
                });
            })
            .expect("draw");

        let panic = catch_unwind(AssertUnwindSafe(|| {
            ratcn.handle_event(mouse(MouseKind::Moved, 0, 0), &state);
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
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let render_probe = |ratcn: &mut Ratcn<PointerState, PointerMsg>,
                            terminal: &mut Terminal<TestBackend>,
                            present| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &state, &theme, |ctx| {
                        if present {
                            ctx.render_component(ChildId::Static("probe"), TransientProbe, area);
                        }
                    });
                })
                .expect("draw");
        };

        render_probe(&mut ratcn, &mut terminal, true);
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 0, 0), &state),
            EventResult::Emit(PointerMsg::Transient(1))
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 0, 0), &state),
            EventResult::Emit(PointerMsg::Transient(2))
        );
        render_probe(&mut ratcn, &mut terminal, false);
        render_probe(&mut ratcn, &mut terminal, true);
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 0, 0), &state),
            EventResult::Emit(PointerMsg::Transient(1))
        );
    }

    #[test]
    fn capture_and_transient_cleanup_finish_before_previous_component_drop() {
        let state = PointerState;
        let transient_dropped = Arc::new(AtomicBool::new(false));
        let component_dropped = Arc::new(AtomicBool::new(false));
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("cleanup"),
                        CleanupComponent {
                            transient_dropped: Arc::clone(&transient_dropped),
                            component_dropped: Arc::clone(&component_dropped),
                            armed: false,
                        },
                        area,
                    );
                });
            })
            .expect("draw");
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 0, 0), &state,),
            EventResult::Consumed
        );

        terminal
            .draw(|frame| ratcn.render(frame, &state, &theme, |_| {}))
            .expect("draw");

        assert!(transient_dropped.load(Ordering::SeqCst));
        assert!(component_dropped.load(Ordering::SeqCst));
        assert!(ratcn.capture_path(MouseButton::Left).is_none());
        assert!(ratcn.is_suppressed(MouseButton::Left));
    }

    #[test]
    fn reverse_paint_order_routes_overlap_to_the_topmost_component() {
        let state = PointerState;
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(ChildId::Static("bottom"), RouteLeaf("bottom"), area);
                    ctx.render_component(ChildId::Static("top"), RouteLeaf("top"), area);
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 0, 0), &state),
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
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("removed"),
                        RecordingPointer {
                            name: "removed",
                            events: Arc::clone(&events),
                            capture: false,
                        },
                        Rect::new(0, 0, 4, 2),
                    );
                });
            })
            .expect("draw");
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("other"),
                        RecordingPointer {
                            name: "other",
                            events: Arc::clone(&events),
                            capture: false,
                        },
                        Rect::new(6, 0, 4, 2),
                    );
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state),
            EventResult::Ignored
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 1, 1), &state),
            EventResult::Ignored
        );
        assert!(events.lock().expect("pointer event log").is_empty());

        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 7, 1), &state);
        ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 7, 1), &state);
        assert_eq!(
            *events.lock().expect("pointer event log"),
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
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let render = |ratcn: &mut Ratcn<PointerState, PointerMsg>,
                      terminal: &mut Terminal<TestBackend>,
                      id,
                      name| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.render_component(
                            ChildId::Static(id),
                            RecordingPointer {
                                name,
                                events: Arc::clone(&events),
                                capture: false,
                            },
                            area,
                        );
                    });
                })
                .expect("draw");
        };

        render(&mut ratcn, &mut terminal, "before", "before");
        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
        render(&mut ratcn, &mut terminal, "after", "after");
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 1, 1), &state),
            EventResult::Consumed
        );
        assert_eq!(
            *events.lock().expect("pointer event log"),
            [
                ("before", MouseKind::Down(MouseButton::Left)),
                ("after", MouseKind::Up(MouseButton::Left)),
            ]
        );

        events.lock().expect("pointer event log").clear();
        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
        ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 1, 1), &state);
        assert_eq!(
            *events.lock().expect("pointer event log"),
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
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let render = |ratcn: &mut Ratcn<PointerState, PointerMsg>,
                      terminal: &mut Terminal<TestBackend>,
                      id,
                      name| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.render_component(
                            ChildId::Static(id),
                            RecordingPointer {
                                name,
                                events: Arc::clone(&events),
                                capture: true,
                            },
                            area,
                        );
                    });
                })
                .expect("draw");
        };

        render(&mut ratcn, &mut terminal, "stable", "before");
        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
        render(&mut ratcn, &mut terminal, "stable", "replacement");
        ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 1, 1), &state);

        assert_eq!(
            *events.lock().expect("pointer event log"),
            [
                ("before", MouseKind::Down(MouseButton::Left)),
                ("replacement", MouseKind::Up(MouseButton::Left)),
                ("replacement", MouseKind::Click(MouseButton::Left)),
            ]
        );

        events.lock().expect("pointer event log").clear();
        render(&mut ratcn, &mut terminal, "stable", "before");
        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
        render(&mut ratcn, &mut terminal, "different", "replacement");
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 1, 1), &state),
            EventResult::Consumed
        );
        assert_eq!(
            *events.lock().expect("pointer event log"),
            [("before", MouseKind::Down(MouseButton::Left))]
        );
    }

    /// Two neighbours with a gap between them, for the press-drift cases: a
    /// click has to survive movement inside one of them and has to die when
    /// the pointer leaves for the other.
    fn render_neighbours(
        ratcn: &mut Ratcn<PointerState, PointerMsg>,
        terminal: &mut Terminal<TestBackend>,
        events: &Arc<Mutex<Vec<(&'static str, MouseKind)>>>,
        capture: bool,
    ) {
        let state = PointerState;
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("left"),
                        RecordingPointer {
                            name: "left",
                            events: Arc::clone(events),
                            capture,
                        },
                        Rect::new(0, 0, 4, 2),
                    );
                    ctx.render_component(
                        ChildId::Static("right"),
                        RecordingPointer {
                            name: "right",
                            events: Arc::clone(events),
                            capture,
                        },
                        Rect::new(6, 0, 4, 2),
                    );
                });
            })
            .expect("draw");
    }

    #[test]
    fn a_press_that_drifts_inside_one_component_still_clicks_it() {
        // The pointer moved a column while held, which is enough to emit
        // `Drag` — but the release is still on the component the press hit and
        // nobody claimed the gesture, so the click stands. A cell-exact rule
        // would silently drop this press, which is the ordinary way a real
        // mouse behaves.
        let state = PointerState;
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        render_neighbours(&mut ratcn, &mut terminal, &events, false);

        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
        ratcn.handle_event(mouse(MouseKind::Moved, 2, 1), &state);
        ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 2, 1), &state);

        assert_eq!(
            *events.lock().expect("pointer event log"),
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
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        render_neighbours(&mut ratcn, &mut terminal, &events, false);

        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
        ratcn.handle_event(mouse(MouseKind::Moved, 7, 1), &state);
        ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 7, 1), &state);

        let log = events.lock().expect("pointer event log");
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
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        render_neighbours(&mut ratcn, &mut terminal, &events, false);

        // Column 5 is the gap between the two components.
        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 5, 1), &state);
        ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 1, 1), &state);

        let log = events.lock().expect("pointer event log");
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
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        render_neighbours(&mut ratcn, &mut terminal, &events, true);

        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
        ratcn.handle_event(mouse(MouseKind::Moved, 2, 1), &state);
        ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 2, 1), &state);

        assert_eq!(
            *events.lock().expect("pointer event log"),
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
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        render_neighbours(&mut ratcn, &mut terminal, &events, true);

        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
        ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 1, 1), &state);

        assert_eq!(
            *events.lock().expect("pointer event log"),
            [
                ("left", MouseKind::Down(MouseButton::Left)),
                ("left", MouseKind::Up(MouseButton::Left)),
                ("left", MouseKind::Click(MouseButton::Left)),
            ]
        );
    }

    #[test]
    fn area_scope_hit_prefers_descendant_then_falls_back_to_scope() {
        let state = HoverFocusState::default();
        let mut ratcn = Ratcn::<HoverFocusState, HoverFocusMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(8, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.scope(
                        ChildId::Static("scope"),
                        Rect::new(0, 0, 8, 2),
                        ScopeOptions::default(),
                        |ctx| {
                            ctx.render_component(
                                ChildId::Static("child"),
                                HoverFocusLeaf::enabled(),
                                Rect::new(0, 0, 3, 2),
                            );
                        },
                    );
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 1, 0), &state),
            EventResult::Consumed
        );
        assert_eq!(
            ratcn.hover_path(),
            [ChildId::Static("scope"), ChildId::Static("child")],
            "the descendant wins the enclosing scope hit"
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 6, 0), &state),
            EventResult::Consumed
        );
        assert_eq!(
            ratcn.hover_path(),
            [ChildId::Static("scope")],
            "off the child, the scope answers for its own area"
        );
    }

    #[test]
    fn focusable_decorative_scope_receives_mouse_focus_and_hover_context() {
        let rendered = Arc::new(Mutex::new(Vec::new()));
        let state = HoverFocusState {
            focus: FocusState::intent([ChildId::Static("other")]),
        };
        let mut ratcn =
            Ratcn::new().focus(|state: &HoverFocusState| &state.focus, HoverFocusMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(8, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let render = |ratcn: &mut Ratcn<HoverFocusState, HoverFocusMsg>,
                      terminal: &mut Terminal<TestBackend>,
                      state: &HoverFocusState| {
            terminal
                .draw(|frame| {
                    ratcn.render(frame, state, &theme, |ctx| {
                        ctx.render_component(
                            ChildId::Static("other"),
                            HoverFocusLeaf::enabled(),
                            Rect::new(0, 0, 2, 2),
                        );
                        let rendered = Arc::clone(&rendered);
                        ctx.scope(
                            ChildId::Static("decoration"),
                            Rect::new(3, 0, 5, 2),
                            ScopeOptions::default().focusable(),
                            move |ctx| {
                                ctx.paint(move |ctx| {
                                    rendered
                                        .lock()
                                        .expect("scope render log")
                                        .push((ctx.hovered, ctx.contains_hover));
                                });
                            },
                        );
                    });
                })
                .expect("draw");
        };

        render(&mut ratcn, &mut terminal, &state);
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 4, 0), &state),
            EventResult::Consumed,
            "the decorative scope is a hover target, and the frame is now stale"
        );
        render(&mut ratcn, &mut terminal, &state);
        // Frame one: not hovered. Frame two: the pointer rests on the scope,
        // and paint sees it.
        assert_eq!(
            *rendered.lock().expect("scope render log"),
            [(false, false), (true, true)]
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 4, 0), &state),
            EventResult::Emit(HoverFocusMsg::Focus(FocusState::intent([ChildId::Static(
                "decoration"
            )])))
        );
    }

    /// A focusable leaf that queues a paint thunk from its own declaration, so
    /// a test can see which node a thunk's flags are read from.
    struct ThunkProbe(FocusRenderLog);

    impl Component<FocusTestState, FocusTestMsg> for ThunkProbe {
        fn render(&mut self, ctx: &mut RenderCtx<'_, FocusTestState, FocusTestMsg>) {
            let log = Arc::clone(&self.0);
            ctx.paint(move |ctx| {
                log.lock()
                    .expect("thunk flag log")
                    .push((ctx.focused, ctx.contains_focus));
            });
        }

        fn is_focusable(&self, _state: &FocusTestState) -> bool {
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
        let scope_flags = Arc::new(Mutex::new(Vec::new()));
        let leaf_flags = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(6, 1)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    let scope_flags = Arc::clone(&scope_flags);
                    let leaf_flags = Arc::clone(&leaf_flags);
                    ctx.scope(
                        ChildId::Static("pane"),
                        area,
                        ScopeOptions::default(),
                        move |ctx| {
                            ctx.paint(move |ctx| {
                                scope_flags
                                    .lock()
                                    .expect("scope flag log")
                                    .push((ctx.focused, ctx.contains_focus));
                            });
                            ctx.render_component(
                                ChildId::Static("leaf"),
                                ThunkProbe(leaf_flags),
                                area,
                            );
                        },
                    );
                });
            })
            .expect("draw");

        assert_eq!(
            *scope_flags.lock().expect("scope flag log"),
            [(false, true)],
            "the scope contains focus without being the focused leaf"
        );
        assert_eq!(
            *leaf_flags.lock().expect("thunk flag log"),
            [(true, true)],
            "the leaf's own thunk is read from the leaf"
        );
    }

    #[test]
    fn root_then_nested_hover_focus_attract_on_successive_moves() {
        let mut state = HoverFocusState {
            focus: FocusState::intent([ChildId::Static("left"), ChildId::Static("first")]),
        };
        let mut ratcn = Ratcn::new()
            .focus(|state: &HoverFocusState| &state.focus, HoverFocusMsg::Focus)
            .hover_focus();
        let mut terminal = Terminal::new(TestBackend::new(12, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    for (scope, x) in [("left", 0), ("right", 6)] {
                        ctx.scope(
                            ChildId::Static(scope),
                            Rect::new(x, 0, 6, 2),
                            ScopeOptions::default().hover_focus(),
                            |ctx| {
                                ctx.render_component(
                                    ChildId::Static("first"),
                                    HoverFocusLeaf::enabled(),
                                    Rect::new(x, 0, 3, 2),
                                );
                                ctx.render_component(
                                    ChildId::Static("second"),
                                    HoverFocusLeaf::enabled(),
                                    Rect::new(x + 3, 0, 3, 2),
                                );
                            },
                        );
                    }
                });
            })
            .expect("draw");

        let EventResult::Emit(HoverFocusMsg::Focus(root_focus)) =
            ratcn.handle_event(mouse(MouseKind::Moved, 10, 0), &state)
        else {
            panic!("root boundary did not attract focus");
        };
        assert_eq!(
            root_focus.path(),
            &[ChildId::Static("right"), ChildId::Static("first")]
        );
        // The same motion did both: hover is the runtime's own, so it lands
        // whole while the one message the event may carry is the focus change.
        assert_eq!(
            ratcn.hover_path(),
            [ChildId::Static("right"), ChildId::Static("second")]
        );
        state.focus = root_focus;

        let EventResult::Emit(HoverFocusMsg::Focus(nested_focus)) =
            ratcn.handle_event(mouse(MouseKind::Moved, 10, 0), &state)
        else {
            panic!("nested boundary did not attract focus after the root");
        };
        assert_eq!(
            nested_focus.path(),
            &[ChildId::Static("right"), ChildId::Static("second")]
        );
        state.focus = nested_focus;

        // Both boundaries satisfied and hover already there: the motion still
        // reports the redraw signal, but it asks for nothing.
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 10, 0), &state),
            EventResult::Consumed
        );
    }

    #[test]
    fn hover_focus_is_off_by_default_and_skips_disabled_targets_and_empty_space() {
        let state = HoverFocusState {
            focus: FocusState::intent([ChildId::Static("enabled")]),
        };
        let mut default =
            Ratcn::new().focus(|state: &HoverFocusState| &state.focus, HoverFocusMsg::Focus);
        let mut hover_focus = Ratcn::new()
            .focus(|state: &HoverFocusState| &state.focus, HoverFocusMsg::Focus)
            .hover_focus();
        let mut terminal = Terminal::new(TestBackend::new(12, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let render = |ratcn: &mut Ratcn<HoverFocusState, HoverFocusMsg>,
                      terminal: &mut Terminal<TestBackend>| {
            terminal
                .draw(|frame| {
                    ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.render_component(
                            ChildId::Static("enabled"),
                            HoverFocusLeaf::enabled(),
                            Rect::new(0, 0, 3, 2),
                        );
                        ctx.render_component(
                            ChildId::Static("disabled"),
                            HoverFocusLeaf::disabled(),
                            Rect::new(4, 0, 3, 2),
                        );
                    });
                })
                .expect("draw");
        };

        render(&mut default, &mut terminal);
        assert_eq!(
            default.handle_event(mouse(MouseKind::Moved, 5, 0), &state),
            EventResult::Consumed,
            "without `hover_focus` a motion moves nothing but hover"
        );
        assert_eq!(default.hover_path(), [ChildId::Static("disabled")]);

        render(&mut hover_focus, &mut terminal);
        assert_eq!(
            hover_focus.handle_event(mouse(MouseKind::Moved, 5, 0), &state),
            EventResult::Consumed,
            "a disabled target hovers without attracting focus"
        );
        assert_eq!(hover_focus.hover_path(), [ChildId::Static("disabled")]);
        assert_eq!(
            hover_focus.handle_event(mouse(MouseKind::Moved, 10, 0), &state),
            EventResult::Consumed,
            "empty space attracts no focus either"
        );
        assert!(
            hover_focus.hover_path().is_empty(),
            "and hover returns to nothing over empty space"
        );
    }

    #[test]
    fn focus_path_validates_latest_surface_focusability_and_scope_descent() {
        let state = HoverFocusState::default();
        let dynamic = ChildId::Dynamic(Arc::from("dynamic"));
        let mut ratcn = Ratcn::<HoverFocusState, HoverFocusMsg>::new();
        assert!(ratcn.focus_path(&[ChildId::Static("pane")]).is_none());
        let mut terminal = Terminal::new(TestBackend::new(8, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.scope(
                        ChildId::Static("pane"),
                        Rect::ZERO,
                        ScopeOptions::default(),
                        |ctx| {
                            ctx.render_component(
                                ChildId::Static("disabled"),
                                HoverFocusLeaf::disabled(),
                                area,
                            );
                            ctx.render_component(
                                ChildId::Static("enabled"),
                                HoverFocusLeaf::enabled(),
                                area,
                            );
                        },
                    );
                    ctx.render_component(dynamic.clone(), HoverFocusLeaf::enabled(), area);
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.focus_path(&[ChildId::Static("pane")]),
            Some(FocusState::intent([
                ChildId::Static("pane"),
                ChildId::Static("enabled")
            ]))
        );
        assert!(
            ratcn
                .focus_path(&[ChildId::Static("pane"), ChildId::Static("disabled")])
                .is_none()
        );
        assert_eq!(
            ratcn.focus_path(std::slice::from_ref(&dynamic)),
            Some(FocusState::intent([dynamic.clone()]))
        );
        assert!(ratcn.focus_path(&[ChildId::Static("missing")]).is_none());

        terminal
            .draw(|frame| ratcn.render(frame, &state, &theme, |_| {}))
            .expect("draw");
        assert!(ratcn.focus_path(std::slice::from_ref(&dynamic)).is_none());
    }

    #[test]
    fn collapsed_components_are_excluded_and_recover_when_geometry_reappears() {
        let mut state = FocusTestState {
            focus: FocusState::intent([ChildId::Static("width-zero")]),
        };
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(8, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let render = |ratcn: &mut Ratcn<FocusTestState, FocusTestMsg>,
                      terminal: &mut Terminal<TestBackend>,
                      recovered| {
            terminal
                .draw(|frame| {
                    ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.render_component(
                            ChildId::Static("width-zero"),
                            FocusLeaf::enabled(),
                            if recovered {
                                Rect::new(0, 0, 1, 1)
                            } else {
                                Rect::new(0, 0, 0, 1)
                            },
                        );
                        ctx.render_component(
                            ChildId::Static("height-zero"),
                            FocusLeaf::enabled(),
                            Rect::new(1, 0, 1, 0),
                        );
                        ctx.render_component(
                            ChildId::Static("visible"),
                            FocusLeaf::enabled(),
                            Rect::new(2, 0, 1, 1),
                        );
                        ctx.scope(
                            ChildId::Static("group"),
                            Rect::ZERO,
                            ScopeOptions::default(),
                            |ctx| {
                                ctx.render_component(
                                    ChildId::Static("child"),
                                    FocusLeaf::enabled(),
                                    Rect::new(4, 0, 1, 1),
                                );
                            },
                        );
                    });
                })
                .expect("draw");
        };

        render(&mut ratcn, &mut terminal, false);
        for id in ["width-zero", "height-zero"] {
            assert!(ratcn.focus_path(&[ChildId::Static(id)]).is_none());
        }
        assert_eq!(
            ratcn.focus_path(&[ChildId::Static("group")]),
            Some(FocusState::intent([
                ChildId::Static("group"),
                ChildId::Static("child")
            ]))
        );
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Ignored,
            "a parked collapsed target must not activate"
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 0, 0), &state),
            EventResult::Ignored,
            "collapsed geometry must not be a mouse target"
        );
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
            EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([ChildId::Static(
                "visible"
            )])))
        );

        render(&mut ratcn, &mut terminal, true);
        assert_eq!(
            ratcn.focus_path(&[ChildId::Static("width-zero")]),
            Some(FocusState::intent([ChildId::Static("width-zero")]))
        );
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(FocusTestMsg::Activated(vec![ChildId::Static("width-zero")]))
        );
        state.focus = FocusState::intent([ChildId::Static("visible")]);
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 0, 0), &state),
            EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([ChildId::Static(
                "width-zero"
            )])))
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

        fn render(&mut self, _ctx: &mut RenderCtx<'_, bool, ()>) {}

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
        let mut ratcn = Ratcn::<bool, ()>::new();
        let mut terminal = Terminal::new(TestBackend::new(8, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let area = Rect::new(0, 0, 8, 2);
        terminal
            .draw(|frame| {
                ratcn.render(frame, &true, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("claims"),
                        PreparedClaims { open: false },
                        area,
                    );
                });
            })
            .expect("draw");

        assert!(
            ratcn.focus_path(&[ChildId::Static("claims")]).is_some(),
            "scope_options was read after prepare"
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 7, 1), &true),
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
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(8, 2)).expect("terminal");
        let theme = Theme::default_dark();

        for (width, usable) in [(1, false), (2, true)] {
            let area = Rect::new(0, 0, width, 1);
            rendered.store(false, Ordering::SeqCst);
            terminal
                .draw(|frame| {
                    ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.render_component(
                            ChildId::Static("area-aware"),
                            AreaAwareComposite {
                                expected_area: area,
                                minimum_width: 2,
                                rendered: Arc::clone(&rendered),
                            },
                            area,
                        );
                        ctx.render_component(
                            ChildId::Static("visible"),
                            FocusLeaf::enabled(),
                            Rect::new(4, 0, 2, 1),
                        );
                    });
                })
                .expect("draw");
            assert!(rendered.load(Ordering::SeqCst));
            assert_eq!(
                ratcn.focus_path(&[ChildId::Static("area-aware")]).is_some(),
                usable
            );
            let enter = ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state);
            assert_eq!(
                matches!(enter, EventResult::Emit(FocusTestMsg::Activated(_))),
                usable
            );
            let down = ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 0, 0), &state);
            assert_eq!(!matches!(down, EventResult::Ignored), usable);
            if !usable {
                assert_eq!(
                    ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
                    EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([ChildId::Static(
                        "visible"
                    )])))
                );
            }
        }

        assert_eq!(
            ratcn.declared_paths(),
            vec![
                vec![ChildId::Static("area-aware")],
                vec![ChildId::Static("area-aware"), ChildId::Static("child")],
                vec![ChildId::Static("visible")],
            ]
        );
    }

    #[test]
    fn zero_area_focusable_scope_groups_descendants_but_cannot_hold_focus_itself() {
        let state = FocusTestState::default();
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(8, 2)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.scope(
                        ChildId::Static("empty"),
                        Rect::ZERO,
                        ScopeOptions::default().focusable(),
                        |_| {},
                    );
                    ctx.render_component(ChildId::Static("visible"), FocusLeaf::enabled(), area);
                });
            })
            .expect("draw");

        assert!(ratcn.focus_path(&[ChildId::Static("empty")]).is_none());
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(FocusTestMsg::Activated(vec![ChildId::Static("visible")]))
        );
    }

    #[test]
    fn focus_path_rejects_inactive_layers_and_descends_in_active_modal() {
        let state = HoverFocusState::default();
        let mut ratcn = Ratcn::<HoverFocusState, HoverFocusMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(8, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(ChildId::Static("base"), HoverFocusLeaf::enabled(), area);
                    ctx.modal(ChildId::Static("lower"), HoverFocusComposite, area);
                    ctx.modal(ChildId::Static("top"), HoverFocusComposite, area);
                });
            })
            .expect("draw");

        for inactive in [ChildId::Static("base"), ChildId::Static("lower")] {
            assert!(ratcn.focus_path(&[inactive]).is_none());
        }
        assert_eq!(
            ratcn.focus_path(&[ChildId::Static("top")]),
            Some(FocusState::intent([
                ChildId::Static("top"),
                ChildId::Static("leaf")
            ]))
        );
    }

    #[test]
    fn raw_button_press_focuses_then_synthesized_click_emits() {
        let mut state = ButtonTimingState {
            focus: FocusState::intent([ChildId::Static("first")]),
            ..ButtonTimingState::default()
        };
        let mut ratcn = Ratcn::new().focus(
            |state: &ButtonTimingState| &state.focus,
            ButtonTimingMsg::Focus,
        );
        let mut terminal = Terminal::new(TestBackend::new(20, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("first"),
                        Button::new("First").on_press(|| ButtonTimingMsg::Replacement),
                        Rect::new(0, 0, 8, 2),
                    );
                    ctx.render_component(
                        ChildId::Static("second"),
                        Button::new("Second").on_press(|| ButtonTimingMsg::Save),
                        Rect::new(10, 0, 8, 2),
                    );
                });
            })
            .expect("draw");

        for button in [MouseButton::Right, MouseButton::Middle] {
            assert_eq!(
                ratcn.handle_event(mouse(MouseKind::Down(button), 11, 0), &state),
                EventResult::Ignored
            );
            assert_eq!(
                ratcn.handle_event(mouse(MouseKind::Up(button), 11, 0), &state),
                EventResult::Ignored
            );
            assert_eq!(state.focus.path(), &[ChildId::Static("first")]);
        }

        let EventResult::Emit(focus) =
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 11, 0), &state)
        else {
            panic!("button down did not request focus");
        };
        assert!(update_button_timing(&mut state, focus));
        assert_eq!(state.focus.path(), &[ChildId::Static("second")]);
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 11, 0), &state),
            EventResult::Consumed
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 11, 0), &state),
            EventResult::Emit(ButtonTimingMsg::Save)
        );
    }

    #[test]
    fn primary_down_result_controls_focus_fallback_after_capture_and_routing() {
        let state = FocusTestState {
            focus: FocusState::intent([ChildId::Static("first")]),
        };
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(16, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("first"),
                        FocusLeaf::enabled(),
                        Rect::new(0, 0, 3, 1),
                    );
                    for (id, x, behavior) in [
                        ("ignored", 4, DownBehavior::CaptureAndIgnore),
                        ("consumed", 8, DownBehavior::Consume),
                        ("emitted", 12, DownBehavior::Emit),
                    ] {
                        ctx.render_component(
                            ChildId::Static(id),
                            DownFocusLeaf(behavior),
                            Rect::new(x, 0, 3, 1),
                        );
                    }
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 4, 0), &state),
            EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([ChildId::Static(
                "ignored"
            )])))
        );
        assert_eq!(
            ratcn.capture_path(MouseButton::Left),
            Some([ChildId::Static("ignored")].as_slice())
        );
        ratcn.handle_event(mouse(MouseKind::Exited, 4, 0), &state);
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 8, 0), &state),
            EventResult::Consumed
        );
        ratcn.handle_event(mouse(MouseKind::Exited, 8, 0), &state);
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 12, 0), &state),
            EventResult::Emit(FocusTestMsg::Activated(vec![ChildId::Static("emitted")]))
        );
    }

    #[test]
    fn click_focused_component_ignores_primary_down_and_focuses_on_click() {
        let state = FocusTestState {
            focus: FocusState::intent([ChildId::Static("first")]),
        };
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(8, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("first"),
                        FocusLeaf::enabled(),
                        Rect::new(0, 0, 3, 2),
                    );
                    ctx.render_component(
                        ChildId::Static("click"),
                        ClickFocusLeaf,
                        Rect::new(4, 0, 3, 2),
                    );
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 5, 0), &state),
            EventResult::Ignored
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 5, 0), &state),
            EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([ChildId::Static(
                "click"
            )])))
        );
    }

    /// The motion contract: with a surface to route against, a motion is
    /// never `Ignored`. Crossing into a component, drifting within one, and
    /// leaving for empty space all report `Consumed` — the host redraws on
    /// anything but `Ignored`, and every motion is news to a frame that may
    /// paint from the pointer position itself.
    #[test]
    fn hover_crosses_consumes_same_target_and_clears_over_empty_space() {
        let state = PointerState;
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    for (id, x) in [("left", 0), ("right", 5)] {
                        ctx.render_component(
                            ChildId::Static(id),
                            HoverLeaf {
                                consume_move: false,
                                rendered: None,
                            },
                            Rect::new(x, 0, 4, 2),
                        );
                    }
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 1, 0), &state),
            EventResult::Consumed,
            "the first crossing moved hover"
        );
        assert_eq!(
            ratcn.hover_path(),
            [ChildId::Static("left")],
            "the crossing put hover on the target it entered"
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 2, 1), &state),
            EventResult::Consumed,
            "motion within one target moves no hover, and is still the redraw signal"
        );
        assert_eq!(
            ratcn.hover_path(),
            [ChildId::Static("left")],
            "drifting inside `left` leaves hover where it was"
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 6, 0), &state),
            EventResult::Consumed,
            "crossing to the other target"
        );
        assert_eq!(ratcn.hover_path(), [ChildId::Static("right")]);
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 4, 0), &state),
            EventResult::Consumed,
            "leaving for empty space is a change like any other"
        );
        assert!(
            ratcn.hover_path().is_empty(),
            "over empty space the pointer is on nothing"
        );
    }

    /// `pointer_within` answers for the declaration that asks, not for the
    /// pointer in general: true on what the pointer is on and on everything
    /// enclosing it, false on a sibling. The root closure has no declaration
    /// of its own, so it asks whether anything is hovered at all.
    #[test]
    fn pointer_within_answers_for_the_asking_declaration() {
        let state = PointerState;
        let log: Arc<Mutex<Vec<(&'static str, bool)>>> = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let mut render = |ratcn: &mut Ratcn<PointerState, PointerMsg>| {
            terminal
                .draw(|frame| {
                    ratcn.render(frame, &state, &theme, |ctx| {
                        log.lock()
                            .expect("log")
                            .push(("root", ctx.pointer_within()));
                        for (id, x) in [("left", 0), ("right", 5)] {
                            let log = Arc::clone(&log);
                            let area = Rect::new(x, 0, 5, 2);
                            ctx.scope(
                                ChildId::Static(id),
                                area,
                                ScopeOptions::default(),
                                move |ctx| {
                                    log.lock().expect("log").push((id, ctx.pointer_within()));
                                    ctx.render_component(
                                        ChildId::Static("leaf"),
                                        HoverLeaf {
                                            consume_move: false,
                                            rendered: None,
                                        },
                                        area,
                                    );
                                },
                            );
                        }
                    });
                })
                .expect("draw");
        };

        render(&mut ratcn);
        assert_eq!(
            *log.lock().expect("log"),
            [("root", false), ("left", false), ("right", false)],
            "nothing is hovered before the pointer has been anywhere"
        );

        ratcn.handle_event(mouse(MouseKind::Moved, 1, 0), &state);
        log.lock().expect("log").clear();
        render(&mut ratcn);
        assert_eq!(
            *log.lock().expect("log"),
            [("root", true), ("left", true), ("right", false)],
            "the pointer is on `left`'s leaf, so only that subtree contains it"
        );
    }

    /// Hover freezes for the length of a claimed gesture. A component that
    /// captured the pointer owns it, and the geometry it drags moves under a
    /// pointer that is by definition on it, so neither the drag events nor the
    /// frames they produce may retarget hover. The release hands it back.
    #[test]
    fn a_captured_gesture_freezes_hover_until_it_ends() {
        let state = PointerState;
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).expect("terminal");
        let surface = [
            ("left", "left", Rect::new(0, 0, 4, 2)),
            ("right", "right", Rect::new(10, 0, 4, 2)),
        ];

        render_drag_surface(&mut ratcn, &mut terminal, &state, &surface);
        ratcn.handle_event(mouse(MouseKind::Moved, 1, 1), &state);
        assert_eq!(ratcn.hover_path(), [ChildId::Static("left")]);

        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
        assert!(ratcn.capture_path(MouseButton::Left).is_some());
        ratcn.handle_event(mouse(MouseKind::Moved, 11, 1), &state);
        assert_eq!(
            ratcn.hover_path(),
            [ChildId::Static("left")],
            "the drag belongs to the component that claimed it"
        );
        render_drag_surface(&mut ratcn, &mut terminal, &state, &surface);
        assert_eq!(
            ratcn.hover_path(),
            [ChildId::Static("left")],
            "and a redraw mid-gesture does not retarget it either"
        );

        ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 11, 1), &state);
        render_drag_surface(&mut ratcn, &mut terminal, &state, &surface);
        assert_eq!(
            ratcn.hover_path(),
            [ChildId::Static("right")],
            "the release ends the claim, and the next frame answers for the pointer again"
        );
    }

    /// The freeze is one rule for every gesture, not just claimed ones. A
    /// press that captured nothing still owns the pointer until it is
    /// released — motion under a held button normalizes to `Drag` and never
    /// writes hover — so a redraw that moves geometry out from under that
    /// pointer must not retarget hover either. The release hands it back.
    #[test]
    fn a_held_press_freezes_hover_against_a_moving_redraw() {
        let state = PointerState;
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let render = |ratcn: &mut Ratcn<PointerState, PointerMsg>,
                      terminal: &mut Terminal<TestBackend>,
                      x| {
            terminal
                .draw(|frame| {
                    ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.render_component(
                            ChildId::Static("target"),
                            HoverLeaf {
                                consume_move: false,
                                rendered: None,
                            },
                            Rect::new(x, 0, 2, 1),
                        );
                    });
                })
                .expect("draw");
        };

        render(&mut ratcn, &mut terminal, 0);
        ratcn.handle_event(mouse(MouseKind::Moved, 1, 0), &state);
        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state);
        assert!(
            !ratcn.any_capture(),
            "nothing claimed this gesture — the freeze is not about captures"
        );

        render(&mut ratcn, &mut terminal, 5);
        assert_eq!(
            ratcn.hover_path(),
            [ChildId::Static("target")],
            "the press owns the pointer, so the redraw does not retarget hover"
        );

        ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 1, 0), &state);
        render(&mut ratcn, &mut terminal, 5);
        assert!(
            ratcn.hover_path().is_empty(),
            "the release ends the gesture, and the pointer is over empty space"
        );
    }

    /// A freeze lasts only as long as its target could still be under the
    /// pointer. The frame that opens a modal cancels the gesture beneath it,
    /// and that same frame must paint the captured node unhovered — a
    /// gesture whose target is covered has no claim on hover left.
    #[test]
    fn a_frame_that_cancels_a_gesture_unhovers_its_target() {
        let rendered = Arc::new(Mutex::new(Vec::new()));
        let state = PointerState;
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let mut render = |ratcn: &mut Ratcn<PointerState, PointerMsg>, modal| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.render_component(
                            ChildId::Static("grip"),
                            CapturingHoverLeaf {
                                rendered: Arc::clone(&rendered),
                            },
                            Rect::new(0, 0, 4, 2),
                        );
                        if modal {
                            ctx.modal(ChildId::Static("modal"), RouteLeaf("modal"), area);
                        }
                    });
                })
                .expect("draw");
        };

        render(&mut ratcn, false);
        ratcn.handle_event(mouse(MouseKind::Moved, 1, 0), &state);
        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state);
        assert!(ratcn.capture_path(MouseButton::Left).is_some());
        ratcn.handle_event(mouse(MouseKind::Drag(MouseButton::Left), 8, 1), &state);
        render(&mut ratcn, false);
        assert_eq!(
            rendered.lock().expect("capturing hover log").last(),
            Some(&(true, true)),
            "the drag keeps hover on the node that claimed it"
        );

        render(&mut ratcn, true);
        assert_eq!(
            rendered.lock().expect("capturing hover log").last(),
            Some(&(false, false)),
            "the modal cancels the gesture and covers its target, on this frame"
        );
        assert_ne!(ratcn.hover_path(), [ChildId::Static("grip")]);
    }

    #[test]
    fn pointer_exit_clears_hover() {
        let state = PointerState;
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(4, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("target"),
                        HoverLeaf {
                            consume_move: false,
                            rendered: None,
                        },
                        area,
                    );
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 1, 0), &state),
            EventResult::Consumed
        );
        assert_eq!(ratcn.hover_path(), [ChildId::Static("target")]);
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Exited, 1, 0), &state),
            EventResult::Consumed
        );
        assert!(
            ratcn.hover_path().is_empty(),
            "a pointer that left the grid is on nothing"
        );
    }

    /// An exit that arrives while the modal stacks disagree still empties
    /// hover, and no later commit brings it back: the pointer is gone, so
    /// there is nothing for the recompute to find it on.
    #[test]
    fn pointer_exit_during_modal_mismatch_keeps_hover_empty() {
        let rendered = Arc::new(Mutex::new(Vec::new()));
        let mut state = ModalPointerState::default();
        let mut ratcn = Ratcn::new().modals(|state: &ModalPointerState| &state.modals);
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let render = |ratcn: &mut Ratcn<ModalPointerState, PointerMsg>,
                      terminal: &mut Terminal<TestBackend>,
                      state: &ModalPointerState| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, state, &theme, |ctx| {
                        ctx.render_component(
                            ChildId::Static("base"),
                            ModalHoverLeaf {
                                rendered: Arc::clone(&rendered),
                            },
                            area,
                        );
                        if state.modals.is_open("modal") {
                            ctx.modal(ChildId::Static("modal"), ModalPointerLeaf, area);
                        }
                    });
                })
                .expect("draw");
        };

        render(&mut ratcn, &mut terminal, &state);
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 1, 0), &state),
            EventResult::Consumed
        );
        render(&mut ratcn, &mut terminal, &state);
        assert_eq!(
            rendered.lock().expect("hover render log").last(),
            Some(&(true, true)),
            "the pointer is on the base"
        );

        state
            .modals
            .open("modal", &mut state.focus)
            .expect("open modal");
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Exited, 1, 0), &state),
            EventResult::Consumed
        );
        render(&mut ratcn, &mut terminal, &state);
        let _ = state.modals.close(&mut state.focus);
        render(&mut ratcn, &mut terminal, &state);
        render(&mut ratcn, &mut terminal, &state);
        assert_eq!(
            rendered.lock().expect("hover render log").last(),
            Some(&(false, false)),
            "the pointer left, and closing the modal does not put it back"
        );

        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 1, 0), &state),
            EventResult::Consumed
        );
        render(&mut ratcn, &mut terminal, &state);
        assert_eq!(
            rendered.lock().expect("hover render log").last(),
            Some(&(true, true)),
            "the pointer coming back is what restores hover"
        );
    }

    /// A redraw that slides the target out from under a pointer that never
    /// moved: the commit re-answers the hit test, so the very next frame
    /// paints unhovered without any event being involved.
    #[test]
    fn redraw_moves_hover_when_the_target_moves_away_from_the_pointer() {
        let rendered = Arc::new(Mutex::new(Vec::new()));
        let state = PointerState;
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let render = |ratcn: &mut Ratcn<PointerState, PointerMsg>,
                      terminal: &mut Terminal<TestBackend>,
                      state: &PointerState,
                      x| {
            terminal
                .draw(|frame| {
                    ratcn.render(frame, state, &theme, |ctx| {
                        ctx.render_component(
                            ChildId::Static("target"),
                            HoverLeaf {
                                consume_move: false,
                                rendered: Some(Arc::clone(&rendered)),
                            },
                            Rect::new(x, 0, 2, 1),
                        );
                    });
                })
                .expect("draw");
        };

        render(&mut ratcn, &mut terminal, &state, 0);
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 1, 0), &state),
            EventResult::Consumed,
            "entering the target"
        );
        render(&mut ratcn, &mut terminal, &state, 0);
        render(&mut ratcn, &mut terminal, &state, 5);

        assert_eq!(
            *rendered.lock().expect("hover render log"),
            [(false, false), (true, true), (false, false)],
            "the frame that moves the target is already the frame that unhovers it"
        );
        assert!(
            ratcn.hover_path().is_empty(),
            "the correction needed no motion"
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 1, 0), &state),
            EventResult::Consumed,
            "a later motion is still the redraw signal, with nothing left to correct"
        );
        assert!(ratcn.hover_path().is_empty());
    }

    /// The hover change a crossing causes does not swallow the motion: the
    /// same event moves hover *and* goes on to the component under it.
    #[test]
    fn crossing_motion_moves_hover_and_still_reaches_a_consuming_component() {
        let state = PointerState;
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("consumer"),
                        HoverLeaf {
                            consume_move: true,
                            rendered: None,
                        },
                        area,
                    );
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 0, 0), &state),
            EventResult::Consumed,
            "the crossing motion reached the consuming component"
        );
        assert_eq!(
            ratcn.hover_path(),
            [ChildId::Static("consumer")],
            "the crossing motion moved hover"
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 1, 0), &state),
            EventResult::Consumed,
            "same-path motion must reach the consuming component"
        );
    }

    #[test]
    fn crossing_motion_moves_hover_and_still_reaches_an_emitting_component() {
        let state = PointerState;
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.scope(
                        ChildId::Static("panel"),
                        area,
                        ScopeOptions::default(),
                        |ctx| {
                            ctx.render_component(
                                ChildId::Static("emitter"),
                                EmittingHoverLeaf,
                                area,
                            );
                        },
                    );
                });
            })
            .expect("draw");

        // The message the component returns is the event's answer; hover
        // landing alongside it costs nothing, because it needs no message.
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 0, 0), &state),
            EventResult::Emit(PointerMsg::Routed("move", MouseKind::Moved, 1))
        );
        assert_eq!(
            ratcn.hover_path(),
            [ChildId::Static("panel"), ChildId::Static("emitter")]
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 1, 0), &state),
            EventResult::Emit(PointerMsg::Routed("move", MouseKind::Moved, 1))
        );
    }

    /// What a redraw does to hover with the pointer sitting still: a failed
    /// pass changes nothing (the previous surface is still what the pointer is
    /// on), a pass that drops the target unhovers, and one that declares it
    /// again under the same pointer hovers it again — all without an event.
    #[test]
    fn removed_target_unhovers_and_a_redeclared_one_hovers_again_under_the_pointer() {
        let rendered = Arc::new(Mutex::new(Vec::new()));
        let state = PointerState;
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let render_target = |ratcn: &mut Ratcn<PointerState, PointerMsg>,
                             terminal: &mut Terminal<TestBackend>| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.render_component(
                            ChildId::Static("target"),
                            HoverLeaf {
                                consume_move: false,
                                rendered: Some(Arc::clone(&rendered)),
                            },
                            area,
                        );
                    });
                })
                .expect("draw");
        };

        render_target(&mut ratcn, &mut terminal);
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 4, 1), &state),
            EventResult::Consumed
        );
        render_target(&mut ratcn, &mut terminal);

        let failed_removal = catch_unwind(AssertUnwindSafe(|| {
            terminal
                .draw(|frame| {
                    ratcn.render(frame, &state, &theme, |_| panic!("failed removal"));
                })
                .expect("draw");
        }));
        assert!(failed_removal.is_err());
        assert_eq!(
            ratcn.hover_path(),
            [ChildId::Static("target")],
            "a rejected pass leaves the surface the pointer is on in place"
        );
        render_target(&mut ratcn, &mut terminal);

        terminal
            .draw(|frame| ratcn.render(frame, &state, &theme, |_| {}))
            .expect("draw");
        assert!(
            ratcn.hover_path().is_empty(),
            "the target is gone, so the pointer is on nothing"
        );
        render_target(&mut ratcn, &mut terminal);

        // One entry per successful render; the failed pass painted nothing and
        // so recorded nothing. The last is the redeclared target, hovered
        // again by the commit that declared it — no event was involved.
        assert_eq!(
            *rendered.lock().expect("hover render log"),
            vec![(false, false), (true, true), (true, true), (true, true)]
        );
        assert_eq!(ratcn.hover_path(), [ChildId::Static("target")]);
    }

    /// Removal settles at the commit, so no later event carries a correction:
    /// the first motion after it has nothing to change and says so.
    #[test]
    fn motion_after_a_removal_has_nothing_left_to_correct() {
        let state = PointerState;
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();

        let render_target = |ratcn: &mut Ratcn<PointerState, PointerMsg>,
                             terminal: &mut Terminal<TestBackend>,
                             state: &PointerState| {
            terminal
                .draw(|frame| {
                    ratcn.render(frame, state, &theme, |ctx| {
                        ctx.render_component(
                            ChildId::Static("target"),
                            HoverLeaf {
                                consume_move: false,
                                rendered: None,
                            },
                            Rect::new(0, 0, 2, 1),
                        );
                    });
                })
                .expect("draw");
        };

        render_target(&mut ratcn, &mut terminal, &state);
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 1, 0), &state),
            EventResult::Consumed,
            "entering the target"
        );
        terminal
            .draw(|frame| ratcn.render(frame, &state, &theme, |_| {}))
            .expect("draw");
        assert!(
            ratcn.hover_path().is_empty(),
            "the removal emptied hover at the commit"
        );

        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 4, 1), &state),
            EventResult::Consumed,
            "the motion is the redraw signal, but it carries no correction"
        );
        render_target(&mut ratcn, &mut terminal, &state);
        assert!(
            ratcn.hover_path().is_empty(),
            "and the pointer is no longer over the redeclared target"
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 1, 0), &state),
            EventResult::Consumed
        );
        assert_eq!(ratcn.hover_path(), [ChildId::Static("target")]);
    }

    #[test]
    fn mouse_before_the_first_render_is_ignored_without_arming_the_tracker() {
        let state = PointerState;
        let mut ratcn = Ratcn::new();
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 0, 0), &state),
            EventResult::Ignored
        );
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        render_drag_surface(
            &mut ratcn,
            &mut terminal,
            &state,
            &[("drag", "drag", Rect::new(0, 0, 5, 2))],
        );
        // Plain motion, not a drag: the press before the first render armed
        // nothing, so this only moves hover, which is what `Consumed` reports.
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 0, 0), &state),
            EventResult::Consumed
        );
    }

    struct LoggingComponent {
        name: &'static str,
        log: Arc<Mutex<Vec<&'static str>>>,
        focusable: bool,
    }

    impl Component<FocusTestState, FocusTestMsg> for LoggingComponent {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, FocusTestState, FocusTestMsg>) {}

        fn paint(&mut self, _ctx: &mut PaintCtx<'_, '_, FocusTestState>) {
            self.log.lock().expect("paint log").push(self.name);
        }

        fn handle_event(
            &mut self,
            event: &Event,
            _state: &FocusTestState,
            ctx: &mut EventCtx<'_>,
        ) -> EventResult<FocusTestMsg> {
            if matches!(event, Event::Key(_)) {
                EventResult::Emit(FocusTestMsg::Activated(ctx.path().to_vec()))
            } else {
                EventResult::Ignored
            }
        }

        fn is_focusable(&self, _state: &FocusTestState) -> bool {
            self.focusable
        }
    }

    struct FocusModal;

    impl Component<FocusTestState, FocusTestMsg> for FocusModal {
        fn render(&mut self, ctx: &mut RenderCtx<'_, FocusTestState, FocusTestMsg>) {
            let area = ctx.area();
            ctx.render_component(ChildId::Static("leaf"), FocusLeaf::enabled(), area);
        }

        fn scope_options(&self) -> ScopeOptions {
            ScopeOptions::default().tab_wrap(TabWrap::Wrap)
        }
    }

    struct EscapeFocusModal;

    impl Component<FocusTestState, FocusTestMsg> for EscapeFocusModal {
        fn render(&mut self, ctx: &mut RenderCtx<'_, FocusTestState, FocusTestMsg>) {
            let area = ctx.area();
            ctx.render_component(ChildId::Static("first"), FocusLeaf::enabled(), area);
            ctx.render_component(ChildId::Static("second"), FocusLeaf::enabled(), area);
        }

        fn scope_options(&self) -> ScopeOptions {
            ScopeOptions::default().tab_wrap(TabWrap::Escape)
        }
    }

    struct PanickingFocusComponent;

    impl Component<FocusTestState, FocusTestMsg> for PanickingFocusComponent {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, FocusTestState, FocusTestMsg>) {
            panic!("modal render failed");
        }
    }

    struct RecordingFocusModal {
        rendered: Arc<Mutex<Vec<(bool, bool)>>>,
    }

    impl Component<FocusTestState, FocusTestMsg> for RecordingFocusModal {
        fn render(&mut self, ctx: &mut RenderCtx<'_, FocusTestState, FocusTestMsg>) {
            let area = ctx.area();
            ctx.render_component(
                ChildId::Static("leaf"),
                FocusLeaf::recording(Arc::clone(&self.rendered)),
                area,
            );
        }

        fn scope_options(&self) -> ScopeOptions {
            ScopeOptions::default()
        }
    }

    #[test]
    fn modal_boundaries_flush_each_layers_passive_overlays_in_stack_order() {
        let state = FocusTestState::default();
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn = Ratcn::<FocusTestState, FocusTestMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("base"),
                        LoggingComponent {
                            name: "base",
                            log: Arc::clone(&log),
                            focusable: false,
                        },
                        area,
                    );
                    let base = Arc::clone(&log);
                    ctx.defer_paint(move |_, _| {
                        base.lock().expect("paint log").push("base overlay");
                    });
                    ctx.modal(
                        ChildId::Static("lower"),
                        LoggingComponent {
                            name: "lower",
                            log: Arc::clone(&log),
                            focusable: false,
                        },
                        area,
                    );
                    let lower = Arc::clone(&log);
                    ctx.defer_paint(move |_, _| {
                        lower.lock().expect("paint log").push("lower overlay");
                    });
                    ctx.modal(
                        ChildId::Static("top"),
                        LoggingComponent {
                            name: "top",
                            log: Arc::clone(&log),
                            focusable: false,
                        },
                        area,
                    );
                    let top = Arc::clone(&log);
                    ctx.defer_paint(move |_, _| top.lock().expect("paint log").push("top overlay"));
                });
            })
            .expect("draw");

        // Components paint in declaration order first. All three overlays
        // here were registered from the root context, so they are base
        // declaration decoration: they flush in registration order after the
        // modal canvases composite, painting above everything — the toast
        // slot. Decoration meant to travel with one layer is deferred from
        // inside that layer instead.
        assert_eq!(
            *log.lock().expect("paint log"),
            [
                "base",
                "lower",
                "top",
                "base overlay",
                "lower overlay",
                "top overlay"
            ]
        );
        assert!(ratcn.modal_is_open());
    }

    /// The other half of the rule above: paint deferred *inside* a layer
    /// flushes onto that layer's canvas once the layer has finished
    /// declaring, so it covers the layer's own content rather than being
    /// covered by it.
    #[test]
    fn overlay_deferred_inside_a_layer_covers_that_layers_content() {
        let state = PointerState;
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(2, 1)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.popup(
                        ChildId::Static("panel"),
                        PopupOptions::default(),
                        Rect::new(0, 0, 2, 1),
                        |ctx| {
                            ctx.paint(|ctx| {
                                ctx.render_widget(
                                    ratatui::text::Line::from("PP"),
                                    Rect::new(0, 0, 2, 1),
                                );
                            });
                            ctx.defer_paint(|painter, _| {
                                painter.render_widget(
                                    ratatui::text::Line::from("O"),
                                    Rect::new(0, 0, 1, 1),
                                );
                            });
                        },
                    );
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((0, 0)).expect("cell").symbol(), "O");
        assert_eq!(buffer.cell((1, 0)).expect("cell").symbol(), "P");
    }

    /// A composite that fills its own area, and a child that draws one cell of
    /// it.
    struct BackdropParent;

    impl Component<(), ()> for BackdropParent {
        fn render(&mut self, ctx: &mut RenderCtx<'_, (), ()>) {
            let area = ctx.area();
            ctx.render_component(ChildId::Static("glyph"), GlyphLeaf("C"), area);
        }

        fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, ()>) {
            let area = ctx.area();
            ctx.render_widget(
                ratatui::text::Line::from("#".repeat(area.width as usize)),
                area,
            );
        }
    }

    /// A leaf whose whole behavior is putting one identifiable glyph in the
    /// top-left cell of its area, so a test can name what reached the screen.
    struct GlyphLeaf(&'static str);

    impl<S, M> Component<S, M> for GlyphLeaf {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, S, M>) {}

        fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, S>) {
            let area = ctx.area();
            ctx.render_widget(
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
        let mut ratcn = Ratcn::<(), ()>::new();
        let mut terminal = Terminal::new(TestBackend::new(3, 1)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &(), &theme, |ctx| {
                    ctx.render_component(ChildId::Static("parent"), BackdropParent, area);
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((0, 0)).expect("cell").symbol(), "C");
        assert_eq!(buffer.cell((1, 0)).expect("cell").symbol(), "#");
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
        let mut ratcn = Ratcn::<ModalTestState, ModalTestMsg>::new()
            .modals(|state: &ModalTestState| &state.modals);
        let mut terminal = Terminal::new(TestBackend::new(3, 1)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(ChildId::Static("first"), GlyphLeaf("A"), area);
                });
            })
            .expect("draw");

        terminal
            .draw(|frame| {
                let area = frame.area();
                frame.render_widget(ratatui::text::Line::from("A"), area);
                // Nothing is open in the app's stack, so this modal root is a
                // declaration the runtime refuses to retain.
                let rejected = catch_unwind(AssertUnwindSafe(|| {
                    ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.modal(ChildId::Static("sheet"), GlyphLeaf("B"), area);
                    });
                }));
                assert!(rejected.is_err());
            })
            .expect("draw");

        assert_eq!(
            terminal
                .backend()
                .buffer()
                .cell((0, 0))
                .expect("cell")
                .symbol(),
            "A",
            "a pass rejected by the modal stack must not have painted"
        );
        assert_eq!(ratcn.declared_paths(), vec![vec![ChildId::Static("first")]]);
    }

    /// The rejection above, watched from the base layer rather than from the
    /// modal that caused it.
    ///
    /// A modal paints into a canvas that only composites at the very end, so
    /// a modal-mismatched pass could paint its *base* content and still leave
    /// the modal's own invisible. That is the leak the check's position has
    /// to prevent, and it is only visible from a base-layer declaration
    /// sharing the cell the last good frame owns.
    #[test]
    fn a_rejected_pass_never_paints_its_base_layer_either() {
        let state = ModalTestState::default();
        let mut ratcn = Ratcn::<ModalTestState, ModalTestMsg>::new()
            .modals(|state: &ModalTestState| &state.modals);
        let mut terminal = Terminal::new(TestBackend::new(3, 1)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(ChildId::Static("first"), GlyphLeaf("A"), area);
                });
            })
            .expect("draw");

        terminal
            .draw(|frame| {
                let area = frame.area();
                frame.render_widget(ratatui::text::Line::from("A"), area);
                let rejected = catch_unwind(AssertUnwindSafe(|| {
                    ratcn.render(frame, &state, &theme, |ctx| {
                        // Base-layer content, which paints straight onto the
                        // frame, declared alongside the modal root the app's
                        // empty stack refuses.
                        ctx.render_component(ChildId::Static("base"), GlyphLeaf("B"), area);
                        ctx.modal(ChildId::Static("sheet"), GlyphLeaf("C"), area);
                    });
                }));
                assert!(rejected.is_err());
            })
            .expect("draw");

        assert_eq!(
            terminal
                .backend()
                .buffer()
                .cell((0, 0))
                .expect("cell")
                .symbol(),
            "A",
            "a rejected pass must not have painted its base layer"
        );
        assert_eq!(ratcn.declared_paths(), vec![vec![ChildId::Static("first")]]);
    }

    /// The two hover flags mean different things, and a container relies on
    /// the difference: `hovered` is "the pointer is on *me*", `contains_hover`
    /// is "the pointer is somewhere inside me".
    ///
    /// They agree everywhere except on an ancestor of the hovered leaf, which
    /// is why the ancestor has to be watched alongside the leaf — either flag
    /// collapsing into the other is invisible from the leaf alone.
    #[test]
    fn hovered_and_contains_hover_are_distinct_at_the_leaf_and_its_ancestor() {
        let state = PointerState;
        let scope_flags = Arc::new(Mutex::new(Vec::new()));
        let leaf_flags = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(6, 1)).expect("terminal");
        let theme = Theme::default_dark();
        let mut render = |ratcn: &mut Ratcn<PointerState, PointerMsg>| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &state, &theme, |ctx| {
                        let scope_flags = Arc::clone(&scope_flags);
                        let leaf_flags = Arc::clone(&leaf_flags);
                        ctx.scope(
                            ChildId::Static("pane"),
                            area,
                            ScopeOptions::default(),
                            move |ctx| {
                                ctx.paint(move |ctx| {
                                    scope_flags
                                        .lock()
                                        .expect("scope hover log")
                                        .push((ctx.hovered, ctx.contains_hover));
                                });
                                ctx.render_component(
                                    ChildId::Static("leaf"),
                                    HoverLeaf {
                                        consume_move: false,
                                        rendered: Some(leaf_flags),
                                    },
                                    area,
                                );
                            },
                        );
                    });
                })
                .expect("draw");
        };

        render(&mut ratcn);
        ratcn.handle_event(mouse(MouseKind::Moved, 1, 0), &state);
        leaf_flags.lock().expect("leaf hover log").clear();
        scope_flags.lock().expect("scope hover log").clear();
        render(&mut ratcn);

        assert_eq!(
            *leaf_flags.lock().expect("leaf hover log"),
            [(true, true)],
            "the pointer is on the leaf, so both halves hold there"
        );
        assert_eq!(
            *scope_flags.lock().expect("scope hover log"),
            [(false, true)],
            "the scope contains the pointer without being under it"
        );
    }

    /// A layer's canvas composites whatever was written to it, whichever write
    /// form did the writing: raw buffer access marks the layer painted exactly
    /// as a widget does, so a popup that only ever calls `with_buffer` still
    /// reaches the frame.
    #[test]
    fn layer_paint_written_through_with_buffer_composites_onto_the_frame() {
        let state = PointerState;
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(2, 1)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.popup(
                        ChildId::Static("panel"),
                        PopupOptions::default(),
                        Rect::new(0, 0, 2, 1),
                        |ctx| {
                            ctx.paint(|ctx| {
                                ctx.with_buffer(|buf| {
                                    buf[(0, 0)].set_symbol("Z");
                                });
                            });
                        },
                    );
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((0, 0)).expect("cell").symbol(), "Z");
    }

    #[test]
    fn later_modal_focus_intent_marks_only_its_descendant_focused() {
        let state = FocusTestState {
            focus: FocusState::intent([ChildId::Static("top")]),
        };
        let lower = Arc::new(Mutex::new(Vec::new()));
        let top = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.modal(
                        ChildId::Static("lower"),
                        RecordingFocusModal {
                            rendered: Arc::clone(&lower),
                        },
                        area,
                    );
                    ctx.modal(
                        ChildId::Static("top"),
                        RecordingFocusModal {
                            rendered: Arc::clone(&top),
                        },
                        area,
                    );
                });
            })
            .expect("draw");

        assert_eq!(*lower.lock().expect("lower focus log"), [(false, false)]);
        assert_eq!(*top.lock().expect("top focus log"), [(true, true)]);
    }

    #[test]
    fn top_modal_alone_receives_and_absorbs_keyboard_input() {
        let state = FocusTestState::default();
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    for (id, name) in [("lower", "lower"), ("top", "top")] {
                        ctx.modal(
                            ChildId::Static(id),
                            LoggingComponent {
                                name,
                                log: Arc::clone(&log),
                                focusable: true,
                            },
                            area,
                        );
                    }
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(FocusTestMsg::Activated(vec![ChildId::Static("top")]))
        );
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Char('z'))), &state),
            EventResult::Emit(FocusTestMsg::Activated(vec![ChildId::Static("top")]))
        );
    }

    #[test]
    fn tab_from_base_focus_enters_and_cannot_escape_the_active_modal() {
        let state = FocusTestState {
            focus: FocusState::intent([ChildId::Static("base")]),
        };
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(ChildId::Static("base"), FocusLeaf::enabled(), area);
                    ctx.modal(ChildId::Static("dialog"), FocusModal, area);
                });
            })
            .expect("draw");

        let expected = FocusState::intent([ChildId::Static("dialog"), ChildId::Static("leaf")]);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(FocusTestMsg::Activated(expected.path().to_vec()))
        );
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
            EventResult::Consumed
        );
    }

    #[test]
    fn app_restores_exact_base_focus_and_can_restore_an_absent_parked_path() {
        let mut state = FocusTestState {
            focus: FocusState::intent([ChildId::Static("base")]),
        };
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let saved = state.focus.clone();
        state.focus = FocusState::intent([ChildId::Static("dialog")]);
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(ChildId::Static("base"), FocusLeaf::enabled(), area);
                    ctx.modal(ChildId::Static("dialog"), FocusModal, area);
                });
            })
            .expect("draw");

        state.focus = saved;
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(ChildId::Static("base"), FocusLeaf::enabled(), area);
                });
            })
            .expect("draw");
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(FocusTestMsg::Activated(vec![ChildId::Static("base")]))
        );

        state.focus = FocusState::intent([ChildId::Static("temporarily-absent")]);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Ignored
        );
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
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
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.modal(ChildId::Static("lower"), FocusModal, area);
                    ctx.modal(ChildId::Static("top"), FocusModal, area);
                });
            })
            .expect("draw");
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(FocusTestMsg::Activated(vec![
                ChildId::Static("top"),
                ChildId::Static("leaf"),
            ]))
        );

        state.focus = FocusState::intent([ChildId::Static("lower")]);
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.modal(ChildId::Static("lower"), FocusModal, area);
                });
            })
            .expect("draw");
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
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
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let render = |ratcn: &mut Ratcn<FocusTestState, FocusTestMsg>,
                      terminal: &mut Terminal<TestBackend>,
                      state: &FocusTestState,
                      lower_open,
                      top_open| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, state, &theme, |ctx| {
                        ctx.scope(
                            ChildId::Static("base"),
                            Rect::ZERO,
                            ScopeOptions::default(),
                            |ctx| {
                                ctx.render_component(
                                    ChildId::Static("base-child"),
                                    FocusLeaf::enabled(),
                                    area,
                                );
                            },
                        );
                        if lower_open {
                            ctx.modal(ChildId::Static("lower"), FocusModal, area);
                        }
                        if top_open {
                            ctx.modal(ChildId::Static("top"), FocusModal, area);
                        }
                    });
                })
                .expect("draw");
        };

        focus_history.push(state.focus.clone());
        state.focus = lower.clone();
        render(&mut ratcn, &mut terminal, &state, true, false);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(FocusTestMsg::Activated(lower.path().to_vec()))
        );

        focus_history.push(state.focus.clone());
        state.focus = top.clone();
        render(&mut ratcn, &mut terminal, &state, true, true);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(FocusTestMsg::Activated(top.path().to_vec()))
        );

        state.focus = focus_history.pop().expect("lower modal focus history");
        assert_eq!(state.focus, lower);
        render(&mut ratcn, &mut terminal, &state, true, false);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(FocusTestMsg::Activated(lower.path().to_vec()))
        );

        state.focus = focus_history.pop().expect("base focus history");
        assert_eq!(state.focus, base);
        render(&mut ratcn, &mut terminal, &state, false, false);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
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
        let rendered = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn = Ratcn::new()
            .focus(|state: &ModalTestState| &state.focus, |_| unreachable!())
            .modals(|state: &ModalTestState| &state.modals);
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.modal(
                        ChildId::Static("dialog"),
                        ModalFocusRoute {
                            rendered: Arc::clone(&rendered),
                        },
                        area,
                    );
                });
            })
            .expect("draw");

        assert_eq!(
            *rendered.lock().expect("modal focus render log"),
            [(false, false)]
        );
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(ModalTestMsg::Routed("dialog"))
        );
    }

    #[test]
    fn modal_binding_suppresses_opening_and_closing_gaps_then_routes_after_sync() {
        let mut state = ModalTestState::default();
        let mut ratcn = Ratcn::new()
            .focus(|state: &ModalTestState| &state.focus, |_| unreachable!())
            .modals(|state: &ModalTestState| &state.modals);
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let render = |ratcn: &mut Ratcn<ModalTestState, ModalTestMsg>,
                      terminal: &mut Terminal<TestBackend>,
                      state: &ModalTestState| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, state, &theme, |ctx| {
                        ctx.render_component(ChildId::Static("base"), ModalRoute("base"), area);
                        if state.modals.is_open("dialog") {
                            ctx.modal(ChildId::Static("dialog"), ModalRoute("dialog"), area);
                        }
                    });
                })
                .expect("draw");
        };

        state
            .modals
            .open("dialog", &mut state.focus)
            .expect("open before first render");
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Ignored,
            "there is no retained surface to protect before the first render"
        );
        let _ = state.modals.close(&mut state.focus);
        render(&mut ratcn, &mut terminal, &state);
        state
            .modals
            .open("dialog", &mut state.focus)
            .expect("open dialog");
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Consumed,
            "opening gap must not reach the retained base"
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 0, 0), &state),
            EventResult::Consumed
        );

        render(&mut ratcn, &mut terminal, &state);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(ModalTestMsg::Routed("dialog"))
        );

        let _ = state.modals.close(&mut state.focus);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Consumed,
            "closing gap must not reach the retained modal"
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 0, 0), &state),
            EventResult::Consumed
        );

        render(&mut ratcn, &mut terminal, &state);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(ModalTestMsg::Routed("base"))
        );
    }

    #[test]
    fn modal_binding_mismatch_preserves_the_previous_surface_atomically() {
        let mut state = ModalTestState::default();
        let mut ratcn = Ratcn::new()
            .focus(|state: &ModalTestState| &state.focus, |_| unreachable!())
            .modals(|state: &ModalTestState| &state.modals);
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(ChildId::Static("base"), ModalRoute("base"), area);
                });
            })
            .expect("initial draw");

        state
            .modals
            .open("expected", &mut state.focus)
            .expect("open expected modal");
        let failed = catch_unwind(AssertUnwindSafe(|| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.modal(ChildId::Static("wrong"), ModalRoute("wrong"), area);
                    });
                })
                .expect("mismatched draw");
        }));

        assert!(failed.is_err());
        assert_eq!(ratcn.declared_paths(), vec![vec![ChildId::Static("base")]]);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Consumed
        );

        let _ = state.modals.close(&mut state.focus);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
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
        let mut ratcn = Ratcn::new().focus(|state: &State| &state.focus, Msg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(20, 6)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("base"),
                        crate::Button::new("Base").on_press(|| Msg::Base),
                        Rect::new(0, 0, 10, 1),
                    );
                    ctx.modal_scope(
                        ChildId::Static("sheet"),
                        area,
                        ScopeOptions::default(),
                        |ctx| {
                            ctx.render_component(
                                ChildId::Static("ok"),
                                crate::Button::new("OK").on_press(|| Msg::Ok),
                                Rect::new(2, 3, 6, 1),
                            );
                        },
                    );
                });
            })
            .expect("draw");

        // Startup focus descends into the modal scope, not the base layer.
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
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
            ratcn.handle_event(click(MouseKind::Down(MouseButton::Left)), &state),
            EventResult::Consumed
        );
        assert_eq!(
            ratcn.handle_event(click(MouseKind::Up(MouseButton::Left)), &state),
            EventResult::Consumed
        );

        // A key nothing inside handles is absorbed, not leaked to the base UI.
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Esc)), &state),
            EventResult::Consumed
        );
    }

    fn render_popup_over_leaf(
        ratcn: &mut Ratcn<PointerState, PointerMsg>,
        terminal: &mut Terminal<TestBackend>,
        state: &PointerState,
        with_dismiss: bool,
    ) {
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, state, &theme, |ctx| {
                    ctx.render_component(ChildId::Static("under"), RouteLeaf("under"), area);
                    let options = if with_dismiss {
                        PopupOptions::default().on_dismiss(|| PointerMsg::Dismissed)
                    } else {
                        PopupOptions::default()
                    };
                    // The popup covers the left half; its content is passive
                    // paint, so presses inside it reach nothing interactive.
                    ctx.popup(
                        ChildId::Static("panel"),
                        options,
                        Rect::new(0, 0, 5, 2),
                        |_| {},
                    );
                });
            })
            .expect("draw");
    }

    #[test]
    fn popup_occludes_its_footprint_and_leaves_the_rest_clickable() {
        let state = PointerState;
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        render_popup_over_leaf(&mut ratcn, &mut terminal, &state, false);

        // Inside the popup's footprint, the occluded leaf must never see the
        // press: the popup's content ignored it, so it is consumed at the
        // popup boundary rather than falling through.
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state),
            EventResult::Consumed
        );
        // Outside the footprint, the leaf is visibly there and stays live.
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 7, 0), &state),
            EventResult::Emit(PointerMsg::Routed(
                "under",
                MouseKind::Down(MouseButton::Left),
                0
            ))
        );
    }

    /// Modal policy is about the modal's subtree, not about declaration
    /// order: a popup declared after the modal but outside it is still
    /// covered, so presses on it are consumed rather than routed.
    #[test]
    fn a_popup_declared_after_a_modal_is_still_covered_by_it() {
        let state = PointerState;
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 4)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.modal(
                        ChildId::Static("dlg"),
                        RouteLeaf("dlg"),
                        Rect::new(0, 2, 10, 2),
                    );
                    // Declared later, so it takes a higher layer number — but
                    // it is a sibling of the modal, not inside it.
                    ctx.popup(
                        ChildId::Static("panel"),
                        PopupOptions::default(),
                        Rect::new(0, 0, 5, 1),
                        |ctx| {
                            ctx.render_component(
                                ChildId::Static("pi"),
                                RouteLeaf("pi"),
                                Rect::new(0, 0, 5, 1),
                            );
                        },
                    );
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state),
            EventResult::Consumed,
            "the modal covers it, so the press must not reach the popup's content"
        );
    }

    /// A hint layer paints above everything and takes nothing: a press over it
    /// reaches the control underneath, and its content cannot be focused *even
    /// when that content claims it is focusable*. The claim has to be refused
    /// by the layer policy rather than by what happens to be declared inside,
    /// so the content here is deliberately focusable.
    #[test]
    fn a_hint_layer_is_inert_to_the_pointer_and_to_focus() {
        struct FocusableLeaf;

        impl Component<PointerState, PointerMsg> for FocusableLeaf {
            fn render(&mut self, _ctx: &mut RenderCtx<'_, PointerState, PointerMsg>) {}

            fn is_focusable(&self, _state: &PointerState) -> bool {
                true
            }
        }

        let state = PointerState;
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("button"),
                        RouteLeaf("button"),
                        Rect::new(0, 0, 6, 1),
                    );
                    // Covers the button completely.
                    ctx.hint(
                        ChildId::Static("tip"),
                        ScopeOptions::default(),
                        Rect::new(0, 0, 6, 1),
                        |ctx| {
                            ctx.render_component(
                                ChildId::Static("text"),
                                FocusableLeaf,
                                Rect::new(0, 0, 6, 1),
                            );
                        },
                    );
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state),
            EventResult::Emit(PointerMsg::Routed(
                "button",
                MouseKind::Down(MouseButton::Left),
                0
            )),
            "the press passes through the hint to the control it describes"
        );
        assert_eq!(
            ratcn.focus_path(&[ChildId::Static("tip"), ChildId::Static("text")]),
            None,
            "a focusable component inside a hint is still not a focus target"
        );
    }

    /// Sibling popups each dismiss when a press lands outside them — including
    /// a press that lands inside the other one. "Outside" is a containment
    /// question, not a comparison of layer numbers.
    #[test]
    fn a_press_inside_one_popup_dismisses_its_sibling() {
        let state = PointerState;
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(12, 4)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.popup(
                        ChildId::Static("first"),
                        PopupOptions::default()
                            .on_dismiss(|| PointerMsg::Routed("first", MouseKind::Moved, 0)),
                        Rect::new(0, 0, 5, 1),
                        |_| {},
                    );
                    ctx.popup(
                        ChildId::Static("second"),
                        PopupOptions::default()
                            .on_dismiss(|| PointerMsg::Routed("second", MouseKind::Moved, 0)),
                        Rect::new(6, 2, 5, 1),
                        |_| {},
                    );
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 7, 2), &state),
            EventResult::Emit(PointerMsg::Routed("first", MouseKind::Moved, 0)),
            "the press is inside `second` and outside `first`, so `first` dismisses"
        );
    }

    /// With popups nested inside one another, a press outside dismisses the
    /// innermost — the one on top — not the one it is nested in.
    #[test]
    fn an_outside_press_dismisses_the_innermost_nested_popup() {
        let state = PointerState;
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 4)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.popup(
                        ChildId::Static("outer"),
                        PopupOptions::default()
                            .on_dismiss(|| PointerMsg::Routed("outer", MouseKind::Moved, 0)),
                        Rect::new(0, 0, 5, 2),
                        |ctx| {
                            ctx.popup(
                                ChildId::Static("inner"),
                                PopupOptions::default().on_dismiss(|| {
                                    PointerMsg::Routed("inner", MouseKind::Moved, 0)
                                }),
                                Rect::new(0, 0, 3, 1),
                                |_| {},
                            );
                        },
                    );
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 9, 3), &state),
            EventResult::Emit(PointerMsg::Routed("inner", MouseKind::Moved, 0)),
            "the innermost popup is the topmost, so it is what a press outside dismisses"
        );
    }

    #[test]
    fn outside_press_emits_the_dismiss_hook_only_when_routing_stayed_silent() {
        let state = PointerState;
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 4)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("button"),
                        RouteLeaf("button"),
                        Rect::new(6, 0, 4, 1),
                    );
                    ctx.popup(
                        ChildId::Static("panel"),
                        PopupOptions::default().on_dismiss(|| PointerMsg::Dismissed),
                        Rect::new(0, 0, 5, 2),
                        |_| {},
                    );
                });
            })
            .expect("draw");

        // A press on inert space outside the popup: nothing routed, so the
        // dismiss hook speaks.
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 8, 3), &state),
            EventResult::Emit(PointerMsg::Dismissed)
        );
        // A press on a control outside the popup: the control's own message
        // wins — the app treats it as the dismissal signal, and the click
        // that follows activates as usual.
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 7, 0), &state),
            EventResult::Emit(PointerMsg::Routed(
                "button",
                MouseKind::Down(MouseButton::Left),
                0
            ))
        );
        // A press inside the popup dismisses nothing.
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state),
            EventResult::Consumed
        );
    }

    struct PopupHost;

    impl Component<FocusTestState, FocusTestMsg> for PopupHost {
        fn render(&mut self, ctx: &mut RenderCtx<'_, FocusTestState, FocusTestMsg>) {
            let area = ctx.area();
            ctx.popup(
                ChildId::Static("panel"),
                PopupOptions::default(),
                area,
                |ctx| {
                    let area = ctx.area();
                    ctx.render_component(ChildId::Static("item"), FocusLeaf::enabled(), area);
                },
            );
        }

        fn handle_event(
            &mut self,
            event: &Event,
            _state: &FocusTestState,
            ctx: &mut EventCtx<'_>,
        ) -> EventResult<FocusTestMsg> {
            if matches!(event, Event::Key(key) if key.code == KeyCode::Esc) {
                EventResult::Emit(FocusTestMsg::Parent(ctx.path().to_vec()))
            } else {
                EventResult::Ignored
            }
        }
    }

    #[test]
    fn keys_bubble_through_the_popup_root_to_the_declaring_component() {
        // Focus sits on the popup's item; Esc is not handled inside the
        // panel, crosses the popup root, and reaches the component that
        // opened the popup — the Select-closes-on-Esc pattern.
        let state = FocusTestState {
            focus: FocusState::intent([
                ChildId::Static("host"),
                ChildId::Static("panel"),
                ChildId::Static("item"),
            ]),
        };
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(ChildId::Static("host"), PopupHost, area);
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(FocusTestMsg::Activated(vec![
                ChildId::Static("host"),
                ChildId::Static("panel"),
                ChildId::Static("item"),
            ]))
        );
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Esc)), &state),
            EventResult::Emit(FocusTestMsg::Parent(vec![ChildId::Static("host")]))
        );
    }

    #[test]
    fn popup_inside_a_modal_sits_above_it_and_routes() {
        let state = PointerState;
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(ChildId::Static("base"), RouteLeaf("base"), area);
                    ctx.modal_scope(
                        ChildId::Static("sheet"),
                        area,
                        ScopeOptions::default(),
                        move |ctx| {
                            ctx.render_component(
                                ChildId::Static("field"),
                                RouteLeaf("field"),
                                Rect::new(5, 0, 5, 2),
                            );
                            ctx.popup(
                                ChildId::Static("panel"),
                                PopupOptions::default(),
                                Rect::new(0, 0, 5, 2),
                                |ctx| {
                                    ctx.render_component(
                                        ChildId::Static("option"),
                                        RouteLeaf("option"),
                                        Rect::new(0, 0, 5, 2),
                                    );
                                },
                            );
                        },
                    );
                });
            })
            .expect("draw");

        // The popup, declared inside the modal, is above the modal floor and
        // receives its own hits.
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state),
            EventResult::Emit(PointerMsg::Routed(
                "option",
                MouseKind::Down(MouseButton::Left),
                0
            ))
        );
        // The modal's own content stays interactive beside the popup.
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 7, 0), &state),
            EventResult::Emit(PointerMsg::Routed(
                "field",
                MouseKind::Down(MouseButton::Left),
                0
            ))
        );
    }

    #[test]
    fn popup_paint_composites_above_later_declared_base_siblings() {
        let state = PointerState;
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(4, 1)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    // The popup paints first in declaration order…
                    ctx.popup(
                        ChildId::Static("panel"),
                        PopupOptions::default(),
                        Rect::new(0, 0, 2, 1),
                        |ctx| {
                            ctx.paint(|ctx| {
                                ctx.render_widget(
                                    ratatui::text::Line::from("PP"),
                                    Rect::new(0, 0, 2, 1),
                                );
                            });
                        },
                    );
                    // …and a base sibling paints over the same cells after —
                    // yet the popup composites on top.
                    ctx.paint(move |ctx| {
                        ctx.render_widget(ratatui::text::Line::from("BBBB"), area);
                    });
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((0, 0)).expect("cell").symbol(), "P");
        assert_eq!(buffer.cell((1, 0)).expect("cell").symbol(), "P");
        assert_eq!(buffer.cell((2, 0)).expect("cell").symbol(), "B");
    }

    fn render_bound_nested_modal(
        ratcn: &mut Ratcn<ModalTestState, ModalTestMsg>,
        terminal: &mut Terminal<TestBackend>,
        state: &ModalTestState,
        rendered: &FocusRenderLog,
    ) {
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, state, &theme, |ctx| {
                    let rendered = Arc::clone(rendered);
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
                                    ctx.render_component(
                                        ChildId::Static("inner"),
                                        ModalFocusLeaf {
                                            rendered: Arc::clone(&rendered),
                                        },
                                        area,
                                    );
                                },
                            );
                        },
                    );
                });
            })
            .expect("draw");
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
        let rendered = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn = Ratcn::new()
            .focus(|state: &ModalTestState| &state.focus, ModalTestMsg::Focus)
            .modals(|state: &ModalTestState| &state.modals);
        let mut terminal = Terminal::new(TestBackend::new(20, 6)).expect("terminal");

        // Frame one: the `[sheet]` open intent descends into the nested
        // modal's first focusable leaf.
        render_bound_nested_modal(&mut ratcn, &mut terminal, &state, &rendered);
        assert_eq!(*rendered.lock().expect("inner render log"), [(true, true)]);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(ModalTestMsg::Routed("inner"))
        );

        // Frame two: a stored full in-modal path survives alignment intact.
        state.focus = FocusState::intent([
            ChildId::Static("pane"),
            ChildId::Static("sheet"),
            ChildId::Static("inner"),
        ]);
        rendered.lock().expect("inner render log").clear();
        render_bound_nested_modal(&mut ratcn, &mut terminal, &state, &rendered);
        assert_eq!(*rendered.lock().expect("inner render log"), [(true, true)]);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(ModalTestMsg::Routed("inner"))
        );
    }

    struct ModalFocusLeaf {
        rendered: FocusRenderLog,
    }

    impl Component<ModalTestState, ModalTestMsg> for ModalFocusLeaf {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, ModalTestState, ModalTestMsg>) {}

        fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, ModalTestState>) {
            self.rendered
                .lock()
                .expect("render log")
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

        fn is_focusable(&self, _state: &ModalTestState) -> bool {
            true
        }
    }

    struct ClickLeaf(&'static str);

    impl Component<PointerState, PointerMsg> for ClickLeaf {
        fn render(&mut self, _ctx: &mut RenderCtx<'_, PointerState, PointerMsg>) {}

        fn handle_event(
            &mut self,
            event: &Event,
            _state: &PointerState,
            _ctx: &mut EventCtx<'_>,
        ) -> EventResult<PointerMsg> {
            match event {
                Event::Mouse(mouse) if matches!(mouse.kind, MouseKind::Click(_)) => {
                    EventResult::Emit(PointerMsg::Routed(self.0, mouse.kind, 0))
                }
                _ => EventResult::Ignored,
            }
        }
    }

    #[test]
    fn one_physical_click_dismisses_the_popup_and_presses_the_button() {
        // The full click-through sequence: the press dismisses (Down), the
        // app closes the popup and redraws, and the release's Click still
        // presses the control the press landed on — the press target
        // survives the popup-closing redraw.
        let state = PointerState;
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let render = |ratcn: &mut Ratcn<PointerState, PointerMsg>,
                      terminal: &mut Terminal<TestBackend>,
                      popup_open: bool| {
            terminal
                .draw(|frame| {
                    ratcn.render(frame, &PointerState, &theme, |ctx| {
                        ctx.render_component(
                            ChildId::Static("button"),
                            ClickLeaf("button"),
                            Rect::new(6, 0, 4, 1),
                        );
                        if popup_open {
                            ctx.popup(
                                ChildId::Static("panel"),
                                PopupOptions::default().on_dismiss(|| PointerMsg::Dismissed),
                                Rect::new(0, 0, 5, 2),
                                |_| {},
                            );
                        }
                    });
                })
                .expect("draw");
        };

        render(&mut ratcn, &mut terminal, true);
        // Press on the visible button: the button ignores Down, so the
        // dismiss hook speaks for the press.
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 7, 0), &state),
            EventResult::Emit(PointerMsg::Dismissed)
        );
        // The app closes the popup and redraws.
        render(&mut ratcn, &mut terminal, false);
        // The release completes the same physical click on the button.
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 7, 0), &state),
            EventResult::Emit(PointerMsg::Routed(
                "button",
                MouseKind::Click(MouseButton::Left),
                0
            ))
        );
    }

    #[test]
    fn nested_modal_scope_behaves_like_a_root_declared_one() {
        // A modal declared from inside a component subtree anchors its
        // identity there but carries full modal policy: focus resolves into
        // it, keys nothing inside handles are consumed at its root, and the
        // base layer stops receiving events.
        let state = FocusTestState::default();
        let rendered = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(20, 6)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(ChildId::Static("base"), FocusLeaf::enabled(), area);
                    let rendered = Arc::clone(&rendered);
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
                                    ctx.render_component(
                                        ChildId::Static("inner"),
                                        FocusLeaf::recording(rendered),
                                        area,
                                    );
                                },
                            );
                        },
                    );
                });
            })
            .expect("draw");

        // Startup focus resolves into the nested modal, full path intact.
        assert_eq!(*rendered.lock().expect("inner render log"), [(true, true)]);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(FocusTestMsg::Activated(vec![
                ChildId::Static("pane"),
                ChildId::Static("sheet"),
                ChildId::Static("inner"),
            ]))
        );
        // A key nothing inside handles is consumed at the modal boundary, not
        // delivered to the base layer or the declaring scope.
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Char('x'))), &state),
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
        let left = Arc::new(Mutex::new(Vec::new()));
        let right = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    let left = Arc::clone(&left);
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
                                    ctx.render_component(
                                        ChildId::Static("item"),
                                        FocusLeaf::recording(left),
                                        area,
                                    );
                                },
                            );
                        },
                    );
                    let right = Arc::clone(&right);
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
                                    ctx.render_component(
                                        ChildId::Static("item"),
                                        FocusLeaf::recording(right),
                                        area,
                                    );
                                },
                            );
                        },
                    );
                });
            })
            .expect("draw");

        // Paint sees the resolved focus, not the raw app snapshot it started
        // from: focus moved off the covered branch and into the layer.
        assert_eq!(*left.lock().expect("left render log"), [(false, false)]);
        assert_eq!(*right.lock().expect("right render log"), [(true, true)]);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(FocusTestMsg::Activated(vec![
                ChildId::Static("right"),
                ChildId::Static("sheet"),
                ChildId::Static("item"),
            ]))
        );
        // Tab is trapped at the layer root, so it never reaches `left`.
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
            EventResult::Consumed
        );
    }

    #[test]
    fn failed_modal_pass_preserves_the_previous_stack() {
        let state = FocusTestState::default();
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn = Ratcn::<FocusTestState, FocusTestMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.modal(
                        ChildId::Static("stable"),
                        LoggingComponent {
                            name: "stable",
                            log: Arc::clone(&log),
                            focusable: false,
                        },
                        area,
                    );
                });
            })
            .expect("draw");
        let failed = catch_unwind(AssertUnwindSafe(|| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.modal(
                            ChildId::Static("replacement"),
                            PanickingFocusComponent,
                            area,
                        );
                    });
                })
                .expect("draw");
        }));

        assert!(failed.is_err());
        assert!(ratcn.modal_is_open());
        assert_eq!(
            ratcn.declared_paths(),
            vec![vec![ChildId::Static("stable")]]
        );
    }

    #[test]
    fn modal_is_the_component_root() {
        let state = FocusTestState {
            focus: FocusState::default(),
        };
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.modal(ChildId::Static("dialog"), FocusModal, area);
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.declared_paths(),
            vec![
                vec![ChildId::Static("dialog")],
                vec![ChildId::Static("dialog"), ChildId::Static("leaf"),],
            ]
        );
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Emit(FocusTestMsg::Activated(vec![
                ChildId::Static("dialog"),
                ChildId::Static("leaf"),
            ]))
        );
    }

    #[test]
    fn zero_area_modal_is_retained_but_excluded_from_keyboard_fallback() {
        let state = FocusTestState::default();
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.modal(
                        ChildId::Static("dialog"),
                        LoggingComponent {
                            name: "dialog",
                            log: Arc::clone(&log),
                            focusable: false,
                        },
                        Rect::ZERO,
                    );
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
            EventResult::Consumed
        );
        assert_eq!(
            ratcn.declared_paths(),
            vec![vec![ChildId::Static("dialog")]]
        );
        assert_eq!(*log.lock().expect("modal log"), ["dialog"]);
    }

    #[test]
    fn modal_wraps_focus_outside_the_component_boundary() {
        let state = FocusTestState {
            focus: FocusState::intent([ChildId::Static("dialog"), ChildId::Static("second")]),
        };
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.modal(ChildId::Static("dialog"), EscapeFocusModal, area);
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
            EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
                ChildId::Static("dialog"),
                ChildId::Static("first"),
            ])))
        );
    }

    #[test]
    fn caught_modal_boundary_failure_is_sticky_and_atomic() {
        let mut ratcn = Ratcn::<(), ()>::new();
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        render_leaf(&mut ratcn, &mut terminal, &ChildId::Static("stable"));
        let theme = Theme::default_dark();
        let failed = catch_unwind(AssertUnwindSafe(|| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &(), &theme, |ctx| {
                        ctx.defer_paint(|_, ()| panic!("base overlay failed"));
                        let caught = catch_unwind(AssertUnwindSafe(|| {
                            ctx.modal(ChildId::Static("modal"), Leaf, area);
                        }));
                        assert!(caught.is_err());
                    });
                })
                .expect("draw");
        }));

        assert!(failed.is_err());
        assert_eq!(
            ratcn.declared_paths(),
            vec![vec![ChildId::Static("stable")]]
        );
    }

    #[test]
    fn caught_lower_modal_overlay_flush_failure_preserves_retained_interaction() {
        let state = PointerState;
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.modal(
                        ChildId::Static("stable"),
                        Draggable { name: "stable" },
                        area,
                    );
                });
            })
            .expect("draw");
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state),
            EventResult::Consumed
        );

        let failed = catch_unwind(AssertUnwindSafe(|| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.modal(
                            ChildId::Static("replacement"),
                            Draggable {
                                name: "replacement",
                            },
                            area,
                        );
                        ctx.defer_paint(|_, _| panic!("lower modal overlay failed"));
                        let caught = catch_unwind(AssertUnwindSafe(|| {
                            ctx.modal(ChildId::Static("top"), RouteLeaf("top"), area);
                        }));
                        assert!(caught.is_err());
                    });
                })
                .expect("draw");
        }));

        assert!(failed.is_err());
        assert!(ratcn.modal_is_open());
        assert_eq!(
            ratcn.declared_paths(),
            vec![vec![ChildId::Static("stable")]]
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 9, 1), &state),
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
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        render_drag_surface(
            &mut ratcn,
            &mut terminal,
            &state,
            &[("drag", "base", Rect::new(0, 0, 5, 2))],
        );
        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(ChildId::Static("drag"), Draggable { name: "base" }, area);
                    ctx.modal(ChildId::Static("modal"), RouteLeaf("modal"), area);
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 9, 1), &state),
            EventResult::Consumed
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 9, 1), &state),
            EventResult::Consumed
        );
        render_drag_surface(
            &mut ratcn,
            &mut terminal,
            &state,
            &[("drag", "base", Rect::new(0, 0, 5, 2))],
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Drag(MouseButton::Left), 9, 1), &state),
            EventResult::Ignored
        );
    }

    /// A modal opening and closing over a pointer that never moves. Covering
    /// takes hover off what is beneath and uncovering gives it back, both on
    /// the frame that does it — the commit re-answers the hit test, and a
    /// modal is just another thing that changes the answer.
    #[test]
    fn a_modal_covering_and_uncovering_moves_hover_without_motion() {
        let rendered = Arc::new(Mutex::new(Vec::new()));
        let state = PointerState;
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let mut render = |ratcn: &mut Ratcn<PointerState, PointerMsg>, modal| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.render_component(
                            ChildId::Static("base"),
                            HoverLeaf {
                                consume_move: false,
                                rendered: Some(Arc::clone(&rendered)),
                            },
                            area,
                        );
                        if modal {
                            ctx.modal(ChildId::Static("modal"), RouteLeaf("modal"), area);
                        }
                    });
                })
                .expect("draw");
        };

        render(&mut ratcn, false);
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 1, 1), &state),
            EventResult::Consumed
        );
        render(&mut ratcn, false);
        assert_eq!(
            rendered.lock().expect("hover log").last(),
            Some(&(true, true))
        );

        render(&mut ratcn, true);
        assert_eq!(
            rendered.lock().expect("hover log").last(),
            Some(&(false, false)),
            "the modal is what the pointer is on now"
        );
        assert_eq!(ratcn.hover_path(), [ChildId::Static("modal")]);

        render(&mut ratcn, false);
        assert_eq!(
            rendered.lock().expect("hover log").last(),
            Some(&(true, true)),
            "and closing it hands the pointer back, on the same frame"
        );
        assert_eq!(ratcn.hover_path(), [ChildId::Static("base")]);
    }

    #[test]
    fn passive_overlay_never_becomes_a_hit_target() {
        let state = PointerState;
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(ChildId::Static("base"), RouteLeaf("base"), area);
                    ctx.defer_paint(|painter, _| {
                        painter.with_buffer(|buf| {
                            buf[(0, 0)].set_symbol("overlay");
                        });
                    });
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 0, 0), &state),
            EventResult::Emit(PointerMsg::Routed(
                "base",
                MouseKind::Down(MouseButton::Left),
                0,
            ))
        );
    }

    #[test]
    fn duplicate_modal_root_ids_fail_before_entering_their_layer() {
        let theme = Theme::default_dark();
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");

        for ids in [&["a", "a"][..], &["a", "b", "a"][..]] {
            let pending_overlay = Arc::new(AtomicBool::new(false));
            let mut ratcn = Ratcn::<(), ()>::new();
            render_leaf(&mut ratcn, &mut terminal, &ChildId::Static("stable"));
            let failed = catch_unwind(AssertUnwindSafe(|| {
                terminal
                    .draw(|frame| {
                        let area = frame.area();
                        ratcn.render(frame, &(), &theme, |ctx| {
                            for (position, id) in ids.iter().enumerate() {
                                ctx.modal(ChildId::Static(id), Leaf, area);
                                if position + 1 == ids.len() - 1 {
                                    let painted = Arc::clone(&pending_overlay);
                                    ctx.defer_paint(move |_, ()| {
                                        painted.store(true, Ordering::SeqCst);
                                    });
                                }
                            }
                        });
                    })
                    .expect("draw");
            }));
            assert!(failed.is_err());
            assert!(!pending_overlay.load(Ordering::SeqCst));
            assert_eq!(
                ratcn.declared_paths(),
                vec![vec![ChildId::Static("stable")]]
            );
        }
    }

    #[test]
    fn base_and_modal_root_id_collision_fails_before_base_overlay_flush() {
        let painted = Arc::new(AtomicBool::new(false));
        let mut ratcn = Ratcn::<(), ()>::new();
        let mut terminal = Terminal::new(TestBackend::new(5, 2)).expect("terminal");
        render_leaf(&mut ratcn, &mut terminal, &ChildId::Static("stable"));
        let theme = Theme::default_dark();
        let failed = catch_unwind(AssertUnwindSafe(|| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &(), &theme, |ctx| {
                        ctx.render_component(ChildId::Static("same"), Leaf, area);
                        let deferred = Arc::clone(&painted);
                        ctx.defer_paint(move |_, ()| deferred.store(true, Ordering::SeqCst));
                        ctx.modal(ChildId::Static("same"), Leaf, area);
                    });
                })
                .expect("draw");
        }));
        assert!(failed.is_err());
        assert!(!painted.load(Ordering::SeqCst));
        assert_eq!(
            ratcn.declared_paths(),
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
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.modal(
                        ChildId::Static("modal"),
                        Dialog::new()
                            .tab_wrap(TabWrap::Escape)
                            .on_dismiss(|| FocusTestMsg::Activated(vec![])),
                        area,
                    );
                });
            })
            .expect("draw");

        let recovered = FocusState::intent([ChildId::Static("modal")]);
        for code in [KeyCode::Tab, KeyCode::BackTab] {
            assert_eq!(
                ratcn.handle_event(Event::Key(KeyEvent::new(code)), &state),
                EventResult::Emit(FocusTestMsg::Focus(recovered.clone()))
            );
        }
        state.focus = FocusState::default();
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
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
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(10, 4)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.modal_scope(
                        ChildId::Static("outer"),
                        area,
                        ScopeOptions::default().tab_wrap(TabWrap::Wrap),
                        |ctx| {
                            ctx.render_component(
                                ChildId::Static("outerleaf"),
                                FocusLeaf::enabled(),
                                area,
                            );
                            // `Escape` on purpose: the modal boundary, not the
                            // scope's own wrap, must be what traps Tab.
                            ctx.modal_scope(
                                ChildId::Static("inner"),
                                area,
                                ScopeOptions::default(),
                                |ctx| {
                                    ctx.render_component(
                                        ChildId::Static("leaf"),
                                        FocusLeaf::enabled(),
                                        area,
                                    );
                                },
                            );
                        },
                    );
                });
            })
            .expect("draw");

        // Tab must not leak into the outer modal's own content.
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
            EventResult::Consumed,
            "the inner modal has one focusable leaf, so Tab wraps onto it"
        );
        // And the key reaches the inner modal's leaf, not the outer one's.
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
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
        let mut ratcn: Ratcn<ModalTestState, ModalTestMsg> =
            Ratcn::new().modals(|state: &ModalTestState| &state.modals);
        let mut terminal = Terminal::new(TestBackend::new(10, 4)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
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
            })
            .expect("declaring the stack in the order it was opened must render");
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
            let mut ratcn =
                Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
            let mut terminal = Terminal::new(TestBackend::new(20, 6)).expect("terminal");
            let theme = Theme::default_dark();
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.scope(
                            ChildId::Static("pane"),
                            Rect::new(0, 0, 20, 2),
                            ScopeOptions::default().tab_wrap(wrap),
                            |ctx| {
                                ctx.render_component(
                                    ChildId::Static("inside"),
                                    FocusLeaf::enabled(),
                                    Rect::new(0, 0, 10, 1),
                                );
                            },
                        );
                        ctx.modal(ChildId::Static("dlg"), FocusModal, area);
                    });
                })
                .expect("draw");

            assert_eq!(
                ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
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
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("outside"),
                        Button::new("Outside")
                            .on_press(|| FocusTestMsg::Activated(vec![ChildId::Static("outside")])),
                        Rect::new(0, 0, 12, 1),
                    );
                    // A collapsed modal takes part in nothing, but it is still
                    // open: keys must not reach what it covers.
                    ctx.modal(ChildId::Static("modal"), FocusLeaf::enabled(), Rect::ZERO);
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
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
        let mut ratcn = Ratcn::new()
            .focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus)
            .focus_key(KeyChord::from('1').alt(), [ChildId::Static("outside")]);
        let mut terminal = Terminal::new(TestBackend::new(20, 6)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
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
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(
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
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(30, 10)).expect("terminal");
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
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
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)), &state),
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
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(30, 10)).expect("terminal");
        let theme = Theme::default_dark();
        let draw = |terminal: &mut Terminal<TestBackend>,
                    ratcn: &mut Ratcn<FocusTestState, FocusTestMsg>,
                    state: &FocusTestState| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, state, &theme, |ctx| {
                        ctx.modal(
                            ChildId::Static("modal"),
                            Dialog::new()
                                .action(
                                    ChildId::Static("ok"),
                                    Button::new("OK").on_press(|| {
                                        FocusTestMsg::Activated(vec![ChildId::Static("ok")])
                                    }),
                                )
                                .action(
                                    ChildId::Static("cancel"),
                                    Button::new("Cancel").on_press(|| {
                                        FocusTestMsg::Activated(vec![ChildId::Static("cancel")])
                                    }),
                                ),
                            area,
                        );
                    });
                })
                .expect("draw");
        };
        draw(&mut terminal, &mut ratcn, &state);

        // Empty focus resolves to the first action; Tab cycles the actions
        // and never emits a path ending at the dialog root itself.
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
            EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
                ChildId::Static("modal"),
                ChildId::Static("cancel"),
            ])))
        );
        state.focus = FocusState::intent([ChildId::Static("modal"), ChildId::Static("cancel")]);
        draw(&mut terminal, &mut ratcn, &state);
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
            EventResult::Emit(FocusTestMsg::Focus(FocusState::intent([
                ChildId::Static("modal"),
                ChildId::Static("ok"),
            ]))),
            "the default Wrap cycles among the actions, never onto the dialog"
        );

        // Without `on_dismiss`, Esc emits nothing — the modal absorbs it
        // rather than letting it reach the base layer.
        assert_eq!(
            ratcn.handle_event(Event::Key(KeyEvent::new(KeyCode::Esc)), &state),
            EventResult::Consumed
        );
    }

    #[test]
    fn top_modal_push_and_pop_cancel_capture_through_release() {
        let state = PointerState;
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let mut render = |ratcn: &mut Ratcn<PointerState, PointerMsg>, top: bool| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.modal(ChildId::Static("lower"), Draggable { name: "lower" }, area);
                        if top {
                            ctx.modal(ChildId::Static("top"), Draggable { name: "top" }, area);
                        }
                    });
                })
                .expect("draw");
        };

        render(&mut ratcn, false);
        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
        render(&mut ratcn, true);
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 9, 1), &state),
            EventResult::Consumed
        );
        ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 9, 1), &state);

        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
        render(&mut ratcn, false);
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 9, 1), &state),
            EventResult::Consumed
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 9, 1), &state),
            EventResult::Consumed
        );
    }

    #[test]
    fn same_top_modal_identity_retains_capture_when_lower_stack_changes() {
        let state = PointerState;
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let mut render = |ratcn: &mut Ratcn<PointerState, PointerMsg>, lower, top_name| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.modal(ChildId::Static(lower), RouteLeaf("lower"), area);
                        ctx.modal(ChildId::Static("top"), Draggable { name: top_name }, area);
                    });
                })
                .expect("draw");
        };

        render(&mut ratcn, "lower-a", "before");
        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);
        render(&mut ratcn, "lower-b", "after");
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 9, 1), &state),
            EventResult::Emit(PointerMsg::Routed(
                "after",
                MouseKind::Drag(MouseButton::Left),
                2,
            ))
        );
    }

    #[test]
    fn same_top_modal_identity_retains_hover_when_lower_stack_changes() {
        let rendered = Arc::new(Mutex::new(Vec::new()));
        let state = PointerState;
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let mut render =
            |ratcn: &mut Ratcn<PointerState, PointerMsg>, state: &PointerState, lower| {
                terminal
                    .draw(|frame| {
                        let area = frame.area();
                        ratcn.render(frame, state, &theme, |ctx| {
                            ctx.modal(ChildId::Static(lower), RouteLeaf("lower"), area);
                            ctx.modal(
                                ChildId::Static("top"),
                                HoverLeaf {
                                    consume_move: false,
                                    rendered: Some(Arc::clone(&rendered)),
                                },
                                area,
                            );
                        });
                    })
                    .expect("draw");
            };

        render(&mut ratcn, &state, "lower-a");
        ratcn.handle_event(mouse(MouseKind::Moved, 1, 1), &state);
        render(&mut ratcn, &state, "lower-a");
        render(&mut ratcn, &state, "lower-b");

        // The pointer never left the top modal, and swapping the stack beneath
        // it does not move what the pointer is on.
        assert_eq!(
            rendered.lock().expect("hover log").last(),
            Some(&(true, true))
        );
        assert_eq!(ratcn.hover_path(), [ChildId::Static("top")]);
    }

    #[test]
    fn modal_transition_cancels_uncaptured_click_through_release() {
        let state = PointerState;
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let mut render = |ratcn: &mut Ratcn<PointerState, PointerMsg>, modal| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.render_component(
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
                                    PointerMsg::Routed(
                                        "modal",
                                        MouseKind::Click(MouseButton::Left),
                                        0,
                                    )
                                }),
                                area,
                            );
                        }
                    });
                })
                .expect("draw");
        };

        render(&mut ratcn, false);
        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state);
        render(&mut ratcn, true);
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 1, 0), &state),
            EventResult::Consumed
        );

        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state);
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 1, 0), &state),
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
        let left = Arc::new(Mutex::new(Vec::new()));
        let right = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn =
            Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    for (id, rendered) in [("left", &left), ("right", &right)] {
                        let rendered = Arc::clone(rendered);
                        ctx.scope(
                            ChildId::Static(id),
                            area,
                            ScopeOptions::default(),
                            move |ctx| {
                                ctx.render_component(
                                    ChildId::Static("shared"),
                                    FocusLeaf::recording(rendered),
                                    area,
                                );
                            },
                        );
                    }
                });
            })
            .expect("draw");

        assert_eq!(*left.lock().expect("left log"), [(true, true)]);
        assert_eq!(*right.lock().expect("right log"), [(false, false)]);
    }

    #[test]
    fn repeated_descendant_ids_render_hover_only_on_the_complete_path() {
        let state = PointerState;
        let left = Arc::new(Mutex::new(Vec::new()));
        let right = Arc::new(Mutex::new(Vec::new()));
        let mut ratcn = Ratcn::<PointerState, PointerMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let mut render = |ratcn: &mut Ratcn<PointerState, PointerMsg>| {
            terminal
                .draw(|frame| {
                    ratcn.render(frame, &state, &theme, |ctx| {
                        for (id, x, rendered) in [("left", 0, &left), ("right", 5, &right)] {
                            let rendered = Arc::clone(rendered);
                            let area = Rect::new(x, 0, 5, 2);
                            ctx.scope(
                                ChildId::Static(id),
                                area,
                                ScopeOptions::default(),
                                move |ctx| {
                                    ctx.render_component(
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
                })
                .expect("draw");
        };

        render(&mut ratcn);
        ratcn.handle_event(mouse(MouseKind::Moved, 1, 0), &state);
        left.lock().expect("left log").clear();
        right.lock().expect("right log").clear();
        render(&mut ratcn);

        assert_eq!(
            ratcn.hover_path(),
            [ChildId::Static("left"), ChildId::Static("shared")]
        );
        assert_eq!(*left.lock().expect("left log"), [(true, true)]);
        assert_eq!(
            *right.lock().expect("right log"),
            [(false, false)],
            "the same leaf id under another parent is a different component"
        );
    }

    #[test]
    fn modal_mismatch_release_clears_the_pre_transition_press() {
        let mut state = ModalTestState::default();
        let mut ratcn = Ratcn::new()
            .focus(|state: &ModalTestState| &state.focus, |_| unreachable!())
            .modals(|state: &ModalTestState| &state.modals);
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        let theme = Theme::default_dark();
        let mut render = |ratcn: &mut Ratcn<ModalTestState, ModalTestMsg>,
                          state: &ModalTestState| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, state, &theme, |ctx| {
                        ctx.render_component(
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
                })
                .expect("draw");
        };

        render(&mut ratcn, &state);
        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state);
        state
            .modals
            .open("dialog", &mut state.focus)
            .expect("open dialog");
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 1, 0), &state),
            EventResult::Consumed
        );

        render(&mut ratcn, &state);
        ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state);
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 1, 0), &state),
            EventResult::Emit(ModalTestMsg::Routed("dialog"))
        );

        state.modals.close(&mut state.focus).expect("close dialog");
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state),
            EventResult::Consumed
        );
        state
            .modals
            .open("dialog", &mut state.focus)
            .expect("reopen before redraw");
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 1, 0), &state),
            EventResult::Consumed
        );
    }
}
