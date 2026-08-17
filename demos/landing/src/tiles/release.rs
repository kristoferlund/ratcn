//! Release tile, dialog declaration, and local dialog position.

use ratatui::{
    layout::{Constraint, Flex, Layout},
    style::{Modifier, Style},
    widgets::{Paragraph, Wrap},
};
use ratcn::{
    Button, Dialog, ListItem, Select, Toast,
    runtime::{CellOffset, DeclareCtx, FocusState, ModalState},
};

use crate::{AppMsg, AppState};

use super::shared::declare_tile_panel;

pub const ID: &str = "release";
pub const DIALOG_ID: &str = "create_release";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ReleaseMedia {
    Vinyl,
    Cd,
    Cassette,
    MiniDisc,
}

const RELEASE_MEDIA: [(ReleaseMedia, &str); 4] = [
    (ReleaseMedia::Vinyl, "Vinyl"),
    (ReleaseMedia::Cd, "CD"),
    (ReleaseMedia::Cassette, "Cassette"),
    (ReleaseMedia::MiniDisc, "MiniDisc"),
];

#[derive(Default)]
pub struct State {
    pub dialog_offset: CellOffset,
    pub media: Option<ReleaseMedia>,
    pub media_cursor: Option<ReleaseMedia>,
    pub media_open: bool,
}

#[derive(Clone, Copy)]
pub enum Msg {
    Open,
    Create,
    Cancel,
    DialogMoved(CellOffset),
    MediaOpenChanged(bool),
    MediaFocused(ReleaseMedia),
    MediaSelected(ReleaseMedia),
}

impl State {
    pub fn update(
        &mut self,
        msg: Msg,
        modals: &mut ModalState,
        focus: &mut FocusState,
    ) -> Option<AppMsg> {
        match msg {
            Msg::Open => {
                self.media_open = false;
                modals
                    .open(DIALOG_ID, focus)
                    .expect("create-release dialog cannot be nested under itself");
                None
            }
            Msg::Create => {
                self.media_open = false;
                modals.close(focus).expect("create-release dialog is open");
                Some(AppMsg::Toast(Toast::success("Release created")))
            }
            Msg::Cancel => {
                self.media_open = false;
                modals.close(focus).expect("create-release dialog is open");
                Some(AppMsg::Toast(Toast::info("Release cancelled")))
            }
            Msg::DialogMoved(offset) => {
                self.dialog_offset = offset;
                None
            }
            Msg::MediaOpenChanged(open) => {
                self.media_open = open;
                None
            }
            Msg::MediaFocused(media) => {
                self.media_cursor = Some(media);
                None
            }
            Msg::MediaSelected(media) => {
                self.media = Some(media);
                self.media_cursor = Some(media);
                self.media_open = false;
                None
            }
        }
    }
}

pub fn declare(ctx: &mut DeclareCtx<'_, AppState, AppMsg>) {
    let area = ctx.area();
    let controls_disabled = ctx.state().controls_disabled;
    let create_release = Button::new("Create release")
        .on_press(|| AppMsg::Release(Msg::Open))
        .disabled(controls_disabled);
    let button_width = create_release.width();
    let content_layout = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(ratcn::ButtonSize::Small.height()),
    ])
    .flex(Flex::Center)
    .spacing(1);
    let button_layout = Layout::horizontal([Constraint::Length(button_width)]).flex(Flex::Center);
    let inner_area = declare_tile_panel(ctx, area, " alt+2 ");
    let [header_area, body_area, button_row] = inner_area.layout(&content_layout);
    ctx.paint(move |ctx| {
        let theme = ctx.theme;
        ctx.widget(
            Paragraph::new("Distribute track")
                .style(
                    Style::default()
                        .fg(theme.foreground)
                        .add_modifier(Modifier::BOLD),
                )
                .centered(),
            header_area,
        );
        ctx.widget(
            Paragraph::new(
                "Upload your first master to start reaching listeners on Spotify, Apple Music and more.",
            )
            .style(Style::default().fg(theme.muted_foreground))
            .centered()
            .wrap(Wrap { trim: true }),
            body_area,
        );
    });
    let [button_area] = button_row.layout(&button_layout);
    ctx.component("create_release", create_release, button_area);
}

pub fn dialog(offset: CellOffset) -> Dialog<AppState, AppMsg> {
    Dialog::new()
        .offset(offset)
        .on_offset_change(|offset| AppMsg::Release(Msg::DialogMoved(offset)))
        .on_dismiss(|| AppMsg::Release(Msg::Cancel))
        .title("Create release")
        .outer_width(50)
        .content(4, |ctx| {
            let [description_area, _gap, select_area] = Layout::vertical([
                Constraint::Length(2),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .areas(ctx.area());
            ctx.paint(move |ctx| {
                ctx.widget(
                    Paragraph::new("Set up artwork, metadata, territories, and release date before sending your track to stores.")
                        .style(Style::default().fg(ctx.theme.muted_foreground))
                        .wrap(Wrap { trim: true }),
                    description_area,
                );
            });
            ctx.component(
                "release_media",
                Select::new(RELEASE_MEDIA.map(|(media, label)| ListItem::new(media, label)))
                    .placeholder("Select release media...")
                    .open(
                        |state: &AppState| state.release_state.media_open,
                        |open| AppMsg::Release(Msg::MediaOpenChanged(open)),
                    )
                    .item_focus(
                        |state: &AppState| state.release_state.media_cursor,
                        |media| AppMsg::Release(Msg::MediaFocused(media)),
                    )
                    .selection(
                        |state: &AppState| state.release_state.media,
                        |media| AppMsg::Release(Msg::MediaSelected(media)),
                    ),
                select_area,
            );
        })
        .action(
            "cancel",
            Button::new("Cancel")
                .secondary()
                .on_press(|| AppMsg::Release(Msg::Cancel)),
        )
        .action(
            "save",
            Button::new("Save").on_press(|| AppMsg::Release(Msg::Create)),
        )
}
