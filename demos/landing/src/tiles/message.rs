//! A message composer: a title, a message, and a Send button that raises a
//! toast.
//!
//! Enter in the title moves on to the message, as a mail client's subject line
//! does. Send, or Ctrl+Enter (Ctrl+J in a terminal) from the message, sends.

use ratatui::{
    layout::{Constraint, Flex, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Padding, Paragraph},
};
use ratcn::{
    Button,
    ButtonSize::Large,
    Input, InputState, TextArea, TextAreaState, Toast,
    runtime::{DeclareCtx, FocusState},
};

use crate::{AppMsg, AppState};

use super::shared::declare_tile_panel_with_padding;

pub const ID: &str = "message";
const TITLE: &str = "title";
const MESSAGE: &str = "message";

#[derive(Default)]
pub struct State {
    pub title: InputState,
    pub message: TextAreaState,
}

#[derive(Clone)]
pub enum Msg {
    Title(InputState),
    Message(TextAreaState),
    Send,
}

impl State {
    /// The toast a send raises, if any.
    pub fn update(&mut self, msg: Msg) -> Option<AppMsg> {
        match msg {
            Msg::Title(title) => self.title = title,
            Msg::Message(message) => self.message = message,
            Msg::Send => {
                let toast = Toast::success("Message sent");
                let toast = match self.title.value() {
                    "" => toast,
                    title => toast.with_description(title.to_owned()),
                };
                *self = Self::default();
                return Some(AppMsg::Toast(toast));
            }
        }
        None
    }
}

pub fn declare(ctx: &mut DeclareCtx<'_, AppState, AppMsg>) {
    let area = ctx.area();
    let disabled = ctx.state().controls_disabled;
    let title = Input::new()
        .value(
            |state: &AppState| &state.message_state.title,
            |title| AppMsg::Message(Msg::Title(title)),
        )
        .title("Title")
        .placeholder("Re: the coffee machine")
        .on_submit(|| AppMsg::FocusChanged(FocusState::intent([ID, MESSAGE])))
        .disabled(disabled);
    let message = TextArea::new()
        .value(
            |state: &AppState| &state.message_state.message,
            |message| AppMsg::Message(Msg::Message(message)),
        )
        .title("Message")
        .placeholder("The coffee machine has feelings.")
        .on_submit(|| AppMsg::Message(Msg::Send))
        .disabled(disabled);
    let send = Button::new("Send")
        .size(Large)
        .on_press(|| AppMsg::Message(Msg::Send))
        .disabled(disabled);

    let inner = declare_tile_panel_with_padding(ctx, area, " alt+2 ", Padding::new(2, 2, 1, 1));
    // The title, the field, and the button keep their rows; the message takes
    // what is left, and is left out once that is too short to show a line.
    let [
        header_area,
        _gap_one,
        title_area,
        message_area,
        _gap_two,
        button_row,
    ] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(title.height()),
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(Large.height()),
    ])
    .areas(inner);
    let [send_area] = Layout::horizontal([Constraint::Length(send.width())])
        .flex(Flex::End)
        .areas(button_row);

    ctx.paint_widget(
        Paragraph::new("Send message").style(
            Style::default()
                .fg(ctx.theme.foreground)
                .add_modifier(Modifier::BOLD),
        ),
        header_area,
    );
    ctx.component(TITLE, title, title_area);
    let message_area = if message_area.height < 3 {
        Rect::ZERO
    } else {
        message_area
    };
    ctx.component(MESSAGE, message, message_area);
    ctx.component("send", send, send_area);
}
