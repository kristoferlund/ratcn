use std::fmt;

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect, Size},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Widget},
};

use ratcn::Theme;
use ratcn::button_shape::{BOTTOM_CAP, TOP_CAP, cap_row, filled_middle, shape_width};
use ratcn::color::{
    DISABLED_DIM, FOCUS_DARKEN, FOCUS_LIGHTEN, HOVER_DARKEN, HOVER_LIGHTEN, darken, dim, lighten,
};
use ratcn::runtime::{
    Component, Event, EventCtx, EventResult, KeyCode, MeasuredComponent, MouseButton, MouseKind,
    PaintCtx, RenderCtx, fixed_height,
};
use ratcn::theme::resolve_style;

/// How tall a button is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ButtonSize {
    /// One row: label only, no room for a border.
    #[default]
    Small,
    /// Three rows: label with a border or fill cap above and below.
    Large,
}

impl ButtonSize {
    /// Rows this size occupies — 1 for `Small`, 3 for `Large`.
    #[must_use]
    pub const fn height(self) -> u16 {
        match self {
            Self::Small => 1,
            Self::Large => 3,
        }
    }
}

/// The visual weight of a button, in the shadcn sense.
///
/// A variant is a shorthand for a set of theme colors, not a behavior: all
/// variants are pressed the same way. Picking one is about how much the button
/// should pull the eye and whether its action is dangerous.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ButtonVariant {
    /// Filled with the theme's primary color. The main action on a screen.
    #[default]
    Default,
    /// Border only, no fill. A secondary action that should stay quiet.
    ///
    /// Needs [`ButtonSize::Large`] to be usable: a `Small` button is a single
    /// row with no space for a border, so `Outline` draws nothing that changes
    /// with focus or hover and the button becomes indistinguishable from its
    /// resting state. Use [`Ghost`](Self::Ghost) for a quiet `Small` button —
    /// it fills on focus and hover instead of drawing an edge.
    Outline,
    /// Filled with the muted secondary color. Less emphasis than `Default`.
    Secondary,
    /// No fill and no border until focused or hovered. The quietest option, and
    /// the one to reach for at [`ButtonSize::Small`], where
    /// [`Outline`](Self::Outline) has no edge to draw.
    Ghost,
    /// Filled with the theme's destructive color. Deleting, discarding.
    Destructive,
}

/// Every color a button can paint, for each interaction state.
///
/// Normally derived for you: [`from_theme`](Self::from_theme) turns a
/// [`Theme`] and a [`ButtonVariant`] into one of these. Build or modify one
/// directly only when a button needs colors the variants do not offer, and pass
/// it through [`Button::style`] or [`ButtonWidget::style`].
///
/// The four sets — base, focused, hovered, disabled — are pre-computed rather
/// than blended at paint time, so a custom style controls every state
/// explicitly instead of inheriting a surprise. Disabled style wins first,
/// followed by hovered, focused, and finally the base style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonStyle {
    /// Whether the button paints as a fill or as a border.
    pub mode: ButtonRenderMode,
    /// Label color at rest.
    pub foreground: Color,
    /// Fill color at rest. In [`Bordered`](ButtonRenderMode::Bordered) mode this
    /// is usually `Color::Reset` so the surface behind shows through.
    pub background: Color,
    /// Border color at rest. Only painted in `Bordered` mode.
    pub border: Color,
    /// Label color while focused.
    pub focused_foreground: Color,
    /// Fill color while focused.
    pub focused_background: Color,
    /// Border color while focused.
    pub focused_border: Color,
    /// Label color while the pointer is over it.
    pub hovered_foreground: Color,
    /// Fill color while hovered.
    pub hovered_background: Color,
    /// Border color while hovered.
    pub hovered_border: Color,
    /// Label color while disabled.
    pub disabled_foreground: Color,
    /// Fill color while disabled.
    pub disabled_background: Color,
    /// Border color while disabled.
    pub disabled_border: Color,
}

/// Whether a button is drawn as a solid block of color or as an outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ButtonRenderMode {
    /// Paint the background across the button; no border is drawn. A `Large`
    /// filled button caps the fill with a row above and below the label.
    #[default]
    Filled,
    /// Draw a border and leave the interior alone.
    Bordered,
}

impl ButtonStyle {
    /// A neutral style using plain ANSI colors, for painting without a
    /// [`Theme`].
    ///
    /// Deliberately conservative: it renders on any terminal, including ones
    /// without truecolor. Prefer [`from_theme`](Self::from_theme) whenever a
    /// theme is available.
    #[must_use]
    pub const fn fallback() -> Self {
        Self {
            mode: ButtonRenderMode::Bordered,
            foreground: Color::Gray,
            background: Color::Reset,
            border: Color::Gray,
            focused_foreground: Color::Black,
            focused_background: Color::Cyan,
            focused_border: Color::Cyan,
            hovered_foreground: Color::Black,
            hovered_background: Color::LightCyan,
            hovered_border: Color::LightCyan,
            disabled_foreground: Color::DarkGray,
            disabled_background: Color::Reset,
            disabled_border: Color::DarkGray,
        }
    }

    /// Derive the full style for `variant` from `theme`.
    ///
    /// Focus and hover colors are computed by shifting the base fill rather than
    /// being separate theme entries, so a custom theme only has to supply the
    /// base colors and every variant stays consistent with it.
    ///
    /// This is what [`Button`] and [`ButtonWidget`] call for you. Call it
    /// directly when you want a variant's colors as a starting point to tweak.
    #[must_use]
    pub const fn from_theme(theme: &Theme, variant: ButtonVariant) -> Self {
        // Focus shifts a fill by a fixed amount, in the direction that makes it
        // stand out: a bright fill (primary, destructive) darkens; a dark fill
        // (secondary, ghost) lightens.
        match variant {
            ButtonVariant::Default => Self::filled(
                theme.primary,
                theme.primary_foreground,
                darken(theme.primary, FOCUS_DARKEN),
                darken(theme.primary, HOVER_DARKEN),
                theme,
            ),
            ButtonVariant::Secondary => Self::filled(
                theme.secondary,
                theme.secondary_foreground,
                lighten(theme.secondary, FOCUS_LIGHTEN),
                lighten(theme.secondary, HOVER_LIGHTEN),
                theme,
            ),
            ButtonVariant::Destructive => Self::filled(
                theme.destructive,
                theme.destructive_foreground,
                darken(theme.destructive, FOCUS_DARKEN),
                darken(theme.destructive, HOVER_DARKEN),
                theme,
            ),
            ButtonVariant::Outline => Self {
                mode: ButtonRenderMode::Bordered,
                foreground: theme.foreground,
                background: theme.background,
                border: theme.border,
                focused_foreground: theme.foreground,
                focused_background: theme.background,
                focused_border: theme.primary,
                hovered_foreground: theme.foreground,
                hovered_background: theme.background,
                hovered_border: darken(theme.primary, HOVER_DARKEN),
                disabled_foreground: theme.muted_foreground,
                disabled_background: theme.background,
                disabled_border: theme.border,
            },
            ButtonVariant::Ghost => Self {
                mode: ButtonRenderMode::Filled,
                foreground: theme.foreground,
                background: theme.background,
                focused_foreground: theme.foreground,
                focused_background: lighten(theme.secondary, FOCUS_LIGHTEN),
                border: theme.background,
                focused_border: lighten(theme.secondary, FOCUS_LIGHTEN),
                hovered_foreground: theme.foreground,
                hovered_background: lighten(theme.secondary, HOVER_LIGHTEN),
                hovered_border: lighten(theme.secondary, HOVER_LIGHTEN),
                disabled_foreground: theme.muted_foreground,
                disabled_background: theme.background,
                disabled_border: theme.background,
            },
        }
    }

    const fn filled(
        background: Color,
        foreground: Color,
        focused_background: Color,
        hovered_background: Color,
        theme: &Theme,
    ) -> Self {
        // Disabled keeps the variant's hue, dimmed toward the surface, so a
        // disabled destructive button still reads as destructive.
        let disabled_background = dim(background, theme.surface, DISABLED_DIM);
        Self {
            mode: ButtonRenderMode::Filled,
            foreground,
            background,
            border: background,
            focused_foreground: foreground,
            focused_background,
            focused_border: focused_background,
            hovered_foreground: foreground,
            hovered_background,
            hovered_border: hovered_background,
            disabled_foreground: theme.muted_foreground,
            disabled_background,
            disabled_border: disabled_background,
        }
    }

    /// The colors and emphasis for one paint pass — the single place
    /// interaction state is turned into style. Precedence: disabled, then
    /// hovered, then focused, then base; hover wins visually when a focused
    /// button is also hovered.
    const fn resolve(&self, focused: bool, hovered: bool, disabled: bool) -> ResolvedButtonStyle {
        if disabled {
            ResolvedButtonStyle {
                foreground: self.disabled_foreground,
                background: self.disabled_background,
                border: self.disabled_border,
            }
        } else if hovered {
            ResolvedButtonStyle {
                foreground: self.hovered_foreground,
                background: self.hovered_background,
                border: self.hovered_border,
            }
        } else if focused {
            ResolvedButtonStyle {
                foreground: self.focused_foreground,
                background: self.focused_background,
                border: self.focused_border,
            }
        } else {
            ResolvedButtonStyle {
                foreground: self.foreground,
                background: self.background,
                border: self.border,
            }
        }
    }
}

/// One paint pass's resolved colors (see `ButtonStyle::resolve`). For a filled
/// button `background` is also the cap fill; `border` only paints in bordered
/// mode.
struct ResolvedButtonStyle {
    foreground: Color,
    background: Color,
    border: Color,
}

/// A button that only draws — an ordinary ratatui [`Widget`] with no focus,
/// events, or state.
///
/// **Usable in any ratatui app.** Nothing here depends on
/// [`Ratcn`](ratcn::runtime::Ratcn) or the component layer: hand it to
/// `frame.render_widget(...)` alongside your own widgets and keep managing focus
/// and events however you already do. Take the look without the runtime.
///
/// You tell it what to paint, including whether to draw as focused or hovered,
/// and it paints. Nothing is tracked between frames.
///
/// Use [`Button`] instead when you want a button that joins ratcn's focus
/// traversal and emits a message when pressed; it paints through this widget
/// internally, so both look identical.
///
/// ```
/// use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};
/// use ratcn::{ButtonWidget, Theme};
///
/// let area = Rect::new(0, 0, 12, 1);
/// let mut buffer = Buffer::empty(area);
/// Widget::render(
///     ButtonWidget::new("Save")
///         .themed(&Theme::default_dark())
///         .focused(true),
///     area,
///     &mut buffer,
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonWidget<'a> {
    label: &'a str,
    focused: bool,
    hovered: bool,
    variant: ButtonVariant,
    style: ButtonStyle,
    theme: Option<Theme>,
    disabled: bool,
    size: ButtonSize,
}

impl<'a> ButtonWidget<'a> {
    /// A small, default-variant button labelled `label`, using
    /// [`ButtonStyle::fallback`] until [`themed`](Self::themed) or
    /// [`style`](Self::style) says otherwise.
    #[must_use]
    pub const fn new(label: &'a str) -> Self {
        Self {
            label,
            focused: false,
            hovered: false,
            variant: ButtonVariant::Default,
            style: ButtonStyle::fallback(),
            theme: None,
            disabled: false,
            size: ButtonSize::Small,
        }
    }

    /// Take colors from `theme`, combined with the current variant.
    ///
    /// Remembers the theme, so changing the variant afterwards re-derives from
    /// it rather than falling back. Storing the theme for that later
    /// [`variant`](Self::variant) re-derivation is also why this `themed` is
    /// not `const`, unlike the other paint widgets' — the documented exception.
    #[must_use]
    pub fn themed(mut self, theme: &Theme) -> Self {
        self.theme = Some(*theme);
        self.style = ButtonStyle::from_theme(theme, self.variant);
        self
    }

    /// Shorthand for [`ButtonVariant::Outline`].
    #[must_use]
    pub fn outline(self) -> Self {
        self.variant(ButtonVariant::Outline)
    }

    /// Shorthand for [`ButtonVariant::Secondary`].
    #[must_use]
    pub fn secondary(self) -> Self {
        self.variant(ButtonVariant::Secondary)
    }

    /// Shorthand for [`ButtonVariant::Ghost`].
    #[must_use]
    pub fn ghost(self) -> Self {
        self.variant(ButtonVariant::Ghost)
    }

    /// Shorthand for [`ButtonVariant::Destructive`].
    #[must_use]
    pub fn destructive(self) -> Self {
        self.variant(ButtonVariant::Destructive)
    }

    /// Set the variant. Re-derives colors from the theme if one was given.
    #[must_use]
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self.resolve_themed_style();
        self
    }

    /// Use these exact colors, ignoring theme and variant.
    ///
    /// This drops any theme set with [`themed`](Self::themed), so a later
    /// [`variant`](Self::variant) call cannot quietly overwrite your colors.
    #[must_use]
    pub const fn style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self.theme = None;
        self
    }

    /// Paint with the focused colors. The widget has no idea what is actually
    /// focused — pass the answer in.
    #[must_use]
    pub const fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Render the button as hovered (pointer over it). Hover is distinct from
    /// focus and wins visually when both are true.
    #[must_use]
    pub const fn hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }

    /// Paint with the disabled colors. Purely visual here — a paint widget
    /// receives no events to suppress.
    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set the height, one row or three. See [`ButtonSize`].
    ///
    /// A large button needs all three rows to paint. When the supplied area is
    /// taller, only the first three rows are painted. Any nonzero width remains
    /// usable.
    #[must_use]
    pub const fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    /// Rows this button needs. Give it less and it paints nothing rather than a
    /// broken partial button.
    #[must_use]
    pub const fn height(&self) -> u16 {
        self.size.height()
    }

    /// Columns this button needs: the label in terminal cells, plus two cells
    /// of padding on each side.
    ///
    /// Use it to build layout constraints from the same instance you are about
    /// to render, so the constraint and the paint cannot disagree.
    #[must_use]
    pub fn width(&self) -> u16 {
        button_width(self.label)
    }

    fn resolve_themed_style(&mut self) {
        if let Some(theme) = self.theme {
            self.style = ButtonStyle::from_theme(&theme, self.variant);
        }
    }
}

impl Widget for ButtonWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // A button is a fixed three-row shape. Rendering into a shorter area
        // produces broken caps/content, so skip instead of drawing a partial
        // button. Flexible widgets can still degrade within smaller areas.
        if area.width == 0 || area.height < self.height() {
            return;
        }
        let area = Rect {
            height: self.height(),
            ..area
        };

        match self.style.mode {
            ButtonRenderMode::Filled => self.render_filled(area, buf),
            ButtonRenderMode::Bordered => self.render_bordered(area, buf),
        }
    }
}

impl ButtonWidget<'_> {
    fn render_filled(self, area: Rect, buf: &mut Buffer) {
        let resolved = self
            .style
            .resolve(self.focused, self.hovered, self.disabled);
        let width = area.width as usize;

        if self.size == ButtonSize::Large {
            Line::from(cap_row(resolved.background, TOP_CAP, width))
                .style(Style::default().fg(resolved.background))
                .render(Rect::new(area.x, area.y, area.width, 1), buf);

            Line::from(cap_row(resolved.background, BOTTOM_CAP, width))
                .style(Style::default().fg(resolved.background))
                .render(Rect::new(area.x, area.y + 2, area.width, 1), buf);
        }

        let content_style = Style::default()
            .fg(resolved.foreground)
            .bg(resolved.background);
        Line::from(filled_middle(self.label, width))
            .style(content_style)
            .render(
                Rect::new(area.x, area.y + self.content_y_offset(), area.width, 1),
                buf,
            );
    }

    fn render_bordered(self, area: Rect, buf: &mut Buffer) {
        let resolved = self
            .style
            .resolve(self.focused, self.hovered, self.disabled);
        let content_style = Style::default()
            .fg(resolved.foreground)
            .bg(resolved.background);

        if self.size == ButtonSize::Large {
            let block = Block::bordered().border_style(Style::default().fg(resolved.border));
            let inner = block.inner(area);
            block.render(area, buf);

            Line::from(self.label)
                .alignment(Alignment::Center)
                .style(content_style)
                .render(inner, buf);
            return;
        }

        Line::from(filled_middle(self.label, area.width as usize))
            .alignment(Alignment::Center)
            .style(content_style)
            .render(area, buf);
    }

    const fn content_y_offset(&self) -> u16 {
        match self.size {
            ButtonSize::Small => 0,
            ButtonSize::Large => 1,
        }
    }
}

fn button_width(label: &str) -> u16 {
    shape_width(label)
}

/// Resolves the button's style from the active theme (the style override).
type StyleFn = Box<dyn Fn(&Theme) -> ButtonStyle>;
/// Builds the message emitted when the button is pressed.
type OnPressFn<M> = Box<dyn Fn() -> M>;

/// A button that can be focused and pressed, declared with
/// [`render_component`](ratcn::runtime::RenderCtx::render_component).
///
/// After [`on_press`](Self::on_press) is set, pressing it — Enter, Space, or a
/// left click — returns the message built by that handler. Without a handler the
/// button is not focusable and ignores activation keys and clicks; use
/// [`ButtonWidget`] when only painting is needed. Right and middle clicks do
/// nothing. Painting is delegated to `ButtonWidget`; this half adds focus and
/// event handling.
///
/// A button holds no state of its own. Its label and disabledness are values you
/// pass at declaration, which means they come from app state and there is
/// nothing to keep in sync. Those declared values also stay in effect for event
/// handling until the next successful render, so a click arriving between an
/// update and a redraw is judged against the button the user actually saw.
///
/// ```
/// use ratcn::Button;
///
/// # struct AppState { can_delete: bool }
/// # enum Msg { Delete }
/// # let state = AppState { can_delete: true };
/// let _button = Button::new("Delete")
///     .destructive()
///     .disabled(!state.can_delete)
///     .on_press(|| Msg::Delete);
/// ```
pub struct Button<M> {
    label: String,
    variant: ButtonVariant,
    size: ButtonSize,
    style: Option<StyleFn>,
    on_press: Option<OnPressFn<M>>,
    disabled: bool,
}

impl<M> fmt::Debug for Button<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Button")
            .field("label", &self.label)
            .field("variant", &self.variant)
            .field("size", &self.size)
            .field("style", &self.style.is_some())
            .field("on_press", &self.on_press.is_some())
            .field("disabled", &self.disabled)
            .finish_non_exhaustive()
    }
}

impl<M> Button<M> {
    /// A default-variant button labelled `label`.
    ///
    /// It does nothing until [`on_press`](Self::on_press) gives it a message to
    /// emit. Without one it is not focusable and ignores activation keys and
    /// clicks. Use [`ButtonWidget`] instead for paint-only presentation.
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            variant: ButtonVariant::Default,
            size: ButtonSize::Small,
            style: None,
            on_press: None,
            disabled: false,
        }
    }

    /// What to emit when the button is pressed: Enter, Space, or a left click.
    ///
    /// The argument is a factory rather than a value because the message is
    /// built at press time. Pressing is payload-free, so it takes no arguments;
    /// events that carry data pass it to the factory instead, which is why
    /// `Msg::SomethingChanged` can often be named directly.
    #[must_use]
    pub fn on_press(mut self, on_press: impl Fn() -> M + 'static) -> Self {
        self.on_press = Some(Box::new(on_press));
        self
    }

    /// Grey the button out and stop it responding.
    ///
    /// A disabled button is not focusable, so Tab skips it, and it ignores
    /// events rather than consuming them. An ignored click can bubble to an
    /// ancestor, but does not pass through to an overlapping sibling behind the
    /// button because hit-testing chooses one target path. Focus already parked
    /// on it stays there rather than jumping somewhere else; the runtime never
    /// silently retargets.
    ///
    /// Pass the value from app state (`.disabled(!state.can_save)`). The value
    /// declared this frame is the one events are judged against until the next
    /// successful render.
    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set the visual variant. See [`ButtonVariant`].
    #[must_use]
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Shorthand for [`ButtonVariant::Outline`].
    #[must_use]
    pub fn outline(self) -> Self {
        self.variant(ButtonVariant::Outline)
    }

    /// Shorthand for [`ButtonVariant::Secondary`].
    #[must_use]
    pub fn secondary(self) -> Self {
        self.variant(ButtonVariant::Secondary)
    }

    /// Shorthand for [`ButtonVariant::Ghost`].
    #[must_use]
    pub fn ghost(self) -> Self {
        self.variant(ButtonVariant::Ghost)
    }

    /// Shorthand for [`ButtonVariant::Destructive`].
    #[must_use]
    pub fn destructive(self) -> Self {
        self.variant(ButtonVariant::Destructive)
    }

    /// Set the height, one row or three. See [`ButtonSize`].
    #[must_use]
    pub const fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    /// Override the button's [`ButtonStyle`], instead of the one the theme and
    /// [`variant`](Button::variant) derive. Resolved from the active theme at
    /// render time, so a style built from `theme` follows theme switches; a
    /// fixed style ignores the argument (`|_| STYLE`).
    ///
    /// ```
    /// use ratcn::{Button, ButtonStyle, ButtonVariant};
    ///
    /// # enum Msg { Archive }
    /// let _button = Button::new("Archive").on_press(|| Msg::Archive).style(|theme| {
    ///     let mut style = ButtonStyle::from_theme(theme, ButtonVariant::Default);
    ///     style.background = theme.accent;
    ///     style.border = theme.accent;
    ///     style
    /// });
    /// ```
    #[must_use]
    pub fn style(mut self, style: impl Fn(&Theme) -> ButtonStyle + 'static) -> Self {
        self.style = Some(Box::new(style));
        self
    }

    /// Columns this button needs: the label in terminal cells, plus two cells
    /// of padding on each side.
    ///
    /// Build layout constraints from the same instance you are about to
    /// declare, so the constraint and the paint cannot disagree.
    #[must_use]
    pub fn width(&self) -> u16 {
        button_width(&self.label)
    }

    /// Rows this button needs, per its [`ButtonSize`].
    #[must_use]
    pub const fn height(&self) -> u16 {
        self.size.height()
    }
}

impl<S, M> Component<S, M> for Button<M> {
    fn render(&mut self, _ctx: &mut RenderCtx<'_, S, M>) {}

    fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, S>) {
        let area = ctx.area();
        let style = resolve_style(self.style.as_deref(), ctx.theme, |theme| {
            ButtonStyle::from_theme(theme, self.variant)
        });
        let widget = ButtonWidget::new(&self.label)
            .style(style)
            .size(self.size)
            .focused(ctx.focused)
            .hovered(ctx.hovered)
            .disabled(self.disabled);
        ctx.render_widget(widget, area);
    }

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &S,
        _ctx: &mut EventCtx<'_>,
    ) -> EventResult<M> {
        if self.disabled {
            return EventResult::Ignored;
        }
        let Some(on_press) = &self.on_press else {
            return EventResult::Ignored;
        };

        // A press is Enter/Space (unmodified) or a primary mouse Click. Focus on
        // Down is the runtime's job, so the button ignores Down/Up itself.
        let pressed = match event {
            Event::Key(key) if !key.modifiers.any() => {
                matches!(key.code, KeyCode::Enter | KeyCode::Char(' '))
            }
            Event::Mouse(mouse) => {
                matches!(mouse.kind, MouseKind::Click(MouseButton::Left))
            }
            _ => false,
        };
        if !pressed {
            return EventResult::Ignored;
        }
        EventResult::Emit(on_press())
    }

    fn is_focusable(&self, _state: &S) -> bool {
        !self.disabled && self.on_press.is_some()
    }

    fn interaction_area(&self, area: Rect) -> Rect {
        fixed_height(area, self.height())
    }
}

impl<S, M> MeasuredComponent<S, M> for Button<M> {
    fn measure(&self) -> Size {
        Size::new(self.width(), self.height())
    }
}
