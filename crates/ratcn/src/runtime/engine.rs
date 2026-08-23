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
//! projections before [`Surface`], and the paint queue and its canvases
//! before [`RenderPass`]. What the pointer is doing between a press and its
//! release lives in [`gesture`](super::gesture), which [`Ratcn`] drives.

use std::{collections::HashMap, fmt};

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Position, Rect},
};

use crate::Theme;
use crate::backdrop::dim_background;

use super::{
    ChildId, Component, DeclareCtx, Event, EventCtx, EventResult, FocusState, KeyCode, KeyEvent,
    ModalState, MouseButton, MouseEvent, MouseKind, PaintCtx, ScopeOptions, Step, TabWrap,
    component::{InteractionFlags, PaintTarget, PointerInputs, TransientMap},
    focus,
    gesture::{Gestures, Press},
};

/// `begin_node`'s node kind, spelled out at each call site: a component node
/// occupies the area it was declared with and carries a [`Component`]; a scope
/// node is kept for identity and parents a subtree.
const COMPONENT_NODE: bool = false;
const SCOPE_NODE: bool = true;

// The largest rectangle a viewport declares as its content, and the largest a
// single paint inside one covers: each becomes a scratch buffer of one Ratatui
// cell per cell.
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

    /// A logical rectangle in screen coordinates, clipped to `clip`. Rows that
    /// project above the screen are dropped.
    fn project_rect(self, area: Rect, clip: Rect) -> Rect {
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
        .intersection(clip);
        if projected.is_empty() {
            Rect::ZERO
        } else {
            projected
        }
    }

    /// `area` with this viewport's scroll undone: the screen rectangle those
    /// logical rows sit at, held against the viewport's top edge where the
    /// offset would carry them above it.
    fn unscrolled(self, area: Rect) -> Rect {
        Rect {
            y: area.y.saturating_sub(self.offset).max(self.screen.y),
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

/// A viewport as one paint carries it: whether the viewport's clip still
/// applies to that paint.
///
/// The rectangle escaped paint addresses is the surface it lands on — the
/// frame, or a layer's canvas — so the methods that need it take it as
/// `surface` from whoever holds that surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Projection {
    /// Content declared inside the viewport. It addresses the whole logical
    /// content and reaches the rows the viewport shows.
    Clipped(Viewport),
    /// Paint that escaped the clip — a layer opened inside the viewport, or a
    /// closure deferred from inside it. It addresses the surface it paints
    /// on, read in logical coordinates, and reaches all of it.
    Escaped(Viewport),
}

impl Projection {
    const fn viewport(self) -> Viewport {
        match self {
            Self::Clipped(viewport) | Self::Escaped(viewport) => viewport,
        }
    }

    /// The screen rectangle paint carrying this reaches, given the `surface`
    /// it lands on.
    fn clip(self, surface: Rect) -> Rect {
        match self {
            Self::Clipped(viewport) => viewport.visible_screen(),
            Self::Escaped(_) => surface,
        }
    }

    /// The whole logical rectangle paint carrying this may write in, given
    /// the `surface` it lands on.
    pub(crate) fn allocation(self, surface: Rect) -> Rect {
        match self {
            Self::Clipped(viewport) => viewport.content(),
            Self::Escaped(viewport) => viewport.logical_frame(surface),
        }
    }

    /// A logical rectangle in screen coordinates, clipped.
    pub(crate) fn project_rect(self, area: Rect, surface: Rect) -> Rect {
        self.viewport().project_rect(area, self.clip(surface))
    }

    /// `point` in logical coordinates. Content counts only where the viewport
    /// shows it; paint that escaped the clip counts wherever the pointer is.
    fn to_logical(self, point: Position) -> Option<Position> {
        match self {
            Self::Clipped(viewport) => viewport.visible_to_logical(point),
            Self::Escaped(viewport) => viewport.to_logical(point),
        }
    }

    /// Every cell of `logical` this projection carries, paired with the
    /// screen cell it lands on.
    pub(crate) fn projected_positions(
        self,
        logical: Rect,
        surface: Rect,
    ) -> impl Iterator<Item = (Position, Position)> {
        let offset = self.viewport().offset;
        self.project_rect(logical, surface)
            .positions()
            .map(move |screen| {
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
    /// The layer open at declaration. A descendant on another layer escaped
    /// the clip.
    layer: Option<usize>,
}

impl ViewportRecord {
    /// How a node or a paint on `layer` reads this viewport. A viewport clips
    /// what was declared on its own layer; a layer opened inside it carries
    /// its content past the clip. The one place that rule is stated.
    fn projection(&self, layer: Option<usize>) -> Projection {
        if self.layer == layer {
            Projection::Clipped(self.viewport)
        } else {
            Projection::Escaped(self.viewport)
        }
    }
}

/// What the finished tree resolved this frame: the focus path paint styles
/// from, and the path the pointer rests on.
///
/// Both are answered once declaring has ended, from the tree the pass built,
/// and both travel into the replay together because every paint reads them
/// together.
#[derive(Debug, Clone, Copy)]
struct Resolved<'a> {
    focus: &'a FocusState,
    hover: &'a [ChildId],
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
    reason = "a policy table: one column per behavior, one row per layer kind"
)]
struct LayerPolicy {
    /// This layer takes the screen over: what lies beneath it dims, events
    /// landing outside it are consumed, focus resolves into it, Tab is
    /// trapped at its root, and keys stop there too. One decision with five
    /// consequences, so they travel together.
    takes_over: bool,
    /// The pointer can hit this layer at all. A layer that cannot is inert
    /// decoration: presses fall through to whatever is beneath it.
    hit_testable: bool,
    /// Anything inside this layer may hold focus. A layer that does not allow
    /// it is skipped by Tab and by every explicit focus request, whatever its
    /// contents claim through [`ScopeOptions::focusable`].
    allows_focus: bool,
    /// A press outside this layer emits its dismiss hook.
    dismiss_on_outside_press: bool,
    /// This layer's coordinates are the screen's. It undoes the scroll of the
    /// viewport it was declared in, once, over its own area, and declares from
    /// there in screen coordinates — free to open a viewport of its own. The
    /// anchored kinds keep that viewport's coordinates and are projected out
    /// of it once.
    screen_level: bool,
}

impl LayerPolicy {
    /// What everything declared outside any layer gets: no policy at all,
    /// except that it can be clicked.
    const fn base() -> Self {
        Self {
            takes_over: false,
            hit_testable: true,
            allows_focus: true,
            dismiss_on_outside_press: false,
            screen_level: false,
        }
    }
}

impl LayerKind {
    /// The whole difference between the layer kinds, in one table.
    const fn policy(self) -> LayerPolicy {
        match self {
            // Takes the screen over: dims, claims interaction, holds focus,
            // swallows keys, and belongs to the screen rather than to whatever
            // viewport declared it.
            Self::Modal => LayerPolicy {
                takes_over: true,
                screen_level: true,
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
    /// This node's own segment of its identity path; [`Surface::path_of`]
    /// derives the whole path from the parent chain.
    id: ChildId,
    parent: Option<usize>,
    children: Vec<usize>,
    area: Rect,
    /// Index into [`Surface::viewports`] of the innermost viewport this node
    /// was declared inside.
    viewport: Option<usize>,
    options: ScopeOptions,
    is_scope: bool,
    component: Option<Box<dyn Component<State, Msg>>>,
    /// The layer this node's paint lands on, indexing [`Surface::layers`]
    /// and the pass's canvases. `None` outside any layer.
    layer: Option<usize>,
}

impl<State, Msg> fmt::Debug for Node<State, Msg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Node")
            .field("id", &self.id)
            .field("parent", &self.parent)
            .field("children", &self.children)
            .field("area", &self.area)
            .field("viewport", &self.viewport)
            .field("options", &self.options)
            .field("is_scope", &self.is_scope)
            .field("component", &self.component.is_some())
            .field("layer", &self.layer)
            .finish()
    }
}

/// One declared layer: the node rooting its subtree, and what it does.
struct Layer<Msg> {
    root: usize,
    kind: LayerKind,
    /// The message a press outside the layer emits, on the kinds that
    /// dismiss.
    on_dismiss: Option<Box<dyn Fn() -> Msg>>,
}

pub(crate) struct Surface<State, Msg> {
    nodes: Vec<Node<State, Msg>>,
    roots: Vec<usize>,
    /// Every layer, in declaration order, indexed by the layer number nodes
    /// carry. Nesting appends, so scanning backwards reaches the topmost
    /// first.
    layers: Vec<Layer<Msg>>,
    /// Every viewport declared this pass, in declaration order.
    viewports: Vec<ViewportRecord>,
}

impl<State, Msg> Default for Surface<State, Msg> {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            roots: Vec::new(),
            layers: Vec::new(),
            viewports: Vec::new(),
        }
    }
}

impl<State, Msg> fmt::Debug for Surface<State, Msg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Surface")
            .field("nodes", &self.nodes.len())
            .field("roots", &self.roots.len())
            .field("layers", &self.layers.len())
            .field("viewports", &self.viewports.len())
            .finish()
    }
}

impl<State, Msg> Surface<State, Msg> {
    /// The identity path of `index`, outermost first. [`Self::inside`] and
    /// [`Self::path_is_prefix_of`] answer structural questions without
    /// building one.
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
        self.path_match(index, path).1
    }

    /// Where `index` sits in this frame's resolved focus and hover — the four
    /// flags [`PaintCtx`] reports.
    fn interaction_flags(&self, index: usize, resolved: Resolved<'_>) -> InteractionFlags {
        let (focused, contains_focus) = self.path_match(index, resolved.focus.path());
        let (hovered, contains_hover) = self.path_match(index, resolved.hover);
        InteractionFlags {
            focused,
            contains_focus,
            hovered,
            contains_hover,
        }
    }

    /// Whether `index`'s identity path *is* `path`, and whether it is a
    /// prefix of it — the leaf question and the within question, as a pair.
    fn path_match(&self, index: usize, path: &[ChildId]) -> (bool, bool) {
        let depth = self.depth(index);
        if depth > path.len() {
            return (false, false);
        }
        let within = self.path_is(index, &path[..depth]);
        (within && depth == path.len(), within)
    }

    fn has_hit_geometry(&self, index: usize) -> bool {
        !self.nodes[index].area.is_empty()
    }

    fn participates(&self, index: usize) -> bool {
        let node = &self.nodes[index];
        (node.is_scope || self.has_hit_geometry(index))
            && node.parent.is_none_or(|parent| self.participates(parent))
    }

    /// Whether `index` takes part in this frame's interaction at all: it and
    /// every ancestor are still declared, and it has geometry to occupy.
    fn present(&self, index: usize) -> bool {
        self.participates(index) && self.has_hit_geometry(index)
    }

    /// Whether the pointer can land on `index`: present, inside whatever layer
    /// has taken the screen over, on a layer the pointer reaches, and not
    /// scrolled out of its viewport. The one eligibility test behind
    /// hit-testing and hover.
    fn hittable(&self, index: usize) -> bool {
        self.present(index)
            && self.interactive(index)
            && self.policy(self.nodes[index].layer).hit_testable
            && self.viewport_visibility(index) != ViewportVisibility::Hidden
    }

    fn children(&self, parent: Option<usize>) -> &[usize] {
        parent.map_or(self.roots.as_slice(), |index| {
            self.nodes[index].children.as_slice()
        })
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
        self.takeover_root()
            .is_none_or(|root| self.inside(index, root))
    }

    /// What the layer `layer` names does to interaction. `None` is the base
    /// layer everything outside any layer is declared into.
    fn policy(&self, layer: Option<usize>) -> LayerPolicy {
        layer.map_or_else(LayerPolicy::base, |index| self.layers[index].kind.policy())
    }

    /// The topmost open layer that satisfies `wants`.
    fn top_layer(&self, wants: impl Fn(&Layer<Msg>) -> bool) -> Option<&Layer<Msg>> {
        self.layers.iter().rev().find(|layer| wants(layer))
    }

    /// The layer that has taken the screen over, if one is open: everything
    /// outside it is inert, unfocusable, and unreachable by a key.
    fn takeover_root(&self) -> Option<usize> {
        self.top_layer(|layer| layer.kind.policy().takes_over)
            .map(|layer| layer.root)
    }

    /// Whether the canvas `index` backs lies beneath the layer that has taken
    /// the screen over, when one is open.
    fn covered_by_takeover(&self, index: usize, takeover: Option<usize>) -> bool {
        takeover.is_some_and(|root| !self.inside(self.layers[index].root, root))
    }

    /// Every modal root, outermost first — what `Ratcn::modals` validates the
    /// app's stack against.
    fn modal_roots(&self) -> impl Iterator<Item = usize> + '_ {
        self.layers
            .iter()
            .filter(|layer| layer.kind == LayerKind::Modal)
            .map(|layer| layer.root)
    }

    /// Whether `index` is `root` or one of its descendants.
    ///
    /// The one containment test, answered by walking parent indices: identity
    /// paths are unique, so this is what comparing paths would say, without
    /// touching an id. Layer numbers order paint and must never be used to
    /// answer it — a layer declared after another takes a higher number
    /// without being inside it.
    fn inside(&self, index: usize, root: usize) -> bool {
        let mut current = Some(index);
        while let Some(node) = current {
            if node == root {
                return true;
            }
            current = self.nodes[node].parent;
        }
        false
    }

    /// The node `path` names, or `None` when this surface does not declare it
    /// whole. The empty path names no node: every declaration has at least its
    /// own id.
    fn leaf_of(&self, path: &[ChildId]) -> Option<usize> {
        let matched = self.nodes_along_path(path);
        (matched.len() == path.len())
            .then(|| matched.last().copied())
            .flatten()
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
    /// See [`Self::focusable`] for the same question about a node *or any of
    /// its descendants*.
    fn takes_focus(&self, index: usize) -> bool {
        self.present(index)
            && self.interactive(index)
            && self.policy(self.nodes[index].layer).allows_focus
            && self.nodes[index].options.focusable
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

    /// The first focusable child of `parent` in `direction`. With no parent
    /// the candidates are the tree roots — or, while a layer has taken the
    /// screen over, that layer's root alone, which traps Tab inside it.
    fn edge_child(&self, parent: Option<usize>, direction: Step) -> Option<usize> {
        match (parent, self.takeover_root()) {
            (Some(index), _) => self.find_focusable(&self.nodes[index].children, direction),
            (None, Some(root)) => self.focusable(root).then_some(root),
            (None, None) => self.find_focusable(&self.roots, direction),
        }
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

    /// The focus path produced by descending into the first focusable child
    /// of `parent` — the traversal roots when there is no parent. The other
    /// primitive: an edge, with no request behind it.
    fn edge_focus(&self, parent: Option<usize>, direction: Step) -> Option<FocusState> {
        let index = self.edge_child(parent, direction)?;
        self.descend_focus(index, direction)
    }

    /// The focus path produced by descending from `index` to its first
    /// focusable leaf, seeded with the node's own ancestor prefix — correct
    /// whether the node is a tree root or a nested layer root.
    ///
    /// The primitive every focus policy ends at: the path it answers with is
    /// a leaf this surface declares, walked to here on the surface's own
    /// terms.
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
    /// - With a layer holding the screen, a declared path outside it resolves
    ///   into that layer; an absent path stays parked even then, so render and
    ///   routing agree on it.
    fn resolve_focus(&self, stored: &FocusState) -> FocusState {
        if let Some(root) = self.takeover_root()
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
            return self.edge_focus(None, Step::Forward).unwrap_or_default();
        }
        let Some(target) = self.leaf_of(stored.path()) else {
            return stored.clone();
        };
        self.edge_focus(Some(target), Step::Forward)
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
        // A descent that succeeds ends on a leaf that participates, and
        // participation runs the whole parent chain, so every node along
        // `path` is focusable whenever this answers with one at all.
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

    /// The viewport that clips `index`, if one does. A node declared on a
    /// layer opened inside a viewport escaped that clip and has none.
    fn clipping_viewport(&self, index: usize) -> Option<&ViewportRecord> {
        let node = &self.nodes[index];
        let record = &self.viewports[node.viewport?];
        matches!(record.projection(node.layer), Projection::Clipped(_)).then_some(record)
    }

    /// The projection `index` was declared through, and `None` where no
    /// viewport carries it.
    fn projection_of(&self, index: usize) -> Option<Projection> {
        let node = &self.nodes[index];
        Some(self.viewports[node.viewport?].projection(node.layer))
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
        self.projection_of(index)
            .map_or(Some(point), |projection| projection.to_logical(point))
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
        if let Some(root) = self.takeover_root()
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
            return self.next_from_scope(current, direction, root_options);
        }

        let Some(current) = matched.last().copied() else {
            return self
                .edge_focus(None, direction)
                .map_or(FocusAdvance::Ignored, FocusAdvance::Move);
        };
        self.next_from_scope(current, direction, root_options)
    }

    /// The next focusable node after `current`, walking outwards until a
    /// scope wraps or the root runs out.
    ///
    /// A step that lands back on the node it started from is still a
    /// `Move`; the caller compares it against the current focus.
    fn next_from_scope(
        &self,
        mut current: usize,
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
            if let Some(next) = self.find_focusable(remaining, direction)
                && let Some(focus) = self.descend_focus(next, direction)
            {
                return FocusAdvance::Move(focus);
            }

            // The root of a layer holding the screen traps Tab regardless of
            // where it sits in the tree; otherwise the enclosing scope decides.
            let tab_wrap = if self.takeover_root() == Some(current) {
                TabWrap::Wrap
            } else {
                parent.map_or(root_options.tab_wrap, |index| {
                    self.nodes[index].options.tab_wrap
                })
            };
            if tab_wrap == TabWrap::Wrap {
                return self
                    .edge_focus(parent, direction)
                    .map_or(FocusAdvance::Consumed, FocusAdvance::Move);
            }
            let Some(parent) = parent else {
                return FocusAdvance::Ignored;
            };
            current = parent;
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

/// The surface an op paints onto, and the projection it paints through.
///
/// Both are fixed where the op was queued, so an op belongs to the layer that
/// was open at its declaration whatever is open at replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PaintSlot {
    /// The layer canvas the op paints onto, `None` for the frame.
    layer: Option<usize>,
    projection: Option<Projection>,
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
    /// Run a closure queued through [`DeclareCtx::paint`] or
    /// [`DeclareCtx::defer_paint`]. `node` is the declaration it was reached
    /// from, or `None` at the root and for deferred paint, which have no
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

/// A private paint surface: a buffer, and the rectangles written into it.
///
/// A layer subtree is declared inline, wherever its owner lives in the tree,
/// and paints *above* everything declared outside it — including siblings
/// declared later. It paints here and composites once the pass is over:
/// `painted` records the rects paint wrote through, only those rects blit —
/// so a modal declared over the full screen composites just the box it
/// painted — and each rect composites opaquely, unwritten cells included.
pub(crate) struct Canvas {
    pub(crate) buffer: Buffer,
    painted: Vec<Rect>,
}

impl Canvas {
    fn new(area: Rect) -> Self {
        Self {
            buffer: Buffer::empty(area),
            painted: Vec::new(),
        }
    }

    /// The part of `area` this canvas can hold. Paint outside it is clipped
    /// away.
    pub(crate) fn clip(&self, area: Rect) -> Rect {
        area.intersection(self.buffer.area)
    }

    /// Record that `area` was painted, clipped to the canvas.
    pub(crate) fn mark_painted(&mut self, area: Rect) {
        let clipped = self.clip(area);
        if !clipped.is_empty() {
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
    pub(crate) transients: &'a mut TransientMap,
}

impl<'a, State> DeclarationEnv<'a, State> {
    /// The environment for a root declaration: the app's own closure, covering
    /// the whole frame.
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
            transients,
        }
    }

    /// The same environment reborrowed for the declarations *inside* the node
    /// just opened, over `area`.
    fn nested(&mut self, area: Rect) -> DeclarationEnv<'_, State> {
        DeclarationEnv {
            frame_area: self.frame_area,
            area,
            state: self.state,
            theme: self.theme,
            transients: &mut *self.transients,
        }
    }
}

pub(crate) struct RenderPass<State, Msg> {
    frame_area: Rect,
    surface: Surface<State, Msg>,
    parent_stack: Vec<usize>,
    /// The identity path of the open declaration chain, maintained in step
    /// with `parent_stack` by [`Self::enter_node`] and [`Self::leave_node`].
    path_cursor: Vec<ChildId>,
    /// Deferred paint thunks, each tagged with the layer it was registered
    /// in. [`Self::finish_frame`] flushes a layer's thunks onto its canvas
    /// just before that canvas composites, so they cover everything the layer
    /// declared, and the base layer's onto the frame after every canvas has
    /// composited, which is what makes root-level `defer_paint` the topmost
    /// slot.
    deferred: Vec<QueuedPaint<State>>,
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
    /// Set when any declaration region unwinds — see [`Self::guarded`]. A
    /// poisoned pass can never commit.
    failed: bool,
    /// One canvas per declared layer, in discovery order.
    canvases: Vec<Canvas>,
    /// The open viewport, indexing [`Surface::viewports`]. A viewport
    /// declared while one is open panics, so there is at most one.
    open_viewport: Option<usize>,
    /// The layer canvases open in declaration nesting order; the innermost
    /// decides where the next paint belongs.
    layer_stack: Vec<usize>,
    /// The buffer paint inside a viewport lays out in, shared by every paint
    /// call the frame makes — see [`PaintTarget`].
    scratch: Buffer,
}

impl<State, Msg> RenderPass<State, Msg> {
    fn new(frame_area: Rect) -> Self {
        Self {
            frame_area,
            surface: Surface::default(),
            parent_stack: Vec::new(),
            path_cursor: Vec::new(),
            deferred: Vec::new(),
            paint_queue: Vec::new(),
            hover_position: None,
            hover_path: Vec::new(),
            failed: false,
            canvases: Vec::new(),
            open_viewport: None,
            layer_stack: Vec::new(),
            scratch: Buffer::empty(Rect::ZERO),
        }
    }

    /// The layer currently being declared into, named by its canvas index.
    /// `None` outside any layer.
    fn current_layer(&self) -> Option<usize> {
        self.layer_stack.last().copied()
    }

    /// The identity path of the declaration currently being declared into —
    /// the key [`DeclareCtx::transient`] reads the transient store with.
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

    /// Queue one declaration's paint for the slot currently being declared
    /// into.
    fn queue(&mut self, paint: DeclaredPaint<State>) {
        self.paint_queue.push(QueuedPaint {
            slot: self.active_slot(),
            paint,
        });
    }

    /// The slot the layer currently being declared into paints in.
    /// Content declared inside the open viewport is clipped to what the
    /// viewport shows; a layer opened inside one escaped that clip and paints
    /// where the offset puts it.
    fn active_slot(&self) -> PaintSlot {
        let layer = self.current_layer();
        PaintSlot {
            layer,
            projection: self
                .open_viewport
                .map(|index| self.surface.viewports[index].projection(layer)),
        }
    }

    /// The slot paint registered through [`DeclareCtx::defer_paint`] runs in:
    /// the layer being declared into, and a projection that escapes the open
    /// viewport's clip.
    fn escaped_slot(&self) -> PaintSlot {
        PaintSlot {
            layer: self.current_layer(),
            projection: self
                .open_viewport
                .map(|index| Projection::Escaped(self.surface.viewports[index].viewport)),
        }
    }

    /// Where the pointer is in the coordinates `slot` paints in, and `None`
    /// where the projection it carries does not reach it.
    fn hover_in(&self, slot: PaintSlot) -> Option<Position> {
        let position = self.hover_position?;
        match slot.projection {
            Some(projection) => projection.to_logical(position),
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
                layer: pass.current_layer(),
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
    /// An anchored layer keeps the coordinates of the viewport it was declared
    /// in, and takes that viewport's projection of `env.area` as its canvas. A
    /// screen-level one undoes the viewport's scroll once — over its own area,
    /// and over the frame its subtree reads — and then declares with no
    /// viewport open at all, so what it declares is in screen coordinates and
    /// may open a viewport of its own. Either way the viewport is back for
    /// whatever the declaration goes on to say after the layer.
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
        let enclosing = self.open_viewport;
        let viewport = enclosing.map(|index| self.surface.viewports[index].viewport);
        let canvas_area = if kind.policy().screen_level {
            self.open_viewport = None;
            env.frame_area = self.frame_area;
            env.area = viewport.map_or(env.area, |viewport| viewport.unscrolled(env.area));
            env.area
        } else {
            viewport.map_or(env.area, |viewport| {
                viewport.project_rect(env.area, self.frame_area)
            })
        };
        self.canvases.push(Canvas::new(canvas_area));
        let canvas = self.canvases.len() - 1;
        self.layer_stack.push(canvas);

        let root = self.surface.nodes.len();
        self.surface.layers.push(Layer {
            root,
            kind,
            on_dismiss,
        });
        declare_root(self, env);
        assert!(
            self.surface
                .nodes
                .get(root)
                .is_some_and(|node| node.layer == Some(canvas)),
            "a layer's root is the first node its declaration opens"
        );

        self.layer_stack.pop();
        self.open_viewport = enclosing;
    }

    /// Run `f` as one declaration region: if it unwinds — a panicking
    /// component, or the runtime's own validation — the pass is poisoned and
    /// can never commit, no matter who catches the panic. Every entry point
    /// that runs user code or validates a declaration goes through here.
    pub(crate) fn guarded<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(self))) {
            Ok(result) => result,
            Err(payload) => {
                self.failed = true;
                std::panic::resume_unwind(payload)
            }
        }
    }

    fn begin_node(
        &mut self,
        id: ChildId,
        area: Rect,
        options: ScopeOptions,
        is_scope: bool,
    ) -> usize {
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
        let viewport = self.open_viewport;
        self.surface.nodes.push(Node {
            id,
            parent,
            children: Vec::new(),
            area,
            viewport,
            options,
            is_scope,
            component: None,
            layer,
        });
        if let Some(parent) = parent {
            self.surface.nodes[parent].children.push(index);
        } else {
            self.surface.roots.push(index);
        }
        index
    }

    pub(crate) fn component(
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
            let index = pass.begin_node(id, interaction_area, options, COMPONENT_NODE);
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
        declare: impl FnOnce(&mut DeclareCtx<'_, State, Msg>),
    ) {
        debug_assert!(
            on_dismiss.is_none() || kind.policy().dismiss_on_outside_press,
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
        mut env: DeclarationEnv<'_, State>,
        declare: impl FnOnce(&mut DeclareCtx<'_, State, Msg>),
    ) {
        self.guarded(|pass| {
            let area = env.area;
            let index = pass.begin_node(id, area, options, SCOPE_NODE);
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
    /// A panic out of `declare` poisons the pass through the [`Self::guarded`]
    /// region of the declaration entry point that is open — `scope`,
    /// `component`, `viewport`, or a layer entry, which carries a region of its
    /// own around the validation and lifecycle it owns. Poisoning is
    /// idempotent, so crossing several regions reads the same as crossing one.
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

    /// Register a deferred closure. It has no identity, so its area is the
    /// whole surface it writes to.
    pub(crate) fn defer_paint(&mut self, paint: impl FnOnce(&mut PaintCtx<'_, State>) + 'static) {
        let slot = self.escaped_slot();
        let surface = slot
            .layer
            .map_or(self.frame_area, |index| self.canvases[index].buffer.area);
        let area = slot
            .projection
            .map_or(surface, |projection| projection.allocation(surface));
        self.deferred.push(QueuedPaint {
            slot,
            paint: DeclaredPaint::Thunk {
                node: None,
                area,
                paint: Box::new(paint),
            },
        });
    }

    /// Run every queued op in declaration order, each onto the surface its
    /// declaration belonged to, with the flags the finished tree resolved.
    fn replay_paint(
        &mut self,
        frame: &mut Frame,
        state: &State,
        theme: &Theme,
        resolved: Resolved<'_>,
    ) {
        for QueuedPaint { slot, paint } in std::mem::take(&mut self.paint_queue) {
            self.paint_op(paint, slot, frame, state, theme, resolved);
        }
    }

    /// Paint one declaration onto the surface its layer names, with the flags
    /// the finished tree resolved for it.
    fn paint_op(
        &mut self,
        op: DeclaredPaint<State>,
        slot: PaintSlot,
        frame: &mut Frame,
        state: &State,
        theme: &Theme,
        resolved: Resolved<'_>,
    ) {
        // Read before the component borrow below, which needs the surface
        // mutably. The root declaration has no node, and so no flags.
        let flags = op.node().map_or_else(InteractionFlags::default, |index| {
            self.surface.interaction_flags(index, resolved)
        });
        let hover_position = self.hover_in(slot);
        let target = match slot.layer {
            None => PaintTarget::frame(frame.buffer_mut(), slot.projection, &mut self.scratch),
            Some(index) => PaintTarget::canvas(
                &mut self.canvases[index],
                slot.projection,
                &mut self.scratch,
            ),
        };
        let mut ctx = PaintCtx {
            target,
            theme,
            area: op.area(),
            flags,
            hover_position,
            state,
        };
        match op {
            // `assert_valid`'s completeness check ran before replay, so
            // every node here has its component.
            DeclaredPaint::Node { index, .. } => self.surface.nodes[index]
                .component
                .as_deref_mut()
                .expect("a checked pass installed every node's component")
                .paint(&mut ctx),
            DeclaredPaint::Thunk { paint, .. } => paint(&mut ctx),
        }
    }

    /// Finish the frame's painting: composite every layer canvas over the
    /// frame — a modal dims what is beneath it first, and the layer's own
    /// deferred thunks land on its canvas above everything it declared — then
    /// flush the base declaration's deferred thunks on top of the result,
    /// making root-level [`DeclareCtx::defer_paint`] the topmost decoration
    /// slot (toast stacks, drag ghosts).
    ///
    /// Layers composite in declaration order, except that every layer outside
    /// the one that has taken the screen over composites before it: what the
    /// takeover covers is inert, and so must not paint above it either.
    fn finish_frame(&mut self, frame: &mut Frame, state: &State, theme: &Theme) {
        let mut deferred = std::mem::take(&mut self.deferred);
        let takeover = self.surface.takeover_root();
        for index in 0..self.canvases.len() {
            if self.surface.covered_by_takeover(index, takeover) {
                self.composite_layer(index, &mut deferred, frame, state, theme);
            }
        }
        for index in 0..self.canvases.len() {
            if !self.surface.covered_by_takeover(index, takeover) {
                self.composite_layer(index, &mut deferred, frame, state, theme);
            }
        }
        self.flush_deferred(&mut deferred, None, frame, state, theme);
    }

    /// Copy one layer's canvas onto the frame — dimming beneath it first when
    /// it takes the screen over — and flush the deferred thunks that belong
    /// to it.
    fn composite_layer(
        &mut self,
        index: usize,
        deferred: &mut Vec<QueuedPaint<State>>,
        frame: &mut Frame,
        state: &State,
        theme: &Theme,
    ) {
        if self.surface.policy(Some(index)).takes_over {
            dim_background(
                frame.buffer_mut(),
                self.canvases[index].buffer.area,
                theme.background,
            );
        }
        self.flush_deferred(deferred, Some(index), frame, state, theme);
        let frame_area = frame.area();
        let canvas = &self.canvases[index];
        for &rect in &canvas.painted {
            copy_rect(
                &canvas.buffer,
                frame.buffer_mut(),
                rect.intersection(frame_area),
            );
        }
    }

    /// Run the thunks `deferred` holds for `layer`, in registration order,
    /// and leave the rest for the layer they belong to.
    fn flush_deferred(
        &mut self,
        deferred: &mut Vec<QueuedPaint<State>>,
        layer: Option<usize>,
        frame: &mut Frame,
        state: &State,
        theme: &Theme,
    ) {
        let (theirs, rest): (Vec<_>, Vec<_>) = std::mem::take(deferred)
            .into_iter()
            .partition(|entry| entry.slot.layer == layer);
        *deferred = rest;
        // Deferred thunks carry `node: None`, so no flag is ever read from this.
        let resolved = Resolved {
            focus: &focus::UNRESOLVED,
            hover: &[],
        };
        for QueuedPaint { slot, paint } in theirs {
            self.paint_op(paint, slot, frame, state, theme, resolved);
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
    fn assert_valid(&self) {
        assert!(!self.failed, "cannot commit a failed declaration pass");
        assert!(
            self.parent_stack.is_empty() && self.layer_stack.is_empty(),
            "cannot commit a declaration pass with unclosed components or layers"
        );
        assert!(
            self.surface
                .nodes
                .iter()
                .all(|node| node.is_scope || node.component.is_some()),
            "cannot commit a declaration pass with incomplete components"
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
/// known before the first cell is written. The one thing that cannot be taken
/// back is a panic thrown by painting itself, after the pass had already been
/// accepted.
pub struct Ratcn<State, Msg> {
    surface: Surface<State, Msg>,
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
    /// it. Between the reveal at the top of a frame and the commit at its end
    /// it holds what that reveal answered for.
    resolved_focus: FocusState,
    /// Whether a reveal is still waiting to be answered: a focus change no
    /// surface has been able to place yet, or one an event asked for
    /// outright. The frame that answers it clears it.
    reveal_pending: bool,
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
    /// [`hint`](DeclareCtx::hint) layers are exempt from paint order: each
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
        declare: impl FnOnce(&mut DeclareCtx<'_, State, Msg>),
    ) {
        let focus_snapshot = self.stored_focus(state);
        // Reveal first: the surface that painted the previous focus is still
        // the one in hand, and it is the tree that can say where the focus now
        // sits and what clips it. What it answers is a transient the
        // declaration below reads. A change this surface cannot place — a path
        // it never declared — stays pending for the frame that can.
        self.reveal_moved_focus(focus_snapshot, state);

        // Declare. Nothing is drawn and no *focus* flag is read: the walk
        // builds the tree and queues the paint it owes. Hover is the one
        // interaction fact that predates the pass, so the declaration may ask
        // for it — see [`DeclareCtx::pointer_within`].
        let mut pass = RenderPass::new(frame.area());
        pass.hover_position = self.pointer;
        pass.hover_path.clone_from(&self.hover);
        pass.with_declare_ctx(
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
        let resolved_focus = pass.surface.resolve_focus(focus_snapshot);
        let resolved_hover = self.resolve_hover(&pass.surface);
        pass.replay_paint(
            frame,
            state,
            theme,
            Resolved {
                focus: &resolved_focus,
                hover: &resolved_hover,
            },
        );
        pass.finish_frame(frame, state, theme);
        self.commit_surface(pass.surface, resolved_hover, resolved_focus);
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
            .map(|index| surface.path_of(index))
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
        let declared = surface.modal_roots().map(|index| &surface.nodes[index].id);
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
    ) {
        let active_modal_changed = self
            .surface
            .modal_roots()
            .last()
            .map(|index| &self.surface.nodes[index].id)
            != next.modal_roots().last().map(|index| &next.nodes[index].id);

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
                surface
                    .leaf_of(path)
                    .is_some_and(|index| surface.participates(index))
            });
        }
        self.transients
            .retain(|path, _| self.surface.leaf_of(path).is_some());
        // The hover and focus this frame painted, published with the surface
        // they were resolved against — a pass that never got here leaves the
        // previous ones in charge, exactly as it leaves the previous surface.
        self.hover = hover;
        // Focus that resolves differently against this surface than against
        // the one the frame opened with never reached that frame's reveal:
        // this tree is the first that can answer for it, so the next frame
        // owes the reveal.
        self.reveal_pending |= focus != self.resolved_focus;
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

        let routed = self.dispatch_chain(&chain, event, state, &mut None, None, None);
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
    /// handled. A popup or a hint leaves keys alone, so an unhandled Esc
    /// under one still reaches whatever declared it.
    fn key_bubble_chain(&self, focus: &FocusState) -> Vec<usize> {
        let mut matched = self.surface.nodes_along_path(focus.path());
        let Some(takeover) = self.surface.takeover_root() else {
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
                let mut path = scope.map_or_else(Vec::new, |index| self.surface.path_of(index));
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
                .map(|index| &self.surface.nodes[index].id);
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
            .map(|index| self.surface.path_of(index))
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
    ///
    /// `chain` is one ancestor line, innermost last, so every node's path is
    /// a prefix of the last one's.
    fn dispatch_chain(
        &mut self,
        chain: &[usize],
        event: &Event,
        state: &State,
        capture: &mut Option<Vec<ChildId>>,
        capture_button: Option<MouseButton>,
        captured_press: Option<Press>,
    ) -> EventResult<Msg> {
        let Some(&innermost) = chain.last() else {
            return EventResult::Ignored;
        };
        let full_path = self.surface.path_of(innermost);
        let outermost_depth = full_path.len() - chain.len();
        for (position, &index) in chain.iter().enumerate().rev() {
            if !self.surface.participates(index) {
                continue;
            }
            let path = full_path[..=outermost_depth + position].to_vec();
            let area = self.surface.nodes[index].area;
            let viewport = self.surface.projection_of(index).map(Projection::viewport);
            // Declaration-space for the component, screen-absolute for the
            // gesture tracker `EventCtx::drag` keeps.
            let projected = match (event, viewport) {
                (Event::Mouse(mouse), Some(viewport)) => {
                    Some(Event::Mouse(viewport.mouse_to_logical(*mouse)))
                }
                _ => None,
            };
            let delivered = projected.as_ref().unwrap_or(event);
            let screen_mouse = match event {
                Event::Mouse(mouse) => Some(*mouse),
                _ => None,
            };
            let Some(component) = self.surface.nodes[index].component.as_mut() else {
                continue;
            };
            let mut ctx = EventCtx::at(
                path,
                area,
                &mut self.transients,
                PointerInputs {
                    capture: Some(capture),
                    button: capture_button,
                    screen_mouse,
                    captured_press,
                },
            );
            let result = component.handle_event(delivered, state, &mut ctx);
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
        let captured_press = self.gestures.captured_press(mouse.kind);
        let result = self.dispatch_chain(
            chain,
            &Event::Mouse(mouse),
            state,
            &mut capture,
            capture_button,
            captured_press,
        );
        if let (Some(button), Some(path)) = (capture_button, capture) {
            self.gestures.claim(button, path);
        }
        result
    }

    /// The focus change a primary `Down` produces when no component handled
    /// it, or `None` when this event is not one or nothing along the chain can
    /// take focus. The search runs over the bubble chain rather than the
    /// whole surface, which keeps focus-on-press inside the hit layer.
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
            .find(|&index| self.surface.focusable(index))?;

        // Focus lands on a leaf, so a focusable container hands off to its
        // first focusable descendant.
        let focus = self.surface.descend_focus(target, Step::Forward)?;
        let current = self.surface.resolve_focus(self.stored_focus(state));
        Some(self.focus_transition_result(focus, &current))
    }

    /// The dismiss message of the topmost popup the press landed outside of,
    /// if any, with `target` naming what it hit. "Outside" is containment, not
    /// depth: the press hit nothing, or hit something that is not inside the
    /// popup's subtree. Popups an open modal covers are inert and never
    /// dismiss.
    fn popup_dismissal(&self, target: Option<&[ChildId]>) -> Option<Msg> {
        // Innermost first, and keep looking: the layer the press landed
        // inside is not dismissed, but one it landed outside of still is.
        let top = self.surface.top_layer(|layer| {
            layer.kind.policy().dismiss_on_outside_press
                && self.surface.interactive(layer.root)
                && self.surface.participates(layer.root)
                && target.is_none_or(|hit| !self.surface.path_is_prefix_of(layer.root, hit))
        })?;
        top.on_dismiss.as_ref().map(|f| f())
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

    /// Reveal focus that has moved since the last frame, or that an event
    /// asked to see again, while the surface it moved across is still the one
    /// in hand.
    ///
    /// Every reveal in the runtime happens here, whatever moved focus: a Tab
    /// the runtime resolved, a press, a [`focus_path`](Self::focus_path) the
    /// app looked up, or a [`FocusState`] its update function stored. What
    /// they share is that the app holds the new path by the time this frame
    /// starts. The component's answer is a transient, which the declaration
    /// that follows reads back and lays out from.
    ///
    /// The first frame whose surface can place the focused target reveals it.
    /// A surface that does not declare the focused leaf has no geometry to
    /// answer with, so the reveal stays pending and each frame asks its own
    /// surface again.
    fn reveal_moved_focus(&mut self, stored: &FocusState, state: &State) {
        let focus = self.surface.resolve_focus(stored);
        if std::mem::take(&mut self.reveal_pending) || focus != self.resolved_focus {
            self.reveal_pending = !self.reveal_focus(&focus, state);
        }
        self.resolved_focus = focus;
    }

    /// Ask the component that declared the viewport clipping `focus`'s target
    /// to bring it into view. A target that is already fully on screen, or
    /// that no viewport clips, reaches nobody.
    ///
    /// `false` when this surface does not declare the focused leaf: it has no
    /// geometry to answer with, and whatever prefix of the path it does
    /// declare belongs to a different node. Focus sits on a whole path or
    /// nowhere, and so does the reveal.
    fn reveal_focus(&mut self, focus: &FocusState, state: &State) -> bool {
        let Some(target) = self.surface.leaf_of(focus.path()) else {
            return false;
        };
        if self.surface.viewport_visibility(target) == ViewportVisibility::Full {
            return true;
        }
        let Some(owner) = self
            .surface
            .clipping_viewport(target)
            .and_then(|record| record.owner)
        else {
            return true;
        };
        let reveal = self.surface.nodes[target].area;
        let path = self.surface.path_of(owner);
        let area = self.surface.nodes[owner].area;
        let Some(component) = self.surface.nodes[owner].component.as_mut() else {
            return true;
        };
        let mut ctx = EventCtx::at(path, area, &mut self.transients, PointerInputs::default());
        component.reveal_in_viewport(reveal, state, &mut ctx);
        true
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
            .map(|index| self.surface.path_of(index))
            .collect()
    }
}

/// Copy `area` out of `source` and into `destination`, cell for cell.
fn copy_rect(source: &Buffer, destination: &mut Buffer, area: Rect) {
    for position in area.positions() {
        if let (Some(cell), Some(target)) = (source.cell(position), destination.cell_mut(position))
        {
            *target = cell.clone();
        }
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

#[cfg(test)]
mod tests;
