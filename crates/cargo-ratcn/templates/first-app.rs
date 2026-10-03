use std::{
    io,
    time::{Duration, Instant},
};

use ratatui::layout::Constraint;
use ratcn::{
    Button, ButtonSize, Theme, Toast, ToasterState, ToasterWidget,
    runtime::{EventResult, FocusState, Ratcn},
    terminal::{Session, SessionEvent, SessionOptions, termina},
};

/// Everything the app knows. `update` is the only place it changes.
struct AppState {
    focus: FocusState,
    toasts: ToasterState<'static>,
}

#[derive(Clone)]
enum Msg {
    FocusChanged(FocusState),
    Hello,
}

impl AppState {
    fn update(&mut self, msg: Msg, now: Duration) {
        match msg {
            Msg::FocusChanged(focus) => self.focus = focus,
            Msg::Hello => self.toasts.push(Toast::success("World"), now),
        }
    }
}

struct App {
    state: AppState,
    ratcn: Ratcn<AppState, Msg>,
}

impl App {
    fn new() -> Self {
        Self {
            state: AppState {
                focus: FocusState::default(),
                toasts: ToasterState::default(),
            },
            ratcn: Ratcn::new().focus(|state: &AppState| &state.focus, Msg::FocusChanged),
        }
    }

    /// Route one event; apply whatever message it produced.
    fn handle_event(&mut self, event: termina::Event, now: Duration) {
        if let EventResult::Emit(msg) = self.ratcn.handle_event(event, &self.state) {
            self.state.update(msg, now);
        }
    }

    fn draw(&mut self, frame: &mut ratatui::Frame, theme: &Theme, now: Duration) {
        // Ratcn never reads a clock; the app says what time it is.
        self.state.toasts.prune_expired(now);

        let area = frame.area();
        self.ratcn.render(frame, area, &self.state, theme, |ctx| {
            let button = Button::new("Hello")
                .size(ButtonSize::Large)
                .on_press(|| Msg::Hello);
            let button_area = area.centered(
                Constraint::Length(button.width()),
                Constraint::Length(ButtonSize::Large.height()),
            );
            ctx.component("hello", button, button_area);
        });
        frame.render_widget(ToasterWidget::new(&self.state.toasts, now).themed(theme), area);
    }
}

fn main() -> io::Result<()> {
    let started = Instant::now();
    let mut app = App::new();
    let mut session = Session::open(SessionOptions::new().mouse().adaptive())?;

    loop {
        let now = started.elapsed();
        let theme = session.theme();
        session
            .terminal_mut()
            .draw(|frame| app.draw(frame, &theme, now))?;

        // Wait for input, or wake when the next toast is due to disappear.
        let timeout = app.state.toasts.time_until_next_expiry(now);
        if let Some(SessionEvent::Input(event)) = session.next(timeout)? {
            let quit = is_quit(&event);
            app.handle_event(event, started.elapsed());
            // A text field copies its selection on Ctrl+C; only a Ctrl+C that
            // copied nothing quits.
            match app.ratcn.take_clipboard() {
                Some(text) => session.set_clipboard(&text)?,
                None if quit => return Ok(()),
                None => {}
            }
        }
    }
}

fn is_quit(event: &termina::Event) -> bool {
    use termina::event::{KeyCode, KeyEventKind, Modifiers};

    matches!(
        event,
        termina::Event::Key(key)
            if key.kind == KeyEventKind::Press
                && key.code == KeyCode::Char('c')
                && key.modifiers.contains(Modifiers::CONTROL)
    )
}
