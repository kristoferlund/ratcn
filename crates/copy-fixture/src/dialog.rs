use std::fmt;

use ratatui::{
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Clear, Padding, Paragraph},
};

use ratcn::Theme;
use ratcn::runtime::{
    CellOffset, ChildId, Component, DragOptions, DragPhase, Event, EventCtx, EventResult, KeyChord,
    KeyCode, MeasuredComponent, PaintCtx, RenderCtx, ScopeOptions, TabWrap, clamp_offset,
    is_border, offset_rect, wrapped_height,
};
use ratcn::text_width::{display_width_u16, wrap_to_width};
use ratcn::theme::resolve_style;

type OnOffsetChangeFn<M> = Box<dyn Fn(CellOffset) -> M>;
type OnDismissFn<M> = Box<dyn Fn() -> M>;
const ACTION_SPACING: u16 = 2;
/// Cells of padding inside the border, on every side of the box.
const PADDING: u16 = 1;
/// Cells of chrome on each side of the box: the one-cell border plus
/// [`PADDING`]. `paint_dialog_box` paints exactly this and the layout
/// functions inset by it; deriving both from the same constant is what keeps
/// event hit-testing aligned with what was painted.
const EDGE: u16 = 1 + PADDING;

/// Every color a dialog can paint.
///
/// [`from_theme`](Self::from_theme) provides the standard mapping from a
/// [`Theme`]. Build or modify a style directly when one dialog needs different
/// colors, then pass it through [`Dialog::style`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogStyle {
    /// Border color around the dialog box.
    pub border: Color,
    /// Title color in the top border.
    pub title_foreground: Color,
    /// Background color filling the dialog box.
    pub background: Color,
    /// Text color of the description paragraph.
    pub description_foreground: Color,
}

impl DialogStyle {
    /// A neutral style using plain ANSI colors, for use without a [`Theme`].
    #[must_use]
    pub const fn fallback() -> Self {
        Self {
            border: Color::Gray,
            title_foreground: Color::White,
            background: Color::Reset,
            description_foreground: Color::DarkGray,
        }
    }

    /// Derive every dialog color from `theme`.
    ///
    /// This is what [`Dialog`] calls when no custom style is configured. Call
    /// it directly when a dialog should start from the active theme and alter
    /// only selected colors.
    #[must_use]
    pub const fn from_theme(theme: &Theme) -> Self {
        Self {
            border: theme.ring,
            title_foreground: theme.ring,
            background: theme.surface,
            description_foreground: theme.muted_foreground,
        }
    }
}

struct DialogDims<'a> {
    title: &'a str,
    description: &'a str,
    width: Option<u16>,
    height: Option<u16>,
    content_height: Option<u16>,
    footer_height: u16,
    footer_width: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::struct_field_names,
    reason = "these are three distinct rects; the `_area` suffix reads clearly"
)]
struct DialogLayout {
    box_area: Rect,
    main_area: Rect,
    footer_area: Rect,
}

fn dialog_box_base(area: Rect, dims: &DialogDims<'_>) -> Rect {
    if area.width == 0 || area.height == 0 {
        return Rect::ZERO;
    }

    // Automatic width: a 48-cell floor so short dialogs do not look cramped,
    // widened to fit the title (border corner, gap, and a title space on each
    // side) or the action row, and kept two cells off each screen edge.
    let title_width = display_width_u16(dims.title);
    let automatic_width = 48u16
        .max(title_width.saturating_add(6))
        .max(dims.footer_width.saturating_add(EDGE * 2))
        .min(area.width.saturating_sub(4))
        .max(1);
    let outer_width = dims
        .width
        .map_or(automatic_width, |width| width.max(1).min(area.width));
    let inner_width = outer_width.saturating_sub(EDGE * 2);
    // The footer plus the gap row that separates it from the main area.
    let footer_block = if dims.footer_height > 0 {
        dims.footer_height.saturating_add(1)
    } else {
        0
    };

    // The dialog only ever measures — a stated content height, or a
    // description's wrapped lines. Custom content without a height is
    // rejected at declaration, so there is no branch that guesses.
    let automatic_height = if let Some(content_height) = dims.content_height {
        content_height
            .saturating_add(footer_block)
            .saturating_add(EDGE * 2)
            .max(3)
    } else {
        wrapped_height(dims.description, inner_width)
            .saturating_add(footer_block)
            .saturating_add(EDGE * 2)
            .max(3)
    };
    let outer_height = dims
        .height
        .map_or(automatic_height, |height| height.max(1))
        .min(area.height);

    area.centered(
        Constraint::Length(outer_width),
        Constraint::Length(outer_height),
    )
}

fn dialog_layout(area: Rect, offset: CellOffset, dims: &DialogDims<'_>) -> DialogLayout {
    let base = dialog_box_base(area, dims);
    if base.width == 0 || base.height == 0 {
        return DialogLayout {
            box_area: Rect::ZERO,
            main_area: Rect::ZERO,
            footer_area: Rect::ZERO,
        };
    }
    let box_area = offset_rect(area, base, offset);
    let inner = Rect {
        x: box_area.x.saturating_add(EDGE),
        y: box_area.y.saturating_add(EDGE),
        width: box_area.width.saturating_sub(EDGE * 2),
        height: box_area.height.saturating_sub(EDGE * 2),
    };
    let (main_area, footer_area) = if dims.footer_height > 0 {
        let [main_area, _gap, footer_area] = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(dims.footer_height),
        ])
        .areas(inner);
        (main_area, footer_area)
    } else {
        (inner, Rect::ZERO)
    };
    DialogLayout {
        box_area,
        main_area,
        footer_area,
    }
}

fn paint_dialog_box<S>(
    ctx: &mut PaintCtx<'_, '_, S>,
    box_area: Rect,
    title: &str,
    style: DialogStyle,
) {
    if box_area.width == 0 || box_area.height == 0 {
        return;
    }
    ctx.render_widget(Clear, box_area);
    let mut block = Block::bordered()
        .border_style(Style::default().fg(style.border))
        .style(Style::default().bg(style.background))
        .padding(Padding::symmetric(PADDING, PADDING));
    if !title.is_empty() {
        block = block.title(
            Line::from(format!(" {title} ")).style(
                Style::default()
                    .fg(style.title_foreground)
                    .add_modifier(Modifier::BOLD),
            ),
        );
    }
    ctx.render_widget(block, box_area);
}

type StyleFn = Box<dyn Fn(&Theme) -> DialogStyle>;
/// A custom body closure, boxed for storage until the declaration paints it.
type BodyFn<S, M> = Box<dyn FnOnce(&mut RenderCtx<'_, S, M>)>;
/// One action's declaration, boxed with the component and id it carries.
type ActionFn<S, M> = Box<dyn FnOnce(&mut RenderCtx<'_, S, M>, Rect)>;

/// What fills the dialog's main area.
///
/// The closure is `FnOnce` and gone once painted, but the variant and its
/// height outlive it: `handle_event` recomputes the same box geometry between
/// frames and needs to know what the main area was sized for.
enum DialogBody<S, M> {
    None,
    /// The [`description`](Dialog::description) paragraph.
    Description,
    Content {
        height: u16,
        declare: Option<BodyFn<S, M>>,
    },
}

/// What fills the dialog's footer strip: nothing, a custom closure, or the
/// standard action row. The three are mutually exclusive by construction,
/// which is what the builder conflict assertions enforce.
enum DialogFooter<S, M> {
    None,
    Custom {
        height: u16,
        declare: Option<BodyFn<S, M>>,
    },
    Actions(Vec<ActionSlot<S, M>>),
}

/// One standard action: its measured size, and the declaration that puts it on
/// screen. The size stays readable after the declaration is consumed, because
/// event-time geometry sizes the action row from it.
struct ActionSlot<S, M> {
    declare: Option<ActionFn<S, M>>,
    size: ratatui::layout::Size,
}

impl<S, M> fmt::Debug for DialogBody<S, M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Description => f.write_str("Description"),
            Self::Content { height, .. } => write!(f, "Content({height})"),
        }
    }
}

impl<S, M> fmt::Debug for DialogFooter<S, M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Custom { height, .. } => write!(f, "Custom({height})"),
            Self::Actions(actions) => write!(f, "Actions({})", actions.len()),
        }
    }
}

/// A modal dialog: a centered, bordered box with a [`title`](Dialog::title) in
/// its top border, a main content area (a [`description`](Dialog::description)
/// paragraph, or a custom [`content`](Dialog::content) closure), and a
/// standard [`action`](Dialog::action) row or a custom [`footer`](Dialog::footer).
///
/// # Declaring one
///
/// A `Dialog` is an ordinary [`Component`], but declare it with
/// [`RenderCtx::modal`] rather than `render_component` — that is what puts it on
/// its own layer, above everything declared before it, and gives it the whole
/// layer's keyboard fallback. Tab cycles inside it instead of escaping, and a
/// key nothing inside handles is absorbed by the layer rather than reaching the
/// app beneath.
///
/// Wire [`on_dismiss`](Dialog::on_dismiss) and the dialog itself becomes a
/// focus target of last resort, so the dismiss key still lands somewhere when
/// nothing inside is focused. Without it the dialog is never focused itself —
/// there would be nothing for it to do with the key.
///
/// Opening and closing is the app's. Keep the open dialogs in a
/// [`ModalState`](ratcn::runtime::ModalState) and bind it with
/// [`Ratcn::modals`](ratcn::runtime::Ratcn::modals); that also handles saving and
/// restoring the focus the user had before the dialog opened.
///
/// # Children
///
/// Anything declared from the [`content`](Dialog::content) or
/// [`footer`](Dialog::footer) callbacks becomes a child of the dialog, sharing
/// its focus, hover, theme, event routing, and layer. Those two callbacks are
/// area overrides, not separate scopes: their children and any
/// [`action`](Dialog::action) buttons all live in one sibling namespace, so ids
/// must be unique across the three.
///
/// # No paint widget
///
/// Unlike the other components, `Dialog` has no `DialogWidget` half. Its frame is
/// computed as pure geometry and then painted, so `handle_event` can recompute
/// the same box for hit-testing without needing a `Frame` — which is what makes
/// dragging it by its border possible. Only the painted box participates in
/// pointer routing, so a non-modal dialog does not block controls outside it.
///
/// Standard actions need no manual measurement or placement:
///
/// ```
/// use ratcn::{Button, Dialog};
///
/// # enum Msg { Cancel, Save }
/// let _dialog: Dialog<(), Msg> = Dialog::new()
///     .title("Delete item")
///     .description("This cannot be undone.")
///     .action("cancel", Button::new("Cancel").secondary().on_press(|| Msg::Cancel))
///     .action("save", Button::new("Save").on_press(|| Msg::Save));
/// ```
pub struct Dialog<S, M> {
    title: String,
    description: String,
    width: Option<u16>,
    height: Option<u16>,
    body: DialogBody<S, M>,
    footer: DialogFooter<S, M>,
    dismiss_key: KeyChord,
    /// Declaration prop retained with the successful runtime surface.
    offset: CellOffset,
    on_offset_change: Option<OnOffsetChangeFn<M>>,
    on_dismiss: Option<OnDismissFn<M>>,
    tab_wrap: TabWrap,
    style: Option<StyleFn>,
    /// Original paint allocation retained for drag clamping after the runtime
    /// narrows this component's event area to the painted box.
    paint_area: Rect,
}

impl<S: 'static, M: 'static> fmt::Debug for Dialog<S, M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Dialog")
            .field("title", &self.title)
            .field("description", &self.description)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("body", &self.body)
            .field("footer", &self.footer)
            .field("dismiss_key", &self.dismiss_key)
            .field("offset", &self.offset)
            .field("on_offset_change", &self.on_offset_change.is_some())
            .field("on_dismiss", &self.on_dismiss.is_some())
            .field("tab_wrap", &self.tab_wrap)
            .field("style", &self.style.is_some())
            .field("paint_area", &self.paint_area)
            .finish()
    }
}

impl<S: 'static, M: 'static> Dialog<S, M> {
    /// Create an empty dialog. Its focus scope wraps
    /// ([`TabWrap::Wrap`]) so Tab cycles among its interactive descendants.
    #[must_use]
    pub fn new() -> Self {
        Self {
            title: String::new(),
            description: String::new(),
            width: None,
            height: None,
            body: DialogBody::None,
            footer: DialogFooter::None,
            dismiss_key: KeyChord::from(KeyCode::Esc),
            offset: CellOffset::default(),
            on_offset_change: None,
            on_dismiss: None,
            tab_wrap: TabWrap::Wrap,
            style: None,
            paint_area: Rect::default(),
        }
    }

    /// The title shown in the dialog's top border.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// A description paragraph for the main content area. The box auto-sizes to
    /// fit it. Ignored if a [`content`](Dialog::content) closure is set.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        // Content wins whichever order the two were called in.
        if !matches!(self.body, DialogBody::Content { .. }) {
            self.body = DialogBody::Description;
        }
        self
    }

    /// Set the preferred outer width in terminal cells, including the border
    /// and padding. The width is clamped to the area supplied to the dialog.
    #[must_use]
    pub const fn outer_width(mut self, width: u16) -> Self {
        self.width = Some(width);
        self
    }

    /// Set the preferred outer height in terminal cells, including the border,
    /// padding, content, and footer. The height is clamped to the area supplied
    /// to the dialog and takes precedence over automatic content measurement.
    #[must_use]
    pub const fn outer_height(mut self, height: u16) -> Self {
        self.height = Some(height);
        self
    }

    /// Draw the main content area yourself, instead of using
    /// [`description`](Dialog::description).
    ///
    /// The callback gets an ordinary [`RenderCtx`] whose
    /// [`area`](RenderCtx::area) is the content strip; paint into it and declare
    /// children with [`RenderCtx::render_component`] as usual. Those children
    /// belong to the dialog's scope, sharing one sibling namespace with the
    /// footer's children and any [`action`](Dialog::action) ids. Focusable
    /// children just work: the runtime discovers them as they declare, so
    /// there is nothing to announce.
    ///
    /// The dialog cannot measure an arbitrary closure, so `height` states the
    /// content strip's exact height in terminal rows.
    ///
    /// The closure is `FnOnce`, so it may consume owned values, but it is stored
    /// on the retained component and so must capture only `'static` values.
    #[must_use]
    pub fn content(
        mut self,
        height: u16,
        f: impl FnOnce(&mut RenderCtx<'_, S, M>) + 'static,
    ) -> Self {
        self.body = DialogBody::Content {
            height,
            declare: Some(Box::new(f)),
        };
        self
    }

    /// Add a measured component to the standard action row.
    ///
    /// Actions are end-aligned with standard spacing. Insertion order is both
    /// visual order (left to right) and focus traversal order. Use
    /// [`footer`](Dialog::footer) instead when the row needs custom layout.
    /// Action ids share the Dialog sibling namespace with custom content
    /// children.
    ///
    /// `action` accepts any component that implements [`MeasuredComponent`],
    /// the trait for components that can report the size they need — that is
    /// what lets the action row lay them out. See the trait's implementors for
    /// the current set. Route footer content that is not one of them through
    /// [`footer`](Dialog::footer).
    ///
    /// # Panics
    ///
    /// Panics if a custom footer was already configured.
    #[must_use]
    pub fn action(
        mut self,
        id: impl Into<ChildId>,
        component: impl MeasuredComponent<S, M> + 'static,
    ) -> Self {
        assert!(
            !matches!(self.footer, DialogFooter::Custom { .. }),
            "standard dialog actions cannot be combined with a custom footer"
        );
        let id = id.into();
        let slot = ActionSlot {
            size: component.measure(),
            declare: Some(Box::new(move |ctx, area| {
                ctx.render_component(id, component, area);
            })),
        };
        match &mut self.footer {
            DialogFooter::Actions(actions) => actions.push(slot),
            footer => *footer = DialogFooter::Actions(vec![slot]),
        }
        self
    }

    /// Lay out a `height`-row footer yourself, instead of the standard
    /// [`action`](Dialog::action) row.
    ///
    /// Reach for this when the row needs something the standard layout does not
    /// do — a checkbox on the left, a status message beside the buttons. The
    /// callback follows the same rules as [`content`](Dialog::content): an
    /// ordinary [`RenderCtx`] over the footer strip, children in the dialog's
    /// sibling namespace, `'static` captures.
    ///
    /// # Panics
    ///
    /// Panics if [`action`](Dialog::action) was already called. A dialog has one
    /// footer, standard or custom, not both.
    #[must_use]
    pub fn footer(
        mut self,
        height: u16,
        f: impl FnOnce(&mut RenderCtx<'_, S, M>) + 'static,
    ) -> Self {
        assert!(
            !matches!(self.footer, DialogFooter::Actions(_)),
            "a custom dialog footer cannot be combined with standard actions"
        );
        self.footer = DialogFooter::Custom {
            height,
            declare: Some(Box::new(f)),
        };
        self
    }

    /// How far the box sits from its centered position, in cells.
    ///
    /// A dialog is centered by default; this shifts it. Pass the offset your app
    /// currently stores, each frame. On its own this just moves the box — pair it
    /// with [`on_offset_change`](Dialog::on_offset_change) to let the user drag
    /// it.
    #[must_use]
    pub const fn offset(mut self, offset: CellOffset) -> Self {
        self.offset = offset;
        self
    }

    /// Make the dialog draggable by its border, and say what to emit as it
    /// moves.
    ///
    /// Fires on every step of the drag rather than only on release, so the box
    /// follows the pointer live — which requires storing the offset and passing
    /// it back through [`offset`](Dialog::offset). The emitted value is clamped
    /// to keep the box inside the area supplied to the dialog.
    #[must_use]
    pub fn on_offset_change(mut self, on_change: impl Fn(CellOffset) -> M + 'static) -> Self {
        self.on_offset_change = Some(Box::new(on_change));
        self
    }

    /// Make the dismiss key — `Esc` unless [`dismiss_key`](Dialog::dismiss_key)
    /// says otherwise — dismiss the dialog, emitting the message `build`
    /// returns (the app names the close action — typically the same one the
    /// Cancel button emits). Without this the dialog emits no dismissal; when
    /// declared as a modal, the runtime still absorbs the unhandled key instead
    /// of routing it to the base UI.
    ///
    /// Wiring this is also what makes the dialog itself a focus target: focus
    /// prefers a focusable descendant (an action, a custom child) and falls
    /// back to the dialog only when there is none, so the dismiss key still
    /// has somewhere to land. A dialog without `on_dismiss` is never focused
    /// itself.
    #[must_use]
    pub fn on_dismiss(mut self, on_dismiss: impl Fn() -> M + 'static) -> Self {
        self.on_dismiss = Some(Box::new(on_dismiss));
        self
    }

    /// Which key dismisses the dialog (default `Esc`).
    ///
    /// Only meaningful together with [`on_dismiss`](Dialog::on_dismiss), which
    /// supplies the message to emit. Accepts anything that converts into a
    /// [`KeyChord`], so a bare `char` or [`KeyCode`] works, with
    /// [`ctrl`](KeyChord::ctrl) / [`alt`](KeyChord::alt) for combinations:
    ///
    /// ```
    /// use ratcn::{Dialog, runtime::KeyChord};
    ///
    /// # enum Msg { Close }
    /// let _dialog: Dialog<(), Msg> = Dialog::new()
    ///     .on_dismiss(|| Msg::Close)
    ///     .dismiss_key(KeyChord::from('w').ctrl());
    /// ```
    #[must_use]
    pub fn dismiss_key(mut self, key: impl Into<KeyChord>) -> Self {
        self.dismiss_key = key.into();
        self
    }

    /// Override the Tab wrap-around behavior (default [`TabWrap::Wrap`]).
    #[must_use]
    pub const fn tab_wrap(mut self, wrap: TabWrap) -> Self {
        self.tab_wrap = wrap;
        self
    }

    /// Replace the theme-derived [`DialogStyle`].
    ///
    /// The closure receives the active theme on each declaration pass, so deriving
    /// the result from that argument follows runtime theme changes. Return a
    /// fixed style to keep the same colors under every theme.
    ///
    /// ```
    /// use ratcn::{Dialog, DialogStyle};
    ///
    /// let _dialog: Dialog<(), ()> = Dialog::new().style(|theme| {
    ///     let mut style = DialogStyle::from_theme(theme);
    ///     style.border = theme.accent;
    ///     style
    /// });
    /// ```
    #[must_use]
    pub fn style(mut self, style: impl Fn(&Theme) -> DialogStyle + 'static) -> Self {
        self.style = Some(Box::new(style));
        self
    }

    /// The standard action row, empty unless a footer of actions was built.
    fn actions(&self) -> &[ActionSlot<S, M>] {
        match &self.footer {
            DialogFooter::Actions(actions) => actions,
            DialogFooter::None | DialogFooter::Custom { .. } => &[],
        }
    }

    fn dims(&self) -> DialogDims<'_> {
        let action_height = self
            .actions()
            .iter()
            .map(|slot| slot.size.height)
            .max()
            .unwrap_or(0);
        let action_width = self
            .actions()
            .iter()
            .enumerate()
            .fold(0u16, |width, (index, slot)| {
                width
                    .saturating_add(slot.size.width)
                    .saturating_add(if index > 0 { ACTION_SPACING } else { 0 })
            });
        DialogDims {
            title: &self.title,
            description: &self.description,
            width: self.width,
            height: self.height,
            content_height: match &self.body {
                DialogBody::Content { height, .. } => Some(*height),
                DialogBody::None | DialogBody::Description => None,
            },
            footer_height: match &self.footer {
                DialogFooter::Custom { height, .. } => *height,
                DialogFooter::None | DialogFooter::Actions(_) => action_height,
            },
            footer_width: action_width,
        }
    }

    fn drag_enabled(&self) -> bool {
        self.on_offset_change.is_some()
    }
}

impl<S: 'static, M: 'static> Default for Dialog<S, M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: 'static, M: 'static> Component<S, M> for Dialog<S, M> {
    fn render(&mut self, ctx: &mut RenderCtx<'_, S, M>) {
        let area = ctx.area();
        self.paint_area = area;
        let layout = dialog_layout(area, self.offset, &self.dims());
        match &mut self.body {
            DialogBody::None | DialogBody::Description => {}
            DialogBody::Content { declare, .. } => {
                if let Some(declare) = declare.take() {
                    ctx.in_area(layout.main_area, declare);
                }
            }
        }
        match &mut self.footer {
            DialogFooter::None => {}
            DialogFooter::Custom { declare, .. } => {
                if let Some(declare) = declare.take() {
                    ctx.in_area(layout.footer_area, declare);
                }
            }
            DialogFooter::Actions(actions) => {
                let constraints = actions
                    .iter()
                    .map(|slot| Constraint::Length(slot.size.width))
                    .collect::<Vec<_>>();
                let areas = Layout::horizontal(constraints)
                    .flex(Flex::End)
                    .spacing(ACTION_SPACING)
                    .split(layout.footer_area);
                for (slot, column) in actions.iter_mut().zip(areas.iter()) {
                    // Bottom-align each action within its column of the row.
                    let height = slot.size.height.min(column.height);
                    let area = Rect::new(
                        column.x,
                        column.bottom().saturating_sub(height),
                        column.width,
                        height,
                    );
                    if let Some(declare) = slot.declare.take() {
                        declare(ctx, area);
                    }
                }
                debug_assert!(
                    actions.iter().all(|slot| slot.declare.is_none()),
                    "every dialog action must be declared"
                );
            }
        }
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, S>) {
        let layout = dialog_layout(ctx.area(), self.offset, &self.dims());
        let style = resolve_style(self.style.as_deref(), ctx.theme, DialogStyle::from_theme);
        // Queued where the dialog was declared, so the box lands beneath
        // everything declared inside it without being painted first here.
        paint_dialog_box(ctx, layout.box_area, &self.title, style);
        if matches!(self.body, DialogBody::Description) && !self.description.is_empty() {
            // Paint from the same wrap that sized the box (`wrapped_height` in
            // `dialog_box_base`), so the description can never clip or pad the
            // height it asked for.
            let lines = wrap_to_width(&self.description, usize::from(layout.main_area.width))
                .into_iter()
                .map(Line::from)
                .collect::<Vec<_>>();
            ctx.render_widget(
                Paragraph::new(lines).style(Style::default().fg(style.description_foreground)),
                layout.main_area,
            );
        }
    }

    fn scope_options(&self) -> ScopeOptions {
        // A dialog itself is only a useful fallback focus target when it can
        // handle its dismiss key. Descendants remain independently focusable.
        let options = ScopeOptions::default().tab_wrap(self.tab_wrap);
        if self.on_dismiss.is_some() {
            options.focusable()
        } else {
            options
        }
    }

    fn interaction_area(&self, area: Rect) -> Rect {
        dialog_layout(area, self.offset, &self.dims()).box_area
    }

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &S,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<M> {
        if let Event::Key(key) = event
            && self.dismiss_key.matches(key)
            && let Some(on_dismiss) = &self.on_dismiss
        {
            return EventResult::Emit(on_dismiss());
        }
        let Event::Mouse(mouse) = event else {
            return EventResult::Ignored;
        };
        // EventCtx exposes the narrowed interaction area; paint_area retains
        // the original allocation needed to clamp the app-owned offset.
        let box_area = ctx.area();
        let base = dialog_box_base(self.paint_area, &self.dims());
        let can_start = self.drag_enabled() && is_border(box_area, mouse.column, mouse.row);
        match ctx.drag(mouse, DragOptions::new(self.offset).start_if(can_start)) {
            DragPhase::Down | DragPhase::Ended { .. } => EventResult::Consumed,
            DragPhase::Moved { offset, .. } => {
                self.on_offset_change
                    .as_ref()
                    .map_or(EventResult::Consumed, |on_offset_change| {
                        EventResult::Emit(on_offset_change(clamp_offset(
                            self.paint_area,
                            base,
                            offset,
                        )))
                    })
            }
            DragPhase::Ignored => EventResult::Ignored,
        }
    }
}
