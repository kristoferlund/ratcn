use std::{io, time::Duration};

#[cfg(target_arch = "wasm32")]
use std::{cell::RefCell, rc::Rc};

use ratatui::{
    layout::{Constraint, Layout},
    style::Style,
    widgets::{Paragraph, Wrap},
};
use ratcn::{
    Button, Dialog, List, ListItem, Theme, Toast, ToasterState, ToasterWidget,
    runtime::{self, CellOffset, EventResult, FocusState, ModalState, Ratcn},
};

#[cfg(not(target_arch = "wasm32"))]
use ratatui::crossterm::event;

#[cfg(target_arch = "wasm32")]
use ratzilla::WebRenderer;

const THEME: Theme = Theme::default_dark();
const WRITERS: [&str; 8] = [
    "Ursula K. Le Guin",
    "Octavia E. Butler",
    "Isaac Asimov",
    "Philip K. Dick",
    "Ann Leckie",
    "Iain M. Banks",
    "Ted Chiang",
    "Martha Wells",
];
const WRITER_LIST_HEIGHT: u16 = 6;
const DIALOG_CONTENT_HEIGHT: u16 = WRITER_LIST_HEIGHT + 3;
const DIALOG_WIDTH: u16 = 52;
const DIALOG_HEIGHT: u16 = DIALOG_CONTENT_HEIGHT + 6;

/// Stable runtime child ids.
mod ids {
    pub const DIALOG: &str = "dialog";
    pub const OPEN: &str = "open";
    pub const WRITERS: &str = "writers";
}

struct App {
    state: AppState,
    ratcn: Ratcn<AppState, Msg>,
}

/// All mutable UI state lives here: domain state and focus alike.
#[derive(Default)]
struct AppState {
    focus: FocusState,
    modals: ModalState,
    dialog_offset: CellOffset,
    focused_writer: Option<&'static str>,
    saved_writers: Vec<&'static str>,
    draft_writers: Vec<&'static str>,
    writer_scroll_position: usize,
    toasts: ToasterState<'static>,
}

#[derive(Clone)]
enum Msg {
    OpenDialog,
    SavePressed,
    CancelPressed,
    FocusChanged(FocusState),
    DialogMoved(CellOffset),
    WriterFocusChanged(&'static str, usize),
    WriterToggled(&'static str),
    WriterScrollChanged(usize),
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> io::Result<()> {
    let mut app = App::new();
    ratatui::run(|terminal| {
        // RAII: mouse reporting is restored on every exit path (a `?` or panic).
        let _input_modes = ratcn::crossterm::InputModes::new()
            .mouse_capture()
            .enable()?;
        loop {
            let now = demo_shared::monotonic_time();
            let _ = app.state.toasts.prune_expired(now);
            terminal.draw(|frame| app.draw(frame, now))?;
            // Wait for input or the next toast expiry, which wakes the loop to prune and redraw.
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
            move |key_event| {
                app.borrow_mut().handle_event(key_event);
            }
        })
        .map_err(|e| io::Error::other(e.to_string()))?;

    terminal
        .on_mouse_event({
            let app = Rc::clone(&app);
            move |mouse_event| {
                app.borrow_mut().handle_event(mouse_event);
            }
        })
        .map_err(|e| io::Error::other(e.to_string()))?;

    terminal.draw_web(move |frame| {
        let now = demo_shared::monotonic_time();
        let mut app = app.borrow_mut();
        let _ = app.state.toasts.prune_expired(now);
        app.draw(frame, now);
    });

    Ok(())
}

impl App {
    fn new() -> Self {
        Self {
            state: AppState::default(),
            ratcn: Ratcn::new()
                .focus(|state: &AppState| &state.focus, Msg::FocusChanged)
                .modals(|state: &AppState| &state.modals),
        }
    }

    fn handle_event(&mut self, event: impl TryInto<runtime::Event>) {
        if let EventResult::Emit(msg) = self.ratcn.handle_event(event, &self.state) {
            match msg {
                Msg::OpenDialog => {
                    self.state
                        .draft_writers
                        .clone_from(&self.state.saved_writers);
                    self.state
                        .modals
                        .open(ids::DIALOG, &mut self.state.focus)
                        .expect("dialog cannot be nested under itself");
                }
                Msg::SavePressed => {
                    self.state
                        .saved_writers
                        .clone_from(&self.state.draft_writers);
                    self.state.modals.close(&mut self.state.focus);
                    self.toast(Toast::success("Selection saved"));
                }
                Msg::CancelPressed => {
                    self.state
                        .draft_writers
                        .clone_from(&self.state.saved_writers);
                    self.state.modals.close(&mut self.state.focus);
                    self.toast(Toast::info("Cancel pressed"));
                }
                Msg::FocusChanged(focus) => self.state.focus = focus,
                Msg::DialogMoved(offset) => self.state.dialog_offset = offset,
                Msg::WriterFocusChanged(writer, offset) => {
                    self.state.focused_writer = Some(writer);
                    self.state.writer_scroll_position = offset;
                }
                Msg::WriterToggled(writer) => {
                    self.state.focused_writer = Some(writer);
                    if let Some(position) = self
                        .state
                        .draft_writers
                        .iter()
                        .position(|selected| *selected == writer)
                    {
                        self.state.draft_writers.remove(position);
                    } else {
                        self.state.draft_writers.push(writer);
                    }
                }
                Msg::WriterScrollChanged(offset) => self.state.writer_scroll_position = offset,
            }
        }
    }

    fn draw(&mut self, frame: &mut ratatui::Frame, now: Duration) {
        let area = frame.area();
        frame
            .buffer_mut()
            .set_style(area, Style::default().bg(THEME.background));
        self.ratcn.render(frame, &self.state, &THEME, |ctx| {
            let open_button = Button::new("Open Dialog").on_press(|| Msg::OpenDialog);
            let button_area = area.centered(
                Constraint::Length(open_button.width()),
                Constraint::Length(ratcn::ButtonSize::Small.height()),
            );
            ctx.render_component(ids::OPEN, open_button, button_area);
            if self.state.modals.is_open(ids::DIALOG) {
                ctx.modal(
                    ids::DIALOG,
                    Self::build_dialog(self.state.dialog_offset),
                    area,
                );
            }
        });
        frame.render_widget(
            ToasterWidget::new(&self.state.toasts, now).themed(&THEME),
            frame.area(),
        );
    }

    fn toast(&mut self, toast: Toast<'static>) {
        self.state.toasts.push(toast, demo_shared::monotonic_time());
    }

    /// The modal dialog is rebuilt into the retained runtime each frame.
    fn build_dialog(offset: CellOffset) -> Dialog<AppState, Msg> {
        Dialog::new()
            .offset(offset)
            .on_offset_change(Msg::DialogMoved)
            .on_dismiss(|| Msg::CancelPressed)
            .title("Sci-fi writers")
            .outer_width(DIALOG_WIDTH)
            .outer_height(DIALOG_HEIGHT)
            .content(4, |ctx| {
                let content_layout = Layout::vertical([
                    Constraint::Length(2),
                    Constraint::Length(WRITER_LIST_HEIGHT),
                ])
                .spacing(1);

                let [text_area, list_area] = ctx.area().layout(&content_layout);

                ctx.paint(move |ctx| {
                    ctx.render_widget(
                        Paragraph::new(
                            "Select your favourite sci-fi writers. Use Up/Down to move and Enter to toggle a writer.",
                        )
                        .style(Style::default().fg(ctx.theme.muted_foreground))
                        .wrap(Wrap { trim: true }),
                        text_area,
                    );
                });

                ctx.render_component(
                    ids::WRITERS,
                    List::new(WRITERS.map(|writer| ListItem::new(writer, writer)))
                        .item_focus(|s: &AppState| s.focused_writer, Msg::WriterFocusChanged)
                        .multi_selection(|s: &AppState, writer| s.draft_writers.contains(writer), Msg::WriterToggled)
                        .scroll(|s: &AppState| s.writer_scroll_position, Msg::WriterScrollChanged),
                    list_area,
                );
            })
            .action(
                "save",
                Button::new("Save").on_press(|| Msg::SavePressed),
            )
            .action(
                "cancel",
                Button::new("Cancel")
                    .secondary()
                    .on_press(|| Msg::CancelPressed),
            )
    }
}
