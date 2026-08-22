//! Responsive component grid and a larger-app state/message example.
//!
//! The shell owns cross-cutting state. Stateful tile modules own their own
//! `State`, local `Msg`, and `State::update`; `AppMsg` wraps those messages so
//! the one Ratcn runtime can route every component through the same app type.

use std::{io, time::Duration};

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::Style,
    text::{Line, Span},
};
use ratcn::{
    Theme, Toast, ToasterState, ToasterWidget,
    runtime::{Event, EventResult, FocusState, KeyChord, ModalState, MouseKind, Ratcn, TabWrap},
};

mod screensaver;
mod tiles;

const TILE_COUNT: usize = tiles::TILES.len();
const TILE_WIDTH: u16 = 42;
const TILE_HEIGHT: u16 = 20;
const TILE_GAP: u16 = 2;
const GRID_PADDING_X: u16 = 0;
const GRID_PADDING_Y: u16 = 3;

/// How often the screensaver wants a frame while it runs.
///
/// Its snow moves in whole cells, one cell every 140 ms at the fastest, so this
/// cadence shows every step a flake takes — asking for more frames would repaint
/// the same one.
const SCREENSAVER_FRAME: Duration = Duration::from_millis(50);

struct App {
    state: AppState,
    ratcn: Ratcn<AppState, AppMsg>,
}

#[derive(Default)]
struct AppState {
    focus: FocusState,
    controls_disabled: bool,
    themes_state: tiles::themes::State,
    /// What the terminal says it looks like, refreshed each frame.
    resolved_theme: Theme,
    notifications_state: tiles::notifications::State,
    release_state: tiles::release::State,
    modals_state: ModalState,
    screensaver: screensaver::State,
    toasts: ToasterState<'static>,
}

impl AppState {
    fn theme(&self) -> Theme {
        self.themes_state.theme(self.resolved_theme)
    }
}

#[derive(Clone)]
enum AppMsg {
    FocusChanged(FocusState),
    ToggleDisableAll,
    ScreensaverActivated,
    ScreensaverDismissed,
    Toast(Toast<'static>),
    Themes(tiles::themes::Msg),
    Notifications(tiles::notifications::Msg),
    Release(tiles::release::Msg),
}

fn main() -> io::Result<()> {
    demo_shared::run(App::new())
}

impl App {
    fn new() -> Self {
        let ratcn = Ratcn::new()
            .focus(|state: &AppState| &state.focus, AppMsg::FocusChanged)
            .modals(|state: &AppState| &state.modals_state)
            .hover_focus()
            .tab_wrap(TabWrap::Wrap);
        // Alt+N jumps to tile N; the table fixes both the grid order and the key.
        let ratcn = tiles::TILES
            .iter()
            .enumerate()
            .fold(ratcn, |ratcn, (index, tile)| {
                let digit = char::from(b'1' + index as u8);
                ratcn.focus_key(KeyChord::from(digit).alt(), [tile.id])
            });
        Self {
            state: AppState::default(),
            ratcn,
        }
    }

    fn update(&mut self, msg: AppMsg) {
        match msg {
            AppMsg::FocusChanged(focus) => self.state.focus = focus,
            AppMsg::ToggleDisableAll => {
                self.state.controls_disabled = !self.state.controls_disabled;
            }
            AppMsg::ScreensaverActivated => {
                self.state.screensaver =
                    screensaver::State::activate(demo_shared::monotonic_time());
                self.state
                    .modals_state
                    .open(screensaver::ID, &mut self.state.focus)
                    .expect("cannot open the screensaver: a modal is already open");
            }
            AppMsg::ScreensaverDismissed => {
                self.state.modals_state.close(&mut self.state.focus);
            }
            AppMsg::Toast(toast) => self.toast(toast),
            AppMsg::Themes(msg) => self.state.themes_state.update(msg),
            AppMsg::Notifications(msg) => self.state.notifications_state.update(msg),
            AppMsg::Release(msg) => {
                let next_msg = self.state.release_state.update(
                    msg,
                    &mut self.state.modals_state,
                    &mut self.state.focus,
                );
                if let Some(msg) = next_msg {
                    self.update(msg);
                }
            }
        }
    }

    fn toast(&mut self, toast: Toast<'static>) {
        self.state.toasts.push(toast, demo_shared::monotonic_time());
    }

    /// App-global keys, taking priority over the focused widget.
    fn app_hotkeys(&mut self, event: &Event) -> bool {
        let Event::Key(key) = event else {
            return false;
        };
        if KeyChord::from('d').alt().matches(key) {
            self.update(AppMsg::ToggleDisableAll);
            return true;
        }
        if KeyChord::from('s').alt().matches(key) {
            self.update(AppMsg::ScreensaverActivated);
            return true;
        }
        false
    }
}

impl demo_shared::Demo for App {
    /// Bracketed paste natively, and the browser's `paste` event on the web:
    /// the wiring is the demonstration, since no component reads a paste yet.
    const PASTE: bool = true;

    /// Paint with the terminal's own colors, falling back to `THEME`. The
    /// picker lists whatever that resolves to alongside the presets.
    const ADAPTIVE: bool = true;

    fn handle_event(&mut self, event: Event) -> bool {
        // The screensaver is dismissed by app policy, before routing: any
        // pointer motion wakes the app. Other events fall through and are
        // absorbed by the modal layer.
        if self.state.modals_state.is_open(screensaver::ID)
            && matches!(&event, Event::Mouse(mouse) if mouse.kind == MouseKind::Moved)
        {
            self.update(AppMsg::ScreensaverDismissed);
            return true;
        }
        let modal_active = self.state.modals_state.top().is_some() || self.ratcn.modal_is_open();
        if !modal_active && self.app_hotkeys(&event) {
            return true;
        }
        match self.ratcn.handle_event(event, &self.state) {
            EventResult::Emit(msg) => {
                self.update(msg);
                true
            }
            EventResult::Consumed => true,
            EventResult::Ignored => false,
        }
    }

    /// The next toast expiry, or — while the screensaver runs — its own frame
    /// cadence, since its snow moves with the clock alone.
    fn wake(&self) -> Option<Duration> {
        let expiry = self
            .state
            .toasts
            .time_until_next_expiry(demo_shared::monotonic_time());
        if self.state.modals_state.is_open(screensaver::ID) {
            Some(expiry.map_or(SCREENSAVER_FRAME, |expiry| expiry.min(SCREENSAVER_FRAME)))
        } else {
            expiry
        }
    }

    fn draw(&mut self, frame: &mut Frame, theme: &Theme) {
        // The picker lists whatever the terminal currently resolves to, so the
        // frame's theme reaches the state before anything reads it.
        self.state.resolved_theme = *theme;
        let now = demo_shared::monotonic_time();
        let _ = self.state.toasts.prune_expired(now);

        let frame_area = frame.area();
        let theme = self.state.theme();
        frame
            .buffer_mut()
            .set_style(frame_area, Style::default().bg(theme.background));
        let state = &self.state;
        self.ratcn.render(frame, state, &theme, |ctx| {
            ctx.paint_widget(header_bar(ctx.theme), header_area(frame_area));
            for (index, tile_area) in tile_areas(frame_area).into_iter().enumerate() {
                tiles::declare(index, ctx, tile_area);
            }
            if state.modals_state.is_open(tiles::release::DIALOG_ID) {
                ctx.modal(
                    tiles::release::DIALOG_ID,
                    tiles::release::dialog(state.release_state.dialog_offset),
                    frame_area,
                );
            }
            if state.modals_state.is_open(screensaver::ID) {
                screensaver::declare(ctx, frame_area, now);
            }
            ctx.defer_paint(move |ctx| {
                let state = ctx.state();
                ctx.widget(
                    ToasterWidget::new(&state.toasts, now).themed(&state.theme()),
                    frame_area,
                );
            });
        });
    }
}

/// The global-shortcut hints, centered on one line.
fn header_bar(theme: &Theme) -> Line<'static> {
    let key = Style::default().fg(theme.foreground);
    let hint = Style::default().fg(theme.muted_foreground);
    Line::from(vec![
        Span::styled("alt+d", key),
        Span::styled(" disable all controls", hint),
        Span::styled("   ", hint),
        Span::styled("alt+s", key),
        Span::styled(" screensaver", hint),
    ])
    .centered()
}

/// The second row, centered within the grid's top padding; empty when the
/// frame is too short to spare it.
fn header_area(frame_area: Rect) -> Rect {
    let height = u16::from(frame_area.height > 1);
    Rect::new(frame_area.x, frame_area.y + 1, frame_area.width, height)
}

fn tile_areas(frame_area: Rect) -> [Rect; TILE_COUNT] {
    let grid_bounds = frame_area.inner(Margin {
        horizontal: GRID_PADDING_X,
        vertical: GRID_PADDING_Y,
    });
    let columns = column_count(grid_bounds.width);
    let rows = TILE_COUNT.div_ceil(columns as usize) as u16;
    let tile_height = tile_height(grid_bounds.height, rows);
    let grid_width = columns * TILE_WIDTH + (columns.saturating_sub(1) * TILE_GAP);
    let grid_height = rows * tile_height + rows.saturating_sub(1) * TILE_GAP;
    let grid_area = grid_bounds.centered(
        Constraint::Length(grid_width),
        Constraint::Length(grid_height),
    );
    let row_areas = grid_area.layout_vec(
        &Layout::vertical(std::iter::repeat_n(
            Constraint::Length(tile_height),
            rows as usize,
        ))
        .spacing(TILE_GAP),
    );
    let column_areas = grid_area.layout_vec(
        &Layout::horizontal(std::iter::repeat_n(
            Constraint::Length(TILE_WIDTH),
            columns as usize,
        ))
        .spacing(TILE_GAP),
    );

    std::array::from_fn(|index| {
        let row = index / columns as usize;
        let column = index % columns as usize;
        Rect::new(
            column_areas[column].x,
            row_areas[row].y,
            column_areas[column].width,
            row_areas[row].height,
        )
    })
}

fn column_count(width: u16) -> u16 {
    let columns = (width + TILE_GAP) / (TILE_WIDTH + TILE_GAP);
    columns.clamp(1, 4)
}

fn tile_height(area_height: u16, rows: u16) -> u16 {
    let gaps = rows.saturating_sub(1) * TILE_GAP;
    let available = area_height.saturating_sub(gaps) / rows;
    TILE_HEIGHT.min(available).max(1)
}
