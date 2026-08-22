use std::fmt;

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect, Size},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Widget},
};

use crate::Theme;
use crate::button_shape::{BOTTOM_CAP, TOP_CAP, cap_row, filled_middle, shape_width};
use crate::color::{DISABLED_DIM, FOCUS_SHIFT, HOVER_SHIFT, away_from, dim, nearest_to};
use crate::geometry::fixed_height;
use crate::runtime::{
    Component, DeclareCtx, Event, EventCtx, EventResult, KeyCode, MeasuredComponent, MouseButton,
    MouseKind, PaintCtx, ScopeOptions,
};
use crate::theme::resolve_style;

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
    /// Needs [`ButtonSize::Large`]: a `Small` button is a single row with no
    /// space for a border, so at that size `Outline` paints the same cells
    /// focused, hovered, and at rest. Use [`Ghost`](Self::Ghost) for a quiet
    /// `Small` button — it fills on focus and hover.
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
/// The four sets — base, focused, hovered, disabled — are computed up front, so
/// a custom style names the colors for every state. Disabled style wins first,
/// followed by hovered, focused, and finally the base style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonStyle {
    /// Whether the button paints as a fill or as a border.
    pub mode: ButtonFill,
    /// Label color at rest.
    pub foreground: Color,
    /// Fill color at rest. In [`Bordered`](ButtonFill::Bordered) mode this
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
pub enum ButtonFill {
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
    /// Plain ANSI colors paint on any terminal, including ones without
    /// truecolor. Use [`from_theme`](Self::from_theme) whenever a theme is
    /// available.
    #[must_use]
    pub const fn fallback() -> Self {
        Self {
            mode: ButtonFill::Bordered,
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
    pub fn from_theme(theme: &Theme, variant: ButtonVariant) -> Self {
        // Focus shifts a fill by a fixed amount, in the direction the theme's
        // own polarity gives it. A loud fill (primary, destructive) deepens
        // toward the end the screen sits at, so it reads as pressed; a quiet
        // one (secondary, ghost) climbs away from the screen, so it reads as
        // raised. On a light terminal both are the other way round in absolute
        // terms, which is why the direction comes from the background.
        let pressed = nearest_to(theme.background);
        let raised = away_from(theme.background);
        match variant {
            ButtonVariant::Default => Self::filled(
                theme.primary,
                theme.primary_foreground,
                dim(theme.primary, pressed, FOCUS_SHIFT),
                dim(theme.primary, pressed, HOVER_SHIFT),
                theme,
            ),
            ButtonVariant::Secondary => Self::filled(
                theme.secondary,
                theme.secondary_foreground,
                dim(theme.secondary, raised, FOCUS_SHIFT),
                dim(theme.secondary, raised, HOVER_SHIFT),
                theme,
            ),
            ButtonVariant::Destructive => Self::filled(
                theme.destructive,
                theme.destructive_foreground,
                dim(theme.destructive, pressed, FOCUS_SHIFT),
                dim(theme.destructive, pressed, HOVER_SHIFT),
                theme,
            ),
            ButtonVariant::Outline => Self {
                mode: ButtonFill::Bordered,
                foreground: theme.foreground,
                background: theme.background,
                border: theme.border,
                focused_foreground: theme.foreground,
                focused_background: theme.background,
                focused_border: theme.primary,
                hovered_foreground: theme.foreground,
                hovered_background: theme.background,
                hovered_border: dim(theme.primary, pressed, HOVER_SHIFT),
                disabled_foreground: theme.muted_foreground,
                disabled_background: theme.background,
                disabled_border: theme.border,
            },
            ButtonVariant::Ghost => Self {
                mode: ButtonFill::Filled,
                foreground: theme.foreground,
                background: theme.background,
                focused_foreground: theme.foreground,
                focused_background: dim(theme.secondary, raised, FOCUS_SHIFT),
                border: theme.background,
                focused_border: dim(theme.secondary, raised, FOCUS_SHIFT),
                hovered_foreground: theme.foreground,
                hovered_background: dim(theme.secondary, raised, HOVER_SHIFT),
                hovered_border: dim(theme.secondary, raised, HOVER_SHIFT),
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
            mode: ButtonFill::Filled,
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
/// [`Ratcn`](crate::runtime::Ratcn) or the component layer: hand it to
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
            ButtonFill::Filled => self.paint_filled(area, buf),
            ButtonFill::Bordered => self.paint_bordered(area, buf),
        }
    }
}

impl ButtonWidget<'_> {
    fn paint_filled(self, area: Rect, buf: &mut Buffer) {
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

    fn paint_bordered(self, area: Rect, buf: &mut Buffer) {
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
/// [`component`](crate::runtime::DeclareCtx::component).
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

    /// Replace the [`ButtonStyle`] the theme and [`variant`](Button::variant)
    /// derive. Resolved from the active theme at render time, so a style built
    /// from `theme` follows theme switches; a fixed style ignores the argument
    /// (`|_| STYLE`).
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
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, S, M>) {}

    fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, S>) {
        let area = ctx.area();
        let style = resolve_style(self.style.as_deref(), ctx.theme, |theme| {
            ButtonStyle::from_theme(theme, self.variant)
        });
        let widget = ButtonWidget::new(&self.label)
            .style(style)
            .size(self.size)
            .focused(ctx.focused())
            .hovered(ctx.hovered())
            .disabled(self.disabled);
        ctx.widget(widget, area);
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

    fn scope_options(&self) -> ScopeOptions {
        ScopeOptions::default().focusable(!self.disabled && self.on_press.is_some())
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

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use super::*;
    use crate::runtime::{ChildId, FocusState, Modifiers, MouseEvent, Ratcn};
    use crate::test_support::Driver;

    #[test]
    fn message_factory_runs_for_each_accepted_press_with_non_clone_message() {
        #[derive(Debug, PartialEq, Eq)]
        struct Msg(usize, String);

        let calls = Rc::new(Cell::new(0));
        let factory_calls = Rc::clone(&calls);
        let mut component = Button::new("Press").on_press(move || {
            let activation = factory_calls.get() + 1;
            factory_calls.set(activation);
            Msg(activation, "pressed".to_string())
        });
        let click = |button| {
            Event::Mouse(MouseEvent {
                kind: MouseKind::Click(button),
                column: 0,
                row: 0,
                modifiers: Modifiers::NONE,
            })
        };

        for button in [MouseButton::Right, MouseButton::Middle] {
            assert_eq!(
                component.handle_event(&click(button), &(), &mut EventCtx::default()),
                EventResult::Ignored
            );
        }
        assert_eq!(
            component.handle_event(&click(MouseButton::Left), &(), &mut EventCtx::default()),
            EventResult::Emit(Msg(1, "pressed".to_string()))
        );
        assert_eq!(
            component.handle_event(
                &Event::Key(crate::runtime::KeyEvent::new(KeyCode::Enter)),
                &(),
                &mut EventCtx::default()
            ),
            EventResult::Emit(Msg(2, "pressed".to_string()))
        );
        assert_eq!(
            component.handle_event(
                &Event::Key(crate::runtime::KeyEvent::new(KeyCode::Char(' '))),
                &(),
                &mut EventCtx::default()
            ),
            EventResult::Emit(Msg(3, "pressed".to_string()))
        );
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn handlerless_button_is_not_focusable_and_ignores_activation() {
        let mut button = Button::<()>::new("Presentational");
        let click = Event::Mouse(MouseEvent {
            kind: MouseKind::Click(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: Modifiers::NONE,
        });

        assert!(!Component::<(), ()>::scope_options(&button).focusable);
        for event in [
            Event::Key(crate::runtime::KeyEvent::new(KeyCode::Enter)),
            Event::Key(crate::runtime::KeyEvent::new(KeyCode::Char(' '))),
            click,
        ] {
            assert_eq!(
                button.handle_event(&event, &(), &mut EventCtx::default()),
                EventResult::Ignored
            );
        }
    }

    #[test]
    fn button_variant_defaults_to_default() {
        assert_eq!(ButtonVariant::default(), ButtonVariant::Default);
    }

    #[test]
    fn disabled_widget_uses_disabled_style() {
        let style = ButtonStyle {
            mode: ButtonFill::Bordered,
            foreground: Color::White,
            background: Color::Black,
            border: Color::White,
            focused_foreground: Color::Black,
            focused_background: Color::Cyan,
            focused_border: Color::Cyan,
            hovered_foreground: Color::Black,
            hovered_background: Color::LightCyan,
            hovered_border: Color::LightCyan,
            disabled_foreground: Color::Yellow,
            disabled_background: Color::Blue,
            disabled_border: Color::Red,
        };
        let area = Rect::new(0, 0, 6, ButtonSize::Large.height());
        let mut buffer = Buffer::empty(area);

        ButtonWidget::new("OK")
            .style(style)
            .size(ButtonSize::Large)
            .focused(true)
            .disabled(true)
            .render(area, &mut buffer);

        let content = buffer.cell((2, 1)).expect("button content cell");
        let border = buffer.cell((0, 0)).expect("button border cell");

        assert_eq!(content.fg, Color::Yellow);
        assert_eq!(content.bg, Color::Blue);
        assert_eq!(border.fg, Color::Red);
    }

    #[test]
    fn explicit_style_is_not_overwritten_by_variant_without_theme() {
        let style = ButtonStyle {
            mode: ButtonFill::Bordered,
            foreground: Color::Yellow,
            background: Color::Blue,
            border: Color::Red,
            focused_foreground: Color::Black,
            focused_background: Color::Cyan,
            focused_border: Color::Green,
            hovered_foreground: Color::Black,
            hovered_background: Color::LightCyan,
            hovered_border: Color::LightGreen,
            disabled_foreground: Color::DarkGray,
            disabled_background: Color::Reset,
            disabled_border: Color::DarkGray,
        };
        let area = Rect::new(0, 0, 6, ButtonSize::Large.height());
        let mut buffer = Buffer::empty(area);

        ButtonWidget::new("OK")
            .themed(&Theme::default())
            .style(style)
            .size(ButtonSize::Large)
            .outline()
            .render(area, &mut buffer);

        let content = buffer.cell((2, 1)).expect("button content cell");
        let border = buffer.cell((0, 0)).expect("button border cell");

        assert_eq!(content.fg, Color::Yellow);
        assert_eq!(content.bg, Color::Blue);
        assert_eq!(border.fg, Color::Red);
    }

    #[test]
    fn hover_wins_over_focus_so_focused_buttons_still_react_to_pointer() {
        let style = ButtonStyle {
            mode: ButtonFill::Filled,
            foreground: Color::White,
            background: Color::Blue,
            border: Color::Blue,
            focused_foreground: Color::White,
            focused_background: Color::Cyan,
            focused_border: Color::Cyan,
            hovered_foreground: Color::White,
            hovered_background: Color::Green,
            hovered_border: Color::Green,
            disabled_foreground: Color::DarkGray,
            disabled_background: Color::Reset,
            disabled_border: Color::DarkGray,
        };
        let area = Rect::new(0, 0, 6, ButtonSize::Small.height());
        let mut buffer = Buffer::empty(area);

        ButtonWidget::new("OK")
            .style(style)
            .focused(true)
            .hovered(true)
            .render(area, &mut buffer);

        let content = buffer.cell((2, 0)).expect("button content cell");

        assert_eq!(content.bg, Color::Green);
    }

    // Emphasis is carried entirely by color now that the bold modifier is gone,
    // so every variant we recommend must change something visible on focus and
    // on hover. `Outline` is excluded on purpose: it signals through its border,
    // which `Small` has no room to draw, and its docs point to `Ghost` instead.
    #[test]
    fn recommended_variants_change_color_on_focus_and_hover() {
        let theme = Theme::default_dark();
        for variant in [
            ButtonVariant::Default,
            ButtonVariant::Secondary,
            ButtonVariant::Ghost,
            ButtonVariant::Destructive,
        ] {
            let style = ButtonStyle::from_theme(&theme, variant);
            let resting = style.resolve(false, false, false);
            let focused = style.resolve(true, false, false);
            let hovered = style.resolve(false, true, false);

            assert!(
                (focused.foreground, focused.background)
                    != (resting.foreground, resting.background),
                "{variant:?} is indistinguishable when focused"
            );
            assert!(
                (hovered.foreground, hovered.background)
                    != (resting.foreground, resting.background),
                "{variant:?} is indistinguishable when hovered"
            );
        }
    }

    #[test]
    fn themed_maps_current_variant_to_style() {
        let theme = Theme::default();
        let area = Rect::new(0, 0, 6, ButtonSize::Large.height());
        let mut buffer = Buffer::empty(area);

        ButtonWidget::new("OK")
            .outline()
            .size(ButtonSize::Large)
            .themed(&theme)
            .render(area, &mut buffer);

        let border = buffer.cell((0, 0)).expect("button border cell");

        assert_eq!(border.fg, theme.border);
    }

    #[test]
    fn themed_then_variant_uses_the_later_variant_style() {
        let theme = Theme::default_dark();
        let area = Rect::new(0, 0, 6, ButtonSize::Small.height());
        let mut buffer = Buffer::empty(area);

        ButtonWidget::new("OK")
            .themed(&theme)
            .secondary()
            .render(area, &mut buffer);

        assert_eq!(
            buffer.cell((2, 0)).expect("button content cell").bg,
            theme.secondary
        );
    }

    #[test]
    fn widget_does_not_render_partial_height() {
        let area = Rect::new(0, 0, 6, ButtonSize::Large.height() - 1);
        let mut buffer = Buffer::empty(area);

        ButtonWidget::new("OK")
            .size(ButtonSize::Large)
            .render(area, &mut buffer);

        let cell = buffer.cell((0, 0)).expect("button area cell");
        assert_eq!(cell.symbol(), " ");
        assert_eq!(cell.fg, Color::Reset);
        assert_eq!(cell.bg, Color::Reset);
    }

    #[test]
    fn widget_modes_do_not_paint_excess_height() {
        let area = Rect::new(0, 0, 6, 5);

        for mode in [ButtonFill::Filled, ButtonFill::Bordered] {
            let mut style = ButtonStyle::fallback();
            style.mode = mode;
            style.background = Color::Blue;
            let mut buffer = Buffer::empty(area);

            ButtonWidget::new("OK")
                .style(style)
                .size(ButtonSize::Large)
                .render(area, &mut buffer);

            assert_ne!(
                buffer.cell((0, 0)).expect("button pixel").symbol(),
                " ",
                "{mode:?} mode must paint within the fixed-height button"
            );
            for y in ButtonSize::Large.height()..area.height {
                for x in 0..area.width {
                    let cell = buffer.cell((x, y)).expect("excess-area cell");
                    assert_eq!(cell.symbol(), " ", "{mode:?} painted ({x}, {y})");
                    assert_eq!(cell.fg, Color::Reset, "{mode:?} styled ({x}, {y})");
                    assert_eq!(cell.bg, Color::Reset, "{mode:?} filled ({x}, {y})");
                }
            }
        }
    }

    #[test]
    fn width_counts_label_cells_not_chars_or_bytes() {
        assert_eq!(ButtonWidget::new("OK").width(), 6);
        assert_eq!(ButtonWidget::new("åβ").width(), 6);
        assert_eq!(Button::<()>::new("åβ").width(), 6);
        assert_eq!(
            ButtonWidget::new("日本").width(),
            8,
            "CJK chars are 2 cells"
        );
        assert_eq!(ButtonWidget::new("🚀").width(), 6, "emoji are 2 cells");
        assert_eq!(
            ButtonWidget::new("e\u{301}").width(),
            5,
            "combining mark adds no cell"
        );
        assert_eq!(
            ButtonWidget::new("👩\u{200d}👩\u{200d}👦").width(),
            6,
            "a ZWJ sequence is one 2-cell glyph, not its char count"
        );
    }

    #[test]
    fn filled_middle_row_centers_wide_label_by_cells() {
        let style = ButtonStyle {
            mode: ButtonFill::Filled,
            foreground: Color::White,
            background: Color::Blue,
            border: Color::Blue,
            focused_foreground: Color::White,
            focused_background: Color::Cyan,
            focused_border: Color::Cyan,
            hovered_foreground: Color::White,
            hovered_background: Color::LightCyan,
            hovered_border: Color::LightCyan,
            disabled_foreground: Color::DarkGray,
            disabled_background: Color::Reset,
            disabled_border: Color::DarkGray,
        };
        // "日本" is 4 cells wide, so at the natural width (label + 4) the
        // label starts 2 cells in and the fill runs to the last cell.
        let area = Rect::new(0, 0, ButtonWidget::new("日本").width(), 1);
        let mut buffer = Buffer::empty(area);

        ButtonWidget::new("日本")
            .style(style)
            .render(area, &mut buffer);

        assert_eq!(buffer.cell((2, 0)).expect("label start").symbol(), "日");
        assert_eq!(buffer.cell((4, 0)).expect("label middle").symbol(), "本");
        for x in [0, 1, 6, 7] {
            let pad = buffer.cell((x, 0)).expect("padding cell");
            assert_eq!(pad.symbol(), " ", "pad at {x}");
            assert_eq!(pad.bg, Color::Blue, "fill spans the button at {x}");
        }
    }

    #[test]
    fn filled_background_spans_the_row_even_just_under_natural_width() {
        // The padding string is what carries the fill. At width label + 2
        // (below the natural label + 4) the fill must still reach the last
        // cell instead of leaving a hole after the label.
        let mut style = ButtonStyle::fallback();
        style.mode = ButtonFill::Filled;
        style.background = Color::Blue;
        let area = Rect::new(0, 0, 4, 1);
        let mut buffer = Buffer::empty(area);

        ButtonWidget::new("OK")
            .style(style)
            .render(area, &mut buffer);

        for x in 0..4 {
            assert_eq!(
                buffer.cell((x, 0)).expect("row cell").bg,
                Color::Blue,
                "fill missing at {x}"
            );
        }
    }

    #[test]
    fn filled_middle_row_centers_combining_mark_label_by_cells() {
        // "Cafe\u{301}" is 5 chars but 4 cells: a char count would place the
        // label one cell left of center at the natural width of 8.
        let theme = Theme::default_dark();
        let style = ButtonStyle::from_theme(&theme, ButtonVariant::Default);
        let label = "Cafe\u{301}";
        let area = Rect::new(0, 0, ButtonWidget::new(label).width(), 1);
        assert_eq!(area.width, 8);
        let mut buffer = Buffer::empty(area);

        ButtonWidget::new(label)
            .style(style)
            .render(area, &mut buffer);

        assert_eq!(buffer.cell((2, 0)).expect("label start").symbol(), "C");
        assert_eq!(
            buffer.cell((5, 0)).expect("label end").symbol(),
            "e\u{301}",
            "the accent shares its base char's cell"
        );
        for x in [0, 1, 6, 7] {
            assert_eq!(
                buffer.cell((x, 0)).expect("padding cell").symbol(),
                " ",
                "pad at {x}"
            );
        }
    }

    #[test]
    fn undersized_area_never_paints_outside_it() {
        // The buffer is wider than the button's area; a wide label that
        // cannot fit must clip inside the area, not spill past it.
        let surround = Rect::new(0, 0, 10, 1);
        let area = Rect::new(2, 0, 3, 1);
        let mut buffer = Buffer::empty(surround);

        ButtonWidget::new("日本語").render(area, &mut buffer);

        for x in [0, 1, 5, 6, 7, 8, 9] {
            let outside = buffer.cell((x, 0)).expect("cell outside the area");
            assert_eq!(outside.symbol(), " ", "outside cell {x} written");
            assert_eq!(outside.bg, Color::Reset, "outside cell {x} styled");
        }

        // Zero-sized areas are a no-op, not a panic.
        let before = buffer.clone();
        ButtonWidget::new("日本語").render(Rect::new(2, 0, 0, 1), &mut buffer);
        ButtonWidget::new("日本語").render(Rect::new(2, 0, 3, 0), &mut buffer);
        assert_eq!(buffer, before);
    }

    #[test]
    fn interaction_area_requires_full_height_and_crops_excess_rows() {
        let small = Button::<()>::new("OK");
        let large = Button::<()>::new("OK").size(ButtonSize::Large);

        assert_eq!(
            Component::<(), ()>::interaction_area(&small, Rect::new(2, 3, 1, 4)),
            Rect::new(2, 3, 1, 1)
        );
        assert_eq!(
            Component::<(), ()>::interaction_area(&large, Rect::new(0, 0, 4, 2)),
            Rect::default()
        );
        assert_eq!(
            Component::<(), ()>::interaction_area(&large, Rect::new(0, 0, 0, 3)),
            Rect::default()
        );
        assert_eq!(
            Component::<(), ()>::interaction_area(&large, Rect::new(2, 3, 1, 5)),
            Rect::new(2, 3, 1, 3)
        );
    }

    #[test]
    fn filled_button_with_reset_background_uses_blank_caps() {
        let style = ButtonStyle {
            mode: ButtonFill::Filled,
            foreground: Color::White,
            background: Color::Reset,
            border: Color::Reset,
            focused_foreground: Color::White,
            focused_background: Color::Reset,
            focused_border: Color::Reset,
            hovered_foreground: Color::White,
            hovered_background: Color::Reset,
            hovered_border: Color::Reset,
            disabled_foreground: Color::DarkGray,
            disabled_background: Color::Reset,
            disabled_border: Color::Reset,
        };
        let area = Rect::new(0, 0, 6, ButtonSize::Large.height());
        let mut buffer = Buffer::empty(area);

        ButtonWidget::new("OK")
            .style(style)
            .size(ButtonSize::Large)
            .render(area, &mut buffer);

        assert_eq!(buffer.cell((0, 0)).expect("top cap").symbol(), " ");
        assert_eq!(buffer.cell((0, 2)).expect("bottom cap").symbol(), " ");
    }

    #[test]
    fn from_theme_derives_focus_and_disabled_fills_from_the_base() {
        let theme = Theme::default_dark();

        // The focused fill is derived by shifting the base rather than stored
        // on the theme. This theme's background is dark, so a loud fill deepens
        // toward it.
        let default = ButtonStyle::from_theme(&theme, ButtonVariant::Default);
        let (Color::Rgb(br, bg, bb), Color::Rgb(fr, fg, fb)) =
            (default.background, default.focused_background)
        else {
            panic!("expected rgb fills");
        };
        assert_ne!(default.focused_background, default.background);
        assert!(
            fr <= br && fg <= bg && fb <= bb,
            "focus should deepen a loud fill toward a dark theme's background"
        );

        // Disabled dims toward the surface but keeps the variant's hue, so a
        // disabled destructive button still reads as destructive.
        let destructive = ButtonStyle::from_theme(&theme, ButtonVariant::Destructive);
        let Color::Rgb(r, g, b) = destructive.disabled_background else {
            panic!("expected an rgb color");
        };
        assert!(
            r > g && r > b,
            "dimmed destructive should remain red-ish: {r},{g},{b}"
        );
    }

    #[test]
    fn style_override_replaces_the_variant_style() {
        let theme = Theme::default_dark();
        // Built inside the declaration closure. No `on_press`, so the button
        // is not a focus candidate and this paints in its base state.
        let button = || {
            Button::<()>::new("OK").style(|theme| {
                let mut style = ButtonStyle::from_theme(theme, ButtonVariant::Default);
                style.background = theme.accent;
                style
            })
        };

        let mut driver = Driver::new(10, 3);
        let area = driver.area();
        driver.render(&(), |ctx| {
            ctx.component(ChildId::Static("button"), button(), area);
        });

        assert_eq!(driver.cell(1, 0).bg, theme.accent);
    }

    #[test]
    fn retained_disabledness_controls_click_focus_and_press_eligibility() {
        #[derive(Default)]
        struct State {
            focus: FocusState,
            disabled: bool,
        }

        #[derive(Debug, Clone, PartialEq)]
        enum Msg {
            Focus(FocusState),
            Pressed,
        }

        let mut state = State {
            focus: FocusState::intent([ChildId::Static("other")]),
            ..State::default()
        };
        let mut driver = Driver::with(
            Ratcn::new().focus(|state: &State| &state.focus, Msg::Focus),
            20,
            1,
        );
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
            let area = driver.area();
            driver.render(state, |ctx| {
                ctx.component(
                    ChildId::Static("other"),
                    Button::<Msg>::new("Other").on_press(|| Msg::Pressed),
                    Rect::new(0, 0, 10, 1),
                );
                ctx.component(
                    ChildId::Static("button"),
                    Button::new("Save")
                        .disabled(state.disabled)
                        .on_press(|| Msg::Pressed),
                    Rect::new(10, 0, area.width.saturating_sub(10), 1),
                );
            });
        };
        let mouse = |kind| {
            Event::Mouse(MouseEvent {
                kind,
                column: 11,
                row: 0,
                modifiers: Modifiers::NONE,
            })
        };

        render(&mut driver, &state);
        state.disabled = true;
        let EventResult::Emit(Msg::Focus(focus)) =
            driver.event(mouse(MouseKind::Down(MouseButton::Left)), &state)
        else {
            panic!("retained enabled button should focus on press");
        };
        state.focus = focus;
        assert_eq!(
            driver.event(mouse(MouseKind::Up(MouseButton::Left)), &state),
            EventResult::Emit(Msg::Pressed)
        );

        render(&mut driver, &state);
        state.disabled = false;
        state.focus = FocusState::intent([ChildId::Static("other")]);
        assert_eq!(
            driver.event(mouse(MouseKind::Down(MouseButton::Left)), &state),
            EventResult::Ignored
        );
        assert_eq!(
            driver.event(mouse(MouseKind::Up(MouseButton::Left)), &state),
            EventResult::Ignored
        );
    }

    #[test]
    fn excess_allocated_rows_do_not_focus_or_press_the_button() {
        #[derive(Default)]
        struct State {
            focus: FocusState,
        }

        #[derive(Debug, PartialEq)]
        enum Msg {
            Focus(FocusState),
            Pressed,
        }

        let mut state = State {
            focus: FocusState::intent([ChildId::Static("other")]),
        };
        let mut driver = Driver::with(
            Ratcn::new().focus(|state: &State| &state.focus, Msg::Focus),
            20,
            4,
        );
        driver.render(&state, |ctx| {
            ctx.component(
                ChildId::Static("other"),
                Button::<Msg>::new("Other").on_press(|| Msg::Pressed),
                Rect::new(0, 0, 8, 1),
            );
            ctx.component(
                ChildId::Static("button"),
                Button::new("Save").on_press(|| Msg::Pressed),
                Rect::new(10, 0, 10, 4),
            );
        });
        let mouse = |kind, row| {
            Event::Mouse(MouseEvent {
                kind,
                column: 11,
                row,
                modifiers: Modifiers::NONE,
            })
        };

        assert_eq!(
            driver.event(mouse(MouseKind::Down(MouseButton::Left), 2), &state),
            EventResult::Ignored
        );
        assert_eq!(
            driver.event(mouse(MouseKind::Up(MouseButton::Left), 2), &state),
            EventResult::Ignored
        );
        assert_eq!(state.focus, FocusState::intent([ChildId::Static("other")]));

        let EventResult::Emit(Msg::Focus(focus)) =
            driver.event(mouse(MouseKind::Down(MouseButton::Left), 0), &state)
        else {
            panic!("the painted button row should focus");
        };
        state.focus = focus;
        assert_eq!(
            driver.event(mouse(MouseKind::Up(MouseButton::Left), 0), &state),
            EventResult::Emit(Msg::Pressed)
        );
    }
}
