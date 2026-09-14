//! A one-line text field: the value lives in app state, the caret in a
//! transient.
//!
//! ```text
//! Name
//! ```
//!
//! The field is an inset well, the same surface a list or a closed select
//! sits in. At rest it is one tone off the background; focus and hover lift
//! it a step, the way every other well does. Empty, it shows a placeholder.
//! Focused, a blinking `|` marks the caret.
//!
//! State is app-owned. The component reads the string through a binding and
//! emits a new one each time the user types, pastes, or deletes. The caret
//! is presentation: it lives in a transient so a rebuild does not lose it
//! and so the app never has to store it.

use std::{fmt, rc::Rc};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::Widget,
};

use crate::{
    Theme,
    color::{DISABLED_DIM, FIELD_FOCUS_SHIFT, FIELD_HOVER_SHIFT, away_from, blendable, dim},
    runtime::{
        Component, DeclareCtx, Event, EventCtx, EventResult, KeyCode, KeyEvent, MouseButton,
        MouseKind, PaintCtx, ScopeOptions,
    },
    text_width,
    theme::resolve_style,
};

/// Cells of well on each side of the value. The same inset a closed select
/// and a cycle leave around their text, so a row of fields lines up.
const PAD: u16 = 1;
/// Half-period of the focused caret. On for this long, off for this long.
const CARET_BLINK: u128 = 530;
/// The insertion mark: a one-cell bar, not a reversed block, so the
/// character under the caret stays readable while the bar is off.
const CARET: &str = "|";

/// An input's colors, sharing the well ladder every inset control uses.
///
/// At rest [`foreground`](Self::foreground) sits on
/// [`background`](Self::background). Hover and focus use the corresponding
/// well backgrounds, with hover beating focus. Empty, the placeholder uses
/// [`placeholder`](Self::placeholder). Disabled mutes the text and the well.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputStyle {
    /// Value color at rest.
    pub foreground: Color,
    /// Background at rest.
    pub background: Color,
    /// Value color while focused.
    pub focused_foreground: Color,
    /// Background while focused.
    pub focused_background: Color,
    /// Value color while hovered.
    pub hovered_foreground: Color,
    /// Background while hovered.
    pub hovered_background: Color,
    /// Placeholder color while the value is empty.
    pub placeholder: Color,
    /// Value color while disabled.
    pub disabled_foreground: Color,
    /// Background while disabled.
    pub disabled_background: Color,
}

impl InputStyle {
    /// The no-theme starting point: plain ANSI colors that render on any
    /// terminal, with the same three well backgrounds a list uses.
    #[must_use]
    pub const fn fallback() -> Self {
        Self {
            foreground: Color::Reset,
            background: Color::Reset,
            focused_foreground: Color::Reset,
            focused_background: Color::Reset,
            hovered_foreground: Color::Reset,
            hovered_background: Color::DarkGray,
            placeholder: Color::DarkGray,
            disabled_foreground: Color::DarkGray,
            disabled_background: Color::Reset,
        }
    }

    /// Colors derived from a theme: the well ladder a list and a closed
    /// select share, with the value keeping the theme's foreground on it.
    #[must_use]
    pub fn from_theme(theme: &Theme) -> Self {
        let backdrop = blendable(theme.background, theme.surface);
        let away = away_from(theme.background);
        Self {
            foreground: theme.foreground,
            background: theme.field,
            focused_foreground: theme.foreground,
            focused_background: dim(theme.field, away, FIELD_FOCUS_SHIFT),
            hovered_foreground: theme.foreground,
            hovered_background: dim(theme.field, away, FIELD_HOVER_SHIFT),
            placeholder: theme.muted_foreground,
            disabled_foreground: dim(theme.muted_foreground, backdrop, DISABLED_DIM),
            disabled_background: dim(theme.field, backdrop, DISABLED_DIM),
        }
    }

    /// One paint pass's colors (see [`Self::from_theme`]). Disabled wins over
    /// hover, which wins over focus, which wins over rest.
    fn resolve(self, focused: bool, hovered: bool, disabled: bool) -> (Color, Color) {
        if disabled {
            (self.disabled_foreground, self.disabled_background)
        } else if hovered {
            (self.hovered_foreground, self.hovered_background)
        } else if focused {
            (self.focused_foreground, self.focused_background)
        } else {
            (self.foreground, self.background)
        }
    }
}

/// An input that only draws — an ordinary ratatui [`Widget`] with no focus,
/// events, or state. It paints the current value, or the placeholder while
/// the value is empty. The caret is the interactive half's business.
#[expect(
    clippy::struct_excessive_bools,
    reason = "focused, hovered, disabled, and caret_on are independent paint flags"
)]
#[derive(Debug)]
pub struct InputWidget<'a> {
    value: &'a str,
    placeholder: &'a str,
    focused: bool,
    hovered: bool,
    disabled: bool,
    caret: Option<usize>,
    caret_on: bool,
    theme: Option<Theme>,
    style: Option<InputStyle>,
}

impl<'a> InputWidget<'a> {
    /// An input showing `value`. Empty, it paints the placeholder instead.
    #[must_use]
    pub const fn new(value: &'a str) -> Self {
        Self {
            value,
            placeholder: "",
            focused: false,
            hovered: false,
            disabled: false,
            caret: None,
            caret_on: true,
            theme: None,
            style: None,
        }
    }

    /// Take colors from `theme`.
    #[must_use]
    pub const fn themed(mut self, theme: &Theme) -> Self {
        self.theme = Some(*theme);
        self
    }

    /// Exact colors, taking precedence over [`themed`](Self::themed).
    #[must_use]
    pub const fn style(mut self, style: InputStyle) -> Self {
        self.style = Some(style);
        self
    }

    /// The text shown while `value` is empty.
    #[must_use]
    pub const fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// Paint the focused well.
    #[must_use]
    pub const fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Paint the hovered well.
    #[must_use]
    pub const fn hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }

    /// Paint muted.
    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Overlay a `|` at grapheme `index` of `value`. `None` hides it. An
    /// index past the last grapheme sits after the value. The bar does not
    /// take a column, so moving it never shifts the letters.
    #[must_use]
    pub const fn caret(mut self, index: Option<usize>) -> Self {
        self.caret = index;
        self
    }

    /// Whether that `|` is lit. The interactive half blinks this; a paint-only
    /// widget defaults to on whenever a caret is set.
    #[must_use]
    pub const fn caret_on(mut self, on: bool) -> Self {
        self.caret_on = on;
        self
    }

    fn resolved_style(&self) -> InputStyle {
        match (self.style, self.theme) {
            (Some(style), _) => style,
            (None, Some(theme)) => InputStyle::from_theme(&theme),
            (None, None) => InputStyle::fallback(),
        }
    }
}

impl Widget for InputWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = crate::geometry::fixed_height(area, 1);
        if area.width == 0 {
            return;
        }
        let style = self.resolved_style();
        let (foreground, background) = style.resolve(self.focused, self.hovered, self.disabled);
        buf.set_style(area, Style::default().fg(foreground).bg(background));

        let inner_width = area.width.saturating_sub(PAD.saturating_mul(2));
        if inner_width == 0 {
            return;
        }
        let inner = Rect {
            x: area.x.saturating_add(PAD),
            width: inner_width,
            ..area
        };

        let empty = self.value.is_empty();
        let shown = if empty { self.placeholder } else { self.value };
        let text_style = if empty && !self.disabled {
            Style::default().fg(style.placeholder).bg(background)
        } else {
            Style::default().fg(foreground).bg(background)
        };
        let visible = visible_window(
            shown,
            usize::from(inner_width),
            self.caret.filter(|_| !empty),
        );
        Line::from(visible.text)
            .style(text_style)
            .render(inner, buf);

        let Some(caret) = self.caret else {
            return;
        };
        if !self.caret_on {
            return;
        }
        let caret_col = if empty {
            0
        } else {
            caret_column(self.value, caret, visible.skip)
        };
        if caret_col >= usize::from(inner_width) {
            return;
        }
        let x = inner
            .x
            .saturating_add(u16::try_from(caret_col).unwrap_or(u16::MAX));
        if let Some(cell) = buf.cell_mut((x, inner.y)) {
            cell.set_symbol(CARET);
            cell.set_fg(foreground);
        }
    }
}

/// Whether the focused caret is in its on half-period, measured from
/// `origin_ms` so a keystroke or click restarts the on half.
fn caret_blink_on(origin_ms: u128) -> bool {
    (elapsed_ms().saturating_sub(origin_ms) / CARET_BLINK).is_multiple_of(2)
}

fn elapsed_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

/// The slice of `text` that fits in `width` cells, scrolled so `caret` stays
/// on screen when the value is wider than the well.
struct VisibleWindow<'a> {
    text: &'a str,
    /// Graphemes skipped on the left of `text`.
    skip: usize,
}

fn visible_window(text: &str, width: usize, caret: Option<usize>) -> VisibleWindow<'_> {
    if width == 0 {
        return VisibleWindow { text: "", skip: 0 };
    }
    if text_width::display_width(text) <= width {
        return VisibleWindow { text, skip: 0 };
    }
    let total = text_width::grapheme_count(text);
    let caret = caret.unwrap_or(total).min(total);
    let caret_cells = caret_column(text, caret, 0);
    let mut skip = 0;
    let mut skipped_cells = 0;
    // Keep the caret in the last cell of the window: skip until the
    // remaining distance is strictly less than `width`.
    for cluster in text_width::graphemes(text) {
        if caret_cells.saturating_sub(skipped_cells) < width {
            break;
        }
        skip += 1;
        skipped_cells += text_width::display_width(cluster);
        if skip >= caret {
            break;
        }
    }
    let start = text_width::grapheme_byte(text, skip);
    VisibleWindow {
        text: text_width::truncate_to_width(&text[start..], width),
        skip,
    }
}

fn caret_column(value: &str, caret: usize, skip: usize) -> usize {
    text_width::graphemes(value)
        .take(caret)
        .skip(skip)
        .map(text_width::display_width)
        .sum()
}

/// Clamp `caret` into `0..=grapheme_count(value)`.
fn clamp_caret(value: &str, caret: usize) -> usize {
    caret.min(text_width::grapheme_count(value))
}

/// Insert `insert` at grapheme `caret` and return the new value with the
/// caret sitting after the insertion.
fn insert_at(value: &str, caret: usize, insert: &str) -> (String, usize) {
    let caret = clamp_caret(value, caret);
    let at = text_width::grapheme_byte(value, caret);
    let mut next = String::with_capacity(value.len() + insert.len());
    next.push_str(&value[..at]);
    next.push_str(insert);
    next.push_str(&value[at..]);
    let added = text_width::grapheme_count(insert);
    (next, caret.saturating_add(added))
}

/// Delete the grapheme before `caret`.
fn delete_before(value: &str, caret: usize) -> Option<(String, usize)> {
    let caret = clamp_caret(value, caret);
    if caret == 0 {
        return None;
    }
    let start = text_width::grapheme_byte(value, caret - 1);
    let end = text_width::grapheme_byte(value, caret);
    let mut next = String::with_capacity(value.len() - (end - start));
    next.push_str(&value[..start]);
    next.push_str(&value[end..]);
    Some((next, caret - 1))
}

/// Delete the grapheme at `caret`.
fn delete_at(value: &str, caret: usize) -> Option<(String, usize)> {
    let caret = clamp_caret(value, caret);
    if caret == text_width::grapheme_count(value) {
        return None;
    }
    splice(value, caret, caret + 1)
}

/// A word grapheme: letters, digits, or `_`. Everything else is a separator
/// (space, punctuation), so `foo_bar` is one word and `foo-bar` is two.
fn is_word(grapheme: &str) -> bool {
    grapheme
        .chars()
        .next()
        .is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
}

/// Start of the word (and any separators after it) sitting immediately
/// before `caret`. `caret` itself when there is nothing to the left.
fn word_before(value: &str, caret: usize) -> usize {
    let caret = clamp_caret(value, caret);
    let clusters: Vec<&str> = text_width::graphemes(value).take(caret).collect();
    let mut i = clusters.len();
    while i > 0 && !is_word(clusters[i - 1]) {
        i -= 1;
    }
    while i > 0 && is_word(clusters[i - 1]) {
        i -= 1;
    }
    i
}

/// End of the word (and any separators before it) sitting immediately
/// after `caret`. `caret` itself when there is nothing to the right.
fn word_after(value: &str, caret: usize) -> usize {
    let caret = clamp_caret(value, caret);
    let mut i = caret;
    for grapheme in text_width::graphemes(value).skip(caret) {
        if is_word(grapheme) {
            break;
        }
        i += 1;
    }
    for grapheme in text_width::graphemes(value).skip(i) {
        if !is_word(grapheme) {
            break;
        }
        i += 1;
    }
    i
}

fn delete_word_before(value: &str, caret: usize) -> Option<(String, usize)> {
    let caret = clamp_caret(value, caret);
    let start = word_before(value, caret);
    splice(value, start, caret)
}

fn delete_word_after(value: &str, caret: usize) -> Option<(String, usize)> {
    let caret = clamp_caret(value, caret);
    splice(value, caret, word_after(value, caret))
}

/// ASCII BS and DEL. Some hosts report erase as a character instead of
/// [`KeyCode::Backspace`].
fn is_erase_char(ch: char) -> bool {
    ch == '\u{8}' || ch == '\u{7f}'
}

fn is_erase_key(code: KeyCode) -> bool {
    matches!(code, KeyCode::Backspace | KeyCode::Char('\u{8}' | '\u{7f}'))
}

/// Backward word-kill, as each host actually reports it.
///
/// - kitty / the browser: Backspace with Control
/// - Windows `ConPTY` and most xterm / WSL: `0x08` → Ctrl+H
/// - macOS Option and emacs-style Alt: Backspace with Alt
fn word_kill_back(key: KeyEvent) -> bool {
    if key.modifiers.shift {
        return false;
    }
    let erase = is_erase_key(key.code);
    if key.modifiers.ctrl && !key.modifiers.alt {
        return erase || matches!(key.code, KeyCode::Char('h'));
    }
    key.modifiers.alt && !key.modifiers.ctrl && erase
}

/// Forward word-kill: Delete with Control (CSI `3;5~` on xterm / WSL) or
/// with Alt (Option+Fn+Delete on macOS).
fn word_kill_forward(key: KeyEvent) -> bool {
    if key.modifiers.shift || !matches!(key.code, KeyCode::Delete) {
        return false;
    }
    (key.modifiers.ctrl && !key.modifiers.alt) || (key.modifiers.alt && !key.modifiers.ctrl)
}

fn splice(value: &str, start: usize, end: usize) -> Option<(String, usize)> {
    if start >= end {
        return None;
    }
    let from = text_width::grapheme_byte(value, start);
    let to = text_width::grapheme_byte(value, end);
    let mut next = String::with_capacity(value.len() - (to - from));
    next.push_str(&value[..from]);
    next.push_str(&value[to..]);
    Some((next, start))
}

/// The caret, kept against this field's identity path. Default is "at the
/// end of whatever value is shown", represented as `None` so a first focus
/// lands after existing text without the app storing a number.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Caret {
    index: Option<usize>,
    /// Subtracted from [`elapsed_ms`] so a keystroke or click lights the bar
    /// instead of leaving it in the off half-period.
    blink_origin_ms: u128,
}

type ReadValueFn<S> = Rc<dyn Fn(&S) -> &str>;
type OnChangeFn<M> = Rc<dyn Fn(String) -> M>;
type StyleFn = Rc<dyn Fn(&Theme) -> InputStyle>;

/// A one-line text field. The value lives in app state and arrives through
/// [`value`](Self::value); without that binding the field paints but is not
/// focusable and answers no events.
///
/// Printable characters insert at the caret, Backspace deletes before it,
/// Delete after it. Ctrl+Backspace and Ctrl+Delete delete the word on that
/// side — separators first, then the word — the same family as the field's
/// own arrows. Left and Right move by grapheme; Home and End jump. Paste
/// inserts the clipboard as one piece, with newlines flattened to spaces so
/// the field stays one line. Tab, Enter, Esc, and Ctrl+W are left to the app.
///
/// The caret is a transient. A click places it at the grapheme under the
/// pointer; a first keystroke with no click yet types at the end.
pub struct Input<S, M> {
    placeholder: String,
    value: Option<(ReadValueFn<S>, OnChangeFn<M>)>,
    disabled: bool,
    style: Option<StyleFn>,
    /// The bound value, resolved once per declaration.
    resolved_value: String,
    /// Caret grapheme index read during declare, for paint. `None` means the
    /// end of the value — a first focus has not yet placed it.
    paint_caret: Option<usize>,
    /// Blink origin read during declare, so a keystroke lights the bar.
    paint_blink_origin_ms: u128,
}

impl<S, M> fmt::Debug for Input<S, M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Input")
            .field("placeholder", &self.placeholder)
            .field("value", &self.value.is_some())
            .field("disabled", &self.disabled)
            .field("style", &self.style.is_some())
            .finish_non_exhaustive()
    }
}

impl<S, M> Input<S, M> {
    /// An empty field with no placeholder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            placeholder: String::new(),
            value: None,
            disabled: false,
            style: None,
            resolved_value: String::new(),
            paint_caret: None,
            paint_blink_origin_ms: 0,
        }
    }

    /// The text shown while the bound value is empty.
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Bind the value and the message that replaces it.
    ///
    /// `read` is consulted for every event, so repeated keystrokes compose
    /// before a redraw, and `on_change` receives the whole new string after
    /// each edit. Without this binding the field is not focusable and answers
    /// no events.
    #[must_use]
    pub fn value(
        mut self,
        read: impl Fn(&S) -> &str + 'static,
        on_change: impl Fn(String) -> M + 'static,
    ) -> Self {
        self.value = Some((Rc::new(read), Rc::new(on_change)));
        self
    }

    /// Paint and answer muted, ignoring every event.
    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Supply exact colors, taking precedence over the theme.
    #[must_use]
    pub fn style(mut self, style: impl Fn(&Theme) -> InputStyle + 'static) -> Self {
        self.style = Some(Rc::new(style));
        self
    }

    fn can_act(&self) -> bool {
        !self.disabled && self.value.is_some()
    }

    fn emit(&self, next: String) -> EventResult<M> {
        let Some((_, on_change)) = &self.value else {
            return EventResult::Ignored;
        };
        EventResult::Emit(on_change(next))
    }

    fn current_value<'a>(&self, state: &'a S) -> &'a str {
        self.value.as_ref().map_or("", |(read, _)| read(state))
    }

    fn caret_of(value: &str, ctx: &mut EventCtx<'_>) -> usize {
        clamp_caret(
            value,
            ctx.transient::<Caret>()
                .index
                .unwrap_or_else(|| text_width::grapheme_count(value)),
        )
    }

    fn set_caret(ctx: &mut EventCtx<'_>, caret: usize) {
        let stored = ctx.transient::<Caret>();
        stored.index = Some(caret);
        stored.blink_origin_ms = elapsed_ms();
    }

    /// Grapheme under `column` in the painted well. The bar overlays a
    /// character without taking a column, so a click maps straight onto the
    /// letter it covers — or past the last letter, to the end.
    fn caret_at_column(value: &str, area: Rect, column: u16, caret: usize) -> usize {
        let inner_x = area.x.saturating_add(PAD);
        let inner_width = area.width.saturating_sub(PAD.saturating_mul(2));
        if inner_width == 0 || column < inner_x {
            return 0;
        }
        let offset = usize::from(column.saturating_sub(inner_x));
        let visible = visible_window(value, usize::from(inner_width), Some(caret));
        let mut used = 0;
        for (i, cluster) in text_width::graphemes(visible.text).enumerate() {
            let width = text_width::display_width(cluster);
            if offset < used + width {
                return visible.skip + i;
            }
            used += width;
        }
        visible.skip + text_width::grapheme_count(visible.text)
    }

    fn apply_delete(
        &self,
        ctx: &mut EventCtx<'_>,
        deleted: Option<(String, usize)>,
    ) -> EventResult<M> {
        match deleted {
            Some((next, caret)) => {
                Self::set_caret(ctx, caret);
                self.emit(next)
            }
            None => EventResult::Consumed,
        }
    }

    fn handle_key(&self, key: KeyEvent, value: &str, ctx: &mut EventCtx<'_>) -> EventResult<M> {
        let caret = Self::caret_of(value, ctx);
        // Word-kill is this field's own chord, the way Ctrl+N is a list's.
        // Hosts disagree on the bytes: see [`word_kill_back`]. Ctrl+W still
        // bubbles — many hosts close with it. Shift+Ctrl is selection.
        if word_kill_back(key) {
            return self.apply_delete(ctx, delete_word_before(value, caret));
        }
        if word_kill_forward(key) {
            return self.apply_delete(ctx, delete_word_after(value, caret));
        }
        if key.modifiers.ctrl || key.modifiers.alt {
            return EventResult::Ignored;
        }
        match key.code {
            // Shift belongs to the character, not to a modifier chord: a
            // shifted `a` arrives as `Char('A')`. Navigation with Shift held
            // is left to the app, matching the shared keyboard map.
            KeyCode::Char(ch) if !is_erase_char(ch) => {
                let mut utf8 = [0; 4];
                let insert = ch.encode_utf8(&mut utf8);
                let (next, next_caret) = insert_at(value, caret, insert);
                Self::set_caret(ctx, next_caret);
                self.emit(next)
            }
            _ if key.modifiers.shift => EventResult::Ignored,
            KeyCode::Backspace | KeyCode::Char('\u{8}' | '\u{7f}') => {
                self.apply_delete(ctx, delete_before(value, caret))
            }
            KeyCode::Delete => self.apply_delete(ctx, delete_at(value, caret)),
            KeyCode::Left => {
                Self::set_caret(ctx, caret.saturating_sub(1));
                EventResult::Consumed
            }
            KeyCode::Right => {
                Self::set_caret(ctx, caret.saturating_add(1));
                EventResult::Consumed
            }
            KeyCode::Home => {
                Self::set_caret(ctx, 0);
                EventResult::Consumed
            }
            KeyCode::End => {
                Self::set_caret(ctx, text_width::grapheme_count(value));
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_paste(&self, pasted: &str, value: &str, ctx: &mut EventCtx<'_>) -> EventResult<M> {
        if pasted.is_empty() {
            return EventResult::Consumed;
        }
        let flattened = pasted.replace("\r\n", " ").replace(['\n', '\r'], " ");
        let caret = Self::caret_of(value, ctx);
        let (next, next_caret) = insert_at(value, caret, &flattened);
        Self::set_caret(ctx, next_caret);
        self.emit(next)
    }
}

impl<S, M> Default for Input<S, M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: 'static, M: 'static> Component<S, M> for Input<S, M> {
    fn prepare(&mut self, state: &S) {
        self.resolved_value = self
            .value
            .as_ref()
            .map_or(String::new(), |(read, _)| read(state).to_owned());
    }

    fn declare(&mut self, ctx: &mut DeclareCtx<'_, S, M>) {
        let stored = ctx.transient::<Caret>().copied().unwrap_or_default();
        self.paint_caret = stored.index;
        self.paint_blink_origin_ms = stored.blink_origin_ms;
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, S>) {
        let style = resolve_style(self.style.as_deref(), ctx.theme, InputStyle::from_theme);
        let caret = ctx.focused().then(|| {
            clamp_caret(
                &self.resolved_value,
                self.paint_caret
                    .unwrap_or_else(|| text_width::grapheme_count(&self.resolved_value)),
            )
        });
        let widget = InputWidget::new(&self.resolved_value)
            .placeholder(&self.placeholder)
            .focused(ctx.focused())
            .hovered(ctx.hovered())
            .disabled(self.disabled)
            .caret(caret)
            .caret_on(caret.is_some() && caret_blink_on(self.paint_blink_origin_ms))
            .style(style);
        ctx.widget(widget, ctx.area());
    }

    fn handle_event(&mut self, event: &Event, state: &S, ctx: &mut EventCtx<'_>) -> EventResult<M> {
        if !self.can_act() {
            return EventResult::Ignored;
        }
        let value = self.current_value(state);
        match event {
            Event::Mouse(mouse) => match mouse.kind {
                MouseKind::Click(MouseButton::Left) => {
                    let caret = Self::caret_at_column(
                        value,
                        ctx.area(),
                        mouse.column,
                        Self::caret_of(value, ctx),
                    );
                    Self::set_caret(ctx, caret);
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            },
            Event::Key(key) => self.handle_key(*key, value, ctx),
            Event::Paste(pasted) => self.handle_paste(pasted, value, ctx),
            // `Event` is non-exhaustive: a copied module compiled against the
            // public API needs this arm. Inside this crate every variant is
            // named above, so the arm is unreachable here.
            #[allow(
                unreachable_patterns,
                reason = "covers future Event variants for a copied module"
            )]
            _ => EventResult::Ignored,
        }
    }

    fn scope_options(&self) -> ScopeOptions {
        ScopeOptions::default().focusable(self.can_act())
    }

    fn interaction_area(&self, area: Rect) -> Rect {
        crate::geometry::fixed_height(area, 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{ChildId, FocusState, Ratcn};
    use crate::test_support::{Driver, key, mouse};

    #[derive(Default)]
    struct State {
        focus: FocusState,
        name: String,
    }

    #[derive(Debug, Clone, PartialEq)]
    enum Msg {
        Focus(FocusState),
        Name(String),
    }

    fn driver() -> Driver<State, Msg> {
        Driver::with(
            Ratcn::new().focus(|state: &State| &state.focus, Msg::Focus),
            24,
            3,
        )
    }

    fn field() -> Input<State, Msg> {
        Input::new()
            .placeholder("Name")
            .value(|state: &State| state.name.as_str(), Msg::Name)
    }

    fn render(driver: &mut Driver<State, Msg>, state: &State) {
        driver.render(state, |ctx| {
            ctx.component(ChildId::Static("name"), field(), Rect::new(2, 1, 16, 1));
        });
    }

    fn apply(state: &mut State, msg: Msg) {
        match msg {
            Msg::Name(name) => state.name = name,
            Msg::Focus(focus) => state.focus = focus,
        }
    }

    fn type_char(driver: &mut Driver<State, Msg>, state: &mut State, ch: char) {
        let EventResult::Emit(msg) = driver.event(key(KeyCode::Char(ch)), state) else {
            panic!("{ch:?} must edit");
        };
        apply(state, msg);
        render(driver, state);
    }

    fn row(driver: &Driver<State, Msg>) -> String {
        (0..24u16)
            .map(|column| driver.cell(column, 1).symbol().to_owned())
            .collect()
    }

    /// Typing inserts at the caret and the next frame paints the new value.
    /// Each event runs through the app's update and a fresh declaration.
    #[test]
    fn typing_inserts_and_paints_the_new_value() {
        let mut driver = driver();
        let mut state = State::default();
        render(&mut driver, &state);

        type_char(&mut driver, &mut state, 'H');
        type_char(&mut driver, &mut state, 'i');
        assert_eq!(state.name, "Hi");
        assert!(row(&driver).contains("Hi"), "{}", row(&driver));
    }

    /// A bound reader runs for every event, so two keystrokes compose even
    /// without a redraw between them.
    #[test]
    fn repeated_edits_read_current_app_value_before_redraw() {
        let mut driver = driver();
        let mut state = State::default();
        render(&mut driver, &state);

        let EventResult::Emit(msg) = driver.event(key(KeyCode::Char('H')), &state) else {
            panic!("H must edit");
        };
        apply(&mut state, msg);
        let EventResult::Emit(msg) = driver.event(key(KeyCode::Char('i')), &state) else {
            panic!("i must edit against the updated value");
        };
        apply(&mut state, msg);
        assert_eq!(state.name, "Hi");
    }

    /// Empty, the field shows its placeholder rather than a blank well.
    /// The widget is unfocused so a caret cannot cover the first letter.
    #[test]
    fn empty_paints_the_placeholder() {
        let area = Rect::new(0, 0, 16, 1);
        let theme = Theme::default_dark();
        let mut buffer = Buffer::empty(area);
        InputWidget::new("")
            .placeholder("Name")
            .themed(&theme)
            .render(area, &mut buffer);
        let row: String = (0..16u16)
            .map(|column| buffer.cell((column, 0)).expect("cell").symbol().to_owned())
            .collect();
        assert!(row.contains("Name"), "{row}");
    }

    /// Backspace deletes the grapheme before the caret; Delete the one after.
    /// At either end the key is consumed so an app hotkey on those keys does
    /// not fire while the field has focus.
    #[test]
    fn backspace_and_delete_edit_around_the_caret() {
        let mut driver = driver();
        let mut state = State {
            name: "Hi".into(),
            ..State::default()
        };
        render(&mut driver, &state);

        assert_eq!(
            driver.event(key(KeyCode::Left), &state),
            EventResult::Consumed
        );
        let EventResult::Emit(msg) = driver.event(key(KeyCode::Backspace), &state) else {
            panic!("Backspace must delete");
        };
        apply(&mut state, msg);
        assert_eq!(state.name, "i");
        render(&mut driver, &state);

        let EventResult::Emit(msg) = driver.event(key(KeyCode::Delete), &state) else {
            panic!("Delete must delete");
        };
        apply(&mut state, msg);
        assert_eq!(state.name, "");
        render(&mut driver, &state);

        assert_eq!(
            driver.event(key(KeyCode::Backspace), &state),
            EventResult::Consumed,
            "Backspace at the start belongs to the field"
        );
    }

    /// Ctrl+Backspace and Ctrl+Delete kill a word: separators first, then
    /// the word. At either end the chord is consumed so it cannot fire an
    /// app binding. Ctrl+W is not this field's — many hosts close with it.
    #[test]
    fn ctrl_backspace_and_ctrl_delete_kill_a_word() {
        let mut driver = driver();
        let mut state = State {
            name: "hello world".into(),
            ..State::default()
        };
        render(&mut driver, &state);

        let ctrl = crate::runtime::Modifiers {
            ctrl: true,
            ..crate::runtime::Modifiers::NONE
        };
        let EventResult::Emit(msg) = driver.event(
            crate::test_support::key_with(KeyCode::Backspace, ctrl),
            &state,
        ) else {
            panic!("Ctrl+Backspace must kill the word before the caret");
        };
        apply(&mut state, msg);
        assert_eq!(state.name, "hello ");
        render(&mut driver, &state);

        let EventResult::Emit(msg) = driver.event(
            crate::test_support::key_with(KeyCode::Backspace, ctrl),
            &state,
        ) else {
            panic!("a second Ctrl+Backspace must take the remaining word");
        };
        apply(&mut state, msg);
        assert_eq!(state.name, "");
        render(&mut driver, &state);

        state.name = "hello world".into();
        render(&mut driver, &state);
        assert_eq!(
            driver.event(key(KeyCode::Home), &state),
            EventResult::Consumed
        );
        let EventResult::Emit(msg) =
            driver.event(crate::test_support::key_with(KeyCode::Delete, ctrl), &state)
        else {
            panic!("Ctrl+Delete must kill the word after the caret");
        };
        apply(&mut state, msg);
        assert_eq!(state.name, " world");

        assert_eq!(
            driver.event(
                crate::test_support::key_with(KeyCode::Char('w'), ctrl),
                &state
            ),
            EventResult::Ignored,
            "Ctrl+W belongs to the app"
        );
    }

    /// Windows `ConPTY` reports Ctrl+Backspace as Ctrl+H (`0x08`), not as
    /// Backspace with Control. The field must still kill the word.
    #[test]
    fn windows_ctrl_backspace_arrives_as_ctrl_h() {
        let mut driver = driver();
        let mut state = State {
            name: "hello world".into(),
            ..State::default()
        };
        render(&mut driver, &state);

        let ctrl = crate::runtime::Modifiers {
            ctrl: true,
            ..crate::runtime::Modifiers::NONE
        };
        let EventResult::Emit(msg) = driver.event(
            crate::test_support::key_with(KeyCode::Char('h'), ctrl),
            &state,
        ) else {
            panic!("Ctrl+H is how Windows ConPTY spells Ctrl+Backspace");
        };
        apply(&mut state, msg);
        assert_eq!(state.name, "hello ");
    }

    /// macOS Option+Backspace and emacs Alt+Backspace are the same chord.
    #[test]
    fn option_backspace_kills_a_word() {
        let mut driver = driver();
        let mut state = State {
            name: "hello world".into(),
            ..State::default()
        };
        render(&mut driver, &state);

        let alt = crate::runtime::Modifiers {
            alt: true,
            ..crate::runtime::Modifiers::NONE
        };
        let EventResult::Emit(msg) = driver.event(
            crate::test_support::key_with(KeyCode::Backspace, alt),
            &state,
        ) else {
            panic!("Option+Backspace must kill the word before the caret");
        };
        apply(&mut state, msg);
        assert_eq!(state.name, "hello ");
    }

    /// A wide grapheme is one caret stop, not two cells of motion.
    #[test]
    fn the_caret_moves_by_grapheme() {
        let mut driver = driver();
        let mut state = State {
            name: "a😀b".into(),
            ..State::default()
        };
        render(&mut driver, &state);

        assert_eq!(
            driver.event(key(KeyCode::Left), &state),
            EventResult::Consumed
        );
        let EventResult::Emit(msg) = driver.event(key(KeyCode::Backspace), &state) else {
            panic!("Backspace must delete the emoji as one grapheme");
        };
        apply(&mut state, msg);
        assert_eq!(state.name, "ab");
    }

    /// Paste arrives as one string. Newlines flatten so the field stays one
    /// line; the caret sits after the inserted text.
    #[test]
    fn paste_inserts_and_flattens_newlines() {
        let mut driver = driver();
        let mut state = State {
            name: "Hi".into(),
            ..State::default()
        };
        render(&mut driver, &state);
        assert_eq!(
            driver.event(key(KeyCode::Home), &state),
            EventResult::Consumed
        );

        let EventResult::Emit(msg) = driver.event(Event::Paste("x\ny".into()), &state) else {
            panic!("Paste must edit");
        };
        apply(&mut state, msg);
        assert_eq!(state.name, "x yHi");
    }

    /// A click places the caret in the well; typing then inserts there, not
    /// at the end. The press itself is ignored so the runtime can still move
    /// focus onto this field, the same way a checkbox click works.
    #[test]
    fn a_click_places_the_caret() {
        let mut driver = driver();
        let mut state = State {
            name: "ABC".into(),
            ..State::default()
        };
        render(&mut driver, &state);

        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 4, 1), &state),
            EventResult::Consumed
        );
        type_char(&mut driver, &mut state, 'x');
        assert_eq!(state.name, "AxBC");
    }

    /// A press on a second field moves focus there. The runtime only does
    /// that when the `Down` is ignored; consuming it would trap Tab-only
    /// navigation.
    #[test]
    fn a_press_moves_focus_to_the_clicked_field() {
        let mut driver = driver();
        let state = State {
            name: "Hi".into(),
            ..State::default()
        };
        driver.render(&state, |ctx| {
            ctx.component(ChildId::Static("name"), field(), Rect::new(2, 0, 16, 1));
            ctx.component(
                ChildId::Static("other"),
                Input::new().value(|s: &State| s.name.as_str(), Msg::Name),
                Rect::new(2, 2, 16, 1),
            );
        });

        let EventResult::Emit(Msg::Focus(focus)) =
            driver.event(mouse(MouseKind::Down(MouseButton::Left), 4, 2), &state)
        else {
            panic!("a press on the second field must move focus");
        };
        assert_eq!(focus, FocusState::intent(["other"]));
    }

    /// Tab, Enter, Esc, and modified keys belong to the app. The field is
    /// for typing, not for committing or dismissing.
    #[test]
    fn traversal_commit_and_modified_keys_pass_through() {
        let mut driver = driver();
        let state = State::default();
        render(&mut driver, &state);

        for code in [KeyCode::Enter, KeyCode::Esc] {
            assert_eq!(
                driver.event(key(code), &state),
                EventResult::Ignored,
                "{code:?} belongs to the app"
            );
        }
        assert!(
            !matches!(
                driver.event(key(KeyCode::Tab), &state),
                EventResult::Emit(Msg::Name(_))
            ),
            "Tab is traversal, not typing"
        );
        assert_eq!(
            driver.event(
                crate::test_support::key_with(
                    KeyCode::Char('s'),
                    crate::runtime::Modifiers {
                        ctrl: true,
                        ..crate::runtime::Modifiers::NONE
                    },
                ),
                &state,
            ),
            EventResult::Ignored,
            "Ctrl+S is an app hotkey"
        );
        assert_eq!(
            driver.event(
                crate::test_support::key_with(
                    KeyCode::Left,
                    crate::runtime::Modifiers {
                        shift: true,
                        ..crate::runtime::Modifiers::NONE
                    },
                ),
                &state,
            ),
            EventResult::Ignored,
            "Shift is never navigation"
        );
    }

    /// Disabled is the loudest state: no events, no traversal, muted well.
    #[test]
    fn a_disabled_input_is_inert_and_paints_muted() {
        let mut driver = driver();
        let state = State::default();
        driver.render(&state, |ctx| {
            ctx.component(
                ChildId::Static("name"),
                field().disabled(true),
                Rect::new(2, 1, 16, 1),
            );
        });

        assert_eq!(
            driver.event(key(KeyCode::Char('a')), &state),
            EventResult::Ignored
        );
        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 4, 1), &state),
            EventResult::Ignored
        );
        assert_eq!(
            driver.event(key(KeyCode::Tab), &state),
            EventResult::Ignored
        );

        let theme = Theme::default_dark();
        let muted = InputStyle::from_theme(&theme).disabled_background;
        assert_eq!(driver.cell(4, 1).bg, muted);
    }

    #[test]
    fn an_unbound_input_is_not_focusable_and_answers_nothing() {
        let mut driver = driver();
        let state = State::default();
        driver.render(&state, |ctx| {
            ctx.component(
                ChildId::Static("name"),
                Input::<State, Msg>::new(),
                Rect::new(2, 1, 16, 1),
            );
        });

        assert_eq!(
            driver.event(key(KeyCode::Tab), &state),
            EventResult::Ignored
        );
        assert_eq!(
            driver.event(key(KeyCode::Char('a')), &state),
            EventResult::Ignored
        );
    }

    /// The well is one row. A taller declaration must not fill cells the
    /// field does not answer on.
    #[test]
    fn the_well_covers_only_the_row_the_input_answers_on() {
        let area = Rect::new(0, 0, 12, 3);
        let theme = Theme::default_dark();
        let mut buffer = Buffer::empty(area);
        InputWidget::new("Hi")
            .focused(true)
            .themed(&theme)
            .render(area, &mut buffer);

        let well = InputStyle::from_theme(&theme).focused_background;
        assert_eq!(buffer.cell((1, 0)).expect("cell").bg, well);
        for row in 1..3u16 {
            assert_eq!(
                buffer.cell((1, row)).expect("cell").bg,
                Color::Reset,
                "row {row} filled beyond the input's one row"
            );
        }
    }

    fn value_row(buffer: &Buffer, width: u16) -> String {
        (0..width)
            .map(|column| buffer.cell((column, 0)).expect("cell").symbol().to_owned())
            .collect()
    }

    fn caret_columns(buffer: &Buffer, width: u16) -> Vec<u16> {
        (0..width)
            .filter(|&column| buffer.cell((column, 0)).expect("cell").symbol() == CARET)
            .collect()
    }

    /// The bar overlays a character without taking a column, so moving it
    /// never shifts the letters. Off, the letters are back as they were.
    #[test]
    fn moving_the_caret_does_not_shift_the_letters() {
        let area = Rect::new(0, 0, 8, 1);
        let theme = Theme::default_dark();

        let mut rest = Buffer::empty(area);
        InputWidget::new("Hi")
            .themed(&theme)
            .render(area, &mut rest);
        let letters = value_row(&rest, 8);

        let mut at_h = Buffer::empty(area);
        InputWidget::new("Hi")
            .focused(true)
            .caret(Some(0))
            .themed(&theme)
            .render(area, &mut at_h);
        assert_eq!(caret_columns(&at_h, 8), vec![1], "bar on H");

        let mut at_i = Buffer::empty(area);
        InputWidget::new("Hi")
            .focused(true)
            .caret(Some(1))
            .themed(&theme)
            .render(area, &mut at_i);
        assert_eq!(caret_columns(&at_i, 8), vec![2], "bar on i");

        let mut at_end = Buffer::empty(area);
        InputWidget::new("Hi")
            .focused(true)
            .caret(Some(2))
            .themed(&theme)
            .render(area, &mut at_end);
        assert_eq!(
            caret_columns(&at_end, 8),
            vec![3],
            "bar after the last letter"
        );

        let mut off = Buffer::empty(area);
        InputWidget::new("Hi")
            .focused(true)
            .caret(Some(1))
            .caret_on(false)
            .themed(&theme)
            .render(area, &mut off);
        assert_eq!(
            value_row(&off, 8),
            letters,
            "the off half-period restores the letters in place"
        );
    }

    /// A value wider than the well still keeps the caret on screen: typing
    /// at the end must not paint a bar that has scrolled off the right.
    #[test]
    fn a_long_value_keeps_the_caret_visible() {
        let area = Rect::new(0, 0, 6, 1);
        let theme = Theme::default_dark();
        let mut buffer = Buffer::empty(area);
        InputWidget::new("abcdefgh")
            .focused(true)
            .caret(Some(8))
            .themed(&theme)
            .render(area, &mut buffer);

        assert_eq!(
            caret_columns(&buffer, 6),
            vec![4],
            "the caret stays in the well after the last visible character"
        );
    }
}
