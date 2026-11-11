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
    Button, ButtonSize, Dialog, Theme, Toast, ToastEntry, ToasterWidget, Tooltip, TooltipSide,
    color::resolve_rgb,
    runtime::{
        DeclareCtx, Event, EventResult, FocusState, KeyCode, ModalState, MouseButton, MouseEvent,
        MouseKind, PointerShape, Ratcn,
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

use canvas::{Frame, ink, mix};
use hud::{BLACK, DIM, GREY, Panel, WHITE, fg};

/// The largest scene the film is laid out for. A bigger terminal gets the
/// scene in its middle and the snow everywhere.
const STAGE: (u16, u16) = (132, 42);
/// The smallest it can be read at.
const SMALLEST: (u16, u16) = (80, 24);
/// The film runs at thirty frames a second: a terminal is not a monitor.
const FRAME: Duration = Duration::from_millis(33);
/// How far an arrow key moves the film.
const SKIP: f64 = 5.0;

mod ids {
    pub const COUNTER: &str = "counter";
    pub const BUTTON: &str = "button";
    pub const REPLAY: &str = "replay";
    pub const DIALOG: &str = "dialog";
}

/// The film clock: a position, and when it was last set running.
struct Clock {
    /// The film's position when it was last started, stopped, or moved.
    position: f64,
    /// The host clock's reading at that moment. [`None`] while paused.
    running_since: Option<Duration>,
}

impl Clock {
    fn at(&self, now: Duration) -> f64 {
        self.position
            + self
                .running_since
                .map_or(0.0, |since| now.saturating_sub(since).as_secs_f64())
    }

    fn seek(&mut self, position: f64, now: Duration) {
        self.position = position.max(0.0);
        self.running_since = self.running_since.map(|_| now);
    }

    fn toggle(&mut self, now: Duration) {
        self.position = self.at(now);
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
}

#[derive(Clone)]
enum Msg {
    Focus(FocusState),
    Increment,
    Replay,
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
    /// Where the last frame put the timeline: its row, first column, width.
    timeline: (u16, u16, u16),
    #[cfg(not(target_arch = "wasm32"))]
    sound: sound::Sound,
}

impl App {
    /// The film, from the top. `sound` plays its soundtrack.
    pub fn new(sound: bool) -> Self {
        #[cfg(target_arch = "wasm32")]
        let _ = sound;
        Self {
            clock: Clock {
                position: 0.0,
                running_since: Some(demo_shared::monotonic_time()),
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
            timeline: (0, 0, 0),
            #[cfg(not(target_arch = "wasm32"))]
            sound: sound::Sound::new(sound),
        }
    }

    /// The film stopped at `position`, silent: one frame to look at.
    pub fn still(position: f64) -> Self {
        let mut app = Self::new(false);
        app.clock = Clock {
            position,
            running_since: None,
        };
        app
    }

    fn update(&mut self, msg: Msg, now: Duration) {
        match msg {
            Msg::Focus(focus) => self.live.focus = focus,
            Msg::Increment => {
                self.live.count = (self.live.count + 1) % (hud::FULL + 1);
                self.live.pressed_at = Some(self.clock.at(now));
            }
            Msg::Replay => {
                self.clock = Clock {
                    position: 0.0,
                    running_since: Some(now),
                };
                self.live.count = 0;
                self.live.pressed_at = None;
            }
            Msg::Nothing => {}
        }
    }

    /// The timeline: where the film is, and how to move it.
    fn draw_timeline(&mut self, buffer: &mut Buffer, area: Rect, t: f64) {
        let row = area.bottom() - 1;
        buffer.set_style(
            Rect::new(area.x, row, area.width, 1),
            Style::default().bg(ink(BLACK)),
        );
        let shown = t.min(score::END);
        let icon = if self.clock.running_since.is_some() {
            "▶"
        } else {
            "‖"
        };
        let clock = format!(
            " {icon} {:02}:{:02} / {:02}:{:02} ",
            shown as u32 / 60,
            shown as u32 % 60,
            score::END as u32 / 60,
            score::END as u32 % 60
        );
        buffer.set_string(area.x, row, &clock, fg(WHITE));
        #[cfg(not(target_arch = "wasm32"))]
        let sound = if self.sound.muted() { "off" } else { "on" };
        #[cfg(target_arch = "wasm32")]
        let sound = "off";
        let keys = format!(" SPACE pause  ←→ scrub  R replay  M sound {sound}  ^C quit ");
        // The keys are shown while they are news, and whenever the film is not
        // running; the rest of the time the bar has the room.
        let idle = self.clock.running_since.is_none() || !(8.0..score::END).contains(&t);
        let keys_width = if idle && area.width >= 110 {
            keys.chars().count() as u16
        } else {
            0
        };
        if keys_width > 0 {
            buffer.set_string(area.right() - keys_width, row, &keys, fg(DIM));
        }
        let chapter = scenes::CHAPTERS
            .iter()
            .rev()
            .find(|(start, _)| t >= *start)
            .map_or("", |(_, name)| name);
        let chapter_width = 24.min(area.width / 5);
        let first = area.x + clock.chars().count() as u16;
        let width = area
            .width
            .saturating_sub(first - area.x + keys_width + chapter_width + 2);
        self.timeline = (row, first, width);
        let head = (shown / score::END * f64::from(width.saturating_sub(1))) as u16;
        for column in 0..width {
            let at = f64::from(column) / f64::from(width.max(2) - 1) * score::END;
            let starts_chapter = scenes::CHAPTERS.iter().any(|(start, _)| {
                (at - start).abs() < score::END / f64::from(width.max(1)) / 2.0 && *start > 0.0
            });
            let (glyph, color) = match column {
                column if column == head => ("●", WHITE),
                column if column < head => ("━", GREY),
                _ if starts_chapter => ("┿", GREY),
                _ => ("─", DIM),
            };
            buffer.set_string(first + column, row, glyph, fg(color));
        }
        buffer.set_string(
            first + width + 1,
            row,
            chapter
                .chars()
                .take(usize::from(chapter_width))
                .collect::<String>(),
            fg(GREY),
        );
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
            .open_when(|live: &Live, hovered| live.hint || hovered)
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

        // The timeline takes a press or a drag anywhere along it.
        let (row, first, width) = self.timeline;
        if let Event::Mouse(MouseEvent {
            kind: MouseKind::Down(MouseButton::Left) | MouseKind::Drag(MouseButton::Left),
            column,
            row: at,
            ..
        }) = event
            && at == row
            && width > 1
            && column >= first
        {
            let along = f64::from((column - first).min(width - 1)) / f64::from(width - 1);
            self.clock.seek(along * score::END, now);
            return true;
        }

        // Once the film is over the counter is live, and gets every event
        // first.
        if t >= score::END {
            match self.ratcn.handle_event(event.clone(), &self.live) {
                EventResult::Emit(msg) => {
                    self.update(msg, now);
                    return true;
                }
                EventResult::Consumed => return true,
                EventResult::Ignored => {}
            }
        }

        let Event::Key(key) = event else {
            return false;
        };
        match key.code {
            KeyCode::Char(' ') => self.clock.toggle(now),
            KeyCode::Left => self.clock.seek(t.min(score::END) - SKIP, now),
            KeyCode::Right => self.clock.seek((t + SKIP).min(score::END), now),
            KeyCode::Char('r' | 'R') | KeyCode::Home => self.update(Msg::Replay, now),
            #[cfg(not(target_arch = "wasm32"))]
            KeyCode::Char('m' | 'M') => self.sound.toggle_mute(),
            _ => return false,
        }
        true
    }

    fn pointer_shape(&self) -> PointerShape {
        self.ratcn.pointer_shape()
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
        #[cfg(not(target_arch = "wasm32"))]
        self.sound
            .follow(t, self.clock.running_since.is_some(), now);

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
        scenes::render(&mut self.stage, t, pressed, theme);

        self.sky.clear();
        hud::snow(&mut self.sky, t, self.stage.snow);
        self.sky.flash = self.stage.flash;
        self.sky.fade = self.stage.fade;
        self.sky.invert = self.stage.invert;
        self.sky.surround = self.stage.surround;
        let seed = (t * 30.0) as u64;
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
        self.draw_timeline(buffer, area, t);
    }
}

#[cfg(test)]
mod tests {
    use demo_shared::Demo as _;

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
            played.clock.position = f64::from(tenth) / 10.0;
            seen.reset();
            played.draw(&mut seen, area, &theme);
            if tenth % 50 == 7 {
                assert_eq!(
                    seen,
                    frame(played.clock.position, 120, 36),
                    "the frame at {} depends on the frames before it",
                    played.clock.position
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
    }
}
