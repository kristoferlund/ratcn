use std::{
    any::{Any, type_name},
    collections::{HashMap, hash_map::Entry},
    fmt,
};

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect, Size};
use ratatui::widgets::{StatefulWidget, Widget};

use crate::Theme;

use super::engine::{DeclarationEnv, LayerKind, RenderPass};
use super::{ChildId, Event, HoverState, KeyChord, MouseButton, TabWrap};

/// What a component did with an event, and what should happen next.
///
/// Events travel from a component up through its ancestors. This value decides
/// whether that continues: `Ignored` passes the event to the parent, while both
/// `Consumed` and `Emit` stop it there.
///
/// The same enum comes back out of
/// [`Ratcn::handle_event`](super::Ratcn::handle_event), so the app sees the
/// final outcome after bubbling has finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventResult<Msg> {
    /// Not handled here. The parent gets a chance at it, and if nothing in the
    /// chain handles it the app sees `Ignored` and can treat it as a global
    /// hotkey.
    Ignored,
    /// Handled, with nothing for the app to do — a keypress that only moved an
    /// internal cursor, for example.
    Consumed,
    /// Handled, and the app should apply this message. Implies consumed.
    ///
    /// Components never write app state themselves; this message is how a
    /// change reaches the app's update function.
    Emit(Msg),
}

/// Everything available while declaring one frame: where to paint, what to
/// paint with, and how to declare children.
///
/// [`Ratcn`](super::Ratcn) creates this and threads it through the declaration
/// pass — the root closure gets one, and so does every component's
/// [`render`](Component::render). Only the library constructs it.
///
/// Painting goes through [`render_widget`](RenderCtx::render_widget),
/// [`render_stateful_widget`](RenderCtx::render_stateful_widget), and
/// [`with_buffer`](RenderCtx::with_buffer). The context deliberately never
/// lends out the ratatui `Frame`: keeping it private means a paint call can
/// read `ctx.theme`, [`ctx.state()`](RenderCtx::state), and the interaction
/// flags while building its widget argument, which a borrowed frame would
/// prevent.
///
/// The four interaction flags describe whichever component, [`scope`](Self::scope),
/// or modal root is currently declaring. The root closure has no identity of
/// its own, so its flags are always false — enter a named `scope` when container
/// paint needs to know whether focus or the pointer is somewhere inside it.
#[expect(
    clippy::struct_excessive_bools,
    reason = "two independent (leaf, within) flag pairs — focus and hover; the bools are the natural shape"
)]
pub struct RenderCtx<'a, 'frame, State, Msg> {
    pub(crate) frame: &'a mut Frame<'frame>,
    pub(crate) area: Rect,
    /// The active theme supplied to [`Ratcn::render`](super::Ratcn::render).
    pub theme: &'a Theme,
    /// The current identified declaration is the focused leaf.
    pub focused: bool,
    /// The focus path passes through or ends at this identified declaration
    /// (the `focus-within` signal for e.g. pane border highlighting).
    pub contains_focus: bool,
    /// The current identified declaration is the hovered leaf. Independent
    /// of `focused`: a component can be hovered without being focused, and vice
    /// versa.
    pub hovered: bool,
    /// The hover path passes through or ends at this identified declaration (the
    /// `hover-within` signal).
    pub contains_hover: bool,
    pub(crate) hover_position: Option<Position>,
    pub(crate) hover: &'a HoverState,
    pub(crate) transients: Option<&'a mut TransientMap>,
    pub(crate) depth: usize,
    pub(crate) pass: Option<&'a mut RenderPass<State, Msg>>,
    pub(crate) state: Option<&'a State>,
}

impl<State, Msg> fmt::Debug for RenderCtx<'_, '_, State, Msg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderCtx")
            .field("area", &self.area)
            .field("theme", self.theme)
            .field("focused", &self.focused)
            .field("contains_focus", &self.contains_focus)
            .field("hovered", &self.hovered)
            .field("contains_hover", &self.contains_hover)
            .field("hover_position", &self.hover_position)
            .field("depth", &self.depth)
            .field("declaration_active", &self.pass.is_some())
            .finish_non_exhaustive()
    }
}

impl<'a, 'frame, State, Msg> RenderCtx<'a, 'frame, State, Msg> {
    /// Whether paint calls reach a paint surface: false only during the
    /// structure pass, where the same declarations run with every write
    /// suppressed. A context built without a pass (a paint-widget unit test)
    /// always paints.
    fn paints(&self) -> bool {
        self.pass.as_deref().is_none_or(RenderPass::paints)
    }

    /// Paint a ratatui widget onto the active paint surface, immediately.
    ///
    /// The widget is consumed here; nothing is deferred or allocated. Because
    /// the context never lends out the frame, the widget expression may read
    /// `ctx` freely (`ctx.theme`, [`state`](RenderCtx::state), interaction
    /// flags) in argument position.
    ///
    /// Inside a [`modal`](Self::modal) or [`popup`](Self::popup) layer the
    /// paint lands on that layer's canvas and composites above everything
    /// declared outside it; otherwise it lands on the frame. A layer
    /// composites the widget's whole declared `area` opaquely — cells the
    /// widget left unwritten come through as empty rather than transparent,
    /// so paint a panel background first, as the built-in layers do. During
    /// the structure pass (see [`Ratcn::render`](super::Ratcn::render)) the
    /// call is a no-op: the widget is built and dropped, nothing is painted.
    pub fn render_widget(&mut self, widget: impl Widget, area: Rect) {
        if !self.paints() {
            return;
        }
        if let Some(canvas) = self
            .pass
            .as_deref_mut()
            .and_then(RenderPass::active_canvas_mut)
        {
            widget.render(area.intersection(canvas.buffer.area), &mut canvas.buffer);
            canvas.mark_painted(area);
        } else {
            self.frame.render_widget(widget, area);
        }
    }

    /// Paint a ratatui stateful widget onto the active paint surface,
    /// immediately.
    ///
    /// The escape hatch for widgets that need a `&mut` widget state during
    /// paint (e.g. ratatui's `List` with `ListState`). Targets the same
    /// surface as [`render_widget`](Self::render_widget); a no-op during the
    /// structure pass — note the widget state is then not touched either.
    pub fn render_stateful_widget<W: StatefulWidget>(
        &mut self,
        widget: W,
        area: Rect,
        state: &mut W::State,
    ) {
        if !self.paints() {
            return;
        }
        if let Some(canvas) = self
            .pass
            .as_deref_mut()
            .and_then(RenderPass::active_canvas_mut)
        {
            widget.render(
                area.intersection(canvas.buffer.area),
                &mut canvas.buffer,
                state,
            );
            canvas.mark_painted(area);
        } else {
            self.frame.render_stateful_widget(widget, area, state);
        }
    }

    /// Run a paint closure over the active paint surface's raw cell buffer,
    /// immediately.
    ///
    /// The escape hatch for direct cell writes (`set_string`, `set_style`,
    /// per-cell edits). The closure receives only the buffer, so values read
    /// from `ctx` must be taken as arguments or moved in. Inside a layer, the
    /// buffer is the layer's canvas and the whole layer footprint counts as
    /// painted for compositing.
    ///
    /// During the structure pass the closure runs against a scratch buffer
    /// instead — its return value may feed declarations, so it cannot simply
    /// be skipped — and whatever it writes there is discarded. Reads then see
    /// only earlier `with_buffer` writes, never painted widget output, so a
    /// return value that feeds declarations must not depend on painted
    /// content.
    #[expect(
        clippy::missing_panics_doc,
        reason = "the expect is unreachable: a suppressed paint call implies an active pass"
    )]
    pub fn with_buffer<R>(&mut self, paint: impl FnOnce(&mut Buffer) -> R) -> R {
        if !self.paints() {
            let area = self.frame.area();
            let pass = self
                .pass
                .as_deref_mut()
                .expect("a suppressed paint call always has a pass");
            return paint(pass.scratch_buffer(area));
        }
        if let Some(canvas) = self
            .pass
            .as_deref_mut()
            .and_then(RenderPass::active_canvas_mut)
        {
            let area = canvas.buffer.area;
            canvas.mark_painted(area);
            paint(&mut canvas.buffer)
        } else {
            paint(self.frame.buffer_mut())
        }
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
    pub fn frame_area(&self) -> Rect {
        self.frame.area()
    }

    /// The pointer position from the most recent mouse event, if it is still
    /// inside the terminal.
    ///
    /// This is paint-only runtime information. It does not change the app-owned
    /// [`HoverState`], which continues to describe the hovered declaration path.
    #[must_use]
    pub const fn hover_position(&self) -> Option<Position> {
        self.hover_position
    }

    /// Read the transient stored at the current declaration's identity path,
    /// if an event handler stored one.
    ///
    /// The render-time counterpart of [`EventCtx::transient`]: event handlers
    /// write scratch values that mean nothing to the app — a wheel-scrolled
    /// viewport offset, say — and the next render reads them here to paint
    /// accordingly.
    ///
    /// `None` when no event handler has stored a value at this path, and in a
    /// context built outside a [`Ratcn`](super::Ratcn) declaration pass (a
    /// paint-widget unit test). Like every transient, the value disappears as
    /// soon as its path stops being declared — see
    /// [`EventCtx::transient`] for the ownership rules; semantic state does
    /// not belong here.
    ///
    /// Use [`transient_mut`](Self::transient_mut) when paint must also settle
    /// the value it reads.
    ///
    /// # Panics
    ///
    /// Panics if the stored transient has a different type: one path holds one
    /// `T`, and reader and writer must agree on it.
    #[must_use]
    pub fn transient<T: 'static>(&self) -> Option<&T> {
        let transients = self.transients.as_deref()?;
        let path = self.pass.as_deref()?.current_path()?;
        Some(transients.get(path)?.expect_ref(path))
    }

    /// [`transient`](Self::transient), for the rare value paint has to settle
    /// rather than merely read.
    ///
    /// Some presentation state can only be resolved while painting, because
    /// only paint knows the geometry: whether a wheel-scrolled viewport still
    /// holds, given where the cursor now is, is the built-in example.
    ///
    /// **The update must be idempotent.** The declaration closure runs twice
    /// per frame (see [`Ratcn::render`](super::Ratcn::render)), so whatever
    /// this writes is written twice and must reach the same value the second
    /// time — and the value the paint pass reads must be the one the structure
    /// pass would have read. Settling a flag (`if moved { held = false }`) or
    /// storing a computed offset qualifies; counting, appending, or toggling
    /// does not. Declared structure must never depend on it.
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
        let path = self.pass.as_deref()?.current_path()?.to_vec();
        let transients = self.transients.as_deref_mut()?;
        Some(transients.get_mut(&path)?.expect_mut(&path))
    }

    /// The app state supplied to the current declaration pass.
    ///
    /// # Panics
    ///
    /// Panics when this context was constructed outside a [`Ratcn`](super::Ratcn)
    /// declaration pass.
    #[must_use]
    pub fn state(&self) -> &'a State {
        self.state
            .expect("RenderCtx::state is unavailable outside a Ratcn declaration pass")
    }

    /// Run a declaration callback against an area override.
    ///
    /// Advanced composite components use this to assign independently chosen
    /// areas to their immediate children while preserving the current identity
    /// scope. It is public so copied component modules can retain the same
    /// runtime contract as their built-in counterpart.
    ///
    /// Not to be confused with [`EventCtx::with_area`], which is a builder
    /// setter: this one opens a sub-area for a callback and declares nothing of
    /// its own.
    #[doc(hidden)]
    pub fn in_area(
        &mut self,
        area: Rect,
        render: impl FnOnce(&mut RenderCtx<'_, 'frame, State, Msg>),
    ) {
        let mut ctx = RenderCtx {
            frame: &mut *self.frame,
            area,
            theme: self.theme,
            focused: self.focused,
            contains_focus: self.contains_focus,
            hovered: self.hovered,
            contains_hover: self.contains_hover,
            hover_position: self.hover_position,
            hover: self.hover,
            transients: self.transients.as_deref_mut(),
            depth: self.depth,
            pass: self.pass.as_deref_mut(),
            state: self.state,
        };
        render(&mut ctx);
    }

    /// The pass and the declaration environment for a child covering `area`.
    ///
    /// Every declaration method needs both, and both come out of the same
    /// borrow of `self`, so they are produced together. This is also the one
    /// place `pass` and `state` are unwrapped: a [`RenderCtx`] built outside a
    /// declaration pass can paint but cannot declare, and `method` names the
    /// caller so the panic says which call was out of place.
    fn declaring(
        &mut self,
        area: Rect,
        method: &str,
    ) -> (
        &mut RenderPass<State, Msg>,
        DeclarationEnv<'_, 'frame, State>,
    ) {
        let state = self
            .state
            .unwrap_or_else(|| panic!("{method} called without declaration state"));
        let env = DeclarationEnv {
            frame: &mut *self.frame,
            area,
            state,
            theme: self.theme,
            hover: self.hover,
            transients: self.transients.as_deref_mut(),
            depth: self.depth,
        };
        let pass = self
            .pass
            .as_deref_mut()
            .unwrap_or_else(|| panic!("{method} called outside a Ratcn declaration pass"));
        (pass, env)
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
    /// Panics when called outside a [`Ratcn`](super::Ratcn) declaration pass or when
    /// `id` duplicates another child of the same parent.
    pub fn scope(
        &mut self,
        id: impl Into<ChildId>,
        area: Rect,
        options: ScopeOptions,
        declare: impl FnOnce(&mut RenderCtx<'_, 'frame, State, Msg>),
    ) {
        let (pass, env) = self.declaring(area, "scope");
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
    /// Use [`scope`](RenderCtx::scope) instead when you only need to group
    /// descendants.
    ///
    /// # Panics
    ///
    /// Panics when called outside a [`Ratcn`](super::Ratcn) declaration pass
    /// or when `id` duplicates another child of the same parent.
    pub fn render_component(
        &mut self,
        id: impl Into<ChildId>,
        component: impl Component<State, Msg> + 'static,
        area: Rect,
    ) {
        let (pass, env) = self.declaring(area, "render_component");
        pass.render_component(id.into(), component, env);
    }

    /// Render a component prepared before this composite's scope options were
    /// resolved.
    #[doc(hidden)]
    pub fn render_prepared_component(
        &mut self,
        id: ChildId,
        component: PreparedComponent<State, Msg>,
        area: Rect,
    ) {
        let (pass, env) = self.declaring(area, "render_prepared_component");
        pass.render_prepared_component(id, component, env);
    }

    /// Declare a component as a modal layer, painted above everything
    /// declared outside it.
    ///
    /// Callable from anywhere — the root closure or any component's render.
    /// The modal becomes a child of whatever is currently declaring, so its
    /// identity path, focus scope, and event bubbling anchor there, and a
    /// component can own its own confirmation dialog with one declaration
    /// guarded by one app-state flag. Layer stacking is declaration order:
    /// each modal or [`popup`](Self::popup) paints above all layers declared
    /// before it, wherever in the tree the declarations happen.
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
    /// With [`Ratcn::modals`](super::Ratcn::modals) bound, a successful render
    /// must declare exactly the ids in the bound
    /// [`ModalState`](super::ModalState), in stack order.
    ///
    /// # Panics
    ///
    /// Panics outside a [`Ratcn`](super::Ratcn) declaration pass, or when `id`
    /// duplicates another modal root's id.
    pub fn modal(
        &mut self,
        id: impl Into<ChildId>,
        component: impl Component<State, Msg> + 'static,
        area: Rect,
    ) {
        let (pass, env) = self.declaring(area, "modal");
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
    /// Because it takes no input, a hint has no dismissal of its own: whatever
    /// opened it — hover, focus — is what closes it, through your own state.
    /// It therefore takes plain [`ScopeOptions`] rather than a
    /// [`PopupOptions`] whose dismiss hook could never fire.
    ///
    /// # Panics
    ///
    /// Panics outside a [`Ratcn`](super::Ratcn) declaration pass, or when `id`
    /// duplicates another child of the same parent.
    pub fn hint(
        &mut self,
        id: impl Into<ChildId>,
        options: ScopeOptions,
        area: Rect,
        declare: impl FnOnce(&mut RenderCtx<'_, 'frame, State, Msg>),
    ) {
        let (pass, env) = self.declaring(area, "hint");
        pass.layer_scope(id.into(), LayerKind::Hint, options, None, env, declare);
    }

    /// Declare a popup layer: a scope painted above everything declared
    /// outside it, without modal policy.
    ///
    /// This is the layer for dropdown panels, menus, and completion lists.
    /// Like [`modal`](Self::modal) it is callable from anywhere and anchors
    /// its subtree at the current declaration, so `if open { ctx.popup(...) }`
    /// inside a component's render is the entire ceremony. Unlike a modal:
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
    /// # Panics
    ///
    /// Panics outside a [`Ratcn`](super::Ratcn) declaration pass, or when `id`
    /// duplicates another child of the same parent.
    pub fn popup(
        &mut self,
        id: impl Into<ChildId>,
        options: PopupOptions<Msg>,
        area: Rect,
        declare: impl FnOnce(&mut RenderCtx<'_, 'frame, State, Msg>),
    ) {
        let (pass, env) = self.declaring(area, "popup");
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
    /// a hand-rolled modal, built from plain declarations instead of a
    /// component.
    ///
    /// The layer mechanics are exactly [`modal`](Self::modal)'s: the area
    /// behind is dimmed, the scope becomes the next modal root, and lower
    /// layers stop receiving events. What goes *inside* is yours, with the
    /// same context [`scope`](Self::scope) gives: paint chrome with the paint
    /// methods, declare children with
    /// [`render_component`](Self::render_component). Reach for this when a
    /// dialog-like layer should stay entirely app-owned;
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
    /// modal root.
    ///
    /// # Panics
    ///
    /// Same conditions as [`modal`](Self::modal).
    pub fn modal_scope(
        &mut self,
        id: impl Into<ChildId>,
        area: Rect,
        options: ScopeOptions,
        declare: impl FnOnce(&mut RenderCtx<'_, 'frame, State, Msg>),
    ) {
        let (pass, env) = self.declaring(area, "modal_scope");
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
    /// hover, or hit target, and cannot be clicked.
    ///
    /// Because the closure runs after the declaration pass has ended, it does
    /// not get a `RenderCtx`. It receives a [`Painter`] over the frame (which
    /// carries the theme) and the app state the pass was declared with;
    /// anything else it needs must be moved into the closure here.
    ///
    /// # Panics
    ///
    /// Panics when called outside a [`Ratcn`](super::Ratcn) declaration pass.
    pub fn defer_paint(&mut self, paint: impl FnOnce(&mut Painter<'_, '_>, &State) + 'static) {
        self.pass
            .as_deref_mut()
            .expect("defer_paint called outside a Ratcn declaration pass")
            .defer_paint(paint);
    }
}

/// Options for a [`popup`](RenderCtx::popup) layer.
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

/// Where deferred paint lands: the frame for the base layer, a layer's canvas
/// for paint deferred inside that layer.
pub(crate) enum PaintTarget<'a, 'frame> {
    Frame(&'a mut Frame<'frame>),
    Canvas(&'a mut super::engine::LayerCanvas),
}

/// Paint-only access to the active paint surface, handed to deferred paint
/// closures.
///
/// Mirrors the paint surface of [`RenderCtx`]: widgets and raw buffer writes,
/// never the frame itself. Paint deferred inside a modal or popup layer lands
/// on that layer's canvas and composites with it.
pub struct Painter<'a, 'frame> {
    pub(crate) target: PaintTarget<'a, 'frame>,
    /// The active theme supplied to [`Ratcn::render`](super::Ratcn::render).
    pub theme: &'a Theme,
}

impl fmt::Debug for Painter<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Painter").finish_non_exhaustive()
    }
}

impl Painter<'_, '_> {
    /// Paint a ratatui widget onto the active paint surface.
    pub fn render_widget(&mut self, widget: impl Widget, area: Rect) {
        match &mut self.target {
            PaintTarget::Frame(frame) => frame.render_widget(widget, area),
            PaintTarget::Canvas(canvas) => {
                widget.render(area.intersection(canvas.buffer.area), &mut canvas.buffer);
                canvas.mark_painted(area);
            }
        }
    }

    /// Paint a ratatui stateful widget onto the active paint surface.
    pub fn render_stateful_widget<W: StatefulWidget>(
        &mut self,
        widget: W,
        area: Rect,
        state: &mut W::State,
    ) {
        match &mut self.target {
            PaintTarget::Frame(frame) => frame.render_stateful_widget(widget, area, state),
            PaintTarget::Canvas(canvas) => {
                widget.render(
                    area.intersection(canvas.buffer.area),
                    &mut canvas.buffer,
                    state,
                );
                canvas.mark_painted(area);
            }
        }
    }

    /// Run a paint closure over the active paint surface's raw cell buffer.
    /// Inside a layer, the whole layer footprint counts as painted.
    pub fn with_buffer<R>(&mut self, paint: impl FnOnce(&mut Buffer) -> R) -> R {
        match &mut self.target {
            PaintTarget::Frame(frame) => paint(frame.buffer_mut()),
            PaintTarget::Canvas(canvas) => {
                let area = canvas.buffer.area;
                canvas.mark_painted(area);
                paint(&mut canvas.buffer)
            }
        }
    }
}

/// How the focus scope around a component's descendants behaves.
///
/// A *scope* is one level of the identity tree. It gives its children a shared
/// parent path, and it is the boundary that Tab traversal, focus hotkeys, and
/// mouse focus all work against. Every component and [`scope`](RenderCtx::scope)
/// opens one; these options say how that scope should behave.
///
/// The runtime reads these from [`Component::scope_options`] *before* the
/// component renders, because it must know the shape of the scope before
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

    /// Let this scope hold focus itself, not only pass it to descendants.
    ///
    /// Needed when a container is a Tab stop in its own right — a scrollable
    /// pane with nothing focusable inside it, for instance. Focus still prefers
    /// a focusable descendant when there is one, so this only makes the scope a
    /// target when there isn't. A zero-area declaration never participates in
    /// traversal regardless.
    #[must_use]
    pub const fn focusable(mut self) -> Self {
        self.focusable = true;
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
    /// Immutable typed access for render-time reads. The mutable accessors on
    /// [`EventCtx`] own the write paths and the type-establishing insert, so a
    /// stored value always carries the type its writer declared.
    /// The mutable counterpart of [`expect_ref`](Self::expect_ref), for
    /// [`RenderCtx::transient_mut`].
    pub(crate) fn expect_mut<T: 'static>(&mut self, path: &[ChildId]) -> &mut T {
        let requested = type_name::<T>();
        let stored = self.type_name;
        assert_eq!(
            stored, requested,
            "transient type mismatch at path {path:?}: stored `{stored}`, requested `{requested}`",
        );
        self.value.downcast_mut::<T>().unwrap_or_else(|| {
            panic!(
                "transient type metadata mismatch at path {path:?}: stored `{stored}`, requested `{requested}`",
            )
        })
    }

    pub(crate) fn expect_ref<T: 'static>(&self, path: &[ChildId]) -> &T {
        let requested = type_name::<T>();
        assert_eq!(
            self.type_name, requested,
            "transient type mismatch at path {path:?}: stored `{}`, requested `{requested}`",
            self.type_name
        );
        self.value.downcast_ref::<T>().unwrap_or_else(|| {
            panic!(
                "transient type metadata mismatch at path {path:?}: stored `{}`, requested `{requested}`",
                self.type_name
            )
        })
    }
}

pub(crate) type TransientMap = HashMap<Vec<ChildId>, TransientValue>;

/// A component prepared before its parent composite declares descendants.
///
/// This supports advanced copyable composites such as `Dialog`.
#[doc(hidden)]
pub struct PreparedComponent<State, Msg> {
    pub(crate) component: Box<dyn Component<State, Msg>>,
    pub(crate) options: ScopeOptions,
    pub(crate) self_focusable: bool,
    pub(crate) focuses_on_click: bool,
}

impl<State, Msg> fmt::Debug for PreparedComponent<State, Msg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedComponent")
            .field("self_focusable", &self.self_focusable)
            .field("focuses_on_click", &self.focuses_on_click)
            .finish_non_exhaustive()
    }
}

impl<State, Msg> PreparedComponent<State, Msg> {
    /// Run [`Component::prepare`] against `state` and record the claims the
    /// runtime must know before descendants are declared: the component's
    /// [`ScopeOptions`], whether it can hold focus, and whether a click rather
    /// than a press focuses it.
    #[doc(hidden)]
    pub fn prepare(mut component: Box<dyn Component<State, Msg>>, state: &State) -> Self {
        component.prepare(state);
        let options = component.scope_options();
        let self_focusable = component.is_focusable(state);
        let focuses_on_click = component.focuses_on_click(state);
        Self {
            component,
            options,
            self_focusable,
            focuses_on_click,
        }
    }
}

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
    transients: Option<&'a mut TransientMap>,
    /// Stands in for the runtime's store when this context was built without
    /// one, so a component under unit test can use
    /// [`transient`](Self::transient) without special-casing. Values live and
    /// die with this context, which is exactly what "no dispatch" means.
    detached_transients: Option<Box<TransientMap>>,
    capture: Option<&'a mut Option<Vec<ChildId>>>,
    capture_button: Option<MouseButton>,
}

impl fmt::Debug for EventCtx<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventCtx")
            .field("path", &self.path)
            .field("transients_available", &self.transients.is_some())
            .field("capture_button", &self.capture_button)
            .finish()
    }
}

impl<'a> EventCtx<'a> {
    pub(crate) fn at(
        path: &[ChildId],
        area: Rect,
        transients: &'a mut TransientMap,
        capture: &'a mut Option<Vec<ChildId>>,
        capture_button: Option<MouseButton>,
    ) -> Self {
        Self {
            path: path.to_vec(),
            area,
            transients: Some(transients),
            detached_transients: None,
            capture: Some(capture),
            capture_button,
        }
    }

    /// The transient store to read and write: the runtime's during dispatch,
    /// otherwise a private one owned by this context.
    fn transients_mut(&mut self) -> &mut TransientMap {
        match &mut self.transients {
            Some(transients) => transients,
            None => self.detached_transients.get_or_insert_with(Box::default),
        }
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
    /// [`RenderCtx::in_area`](RenderCtx::in_area) is: a component module
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
    /// Components that need event-time geometry (a dialog hit-testing its own
    /// border for a drag, say) read it from here instead of caching the area
    /// themselves during render. Zero outside a [`Ratcn`](super::Ratcn) event
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
    /// Paint reads the same value back with
    /// [`RenderCtx::transient`](RenderCtx::transient), which is how a wheel
    /// scroll survives a redraw. Write from here whenever an event can carry
    /// the change; [`RenderCtx::transient_mut`](RenderCtx::transient_mut) is
    /// the narrow exception, for a value only paint can settle.
    ///
    /// In a context built without a dispatch — `EventCtx::default()` in a
    /// component unit test — the value lives and dies with that context
    /// instead of the runtime's store, so a component that uses a transient
    /// can still be tested directly. Nothing persists from one such context
    /// to the next, so behavior that spans events has to be tested through
    /// [`Ratcn`](super::Ratcn).
    ///
    /// # Panics
    ///
    /// Panics if this path already stores a transient of a different type —
    /// one path holds one `T`.
    pub fn transient<T: Default + 'static>(&mut self) -> &mut T {
        let requested = type_name::<T>();
        let path = self.path.clone();
        let value = match self.transients_mut().entry(path.clone()) {
            Entry::Vacant(entry) => entry.insert(TransientValue {
                type_name: requested,
                value: Box::<T>::default(),
            }),
            Entry::Occupied(entry) => {
                let stored = entry.get().type_name;
                assert_eq!(
                    stored, requested,
                    "transient type mismatch at path {path:?}: stored `{stored}`, requested `{requested}`",
                );
                entry.into_mut()
            }
        };
        let stored = value.type_name;
        value.value.downcast_mut::<T>().unwrap_or_else(|| {
            panic!(
                "transient type metadata mismatch at path {path:?}: stored `{stored}`, requested `{requested}`",
            )
        })
    }

    pub(super) fn transient_if_present<T: 'static>(&mut self) -> Option<&mut T> {
        let path = self.path.clone();
        let value = self.transients_mut().get_mut(&path)?;
        let requested = type_name::<T>();
        let stored = value.type_name;
        assert_eq!(
            stored, requested,
            "transient type mismatch at path {path:?}: stored `{stored}`, requested `{requested}`",
        );
        Some(value.value.downcast_mut::<T>().unwrap_or_else(|| {
            panic!(
                "transient type metadata mismatch at path {path:?}: stored `{stored}`, requested `{requested}`",
            )
        }))
    }

    pub(super) fn take_transient<T: 'static>(&mut self) -> Option<T> {
        let path = self.path.clone();
        let value = self.transients_mut().remove(&path)?;
        let requested = type_name::<T>();
        let stored = value.type_name;
        assert_eq!(
            stored, requested,
            "transient type mismatch at path {path:?}: stored `{stored}`, requested `{requested}`",
        );
        Some(*value.value.downcast::<T>().unwrap_or_else(|_| {
            panic!(
                "transient type metadata mismatch at path {path:?}: stored `{stored}`, requested `{requested}`",
            )
        }))
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
            self.capture_button,
            Some(button),
            "EventCtx::capture_pointer({button:?}) requires the matching MouseKind::Down"
        );
        let capture = self
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
///   [`page_enabled`](crate::linear_nav::page_enabled).
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
/// must not own durable state. It reads `State`, keeps only render-derived
/// caches (its last painted area, for instance) and the props it was declared
/// with, and reports anything the app needs to know by returning a message.
///
/// # What happens each frame
///
/// 1. [`prepare`](Self::prepare) — prepare from the declaring state.
/// 2. [`scope_options`](Self::scope_options) and
///    [`is_focusable`](Self::is_focusable) — read *before* any painting,
///    because focus for the whole frame is decided in one pass.
/// 3. [`interaction_area`](Self::interaction_area) — derive the geometry used
///    for focus, hit-testing, and events from the final paint area.
/// 4. [`render`](Self::render) — paint, and declare descendants if any.
///
/// The instances from the last successful pass are then retained, and those are
/// the instances [`handle_event`](Self::handle_event) is called on afterwards —
/// possibly against app state newer than the one they were declared with.
pub trait Component<State, Msg> {
    /// Prepare this component from the state it is being declared with, before
    /// [`scope_options`](Component::scope_options) is read.
    ///
    /// The hook exists for composites whose scope claims depend on their
    /// children — `Dialog` decides here whether it will have focusable
    /// descendants. Leaf components take their props as plain values at
    /// declaration and can ignore it.
    fn prepare(&mut self, _state: &State) {}

    /// Paint the component. `ctx` carries the paint surface, area, app state,
    /// theme, and interaction state.
    ///
    /// Paint your own background and border *before* declaring descendants.
    /// Hit-testing follows declaration order and knows nothing about direct
    /// frame writes, so painting over a child after declaring it hides the
    /// child visually while it still swallows clicks.
    fn render(&mut self, ctx: &mut RenderCtx<'_, '_, State, Msg>);

    /// The scope this component opens around its descendants. Read once, before
    /// [`render`](Component::render), so it cannot depend on paint.
    fn scope_options(&self) -> ScopeOptions {
        ScopeOptions::default()
    }

    /// Return the area used for focus, hit-testing, and event routing.
    ///
    /// The runtime calls this once with the final area passed to
    /// [`RenderCtx::render_component`], after [`prepare`](Self::prepare) and
    /// before [`render`](Self::render). Painting still receives the original
    /// area. Returning an area with zero width or height keeps the component's
    /// identity and still calls `render`, but excludes its whole subtree from
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

    /// Whether the component can hold focus, and so takes part in Tab
    /// traversal. Defaults to `false`; interactive leaves override it, often
    /// gated on state (e.g. a disabled button is not focusable). The runtime
    /// also requires [`interaction_area`](Self::interaction_area) to return a
    /// non-empty area.
    fn is_focusable(&self, _state: &State) -> bool {
        false
    }

    /// Whether a synthesized click (rather than a primary press) focuses this
    /// component. Defaults to `false`: most components focus on `Down`. A
    /// component returning `true` is not focused by `Down` and is instead
    /// focused by its follow-up `Click`.
    #[doc(hidden)]
    fn focuses_on_click(&self, _state: &State) -> bool {
        false
    }
}

/// A [`Component`] that can report its preferred size before rendering.
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
