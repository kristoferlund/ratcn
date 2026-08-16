//! Responsive component grid and a larger-app state/message example.
//!
//! The shell owns cross-cutting state. Stateful tile modules own their own
//! `State`, local `Msg`, and `State::update`; `AppMsg` wraps those messages so
//! the one Ratcn runtime can route every component through the same app type.

use std::{io, time::Duration};

#[cfg(target_arch = "wasm32")]
use std::{cell::RefCell, rc::Rc};

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::Style,
    text::{Line, Span},
};
use ratcn::{
    Theme, Toast, ToasterState, ToasterWidget,
    runtime::{
        self, EventResult, FocusState, HoverState, KeyChord, ModalState, MouseKind, Ratcn, TabWrap,
    },
};

mod screensaver;
mod tiles;

#[cfg(not(target_arch = "wasm32"))]
use ratatui::crossterm::event;

#[cfg(target_arch = "wasm32")]
use ratzilla::WebRenderer;

const TILE_COUNT: usize = tiles::TILES.len();
const TILE_WIDTH: u16 = 42;
const TILE_HEIGHT: u16 = 20;
const TILE_GAP: u16 = 2;
const GRID_PADDING_X: u16 = 0;
const GRID_PADDING_Y: u16 = 3;

#[cfg(not(target_arch = "wasm32"))]
const SCREENSAVER_FRAME: Duration = Duration::from_millis(50);

struct App {
    state: AppState,
    ratcn: Ratcn<AppState, AppMsg>,
}

#[derive(Default)]
struct AppState {
    focus: FocusState,
    hover: HoverState,
    controls_disabled: bool,
    themes_state: tiles::themes::State,
    notifications_state: tiles::notifications::State,
    release_state: tiles::release::State,
    modals_state: ModalState,
    screensaver: screensaver::State,
    toasts: ToasterState<'static>,
}

impl AppState {
    fn theme(&self) -> Theme {
        self.themes_state.theme()
    }
}

#[derive(Clone)]
enum AppMsg {
    FocusChanged(FocusState),
    HoverChanged(HoverState),
    ToggleDisableAll,
    ScreensaverActivated,
    ScreensaverDismissed,
    Toast(Toast<'static>),
    Themes(tiles::themes::Msg),
    Notifications(tiles::notifications::Msg),
    Release(tiles::release::Msg),
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> io::Result<()> {
    let mut app = App::new();
    ratatui::run(|terminal| {
        // RAII: terminal input modes are restored on every exit path.
        let _input_modes = ratcn::crossterm::InputModes::new()
            .mouse_capture()
            .bracketed_paste()
            .enable()?;
        loop {
            let now = demo_shared::monotonic_time();
            let _ = app.state.toasts.prune_expired(now);
            terminal.draw(|frame| app.draw(frame, now))?;
            // Wait for input, the next toast expiry, or — while the
            // screensaver runs — the next animation frame. A timeout loops
            // back around to redraw.
            let toast_timeout = app
                .state
                .toasts
                .time_until_next_expiry(demo_shared::monotonic_time());
            let timeout = if app.state.modals_state.is_open(screensaver::ID) {
                Some(toast_timeout.map_or(SCREENSAVER_FRAME, |t| t.min(SCREENSAVER_FRAME)))
            } else {
                toast_timeout
            };
            if let Some(timeout) = timeout
                && !event::poll(timeout)?
            {
                continue;
            }
            let event = event::read()?;
            if demo_shared::is_quit(&event) {
                break Ok(());
            }
            // Raw backend events go straight in; the runtime converts and
            // synthesizes Click/Drag.
            app.handle_event(event);
        }
    })
}

#[cfg(target_arch = "wasm32")]
fn main() -> io::Result<()> {
    let app = Rc::new(RefCell::new(App::new()));
    // Canvas padding is fixed at construction, so it tracks the starting theme.
    // Switching themes at runtime leaves it on the previous background.
    let backend = demo_shared::web_backend(app.borrow().state.theme().background)?;
    let mut terminal = ratatui::Terminal::new(backend)?;
    let paste_listener = demo_shared::BrowserPasteListener::install({
        let app = Rc::clone(&app);
        move |text| {
            let mut app = app.borrow_mut();
            if !app.ratcn.has_rendered() {
                return false;
            }
            app.handle_event(runtime::Event::Paste(text));
            true
        }
    })?;

    terminal
        .on_key_event({
            let app = Rc::clone(&app);
            move |key_event| {
                app.borrow_mut().handle_event(key_event);
            }
        })
        .map_err(|e| io::Error::other(e.to_string()))?;

    // Browser mouse: ratzilla reports cell coordinates directly; the runtime's
    // tracker synthesizes Click/Drag.
    terminal
        .on_mouse_event({
            let app = Rc::clone(&app);
            move |mouse_event| {
                app.borrow_mut().handle_event(mouse_event);
            }
        })
        .map_err(|e| io::Error::other(e.to_string()))?;

    terminal.draw_web(move |frame| {
        let _ = &paste_listener;
        let now = demo_shared::monotonic_time();
        let mut app = app.borrow_mut();
        let _ = app.state.toasts.prune_expired(now);
        app.draw(frame, now);
    });

    Ok(())
}

impl App {
    fn new() -> Self {
        let ratcn = Ratcn::new()
            .focus(|state: &AppState| &state.focus, AppMsg::FocusChanged)
            .hover(|state: &AppState| &state.hover, AppMsg::HoverChanged)
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
            AppMsg::HoverChanged(hover) => self.state.hover = hover,
            AppMsg::ToggleDisableAll => {
                self.state.controls_disabled = !self.state.controls_disabled;
            }
            AppMsg::ScreensaverActivated => {
                self.state.screensaver =
                    screensaver::State::activate(demo_shared::monotonic_time());
                self.state
                    .modals_state
                    .open(screensaver::ID, &mut self.state.focus)
                    .expect("the screensaver only opens from the base layer");
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

    fn handle_event(&mut self, event: impl TryInto<runtime::Event>) {
        let Ok(event): Result<runtime::Event, _> = event.try_into() else {
            return;
        };
        // The screensaver is dismissed by app policy, before routing: any
        // pointer motion wakes the app. Other events fall through and are
        // absorbed by the modal layer.
        if self.state.modals_state.is_open(screensaver::ID)
            && matches!(&event, runtime::Event::Mouse(mouse) if mouse.kind == MouseKind::Moved)
        {
            self.update(AppMsg::ScreensaverDismissed);
            return;
        }
        let modal_active = !self.state.modals_state.ids().is_empty() || self.ratcn.modal_is_open();
        if !modal_active && self.app_hotkeys(&event) {
            return;
        }
        if let EventResult::Emit(msg) = self.ratcn.handle_event(event, &self.state) {
            self.update(msg);
        }
    }

    /// App-global keys, taking priority over the focused widget.
    fn app_hotkeys(&mut self, event: &runtime::Event) -> bool {
        let runtime::Event::Key(key) = event else {
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

    fn draw(&mut self, frame: &mut Frame, now: Duration) {
        let frame_area = frame.area();
        let theme = self.state.theme();
        frame
            .buffer_mut()
            .set_style(frame_area, Style::default().bg(theme.background));
        let state = &self.state;
        self.ratcn.render(frame, state, &theme, |ctx| {
            ctx.paint(move |ctx| {
                ctx.render_widget(header_bar(ctx.theme), header_area(frame_area));
            });
            for (index, tile_area) in tile_areas(frame_area).into_iter().enumerate() {
                tiles::render(index, ctx, tile_area);
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
            ctx.defer_paint(move |painter, state| {
                painter.render_widget(
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
