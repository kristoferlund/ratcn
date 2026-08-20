use std::{io, time::Duration};

use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::Style,
    widgets::{Paragraph, Wrap},
};
use ratcn::{
    Button, Dialog, List, ListItem, Theme, Toast, ToasterState, ToasterWidget,
    runtime::{CellOffset, Event, EventResult, FocusState, ModalState, Ratcn},
};

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

fn main() -> io::Result<()> {
    demo_shared::run(App::new())
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

    fn update(&mut self, msg: Msg) {
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

impl demo_shared::Demo for App {
    fn handle_event(&mut self, event: Event) -> bool {
        match self.ratcn.handle_event(event, &self.state) {
            EventResult::Emit(msg) => {
                self.update(msg);
                true
            }
            EventResult::Consumed => true,
            EventResult::Ignored => false,
        }
    }

    /// The next toast expiry, which wakes the host to prune and redraw.
    fn wake(&self) -> Option<Duration> {
        self.state
            .toasts
            .time_until_next_expiry(demo_shared::monotonic_time())
    }

    fn draw(&mut self, frame: &mut Frame, theme: &Theme) {
        let now = demo_shared::monotonic_time();
        let _ = self.state.toasts.prune_expired(now);

        let area = frame.area();
        frame
            .buffer_mut()
            .set_style(area, Style::default().bg(theme.background));
        self.ratcn.render(frame, &self.state, theme, |ctx| {
            let open_button = Button::new("Open Dialog").on_press(|| Msg::OpenDialog);
            let button_area = area.centered(
                Constraint::Length(open_button.width()),
                Constraint::Length(ratcn::ButtonSize::Small.height()),
            );
            ctx.component(ids::OPEN, open_button, button_area);
            if self.state.modals.is_open(ids::DIALOG) {
                ctx.modal(
                    ids::DIALOG,
                    Self::build_dialog(self.state.dialog_offset),
                    area,
                );
            }
        });
        frame.render_widget(
            ToasterWidget::new(&self.state.toasts, now).themed(theme),
            frame.area(),
        );
    }
}

impl App {
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

                ctx.paint_widget(
                    Paragraph::new(
                        "Select your favourite sci-fi writers. Use Up/Down to move and Enter to toggle a writer.",
                    )
                    .style(Style::default().fg(ctx.theme.muted_foreground))
                    .wrap(Wrap { trim: true }),
                    text_area,
                );

                ctx.component(
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
