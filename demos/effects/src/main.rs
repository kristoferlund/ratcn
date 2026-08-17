//! Side effects with ratcn: fetch a dad joke without blocking the UI.
//!
//! The pattern: `update` is the only writer of app state and stays pure —
//! instead of doing I/O it *returns* an [`Effect`]. `execute` runs the effect
//! in the background (a thread natively, a spawned future on wasm), and the
//! result comes back as a [`Msg`] on an mpsc channel the app drains at the top
//! of every frame. The UI never waits on the network.

mod fetch;

use std::{
    borrow::Cow,
    io,
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Flex, Layout},
    style::{Color, Style},
    widgets::{Paragraph, Wrap},
};
use ratcn::{
    Button, ButtonSize, Theme,
    runtime::{Event, EventResult, FocusState, Ratcn, wrapped_height},
};

/// How often to look for a completion while a fetch is in flight. The channel
/// wakes nothing by itself, so the host has to come back and ask.
const POLL_INTERVAL: Duration = Duration::from_millis(50);
const CONTENT_WIDTH: u16 = 50;
const THEME: Theme = Theme::default_dark();

mod ids {
    pub const REFRESH: &str = "refresh";
}

struct App {
    state: AppState,
    ratcn: Ratcn<AppState, Msg>,
    /// Effects report back through this pair: `execute` gets a clone of the
    /// sender, and [`App::drain`] applies whatever has arrived.
    sender: Sender<Msg>,
    receiver: Receiver<Msg>,
}

#[derive(Default)]
struct AppState {
    focus: FocusState,
    joke: JokeState,
}

#[derive(Debug, Default, PartialEq, Eq)]
enum JokeState {
    #[default]
    Idle,
    Loading,
    Ready(String),
    Failed(String),
}

impl JokeState {
    const fn is_loading(&self) -> bool {
        matches!(self, Self::Loading)
    }
}

enum Msg {
    FocusChanged(FocusState),
    RefreshRequested,
    /// Sent from the background fetch when it finishes, success or not.
    JokeFetchCompleted(Result<String, String>),
}

/// Work that `update` wants done but must not do itself: `update` stays a pure
/// state transition, and the caller runs the effect outside it.
#[derive(Debug, PartialEq, Eq)]
enum Effect {
    FetchJoke,
}

impl App {
    fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            state: AppState::default(),
            ratcn: Ratcn::new().focus(|state: &AppState| &state.focus, Msg::FocusChanged),
            sender,
            receiver,
        }
    }

    /// Apply one message to app state, then start any effect it asked for.
    /// Every message goes through here, whether it came from a UI event or
    /// from a completed background fetch.
    fn dispatch(&mut self, msg: Msg) {
        if let Some(effect) = update(&mut self.state, msg) {
            execute(effect, self.sender.clone());
        }
    }

    /// Apply every message the background work has queued since the last
    /// frame. Returns whether anything changed, so the caller knows to redraw.
    fn drain(&mut self) -> bool {
        let mut dispatched = false;
        while let Ok(msg) = self.receiver.try_recv() {
            self.dispatch(msg);
            dispatched = true;
        }
        dispatched
    }
}

impl demo_shared::Demo for App {
    fn background(&self) -> Color {
        THEME.background
    }

    /// Route one input event through ratcn; a component that reacts emits a
    /// `Msg`, which feeds the same `dispatch` path as everything else.
    fn handle_event(&mut self, event: Event) -> bool {
        match self.ratcn.handle_event(event, &self.state) {
            EventResult::Emit(msg) => {
                self.dispatch(msg);
                true
            }
            EventResult::Consumed => true,
            EventResult::Ignored => false,
        }
    }

    /// A fetch in flight is the only thing here that finishes without an input
    /// event, so it is the only reason to wake on the clock.
    fn wake(&self) -> Option<Duration> {
        self.state.joke.is_loading().then_some(POLL_INTERVAL)
    }

    fn draw(&mut self, frame: &mut Frame) {
        // Completions arrive between frames; applying them here is what turns a
        // wake-up into the frame that shows the new joke.
        let _ = self.drain();

        let area = frame.area();
        frame
            .buffer_mut()
            .set_style(area, Style::default().bg(THEME.background));

        let joke = joke_text(&self.state.joke);
        self.ratcn.render(frame, &self.state, &THEME, |ctx| {
            let content_width = area.width.min(CONTENT_WIDTH);
            let joke_height = wrapped_height(&joke, content_width).max(1);
            let content_height = joke_height + 1 + ButtonSize::Large.height();
            let content_area = area.centered(
                Constraint::Length(content_width),
                Constraint::Length(content_height),
            );
            let [joke_area, button_area] = Layout::vertical([
                Constraint::Length(joke_height),
                Constraint::Length(ButtonSize::Large.height()),
            ])
            .spacing(1)
            .areas(content_area);
            let joke = joke.clone().into_owned();
            ctx.paint(move |ctx| {
                ctx.widget(
                    Paragraph::new(joke)
                        .alignment(Alignment::Center)
                        .wrap(Wrap { trim: true })
                        .style(Style::default().fg(ctx.theme.foreground)),
                    joke_area,
                );
            });

            let loading = ctx.state().joke.is_loading();
            let refresh = Button::new(if loading {
                "Fetching..."
            } else {
                "Another joke"
            })
            .size(ButtonSize::Large)
            .on_press(|| Msg::RefreshRequested)
            .disabled(loading);
            let [button_area] = Layout::horizontal([Constraint::Length(refresh.width())])
                .flex(Flex::Center)
                .areas(button_area);
            ctx.component(ids::REFRESH, refresh, button_area);
        });
    }
}

/// The only place app state changes. Pure: no I/O here, only state
/// transitions and, when I/O is needed, a returned [`Effect`].
fn update(state: &mut AppState, msg: Msg) -> Option<Effect> {
    match msg {
        Msg::FocusChanged(focus) => state.focus = focus,
        Msg::RefreshRequested if !state.joke.is_loading() => {
            state.joke = JokeState::Loading;
            return Some(Effect::FetchJoke);
        }
        // Already loading: don't start a second request.
        Msg::RefreshRequested => {}
        Msg::JokeFetchCompleted(Ok(joke)) if state.joke.is_loading() => {
            state.joke = JokeState::Ready(joke);
        }
        Msg::JokeFetchCompleted(Err(error)) if state.joke.is_loading() => {
            state.joke = JokeState::Failed(error);
        }
        // A completion with no request pending is stale; ignore it.
        Msg::JokeFetchCompleted(_) => {}
    }
    None
}

/// Run one effect in the background. The `sender` is the effect's only way to
/// report back: it may not touch app state, only send a `Msg`.
fn execute(effect: Effect, sender: Sender<Msg>) {
    match effect {
        Effect::FetchJoke => fetch::fetch_joke(sender),
    }
}

fn joke_text(state: &JokeState) -> Cow<'_, str> {
    match state {
        JokeState::Idle => Cow::Borrowed("Waiting for the first joke..."),
        JokeState::Loading => Cow::Borrowed("Fetching a fresh dad joke..."),
        JokeState::Ready(joke) => Cow::Borrowed(joke),
        JokeState::Failed(error) => Cow::Owned(format!("The joke could not be fetched: {error}")),
    }
}

fn main() -> io::Result<()> {
    let mut app = App::new();
    // Kick off the first fetch before the first frame.
    app.dispatch(Msg::RefreshRequested);
    demo_shared::run(app)
}

// The payoff of a pure `update`: state logic is testable without a terminal,
// a network, or an event loop.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_changes_state_and_returns_the_effect() {
        let mut state = AppState::default();

        let effect = update(&mut state, Msg::RefreshRequested);

        assert_eq!(state.joke, JokeState::Loading);
        assert_eq!(effect, Some(Effect::FetchJoke));
    }

    #[test]
    fn refresh_does_not_start_a_second_request_while_loading() {
        let mut state = AppState {
            joke: JokeState::Loading,
            ..AppState::default()
        };

        assert_eq!(update(&mut state, Msg::RefreshRequested), None);
        assert_eq!(state.joke, JokeState::Loading);
    }

    #[test]
    fn completion_without_a_pending_request_is_ignored() {
        let mut state = AppState::default();

        assert_eq!(
            update(
                &mut state,
                Msg::JokeFetchCompleted(Ok("unexpected completion".to_owned())),
            ),
            None
        );
        assert_eq!(state.joke, JokeState::Idle);
    }

    #[test]
    fn draining_completion_messages_updates_app_state() {
        let mut app = App::new();
        app.state.joke = JokeState::Loading;
        app.sender
            .send(Msg::JokeFetchCompleted(Ok(
                "Delivered through the queue.".to_owned()
            )))
            .expect("the app owns the receiver");

        assert!(app.drain());

        assert_eq!(
            app.state.joke,
            JokeState::Ready("Delivered through the queue.".to_owned())
        );
    }
}
