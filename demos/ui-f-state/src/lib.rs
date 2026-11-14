//! `ui = f(state)`: a short film about what ratcn is built on, that is itself
//! a ratcn app.
//!
//! The film is the idea it explains. Its whole state is one number, the film
//! clock, and every frame is painted from that number alone: no scene keeps
//! anything from the frame before. That is why it can be paused, scrubbed and
//! replayed from the timeline for free, and why the soundtrack — synthesized
//! from the same clock — stays with it.
//!
//! When the film ends the screen is handed over: the counter it has been
//! drawing with paint-only widgets is declared through the engine instead,
//! and the viewer's own mouse and keyboard drive the loop.

use std::time::Duration;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    widgets::Widget,
};
#[cfg(not(target_arch = "wasm32"))]
use ratcn::terminal::TerminalProgress;
use ratcn::{
    Button, ButtonSize, Dialog, Theme, Toast, ToastEntry, ToasterState, ToasterWidget, Tooltip,
    TooltipSide,
    color::resolve_rgb,
    runtime::{
        DeclareCtx, Event, EventResult, FocusState, KeyCode, ModalState, PointerShape, Ratcn,
    },
};

mod canvas;
mod figures;
mod hud;
mod scenes;
// The soundtrack reads cues the picture has no use for, and it is native only.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
mod score;
#[cfg(not(target_arch = "wasm32"))]
mod sound;
mod transport;

use canvas::{Frame, ink, mix};
use hud::{BLACK, Panel, WHITE, fg};
use transport::Transport;

/// The largest scene the film is laid out for. A bigger terminal gets the
/// scene in its middle and the snow everywhere.
const STAGE: (u16, u16) = (132, 42);
/// The smallest it can be read at.
const SMALLEST: (u16, u16) = (80, 24);
/// How long the film stands still at the end of each section, unless told
/// otherwise: long enough to finish reading.
pub const PAUSE: f64 = 2.0;
/// The film runs at thirty frames a second: a terminal is not a monitor.
const FRAME: Duration = Duration::from_millis(33);
mod ids {
    pub const COUNTER: &str = "counter";
    pub const BUTTON: &str = "button";
    pub const REPLAY: &str = "replay";
    pub const DIALOG: &str = "dialog";
}

/// The film clock: a position, and when it was last set running.
///
/// It counts the pauses at the end of each section as well as the film, so
/// the film's own position is one step away: [`score::film_at`].
struct Clock {
    /// The clock's reading when it was last started, stopped, or moved.
    position: f64,
    /// The host clock's reading at that moment. [`None`] while paused.
    running_since: Option<Duration>,
    /// Seconds the film stands still at the end of each section.
    pause: f64,
}

impl Clock {
    /// Where the film is.
    fn at(&self, now: Duration) -> f64 {
        score::film_at(self.wall(now), self.pause)
    }

    /// The clock itself, which does not stand still when the film does. What
    /// only breathes is animated by this one.
    fn wall(&self, now: Duration) -> f64 {
        self.position
            + self
                .running_since
                .map_or(0.0, |since| now.saturating_sub(since).as_secs_f64())
    }

    /// Move the film to `film`.
    fn seek(&mut self, film: f64, now: Duration) {
        self.position = score::play_at(film.max(0.0), self.pause);
        self.running_since = self.running_since.map(|_| now);
    }

    fn toggle(&mut self, now: Duration) {
        self.position = self.wall(now);
        self.running_since = match self.running_since {
            Some(_) => None,
            None => Some(now),
        };
    }
}

/// What the engine draws from: the counter once it is the viewer's, and the
/// same shape filled in from the script while the film shows off its layers.
#[derive(Default)]
struct Live {
    count: u32,
    focus: FocusState,
    modals: ModalState,
    /// The script holds the tooltip open, whatever the pointer is doing.
    hint: bool,
    /// The film clock at the viewer's last press.
    pressed_at: Option<f64>,
    /// One toast per press, saying what the press did.
    toasts: ToasterState<'static>,
}

#[derive(Clone)]
enum Msg {
    Focus(FocusState),
    Increment,
    Replay,
    /// Pause the film, or set it running again.
    Toggle,
    /// Move the film to this many seconds in.
    Seek(f64),
    TransportFocus(FocusState),
    /// A scripted dialog's buttons: nothing presses them.
    Nothing,
}

pub struct App {
    clock: Clock,
    /// The scene, and the snow behind it.
    stage: Frame,
    sky: Frame,
    live: Live,
    ratcn: Ratcn<Live, Msg>,
    /// The transport along the bottom row, and the engine it is declared
    /// through. It has an engine of its own so that it stays in reach while
    /// the film's scripted dialog holds the scene's.
    transport: Transport,
    controls: Ratcn<Transport, Msg>,
    #[cfg(not(target_arch = "wasm32"))]
    sound: sound::Sound,
}

impl App {
    /// The film, from the top. `sound` plays its soundtrack, and the film
    /// stands still for `pause` seconds at the end of each section.
    pub fn new(sound: bool, pause: f64) -> Self {
        #[cfg(target_arch = "wasm32")]
        let _ = sound;
        Self {
            clock: Clock {
                position: 0.0,
                running_since: Some(demo_shared::monotonic_time()),
                pause,
            },
            stage: Frame::new(SMALLEST.0, SMALLEST.1),
            sky: Frame::new(SMALLEST.0, SMALLEST.1),
            live: Live {
                focus: FocusState::intent([ids::COUNTER, ids::BUTTON]),
                ..Live::default()
            },
            ratcn: Ratcn::new()
                .focus(|live: &Live| &live.focus, Msg::Focus)
                .modals(|live: &Live| &live.modals),
            transport: Transport::new(),
            controls: Ratcn::new().focus(
                |transport: &Transport| &transport.focus,
                Msg::TransportFocus,
            ),
            #[cfg(not(target_arch = "wasm32"))]
            sound: sound::Sound::new(sound, pause),
        }
    }

    /// The film stopped at `position`, silent: one frame to look at.
    pub fn still(position: f64) -> Self {
        let mut app = Self::new(false, PAUSE);
        app.clock.seek(position, Duration::ZERO);
        app.clock.running_since = None;
        app
    }

    /// Bring the transport's state up to the film: it is drawn from, and its
    /// keys are read against, where the film is now.
    fn sync_transport(&mut self, now: Duration) {
        self.transport.position = self.clock.at(now);
        self.transport.running = self.clock.running_since.is_some();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.transport.sound = !self.sound.muted();
        }
    }

    fn update(&mut self, msg: Msg, now: Duration) {
        match msg {
            Msg::Focus(focus) => self.live.focus = focus,
            Msg::Increment => {
                self.live.count = (self.live.count + 1) % (hud::FULL + 1);
                self.live.pressed_at = Some(self.clock.at(now));
                self.live.toasts.push(
                    Toast::info("Msg::Increment")
                        .with_description(format!("update: count = {}", self.live.count)),
                    now,
                );
            }
            Msg::Replay => {
                self.clock.position = 0.0;
                self.clock.running_since = Some(now);
                self.live.count = 0;
                self.live.pressed_at = None;
                self.live.toasts = ToasterState::new();
            }
            Msg::Toggle => self.clock.toggle(now),
            Msg::Seek(position) => self.clock.seek(position, now),
            Msg::TransportFocus(focus) => self.transport.focus = focus,
            Msg::Nothing => {}
        }
    }
}

/// The counter, declared through the engine: the panel's paint, a real
/// button under a real tooltip, and a dialog when the state says one is open.
fn declare(ctx: &mut DeclareCtx<'_, Live, Msg>, panel: Rect, replay: Option<Rect>, stage: Rect) {
    let count = ctx.state().count;
    let theme = *ctx.theme;
    ctx.paint(move |ctx| {
        ctx.with_buffer(panel, |area, buffer| {
            Panel {
                count,
                theme: &theme,
            }
            .render(area, buffer)
        });
    });
    ctx.component(
        ids::COUNTER,
        Tooltip::new("Emits Msg::Increment")
            .side(TooltipSide::Right)
            // Open when the script says so. The live button answers a press
            // with a toast instead.
            .open_when(|live: &Live, _hovered| live.hint)
            .trigger(|ctx| {
                let area = ctx.area();
                ctx.component(
                    ids::BUTTON,
                    Button::new(hud::BUTTON_LABEL)
                        .size(ButtonSize::Large)
                        .on_press(|| Msg::Increment),
                    area,
                );
            }),
        Panel::button(panel),
    );
    if let Some(replay) = replay {
        ctx.component(
            ids::REPLAY,
            Button::new("Replay").secondary().on_press(|| Msg::Replay),
            replay,
        );
    }
    if ctx.state().modals.is_open(ids::DIALOG) {
        ctx.modal(
            ids::DIALOG,
            Dialog::new()
                .title("Reset the count?")
                .description("The dialog is state as well: open because the app says so.")
                .outer_width(46)
                .on_dismiss(|| Msg::Nothing)
                .action("reset", Button::new("Reset").on_press(|| Msg::Nothing))
                .action(
                    "cancel",
                    Button::new("Cancel").secondary().on_press(|| Msg::Nothing),
                ),
            stage,
        );
    }
}

impl demo_shared::Demo for App {
    fn wake(&self) -> Option<Duration> {
        self.clock.running_since.map(|_| FRAME)
    }

    fn handle_event(&mut self, event: Event) -> bool {
        let now = demo_shared::monotonic_time();
        let t = self.clock.at(now);

        // Once the film is over the counter is live. It gets the keys first;
        // the pointer goes to the transport first, which may be holding a drag
        // that has wandered off its row.
        let live = t >= score::END;
        let keys = matches!(event, Event::Key(_));
        self.sync_transport(now);
        let mut result = EventResult::Ignored;
        if live && keys {
            result = self.ratcn.handle_event(event.clone(), &self.live);
        }
        if matches!(result, EventResult::Ignored) {
            result = self.controls.handle_event(event.clone(), &self.transport);
        }
        if matches!(result, EventResult::Ignored) && live && !keys {
            result = self.ratcn.handle_event(event.clone(), &self.live);
        }
        match result {
            EventResult::Emit(msg) => {
                self.update(msg, now);
                return true;
            }
            EventResult::Consumed => return true,
            EventResult::Ignored => {}
        }

        // What nothing claimed is a hotkey.
        match event {
            Event::Key(key) if matches!(key.code, KeyCode::Char('r' | 'R')) => {
                self.update(Msg::Replay, now);
            }
            #[cfg(not(target_arch = "wasm32"))]
            Event::Key(key) if matches!(key.code, KeyCode::Char('m' | 'M')) => {
                self.sound.toggle_mute();
            }
            _ => return false,
        }
        true
    }

    fn pointer_shape(&self) -> PointerShape {
        match self.controls.pointer_shape() {
            PointerShape::Default => self.ratcn.pointer_shape(),
            shape => shape,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn terminal_title(&self) -> Option<&str> {
        Some("ui = f(state)")
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn terminal_progress(&self) -> TerminalProgress {
        let t = self.clock.at(demo_shared::monotonic_time());
        if t < score::END {
            TerminalProgress::Normal((t / score::END * 100.0) as u8)
        } else {
            TerminalProgress::Hidden
        }
    }

    fn draw(&mut self, buffer: &mut Buffer, area: Rect, theme: &Theme) {
        let now = demo_shared::monotonic_time();
        let t = self.clock.at(now);
        // The film may be standing still; the snow and the scarves are not,
        // and neither is the music.
        let wall = self.clock.wall(now);
        #[cfg(not(target_arch = "wasm32"))]
        self.sound
            .follow(wall, self.clock.running_since.is_some(), now);

        let paper = resolve_rgb(theme.background).map_or(BLACK, |(r, g, b)| [r, g, b]);
        buffer.set_style(area, Style::default().bg(ink(paper)).fg(ink(WHITE)));
        if area.width < SMALLEST.0 || area.height < SMALLEST.1 {
            let note = format!("ui = f(state) needs {}×{} cells", SMALLEST.0, SMALLEST.1);
            buffer.set_string(area.x, area.y, note, fg(WHITE));
            return;
        }

        // The scene sits in the middle of the terminal, above the timeline.
        let size = (area.width.min(STAGE.0), (area.height - 1).min(STAGE.1));
        let stage = Rect::new(
            area.x + (area.width - size.0) / 2,
            area.y + (area.height - 1 - size.1) / 2,
            size.0,
            size.1,
        );
        if (self.stage.w, self.stage.h) != (i32::from(size.0), i32::from(size.1)) {
            self.stage = Frame::new(size.0, size.1);
        }
        if (self.sky.w, self.sky.h) != (i32::from(area.width), i32::from(area.height)) {
            self.sky = Frame::new(area.width, area.height);
        }

        let pressed = self.live.pressed_at.map(|at| t - at);
        self.stage.clear();
        self.stage.wall = wall;
        scenes::render(&mut self.stage, t, pressed, theme);

        self.sky.clear();
        hud::snow(&mut self.sky, wall, self.stage.snow);
        self.sky.flash = self.stage.flash;
        self.sky.fade = self.stage.fade;
        self.sky.invert = self.stage.invert;
        self.sky.surround = self.stage.surround;
        let seed = (wall * 30.0) as u64;
        self.sky.compose(buffer, area, paper, false, seed);
        self.stage.compose(buffer, stage, paper, true, seed);

        // The engine's turn: scripted while the film shows its layers, and
        // the viewer's once the film is over.
        let (w, h) = (i32::from(size.0), i32::from(size.1));
        let within =
            |rect: Rect| Rect::new(stage.x + rect.x, stage.y + rect.y, rect.width, rect.height);
        if let Some(script) = scenes::powers::layers(t, w, h) {
            let mut scripted = Live {
                count: hud::FULL,
                hint: script.hint,
                ..Live::default()
            };
            if script.modal {
                scripted
                    .modals
                    .open(ids::DIALOG, &mut scripted.focus)
                    .expect("a fresh stack has nothing open under the dialog");
            }
            let panel = within(scenes::powers::panel_rect(w, h));
            self.ratcn
                .render_into(buffer, stage, &scripted, theme, |ctx| {
                    declare(ctx, panel, None, stage);
                });
            if let Some(age) = script.toast {
                let toast = Toast::success("Saved").with_description("count = 12");
                ToasterWidget::from_entries(
                    &[ToastEntry::new(toast, Duration::ZERO)],
                    Duration::from_secs_f64(age),
                )
                .themed(theme)
                .render(stage, buffer);
            }
            let (x, y) = (
                stage.x + script.pointer.0 as u16,
                stage.y + script.pointer.1 as u16,
            );
            if let Some(cell) = buffer.cell_mut((x, y)) {
                cell.set_symbol("◤");
                cell.set_style(Style::default().fg(ink(WHITE)).add_modifier(Modifier::BOLD));
            }
        } else if t >= score::END {
            let panel = within(scenes::finale::panel_rect(w, h));
            let replay = within(scenes::finale::replay_rect(w, h));
            self.ratcn
                .render_into(buffer, stage, &self.live, theme, |ctx| {
                    declare(ctx, panel, Some(replay), stage);
                });
            self.live.toasts.prune_expired(now);
            ToasterWidget::new(&self.live.toasts, now)
                .themed(theme)
                .render(stage, buffer);
        }

        // A paused film says so.
        if self.clock.running_since.is_none() {
            let note = " PAUSED ";
            buffer.set_string(
                stage.x + (stage.width - note.len() as u16) / 2,
                stage.y,
                note,
                Style::default()
                    .fg(ink(BLACK))
                    .bg(ink(mix(WHITE, BLACK, 0.1))),
            );
        }
        self.sync_transport(now);
        let row = Rect::new(area.x, area.bottom() - 1, area.width, 1);
        buffer.set_style(row, Style::default().bg(ink(BLACK)));
        self.controls
            .render_into(buffer, row, &self.transport, theme, |ctx| {
                transport::declare(ctx, row);
            });
    }
}

#[cfg(test)]
mod tests {
    use demo_shared::Demo as _;
    use ratcn::runtime::{Modifiers, MouseButton, MouseEvent, MouseKind};

    use super::*;

    fn frame(position: f64, width: u16, height: u16) -> Buffer {
        let area = Rect::new(0, 0, width, height);
        let mut buffer = Buffer::empty(area);
        App::still(position).draw(&mut buffer, area, &Theme::default_dark());
        buffer
    }

    /// The claim the film makes about itself: a frame depends on the clock
    /// and on nothing that came before it. Scrubbing relies on it.
    #[test]
    fn a_frame_is_a_function_of_the_clock_alone() {
        let area = Rect::new(0, 0, 120, 36);
        let theme = Theme::default_dark();
        let mut played = App::still(0.0);
        let mut seen = Buffer::empty(area);
        // One app drawn at every position in turn, as a viewing would.
        for tenth in 0..=900 {
            played.clock.seek(f64::from(tenth) / 10.0, Duration::ZERO);
            seen.reset();
            played.draw(&mut seen, area, &theme);
            if tenth % 50 == 7 {
                assert_eq!(
                    seen,
                    frame(f64::from(tenth) / 10.0, 120, 36),
                    "the frame at {} depends on the frames before it",
                    f64::from(tenth) / 10.0
                );
            }
        }
    }

    /// Every scene has to survive the smallest terminal and the largest stage.
    #[test]
    fn every_moment_draws_at_every_size() {
        for (width, height) in [(80, 24), (100, 30), (132, 43), (270, 75)] {
            for half in 0..=180 {
                frame(f64::from(half) / 2.0, width, height);
            }
        }
    }

    #[test]
    fn the_screen_is_one_repaint_behind_the_state() {
        let length = score::TURNS[1] - score::TURNS[0];
        let written = score::TURNS[0] + (score::UPDATE_AT + 0.01) * length;
        assert_eq!(score::state_count(written), score::FIRST_COUNT + 1);
        assert_eq!(
            score::shown_count(written),
            score::FIRST_COUNT,
            "update has run, and the frame showing it has not been painted yet"
        );
        assert_eq!(score::shown_count(score::POWERS), hud::FULL);
    }

    fn mouse(kind: MouseKind, column: u16, row: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: Modifiers::NONE,
        })
    }

    /// The transport is components, not a hit test in the event handler: a
    /// press on the scrubber reaches the film as a message, and a drag that
    /// leaves the row is still the scrubber's.
    #[test]
    fn the_scrubber_moves_the_film_through_update() {
        let area = Rect::new(0, 0, 120, 36);
        let mut app = App::still(10.0);
        let mut buffer = Buffer::empty(area);
        app.draw(&mut buffer, area, &Theme::default_dark());
        let now = demo_shared::monotonic_time;

        let end = (0..area.width)
            .rev()
            .find(|column| ["─", "┿"].contains(&buffer[(*column, 35)].symbol()))
            .expect("the scrubber is on the bottom row");
        assert!(app.handle_event(mouse(MouseKind::Down(MouseButton::Left), end, 35)));
        assert!(
            (app.clock.at(now()) - score::END).abs() < 0.01,
            "a press on the end of the bar is the film's last moment"
        );
        assert!(app.handle_event(mouse(MouseKind::Drag(MouseButton::Left), 0, 20)));
        assert!(
            app.clock.at(now()) < 0.01,
            "the drag left the row and still moved the film"
        );

        assert!(app.handle_event(Event::Key(KeyCode::Right.into())));
        assert!((app.clock.at(now()) - 5.0).abs() < 0.01);
        assert!(app.handle_event(Event::Key(KeyCode::Char(' ').into())));
        assert!(app.clock.running_since.is_some(), "Space set it running");
    }

    /// At the end of a section the film stands still for the pause and the
    /// clock the snow falls by does not, and then the film carries on from
    /// where it stood.
    #[test]
    fn the_film_pauses_between_sections_and_stays_alive_meanwhile() {
        let hold = score::HOLDS[2];
        let reached = score::play_at(hold, PAUSE);
        assert!((score::film_at(reached + PAUSE / 2.0, PAUSE) - hold).abs() < 1e-9);
        assert!((score::film_at(reached + PAUSE + 1.0, PAUSE) - (hold + 1.0)).abs() < 1e-9);
        for tenth in 0..=850 {
            let film = f64::from(tenth) / 10.0;
            let round_trip = score::film_at(score::play_at(film, PAUSE), PAUSE);
            assert!(
                (round_trip - film).abs() < 1e-9,
                "{film} came back as {round_trip}"
            );
        }
        assert!(
            (score::film_at(40.0, 0.0) - 40.0).abs() < 1e-9,
            "no pause is the film straight through"
        );
    }

    /// The live counter at the end is a real one: the button's message is the
    /// only thing that changes the count.
    #[test]
    fn pressing_the_live_button_goes_through_update() {
        let area = Rect::new(0, 0, 120, 36);
        let mut app = App::still(score::END + 1.0);
        let mut buffer = Buffer::empty(area);
        app.draw(&mut buffer, area, &Theme::default_dark());

        assert!(app.handle_event(Event::Key(KeyCode::Enter.into())));
        assert_eq!(app.live.count, 1);
        app.draw(&mut buffer, area, &Theme::default_dark());
        let text: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(
            text.contains("Msg::Increment"),
            "the press is answered with a toast naming the message"
        );
    }
}
