//! Tooltips on a row of buttons: hover or Tab to one and its bubble floats
//! beside it.
//!
//! Two things are on show. First, `open_when` combines the hover the runtime
//! keeps with the focus path the app keeps, counting focus only while the
//! keyboard is what is driving — so a tooltip appears on hover *and* on Tab,
//! and goes away again when the pointer or focus moves on. Nothing is stored
//! for the bubble itself and nothing in `update` writes it. Second, placement:
//! each button in the row is named for the side its bubble prefers, and the
//! one pinned to the top row of the demo's area has no room above it, so its
//! bubble flips below.

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Rect},
    style::Style,
};
use ratcn::{
    Button, Theme, Tooltip, TooltipSide,
    runtime::{Event, EventResult, FocusState, KeyCode, Ratcn, TabWrap},
};

/// The child id every Tooltip gives the button inside it. Unique among its own
/// siblings, which is all an id has to be.
const TRIGGER: &str = "button";

mod ids {
    pub const LEFT: &str = "left";
    pub const TOP: &str = "top";
    pub const BOTTOM: &str = "bottom";
    pub const RIGHT: &str = "right";
    pub const EDGE: &str = "edge";
}

/// What every button in the row explains. Only the edge one differs, because
/// it is there to show the bubble flipping rather than the text.
const TIP: &str = "Hi!";

/// One explained button: the Tooltip's id, the button's label, the explanation,
/// and the side the bubble prefers.
type Explained = (&'static str, &'static str, &'static str, TooltipSide);

/// The centered row. Each button is labelled with the side its bubble prefers.
const ROW: [Explained; 4] = [
    (ids::LEFT, "Left", TIP, TooltipSide::Left),
    (ids::TOP, "Top", TIP, TooltipSide::Top),
    (ids::BOTTOM, "Bottom", TIP, TooltipSide::Bottom),
    (ids::RIGHT, "Right", TIP, TooltipSide::Right),
];

/// Pinned to the top row of the demo's area, so `Top` has nowhere to go and
/// flips.
const EDGE: Explained = (
    ids::EDGE,
    "Edge",
    "This one prefers the top, but there is no room above it, so the bubble flips below",
    TooltipSide::Top,
);

#[derive(Default)]
struct AppState {
    focus: FocusState,
    /// Whether keyboard input drives focus tooltips. Esc preserves this mode.
    ///
    /// A click focuses what it hits, so focus alone cannot say whether the
    /// user is navigating by keyboard: pairing it with this is what keeps a
    /// bubble from lingering after a click, the same distinction the web
    /// draws between `:focus` and `:focus-visible`. The app owns it because
    /// the app sees every event.
    keyboard: bool,
}

#[derive(Clone)]
enum Msg {
    Focus(FocusState),
    Pressed,
}

pub struct App {
    state: AppState,
    ratcn: Ratcn<AppState, Msg>,
}

/// The button a Tooltip explains. Built twice per frame — once to measure, once
/// to declare — so its width and its area cannot disagree.
fn button(label: &'static str) -> Button<Msg> {
    Button::new(label).on_press(|| Msg::Pressed)
}

/// Wrap `label`'s button in the tooltip that explains it.
///
/// `id` is the Tooltip's own id at the root, which makes it the first element
/// of the focus path of everything inside it — so a prefix query answers "is
/// the keyboard on this button?"
fn explained((id, label, tip, side): Explained) -> Tooltip<AppState, Msg> {
    Tooltip::new(tip)
        .side(side)
        // The hover half is the runtime's answer, handed to the reader; only
        // the keyboard half is the app's. Focus counts only while the keyboard
        // is driving: a click focuses what it hits, and a bubble that outlived
        // the click would sit there until focus moved on.
        .open_when(move |state: &AppState, hovered| {
            hovered || (state.keyboard && state.focus.contains_path([id]))
        })
        .trigger(move |ctx| {
            let area = ctx.area();
            ctx.component(TRIGGER, button(label), area);
        })
}

impl App {
    pub fn new() -> Self {
        Self {
            state: AppState::default(),
            ratcn: Ratcn::new()
                .focus(|state: &AppState| &state.focus, Msg::Focus)
                .tab_wrap(TabWrap::Wrap),
        }
    }

    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Focus(focus) => self.state.focus = focus,
            Msg::Pressed => {}
        }
    }
}

impl demo_shared::Demo for App {
    fn handle_event(&mut self, event: Event) -> bool {
        // Esc preserves input mode and remains available to an enclosing host.
        let keyboard = match &event {
            Event::Mouse(_) => false,
            Event::Key(key) if key.code == KeyCode::Esc => self.state.keyboard,
            _ => true,
        };
        // Switching device changes which bubbles are open, so it needs a frame
        // in its own right: a key the runtime ignores still reveals the bubble
        // on the focused trigger.
        let switched = self.state.keyboard != keyboard;
        self.state.keyboard = keyboard;
        let routed = match self.ratcn.handle_event(event, &self.state) {
            EventResult::Emit(msg) => {
                self.update(msg);
                true
            }
            EventResult::Consumed => true,
            EventResult::Ignored => false,
        };
        switched || routed
    }

    fn draw(&mut self, buffer: &mut Buffer, area: Rect, theme: &Theme) {
        buffer.set_style(area, Style::default().bg(theme.background));

        let [edge_area, rest_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);
        let [row_area] = Layout::vertical([Constraint::Length(1)])
            .flex(Flex::Center)
            .areas(rest_area);

        let state = &self.state;
        self.ratcn.render_into(buffer, area, state, theme, |ctx| {
            let [edge_button_area] =
                Layout::horizontal([Constraint::Length(button(EDGE.1).width())])
                    .flex(Flex::Center)
                    .areas(edge_area);
            ctx.component(EDGE.0, explained(EDGE), edge_button_area);

            let areas: [_; ROW.len()] = Layout::horizontal(
                ROW.map(|(_, label, _, _)| Constraint::Length(button(label).width())),
            )
            .flex(Flex::Center)
            .spacing(2)
            .areas(row_area);
            for (entry, button_area) in ROW.into_iter().zip(areas) {
                ctx.component(entry.0, explained(entry), button_area);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use demo_shared::Demo as _;
    use ratcn::runtime::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseKind};

    use super::*;

    fn typed() -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char('x')))
    }

    fn clicked() -> Event {
        Event::Mouse(MouseEvent {
            kind: MouseKind::Click(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: Default::default(),
        })
    }

    /// The device switch opens and closes bubbles by itself, so it has to ask
    /// for the frame that shows the change — the runtime routing the event is a
    /// separate question, and a key nothing reacts to still switches the device.
    #[test]
    fn switching_input_device_needs_a_frame_of_its_own() {
        let mut app = App::new();

        assert!(
            app.handle_event(typed()),
            "mouse to keyboard reveals the bubble on the focused trigger"
        );
        assert!(app.state.keyboard);
        assert!(
            !app.handle_event(typed()),
            "a second key nothing reacts to changes nothing"
        );
        assert!(
            app.handle_event(clicked()),
            "keyboard to mouse hides it again"
        );
        assert!(!app.state.keyboard);
    }

    #[test]
    fn ignored_escape_preserves_the_standalone_hover_and_needs_no_repaint() {
        let mut app = App::new();
        let area = Rect::new(0, 0, 80, 20);
        let theme = Theme::default_dark();
        let mut before = Buffer::empty(area);
        app.draw(&mut before, area, &theme);
        assert!(app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseKind::Moved,
            column: 40,
            row: 0,
            modifiers: Default::default(),
        })));
        before.reset();
        app.draw(&mut before, area, &theme);
        let text: String = before.content.iter().map(|cell| cell.symbol()).collect();
        assert!(
            text.contains("This one"),
            "hover must paint the edge tooltip before Esc"
        );
        assert!(!app.handle_event(Event::Key(KeyEvent::new(KeyCode::Esc))));
        assert!(!app.state.keyboard);
        let mut after = Buffer::empty(area);
        app.draw(&mut after, area, &theme);
        assert_eq!(
            after, before,
            "ignoring Esc must not silently change tooltip paint"
        );
    }
}
