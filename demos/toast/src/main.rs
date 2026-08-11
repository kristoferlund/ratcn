//! Toasts: transient notifications the app owns and the widget draws.
//!
//! The interesting part is the clock. Ratcn never reads one, so expiry is driven
//! entirely from here: push with a timestamp, ask
//! `time_until_next_expiry` when to wake, and `prune_expired` when that
//! timeout elapses.

use std::{io, time::Duration};

#[cfg(target_arch = "wasm32")]
use std::{cell::RefCell, rc::Rc};

use ratatui::{
    layout::{Constraint, Layout},
    style::Style,
};
use ratcn::{
    Button, ButtonSize, Theme, Toast, ToastKind, ToasterState, ToasterWidget,
    runtime::{self, EventResult, FocusState, HoverState, Ratcn, TabWrap},
};

#[cfg(not(target_arch = "wasm32"))]
use ratatui::crossterm::event;

#[cfg(target_arch = "wasm32")]
use ratzilla::WebRenderer;

const THEME: Theme = Theme::default_dark();

/// One entry per toast kind, with the text it shows.
const FLAVORS: [(ToastKind, &str, &str); 6] = [
    (ToastKind::Success, "Success toast", "That went well."),
    (ToastKind::Error, "Error toast", "That did not go well."),
    (ToastKind::Warning, "Warning toast", "Proceed carefully."),
    (ToastKind::Info, "Info toast", "Something worth knowing."),
    (ToastKind::Loading, "Loading toast", "Still in progress."),
    (ToastKind::Default, "Default toast", "No particular news."),
];

mod ids {
    pub const BUTTON: &str = "make-toast";
    pub const SAVE: &str = "save";
}

#[derive(Default)]
struct AppState {
    focus: FocusState,
    hover: HoverState,
    toasts: ToasterState<'static>,
    /// Advanced on every press, so consecutive toasts differ.
    seed: u32,
    save_in_progress: bool,
}

impl AppState {
    /// A tiny xorshift, so the demo needs no `rand` dependency and behaves the
    /// same natively and in the browser.
    fn next_flavor(&mut self) -> (ToastKind, &'static str, &'static str) {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        FLAVORS[self.seed as usize % FLAVORS.len()]
    }

    fn toggle_save(&mut self, now: Duration) {
        if self.save_in_progress {
            let _ = self.toasts.replace("save", Toast::success("Saved"), now);
        } else {
            self.toasts
                .push(Toast::loading("Saving").persistent().id("save"), now);
        }
        self.save_in_progress = !self.save_in_progress;
    }
}

#[derive(Clone)]
enum Msg {
    Focus(FocusState),
    Hover(HoverState),
    MakeToast,
    ToggleSave,
}

struct App {
    state: AppState,
    ratcn: Ratcn<AppState, Msg>,
}

impl App {
    fn new() -> Self {
        Self {
            state: AppState {
                // Seeded from the clock so a run does not always start on the
                // same flavor. Any non-zero value works; xorshift sticks at 0.
                seed: demo_shared::monotonic_time().subsec_nanos() | 1,
                ..AppState::default()
            },
            ratcn: Ratcn::new()
                .focus(|s: &AppState| &s.focus, Msg::Focus)
                .hover(|s: &AppState| &s.hover, Msg::Hover)
                .tab_wrap(TabWrap::Wrap),
        }
    }

    fn handle_event(&mut self, event: impl TryInto<runtime::Event>) {
        if let EventResult::Emit(msg) = self.ratcn.handle_event(event, &self.state) {
            match msg {
                Msg::Focus(focus) => self.state.focus = focus,
                Msg::Hover(hover) => self.state.hover = hover,
                Msg::MakeToast => {
                    let (kind, title, description) = self.state.next_flavor();
                    self.state.toasts.push(
                        Toast::new(title).kind(kind).description(description),
                        demo_shared::monotonic_time(),
                    );
                }
                Msg::ToggleSave => self.state.toggle_save(demo_shared::monotonic_time()),
            }
        }
    }

    fn draw(&mut self, frame: &mut ratatui::Frame, now: Duration) {
        let area = frame.area();
        frame
            .buffer_mut()
            .set_style(area, Style::default().bg(THEME.background));

        self.ratcn.render(frame, &self.state, &THEME, |ctx| {
            let random_button = Button::new("Make me a toast!")
                .size(ButtonSize::Large)
                .on_press(|| Msg::MakeToast);
            let save_label = if self.state.save_in_progress {
                "Finish save"
            } else {
                "Start save"
            };
            let save_button = Button::new(save_label)
                .size(ButtonSize::Large)
                .on_press(|| Msg::ToggleSave);
            let buttons_area = area.centered(
                Constraint::Length(random_button.width().max(save_button.width())),
                Constraint::Length(ButtonSize::Large.height() * 2 + 1),
            );
            let [random_area, save_area] = Layout::vertical([
                Constraint::Length(ButtonSize::Large.height()),
                Constraint::Length(ButtonSize::Large.height()),
            ])
            .spacing(1)
            .areas(buttons_area);
            ctx.render_component(ids::BUTTON, random_button, random_area);
            ctx.render_component(ids::SAVE, save_button, save_area);
        });

        // Toasts are decoration over the whole frame, painted after the
        // declaration pass so they sit above everything.
        frame.render_widget(
            ToasterWidget::new(&self.state.toasts, now).themed(&THEME),
            area,
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> io::Result<()> {
    let mut app = App::new();
    ratatui::run(|terminal| {
        let _input_modes = ratcn::crossterm::InputModes::new()
            .mouse_capture()
            .enable()?;
        loop {
            let now = demo_shared::monotonic_time();
            let _ = app.state.toasts.prune_expired(now);
            terminal.draw(|frame| app.draw(frame, now))?;

            // Block on input, but wake for the next expiry so a toast
            // disappears on time even when nothing is typed.
            if let Some(timeout) = app
                .state
                .toasts
                .time_until_next_expiry(demo_shared::monotonic_time())
                && !event::poll(timeout)?
            {
                continue;
            }
            let event = event::read()?;
            if demo_shared::is_quit(&event) {
                break Ok(());
            }
            app.handle_event(event);
        }
    })
}

#[cfg(target_arch = "wasm32")]
fn main() -> io::Result<()> {
    let backend = demo_shared::web_backend(THEME.background)?;
    let mut terminal = ratatui::Terminal::new(backend)?;
    let app = Rc::new(RefCell::new(App::new()));

    terminal
        .on_key_event({
            let app = Rc::clone(&app);
            move |key_event| app.borrow_mut().handle_event(key_event)
        })
        .map_err(|e| io::Error::other(e.to_string()))?;
    terminal
        .on_mouse_event({
            let app = Rc::clone(&app);
            move |mouse_event| app.borrow_mut().handle_event(mouse_event)
        })
        .map_err(|e| io::Error::other(e.to_string()))?;

    // The browser redraws continuously, so pruning each frame is enough — no
    // timer needed.
    terminal.draw_web(move |frame| {
        let now = demo_shared::monotonic_time();
        let mut app = app.borrow_mut();
        let _ = app.state.toasts.prune_expired(now);
        app.draw(frame, now);
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_flavor_is_reachable() {
        let mut state = AppState {
            seed: 1,
            ..AppState::default()
        };
        let mut seen = std::collections::HashSet::new();
        for _ in 0..1_000 {
            seen.insert(state.next_flavor().1);
        }
        assert_eq!(
            seen.len(),
            FLAVORS.len(),
            "the sequence must reach every kind"
        );
    }

    #[test]
    fn expired_toasts_are_pruned_and_the_rest_kept() {
        let mut state = AppState::default();
        state.toasts.push(
            Toast::new("gone").duration(Duration::from_secs(1)),
            Duration::ZERO,
        );
        state
            .toasts
            .push(Toast::new("stays").persistent(), Duration::ZERO);

        assert!(state.toasts.prune_expired(Duration::from_secs(2)));
        assert_eq!(state.toasts.len(), 1);
    }

    #[test]
    fn save_flow_replaces_loading_with_success() {
        let mut state = AppState::default();

        state.toggle_save(Duration::ZERO);
        assert_eq!(state.toasts.len(), 1);
        assert_eq!(
            state.toasts.entries()[0].toast().toast_kind(),
            ToastKind::Loading
        );

        state.toggle_save(Duration::from_secs(1));
        assert_eq!(state.toasts.len(), 1);
        assert_eq!(
            state.toasts.entries()[0].toast().toast_kind(),
            ToastKind::Success
        );
        assert_eq!(
            state.toasts.time_until_next_expiry(Duration::from_secs(1)),
            Some(Duration::from_secs(4))
        );
    }
}
