//! The interaction runtime: what a frame declared, how it is painted, and
//! where events go.
//!
//! Three types carry that, in the order they appear here. [`Surface`] is the
//! retained tree — one node per declaration, holding its identity path,
//! geometry, layer, viewport, and component instance — and answers every
//! question about what exists and where. [`RenderPass`] builds the next
//! surface as the declaration closure runs, queues the paint each declaration
//! owes, and commits only a pass that finished cleanly. [`Ratcn`] owns the
//! committed surface, holds the app's focus and modal bindings, and routes
//! input against it.
//!
//! Supporting types sit with the one that uses them: viewports and
//! projections before [`Surface`], and the paint queue before [`RenderPass`].
//! What the pointer is doing between a press and its release lives in
//! [`gesture`](super::gesture), which [`Ratcn`] drives.

use std::{collections::HashMap, fmt, ops::Range};

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Position, Rect},
};

use crate::Theme;
use crate::backdrop::dim_background;

use super::{
    ChildId, Component, DeclareCtx, Event, EventCtx, EventResult, FocusState, KeyEvent, ModalState,
    MouseButton, MouseEvent, MouseKind, PaintCtx, ScopeOptions, Step, TabWrap,
    component::{InteractionFlags, PaintRoute, PaintTarget, PointerInputs, TransientMap},
    focus,
    gesture::Gestures,
};

// The largest rectangle a viewport declares as its content, and the largest a
// single paint inside one or inside a layer covers: a paint becomes a scratch
// buffer of one Ratatui cell per cell.
pub(crate) const MAX_VIEWPORT_CELLS: u32 = 262_144;

type ModalRead<State> = Box<dyn Fn(&State) -> &ModalState>;

struct FocusBinding<State, Msg> {
    read: Box<dyn Fn(&State) -> &FocusState>,
    on_change: Box<dyn Fn(FocusState) -> Msg>,
}

/// One vertical logical-content coordinate space projected into a screen rect.
///
/// The logical content shares the screen rectangle's origin and width and is
/// `content_height` rows tall. `offset` is the first content row on screen, so
/// the whole projection is a row shift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Viewport {
    screen: Rect,
    content_height: u16,
    offset: u16,
}

/// How much of a node its viewport shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewportVisibility {
    /// No viewport clips this node, or every row of it is on screen.
    Full,
    /// Some rows are on screen.
    Partial,
    /// No row is on screen.
    Hidden,
}

impl Viewport {
    /// The full logical allocation descendants are declared against.
    fn content(self) -> Rect {
        Rect::new(
            self.screen.x,
            self.screen.y,
            self.screen.width,
            self.content_height,
        )
    }

    /// The part of the screen rectangle the content covers. Content shorter
    /// than the rectangle leaves the rows past its end to what is beneath.
    fn visible_screen(self) -> Rect {
        Rect::new(
            self.screen.x,
            self.screen.y,
            self.screen.width,
            self.screen.height.min(self.content_height),
        )
    }

    /// The logical rows the screen rectangle shows at this offset.
    fn visible_content(self) -> Rect {
        Rect::new(
            self.screen.x,
            self.screen.y.saturating_add(self.offset),
            self.screen.width,
            self.screen.height,
        )
        .intersection(self.content())
    }

    fn visibility(self, area: Rect) -> ViewportVisibility {
        let visible = area.intersection(self.visible_content());
        if visible == area {
            ViewportVisibility::Full
        } else if visible.is_empty() {
            ViewportVisibility::Hidden
        } else {
            ViewportVisibility::Partial
        }
    }

    /// The one screen-to-logical translation: a row moves down by the offset
    /// and a column stays put. `None` past the coordinate limit.
    fn to_logical(self, point: Position) -> Option<Position> {
        Some(Position::new(point.x, point.y.checked_add(self.offset)?))
    }

    /// [`Self::to_logical`] for content this viewport clips: a point counts
    /// only over the rows the viewport shows.
    fn visible_to_logical(self, point: Position) -> Option<Position> {
        self.visible_screen()
            .contains(point)
            .then(|| self.to_logical(point))
            .flatten()
    }

    /// [`Self::to_logical`], clamped to the last representable row for the
    /// callers that owe an answer for every point.
    fn to_logical_clamped(self, point: Position) -> Position {
        self.to_logical(point)
            .unwrap_or_else(|| Position::new(point.x, u16::MAX))
    }

    fn mouse_to_logical(self, mouse: MouseEvent) -> MouseEvent {
        let point = self.to_logical_clamped(Position::new(mouse.column, mouse.row));
        MouseEvent {
            column: point.x,
            row: point.y,
            ..mouse
        }
    }

    /// How paint declared inside this viewport reaches the screen: shifted
    /// up by the offset, onto the rows it shows.
    fn projection(self) -> Projection {
        Projection {
            offset: self.offset,
            clip: self.visible_screen(),
        }
    }

    /// `area` with this viewport's scroll undone: the screen rectangle those
    /// logical rows sit at, unclipped — above or below the viewport's own
    /// rows included. Only the coordinate origin holds a row the offset would
    /// carry past it.
    fn unscrolled(self, area: Rect) -> Rect {
        Rect {
            y: area.y.saturating_sub(self.offset),
            ..area
        }
    }

    /// The frame rectangle in this viewport's logical coordinates.
    fn logical_frame(self, frame: Rect) -> Rect {
        Rect {
            y: frame.y.saturating_add(self.offset),
            ..frame
        }
    }
}

/// How one paint reaches the frame when it may not write straight onto it:
/// laid out in its own coordinates, shifted up by `offset` rows, and kept
/// only where it lands inside `clip`.
///
/// Paint inside a viewport carries the viewport's offset and the rows it
/// shows; layer paint carries the render area as its clip, so what a layer
/// paints never reaches past it. See [`PaintTarget`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Projection {
    offset: u16,
    clip: Rect,
}

impl Projection {
    /// Paint in screen coordinates, kept inside `clip`.
    pub(crate) const fn clipped(clip: Rect) -> Self {
        Self { offset: 0, clip }
    }

    /// The same projection, reaching no further than `bound` as well.
    fn within(self, bound: Rect) -> Self {
        Self {
            clip: self.clip.intersection(bound),
            ..self
        }
    }

    /// The screen rectangle paint through this reaches.
    pub(crate) const fn clip(self) -> Rect {
        self.clip
    }

    /// A rectangle in paint coordinates, in screen coordinates and clipped.
    /// Rows that project above the screen are dropped.
    fn project_rect(self, area: Rect) -> Rect {
        let above = self.offset.saturating_sub(area.y);
        if above >= area.height {
            return Rect::ZERO;
        }
        // `max` then subtract: both operands are at least `offset`.
        let projected = Rect::new(
            area.x,
            area.y.max(self.offset) - self.offset,
            area.width,
            area.height - above,
        )
        .intersection(self.clip);
        if projected.is_empty() {
            Rect::ZERO
        } else {
            projected
        }
    }

    /// Every cell of `area` this projection keeps, paired with the screen
    /// cell it lands on.
    pub(crate) fn projected_positions(
        self,
        area: Rect,
    ) -> impl Iterator<Item = (Position, Position)> {
        let offset = self.offset;
        self.project_rect(area).positions().map(move |screen| {
            (
                Position::new(screen.x, screen.y.saturating_add(offset)),
                screen,
            )
        })
    }
}

/// One declared viewport, and where it sits in the tree being built.
struct ViewportRecord {
    viewport: Viewport,
    /// The declaration that opened it. `None` when the root closure declared
    /// it, which no component owns and so nothing can be asked to scroll.
    owner: Option<usize>,
}

/// What the finished tree resolved this frame: the node paint styles as
/// focused, and the node the pointer rests on, each `None` where there is
/// none.
///
/// Both are answered once declaring has ended, from the tree the pass built,
/// and both travel into the replay together because every paint reads them
/// together.
#[derive(Debug, Clone, Copy)]
struct Resolved {
    focus: Option<usize>,
    hover: Option<usize>,
}

/// What asking to reveal focus in a tree came to.
enum Reveal {
    /// The tree does not declare the focused path; a later one may.
    Absent,
    /// Nothing scrolled: no focus, a target on screen, no owner to ask, or
    /// an owner that left its offset where it was.
    Settled,
    /// The viewport's owner moved its offset.
    Scrolled,
}

enum FocusAdvance {
    Move(FocusState),
    Consumed,
    Ignored,
}

/// What kind of layer a node roots, when it roots one.
///
/// A layer is a subtree painted above everything declared outside it. Every
/// kind shares that mechanism — a tag, paint order, and screen coordinates:
/// a layer undoes the scroll of the viewport that declared it once, over its
/// own area, and declares from there in screen coordinates. The kinds differ
/// only in what they do to interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayerKind {
    /// Takes the screen over: what lies beneath it dims, events landing
    /// outside it are consumed, focus resolves into it, Tab is trapped at its
    /// root, and keys stop there too.
    Modal,
    /// Holds the pointer over its own footprint and, when it carries a
    /// dismiss hook, emits it on a press outside itself — but never steals
    /// focus and lets keys reach its declarer. Anchored: skipped while its
    /// declaration is scrolled out of sight.
    Popup,
    /// Says something and takes nothing: not a pointer target, so a press
    /// goes to whatever it covers, and not a focus target, so Tab passes it by
    /// even if what is inside claims to be focusable. Anchored, like a popup.
    Hint,
}

impl LayerKind {
    /// Whether this kind takes the screen over — see [`Self::Modal`].
    const fn takes_over(self) -> bool {
        matches!(self, Self::Modal)
    }

    /// Whether this kind is inert: neither the pointer nor focus can land
    /// inside it — see [`Self::Hint`].
    const fn inert(self) -> bool {
        matches!(self, Self::Hint)
    }
}

pub(crate) struct Node<State, Msg> {
    /// Where this node's identity path sits in [`Surface::path_ids`]: the
    /// whole path, outermost first, ending in the node's own id.
    path: Range<usize>,
    parent: Option<usize>,
    /// One past the last index of this node's subtree. Declaration order is
    /// pre-order, so the subtree is exactly `index..subtree_end`.
    subtree_end: usize,
    area: Rect,
    /// Index into [`Surface::viewports`] of the innermost viewport this node
    /// was declared inside.
    viewport: Option<usize>,
    options: ScopeOptions,
    /// The component declared here; `None` for a scope, which is kept for
    /// identity and parents a subtree.
    component: Option<Box<dyn Component<State, Msg>>>,
    /// The layer this node was declared on, indexing [`Surface::layers`].
    /// `None` outside any layer.
    layer: Option<usize>,
    /// Whether this node takes part in the frame's interaction at all: every
    /// ancestor does, and it is a scope or has geometry to occupy. Settled by
    /// [`Surface::finish`], like `focusable`.
    live: bool,
    /// Whether focus can land anywhere in this subtree — on the node itself
    /// or on any descendant. [`Surface::takes_focus`] answers for the node
    /// alone.
    focusable: bool,
    /// Whether focus comes to rest here: the node takes focus itself and no
    /// descendant can. Every descent ends on one, and traversal steps
    /// between them in declaration order.
    focus_leaf: bool,
}

impl<State, Msg> fmt::Debug for Node<State, Msg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Node")
            .field("path", &self.path)
            .field("parent", &self.parent)
            .field("subtree_end", &self.subtree_end)
            .field("area", &self.area)
            .field("viewport", &self.viewport)
            .field("options", &self.options)
            .field("component", &self.component.is_some())
            .field("layer", &self.layer)
            .field("live", &self.live)
            .field("focusable", &self.focusable)
            .field("focus_leaf", &self.focus_leaf)
            .finish()
    }
}

/// One declared layer: the node rooting its subtree, where it sits, and what
/// it does.
struct Layer<Msg> {
    root: usize,
    kind: LayerKind,
    /// The area it was declared over, in screen coordinates: what a layer
    /// taking the screen over dims beneath itself.
    area: Rect,
    /// The message a press outside the layer emits: only ever a popup's, and
    /// only when the app bound one.
    on_dismiss: Option<Box<dyn Fn() -> Msg>>,
}

pub(crate) struct Surface<State, Msg> {
    nodes: Vec<Node<State, Msg>>,
    /// Scoped identity lookup: a node by its parent and its own id.
    child_index: HashMap<(Option<usize>, ChildId), usize>,
    /// Every layer, in declaration order, indexed by the layer number nodes
    /// carry. Nesting appends, so scanning backwards reaches the topmost
    /// first.
    layers: Vec<Layer<Msg>>,
    /// Every viewport declared this pass, in declaration order.
    viewports: Vec<ViewportRecord>,
    /// Every node's identity path, laid end to end, each indexed by its
    /// node's `path`.
    path_ids: Vec<ChildId>,
    /// The root of the layer that has taken the screen over, if one is open:
    /// everything outside it is inert, unfocusable, and unreachable by a key.
    /// Settled by [`Self::finish`].
    takeover: Option<usize>,
}

impl<State, Msg> Default for Surface<State, Msg> {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            child_index: HashMap::new(),
            layers: Vec::new(),
            viewports: Vec::new(),
            path_ids: Vec::new(),
            takeover: None,
        }
    }
}

impl<State, Msg> fmt::Debug for Surface<State, Msg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Surface")
            .field("nodes", &self.nodes.len())
            .field("child_index", &self.child_index.len())
            .field("layers", &self.layers.len())
            .field("viewports", &self.viewports.len())
            .field("path_ids", &self.path_ids.len())
            .field("takeover", &self.takeover)
            .finish()
    }
}

impl<State, Msg> Surface<State, Msg> {
    /// Answer, once the tree is complete, every per-node question that needs
    /// the whole of it: which nodes are live, which subtrees focus can land
    /// in, and which layer has taken the screen over. The predicates below
    /// then read these facts instead of walking the tree.
    fn finish(&mut self) {
        self.takeover = self
            .top_layer(|layer| layer.kind.takes_over())
            .map(|layer| layer.root);
        // Every parent precedes its children, so one forward walk settles
        // liveness down the tree and one reverse walk settles focusability up
        // it.
        for index in 0..self.nodes.len() {
            let node = &self.nodes[index];
            let live = (node.component.is_none() || !node.area.is_empty())
                && node.parent.is_none_or(|parent| self.nodes[parent].live);
            self.nodes[index].live = live;
        }
        for index in (0..self.nodes.len()).rev() {
            // So far `focusable` holds whether any child is.
            let beneath = self.nodes[index].focusable;
            let takes_focus = self.takes_focus(index);
            let node = &mut self.nodes[index];
            node.focusable = takes_focus || beneath;
            node.focus_leaf = takes_focus && !beneath;
            if let (true, Some(parent)) = (node.focusable, node.parent) {
                self.nodes[parent].focusable = true;
            }
        }
    }

    /// The identity path of `index`, outermost first.
    fn path_of(&self, index: usize) -> &[ChildId] {
        &self.path_ids[self.nodes[index].path.clone()]
    }

    /// `index`'s own segment of its identity path.
    fn id_of(&self, index: usize) -> &ChildId {
        &self.path_ids[self.nodes[index].path.end - 1]
    }

    /// Whether `index`'s identity path is `path` or a prefix of it.
    fn path_is_prefix_of(&self, index: usize, path: &[ChildId]) -> bool {
        path.starts_with(self.path_of(index))
    }

    /// Where `index` sits in this frame's resolved focus and hover — the four
    /// flags [`PaintCtx`] reports.
    fn interaction_flags(&self, index: usize, resolved: Resolved) -> InteractionFlags {
        let (focused, contains_focus) = self.leaf_match(index, resolved.focus);
        let (hovered, contains_hover) = self.leaf_match(index, resolved.hover);
        InteractionFlags {
            focused,
            contains_focus,
            hovered,
            contains_hover,
        }
    }

    /// Whether `leaf` *is* `index`, and whether it lies in `index`'s subtree
    /// — the leaf question and the within question, as a pair.
    fn leaf_match(&self, index: usize, leaf: Option<usize>) -> (bool, bool) {
        leaf.map_or((false, false), |leaf| {
            (leaf == index, self.inside(leaf, index))
        })
    }

    /// Whether `index` takes part in this frame's interaction at all: it and
    /// every ancestor are still declared, and it has geometry to occupy.
    fn present(&self, index: usize) -> bool {
        self.nodes[index].live && !self.nodes[index].area.is_empty()
    }

    /// Whether the pointer can land on `index`: present, inside whatever layer
    /// has taken the screen over, on a layer the pointer reaches, and not
    /// scrolled out of its viewport. The one eligibility test behind
    /// hit-testing and hover.
    fn hittable(&self, index: usize) -> bool {
        self.present(index)
            && self.interactive(index)
            && !self.on_inert_layer(index)
            && self.viewport_visibility(index) != ViewportVisibility::Hidden
    }

    /// Whether this node is inside the layer that has taken the screen over,
    /// and so can still be interacted with. With no such layer open,
    /// everything can.
    ///
    /// Membership is containment in the tree, not layer number: a layer
    /// declared after the one that took over takes a higher layer number
    /// without being inside it. Layer numbers order paint; this orders
    /// interaction, and only the ancestor chain can answer it. Checked on
    /// interaction targets (hit, focus leaves), not on ancestors: a nested
    /// layer root's ancestors provide identity and structure, not interaction.
    fn interactive(&self, index: usize) -> bool {
        self.takeover.is_none_or(|root| self.inside(index, root))
    }

    /// Whether `index` was declared on an inert layer, which neither the
    /// pointer nor focus reaches. Nothing outside any layer is inert.
    fn on_inert_layer(&self, index: usize) -> bool {
        self.nodes[index]
            .layer
            .is_some_and(|layer| self.layers[layer].kind.inert())
    }

    /// The topmost open layer that satisfies `wants`.
    fn top_layer(&self, wants: impl Fn(&Layer<Msg>) -> bool) -> Option<&Layer<Msg>> {
        self.layers.iter().rev().find(|layer| wants(layer))
    }

    /// Whether layer `index` lies beneath the layer that has taken the screen
    /// over, when one is open.
    fn covered_by_takeover(&self, index: usize, takeover: Option<usize>) -> bool {
        takeover.is_some_and(|root| !self.inside(self.layers[index].root, root))
    }

    /// Every modal root, outermost first — what `Ratcn::modals` validates the
    /// app's stack against.
    fn modal_roots(&self) -> impl Iterator<Item = usize> + '_ {
        self.layers
            .iter()
            .filter(|layer| layer.kind.takes_over())
            .map(|layer| layer.root)
    }

    /// Whether `index` is `root` or one of its descendants.
    ///
    /// The one containment test, answered by the subtree's index range:
    /// identity paths are unique, so this is what comparing paths would say,
    /// without touching an id. Layer numbers order paint and must never be
    /// used to answer it — a layer declared after another takes a higher
    /// number without being inside it.
    fn inside(&self, index: usize, root: usize) -> bool {
        self.subtree(root).contains(&index)
    }

    /// The node `path` names, or `None` when this surface does not declare it
    /// whole. The empty path names no node: every declaration has at least its
    /// own id.
    fn leaf_of(&self, path: &[ChildId]) -> Option<usize> {
        let mut parent = None;
        for id in path {
            parent = Some(*self.child_index.get(&(parent, id.clone()))?);
        }
        parent
    }

    /// The node indices along `path`, outermost first, stopping at the first
    /// segment this surface does not declare. A result shorter than `path` is
    /// how callers detect a path that no longer resolves.
    fn nodes_along_path(&self, path: &[ChildId]) -> Vec<usize> {
        let mut parent = None;
        let mut matched = Vec::new();
        for id in path {
            let Some(&index) = self.child_index.get(&(parent, id.clone())) else {
                break;
            };
            matched.push(index);
            parent = Some(index);
        }
        matched
    }

    /// The topmost interactive node under `point`, across all layers at or
    /// above the floor: highest layer wins, then latest declaration within it.
    /// This is target *selection* — an event never falls through geometry to a
    /// lower layer; it routes to this one node and bubbles up its ancestors.
    fn hit_index(&self, point: Position) -> Option<usize> {
        let mut best: Option<(Option<usize>, usize)> = None;
        for (index, node) in self.nodes.iter().enumerate() {
            if self.hittable(index)
                && self
                    .logical_point(index, point)
                    .is_some_and(|point| node.area.contains(point))
            {
                let key = (node.layer, index);
                if best.is_none_or(|best| key > best) {
                    best = Some(key);
                }
            }
        }
        best.map(|(_, index)| index)
    }

    fn is_layer_root(&self, index: usize) -> bool {
        self.layers.iter().any(|layer| layer.root == index)
    }

    /// The ancestor chain a mouse event at `path` bubbles through: from the
    /// innermost enclosing layer root down to the hit node. A layer confines
    /// its pointer events — they are consumed at its root rather than
    /// delivered to the occluded content beneath or to the component that
    /// declared the layer. Such a chain starts at that root, which is how
    /// [`Ratcn::route_mouse`] recognizes the boundary.
    fn mouse_bubble_chain(&self, path: &[ChildId]) -> Vec<usize> {
        let mut matched = self.nodes_along_path(path);
        if let Some(position) = matched.iter().rposition(|&index| self.is_layer_root(index)) {
            matched.drain(..position);
        }
        matched
    }

    /// Whether this node can hold focus itself, right now.
    ///
    /// The effective answer, not the claim: [`ScopeOptions::focusable`] is
    /// only what the declaration asked for, and it is the last of five
    /// conditions checked here. The node must also still be part of the tree,
    /// have hit geometry, sit inside the layer that has taken the screen over
    /// if one is open, and belong to a layer that allows focus at all.
    ///
    /// See [`Node::focusable`] for the same question about a node *or any of
    /// its descendants*.
    fn takes_focus(&self, index: usize) -> bool {
        self.present(index)
            && self.interactive(index)
            && !self.on_inert_layer(index)
            && self.nodes[index].options.focusable
    }

    /// `index`'s subtree, as the index range declaration order gives it.
    fn subtree(&self, index: usize) -> Range<usize> {
        index..self.nodes[index].subtree_end
    }

    /// The first focus leaf in `range`, scanning declaration order forward or
    /// in reverse per `direction`.
    fn leaf_in(&self, range: Range<usize>, direction: Step) -> Option<usize> {
        let mut leaves = range.filter(|&index| self.nodes[index].focus_leaf);
        match direction {
            Step::Forward => leaves.next(),
            Step::Backward => leaves.next_back(),
        }
    }

    /// The focus that lands on `leaf`.
    fn focus_on(&self, leaf: usize) -> FocusState {
        FocusState::intent(self.path_of(leaf).iter().cloned())
    }

    /// The focus path produced by descending from `index` to the first focus
    /// leaf of its subtree in `direction`.
    ///
    /// The primitive every focus policy ends at: the path it answers with is
    /// a leaf this surface declares, reached on the surface's own terms.
    fn descend_focus(&self, index: usize, direction: Step) -> Option<FocusState> {
        self.leaf_in(self.subtree(index), direction)
            .map(|leaf| self.focus_on(leaf))
    }

    /// The first focus leaf of the whole tree in `direction` — the other
    /// primitive: an edge, with no request behind it. While a layer holds
    /// the screen that is its edge, since nothing outside it takes focus.
    fn edge_focus(&self, direction: Step) -> Option<FocusState> {
        self.leaf_in(0..self.nodes.len(), direction)
            .map(|leaf| self.focus_on(leaf))
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
    /// - Explicit no-focus stays unfocused, even with a takeover layer.
    /// - An otherwise empty path resolves to the first participating candidate,
    ///   descended to its first focusable leaf (startup focus).
    /// - A path naming a container descends to the container's first focusable
    ///   leaf.
    /// - A path this surface did not declare, or that is no longer focusable,
    ///   stays as it is — parked, never silently retargeted.
    /// - With a layer holding the screen, a declared path outside it resolves
    ///   into that layer; an absent path stays parked even then, so render and
    ///   routing agree on it.
    fn resolve_focus(&self, stored: &FocusState) -> FocusState {
        if stored.is_none() {
            return FocusState::none();
        }
        if let Some(root) = self.takeover
            && !self.path_is_prefix_of(root, stored.path())
        {
            // The layer steals focus from an empty path and from paths it
            // occludes — but an absent path stays parked, so render and
            // routing keep agreeing on it.
            if !stored.path().is_empty() && self.leaf_of(stored.path()).is_none() {
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
            return self.edge_focus(Step::Forward).unwrap_or_default();
        }
        let Some(target) = self.leaf_of(stored.path()) else {
            return stored.clone();
        };
        self.descend_focus(target, Step::Forward)
            .unwrap_or_else(|| stored.clone())
    }

    /// The focus one named path asks for: the leaf reached by descending from
    /// the node it names. `None` when this surface does not declare that node
    /// whole, or when nothing inside it can hold focus.
    ///
    /// The policy behind every explicit request — a
    /// [`focus_key`](Ratcn::focus_key) binding, [`Ratcn::focus_path`], a
    /// hover-focus boundary — where [`Self::resolve_focus`] answers for the
    /// path the app is holding.
    fn focus_at_path(&self, path: &[ChildId]) -> Option<FocusState> {
        let matched = self.nodes_along_path(path);
        if matched.len() != path.len() {
            return None;
        }
        // A descent that succeeds ends on a leaf that is live, and liveness
        // runs the whole parent chain, so every node along `path` is
        // focusable whenever this answers with one at all.
        self.descend_focus(*matched.last()?, Step::Forward)
    }

    /// The focus a pointer resting on `path` asks for, and `None` when it
    /// rests inside no [`hover_focus`](ScopeOptions::hover_focus) scope that
    /// wants it.
    ///
    /// The outermost such scope along the path wins, and one that already
    /// holds the focus is passed over: a pointer crossing from one pane into
    /// another hands focus to the pane it entered. A scope with no focusable
    /// leaf is passed over too, since [`Self::focus_at_path`] answers `None`
    /// for it. Motion is the most frequent event there is, so `stored` is
    /// resolved once a scope asking for focus turns up, and not before.
    fn focus_for_hover(
        &self,
        path: &[ChildId],
        stored: &FocusState,
        root_options: &ScopeOptions,
    ) -> Option<FocusState> {
        let matched = self.nodes_along_path(path);
        if matched.len() != path.len() {
            return None;
        }
        let mut resolved = None;

        // Each node's parent decides whether hover moves focus onto it: the
        // root options for the outermost, the node above it for the rest. The
        // innermost node's own options speak for children off this path, so
        // the pairing stops one short of them. `matched` resolved `path`
        // whole, so position `n` is the node named by `path[..=n]`.
        std::iter::once(root_options)
            .chain(matched.iter().map(|&parent| &self.nodes[parent].options))
            .take(path.len())
            .enumerate()
            .find_map(|(position, options)| {
                if !options.hover_focus {
                    return None;
                }
                let child_path = &path[..=position];
                let focus = resolved.get_or_insert_with(|| self.resolve_focus(stored));
                (!focus.path().starts_with(child_path))
                    .then(|| self.focus_at_path(child_path))
                    .flatten()
            })
    }

    /// The viewport that clips `index`, if one does. A layer leaves the
    /// viewport it was opened in, so what it declares is clipped only by a
    /// viewport of its own.
    fn clipping_viewport(&self, index: usize) -> Option<&ViewportRecord> {
        Some(&self.viewports[self.nodes[index].viewport?])
    }

    /// The viewport `index` was declared inside, and `None` where none is.
    fn viewport_of(&self, index: usize) -> Option<Viewport> {
        self.clipping_viewport(index).map(|record| record.viewport)
    }

    /// How much of `index` its viewport shows. The one answer behind pointer
    /// eligibility, anchor culling, and whether focus needs revealing.
    fn viewport_visibility(&self, index: usize) -> ViewportVisibility {
        self.clipping_viewport(index)
            .map_or(ViewportVisibility::Full, |record| {
                record.viewport.visibility(self.nodes[index].area)
            })
    }

    /// `point` in the coordinate space `index` was declared with.
    fn logical_point(&self, index: usize, point: Position) -> Option<Position> {
        self.viewport_of(index)
            .map_or(Some(point), |viewport| viewport.visible_to_logical(point))
    }

    fn next_focus(
        &self,
        focus: &FocusState,
        direction: Step,
        root_options: &ScopeOptions,
    ) -> FocusAdvance {
        // A path parked outside the layer holding the screen belongs to
        // nothing traversal may use: the scope holding it is covered, so
        // consulting its `tab_wrap` would let a wrapping pane swallow Tab
        // forever with the layer unreachable. Start from that layer's own
        // edge instead.
        if let Some(root) = self.takeover
            && !self.path_is_prefix_of(root, focus.path())
        {
            return self
                .descend_focus(root, direction)
                .map_or(FocusAdvance::Consumed, FocusAdvance::Move);
        }
        let matched = self.nodes_along_path(focus.path());
        let Some(&current) = matched.last() else {
            if let Some(next) = self.edge_focus(direction) {
                return FocusAdvance::Move(next);
            }
            // An absent path is parked in the root scope, which decides; the
            // empty path asks for nothing.
            let parked = !focus.path().is_empty();
            return if parked && root_options.tab_wrap == TabWrap::Wrap {
                FocusAdvance::Consumed
            } else {
                FocusAdvance::Ignored
            };
        };
        if matched.len() != focus.path().len() {
            // Parked beneath `current`: its own descendants come first, and a
            // wrapping `current` keeps the step even when it has none.
            let beneath = current + 1..self.nodes[current].subtree_end;
            if let Some(leaf) = self.leaf_in(beneath, direction) {
                return FocusAdvance::Move(self.focus_on(leaf));
            }
            if self.nodes[current].options.tab_wrap == TabWrap::Wrap {
                return FocusAdvance::Consumed;
            }
        }
        self.next_from(current, direction, root_options)
    }

    /// The next focus leaf past `start`, inside the window Tab wraps in,
    /// wrapping to the window's edge when there is none.
    ///
    /// The window is [`Self::wrap_window`]'s. With none, the whole tree is
    /// scanned and a step past its last leaf escapes as `Ignored`. Stepping
    /// backward skips `start`'s own ancestors: they precede it in declaration
    /// order, but hold it rather than come before it.
    ///
    /// A step that lands back on the node it started from is still a
    /// `Move`; the caller compares it against the current focus.
    fn next_from(
        &self,
        start: usize,
        direction: Step,
        root_options: &ScopeOptions,
    ) -> FocusAdvance {
        let window = self.wrap_window(start, root_options);
        let bounds = window.clone().unwrap_or(0..self.nodes.len());
        let next = match direction {
            Step::Forward => self.leaf_in(self.nodes[start].subtree_end..bounds.end, direction),
            Step::Backward => (bounds.start..start).rev().find(|&index| {
                self.nodes[index].focus_leaf && self.nodes[index].subtree_end <= start
            }),
        };
        match (next, window) {
            (Some(leaf), _) => FocusAdvance::Move(self.focus_on(leaf)),
            (None, Some(window)) => self
                .leaf_in(window, direction)
                .map_or(FocusAdvance::Consumed, |leaf| {
                    FocusAdvance::Move(self.focus_on(leaf))
                }),
            (None, None) => FocusAdvance::Ignored,
        }
    }

    /// The nodes Tab wraps within from `start`, walking outwards: the
    /// descendants of the innermost enclosing scope that wraps, the whole
    /// tree when only the root options do, and `None` when nothing does.
    ///
    /// The root of a layer holding the screen traps Tab regardless of where
    /// it sits in the tree, so reaching it closes the window over its own
    /// subtree before any scope above it is asked.
    fn wrap_window(&self, start: usize, root_options: &ScopeOptions) -> Option<Range<usize>> {
        let mut current = start;
        loop {
            if self.takeover == Some(current) {
                return Some(self.subtree(current));
            }
            match self.nodes[current].parent {
                Some(parent) if self.nodes[parent].options.tab_wrap == TabWrap::Wrap => {
                    return Some(parent + 1..self.nodes[parent].subtree_end);
                }
                Some(parent) => current = parent,
                None => {
                    return (root_options.tab_wrap == TabWrap::Wrap).then_some(0..self.nodes.len());
                }
            }
        }
    }
}

type PaintThunk<State> = Box<dyn FnOnce(&mut PaintCtx<'_, State>)>;

/// One entry of the frame's paint queue: what to draw, and where it lands.
///
/// Declaring and drawing are separate walks. The declaration walk queues these
/// in the order it reaches them and draws nothing;
/// [`RenderPass::replay_paint`] runs the queue afterwards, when the tree is
/// complete and focus has resolved. Order in the queue is therefore the paint
/// order, and a component is queued where it opens, so its own paint precedes
/// its descendants'.
struct QueuedPaint<State> {
    slot: PaintSlot,
    paint: DeclaredPaint<State>,
}

/// The layer an op paints on, and the viewport it paints through.
///
/// Both are fixed where the op was queued, so an op belongs to the layer that
/// was open at its declaration whatever is open at replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PaintSlot {
    /// The layer the op paints on, `None` for the base declaration.
    layer: Option<usize>,
    viewport: Option<Viewport>,
}

/// One declaration's own paint.
///
/// Both forms name the node whose position they paint at, because that node is
/// what their interaction flags are read from once focus has resolved. Only
/// the area is captured: it is a declaration fact, settled where the op was
/// queued, while the flags are not facts yet.
enum DeclaredPaint<State> {
    /// Call [`Component::paint`] on the component installed at this node.
    Node { index: usize, area: Rect },
    /// Run a closure queued through [`DeclareCtx::paint`]. `node` is the
    /// declaration it was reached from, or `None` at the root, which has no
    /// identity and therefore no flags.
    Thunk {
        node: Option<usize>,
        area: Rect,
        paint: PaintThunk<State>,
    },
}

impl<State> DeclaredPaint<State> {
    /// The declaration this paint belongs to, and so the one its interaction
    /// flags come from. `None` at the root, which has no identity.
    const fn node(&self) -> Option<usize> {
        match self {
            Self::Node { index, .. } => Some(*index),
            Self::Thunk { node, .. } => *node,
        }
    }

    const fn area(&self) -> Rect {
        match self {
            Self::Node { area, .. } | Self::Thunk { area, .. } => *area,
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
/// child chooses where it goes. Viewports also express `frame_area` in their
/// logical coordinates; the state and theme stay constant for the pass.
pub(crate) struct DeclarationEnv<'a, State> {
    /// The root area supplied to [`Ratcn::render`], expressed in the current
    /// declaration's coordinates: screen coordinates outside a viewport,
    /// logical coordinates inside one.
    pub(crate) frame_area: Rect,
    pub(crate) area: Rect,
    pub(crate) state: &'a State,
    pub(crate) theme: &'a Theme,
    pub(crate) transients: &'a TransientMap,
}

impl<State> Clone for DeclarationEnv<'_, State> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<State> Copy for DeclarationEnv<'_, State> {}

impl<'a, State> DeclarationEnv<'a, State> {
    /// The environment for a root declaration: the app's own closure, covering
    /// the supplied render area.
    fn root(
        frame_area: Rect,
        state: &'a State,
        theme: &'a Theme,
        transients: &'a TransientMap,
    ) -> Self {
        Self {
            frame_area,
            area: frame_area,
            state,
            theme,
            transients,
        }
    }

    /// The same environment for the declarations *inside* the node just
    /// opened, over `area`.
    const fn nested(self, area: Rect) -> Self {
        Self { area, ..self }
    }
}

pub(crate) struct RenderPass<State, Msg> {
    frame_area: Rect,
    /// Declaration settlements are published only after painting succeeds.
    pub(crate) settled_transients: TransientMap,
    surface: Surface<State, Msg>,
    parent_stack: Vec<usize>,
    /// Every paint this frame owes, in the order the declaration walk reached
    /// it, replayed by [`Self::replay_paint`] once the walk is over.
    paint_queue: Vec<QueuedPaint<State>>,
    /// Where the pointer is, and what it rests on, as the runtime knew both
    /// when this pass started. Hover is pre-frame data — it was resolved
    /// against the last committed surface — so unlike focus it can be read
    /// while declaring, by [`DeclareCtx::pointer_within`]. The position itself
    /// reaches paint as [`PaintCtx::hover_position`]; the path this frame
    /// resolves travels into [`Self::replay_paint`] as [`Resolved`], the way
    /// focus does.
    hover_position: Option<Position>,
    hover_path: Vec<ChildId>,
    /// Declaration regions entered and not yet left — see [`Self::guarded`].
    /// A region that unwinds is never left, so a pass one unwound in can
    /// never commit.
    open_regions: usize,
    /// The open viewport, indexing [`Surface::viewports`]. A viewport
    /// declared while one is open panics, so there is at most one.
    open_viewport: Option<usize>,
    /// The layers open in declaration nesting order, indexing
    /// [`Surface::layers`]; the innermost decides where the next paint
    /// belongs.
    layer_stack: Vec<usize>,
    /// The buffer clipped paint lays out in, shared by every paint call the
    /// frame makes — see [`PaintTarget`].
    scratch: Buffer,
}

impl<State, Msg> RenderPass<State, Msg> {
    fn new(frame_area: Rect) -> Self {
        Self {
            frame_area,
            surface: Surface::default(),
            settled_transients: HashMap::new(),
            parent_stack: Vec::new(),
            paint_queue: Vec::new(),
            hover_position: None,
            hover_path: Vec::new(),
            open_regions: 0,
            open_viewport: None,
            layer_stack: Vec::new(),
            scratch: Buffer::empty(Rect::ZERO),
        }
    }

    /// The layer currently being declared into.
    /// `None` outside any layer.
    fn current_layer(&self) -> Option<usize> {
        self.layer_stack.last().copied()
    }

    /// The identity path of the declaration currently being declared into —
    /// the key [`DeclareCtx::transient`] reads the transient store with.
    pub(crate) fn current_path(&self) -> Option<&[ChildId]> {
        let &index = self.parent_stack.last()?;
        Some(self.surface.path_of(index))
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
    /// [`Self::leave_node`].
    fn enter_node(&mut self, index: usize) {
        self.parent_stack.push(index);
    }

    /// Close the innermost open declaration: everything declared since it
    /// opened is its subtree.
    fn leave_node(&mut self) {
        let index = self
            .parent_stack
            .pop()
            .expect("a declaration closes only after it opened");
        self.surface.nodes[index].subtree_end = self.surface.nodes.len();
    }

    /// Queue one declaration's paint for the slot currently being declared
    /// into.
    fn queue(&mut self, paint: DeclaredPaint<State>) {
        self.paint_queue.push(QueuedPaint {
            slot: self.active_slot(),
            paint,
        });
    }

    /// The slot the declaration currently open paints in: the layer being
    /// declared into, and the open viewport, which clips its content to what
    /// it shows.
    fn active_slot(&self) -> PaintSlot {
        PaintSlot {
            layer: self.current_layer(),
            viewport: self
                .open_viewport
                .map(|index| self.surface.viewports[index].viewport),
        }
    }

    /// Where the pointer is in the coordinates `slot` paints in, and `None`
    /// where the viewport it carries does not show it.
    fn hover_in(&self, slot: PaintSlot) -> Option<Position> {
        let position = self.hover_position?;
        match slot.viewport {
            Some(viewport) => viewport.visible_to_logical(position),
            None => Some(position),
        }
    }

    /// Queue a closure registered through [`DeclareCtx::paint`], tagged with
    /// the declaration it was reached from so replay can read that node's
    /// flags.
    pub(crate) fn queue_thunk(
        &mut self,
        area: Rect,
        paint: impl FnOnce(&mut PaintCtx<'_, State>) + 'static,
    ) {
        let node = self.parent_stack.last().copied();
        self.queue(DeclaredPaint::Thunk {
            node,
            area,
            paint: Box::new(paint),
        });
    }

    /// Open a viewport, declare its content through `declare`, and close it.
    pub(crate) fn viewport(
        &mut self,
        screen: Rect,
        content_height: u16,
        offset: u16,
        mut env: DeclarationEnv<'_, State>,
        declare: impl FnOnce(&mut DeclareCtx<'_, State, Msg>),
    ) {
        self.guarded(|pass| {
            assert!(
                pass.open_viewport.is_none(),
                "a viewport cannot be declared inside another viewport"
            );
            let cells = u32::from(screen.width) * u32::from(content_height);
            assert!(
                cells <= MAX_VIEWPORT_CELLS,
                "viewport content is {cells} cells; the maximum is {MAX_VIEWPORT_CELLS}"
            );
            let viewport = Viewport {
                screen,
                content_height,
                offset: offset.min(content_height.saturating_sub(screen.height)),
            };
            pass.open_viewport = Some(pass.surface.viewports.len());
            pass.surface.viewports.push(ViewportRecord {
                viewport,
                owner: pass.parent_stack.last().copied(),
            });
            env.area = viewport.content();
            env.frame_area = viewport.logical_frame(pass.frame_area);
            pass.with_declare_ctx(env, declare);
            pass.open_viewport = None;
        });
    }

    /// Open a layer, declare its root subtree through `declare_root`, and
    /// close it.
    ///
    /// The single place the layer lifecycle is written, coordinates included.
    /// Every layer belongs to the screen, whatever its kind: it undoes the
    /// open viewport's scroll once — over its own area, and over the frame
    /// its subtree reads — and then declares with no viewport open at all, so
    /// what it declares is in screen coordinates and may open a viewport of
    /// its own. The viewport is back for whatever the declaration goes on to
    /// say after the layer.
    ///
    /// The layer is recorded before `declare_root` opens its root node, so
    /// the subtree beneath declares with the layer already in place.
    fn layer<'a>(
        &mut self,
        kind: LayerKind,
        on_dismiss: Option<Box<dyn Fn() -> Msg>>,
        mut env: DeclarationEnv<'a, State>,
        declare_root: impl FnOnce(&mut Self, DeclarationEnv<'a, State>),
    ) {
        let enclosing = self.open_viewport.take();
        if let Some(index) = enclosing {
            env.area = self.surface.viewports[index].viewport.unscrolled(env.area);
        }
        env.frame_area = self.frame_area;
        let layer = self.surface.layers.len();
        self.layer_stack.push(layer);

        let root = self.surface.nodes.len();
        self.surface.layers.push(Layer {
            root,
            kind,
            area: env.area,
            on_dismiss,
        });
        declare_root(self, env);
        assert!(
            self.surface
                .nodes
                .get(root)
                .is_some_and(|node| node.layer == Some(layer)),
            "a layer's root is the first node its declaration opens"
        );

        self.layer_stack.pop();
        self.open_viewport = enclosing;
    }

    /// Run `f` as one declaration region: if it unwinds — a panicking
    /// component, or the runtime's own validation — the region is never
    /// left, and the pass can never commit, no matter who catches the panic.
    /// Every entry point that runs user code or validates a declaration goes
    /// through here.
    pub(crate) fn guarded<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.open_regions += 1;
        let result = f(self);
        self.open_regions -= 1;
        result
    }

    fn begin_node(&mut self, id: ChildId, area: Rect, options: ScopeOptions) -> usize {
        let parent = self.parent_stack.last().copied();
        let index = self.surface.nodes.len();
        assert!(
            self.surface
                .child_index
                .insert((parent, id.clone()), index)
                .is_none(),
            "duplicate child id `{id}` in one declaration scope"
        );
        let start = self.surface.path_ids.len();
        if let Some(parent) = parent {
            let parent_path = self.surface.nodes[parent].path.clone();
            self.surface.path_ids.extend_from_within(parent_path);
        }
        self.surface.path_ids.push(id);
        let path = start..self.surface.path_ids.len();
        let layer = self.current_layer();
        let viewport = self.open_viewport;
        self.surface.nodes.push(Node {
            path,
            parent,
            subtree_end: index + 1,
            area,
            viewport,
            options,
            component: None,
            layer,
            live: false,
            focusable: false,
            focus_leaf: false,
        });
        index
    }

    pub(crate) fn component(
        &mut self,
        id: ChildId,
        component: impl Component<State, Msg> + 'static,
        env: DeclarationEnv<'_, State>,
    ) {
        let state = env.state;
        self.guarded(|pass| {
            let mut component: Box<dyn Component<State, Msg>> = Box::new(component);
            // Every claim the runtime needs before descendants exist is read
            // here, in this order: focus for the whole frame is decided in one
            // pass, so none of it may depend on what painting produces.
            let options = component.scope_options(state);
            let area = env.area;
            let interaction_area = component.interaction_area(area, state);
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
            let index = pass.begin_node(id, interaction_area, options);
            pass.enter_node(index);
            // Queued before the subtree declares, so the component's own
            // paint replays ahead of its descendants' — the paint-before-
            // children contract, kept by position in the queue rather than by
            // each component's care. `area` is the node's paint allocation,
            // which `Component::interaction_area` may have narrowed for the
            // node itself but never for what it draws.
            pass.queue(DeclaredPaint::Node { index, area });
            pass.with_declare_ctx(env.nested(area), |ctx| component.declare(ctx));
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
                .any(|index| self.surface.id_of(index) == id),
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
            pass.layer(LayerKind::Modal, None, env, |pass, env| {
                pass.component(id, component, env);
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
        declare: impl FnOnce(&mut DeclareCtx<'_, State, Msg>),
    ) {
        self.guarded(|pass| {
            pass.assert_unique_modal_id(&id);
            pass.layer(LayerKind::Modal, None, env, |pass, env| {
                pass.scope(id, options, env, declare);
            });
        });
    }

    /// Declare a layer whose root is an app-declared scope rather than a
    /// component: the popup and hint form of [`layer`](Self::layer).
    ///
    /// `on_dismiss` belongs to the caller rather than to the kind: only a
    /// popup carries one, and only when the app bound it.
    pub(crate) fn layer_scope(
        &mut self,
        id: ChildId,
        kind: LayerKind,
        options: ScopeOptions,
        on_dismiss: Option<Box<dyn Fn() -> Msg>>,
        env: DeclarationEnv<'_, State>,
        declare: impl FnOnce(&mut DeclareCtx<'_, State, Msg>),
    ) {
        debug_assert!(
            on_dismiss.is_none() || kind == LayerKind::Popup,
            "a dismiss hook on a layer kind that never dismisses"
        );
        self.guarded(|pass| {
            // Both kinds here are anchored, and an anchored layer goes where
            // its declaration goes, out of sight included.
            if !pass.current_viewport_anchor_visible() {
                return;
            }
            pass.layer(kind, on_dismiss, env, |pass, env| {
                pass.scope(id, options, env, declare);
            });
        });
    }

    /// Whether the declaration an anchored layer would hang from is on
    /// screen: the question that decides whether a popup or hint scrolled out
    /// of its viewport is declared at all.
    fn current_viewport_anchor_visible(&self) -> bool {
        self.parent_stack.last().is_none_or(|&parent| {
            self.surface.viewport_visibility(parent) != ViewportVisibility::Hidden
        })
    }

    pub(crate) fn scope(
        &mut self,
        id: ChildId,
        options: ScopeOptions,
        env: DeclarationEnv<'_, State>,
        declare: impl FnOnce(&mut DeclareCtx<'_, State, Msg>),
    ) {
        self.guarded(|pass| {
            let area = env.area;
            let index = pass.begin_node(id, area, options);
            pass.enter_node(index);
            pass.with_declare_ctx(env.nested(area), declare);
            pass.leave_node();
        });
    }

    /// Run one declaration closure over the current parent node. The single
    /// construction site for declaration [`DeclareCtx`]s: root, scope, modal,
    /// [`DeclareCtx::in_area`], and [`Component::declare`] all pass through
    /// here.
    ///
    /// A panic out of `declare` leaves open the [`Self::guarded`] region of
    /// the declaration entry point that is open — `scope`, `component`,
    /// `viewport`, or a layer entry, which carries a region of its own around
    /// the validation and lifecycle it owns — and so rejects the pass.
    pub(crate) fn with_declare_ctx(
        &mut self,
        env: DeclarationEnv<'_, State>,
        declare: impl FnOnce(&mut DeclareCtx<'_, State, Msg>),
    ) {
        let DeclarationEnv {
            frame_area,
            area,
            state,
            theme,
            transients,
        } = env;
        let hover_position = self.hover_in(self.active_slot());
        let mut ctx = DeclareCtx {
            frame_area,
            area,
            theme,
            hover_position,
            transients,
            pass: self,
            state,
        };
        declare(&mut ctx);
    }

    /// Run every queued op onto the frame with the flags the finished tree
    /// resolved: the base declaration's ops in declaration order, then each
    /// layer's in composite order.
    ///
    /// Layers are transparent — a layer covers what is beneath it only where
    /// its own ops write — so compositing is nothing but this order. Layers
    /// paint in declaration order, except that every layer outside the one
    /// that has taken the screen over paints before it: what the takeover
    /// covers is inert, and so must not paint above it either. The takeover
    /// dims what is beneath it immediately before its own ops run. Ops of one
    /// layer keep the order they were declared in.
    fn replay_paint(
        &mut self,
        buffer: &mut Buffer,
        state: &State,
        theme: &Theme,
        resolved: Resolved,
    ) {
        let mut layers: Vec<Vec<QueuedPaint<State>>> =
            self.surface.layers.iter().map(|_| Vec::new()).collect();
        for op in std::mem::take(&mut self.paint_queue) {
            match op.slot.layer {
                None => self.paint_op(op, buffer, state, theme, resolved),
                Some(layer) => layers[layer].push(op),
            }
        }
        let takeover = self.surface.takeover;
        let (covered, uncovered): (Vec<usize>, Vec<usize>) =
            (0..layers.len()).partition(|&index| self.surface.covered_by_takeover(index, takeover));
        for index in covered.into_iter().chain(uncovered) {
            let layer = &self.surface.layers[index];
            if layer.kind.takes_over() {
                dim_background(
                    buffer,
                    layer.area.intersection(self.frame_area),
                    theme.background,
                );
            }
            for op in std::mem::take(&mut layers[index]) {
                self.paint_op(op, buffer, state, theme, resolved);
            }
        }
    }

    /// Paint one declaration onto the frame, through the viewport and the
    /// layer clip its slot names, with the flags the finished tree resolved
    /// for it.
    fn paint_op(
        &mut self,
        QueuedPaint { slot, paint: op }: QueuedPaint<State>,
        buffer: &mut Buffer,
        state: &State,
        theme: &Theme,
        resolved: Resolved,
    ) {
        // Read before the component borrow below, which needs the surface
        // mutably. The root declaration has no node, and so no flags.
        let flags = op.node().map_or_else(InteractionFlags::default, |index| {
            self.surface.interaction_flags(index, resolved)
        });
        let hover_position = self.hover_in(slot);
        let route = match (slot.viewport, slot.layer) {
            (None, None) => PaintRoute::Direct,
            (Some(viewport), None) => PaintRoute::Projected(viewport.projection()),
            // Layer paint stays inside the render area, whatever it writes.
            (None, Some(_)) => PaintRoute::Clipped(self.frame_area),
            (Some(viewport), Some(_)) => {
                PaintRoute::Projected(viewport.projection().within(self.frame_area))
            }
        };
        let mut ctx = PaintCtx {
            target: PaintTarget::new(buffer, route, &mut self.scratch),
            theme,
            area: op.area(),
            flags,
            hover_position,
            state,
        };
        match op {
            // `assert_valid` saw every region close before replay, and a
            // component region closes only once its component is installed.
            DeclaredPaint::Node { index, .. } => self.surface.nodes[index]
                .component
                .as_deref_mut()
                .expect("a checked pass installed every node's component")
                .paint(&mut ctx),
            DeclaredPaint::Thunk { paint, .. } => paint(&mut ctx),
        }
    }

    /// Every reason to reject this pass, checked while nothing has painted
    /// yet.
    ///
    /// A closure the app runs directly — the root one, or one reached through
    /// [`DeclareCtx::in_area`] — carries no [`Self::guarded`] region of its
    /// own: a panic crossing it unwinds past this check and past the commit,
    /// while a panic a declaration inside it raises was recorded by that
    /// declaration's region before an app closure could catch it.
    ///
    /// Every open declaration, layer, and viewport, and every component still
    /// to be installed, sits inside a region, so regions all closing is also
    /// what says the tree is complete.
    fn assert_valid(&self) {
        assert!(
            self.open_regions == 0,
            "cannot commit a failed declaration pass"
        );
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
/// Two consequences follow:
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
/// known before the first cell is written. A declaration writes nothing
/// outside its pass — [`DeclareCtx::transient`] is staged until commit —
/// so a rejected pass leaves no trace. Two things cannot be taken back: the
/// offset a [`Component::reveal_in_viewport`] stored for a frame whose second
/// declaration is then rejected, and a panic thrown by painting itself, after
/// the pass had already been accepted.
pub struct Ratcn<State, Msg> {
    surface: Surface<State, Msg>,
    /// Whether a declaration pass has ever committed. Until one has there is
    /// no surface to route through, and every event is ignored.
    has_rendered: bool,
    focus_binding: Option<FocusBinding<State, Msg>>,
    modal_binding: Option<ModalRead<State>>,
    root_options: ScopeOptions,
    /// What every button whose gesture is under way is doing, and what one
    /// raw mouse event becomes because of it.
    gestures: Gestures,
    transients: TransientMap,
    /// Where the pointer physically is, from the last mouse event. `None`
    /// until the first one, and again once the pointer leaves the terminal.
    pointer: Option<Position>,
    /// The identity path of whatever the pointer rests on, empty over empty
    /// space. Derived from `pointer` and the retained surface, and rewritten
    /// wherever either changes — pointer motion, and every commit.
    hover: Vec<ChildId>,
    /// The focus the retained surface resolved and painted. Comparing a fresh
    /// resolution against it is how a focus change is noticed, whoever made
    /// it.
    resolved_focus: FocusState,
    /// Whether a reveal is still waiting to be answered: focus parked on a
    /// path no surface has declared yet, or a reveal an event asked for
    /// outright. The frame that answers it clears it.
    reveal_pending: bool,
    /// The latest text an event put on the clipboard, until the host takes it.
    clipboard: Option<String>,
}

impl<State, Msg> fmt::Debug for Ratcn<State, Msg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ratcn")
            .field("surface", &self.surface)
            .field("has_rendered", &self.has_rendered)
            .field("focus_binding", &self.focus_binding.is_some())
            .field("modal_binding", &self.modal_binding.is_some())
            .field("root_options", &self.root_options)
            .field("gestures", &self.gestures)
            .field("transients", &self.transients.len())
            .field("pointer", &self.pointer)
            .field("hover", &self.hover)
            .field("resolved_focus", &self.resolved_focus)
            .field("reveal_pending", &self.reveal_pending)
            .field("clipboard", &self.clipboard)
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
            gestures: Gestures::default(),
            transients: HashMap::new(),
            pointer: None,
            hover: Vec::new(),
            resolved_focus: FocusState::default(),
            reveal_pending: false,
            clipboard: None,
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
    /// Binding it gives two guarantees:
    ///
    /// - **No events land on the wrong layer.** Between the message that opens
    ///   or closes a modal and the redraw that declares it, the retained
    ///   surface describes the layer before the change. While the two
    ///   disagree, events are consumed and nothing routes.
    /// - **Focus is correct on the modal's first frame.** Knowing the top modal
    ///   before declaration starts lets focus paint and event routing agree from
    ///   the start of the frame, rather than a lower layer painting focus and
    ///   the modal claiming it a frame later. A focus path outside the top modal
    ///   is pulled to that modal's root; a path already inside it is left
    ///   exactly as it is, parked or not.
    ///
    /// In exchange, every successful render must declare exactly these ids, in
    /// stack order, with [`DeclareCtx::modal`] — a mismatch panics rather than
    /// silently diverging.
    ///
    /// Apps using modals should bind this. Without it, [`DeclareCtx::modal`]
    /// still layers and routes correctly, but neither guarantee above applies.
    #[must_use]
    pub fn modals(mut self, read: impl Fn(&State) -> &ModalState + 'static) -> Self {
        self.modal_binding = Some(Box::new(read));
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

    /// The app-held focus, as stored; [`Surface::resolve_focus`] aligns it
    /// with the declared modal roots.
    fn stored_focus<'s>(&self, state: &'s State) -> &'s FocusState {
        self.focus_binding
            .as_ref()
            .map_or(&focus::UNRESOLVED, |binding| (binding.read)(state))
    }

    /// Declare and paint one frame, then keep it as the surface events route
    /// against.
    ///
    /// Call this once per frame, from inside ratatui's `Terminal::draw`. The
    /// `declare` closure is the whole UI for this frame: build components from
    /// `state`, place them with [`component`](DeclareCtx::component), group
    /// them with [`scope`](DeclareCtx::scope), queue whatever else you want
    /// drawn with [`paint_widget`](DeclareCtx::paint_widget) or
    /// [`paint`](DeclareCtx::paint). Nothing is retained between frames, so
    /// there is no widget tree to keep in sync — what you declare is what
    /// exists.
    ///
    /// The pass has to finish completely before it counts: declaration,
    /// component paint, runtime validation, and layer paint all have to
    /// succeed. Only then does the new surface replace the old one, and it
    /// happens in one step. A pass that panics or fails validation leaves the
    /// previous surface handling events, so a bad frame degrades interaction to
    /// "one frame stale" rather than breaking it — and it leaves the previous
    /// *frame* too. Declaration, validation, and the modal-stack check all
    /// finish before the first cell is written, so a rejected pass never
    /// reaches the screen. Only a panic thrown by painting itself, once the
    /// pass has been accepted, can leave cells behind.
    ///
    /// `area` is the root declaration area, in absolute frame coordinates.
    /// Pass `frame.area()` for a whole-frame app, or a pane's rectangle for a
    /// hosted tree. Choose an area within the frame; it is passed unchanged
    /// for layout, not silently clamped. Floating components read its bounds
    /// through [`DeclareCtx::frame_area`]; layer paint and modal backdrop
    /// dimming are clipped to it. Viewports retain their logical
    /// coordinate transforms, and input events still use screen coordinates.
    ///
    /// This is not a paint sandbox or a root hit-test boundary. Base-layer
    /// paint is not clipped to `area`: widgets can paint outside their rects,
    /// and [`PaintCtx::with_buffer`] gives base paint outside viewports and
    /// layers the whole destination buffer. The host still owns input routing between trees.
    ///
    /// This delegates to [`render_into`](Self::render_into) with the frame's
    /// buffer. Use that entry point for a caller-owned offscreen buffer rather
    /// than constructing a terminal just to obtain a frame.
    ///
    /// # Declaring, then drawing
    ///
    /// Nothing draws while the closure runs. It runs once, or twice when focus
    /// lands on content a viewport clips and the component that declared the
    /// viewport scrolls to reveal it: the first declaration is discarded for
    /// one built with the new offset. Keep side effects out of it.
    /// Declaration records what exists and where; [`Component::paint`] and the closures
    /// [`DeclareCtx::paint`] queues are replayed afterwards, in the order the
    /// declaration reached them. Focus and hover resolve in between, against
    /// the finished tree, so every interaction flag a paint reads is derived
    /// from a tree that is already complete — which is the whole reason the
    /// two walks are separate, and why [`DeclareCtx`] has no flags to offer.
    ///
    /// One consequence is worth stating plainly: **structure may not depend
    /// on the interaction flags**, because there are none to depend on while
    /// declaring. Which components exist, their ids, and their areas may
    /// depend on anything in `state`, app-held focus included — and on
    /// [`DeclareCtx::pointer_within`], which reports hover as it stood when
    /// the pass began rather than as this frame will resolve it.
    ///
    /// # Ordering within the pass
    ///
    /// Declaration order is meaningful. It sets Tab order, it sets paint
    /// order — a component draws before its own descendants — and it sets
    /// hit-testing order, with later declarations on top — within one layer.
    /// [`modal`](DeclareCtx::modal), [`popup`](DeclareCtx::popup), and
    /// [`hint`](DeclareCtx::hint) layers are exempt from paint order: every
    /// layer paints after the whole base declaration, in the order the layers
    /// were declared, so base content declared *after* a layer still paints
    /// beneath it. Layers may therefore be declared from anywhere in the
    /// tree, whenever their owner declares. Layers are transparent: a layer
    /// covers only the cells its content writes, so one that should hide what
    /// is beneath it paints a background (`Clear`, then a filled block).
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
        area: Rect,
        state: &State,
        theme: &Theme,
        declare: impl FnMut(&mut DeclareCtx<'_, State, Msg>),
    ) {
        self.render_into(frame.buffer_mut(), area, state, theme, declare);
    }

    /// Declare and paint into a caller-owned buffer, then retain the surface
    /// for event routing, just like [`render`](Self::render).
    ///
    /// Use this for offscreen content, such as a page taller than its visible
    /// window. For ordinary terminal drawing, prefer [`render`](Self::render).
    /// Both use the same declaration, validation, focus/hover resolution,
    /// painting, and commit lifecycle. A rejected declaration leaves the buffer
    /// untouched; a panic during painting can leave partial writes, but never
    /// replaces the previously retained surface.
    ///
    /// The caller allocates and, when needed, clears or resizes `buffer` before
    /// the call. This method does not clear it: cells paint leaves alone keep
    /// their previous contents. Choose `area` within `buffer.area`; it is passed
    /// unchanged for layout, with no containment validation or silent clamping.
    /// Areas use the buffer's absolute coordinates, including a nonzero origin,
    /// not coordinates relative to `area`. Viewports retain their logical
    /// transforms. The caller owns copying a visible window to the screen and
    /// translating screen pointer positions back into buffer coordinates before
    /// [`handle_event`](Self::handle_event).
    ///
    /// `area` supplies floating placement bounds and clips layer paint and
    /// modal dimming, not arbitrary base paint or root hit-testing. Base widgets
    /// can paint outside their rects, and [`PaintCtx::with_buffer`] in base
    /// paint outside viewports and layers receives the whole destination
    /// buffer, as it does with `render`.
    ///
    /// A bare buffer carries no cursor metadata, and this method reports no
    /// caret position. Future caret-bearing components may require a caret
    /// result from this API; the host would decide how to display it.
    ///
    /// # Panics
    ///
    /// The same declaration and component-paint failures as [`render`](Self::render).
    pub fn render_into(
        &mut self,
        buffer: &mut Buffer,
        area: Rect,
        state: &State,
        theme: &Theme,
        mut declare: impl FnMut(&mut DeclareCtx<'_, State, Msg>),
    ) {
        let focus_snapshot = self.stored_focus(state);
        let mut pass = self.declare_pass(area, state, theme, &mut declare);
        // Focus resolves once, over the finished tree, and only then does
        // anything learn where it landed.
        let mut resolved_focus = pass.surface.resolve_focus(focus_snapshot);
        // Reveal against the tree just declared: it is the one that knows
        // where the focused target sits, even when this frame declared it for
        // the first time. The answer is a transient the declaration reads, and
        // the offset it changes has already placed this tree's layers and
        // paint, so a reveal that scrolls declares the frame once more. Focus
        // parked on a path this tree lacks stays pending for one that has it.
        let mut reveal_pending = false;
        if self.reveal_pending || resolved_focus != self.resolved_focus {
            match self.reveal_focus(&mut pass.surface, &resolved_focus, state) {
                Reveal::Absent => reveal_pending = true,
                Reveal::Settled => {}
                Reveal::Scrolled => {
                    pass = self.declare_pass(area, state, theme, &mut declare);
                    resolved_focus = pass.surface.resolve_focus(focus_snapshot);
                    reveal_pending = pass.surface.leaf_of(resolved_focus.path()).is_none();
                }
            }
        }
        // Hover re-answers its own question against the tree — the pointer has
        // not moved, but what is under it may have — so paint reports this
        // frame's hover rather than the one the declaration was built from.
        let resolved_hover = self.resolve_hover(&pass.surface);
        let resolved = Resolved {
            focus: pass
                .surface
                .leaf_of(resolved_focus.path())
                .filter(|&target| pass.surface.takes_focus(target)),
            hover: pass.surface.leaf_of(&resolved_hover),
        };
        pass.replay_paint(buffer, state, theme, resolved);
        for (path, slots) in pass.settled_transients {
            self.transients.entry(path).or_default().extend(slots);
        }
        self.commit_surface(pass.surface, resolved_hover, resolved_focus, reveal_pending);
    }

    /// Declare and validate one pass. Nothing is drawn and no *focus* flag is
    /// read: the walk builds the tree and queues the paint it owes, and
    /// writes nothing outside the pass, so a pass can be dropped — rejected,
    /// or superseded by a reveal — without trace. Hover is the one
    /// interaction fact that predates the pass, so the declaration may ask
    /// for it — see [`DeclareCtx::pointer_within`].
    fn declare_pass(
        &self,
        area: Rect,
        state: &State,
        theme: &Theme,
        declare: &mut impl FnMut(&mut DeclareCtx<'_, State, Msg>),
    ) -> RenderPass<State, Msg> {
        let mut pass = RenderPass::new(area);
        pass.hover_position = self.pointer;
        pass.hover_path.clone_from(&self.hover);
        pass.with_declare_ctx(
            DeclarationEnv::root(area, state, theme, &self.transients),
            declare,
        );
        // Every reason to reject a pass is known once declaration ends, and
        // nothing has painted yet — so the checks run first and a rejected
        // pass never reaches the screen at all. They also establish what
        // `resolve_focus` needs: a complete tree.
        pass.assert_valid();
        pass.surface.finish();
        self.assert_modal_stack(&pass.surface, state);
        pass
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
        if self.gestures.in_flight()
            && surface
                .leaf_of(&self.hover)
                .is_some_and(|index| surface.hittable(index))
        {
            return self.hover.clone();
        }
        self.pointer
            .and_then(|position| surface.hit_index(position))
            .map(|index| surface.path_of(index).to_vec())
            .unwrap_or_default()
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
        let semantic = binding(state).ids();
        let declared = surface.modal_roots().map(|index| surface.id_of(index));
        assert!(
            semantic.clone().eq(declared),
            "declared modal roots do not match app-owned modal ids: expected {:?}",
            semantic.collect::<Vec<_>>()
        );
    }

    /// Publish `next` as the retained surface, and carry the cross-frame
    /// pointer and transient bookkeeping onto it.
    ///
    /// Everything here outlives a single frame and so has to be reconciled
    /// when the ground moves: a captured gesture's component may no longer be
    /// declared, transients are keyed by paths this surface may not contain,
    /// and the pointer may now rest on something else entirely — see
    /// [`resolve_hover`](Self::resolve_hover). A modal opening or
    /// closing is the disruptive case and gets its own treatment — every
    /// tracked gesture is abandoned rather than re-checked, because the layer
    /// that appeared or vanished changes what the pointer was ever over.
    fn commit_surface(
        &mut self,
        next: Surface<State, Msg>,
        hover: Vec<ChildId>,
        focus: FocusState,
        reveal_pending: bool,
    ) {
        let active_modal_changed = self
            .surface
            .modal_roots()
            .last()
            .map(|index| self.surface.id_of(index))
            != next.modal_roots().last().map(|index| next.id_of(index));

        // Dropped at the end: the previous components drop only after the
        // bookkeeping below has let go of the paths they owned.
        let previous = std::mem::replace(&mut self.surface, next);
        self.has_rendered = true;

        if active_modal_changed {
            self.gestures.cancel();
        } else {
            let Self {
                gestures, surface, ..
            } = self;
            gestures.cancel_lost_claims(|path| {
                surface.leaf_of(path).is_some_and(|index| {
                    surface.nodes[index].live
                        && surface.interactive(index)
                        && !surface.on_inert_layer(index)
                })
            });
        }
        self.transients
            .retain(|path, _| self.surface.leaf_of(path).is_some());
        // The hover, focus, and reveal this frame settled, published with the
        // surface they were resolved against — a pass that never got here
        // leaves the previous ones in charge, exactly as it leaves the
        // previous surface.
        self.hover = hover;
        self.reveal_pending = reveal_pending;
        self.resolved_focus = focus;
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
            ref event => self.route_to_focus(event, state),
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

    /// The latest text a component put on the clipboard while handling an
    /// event, taken: a second call answers `None` until another event writes.
    ///
    /// The host carries the write out after each
    /// [`handle_event`](Self::handle_event): natively with the `termina`
    /// feature's `Session::set_clipboard`, in the browser with the `ratzilla`
    /// feature's `BrowserClipboard`.
    #[must_use = "the text is gone from the runtime once taken"]
    pub fn take_clipboard(&mut self) -> Option<String> {
        self.clipboard.take()
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
    fn route_to_focus(&mut self, event: &Event, state: &State) -> EventResult<Msg> {
        let focus = self.surface.resolve_focus(self.stored_focus(state));
        let chain = self.key_bubble_chain(&focus);

        let routed = self.dispatch_chain(&chain, event, state);
        if !matches!(routed, EventResult::Ignored) {
            return routed;
        }

        let Event::Key(key) = event else {
            return EventResult::Ignored;
        };
        match key.traversal_step() {
            Some(direction) => {
                match self
                    .surface
                    .next_focus(&focus, direction, &self.root_options)
                {
                    FocusAdvance::Move(next) => self.focus_transition_result(next, &focus),
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
    /// at the root of the layer that has taken the screen over.
    ///
    /// Keys never cross such a layer outward. Bubbling stops at its root,
    /// which doubles as the layer-wide fallback for keys nothing inside
    /// handled. A popup's unhandled keys reach its declaring component. A
    /// hint is inert: stored intent beneath it may reach an outside ancestor,
    /// but never the hint's content.
    fn key_bubble_chain(&self, focus: &FocusState) -> Vec<usize> {
        let mut matched = self.surface.nodes_along_path(focus.path());
        // Focusability controls traversal, not fallback delivery: an open
        // Select with no options still needs Esc on its parked path. Layer
        // inertness, in contrast, blocks delivery regardless of the component.
        if let Some(position) = matched
            .iter()
            .position(|&index| self.surface.on_inert_layer(index))
        {
            matched.truncate(position);
        }
        let Some(takeover) = self.surface.takeover else {
            return matched;
        };
        match matched.iter().position(|&index| index == takeover) {
            Some(position) => {
                matched.drain(..position);
                matched
            }
            // Focus is parked outside the layer: either it has nothing
            // focusable to take the path over, or the stored path is absent
            // from this surface. The chain becomes that root alone — keeping
            // the outside chain would offer the key to the covered component
            // first, since the chain is walked deepest-first.
            None => vec![takeover],
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
        &mut self,
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
                let mut path =
                    scope.map_or_else(Vec::new, |index| self.surface.path_of(index).to_vec());
                path.extend(binding.path.iter().cloned());
                let Some(next) = self.surface.focus_at_path(&path) else {
                    continue;
                };
                return Some(self.focus_transition_result(next, focus));
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
            .then(|| self.surface.focus_at_path(path))
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
            let retained = self
                .surface
                .modal_roots()
                .map(|index| self.surface.id_of(index));
            binding(state).ids().eq(retained)
        })
    }

    /// Handle one *raw* mouse event: pointer bookkeeping, gesture synthesis,
    /// then delivery of everything that synthesis produced.
    ///
    /// Backends report `Down`, `Up`, and `Moved`; components consume `Click`,
    /// `Drag`, and `DragEnd`. [`Gestures::normalize`] bridges the two, so one
    /// raw event can expand into several normalized ones — a release becomes
    /// `Up` and then `Click` or `DragEnd`. Each is delivered in turn, but only
    /// until one emits: the app sees at most one message per raw event, the
    /// same contract the keyboard path keeps.
    ///
    /// Every event synthesis makes lands on the cell the raw event reported,
    /// and the surface cannot move while they are being delivered, so the one
    /// hit test taken here answers for all of them: what the release is judged
    /// against, what each event routes to, and what a press outside a popup
    /// dismisses.
    ///
    /// The gesture that a release ends is closed after the whole batch, never
    /// at the `Up` — see [`gesture`](super::gesture).
    fn handle_mouse(&mut self, raw: MouseEvent, state: &State) -> EventResult<Msg> {
        if raw.kind == MouseKind::Exited {
            return self.handle_pointer_exit();
        }
        self.observe_pointer(raw);
        let hit = self.hit_path(Position::new(raw.column, raw.row));
        let events = self.gestures.normalize(raw, hit.as_deref());
        // The one event synthesis drops is motion under a held button that has
        // not left its cell: nothing to deliver, but the app must not read it
        // as unhandled.
        let mut result = if events.is_empty() {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        };
        for mouse in events {
            let next = self.deliver_mouse(mouse, hit.as_deref(), state);
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
        if let MouseKind::Up(button) = raw.kind {
            self.gestures.end(button);
        }
        result
    }

    /// The pointer left the backend's grid: abandon every tracked gesture and
    /// clear hover. Nothing routes, so this always counts as handled.
    fn handle_pointer_exit(&mut self) -> EventResult<Msg> {
        self.gestures.forget_all();
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
    fn deliver_mouse(
        &mut self,
        mouse: MouseEvent,
        hit: Option<&[ChildId]>,
        state: &State,
    ) -> EventResult<Msg> {
        if self.gestures.swallows(mouse.kind) {
            return EventResult::Consumed;
        }
        let routed = self.route_mouse(mouse, hit, state);
        match (mouse.kind, &routed) {
            (MouseKind::Down(_), EventResult::Ignored | EventResult::Consumed) => {
                self.popup_dismissal(hit).map_or(routed, EventResult::Emit)
            }
            _ => routed,
        }
    }

    /// The stale-modal-window half of mouse handling: while the semantic modal
    /// stack disagrees with the retained one, events must not route, but the
    /// cross-event pointer bookkeeping still has to advance exactly as
    /// [`handle_mouse`](Self::handle_mouse) would advance it — the shared
    /// [`observe_pointer`](Self::observe_pointer) step plus a pass through
    /// [`Gestures::normalize`], whose output goes nowhere — or gestures
    /// desynchronize across the gap. Presses that start inside the gap are
    /// suppressed until their release.
    fn consume_mouse_without_routing(&mut self, raw: MouseEvent) {
        if raw.kind == MouseKind::Exited {
            self.pointer_gone();
            self.gestures.forget_all();
            return;
        }
        self.observe_pointer(raw);
        self.gestures.cancel();
        // Nothing routes across this gap, so the synthesized follow-up is
        // discarded; the gestures still have to advance exactly as they would
        // have.
        let hit = self.hit_path(Position::new(raw.column, raw.row));
        let _ = self.gestures.normalize(raw, hit.as_deref());
        if let MouseKind::Down(button) = raw.kind {
            self.gestures.suppress(button);
        }
        if let MouseKind::Up(button) = raw.kind {
            self.gestures.end(button);
        }
    }

    /// Where the pointer now is. Every non-exited mouse event records it,
    /// on the routing path and on the stale-modal-window consume path alike.
    fn observe_pointer(&mut self, raw: MouseEvent) {
        self.pointer = Some(Position::new(raw.column, raw.row));
    }

    /// The identity path of the topmost interactive node under `point`.
    fn hit_path(&self, point: Position) -> Option<Vec<ChildId>> {
        self.surface
            .hit_index(point)
            .map(|index| self.surface.path_of(index).to_vec())
    }

    /// Offer `event` to each component in `chain`, deepest first, stopping at
    /// the first that does not ignore it.
    ///
    /// The one dispatch loop. Keys and pointer events build different chains —
    /// keys from the focus path, the pointer from what it hit — but bubble
    /// through them identically, so this is where "unhandled events bubble up"
    /// is actually implemented. A pointer `Down` is the one event a
    /// component may claim the gesture of, through
    /// [`EventCtx::capture_pointer`]; the claim is recorded once the chain
    /// is done.
    fn dispatch_chain(
        &mut self,
        chain: &[usize],
        event: &Event,
        state: &State,
    ) -> EventResult<Msg> {
        let mouse = match event {
            Event::Mouse(mouse) => Some(*mouse),
            _ => None,
        };
        let pressed = mouse.and_then(|mouse| match mouse.kind {
            MouseKind::Down(button) => Some(button),
            _ => None,
        });
        let capture_owner = mouse
            .and_then(|mouse| self.gestures.capture_for(mouse.kind))
            .map(ToOwned::to_owned);
        let captured_press = mouse.and_then(|mouse| self.gestures.captured_press(mouse.kind));
        let mut claim = None;
        let mut result = EventResult::Ignored;
        for &index in chain.iter().rev() {
            if !self.surface.nodes[index].live {
                continue;
            }
            let path = self.surface.path_of(index).to_vec();
            let area = self.surface.nodes[index].area;
            let viewport = self.surface.viewport_of(index);
            // Declaration-space for the component, screen-absolute for the
            // gesture tracker `EventCtx::drag` keeps.
            let projected = match (event, viewport) {
                (Event::Mouse(mouse), Some(viewport)) => {
                    Some(Event::Mouse(viewport.mouse_to_logical(*mouse)))
                }
                _ => None,
            };
            let delivered = projected.as_ref().unwrap_or(event);
            let owns_capture = capture_owner.as_deref().is_some_and(|owner| path == owner);
            let Some(component) = self.surface.nodes[index].component.as_mut() else {
                continue;
            };
            let mut ctx = EventCtx::at(
                path,
                area,
                &mut self.transients,
                PointerInputs {
                    claim: pressed.map(|button| (button, &mut claim)),
                    screen_mouse: mouse,
                    captured_press: captured_press.filter(|_| owns_capture),
                },
            );
            result = component.handle_event(delivered, state, &mut ctx);
            if let Some(text) = ctx.clipboard.take() {
                self.clipboard = Some(text);
            }
            if !matches!(result, EventResult::Ignored) {
                break;
            }
        }
        if let (Some(button), Some(path)) = (pressed, claim) {
            self.gestures.claim(button, path);
        }
        result
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
    fn route_mouse(
        &mut self,
        mouse: MouseEvent,
        hit: Option<&[ChildId]>,
        state: &State,
    ) -> EventResult<Msg> {
        let path = self.pointer_target(mouse, hit);
        let moved = mouse.kind == MouseKind::Moved;
        // Motion moves hover; so does a press, since a backend may report
        // one with no motion before it.
        if moved || matches!(mouse.kind, MouseKind::Down(_)) {
            self.hover = path.clone().unwrap_or_default();
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

        let chain = self.surface.mouse_bubble_chain(&path);
        let routed = self.dispatch_chain(&chain, &Event::Mouse(mouse), state);
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
        // whatever declared the layer. Such a chain starts at that layer's
        // root.
        if chain
            .first()
            .is_some_and(|&index| self.surface.is_layer_root(index))
        {
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }

    /// What this event is aimed at: the component that captured the gesture if
    /// one did, otherwise whatever geometry puts under the pointer.
    ///
    /// Which of the two answers applies is [`Gestures::capture_for`]'s to
    /// give: only the events that continue a gesture consult a claim.
    fn pointer_target(&self, mouse: MouseEvent, hit: Option<&[ChildId]>) -> Option<Vec<ChildId>> {
        self.gestures
            .capture_for(mouse.kind)
            .or(hit)
            .map(<[ChildId]>::to_vec)
    }

    /// The focus change a primary `Down` produces when no component handled
    /// it, or `None` when this event is not one or nothing along the chain can
    /// take focus. The search runs over the bubble chain rather than the
    /// whole surface, which keeps focus-on-press inside the hit layer.
    ///
    /// A press that lands in a scope already holding focus — its dead space,
    /// or a child that takes none — is consumed and leaves focus alone, with
    /// no reveal: the user pressed the pane they are in, not its first
    /// control, and nothing asked to be scrolled into view. Only a focused
    /// leaf that takes focus holds it; a path parked anywhere else is
    /// rescued as if the scope held none.
    fn focus_on_press(
        &mut self,
        chain: &[usize],
        mouse: MouseEvent,
        state: &State,
    ) -> Option<EventResult<Msg>> {
        if mouse.kind != MouseKind::Down(MouseButton::Left) {
            return None;
        }
        let target = chain
            .iter()
            .rev()
            .copied()
            .find(|&index| self.surface.nodes[index].focusable)?;

        let current = self.surface.resolve_focus(self.stored_focus(state));
        // Held means a leaf that really takes focus: a path parked on
        // something that takes none, or on nothing declared, is left for the
        // press to rescue.
        let held = self
            .surface
            .leaf_of(current.path())
            .filter(|&leaf| self.surface.takes_focus(leaf));
        if held.is_some_and(|leaf| leaf != target)
            && self.surface.path_is_prefix_of(target, current.path())
        {
            return Some(EventResult::Consumed);
        }
        // Focus lands on a leaf, so a focusable container hands off to its
        // first focusable descendant.
        let focus = self.surface.descend_focus(target, Step::Forward)?;
        Some(self.focus_transition_result(focus, &current))
    }

    /// The dismiss message of the topmost popup with a dismiss hook that the
    /// press landed outside of, if any, with `target` naming what it hit.
    /// "Outside" is containment, not depth: the press hit nothing, or hit
    /// something that is not inside the popup's subtree. A popup without a
    /// hook has nothing to say and does not shadow one beneath it. Popups an
    /// open modal covers are inert and never dismiss.
    fn popup_dismissal(&self, target: Option<&[ChildId]>) -> Option<Msg> {
        // Innermost first, and keep looking: the layer the press landed
        // inside is not dismissed, but one it landed outside of still is.
        let top = self.surface.top_layer(|layer| {
            layer.on_dismiss.is_some()
                && self.surface.interactive(layer.root)
                && self.surface.nodes[layer.root].live
                && target.is_none_or(|hit| !self.surface.path_is_prefix_of(layer.root, hit))
        })?;
        top.on_dismiss.as_ref().map(|dismiss| dismiss())
    }

    /// The focus change a motion onto `path` produces when it crosses a
    /// [`hover_focus`](ScopeOptions::hover_focus) boundary, and `None`
    /// otherwise. Hover itself is already written by the time this runs, so
    /// the one message a motion can carry is this one.
    fn stage_hover_focus(&mut self, path: &[ChildId], state: &State) -> Option<EventResult<Msg>> {
        let stored = self.stored_focus(state);
        let next = self
            .surface
            .focus_for_hover(path, stored, &self.root_options)?;
        Some(self.focus_result(next))
    }

    /// The app's focus message for `focus`. Consumed when nothing is bound to
    /// carry it, since the change has nowhere to be stored.
    fn focus_result(&self, focus: FocusState) -> EventResult<Msg> {
        self.focus_binding
            .as_ref()
            .map_or(EventResult::Consumed, |binding| {
                EventResult::Emit((binding.on_change)(focus))
            })
    }

    /// Ask the component that declared the viewport clipping `focus`'s target
    /// in `surface` to bring it into view.
    ///
    /// Every reveal in the runtime happens here, whatever moved focus: a Tab
    /// the runtime resolved, a press, a [`focus_path`](Self::focus_path) the
    /// app looked up, or a [`FocusState`] its update function stored. What
    /// they share is that the app holds the new path by the time the frame
    /// is declared.
    ///
    /// Focus sits on a whole path or nowhere, and so does the reveal: a
    /// surface that declares only a prefix of the path answers
    /// [`Reveal::Absent`], never for the prefix's node.
    fn reveal_focus(
        &mut self,
        surface: &mut Surface<State, Msg>,
        focus: &FocusState,
        state: &State,
    ) -> Reveal {
        if focus.is_none() {
            return Reveal::Settled;
        }
        let Some(target) = surface.leaf_of(focus.path()) else {
            return Reveal::Absent;
        };
        if !surface.takes_focus(target)
            || surface.viewport_visibility(target) == ViewportVisibility::Full
        {
            return Reveal::Settled;
        }
        let Some(owner) = surface
            .clipping_viewport(target)
            .and_then(|record| record.owner)
        else {
            return Reveal::Settled;
        };
        let reveal = surface.nodes[target].area;
        let path = surface.path_of(owner).to_vec();
        let area = surface.nodes[owner].area;
        let Some(component) = surface.nodes[owner].component.as_mut() else {
            return Reveal::Settled;
        };
        let mut ctx = EventCtx::at(path, area, &mut self.transients, PointerInputs::default());
        if component.reveal_in_viewport(reveal, state, &mut ctx) {
            Reveal::Scrolled
        } else {
            Reveal::Settled
        }
    }

    /// The result of a focus step that resolved to `next`.
    ///
    /// A step that lands where focus already is consumes the event and owes a
    /// reveal. Focus does not move, so there is no message to send and no
    /// change for the next frame to notice — yet asking for a control by name
    /// is a request to see it, and the frame that follows answers it.
    fn focus_transition_result(
        &mut self,
        next: FocusState,
        current: &FocusState,
    ) -> EventResult<Msg> {
        if next == *current {
            self.reveal_pending = true;
            return EventResult::Consumed;
        }
        self.focus_result(next)
    }
}

/// What the tests in this module read back out of a committed surface.
#[cfg(test)]
impl<State, Msg> Ratcn<State, Msg> {
    fn hover_path(&self) -> &[ChildId] {
        &self.hover
    }

    fn declared_paths(&self) -> Vec<Vec<ChildId>> {
        (0..self.surface.nodes.len())
            .map(|index| self.surface.path_of(index).to_vec())
            .collect()
    }
}

#[cfg(test)]
mod tests;
