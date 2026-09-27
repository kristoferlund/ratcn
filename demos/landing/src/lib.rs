//! Responsive component grid and a larger-app state/message example.
//!
//! The shell owns cross-cutting state. Stateful tile modules own their own
//! `State`, local `Msg`, and `State::update`; `AppMsg` wraps those messages so
//! the one Ratcn runtime can route every component through the same app type.

use std::time::Duration;

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Margin, Rect},
    style::Style,
    text::{Line, Span, Text},
};
use ratcn::{
    Theme, Toast, ToasterState, ToasterWidget,
    runtime::{
        Event, EventResult, FocusState, KeyChord, KeyCode, ModalState, MouseKind, Ratcn, TabWrap,
    },
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
const QUAKE_DOWNLOAD_FRAME: Duration = Duration::from_millis(100);

pub struct App {
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
    agent_settings_state: tiles::tooltip::State,
    payout_state: tiles::payout::State,
    quake_state: tiles::release_pulse::State,
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
    AgentSettings(tiles::tooltip::Msg),
    Payout(tiles::payout::Msg),
    Quake(tiles::release_pulse::Msg),
    Release(tiles::release::Msg),
}

impl App {
    pub fn new() -> Self {
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
            AppMsg::AgentSettings(msg) => self.state.agent_settings_state.update(msg),
            AppMsg::Payout(msg) => self.state.payout_state.update(msg),
            AppMsg::Quake(msg) => self.state.quake_state.update(msg),
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
        // The screensaver is dismissed by app policy, before routing: pointer
        // motion or plain Esc wakes the app. Other events fall through and are
        // absorbed by the modal layer.
        if self.state.modals_state.is_open(screensaver::ID)
            && (matches!(&event, Event::Mouse(mouse) if mouse.kind == MouseKind::Moved)
                || matches!(
                    &event,
                    Event::Key(key) if key.code == KeyCode::Esc && !key.modifiers.any()
                ))
        {
            self.update(AppMsg::ScreensaverDismissed);
            return true;
        }
        let modal_active = self.state.modals_state.top().is_some();
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

    /// The next toast expiry, bounded by the cadence the animated tile needs;
    /// the screensaver runs at its tighter cadence while its snow is visible.
    fn wake(&self) -> Option<Duration> {
        let expiry = self
            .state
            .toasts
            .time_until_next_expiry(demo_shared::monotonic_time());
        let frame = if self.state.modals_state.is_open(screensaver::ID) {
            SCREENSAVER_FRAME
        } else {
            QUAKE_DOWNLOAD_FRAME
        };
        Some(expiry.map_or(frame, |expiry| expiry.min(frame)))
    }

    fn draw(&mut self, buffer: &mut Buffer, area: Rect, theme: &Theme) {
        // The picker lists whatever the terminal currently resolves to, so the
        // frame's theme reaches the state before anything reads it.
        self.state.resolved_theme = *theme;
        let now = demo_shared::monotonic_time();
        let _ = self.state.toasts.prune_expired(now);

        let theme = self.state.theme();
        buffer.set_style(area, Style::default().bg(theme.background));
        let state = &self.state;
        self.ratcn.render_into(buffer, area, state, &theme, |ctx| {
            ctx.paint_widget(header_bar(ctx.theme), header_area(area));
            for (index, tile_area) in tile_areas(area).into_iter().enumerate() {
                tiles::declare(index, ctx, tile_area);
            }
            if state.modals_state.is_open(tiles::release::DIALOG_ID) {
                ctx.modal(
                    tiles::release::DIALOG_ID,
                    tiles::release::dialog(state.release_state.dialog_offset),
                    area,
                );
            }
            if state.modals_state.is_open(screensaver::ID) {
                screensaver::declare(ctx, area, now);
            }
            ctx.defer_paint(move |ctx| {
                let state = ctx.state();
                ctx.widget(
                    ToasterWidget::new(&state.toasts, now).themed(&state.theme()),
                    area,
                );
            });
        });
    }
}

/// Navigation and app shortcuts fit in the grid's existing top padding.
fn header_bar(theme: &Theme) -> Text<'static> {
    let key = Style::default().fg(theme.foreground);
    let hint = Style::default().fg(theme.muted_foreground);
    Text::from(vec![
        Line::from(vec![
            Span::styled("tab/shift+tab", key),
            Span::styled(" controls   ", hint),
            Span::styled("arrows/hjkl", key),
            Span::styled(" items", hint),
        ]),
        Line::from(vec![
            Span::styled("alt+d", key),
            Span::styled(" disable", hint),
            Span::styled("   ", hint),
            Span::styled("alt+s", key),
            Span::styled(" screensaver", hint),
        ]),
    ])
    .centered()
}

/// Two rows within the grid's top padding, clipped for a shorter area.
fn header_area(area: Rect) -> Rect {
    let height = area.height.saturating_sub(1).min(2);
    Rect::new(area.x, area.y + 1, area.width, height)
}

fn tile_areas(area: Rect) -> [Rect; TILE_COUNT] {
    let grid_bounds = area.inner(Margin {
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

/// Rows the tile grid needs at `width`, at its full tile height.
#[must_use]
pub fn grid_height(width: u16) -> u16 {
    let rows = TILE_COUNT.div_ceil(column_count(width) as usize) as u16;
    2 * GRID_PADDING_Y + rows * TILE_HEIGHT + rows.saturating_sub(1) * TILE_GAP
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

#[cfg(test)]
mod tests {
    use demo_shared::Demo as _;
    use ratatui::{Terminal, backend::TestBackend};
    use ratcn::runtime::{KeyEvent, Modifiers};

    use super::*;

    const TEST_WIDTH: u16 = 4 * TILE_WIDTH + 3 * TILE_GAP;
    const TEST_HEIGHT: u16 = 2 * GRID_PADDING_Y + 2 * TILE_HEIGHT + TILE_GAP;

    fn app() -> (App, Terminal<TestBackend>) {
        (
            App::new(),
            Terminal::new(TestBackend::new(TEST_WIDTH, TEST_HEIGHT)).expect("terminal"),
        )
    }

    fn draw(app: &mut App, terminal: &mut Terminal<TestBackend>) {
        terminal
            .draw(|frame| {
                let area = frame.area();
                app.draw(frame.buffer_mut(), area, &Theme::default_dark());
            })
            .expect("draw");
    }

    fn press(app: &mut App, terminal: &mut Terminal<TestBackend>, code: KeyCode) -> bool {
        press_with(app, terminal, code, Modifiers::NONE)
    }

    fn press_with(
        app: &mut App,
        terminal: &mut Terminal<TestBackend>,
        code: KeyCode,
        modifiers: Modifiers,
    ) -> bool {
        draw(app, terminal);
        app.handle_event(Event::Key(KeyEvent { code, modifiers }))
    }

    fn focus_tile(app: &mut App, terminal: &mut Terminal<TestBackend>, number: char) {
        if app.state.focus.path().is_empty() && !app.state.focus.is_none() {
            app.state.focus = FocusState::none();
        }
        assert!(press_with(
            app,
            terminal,
            KeyCode::Char(number),
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
        ));
    }

    fn assert_focus(app: &App, path: &[&str]) {
        assert_eq!(
            app.state.focus,
            FocusState::intent(path.iter().map(|id| (*id).to_owned())),
            "unexpected focus path"
        );
    }

    fn rendered(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn keyboard_hints_fit_the_single_column_layout_without_moving_tiles() {
        let hints = header_bar(&Theme::default_dark());
        assert!(hints.width() <= usize::from(TILE_WIDTH));
        assert_eq!(hints.height(), 2);
        let area = Rect::new(0, 0, TILE_WIDTH, grid_height(TILE_WIDTH));
        assert!(header_area(area).bottom() <= tile_areas(area)[0].y);
    }

    /// The height a host reserves has to be the height the grid lays itself out
    /// in, or a scrolled page would clip its last row or trail empty space.
    #[test]
    fn grid_height_is_the_height_tile_areas_fills_at_full_tile_height() {
        // One column, and the widest the grid ever gets.
        for width in [TILE_WIDTH, 4 * (TILE_WIDTH + TILE_GAP)] {
            let height = grid_height(width);
            let tiles = tile_areas(Rect::new(0, 0, width, height));
            assert!(
                tiles.iter().all(|tile| tile.height == TILE_HEIGHT),
                "at width {width} the tiles are squeezed below their full height"
            );
            let bottom = tiles
                .iter()
                .map(|tile| tile.bottom())
                .max()
                .expect("a tile");
            assert_eq!(
                bottom + GRID_PADDING_Y,
                height,
                "at width {width} the grid does not end where the height says"
            );
        }
    }

    #[test]
    fn every_tile_hotkey_lands_on_its_first_available_control() {
        let (mut app, mut terminal) = app();
        let expected = [
            ("1", &[tiles::themes::ID, "themes"] as &[_]),
            ("2", &[tiles::release::ID, "create_release"] as &[_]),
            ("3", &[tiles::button_variants::ID, "default"] as &[_]),
            ("4", &[tiles::notifications::ID, "notifications"] as &[_]),
            ("5", &[tiles::tooltip::ID, "Change mode"] as &[_]),
            ("6", &[tiles::contributions::ID] as &[_]),
            ("7", &[tiles::payout::ID, "currency"] as &[_]),
            ("8", &[tiles::release_pulse::ID, "assets"] as &[_]),
        ];

        for (number, path) in expected {
            focus_tile(
                &mut app,
                &mut terminal,
                number.chars().next().expect("digit"),
            );
            assert_focus(&app, path);
        }

        let before = app.state.focus.clone();
        assert!(!press(&mut app, &mut terminal, KeyCode::Char('1')));
        assert_eq!(
            app.state.focus, before,
            "plain digits are not focus hotkeys"
        );
        assert!(!press_with(
            &mut app,
            &mut terminal,
            KeyCode::Char('1'),
            Modifiers {
                ctrl: true,
                alt: true,
                ..Modifiers::NONE
            },
        ));
        assert_eq!(
            app.state.focus, before,
            "Ctrl+Alt is not an Alt-only hotkey"
        );
    }

    #[test]
    fn theme_list_keeps_vertical_navigation_internal() {
        let (mut app, mut terminal) = app();
        focus_tile(&mut app, &mut terminal, '1');
        let path = [tiles::themes::ID, "themes"];
        assert_focus(&app, &path);

        assert!(press(&mut app, &mut terminal, KeyCode::Char('j')));
        assert!(press(&mut app, &mut terminal, KeyCode::Char('j')));
        assert_focus(&app, &path);
        assert!(press(&mut app, &mut terminal, KeyCode::Enter));
        assert_ne!(app.state.theme(), Theme::default_dark());

        assert!(press(&mut app, &mut terminal, KeyCode::Char('k')));
        assert_focus(&app, &path);
        assert!(press(&mut app, &mut terminal, KeyCode::Enter));
        assert_eq!(app.state.theme(), Theme::default_dark());

        assert!(!press_with(
            &mut app,
            &mut terminal,
            KeyCode::Char('j'),
            Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
        ));
        assert_focus(&app, &path);
    }

    #[test]
    fn release_tile_has_one_independent_control() {
        let (mut app, mut terminal) = app();
        focus_tile(&mut app, &mut terminal, '2');
        let path = [tiles::release::ID, "create_release"];

        for code in [
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Char('h'),
            KeyCode::Char('j'),
            KeyCode::Char('k'),
            KeyCode::Char('l'),
        ] {
            assert!(!press(&mut app, &mut terminal, code), "{code:?} traversed");
            assert_focus(&app, &path);
        }
        assert!(press(&mut app, &mut terminal, KeyCode::Tab));
        assert_focus(&app, &[tiles::button_variants::ID, "default"]);
        assert!(press(&mut app, &mut terminal, KeyCode::BackTab));
        assert_focus(&app, &path);
    }

    #[test]
    fn button_tile_uses_tab_only_and_reaches_all_five_buttons() {
        let (mut app, mut terminal) = app();
        focus_tile(&mut app, &mut terminal, '3');
        let first = [tiles::button_variants::ID, "default"];

        for code in [
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Char('h'),
            KeyCode::Char('j'),
            KeyCode::Char('k'),
            KeyCode::Char('l'),
        ] {
            assert!(!press(&mut app, &mut terminal, code), "{code:?} traversed");
            assert_focus(&app, &first);
        }

        for id in ["secondary", "outline", "ghost", "destructive"] {
            assert!(press(&mut app, &mut terminal, KeyCode::Tab));
            assert_focus(&app, &[tiles::button_variants::ID, id]);
        }
        assert!(press(&mut app, &mut terminal, KeyCode::Tab));
        assert_focus(&app, &[tiles::notifications::ID, "notifications"]);
        assert!(press(&mut app, &mut terminal, KeyCode::BackTab));
        assert_focus(&app, &[tiles::button_variants::ID, "destructive"]);
    }

    #[test]
    fn notifications_list_keeps_j_and_k_internal_and_rejects_modifiers() {
        let (mut app, mut terminal) = app();
        focus_tile(&mut app, &mut terminal, '4');
        let path = [tiles::notifications::ID, "notifications"];

        for (code, expected) in [
            (KeyCode::Char('j'), "Security alerts"),
            (KeyCode::Char('k'), "Transaction alerts"),
            (KeyCode::Down, "Security alerts"),
        ] {
            draw(&mut app, &mut terminal);
            let EventResult::Emit(AppMsg::Notifications(tiles::notifications::Msg::FocusChanged(
                focused,
            ))) = app
                .ratcn
                .handle_event(Event::Key(KeyEvent::new(code)), &app.state)
            else {
                panic!("{code:?} must move the List cursor, not just consume the key");
            };
            assert_eq!(focused, expected);
            app.update(AppMsg::Notifications(
                tiles::notifications::Msg::FocusChanged(focused),
            ));
            assert_focus(&app, &path);
        }
        assert!(!press_with(
            &mut app,
            &mut terminal,
            KeyCode::Char('k'),
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
        ));
        assert_focus(&app, &path);
    }

    #[test]
    fn cycle_settings_use_tab_between_fields_and_horizontal_keys_for_values() {
        let (mut app, mut terminal) = app();
        focus_tile(&mut app, &mut terminal, '5');
        let change_mode = [tiles::tooltip::ID, "Change mode"];

        assert!(press(&mut app, &mut terminal, KeyCode::Char('l')));
        assert_focus(&app, &change_mode);
        draw(&mut app, &mut terminal);
        assert!(rendered(&terminal).contains("Apply directly"));

        assert!(press(&mut app, &mut terminal, KeyCode::Char('h')));
        draw(&mut app, &mut terminal);
        assert!(rendered(&terminal).contains("Review first"));
        for code in [
            KeyCode::Char('j'),
            KeyCode::Char('k'),
            KeyCode::Up,
            KeyCode::Down,
        ] {
            assert!(
                !press(&mut app, &mut terminal, code),
                "{code:?} traversed fields"
            );
            assert_focus(&app, &change_mode);
        }

        assert!(press(&mut app, &mut terminal, KeyCode::Tab));
        assert_focus(&app, &[tiles::tooltip::ID, "Retry policy"]);
        assert!(press(&mut app, &mut terminal, KeyCode::Char('l')));
        draw(&mut app, &mut terminal);
        assert!(rendered(&terminal).contains("Back off"));
        assert!(press(&mut app, &mut terminal, KeyCode::Tab));
        assert_focus(&app, &[tiles::tooltip::ID, "Updates"]);
        assert!(press(&mut app, &mut terminal, KeyCode::BackTab));
        assert_focus(&app, &[tiles::tooltip::ID, "Retry policy"]);
    }

    #[test]
    fn controls_free_tile_tabs_to_the_next_independent_control() {
        let (mut app, mut terminal) = app();
        focus_tile(&mut app, &mut terminal, '6');
        assert_focus(&app, &[tiles::contributions::ID]);
        assert!(!press(&mut app, &mut terminal, KeyCode::Down));
        assert_focus(&app, &[tiles::contributions::ID]);

        assert!(press(&mut app, &mut terminal, KeyCode::Tab));
        assert_focus(&app, &[tiles::payout::ID, "currency"]);
    }

    #[test]
    fn payout_select_closes_on_first_tab_then_traverses_actions() {
        let (mut app, mut terminal) = app();
        focus_tile(&mut app, &mut terminal, '7');
        let currency = [tiles::payout::ID, "currency"];

        assert!(press(&mut app, &mut terminal, KeyCode::Enter));
        assert!(!press_with(
            &mut app,
            &mut terminal,
            KeyCode::Char('j'),
            Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
        ));
        assert!(press(&mut app, &mut terminal, KeyCode::Char('j')));
        assert_focus(&app, &currency);

        assert!(press(&mut app, &mut terminal, KeyCode::Tab));
        assert_focus(&app, &currency);
        assert!(press(&mut app, &mut terminal, KeyCode::Enter));
        assert!(press(&mut app, &mut terminal, KeyCode::Enter));
        draw(&mut app, &mut terminal);
        assert!(rendered(&terminal).contains("EUR - Euro"));

        assert!(press(&mut app, &mut terminal, KeyCode::Tab));
        assert_focus(&app, &[tiles::payout::ID, "cancel"]);
        assert!(press(&mut app, &mut terminal, KeyCode::Tab));
        assert_focus(&app, &[tiles::payout::ID, "save"]);
        assert!(press(&mut app, &mut terminal, KeyCode::Tab));
        assert_focus(&app, &[tiles::release_pulse::ID, "assets"]);
        assert!(press(&mut app, &mut terminal, KeyCode::BackTab));
        assert_focus(&app, &[tiles::payout::ID, "save"]);
    }

    #[test]
    fn quake_assets_navigate_and_toggle_as_one_list_and_tab_leaves_it() {
        let (mut app, mut terminal) = app();
        focus_tile(&mut app, &mut terminal, '8');
        let path = [tiles::release_pulse::ID, "assets"];

        for (code, expected) in [
            (KeyCode::Char('j'), "Powerup icons"),
            (KeyCode::Down, "Quad damage glow"),
            (KeyCode::Char('k'), "Powerup icons"),
            (KeyCode::Up, "Tournament skins"),
            (KeyCode::End, "Quad damage glow"),
            (KeyCode::Home, "Tournament skins"),
        ] {
            draw(&mut app, &mut terminal);
            let EventResult::Emit(AppMsg::Quake(tiles::release_pulse::Msg::FocusChanged(focused))) =
                app.ratcn
                    .handle_event(Event::Key(KeyEvent::new(code)), &app.state)
            else {
                panic!("{code:?} must move the asset cursor without changing component focus");
            };
            assert_eq!(focused, expected);
            app.update(AppMsg::Quake(tiles::release_pulse::Msg::FocusChanged(
                focused,
            )));
            assert_focus(&app, &path);
        }
        for code in [KeyCode::Enter, KeyCode::Char(' ')] {
            draw(&mut app, &mut terminal);
            let EventResult::Emit(AppMsg::Quake(tiles::release_pulse::Msg::Toggled(value))) = app
                .ratcn
                .handle_event(Event::Key(KeyEvent::new(code)), &app.state)
            else {
                panic!("{code:?} must toggle the focused asset");
            };
            assert_eq!(value, "Tournament skins");
            app.update(AppMsg::Quake(tiles::release_pulse::Msg::Toggled(value)));
            assert_focus(&app, &path);
        }
        assert!(press(&mut app, &mut terminal, KeyCode::Char('k')));
        assert_focus(&app, &path); // At the first item, navigation stays in the list.
        assert!(press(&mut app, &mut terminal, KeyCode::Tab));
        assert_focus(&app, &[tiles::themes::ID, "themes"]);
        assert!(press(&mut app, &mut terminal, KeyCode::BackTab));
        assert_focus(&app, &path);
        assert!(press(&mut app, &mut terminal, KeyCode::BackTab));
        assert_focus(&app, &[tiles::payout::ID, "save"]);
    }

    #[test]
    fn disabled_controls_are_skipped_by_traversal_and_focus_hotkeys() {
        let (mut app, mut terminal) = app();
        focus_tile(&mut app, &mut terminal, '3');
        assert!(press_with(
            &mut app,
            &mut terminal,
            KeyCode::Char('d'),
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
        ));

        assert!(press(&mut app, &mut terminal, KeyCode::Tab));
        assert_focus(&app, &[tiles::contributions::ID]);
        focus_tile(&mut app, &mut terminal, '6');
        let before = app.state.focus.clone();
        assert!(!press_with(
            &mut app,
            &mut terminal,
            KeyCode::Char('3'),
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
        ));
        assert_eq!(
            app.state.focus, before,
            "disabled tile accepted its focus hotkey"
        );
        assert!(press(&mut app, &mut terminal, KeyCode::BackTab));
        assert_eq!(
            app.state.focus, before,
            "disabled controls entered traversal"
        );
    }

    #[test]
    fn plain_escape_dismisses_screensaver_and_restores_focus() {
        let (mut app, mut terminal) = app();
        focus_tile(&mut app, &mut terminal, '3');
        let return_focus = app.state.focus.clone();
        assert!(press_with(
            &mut app,
            &mut terminal,
            KeyCode::Char('s'),
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
        ));
        assert!(app.state.modals_state.is_open(screensaver::ID));

        let _ = press_with(
            &mut app,
            &mut terminal,
            KeyCode::Esc,
            Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
        );
        assert!(
            app.state.modals_state.is_open(screensaver::ID),
            "modified Esc must not dismiss"
        );

        assert!(press(&mut app, &mut terminal, KeyCode::Esc));
        assert!(!app.state.modals_state.is_open(screensaver::ID));
        assert_eq!(app.state.focus, return_focus);
    }
}
