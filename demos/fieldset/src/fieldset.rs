//! `Fieldset`: a labeled group box with a caller-supplied body, a measured
//! action beside its label, and a collapse the app owns.
//!
//! This is the worked example behind the composite walkthrough in
//! `docs/docs/concepts/building-a-composite.md`, and that page quotes this file
//! region by region rather than paraphrasing it. It lives in a demo crate
//! rather than in `ratcn` on purpose: a composite of your own is an ordinary
//! `Component`, so this one is written outside the library, against its
//! published API only.

use ratatui::{
    layout::{Constraint, Layout, Position, Rect, Size},
    style::{Modifier, Style},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
};
use ratcn::runtime::{
    ChildId, Component, DeclareCtx, Event, EventCtx, EventResult, KeyCode, MeasuredComponent,
    MouseButton, MouseKind, PaintCtx, ScopeOptions,
};

/// Cells of chrome on each side of the box: the one-cell border plus one cell
/// of padding inside it.
const EDGE_X: u16 = 2;
/// Rows of chrome above and below the box: the border alone.
const EDGE_Y: u16 = 1;
/// The row that carries the marker, the label, and the action.
const HEADER_HEIGHT: u16 = 1;
/// Cells between the label and the action beside it.
const LABEL_GAP: u16 = 2;
const COLLAPSED_MARKER: &str = "\u{25b8} ";
const EXPANDED_MARKER: &str = "\u{25be} ";

/// A body region the caller fills, boxed until `declare` hands it its strip.
type BodyFn<S, M> = Box<dyn FnOnce(&mut DeclareCtx<'_, S, M>)>;
/// The action's declaration, boxed with the component and the id it carries.
type ActionFn<S, M> = Box<dyn FnOnce(&mut DeclareCtx<'_, S, M>, Rect)>;
/// Reads the collapsed flag out of app state, at the moment it is asked.
type CollapsedFn<S> = Box<dyn Fn(&S) -> bool>;
/// Builds the message that asks the app to collapse or expand.
type OnToggleFn<M> = Box<dyn Fn(bool) -> M>;

// #region facts
/// Everything the fieldset's geometry is made of.
///
/// These are the layout *facts*, kept apart from the closures they describe,
/// because `declare` takes those closures and `handle_event` still has to
/// re-derive the same rects a frame later.
struct Facts {
    /// Pinned in [`Component::prepare`], because `interaction_area` is asked
    /// for the box with no state to read the binding from.
    collapsed: bool,
    /// Rows the caller asked for, whether or not the body closure is still
    /// there to fill them.
    body_height: u16,
    /// The action's size, measured when it was handed over.
    action: Size,
    disabled: bool,
}

struct FieldsetLayout {
    box_area: Rect,
    header_area: Rect,
    label_area: Rect,
    action_area: Rect,
    body_area: Rect,
}

/// The one geometry function. `declare`, `paint`, `interaction_area`, and
/// `handle_event` all derive their rects here, from the same facts, and `height`
/// answers from the same two helpers below — so no two of them can disagree
/// about where the header is or how tall the box gets.
fn layout(area: Rect, facts: &Facts) -> FieldsetLayout {
    let height = box_height(facts).min(area.height);
    let box_area = Rect { height, ..area };
    if box_area.is_empty() {
        return FieldsetLayout {
            box_area: Rect::ZERO,
            header_area: Rect::ZERO,
            label_area: Rect::ZERO,
            action_area: Rect::ZERO,
            body_area: Rect::ZERO,
        };
    }
    // Inset with a plain block: `inner` depends only on borders and padding, so
    // the styled block `paint` builds from the theme cannot move the layout.
    let inner = Block::bordered()
        .padding(Padding::horizontal(EDGE_X - 1))
        .inner(box_area);
    let [header_area, body_area] =
        Layout::vertical([Constraint::Length(header_height(facts)), Constraint::Min(0)])
            .areas(inner);
    let action_width = facts.action.width.min(header_area.width);
    let action_area = Rect {
        x: header_area.right() - action_width,
        width: action_width,
        height: facts.action.height.min(header_area.height),
        ..header_area
    };
    let label_area = Rect {
        width: header_area
            .width
            .saturating_sub(action_width)
            .saturating_sub(if action_width > 0 { LABEL_GAP } else { 0 }),
        ..header_area
    };
    FieldsetLayout {
        box_area,
        header_area,
        label_area,
        action_area,
        body_area,
    }
}

/// The header is as tall as the tallest thing in it, which is why the action is
/// measured rather than assumed to be one row.
fn header_height(facts: &Facts) -> u16 {
    HEADER_HEIGHT.max(facts.action.height)
}

fn box_height(facts: &Facts) -> u16 {
    let body = if facts.collapsed {
        0
    } else {
        facts.body_height
    };
    header_height(facts)
        .saturating_add(body)
        .saturating_add(EDGE_Y * 2)
}
// #endregion facts

// #region struct
/// A labeled group box: a border, a header row carrying the label and one
/// optional action, and a body the caller declares into.
///
/// The collapse is the app's. `Fieldset` reads it and asks for changes; it
/// stores nothing about whether it is open.
pub struct Fieldset<S, M> {
    label: String,
    /// Read whenever the answer is needed, not copied in at declaration: see
    /// `prepare` and `handle_event`.
    collapsed: Option<CollapsedFn<S>>,
    on_toggle: Option<OnToggleFn<M>>,
    disabled: bool,
    body_height: u16,
    body: Option<BodyFn<S, M>>,
    action_size: Size,
    action: Option<ActionFn<S, M>>,
    /// The collapsed flag as this declaration saw it, pinned by `prepare`.
    collapsed_now: bool,
    /// The paint allocation, retained because `interaction_area` narrows what
    /// `handle_event` is told the area was.
    paint_area: Rect,
}
// #endregion struct

impl<S: 'static, M: 'static> Fieldset<S, M> {
    /// A fieldset with `label` in its header, no body, and no collapse.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            collapsed: None,
            on_toggle: None,
            disabled: false,
            body_height: 0,
            body: None,
            action_size: Size::ZERO,
            action: None,
            collapsed_now: false,
            paint_area: Rect::ZERO,
        }
    }

    // #region body
    /// Fill the body yourself: `height` rows, declared by `body`.
    ///
    /// The callback gets an ordinary [`DeclareCtx`] over the body strip — paint
    /// into it, declare components into it. Those components are the
    /// fieldset's own children, in the same sibling namespace as the
    /// [`action`](Self::action), so their ids must not collide with it.
    ///
    /// The closure is `FnOnce` so it can consume what the caller moves in, and
    /// it is stored until `declare`, so it may only capture `'static` values.
    pub fn body(
        mut self,
        height: u16,
        body: impl FnOnce(&mut DeclareCtx<'_, S, M>) + 'static,
    ) -> Self {
        self.body_height = height;
        self.body = Some(Box::new(body));
        self
    }
    // #endregion body

    // #region action
    /// Put one measured component in the header row, end-aligned beside the
    /// label.
    ///
    /// The size is taken here, while the component is still in hand: `declare`
    /// has to know how wide the action is *before* it declares it, and by then
    /// the component is inside the closure below.
    pub fn action(
        mut self,
        id: impl Into<ChildId>,
        action: impl MeasuredComponent<S, M> + 'static,
    ) -> Self {
        let id = id.into();
        self.action_size = action.measure();
        self.action = Some(Box::new(move |ctx, area| {
            ctx.component(id, action, area);
        }));
        self
    }
    // #endregion action

    // #region collapsed
    /// Read whether the fieldset is collapsed out of app state.
    ///
    /// A binding rather than a `bool`, because a collapse composes on the
    /// answer as it is *now*: two toggle keys can arrive before the next frame
    /// is drawn, and the second must undo the first rather than repeat it.
    pub fn collapsed(mut self, collapsed: impl Fn(&S) -> bool + 'static) -> Self {
        self.collapsed = Some(Box::new(collapsed));
        self
    }

    /// Let the user collapse and expand the fieldset, and say what to emit when
    /// they ask. The argument is the collapsed state being asked for.
    ///
    /// Wiring this is also what makes the fieldset itself focusable, so the
    /// toggle key has somewhere to land when there is nothing focusable inside
    /// — a collapsed group with no action of its own, say.
    pub fn on_toggle(mut self, on_toggle: impl Fn(bool) -> M + 'static) -> Self {
        self.on_toggle = Some(Box::new(on_toggle));
        self
    }
    // #endregion collapsed

    // #region disabled
    /// Dim the whole group and take it out of interaction.
    ///
    /// For a section that does not apply yet. The dimming covers the body's
    /// components, which is why it cannot be painted from `paint`, and the
    /// interaction area goes empty, which excludes the descendants without
    /// removing them.
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    // #endregion disabled

    // #region height
    /// The rows this fieldset needs, for a caller stacking several of them in a
    /// column.
    ///
    /// [`MeasuredComponent`] cannot answer this. Its `measure` takes no state,
    /// because a container asks it while pushing the child — before `prepare`,
    /// and with nothing to read the collapse out of. So the question is a method
    /// that takes what it depends on, answered by the same arithmetic `layout`
    /// uses rather than a second copy of it.
    pub fn height(&self, state: &S) -> u16 {
        let collapsed = self.is_collapsed(state);
        box_height(&Facts {
            collapsed,
            ..self.facts()
        })
    }
    // #endregion height

    // #region mapping
    fn is_collapsed(&self, state: &S) -> bool {
        self.collapsed.as_ref().is_some_and(|read| read(state))
    }

    /// The facts this declaration's geometry is derived from — the half of the
    /// fields that taking a closure does not empty.
    fn facts(&self) -> Facts {
        Facts {
            collapsed: self.collapsed_now,
            body_height: self.body_height,
            action: self.action_size,
            disabled: self.disabled,
        }
    }
    // #endregion mapping

    fn marker(&self, collapsed: bool) -> &'static str {
        match (self.on_toggle.is_some(), collapsed) {
            (true, true) => COLLAPSED_MARKER,
            (true, false) => EXPANDED_MARKER,
            (false, _) => "",
        }
    }
}

// #region impl-declare
impl<S: 'static, M: 'static> Component<S, M> for Fieldset<S, M> {
    /// Pin the collapsed flag for this declaration: `interaction_area` is asked
    /// for the box with no state in hand, and `paint` has to agree with the box
    /// `declare` laid out even if the app has moved on since.
    fn prepare(&mut self, state: &S) {
        self.collapsed_now = self.is_collapsed(state);
    }

    fn declare(&mut self, ctx: &mut DeclareCtx<'_, S, M>) {
        let area = ctx.area();
        // Retained for `handle_event`: the runtime narrows this component's
        // event area to the box, and the geometry function wants the whole
        // allocation.
        self.paint_area = area;
        let layout = layout(area, &self.facts());

        // Declaration order is Tab order, and here it is also reading order:
        // the header's action, then the body under it.
        if let Some(action) = self.action.take() {
            action(ctx, layout.action_area);
        }
        if !self.collapsed_now
            && let Some(body) = self.body.take()
        {
            ctx.in_area(layout.body_area, body);
        }

        if self.disabled {
            // A wash over the body's own components cannot come from
            // `Component::paint`, which is queued where this declaration
            // *opened* — under everything declared inside it. Queueing a paint
            // closure here, after those declarations, puts it after them in the
            // same queue, on the same layer.
            let box_area = layout.box_area;
            ctx.paint(move |ctx| {
                let dim = Style::default().fg(ctx.theme.muted_foreground);
                ctx.with_buffer(|buffer| buffer.set_style(box_area, dim));
            });
        }
    }
    // #endregion impl-declare

    // #region impl-paint
    fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, S>) {
        let facts = self.facts();
        let layout = layout(ctx.area(), &facts);
        if layout.box_area.is_empty() {
            return;
        }
        // `contains_focus`, not `focused`: the group is the thing that holds the
        // focus, whether it sits on the fieldset itself or on a control inside
        // the body.
        let border_color = if ctx.contains_focus {
            ctx.theme.ring
        } else {
            ctx.theme.border
        };
        ctx.widget(
            Block::bordered()
                .border_set(border::ROUNDED)
                .border_style(Style::default().fg(border_color)),
            layout.box_area,
        );
        // The marker is the toggle's affordance, so it follows the pointer on
        // the header itself — `hovered`, not `contains_hover`: the pointer being
        // on the action button is not the pointer being on the header.
        let marker_color = if ctx.focused {
            ctx.theme.ring
        } else if ctx.hovered {
            ctx.theme.accent
        } else {
            ctx.theme.muted_foreground
        };
        let label = Line::from(vec![
            Span::styled(
                self.marker(facts.collapsed),
                Style::default().fg(marker_color),
            ),
            Span::styled(
                self.label.clone(),
                Style::default()
                    .fg(ctx.theme.foreground)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
        ctx.widget(Paragraph::new(label), layout.label_area);
    }
    // #endregion impl-paint

    // #region impl-interaction
    fn scope_options(&self) -> ScopeOptions {
        // Focus prefers a focusable descendant, so this only makes the fieldset
        // itself a Tab stop when there is nothing inside to focus — the case
        // where the toggle key would otherwise have nowhere to land. Tab
        // wrapping is left alone: a fieldset is a group in a page, not a trap
        // like a dialog.
        if self.on_toggle.is_some() {
            ScopeOptions::default().focusable()
        } else {
            ScopeOptions::default()
        }
    }

    fn interaction_area(&self, area: Rect) -> Rect {
        let facts = self.facts();
        if facts.disabled {
            // Identity and paint stay; the fieldset and everything in it drop
            // out of focus, hit-testing, and event routing.
            return Rect::ZERO;
        }
        // The box is often shorter than the allocation — collapsed, or handed a
        // slot that fills the remaining space. Rows the fieldset never painted
        // must not take clicks.
        layout(area, &facts).box_area
    }

    /// `_ctx` is unused on purpose: [`EventCtx::area`] reports the *narrowed*
    /// interaction area, and this fieldset derives its rects from the whole
    /// allocation it retained. Reach for `EventCtx::area` in a composite that
    /// does not override `interaction_area` — then it is the same rect, without
    /// the field.
    fn handle_event(
        &mut self,
        event: &Event,
        state: &S,
        _ctx: &mut EventCtx<'_>,
    ) -> EventResult<M> {
        let Some(on_toggle) = &self.on_toggle else {
            return EventResult::Ignored;
        };
        // Read the binding against current state. The frame this instance was
        // declared from may already be out of date: an earlier key in the same
        // batch may have collapsed it.
        let collapsed = self.is_collapsed(state);
        let asked = match event {
            Event::Key(key) if !key.modifiers.any() => match key.code {
                KeyCode::Enter | KeyCode::Char(' ') => Some(!collapsed),
                // The accordion idiom, and it works from inside: a Left the
                // body's own controls declined bubbles up to here.
                KeyCode::Left if !collapsed => Some(true),
                KeyCode::Right if collapsed => Some(false),
                _ => None,
            },
            Event::Mouse(mouse) if matches!(mouse.kind, MouseKind::Click(MouseButton::Left)) => {
                // Event-time geometry, from the retained facts, through the one
                // function that placed the header in the first place.
                let header = layout(self.paint_area, &self.facts()).header_area;
                header
                    .contains(Position::new(mouse.column, mouse.row))
                    .then_some(!collapsed)
            }
            _ => None,
        };
        // Everything else keeps bubbling: Tab, the app's own hotkeys, and a
        // Left or Right this fieldset has no answer for.
        asked.map_or(EventResult::Ignored, |collapsed| {
            EventResult::Emit(on_toggle(collapsed))
        })
    }
    // #endregion impl-interaction
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};
    use ratcn::{
        Button, ButtonSize, Theme,
        runtime::{FocusState, KeyEvent, Modifiers, MouseEvent, Ratcn},
    };

    use super::*;

    const AREA: Rect = Rect::new(0, 0, 40, 12);
    const BODY_HEIGHT: u16 = 2;

    #[derive(Default)]
    struct State {
        focus: FocusState,
        collapsed: bool,
    }

    #[derive(Debug, Clone, PartialEq)]
    enum Msg {
        Focus(FocusState),
        Collapse(bool),
        Pressed,
        BodyPressed,
    }

    /// One fieldset, wired the way the demo wires it: a measured action in the
    /// header and a focusable button in the body. The action's size is the
    /// header's size — a large button is three rows tall.
    fn group(action: ButtonSize) -> Fieldset<State, Msg> {
        Fieldset::new("Notifications")
            .collapsed(|state: &State| state.collapsed)
            .on_toggle(Msg::Collapse)
            .action(
                "mute",
                Button::new("Mute").size(action).on_press(|| Msg::Pressed),
            )
            .body(BODY_HEIGHT, |ctx| {
                let area = ctx.area();
                ctx.component(
                    "email",
                    Button::new("Email").on_press(|| Msg::BodyPressed),
                    Rect { height: 1, ..area },
                );
            })
    }

    fn fieldset() -> Fieldset<State, Msg> {
        group(ButtonSize::Small)
    }

    /// A group with nothing focusable in it — no action, and a body that only
    /// paints. This is the shape in which the fieldset itself is the Tab stop.
    fn plain() -> Fieldset<State, Msg> {
        Fieldset::new("Notifications")
            .collapsed(|state: &State| state.collapsed)
            .on_toggle(Msg::Collapse)
            .body(BODY_HEIGHT, |ctx| {
                let area = ctx.area();
                ctx.paint_widget(Paragraph::new("email, push"), area);
            })
    }

    fn draw_group(
        ratcn: &mut Ratcn<State, Msg>,
        terminal: &mut Terminal<TestBackend>,
        state: &State,
        group: Fieldset<State, Msg>,
    ) {
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                ratcn.render(frame, state, &theme, |ctx| {
                    ctx.component("group", group, AREA);
                });
            })
            .expect("draw");
    }

    fn draw(ratcn: &mut Ratcn<State, Msg>, terminal: &mut Terminal<TestBackend>, state: &State) {
        draw_group(ratcn, terminal, state, fieldset());
    }

    fn app() -> (Ratcn<State, Msg>, Terminal<TestBackend>) {
        (
            Ratcn::new().focus(|state: &State| &state.focus, Msg::Focus),
            Terminal::new(TestBackend::new(AREA.width, AREA.height)).expect("terminal"),
        )
    }

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code))
    }

    fn click(column: u16, row: u16) -> [Event; 3] {
        [
            MouseKind::Down(MouseButton::Left),
            MouseKind::Up(MouseButton::Left),
            MouseKind::Click(MouseButton::Left),
        ]
        .map(|kind| {
            Event::Mouse(MouseEvent {
                kind,
                column,
                row,
                modifiers: Modifiers::NONE,
            })
        })
    }

    fn rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|row| {
                (0..buffer.area.width)
                    .map(|column| buffer.cell((column, row)).expect("cell").symbol())
                    .collect()
            })
            .collect()
    }

    /// The reason `collapsed` is a binding and not a `bool`. Two keys can reach
    /// the retained instance before the next frame is drawn; the second has to
    /// compose on what the first asked for, not on what the frame showed.
    #[test]
    fn two_toggles_without_a_frame_between_them_expand_and_then_collapse() {
        let (mut ratcn, mut terminal) = app();
        let mut state = State {
            collapsed: true,
            ..State::default()
        };
        draw_group(&mut ratcn, &mut terminal, &state, plain());

        assert_eq!(
            ratcn.handle_event(key(KeyCode::Enter), &state),
            EventResult::Emit(Msg::Collapse(false))
        );
        // The app applied it. No frame has been drawn since.
        state.collapsed = false;
        assert_eq!(
            ratcn.handle_event(key(KeyCode::Enter), &state),
            EventResult::Emit(Msg::Collapse(true)),
            "the second key read the state the first one produced"
        );
    }

    /// A group with nothing focusable inside it still has to be reachable, or a
    /// collapsed section would be a keyboard dead end. That is what
    /// `scope_options().focusable()` buys, and it costs nothing when there *is*
    /// something inside: focus prefers the descendant.
    #[test]
    fn a_group_with_nothing_focusable_inside_is_the_focus_target_itself() {
        let (mut ratcn, mut terminal) = app();
        let state = State {
            collapsed: true,
            ..State::default()
        };

        draw_group(&mut ratcn, &mut terminal, &state, plain());
        ratcn.handle_event(key(KeyCode::Tab), &state);
        assert_eq!(
            ratcn.handle_event(key(KeyCode::Right), &state),
            EventResult::Emit(Msg::Collapse(false))
        );

        draw(&mut ratcn, &mut terminal, &state);
        assert_eq!(
            ratcn.handle_event(key(KeyCode::Left), &state),
            EventResult::Ignored,
            "already collapsed: nothing to do, and an ancestor may want the key"
        );
    }

    /// The accordion idiom has to work from inside the group, which it does for
    /// free — the body's button declines Left and it bubbles to the fieldset.
    #[test]
    fn left_collapses_the_group_from_a_control_inside_its_body() {
        let (mut ratcn, mut terminal) = app();
        let state = State {
            focus: FocusState::intent([ChildId::Static("group"), ChildId::Static("email")]),
            collapsed: false,
        };
        draw(&mut ratcn, &mut terminal, &state);

        assert_eq!(
            ratcn.handle_event(key(KeyCode::Left), &state),
            EventResult::Emit(Msg::Collapse(true))
        );
        assert_eq!(
            ratcn.handle_event(key(KeyCode::Enter), &state),
            EventResult::Emit(Msg::BodyPressed),
            "keys the body does handle still stop there"
        );
    }

    /// The header is a hit target derived at event time from the retained
    /// facts. The rows below a collapsed box were never painted, so they belong
    /// to whatever is underneath, not to the fieldset.
    #[test]
    fn the_header_toggles_on_click_and_the_rows_below_a_collapsed_box_do_not() {
        let (mut ratcn, mut terminal) = app();
        let state = State {
            collapsed: true,
            ..State::default()
        };
        draw(&mut ratcn, &mut terminal, &state);
        let mut group = fieldset();
        group.prepare(&state);
        let layout = layout(AREA, &group.facts());

        let clicked = click(layout.label_area.x, layout.label_area.y)
            .into_iter()
            .map(|event| ratcn.handle_event(event, &state))
            .next_back()
            .expect("a click ends in a Click");
        assert_eq!(clicked, EventResult::Emit(Msg::Collapse(false)));

        for event in click(layout.box_area.x, layout.box_area.bottom()) {
            assert_eq!(
                ratcn.handle_event(event, &state),
                EventResult::Ignored,
                "a click below the collapsed box is not the fieldset's"
            );
        }
    }

    /// `disabled` is not just a color: the section is inert, and that has to
    /// include the components the caller declared inside it.
    #[test]
    fn a_disabled_fieldset_ignores_events_meant_for_its_own_children() {
        let (mut ratcn, mut terminal) = app();
        let state = State::default();
        draw_group(&mut ratcn, &mut terminal, &state, fieldset().disabled(true));

        assert_eq!(
            ratcn.handle_event(key(KeyCode::Enter), &state),
            EventResult::Ignored,
            "nothing inside a disabled fieldset can be focused or pressed"
        );
        let rendered = rows(&terminal).concat();
        assert!(
            rendered.contains("Notifications"),
            "it is dimmed, not removed: {rendered}"
        );
    }

    /// The wash has to land on top of the body's own components, which is what
    /// `defer_paint` buys and what a `paint` implementation cannot do: paint is
    /// queued where the declaration opens, underneath everything inside it.
    #[test]
    fn the_disabled_wash_covers_what_the_body_declared() {
        let (mut ratcn, mut terminal) = app();
        let state = State::default();
        let muted = Theme::default_dark().muted_foreground;

        draw_group(&mut ratcn, &mut terminal, &state, fieldset().disabled(true));
        assert_eq!(
            body_button_color(&terminal),
            muted,
            "the button inside the body paints after the fieldset and before the wash"
        );

        draw_group(&mut ratcn, &mut terminal, &state, fieldset());
        assert_ne!(
            body_button_color(&terminal),
            muted,
            "and it keeps its own colors when the group is live"
        );
    }

    /// The foreground the body's button label was left with: the first `E` on
    /// screen belongs to its `Email`.
    fn body_button_color(terminal: &Terminal<TestBackend>) -> ratatui::style::Color {
        let buffer = terminal.backend().buffer();
        buffer
            .content()
            .iter()
            .find(|cell| cell.symbol() == "E")
            .expect("the body's button label")
            .fg
    }

    /// `height` and `layout` share their arithmetic, so a caller who allocates
    /// exactly what the fieldset asked for gets a box that fills it — collapsed
    /// or open, and whatever height the action turned out to measure. The large
    /// action is the case a hard-coded one-row header would get wrong.
    #[test]
    fn the_height_asked_for_is_the_height_the_box_lays_itself_out_to() {
        for (action, header) in [(ButtonSize::Small, 1_u16), (ButtonSize::Large, 3)] {
            for collapsed in [false, true] {
                let state = State {
                    collapsed,
                    ..State::default()
                };
                let mut fieldset = group(action);
                let asked = fieldset.height(&state);
                fieldset.prepare(&state);

                let layout = layout(AREA, &fieldset.facts());
                let case = format!("{action:?} action, collapsed: {collapsed}");

                assert_eq!(asked, layout.box_area.height, "{case}");
                assert_eq!(
                    layout.header_area.height, header,
                    "the header is as tall as the action it measured ({case})",
                );
                assert_eq!(
                    layout.box_area.height,
                    header + EDGE_Y * 2 + if collapsed { 0 } else { BODY_HEIGHT },
                    "border, header, and the body only when open ({case})",
                );
            }
        }
    }
}
