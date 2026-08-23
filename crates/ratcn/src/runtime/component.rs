//! Components, and the three contexts a frame hands them.
//!
//! [`Component`] is what an interactive piece implements. A fresh instance is
//! built from app state every frame: it declares its area and its
//! descendants, paints once focus has resolved, and answers input with an
//! [`EventResult`] the app matches on.
//!
//! Each phase carries its own context, and what the context holds is what the
//! phase may read. [`DeclareCtx`] builds the tree — child declarations,
//! layers, scopes, viewports — and knows nothing of focus. [`PaintCtx`] writes
//! cells, with the theme and the interaction flags. [`EventCtx`] routes one
//! event against the retained tree, and holds pointer capture and the
//! transient values a gesture keeps between events. [`ScopeOptions`] shapes
//! how focus travels through a subtree.

use std::{
    any::{Any, type_name},
    collections::HashMap,
    fmt,
};

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect, Size};
use ratatui::widgets::{StatefulWidget, Widget};

use crate::Theme;

use super::engine::{DeclarationEnv, LayerKind, RenderPass};
use super::gesture::Press;
use super::{ChildId, Event, EventResult, KeyChord, MouseButton, MouseEvent, TabWrap};

/// Everything available while declaring one frame: how to declare children,
/// and the geometry and state to declare them from.
///
/// [`Ratcn`](super::Ratcn) creates this and threads it through the declaration
/// pass — the root closure gets one, and so does every component's
/// [`declare`](Component::declare). Only the library constructs it, and only
/// inside a pass, so everything here is always available.
///
/// Declaring does not paint. Nothing here writes to a cell — the context does
/// not carry the frame at all, only [`frame_area`](Self::frame_area), the one
/// thing about it a declaration has to know. Paint belongs to
/// [`Component::paint`] and to the closures [`paint`](Self::paint) queues,
/// which the runtime replays in declaration order once the whole tree is
/// known. The interaction flags a paint call styles from live on
/// [`PaintCtx`] for the same reason — focus resolves against the tree this
/// context is still building, so while it exists there is nothing to report.
/// Hover is the exception, because it predates the pass rather than following
/// from it: [`pointer_within`](Self::pointer_within) is readable here.
///
/// # Argument order
///
/// Every declaration ends with its payload, and one that carries an identity
/// opens with the [`ChildId`]. `area` takes the side of the payload that keeps
/// the call readable: it precedes a closure payload — [`in_area`](Self::in_area),
/// [`scope`](Self::scope), [`modal_scope`](Self::modal_scope),
/// [`popup`](Self::popup), [`hint`](Self::hint), [`viewport`](Self::viewport) —
/// and follows a value payload — [`component`](Self::component),
/// [`modal`](Self::modal), [`paint_widget`](Self::paint_widget), which is the
/// order `render_widget(widget, area)` reads in. Whatever else a declaration
/// needs sits between the two, as `viewport`'s content height and offset do.
pub struct DeclareCtx<'a, State, Msg> {
    pub(crate) frame_area: Rect,
    pub(crate) area: Rect,
    /// The active theme supplied to [`Ratcn::render`](super::Ratcn::render).
    pub theme: &'a Theme,
    pub(crate) hover_position: Option<Position>,
    pub(crate) transients: &'a mut TransientMap,
    pub(crate) pass: &'a mut RenderPass<State, Msg>,
    pub(crate) state: &'a State,
}

/// Where one identified declaration sits in this frame's focus and hover, as
/// the four flags paint styles from.
///
/// They travel as a unit because they are answered as one, from the same node
/// against the focus and the hover this frame resolved, at the one moment they
/// can be answered at all: after declaring has ended.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "two independent (leaf, within) flag pairs — focus and hover; the bools are the natural shape"
)]
pub(crate) struct InteractionFlags {
    pub(crate) focused: bool,
    pub(crate) contains_focus: bool,
    pub(crate) hovered: bool,
    pub(crate) contains_hover: bool,
}

impl<State, Msg> fmt::Debug for DeclareCtx<'_, State, Msg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeclareCtx")
            .field("area", &self.area)
            .field("frame_area", &self.frame_area)
            .field("theme", self.theme)
            .field("hover_position", &self.hover_position)
            .finish_non_exhaustive()
    }
}

impl<'a, State, Msg> DeclareCtx<'a, State, Msg> {
    /// Queue paint for the position this declaration occupies.
    ///
    /// The app-level counterpart of [`Component::paint`], for chrome that has
    /// no component of its own: a pane border, a background wash, a label.
    /// The closure runs once, during the replay that follows the declaration
    /// walk, at the point in the queue where this call was reached — so it
    /// paints in declaration order relative to the components around it, and
    /// before anything declared after it. Inside a [`modal`](Self::modal),
    /// [`popup`](Self::popup), or [`hint`](Self::hint) layer it lands on that
    /// layer's canvas and composites above everything declared outside;
    /// otherwise it lands on the frame.
    ///
    /// Because it runs after declaration has ended, the closure has to own
    /// what it draws with: it is `'static` and gets a [`PaintCtx`] rather
    /// than this context. That context reports the area and the interaction
    /// flags of the declaration `paint` was called from, plus the theme and
    /// the state the pass was declared with — everything style depends on.
    /// Layout the closure's caller computed must be moved in.
    ///
    /// The flags are the declaring node's, and the root closure has no
    /// identity of its own — paint queued there always reports all four as
    /// false. Enter a named [`scope`](Self::scope) when container chrome needs
    /// to know whether focus or the pointer is somewhere inside it.
    pub fn paint(&mut self, paint: impl FnOnce(&mut PaintCtx<'_, State>) + 'static) {
        let area = self.area;
        self.pass.queue_thunk(area, paint);
    }

    /// Queue one widget, drawn at `area`.
    ///
    /// The shorthand for the common single-write case: exactly
    /// `self.paint(move |ctx| ctx.widget(widget, area))`, queued at the same
    /// point and painted under the same rules — declaration order against the
    /// components around it, onto the enclosing layer's canvas when there is
    /// one.
    ///
    /// Reach for it when a write is independent: one widget, one area, nothing
    /// else in the op. Use [`paint`](Self::paint) when several writes share
    /// data the closure captures once, when a write reads the interaction
    /// flags, or when the writes belong together as one op — a background and
    /// the border over it must not be split into two, since anything declared
    /// between them would paint in the gap.
    ///
    /// The widget is owned: `'static` is what lets it outlive the declaration
    /// and travel to the replay. Widgets built from owned content qualify —
    /// `Paragraph::new(String)`, a `Block` with an owned title. One borrowing
    /// from app state or from a local does not, and wants
    /// [`paint`](Self::paint) with the borrowed parts turned into owned
    /// captures before the closure is built.
    pub fn paint_widget<W: Widget + 'static>(&mut self, widget: W, area: Rect) {
        self.paint(move |ctx| ctx.widget(widget, area));
    }

    /// The area supplied for the current declaration.
    #[must_use]
    pub const fn area(&self) -> Rect {
        self.area
    }

    /// The terminal frame area for this declaration pass.
    ///
    /// Unlike [`area`](Self::area), this is not changed by component, scope,
    /// modal, or popup declarations. Composite components use it when placing
    /// an overlay relative to their declaration while keeping that overlay
    /// within the terminal bounds.
    #[must_use]
    pub const fn frame_area(&self) -> Rect {
        self.frame_area
    }

    /// Whether the hover the previous frame resolved rests on the current
    /// declaration or on something inside it — a component and its children,
    /// a [`scope`](Self::scope), or from the root closure anything declared
    /// at all. Structure may depend on it; hover that changes without the
    /// pointer moving reaches it one frame after [`PaintCtx::contains_hover`].
    #[must_use]
    pub fn pointer_within(&self) -> bool {
        self.pass.pointer_within_current()
    }

    /// Whether the pointer rests inside this declaration *and* on the
    /// rectangle it was given.
    ///
    /// [`pointer_within`](Self::pointer_within) follows subtree identity
    /// across layers, so an escaped popup keeps its owner's answer true wherever
    /// the popup sits. This adds the geometric half, for a component whose own
    /// rectangle can move out from under a still pointer — through reflow, or
    /// because a [`viewport`](Self::viewport) scrolled it away.
    #[must_use]
    pub fn pointer_within_area(&self) -> bool {
        self.pointer_within()
            && self
                .hover_position
                .is_some_and(|position| self.area.contains(position))
    }

    /// Read the transient stored at the current declaration's identity path,
    /// if an event handler stored one.
    ///
    /// The declaration-time counterpart of [`EventCtx::transient`]: event
    /// handlers write scratch values that mean nothing to the app — a
    /// wheel-scrolled viewport offset, say — and the next declaration reads
    /// them here to lay out accordingly.
    ///
    /// `None` when no event handler has stored a value at this path. Like every
    /// transient, the value disappears as soon as its path stops being
    /// declared — see [`EventCtx::transient`] for the ownership rules; semantic
    /// state does not belong here.
    ///
    /// Use [`transient_mut`](Self::transient_mut) when the declaration must
    /// also settle the value it reads.
    ///
    /// # Panics
    ///
    /// Panics if the stored transient has a different type: one path holds one
    /// `T`, and reader and writer must agree on it.
    #[must_use]
    pub fn transient<T: 'static>(&self) -> Option<&T> {
        let path = self.pass.current_path()?;
        Some(self.transients.get(path)?.expect_ref(path))
    }

    /// [`transient`](Self::transient), for the rare value a declaration has to
    /// settle rather than merely read.
    ///
    /// Some presentation state can only be resolved once the layout is known,
    /// because only the layout answers it: whether a wheel-scrolled viewport
    /// still holds, given where the cursor now is, is the built-in example —
    /// [`List`](crate::List) settles it in its `declare`, alongside the
    /// arithmetic that produces the offset it stores.
    ///
    /// The write lands once per frame, where the declaration makes it, and is
    /// read back by the next frame's declaration — and by any event handler
    /// that writes it in between. Settling a flag
    /// (`if moved { held = false }`) or storing a computed offset is what
    /// this is for; anything the app should read, persist, or act on belongs
    /// in app state.
    ///
    /// Prefer writing from [`EventCtx::transient`] whenever an event can carry
    /// the change instead.
    ///
    /// `None` until an event handler has stored a value: this never inserts
    /// one, which is what keeps a transient's lifetime tied to the events
    /// that created it rather than to a pass that may yet fail.
    ///
    /// # Panics
    ///
    /// Panics if the stored transient has a different type: one path holds one
    /// `T`, and reader and writer must agree on it.
    pub fn transient_mut<T: 'static>(&mut self) -> Option<&mut T> {
        let path = self.pass.current_path()?;
        Some(self.transients.get_mut(path)?.expect_mut(path))
    }

    /// The app state supplied to the current declaration pass.
    #[must_use]
    pub const fn state(&self) -> &'a State {
        self.state
    }

    /// Run a declaration callback against an area override.
    ///
    /// This is how a composite hands a caller-supplied body the strip it laid
    /// out for it: the callback sees `area` as its [`area`](Self::area), while
    /// the identity scope stays the composite's, so anything the body declares
    /// is an ordinary sibling of the composite's other children and shares
    /// their id namespace. [`Dialog`](crate::Dialog) places its content and
    /// footer bodies this way. The call declares nothing of its own and takes
    /// no identity. [`EventCtx::with_area`] is a builder setter and a
    /// different thing.
    pub fn in_area(&mut self, area: Rect, declare: impl FnOnce(&mut DeclareCtx<'_, State, Msg>)) {
        let (pass, env) = self.declaring(area);
        pass.with_declare_ctx(env, declare);
    }

    /// Declare descendants in a vertically scrollable coordinate space.
    ///
    /// `screen` is the visible rectangle. Descendants are declared against
    /// logical content that shares its origin and width and is
    /// `content_height` rows tall, with `offset` naming the first content row
    /// on screen; a larger offset is clamped to the last one that fills the
    /// rectangle. Every widget paints with its full logical allocation, and
    /// the result is translated and clipped afterwards. Pointer input arrives
    /// in the same logical coordinates, and paint outside the logical content
    /// is clipped away.
    ///
    /// A [`popup`](Self::popup), [`hint`](Self::hint), or
    /// [`defer_paint`](Self::defer_paint) closure declared inside escapes the
    /// clip and keeps these logical coordinates, projected into screen
    /// coordinates once. A popup or hint anchored to a declaration the
    /// viewport has scrolled out of sight is skipped for the frame, and comes
    /// back with its anchor.
    ///
    /// A [`modal`](Self::modal) goes further and leaves the viewport behind:
    /// it takes its area in these coordinates like any other layer, opens at
    /// the place on screen they name, and declares in screen coordinates from
    /// there — so it may hold a viewport of its own.
    ///
    /// This is the mechanism behind [`ScrollArea`](crate::ScrollArea), and
    /// what a component of your own builds a viewport from. The offset such a
    /// component chooses on the runtime's behalf comes from
    /// [`Component::reveal_in_viewport`].
    ///
    /// # Panics
    ///
    /// Panics when a viewport is declared inside another, and when the
    /// logical content exceeds 262,144 cells. A [`modal`](Self::modal)
    /// declared between the two ends the enclosing viewport, so a scroll area
    /// inside a dialog inside a scroll area is ordinary nesting; a
    /// [`popup`](Self::popup) or [`hint`](Self::hint) keeps the viewport it
    /// was declared in, and a viewport inside one of those is nested.
    pub fn viewport(
        &mut self,
        screen: Rect,
        content_height: u16,
        offset: u16,
        declare: impl FnOnce(&mut DeclareCtx<'_, State, Msg>),
    ) {
        let (pass, env) = self.declaring(screen);
        pass.viewport(screen, content_height, offset, env, declare);
    }

    /// The pass and the declaration environment for a child covering `area`.
    ///
    /// Every declaration method needs both, and both come out of the same
    /// borrow of `self`, so they are produced together.
    fn declaring(
        &mut self,
        area: Rect,
    ) -> (&mut RenderPass<State, Msg>, DeclarationEnv<'_, State>) {
        let env = DeclarationEnv {
            frame_area: self.frame_area,
            area,
            state: self.state,
            theme: self.theme,
            transients: &mut *self.transients,
        };
        (&mut *self.pass, env)
    }

    /// Open a nested identity and focus scope around some descendants, without
    /// writing a component for it.
    ///
    /// Reach for this when a region needs its own Tab boundary, its own focus
    /// hotkey, or just a shared path segment — a pane in a layout, a toolbar, a
    /// screen — but has no behavior worth a [`Component`] impl. Children
    /// declared inside `declare` get `id` prepended to their paths, and
    /// `options` configures the scope exactly as a component's
    /// [`scope_options`](Component::scope_options) would.
    ///
    /// For hit-testing a scope sits behind its descendants: a click goes to the
    /// innermost child under the pointer and only reaches the scope when nothing
    /// inside was hit. `area` is both what the scope reports as its own area and
    /// what it hit-tests against, so a zero-area scope still parents its
    /// descendants but can never be hovered, mouse-focused, or clicked.
    ///
    /// # Panics
    ///
    /// Panics when `id` duplicates another child of the same parent.
    pub fn scope(
        &mut self,
        id: impl Into<ChildId>,
        area: Rect,
        options: ScopeOptions,
        declare: impl FnOnce(&mut DeclareCtx<'_, State, Msg>),
    ) {
        let (pass, env) = self.declaring(area);
        pass.scope(id.into(), options, env, declare);
    }

    /// Declare and paint a child component under the current scope.
    ///
    /// This is the main way components get onto the screen. `id` has to be
    /// unique among the current scope's children and stable across frames, and
    /// the order of these calls is the order Tab moves through the results.
    ///
    /// A component given zero width or height is still painted and still
    /// declared, but takes no part in focus traversal or mouse interaction on
    /// this surface — useful for a collapsed pane that should keep its identity.
    /// Use [`scope`](DeclareCtx::scope) instead when you only need to group
    /// descendants.
    ///
    /// # Panics
    ///
    /// Panics when `id` duplicates another child of the same parent.
    pub fn component(
        &mut self,
        id: impl Into<ChildId>,
        component: impl Component<State, Msg> + 'static,
        area: Rect,
    ) {
        let (pass, env) = self.declaring(area);
        pass.component(id.into(), component, env);
    }

    /// Declare a component as a modal layer, painted above everything
    /// declared outside it.
    ///
    /// Callable from anywhere — the root closure or any component's `declare`.
    /// The modal becomes a child of whatever is currently declaring, so its
    /// identity path, focus scope, and event bubbling anchor there, and a
    /// component can own its own confirmation dialog with one declaration
    /// guarded by one app-state flag. Layers composite in declaration order,
    /// wherever in the tree they are declared — except that a layer declared
    /// outside the topmost modal composites beneath it whatever the order,
    /// and is dimmed with everything else the modal covers.
    ///
    /// Modal policy: the area behind the modal is dimmed, events outside it
    /// are consumed rather than routed, focus resolves into it, and Tab wraps
    /// at its boundary. A key nothing inside handles still bubbles to the
    /// modal root rather than escaping beneath, so Esc-to-close works even
    /// when no descendant is focused.
    ///
    /// An empty interaction area retains the modal path but excludes the modal
    /// and its descendants from focus, hit-testing, and event routing.
    ///
    /// A modal escapes a [`viewport`](Self::viewport) it is declared inside.
    /// `area` is in the coordinates of the declaration that gave it, as every
    /// layer's is, and the modal opens at the place on screen those
    /// coordinates name; a row the viewport has scrolled past the top names
    /// the viewport's top edge. From there the modal is screen-level: its
    /// [`area`](Self::area), its [`frame_area`](Self::frame_area), and
    /// anything it declares are in screen coordinates, and it may open a
    /// viewport of its own.
    ///
    /// With [`Ratcn::modals`](super::Ratcn::modals) bound, a successful render
    /// must declare exactly the ids in the bound
    /// [`ModalState`](super::ModalState), in stack order.
    ///
    /// # Panics
    ///
    /// Panics when `id` duplicates another modal root's id.
    pub fn modal(
        &mut self,
        id: impl Into<ChildId>,
        component: impl Component<State, Msg> + 'static,
        area: Rect,
    ) {
        let (pass, env) = self.declaring(area);
        pass.modal(id.into(), component, env);
    }

    /// Declare a hint layer: a subtree painted above everything else that
    /// takes no input at all.
    ///
    /// This is the layer for tooltips and other content that explains rather
    /// than acts. Like [`modal`](Self::modal) and [`popup`](Self::popup) it is
    /// callable from anywhere and anchors at the current declaration, so
    /// `if showing { ctx.hint(...) }` inside a component is the whole
    /// ceremony. What separates it from a popup:
    ///
    /// - It is not a pointer target. A press over a hint goes to whatever the
    ///   hint covers, so a tooltip can never swallow the click it is
    ///   describing.
    /// - Nothing outside is captured and nothing is dimmed.
    /// - Focus is never moved into it, and keys bubble through to whatever
    ///   declared it.
    ///
    /// A hint anchors to the declaration it was reached from, and follows it
    /// out of sight: when a [`viewport`](Self::viewport) has scrolled that
    /// declaration off screen, the hint is skipped for the frame and returns
    /// when its anchor does.
    ///
    /// Because it takes no input, a hint has no dismissal of its own: whatever
    /// opened it — hover, focus — is what closes it, through your own state.
    /// It therefore takes plain [`ScopeOptions`] rather than a
    /// [`PopupOptions`] whose dismiss hook could never fire.
    ///
    /// # Panics
    ///
    /// Panics when `id` duplicates another child of the same parent.
    pub fn hint(
        &mut self,
        id: impl Into<ChildId>,
        area: Rect,
        options: ScopeOptions,
        declare: impl FnOnce(&mut DeclareCtx<'_, State, Msg>),
    ) {
        let (pass, env) = self.declaring(area);
        pass.layer_scope(id.into(), LayerKind::Hint, options, None, env, declare);
    }

    /// Declare a popup layer: a scope painted above everything declared
    /// outside it, without modal policy.
    ///
    /// This is the layer for dropdown panels, menus, and completion lists.
    /// Like [`modal`](Self::modal) it is callable from anywhere and anchors
    /// its subtree at the current declaration, so `if open { ctx.popup(...) }`
    /// inside a component's `declare` is the entire ceremony. Unlike a modal:
    ///
    /// - Nothing is dimmed, and nothing outside the popup is captured: a
    ///   press outside its footprint routes to whatever is visibly there — a
    ///   button under the pointer still presses.
    /// - The popup occludes exactly its own footprint. A press inside it that
    ///   nothing handles is consumed at the popup root, never delivered to
    ///   the control it covers.
    /// - Focus is never stolen. Move focus into the popup through your own
    ///   messages, in the same update that opens it.
    /// - A press outside the popup emits the
    ///   [`on_dismiss`](PopupOptions::on_dismiss) message, if one is bound —
    ///   dismissal and click-through compose, because the press dismisses and
    ///   the click that follows it activates.
    ///
    /// Keys bubble *through* the popup root to the declaring component, so an
    /// Esc nothing in the panel handles reaches whatever opened it.
    ///
    /// A popup anchors to the declaration it was reached from, and follows it
    /// out of sight: when a [`viewport`](Self::viewport) has scrolled that
    /// declaration off screen, the popup is skipped for the frame and returns
    /// when its anchor does.
    ///
    /// # Panics
    ///
    /// Panics when `id` duplicates another child of the same parent.
    pub fn popup(
        &mut self,
        id: impl Into<ChildId>,
        area: Rect,
        options: PopupOptions<Msg>,
        declare: impl FnOnce(&mut DeclareCtx<'_, State, Msg>),
    ) {
        let (pass, env) = self.declaring(area);
        pass.layer_scope(
            id.into(),
            LayerKind::Popup,
            options.scope_options,
            options.on_dismiss,
            env,
            declare,
        );
    }

    /// Declare a scope as a modal layer on top of everything declared so far —
    /// a modal built from plain declarations.
    ///
    /// The layer mechanics are exactly [`modal`](Self::modal)'s: the area
    /// behind is dimmed, the scope becomes the next modal root, and lower
    /// layers stop receiving events. What goes *inside* is yours, with the
    /// same context [`scope`](Self::scope) gives: paint chrome with the paint
    /// methods, declare children with [`component`](Self::component). Reach
    /// for this when a dialog-like layer should stay entirely app-owned;
    /// [`Dialog`](crate::Dialog) is the packaged alternative with chrome,
    /// dragging, and dismiss keys built in.
    ///
    /// `options` follows [`scope`](Self::scope)'s contract. Unlike a
    /// [`Dialog`](crate::Dialog), nothing here emits on Esc; a key nothing
    /// inside handles is absorbed by the layer, and dismissal is whatever
    /// message your own controls emit.
    ///
    /// With [`Ratcn::modals`](super::Ratcn::modals) bound, the id counts
    /// toward the bound [`ModalState`](super::ModalState) like any other
    /// modal root, and it escapes an enclosing
    /// [`viewport`](Self::viewport) exactly as [`modal`](Self::modal) does.
    ///
    /// # Panics
    ///
    /// Same conditions as [`modal`](Self::modal).
    pub fn modal_scope(
        &mut self,
        id: impl Into<ChildId>,
        area: Rect,
        options: ScopeOptions,
        declare: impl FnOnce(&mut DeclareCtx<'_, State, Msg>),
    ) {
        let (pass, env) = self.declaring(area);
        pass.modal_scope(id.into(), options, env, declare);
    }

    /// Schedule paint that should land on top of everything else in this layer.
    ///
    /// Ordinary paint happens in declaration order, so a component cannot draw
    /// over siblings declared after it. This defers a closure until the
    /// current layer has finished declaring: deferred paint registered inside
    /// a [`modal`](Self::modal) or [`popup`](Self::popup) flushes into that
    /// layer's canvas and composites with it, while deferred paint registered
    /// in the base declaration flushes after every layer has composited —
    /// making it the topmost decoration slot, where toast stacks and drag
    /// ghosts live.
    ///
    /// Deferred paint is decoration only: it has no identity, geometry, focus,
    /// hover, or hit target, and cannot be clicked. Its [`PaintCtx`] therefore
    /// reports all four interaction flags as false, and
    /// [`area`](PaintCtx::area) is the whole surface it writes to — the
    /// layer's footprint inside a layer, the frame otherwise.
    ///
    /// Because the closure runs after the declaration pass has ended, it does
    /// not get a `DeclareCtx`. It is `'static` and receives a [`PaintCtx`],
    /// which carries the theme and the app state the pass was declared with;
    /// anything else it needs must be moved into the closure here.
    pub fn defer_paint(&mut self, paint: impl FnOnce(&mut PaintCtx<'_, State>) + 'static) {
        self.pass.defer_paint(paint);
    }
}

/// Options for a [`popup`](DeclareCtx::popup) layer.
///
/// A popup needs little configuration — its whole point is that it behaves
/// like an ordinary subtree, just painted on top. The one popup-specific hook
/// is [`on_dismiss`](Self::on_dismiss); the inner scope's behavior can be
/// shaped with [`scope_options`](Self::scope_options).
pub struct PopupOptions<Msg> {
    pub(crate) scope_options: ScopeOptions,
    pub(crate) on_dismiss: Option<Box<dyn Fn() -> Msg>>,
}

impl<Msg> Default for PopupOptions<Msg> {
    fn default() -> Self {
        Self {
            scope_options: ScopeOptions::default(),
            on_dismiss: None,
        }
    }
}

impl<Msg> fmt::Debug for PopupOptions<Msg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PopupOptions")
            .field("scope_options", &self.scope_options)
            .field("on_dismiss", &self.on_dismiss.is_some())
            .finish()
    }
}

impl<Msg> PopupOptions<Msg> {
    /// The message emitted when a press lands outside the popup's footprint.
    ///
    /// The press itself still routes to whatever it hit — dismissal observes,
    /// it does not consume — so clicking a visible button both closes the
    /// popup (this message, applied on the press) and activates the button
    /// (its own message, on the click that follows). The hook fires only when
    /// routing the press produced no message of its own; when the press lands
    /// on a focusable control, the focus-change message is the dismissal
    /// signal instead, and the app closes the popup in that update.
    #[must_use]
    pub fn on_dismiss(mut self, message: impl Fn() -> Msg + 'static) -> Self {
        self.on_dismiss = Some(Box::new(message));
        self
    }

    /// Options for the popup's own scope — Tab wrapping, focusability, focus
    /// keys — with [`ScopeOptions`]' usual meanings.
    #[must_use]
    pub fn scope_options(mut self, options: ScopeOptions) -> Self {
        self.scope_options = options;
        self
    }
}

/// The surface paint lands on: the frame for the base layer, a layer's canvas
/// for paint that belongs to that layer.
enum PaintSurface<'a> {
    Frame(&'a mut Buffer),
    Canvas(&'a mut super::engine::Canvas),
}

impl PaintSurface<'_> {
    /// The rectangle this surface covers.
    fn area(&self) -> Rect {
        match self {
            Self::Frame(buffer) => buffer.area,
            Self::Canvas(canvas) => canvas.buffer.area,
        }
    }
}

/// Where paint lands, how it is projected on the way there, and the buffer it
/// lays out in when it is projected.
///
/// The three write forms are implemented once, here, because routing is the
/// only thing they do that [`PaintCtx`] does not.
pub(crate) struct PaintTarget<'a> {
    surface: PaintSurface<'a>,
    projection: Option<super::engine::Projection>,
    /// Where projected paint lays out before it is copied back. One buffer
    /// serves the whole frame's paint calls, resized and blanked per call.
    scratch: &'a mut Buffer,
}

impl<'a> PaintTarget<'a> {
    pub(crate) fn frame(
        buffer: &'a mut Buffer,
        projection: Option<super::engine::Projection>,
        scratch: &'a mut Buffer,
    ) -> Self {
        Self {
            surface: PaintSurface::Frame(buffer),
            projection,
            scratch,
        }
    }

    pub(crate) fn canvas(
        canvas: &'a mut super::engine::Canvas,
        projection: Option<super::engine::Projection>,
        scratch: &'a mut Buffer,
    ) -> Self {
        Self {
            surface: PaintSurface::Canvas(canvas),
            projection,
            scratch,
        }
    }

    /// Paint `area` through this target: `paint` receives the buffer to write
    /// and the rectangle to write it in.
    ///
    /// The three write forms differ only in what they hand that closure, so
    /// routing — which buffer, which clip, which projection, what counts as
    /// painted — is settled once, here.
    fn paint_at<R>(&mut self, area: Rect, paint: impl FnOnce(Rect, &mut Buffer) -> R) -> R {
        let surface = self.surface.area();
        match (&mut self.surface, self.projection) {
            (PaintSurface::Frame(buffer), None) => paint(area, buffer),
            (PaintSurface::Frame(buffer), Some(projection)) => {
                with_projected_buffer(buffer, self.scratch, projection, surface, area, |buffer| {
                    paint(area, buffer)
                })
            }
            (PaintSurface::Canvas(canvas), None) => {
                let clipped = canvas.clip(area);
                let result = paint(clipped, &mut canvas.buffer);
                canvas.mark_painted(clipped);
                result
            }
            (PaintSurface::Canvas(canvas), Some(projection)) => {
                let painted = projection.project_rect(area, surface);
                let result = with_projected_buffer(
                    &mut canvas.buffer,
                    self.scratch,
                    projection,
                    surface,
                    area,
                    |buffer| paint(area, buffer),
                );
                canvas.mark_painted(painted);
                result
            }
        }
    }

    /// The whole of what this target writes, in the coordinates its paint
    /// closure uses — the allocation a free-form [`Self::with_buffer`] covers.
    pub(crate) fn whole_area(&mut self) -> Rect {
        let surface = self.surface.area();
        self.projection
            .map_or(surface, |projection| projection.allocation(surface))
    }

    fn widget(&mut self, widget: impl Widget, area: Rect) {
        self.paint_at(area, |area, buffer| widget.render(area, buffer));
    }

    fn stateful_widget<W: StatefulWidget>(&mut self, widget: W, area: Rect, state: &mut W::State) {
        self.paint_at(area, |area, buffer| widget.render(area, buffer, state));
    }

    fn with_buffer<R>(&mut self, paint: impl FnOnce(&mut Buffer) -> R) -> R {
        let area = self.whole_area();
        self.paint_at(area, |_, buffer| paint(buffer))
    }
}

/// Paint `logical` into `target` through `projection`.
///
/// The closure sees `scratch` covering exactly the logical rectangle, so a
/// widget lays out against its declared allocation. `scratch` is blanked
/// first, so a widget reads a blank wherever it has written nothing. The
/// cells the projection reaches are then seeded from the target beforehand
/// and copied back afterwards, so what the closure leaves untouched keeps
/// whatever was already there, and what falls outside the projection's clip
/// stays out of the target.
fn with_projected_buffer<R>(
    target: &mut Buffer,
    scratch: &mut Buffer,
    projection: super::engine::Projection,
    surface: Rect,
    logical: Rect,
    paint: impl FnOnce(&mut Buffer) -> R,
) -> R {
    let cells = logical.area();
    assert!(
        cells <= super::engine::MAX_VIEWPORT_CELLS,
        "a paint inside a viewport covers {logical}, {cells} cells; the maximum is {}",
        super::engine::MAX_VIEWPORT_CELLS
    );
    scratch.resize(logical);
    scratch.reset();
    for (logical_position, screen_position) in projection.projected_positions(logical, surface) {
        if let (Some(source), Some(destination)) = (
            target.cell(screen_position),
            scratch.cell_mut(logical_position),
        ) {
            *destination = source.clone();
        }
    }

    let result = paint(scratch);

    for (logical_position, screen_position) in projection.projected_positions(logical, surface) {
        if let (Some(source), Some(destination)) = (
            scratch.cell(logical_position),
            target.cell_mut(screen_position),
        ) {
            *destination = source.clone();
        }
    }
    result
}

/// Everything painting one declaration needs: where to draw, what to draw
/// with, and where that declaration sits in this frame's interaction.
///
/// [`Component::paint`] gets one, and so does every closure queued with
/// [`DeclareCtx::paint`]. Both run during the replay that follows the
/// declaration walk, which is why this context can declare nothing: by the
/// time it exists the tree is closed and focus is resolved. That is also the
/// only reason it can carry the four interaction flags at all — they are
/// derived from that resolution, and there is nothing to derive them from
/// while the tree is still being built.
///
/// Painting goes through [`widget`](Self::widget),
/// [`stateful_widget`](Self::stateful_widget), and
/// [`with_buffer`](Self::with_buffer). The context keeps the frame's buffer
/// to itself, so a paint call can read `ctx.theme`,
/// [`ctx.state()`](Self::state), and the interaction flags while building its
/// widget argument.
pub struct PaintCtx<'a, State> {
    pub(crate) target: PaintTarget<'a>,
    /// The active theme supplied to [`Ratcn::render`](super::Ratcn::render).
    pub theme: &'a Theme,
    pub(crate) area: Rect,
    pub(crate) flags: InteractionFlags,
    pub(crate) hover_position: Option<Position>,
    pub(crate) state: &'a State,
}

impl<State> fmt::Debug for PaintCtx<'_, State> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PaintCtx")
            .field("area", &self.area)
            .field("theme", self.theme)
            .field("flags", &self.flags)
            .field("hover_position", &self.hover_position)
            .finish_non_exhaustive()
    }
}

impl<'a, State> PaintCtx<'a, State> {
    /// Paint a ratatui widget onto the active paint surface.
    ///
    /// The widget is consumed here; nothing is deferred or allocated. Because
    /// the context never lends out the buffer, the widget expression may read
    /// `ctx` freely (`ctx.theme`, [`state`](Self::state), interaction flags)
    /// in argument position.
    ///
    /// Inside a [`modal`](DeclareCtx::modal), [`popup`](DeclareCtx::popup), or
    /// [`hint`](DeclareCtx::hint) layer the paint lands on that layer's canvas
    /// and composites above everything declared outside it; otherwise it
    /// lands on the frame. A layer composites the widget's whole declared
    /// `area` opaquely — cells the widget left unwritten come through as
    /// empty rather than transparent, so paint a panel background first, as
    /// the built-in layers do.
    pub fn widget(&mut self, widget: impl Widget, area: Rect) {
        self.target.widget(widget, area);
    }

    /// Paint a ratatui stateful widget onto the active paint surface.
    ///
    /// The escape hatch for widgets that need a `&mut` widget state during
    /// paint (e.g. ratatui's `List` with `ListState`). Targets the same
    /// surface as [`widget`](Self::widget).
    pub fn stateful_widget<W: StatefulWidget>(
        &mut self,
        widget: W,
        area: Rect,
        state: &mut W::State,
    ) {
        self.target.stateful_widget(widget, area, state);
    }

    /// Run a paint closure over the active paint surface's raw cell buffer.
    ///
    /// The escape hatch for direct cell writes (`set_string`, `set_style`,
    /// per-cell edits). The closure receives only the buffer, so values read
    /// from `ctx` must be taken as arguments or moved in. Inside a layer, the
    /// buffer is the layer's canvas and the whole layer footprint counts as
    /// painted for compositing.
    pub fn with_buffer<R>(&mut self, paint: impl FnOnce(&mut Buffer) -> R) -> R {
        self.target.with_buffer(paint)
    }

    /// The area of the declaration this paint belongs to: a component's paint
    /// allocation, or the [`DeclareCtx::area`] a queued closure was reached
    /// with.
    #[must_use]
    pub const fn area(&self) -> Rect {
        self.area
    }

    /// This declaration is the focused leaf.
    #[must_use]
    pub const fn focused(&self) -> bool {
        self.flags.focused
    }

    /// The focus path passes through or ends at this declaration (the
    /// `focus-within` signal for e.g. pane border highlighting).
    #[must_use]
    pub const fn contains_focus(&self) -> bool {
        self.flags.contains_focus
    }

    /// This declaration is the hovered leaf. Independent of
    /// [`focused`](Self::focused): a component can be hovered without being
    /// focused, and vice versa.
    #[must_use]
    pub const fn hovered(&self) -> bool {
        self.flags.hovered
    }

    /// The hover path passes through or ends at this declaration (the
    /// `hover-within` signal).
    #[must_use]
    pub const fn contains_hover(&self) -> bool {
        self.flags.contains_hover
    }

    /// The pointer position from the most recent mouse event, if it is still
    /// inside the terminal.
    ///
    /// Raw geometry, for paint that has to know *where* in the declaration the
    /// pointer is — which row of a list, which tab of a row — rather than
    /// merely whether it is inside, which [`hovered`](Self::hovered) and
    /// [`contains_hover`](Self::contains_hover) already answer.
    #[must_use]
    pub const fn hover_position(&self) -> Option<Position> {
        self.hover_position
    }

    /// The app state the pass was declared with.
    #[must_use]
    pub const fn state(&self) -> &'a State {
        self.state
    }
}

/// How the focus scope around a component's descendants behaves.
///
/// A *scope* is one level of the identity tree. It gives its children a shared
/// parent path, and it is the boundary that Tab traversal, focus hotkeys, and
/// mouse focus all work against. Every component and [`scope`](DeclareCtx::scope)
/// opens one; these options say how that scope should behave.
///
/// The runtime reads these from [`Component::scope_options`] *before* the
/// component declares, because it must know the shape of the scope before
/// descendants are declared into it. They therefore cannot depend on anything
/// computed during paint — use [`Component::prepare`] if a claim depends on
/// app state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScopeOptions {
    pub(crate) tab_wrap: TabWrap,
    pub(crate) focusable: bool,
    pub(crate) hover_focus: bool,
    pub(crate) focus_keys: Vec<FocusKeyBinding>,
}

impl ScopeOptions {
    /// What Tab does when it runs off the end of this scope's children.
    ///
    /// See [`TabWrap`]: the default lets the event escape so an ancestor
    /// advances, which is how whole-app Tab order emerges. [`TabWrap::Wrap`]
    /// makes this scope a trap, as a dialog wants.
    #[must_use]
    pub const fn tab_wrap(mut self, tab_wrap: TabWrap) -> Self {
        self.tab_wrap = tab_wrap;
        self
    }

    /// Whether this scope holds focus itself, and so takes part in Tab
    /// traversal. Defaults to `false`.
    ///
    /// An interactive leaf answers from the props it was declared with, so a
    /// disabled button says `false`; anything that has to be derived from app
    /// state is settled in [`Component::prepare`] first. A container asks for
    /// it when it is a Tab stop in its own right — a scrollable pane with
    /// nothing focusable inside it, for instance. Focus still prefers a
    /// focusable descendant when there is one, so this only makes the scope a
    /// target when there isn't.
    ///
    /// The runtime also requires a non-empty
    /// [`Component::interaction_area`]; a zero-area declaration never
    /// participates in traversal regardless.
    #[must_use]
    pub const fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Make pointer motion inside this scope move focus, not just hover.
    ///
    /// Off by default, because hover moving focus would steal keystrokes from
    /// whatever the user is typing in as soon as the mouse drifted. Switch it
    /// on for layouts where following the mouse is the expected behavior, such
    /// as a pane grid.
    ///
    /// Motion onto a different direct child focuses that child's first
    /// focusable leaf. That emits a focus message like any other focus change;
    /// once your state reflects it, further motion descends normally.
    ///
    /// **Set this on the scope whose children the mouse should choose
    /// between**, which is usually the root — see
    /// [`Ratcn::hover_focus`](super::Ratcn::hover_focus). Only motion across
    /// this scope's *direct children* moves focus; motion between components
    /// inside one of those children does not. Setting it on a pane rather than
    /// on the grid that holds the panes is the common mistake: every drift
    /// between two controls in that pane then steals focus.
    ///
    /// Focus follows the mouse in but never out. Leaving the scope clears
    /// hover and leaves focus where it was.
    #[must_use]
    pub const fn hover_focus(mut self) -> Self {
        self.hover_focus = true;
        self
    }

    /// Bind a key chord that jumps focus to `path`, resolved relative to this
    /// scope.
    ///
    /// The classic use is pane hotkeys (`Alt+1`, `Alt+2`) declared once on a
    /// root or container. Bindings are checked after the focused component and
    /// its ancestors have declined the key, walking outward from the focused
    /// leaf's scope to the root, so an inner binding wins over an outer one for
    /// the same chord.
    ///
    /// A binding whose path names nothing in the current surface is skipped and
    /// the search continues. Landing on a container descends to its first
    /// focusable leaf; there is no memory of what was focused there before.
    #[must_use]
    pub fn focus_key(
        mut self,
        chord: impl Into<KeyChord>,
        path: impl IntoIterator<Item = impl Into<ChildId>>,
    ) -> Self {
        self.focus_keys.push(FocusKeyBinding {
            chord: chord.into(),
            path: path.into_iter().map(Into::into).collect(),
        });
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FocusKeyBinding {
    pub(crate) chord: KeyChord,
    pub(crate) path: Vec<ChildId>,
}

pub(crate) struct TransientValue {
    type_name: &'static str,
    value: Box<dyn Any>,
}

impl TransientValue {
    /// The stored value as a `T`, borrowed.
    ///
    /// # Panics
    ///
    /// Panics when this path stores another type: one path holds one `T`, and
    /// reader and writer must agree on it. The same goes for
    /// [`expect_mut`](Self::expect_mut) and
    /// [`expect_owned`](Self::expect_owned).
    pub(crate) fn expect_ref<T: 'static>(&self, path: &[ChildId]) -> &T {
        match self.value.downcast_ref::<T>() {
            Some(value) => value,
            None => transient_mismatch::<T>(path, self.type_name),
        }
    }

    /// The stored value as a `T`, borrowed mutably.
    pub(crate) fn expect_mut<T: 'static>(&mut self, path: &[ChildId]) -> &mut T {
        let stored = self.type_name;
        match self.value.downcast_mut::<T>() {
            Some(value) => value,
            None => transient_mismatch::<T>(path, stored),
        }
    }

    /// The stored value as a `T`, taken out of the store.
    pub(crate) fn expect_owned<T: 'static>(self, path: &[ChildId]) -> T {
        let stored = self.type_name;
        match self.value.downcast::<T>() {
            Ok(value) => *value,
            Err(_) => transient_mismatch::<T>(path, stored),
        }
    }
}

/// The one message for a transient read that names a type its path does not
/// hold.
fn transient_mismatch<T: 'static>(path: &[ChildId], stored: &'static str) -> ! {
    let requested = type_name::<T>();
    panic!("transient type mismatch at path {path:?}: stored `{stored}`, requested `{requested}`")
}

pub(crate) type TransientMap = HashMap<Vec<ChildId>, TransientValue>;

/// The extra facilities a component gets while handling an event.
///
/// Passed to [`Component::handle_event`] alongside the event and the app state.
/// It carries the component's identity path plus the two things a component
/// cannot obtain any other way: scratch storage that survives between events
/// ([`transient`](Self::transient)) and mouse capture
/// ([`capture_pointer`](Self::capture_pointer)).
#[derive(Default)]
pub struct EventCtx<'a> {
    path: Vec<ChildId>,
    area: Rect,
    transients: TransientStore<'a>,
    pub(super) pointer: PointerInputs<'a>,
}

/// Where a context's transients live: the runtime's store during dispatch,
/// otherwise one owned by the context — so a component under unit test can
/// use [`EventCtx::transient`], with values that live and die with the
/// context.
enum TransientStore<'a> {
    Runtime(&'a mut TransientMap),
    Detached(TransientMap),
}

impl Default for TransientStore<'_> {
    fn default() -> Self {
        Self::Detached(TransientMap::new())
    }
}

/// The pointer facts one dispatch carries into an [`EventCtx`].
#[derive(Default)]
pub(crate) struct PointerInputs<'a> {
    /// Where a [`EventCtx::capture_pointer`] claim is recorded.
    pub(crate) capture: Option<&'a mut Option<Vec<ChildId>>>,
    /// The button holding the capture this event belongs to.
    pub(crate) button: Option<MouseButton>,
    /// The event as it arrived, before any declaration-space projection.
    pub(crate) screen_mouse: Option<MouseEvent>,
    /// The press that opened the gesture this event continues, when the
    /// event reached this component through its capture claim. `None` for an
    /// event that arrived by hit-test.
    pub(crate) captured_press: Option<Press>,
}

impl fmt::Debug for EventCtx<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventCtx")
            .field("path", &self.path)
            .field(
                "transients_available",
                &matches!(self.transients, TransientStore::Runtime(_)),
            )
            .field("capture_button", &self.pointer.button)
            .field("screen_mouse", &self.pointer.screen_mouse)
            .finish()
    }
}

impl<'a> EventCtx<'a> {
    /// The context one dispatch hands a component, taking over the identity
    /// path the caller derived for it.
    pub(crate) fn at(
        path: Vec<ChildId>,
        area: Rect,
        transients: &'a mut TransientMap,
        pointer: PointerInputs<'a>,
    ) -> Self {
        Self {
            path,
            area,
            transients: TransientStore::Runtime(transients),
            pointer,
        }
    }

    /// The transient store to read and write, and this component's key into
    /// it: the runtime's store during dispatch, otherwise a private one owned
    /// by this context.
    ///
    /// Every transient access needs both, and the context already holds the
    /// key, so the two are borrowed together.
    fn transient_slot(&mut self) -> (&mut TransientMap, &[ChildId]) {
        let store = match &mut self.transients {
            TransientStore::Runtime(transients) => &mut **transients,
            TransientStore::Detached(transients) => transients,
        };
        (store, &self.path)
    }

    /// Set the area this context reports, for a context built outside a
    /// dispatch.
    ///
    /// The runtime fills the area in from the last successful render, so this
    /// is for unit tests: `EventCtx::default().with_area(rect)` gives a
    /// component the geometry its key and mouse handling needs, where a bare
    /// [`default`](Self::default) would hand it a zero-sized rect. Controls
    /// that page by viewport or hit-test rows behave differently at zero size,
    /// so supplying a real area is usually what makes such a test meaningful.
    ///
    /// It is public for the same reason
    /// [`DeclareCtx::in_area`](DeclareCtx::in_area) is: a component module
    /// copied into your own project should be testable exactly as it is here.
    #[must_use]
    pub fn with_area(mut self, area: Rect) -> Self {
        self.area = area;
        self
    }

    /// Identity path of the component currently receiving the event.
    #[must_use]
    pub fn path(&self) -> &[ChildId] {
        &self.path
    }

    /// The area this component was declared with on the last successful
    /// render — the same rect the event was hit-tested against.
    ///
    /// Inside a [`viewport`](DeclareCtx::viewport) this is content geometry,
    /// and the [`MouseEvent`] coordinates dispatched with it share that space.
    /// So do [`DragPhase`](super::DragPhase) positions; see
    /// [`drag`](Self::drag).
    ///
    /// Components that need event-time geometry — a dialog hit-testing its own
    /// border for a drag, say — read it from here, so nothing has to cache the
    /// area while declaring. Zero outside a [`Ratcn`](super::Ratcn) event
    /// dispatch, such as a unit test built from `EventCtx::default()`.
    #[must_use]
    pub const fn area(&self) -> Rect {
        self.area
    }

    /// Scratch storage of type `T`, keyed by this component's identity path.
    ///
    /// Some interactions span several events while meaning nothing to the app —
    /// bookkeeping like "which cell did this drag start on". That cannot live
    /// on the component, because every frame declares a fresh instance, and it
    /// should not clutter app state. This is the middle ground: `T::default()`
    /// on first access, then the same value on every later event, for as long
    /// as this path keeps appearing in successful renders. It survives the
    /// component instance being replaced and its siblings being reordered.
    ///
    /// Do not keep semantic state here. Anything the app should read, persist,
    /// or act on belongs in app state, reached by emitting a message. A
    /// transient is dropped as soon as its path stops being declared, and
    /// nothing warns you when that happens.
    ///
    /// The next declaration reads the same value back with
    /// [`DeclareCtx::transient`](DeclareCtx::transient), which is how a wheel
    /// scroll survives a redraw. Write from here whenever an event can carry
    /// the change; [`DeclareCtx::transient_mut`](DeclareCtx::transient_mut) is
    /// the narrow exception, for a value only the layout can settle.
    ///
    /// In a context built without a dispatch — `EventCtx::default()` in a
    /// component unit test — the value lives and dies with that context, so a
    /// component that uses a transient can be tested directly. Nothing
    /// persists from one such context to the next, so behavior that spans
    /// events is tested through [`Ratcn`](super::Ratcn).
    ///
    /// # Panics
    ///
    /// Panics if this path already stores a transient of a different type —
    /// one path holds one `T`.
    pub fn transient<T: Default + 'static>(&mut self) -> &mut T {
        let (store, path) = self.transient_slot();
        // `entry` would want an owned key; the borrowed one is enough to look
        // with, and only a first access stores a copy of it.
        if !store.contains_key(path) {
            store.insert(
                path.to_vec(),
                TransientValue {
                    type_name: type_name::<T>(),
                    value: Box::<T>::default(),
                },
            );
        }
        store
            .get_mut(path)
            .expect("the path holds a transient, stored just above if it did not")
            .expect_mut(path)
    }

    pub(super) fn transient_if_present<T: 'static>(&mut self) -> Option<&mut T> {
        let (store, path) = self.transient_slot();
        Some(store.get_mut(path)?.expect_mut(path))
    }

    pub(super) fn take_transient<T: 'static>(&mut self) -> Option<T> {
        let (store, path) = self.transient_slot();
        Some(store.remove(path)?.expect_owned(path))
    }

    /// Send the rest of this button's gesture here, wherever the pointer goes.
    ///
    /// Mouse events normally route by hit-testing, so a drag that leaves the
    /// component's area stops reaching it. Capturing on the `Down` makes every
    /// following `Drag` and the closing `Up` for that button arrive at this
    /// path regardless of pointer position — the usual way to implement
    /// dragging a scrollbar thumb or moving a dialog by its border.
    ///
    /// Only the first capture in a gesture takes effect. The capture is
    /// released when the gesture ends, and dropped if this path stops being
    /// declared before then.
    ///
    /// # Panics
    ///
    /// Panics unless called while handling the matching
    /// [`MouseKind::Down`](super::MouseKind::Down) in a
    /// [`Ratcn`](super::Ratcn) event dispatch — capture can only begin a
    /// gesture, not join one in progress.
    pub fn capture_pointer(&mut self, button: MouseButton) {
        assert_eq!(
            self.pointer.button,
            Some(button),
            "EventCtx::capture_pointer({button:?}) requires the matching MouseKind::Down"
        );
        let capture = self
            .pointer
            .capture
            .as_deref_mut()
            .expect("EventCtx::capture_pointer is unavailable outside Ratcn event dispatch");
        if capture.is_none() {
            *capture = Some(self.path.clone());
        }
    }
}

/// Which way a one-step move goes through an ordered sequence.
///
/// Two orderings in this library are traversed a step at a time, and both use
/// this type:
///
/// - **Focusable components**, in declaration order. Tab steps `Forward`,
///   Shift+Tab (`BackTab`) steps `Backward`.
/// - **A control's items**, in index order — list rows, tabs, select options.
///   The index arithmetic in [`linear_nav`](crate::linear_nav) takes this
///   argument: [`step_enabled`](crate::linear_nav::step_enabled) and
///   [`nav_key_target`](crate::linear_nav::nav_key_target).
///
/// Named `Step` rather than `Direction` because ratatui's prelude already
/// exports a `Direction` (the horizontal/vertical layout axis), and the two
/// mean different things.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Toward the next component or item in order.
    Forward,
    /// Toward the previous component or item in order.
    Backward,
}

/// Something that can be focused, painted, and handed events — the interactive
/// half of a ratcn component.
///
/// A component is built fresh from app state on every declaration pass, so it
/// must not own durable state. It reads `State`, keeps only declaration-derived
/// caches (its last painted area, for instance) and the props it was declared
/// with, and reports anything the app needs to know by returning a message.
///
/// # What happens each frame
///
/// 1. [`prepare`](Self::prepare) — pin what the steps below read out of app
///    state.
/// 2. [`scope_options`](Self::scope_options) — read *before* any painting,
///    because focus for the whole frame is decided in one pass.
/// 3. [`interaction_area`](Self::interaction_area) — derive the geometry used
///    for focus, hit-testing, and events from the final paint area.
/// 4. [`declare`](Self::declare) — lay out, and declare descendants if any.
/// 5. [`paint`](Self::paint) — draw, once the whole tree has been declared
///    and focus has resolved.
///
/// The instances from the last successful pass are then retained, and those are
/// the instances [`handle_event`](Self::handle_event) is called on afterwards —
/// possibly against app state newer than the one they were declared with.
pub trait Component<State, Msg> {
    /// Prepare this component from the state it is being declared with.
    ///
    /// The runtime runs this once per declaration, before it reads either of
    /// [`scope_options`](Component::scope_options) or
    /// [`interaction_area`](Component::interaction_area) — so a component may
    /// answer both from state computed here.
    ///
    /// That is what the hook is for: pinning declaration-time state once,
    /// rather than deriving it again in every answer.
    /// [`Select`](crate::Select) resolves here whether it is open. It is also
    /// where the built-ins fail loud on a malformed declaration —
    /// [`List`](crate::List), [`Select`](crate::Select), and
    /// [`Tabs`](crate::Tabs) assert their item values are unique — so the
    /// panic names the declaring component rather than surfacing later as a
    /// routing oddity. Put a check whose answer changes only with the props
    /// behind `cfg!(debug_assertions)`: every frame declares a fresh instance,
    /// so every frame runs this hook.
    ///
    /// Leaf components take their props as plain values at declaration and can
    /// ignore it.
    fn prepare(&mut self, _state: &State) {}

    /// The scope this component opens around its descendants. Read once, before
    /// [`declare`](Component::declare), so it cannot depend on paint.
    fn scope_options(&self) -> ScopeOptions {
        ScopeOptions::default()
    }

    /// Return the area used for focus, hit-testing, and event routing.
    ///
    /// The runtime calls this once with the final area passed to
    /// [`DeclareCtx::component`], after [`prepare`](Self::prepare) and
    /// before [`declare`](Self::declare). Painting still receives the original
    /// area. Returning an area with zero width or height keeps the component's
    /// identity and still calls `declare`, but excludes its whole subtree from
    /// focus traversal, hit-testing, pointer capture, and event routing for the
    /// retained surface.
    ///
    /// The default returns the supplied area unchanged. Override this when the
    /// interactive pixels occupy only part of the paint allocation. A non-empty
    /// result must be fully contained in the supplied area; interaction cannot
    /// extend beyond pixels the component was allowed to paint.
    ///
    /// # Panics
    ///
    /// [`Ratcn::render`](super::Ratcn::render) panics if this returns a non-empty
    /// area that is not fully contained in `area`. The failed pass does not
    /// replace the previous retained surface.
    fn interaction_area(&self, area: Rect) -> Rect {
        area
    }

    /// Declare the component: lay out its area, declare its descendants, and
    /// record whatever [`handle_event`](Self::handle_event) will need to read
    /// back.
    ///
    /// This paints nothing. What belongs here is everything the answer to
    /// "what exists, and where" is made of: layout arithmetic, child
    /// declarations, and the retained geometry event routing hit-tests
    /// against. None of it can depend on the interaction flags, which do not
    /// exist yet — focus resolves against the tree this is still building.
    ///
    /// Anything that draws belongs in [`paint`](Self::paint).
    fn declare(&mut self, ctx: &mut DeclareCtx<'_, State, Msg>);

    /// Paint the component. `ctx` carries the paint surface, area, app state,
    /// theme, and interaction state.
    ///
    /// Every component's paint is queued where [`declare`](Self::declare)
    /// declared it and replayed once the whole tree is known, so this runs
    /// exactly once per frame, with focus resolved. Order is declaration
    /// order, and a component is queued at the point it opens — before its
    /// descendants — so a container's background and border land beneath
    /// what it declares inside itself without any care taken here.
    ///
    /// A component that draws nothing of its own leaves this defaulted.
    fn paint(&mut self, _ctx: &mut PaintCtx<'_, State>) {}

    /// Offer this component an event.
    ///
    /// Return [`EventResult::Ignored`] (the default) to let it bubble to the
    /// parent, `Consumed` to stop it, or `Emit` to stop it and hand the app a
    /// message. `state` is current app state, which may be newer than the state
    /// this instance was declared with.
    ///
    /// An ignored primary-button `Down` also leaves the runtime free to apply
    /// its focus fallback after bubbling. Calling [`EventCtx::capture_pointer`]
    /// does not consume the event, so a component may capture and still return
    /// `Ignored` when it wants that fallback. Return `Consumed` to veto the
    /// fallback, or `Emit` when the component's message takes precedence.
    fn handle_event(
        &mut self,
        _event: &Event,
        _state: &State,
        _ctx: &mut EventCtx<'_>,
    ) -> EventResult<Msg> {
        EventResult::Ignored
    }

    /// Bring `target` into view inside this component's
    /// [`viewport`](DeclareCtx::viewport).
    ///
    /// The runtime calls this on the component that declared the viewport
    /// whenever focus lands on a descendant the viewport is clipping, at the
    /// start of a frame. Every way focus moves arrives here: Tab, a press, and
    /// a path the app's update function stores. `target` is the descendant's
    /// logical area, in the coordinates the viewport was declared with.
    ///
    /// The first frame whose retained surface can place the target reveals
    /// it. Focus arriving together with the surface that first declares its
    /// target — an app's startup focus, focus handed back as a modal closes, a
    /// row declared for the first time — is answered one frame later, by the
    /// render after that surface is retained.
    ///
    /// The offset the component chooses belongs in an
    /// [`EventCtx::transient`], which the declaration that follows reads. The
    /// reveal is a channel of its own: the app's focus message is emitted
    /// whatever happens here.
    fn reveal_in_viewport(&mut self, _target: Rect, _state: &State, _ctx: &mut EventCtx<'_>) {}
}

/// A [`Component`] that can report its preferred size before it is declared.
///
/// Layout containers use this small core contract without depending on a
/// specific component module.
pub trait MeasuredComponent<State, Msg>: Component<State, Msg> {
    /// The component's preferred width and height in terminal cells.
    ///
    /// A container may still hand over a smaller area when space is
    /// constrained, so a component must keep coping with an undersized area.
    fn measure(&self) -> Size;
}
