//! The host every demo runs on: one [`Demo`] implementation, two platforms.
//!
//! A demo says what it paints, what it does with an event, and when the clock
//! should wake it. [`run`] owns everything else — the terminal or canvas, the
//! event listeners, and the redraw policy.
//!
//! # Frames happen for a reason
//!
//! The host renders when something says the last frame is stale: the first
//! frame, an event the demo reports as handled, a [`Demo::wake`] deadline, or a
//! resize. Nothing else. An idle demo costs one blocked read natively, and
//! nothing at all in the browser.

use std::{io, time::Duration};

#[cfg(not(target_arch = "wasm32"))]
use ratatui::crossterm::event::Event as CrosstermEvent;
use ratatui::{Frame, Terminal, backend::Backend, style::Color};
use ratcn::runtime::Event;

/// The shortest wait worth honoring: a wake sooner than the display refreshes
/// cannot show anything new. [`Demo::wake`] values this small or smaller mean
/// "keep rendering", which is how a demo animates.
pub const ANIMATION_FRAME: Duration = Duration::from_millis(16);

/// One demo, in the shape a host can drive.
pub trait Demo {
    /// Whether the demo reads input at all.
    ///
    /// A demo that only paints leaves this off: no browser listeners, and a
    /// native terminal keeps its own mouse — with capture on, terminals
    /// usually stop offering text selection.
    const INPUT: bool = true;

    /// Whether the host delivers clipboard pastes — bracketed paste natively,
    /// the browser's `paste` event on the web.
    const PASTE: bool = false;

    /// The color behind the grid.
    ///
    /// In the browser this is the canvas padding: the terminal grid covers a
    /// whole number of cells, so a few pixels are usually left over along the
    /// right and bottom edges.
    fn background(&self) -> Color;

    /// Paint one frame.
    fn draw(&mut self, frame: &mut Frame);

    /// Route one event, returning whether the screen now needs redrawing.
    ///
    /// [`EventResult::Ignored`](ratcn::runtime::EventResult::Ignored) is the
    /// runtime's answer for an event nothing reacted to, which makes it the
    /// answer to this question too: anything else changed what the next frame
    /// should look like.
    fn handle_event(&mut self, event: Event) -> bool {
        let _ = event;
        false
    }

    /// How long the host may wait, with no event arriving at all, before it has
    /// to render again. [`None`] waits indefinitely.
    ///
    /// This is the only way a demo whose screen depends on the clock — a toast
    /// that expires, an animation, a background fetch still in flight — gets
    /// the frame that no input event is going to ask for.
    ///
    /// It is a deadline, not a tick: a demo reads the clock in [`draw`](Self::draw)
    /// and works from that reading, so any frame serves the deadline. Natively
    /// that is what keeps sustained input from starving it — a wait answered by
    /// an event never runs out, but it does draw.
    fn wake(&self) -> Option<Duration> {
        None
    }
}

/// Run `demo` until it quits (natively) or forever (in the browser).
///
/// # Errors
///
/// Returns an I/O error if the terminal or the browser canvas cannot be set up,
/// and natively if drawing or reading input fails.
#[cfg(not(target_arch = "wasm32"))]
pub fn run<D: Demo + 'static>(mut demo: D) -> io::Result<()> {
    ratatui::run(|terminal| {
        let modes = ratcn::crossterm::InputModes::new();
        let modes = if D::INPUT {
            modes.mouse_capture()
        } else {
            modes
        };
        let modes = if D::PASTE {
            modes.bracketed_paste()
        } else {
            modes
        };
        // RAII: the modes are restored on every exit path, `?` and panic alike.
        let _input_modes = modes.enable()?;

        drive(&mut demo, terminal, &mut TerminalEvents)
    })
}

/// Where the native host's waiting happens.
///
/// A demo waits on the terminal. The tests drive the same loop from a scripted
/// source, which is also how they see the waits the host asks for.
#[cfg(not(target_arch = "wasm32"))]
trait Events {
    /// Wait for the next event, for at most `timeout` — indefinitely when it is
    /// [`None`]. `Ok(None)` means the wait ran out with nothing to route.
    fn next(&mut self, timeout: Option<Duration>) -> io::Result<Option<CrosstermEvent>>;
}

/// The terminal itself: poll when there is a deadline, block when there is not.
#[cfg(not(target_arch = "wasm32"))]
struct TerminalEvents;

#[cfg(not(target_arch = "wasm32"))]
impl Events for TerminalEvents {
    fn next(&mut self, timeout: Option<Duration>) -> io::Result<Option<CrosstermEvent>> {
        use ratatui::crossterm::event;

        if let Some(timeout) = timeout
            && !event::poll(timeout)?
        {
            return Ok(None);
        }
        event::read().map(Some)
    }
}

/// Draw, wait, route, repeat, until the quit key.
#[cfg(not(target_arch = "wasm32"))]
fn drive<D, B, E>(demo: &mut D, terminal: &mut Terminal<B>, events: &mut E) -> io::Result<()>
where
    D: Demo,
    B: Backend<Error = io::Error>,
    E: Events,
{
    let mut stale = true;
    loop {
        if stale {
            stale = draw_frame(demo, terminal)?;
        }

        // The wait is as long as the demo will allow. A deadline sooner than one
        // frame is one frame: a zero wait would spin.
        //
        // Sustained input can starve the deadline — a wait that keeps being
        // answered by an event never runs out — which is why a demo recomputes
        // its time-derived state inside `draw` rather than on the wake-up. Every
        // one of these frames is drawn from a fresh reading of the clock, so the
        // deadline's work happens whether the wake-up or an event caused the
        // frame. The browser host has no such asymmetry: its timer is
        // independent of input.
        let timeout = demo.wake().map(|delay| delay.max(ANIMATION_FRAME));
        let Some(event) = events.next(timeout)? else {
            // The wait ran out: the deadline the demo named has arrived, and no
            // event needs inventing for it.
            stale = true;
            continue;
        };

        if is_quit(&event) {
            return Ok(());
        }
        // A resize is not an app event: the new size reaches the demo as the
        // next frame's area, so all the host owes it is that frame.
        stale |= matches!(event, CrosstermEvent::Resize(..));
        if let Ok(event) = Event::try_from(event) {
            stale |= demo.handle_event(event);
        }
    }
}

/// Draw one frame, and report whether the grid changed size while it was being
/// flushed — which leaves the frame just drawn already out of date.
///
/// Ratzilla adopts a canvas resize during the flush, after Ratatui has sized the
/// buffer for the frame, so the frame that notices is the one before the
/// corrected one.
///
/// # Errors
///
/// Returns the backend's error if the frame cannot be drawn or the grid cannot
/// be measured.
fn draw_frame<D, B>(demo: &mut D, terminal: &mut Terminal<B>) -> io::Result<bool>
where
    D: Demo,
    B: Backend<Error = io::Error>,
{
    let drawn = terminal.draw(|frame| demo.draw(frame))?.area.as_size();
    Ok(terminal.size()? != drawn)
}

/// Run `demo` in the browser. Returns once the host is wired up; the demo keeps
/// running on browser events and animation frames.
///
/// # Errors
///
/// Returns an I/O error if the canvas backend or one of the listeners cannot be
/// installed.
#[cfg(target_arch = "wasm32")]
pub fn run<D: Demo + 'static>(demo: D) -> io::Result<()> {
    web_host::start(demo)
}

/// Ctrl+C, the demos' quit key.
#[cfg(not(target_arch = "wasm32"))]
fn is_quit(event: &CrosstermEvent) -> bool {
    use ratatui::crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};

    matches!(
        event,
        CrosstermEvent::Key(key)
            if key.kind == KeyEventKind::Press
                && key.code == KeyCode::Char('c')
                && key.modifiers.contains(KeyModifiers::CONTROL)
    )
}

/// The browser host: events in, animation frames out, nothing in between.
#[cfg(target_arch = "wasm32")]
mod web_host {
    use std::{
        cell::{Cell, RefCell},
        io,
        rc::Rc,
    };

    use gloo_timers::callback::Timeout;
    use ratatui::Terminal;
    use ratcn::runtime::Event;
    use ratzilla::{
        WebGl2Backend, WebRenderer,
        web_sys::{
            self,
            wasm_bindgen::{JsCast, prelude::Closure},
        },
    };

    use super::{ANIMATION_FRAME, BrowserPasteListener, Demo};

    /// Drives one demo on its own animation frames.
    ///
    /// Ratzilla's `draw_web` is deliberately unused: it consumes the terminal
    /// and re-arms `requestAnimationFrame` unconditionally, so a fully idle
    /// demo still renders a complete frame sixty times a second. The terminal
    /// it drives is an ordinary Ratatui [`Terminal`], so this host keeps the
    /// terminal instead and asks for a frame only when there is something new
    /// to show.
    struct Host<D> {
        demo: RefCell<D>,
        terminal: RefCell<Terminal<WebGl2Backend>>,
        /// The callback handed to `requestAnimationFrame`, kept alive for as
        /// long as the host is.
        frame: RefCell<Option<Closure<dyn FnMut()>>>,
        /// Whether a frame is already requested, so a burst of events costs one
        /// frame rather than one frame each.
        requested: Cell<bool>,
        /// The pending [`Demo::wake`] deadline. Dropping it cancels the timer.
        timer: RefCell<Option<Timeout>>,
        /// Whether a frame has been drawn. Before the first one the runtime has
        /// no geometry and ignores every event.
        rendered: Cell<bool>,
        /// The document paste listener, held for its `Drop`: the host owns it for
        /// as long as it drives the demo, and letting go of it uninstalls it.
        _paste: RefCell<Option<BrowserPasteListener>>,
        /// The window resize listener, held for the same reason.
        _resize: RefCell<Option<Closure<dyn FnMut()>>>,
    }

    /// Wire up the demo and render its first frame.
    ///
    /// Nothing holds the returned host: the animation-frame callback owns a
    /// reference to the host, the host owns the callback, and that cycle is
    /// what keeps the demo alive after `main` returns.
    pub fn start<D: Demo + 'static>(demo: D) -> io::Result<()> {
        let backend = super::web_backend(demo.background())?;
        let host = Rc::new(Host {
            demo: RefCell::new(demo),
            terminal: RefCell::new(Terminal::new(backend)?),
            frame: RefCell::new(None),
            requested: Cell::new(false),
            timer: RefCell::new(None),
            rendered: Cell::new(false),
            _paste: RefCell::new(None),
            _resize: RefCell::new(None),
        });

        *host.frame.borrow_mut() = Some(Closure::new({
            let host = Rc::clone(&host);
            move || host.render()
        }));

        if D::INPUT {
            let mut terminal = host.terminal.borrow_mut();
            terminal
                .on_key_event({
                    let host = Rc::clone(&host);
                    move |key| {
                        host.on_event(key);
                    }
                })
                .map_err(|error| io::Error::other(error.to_string()))?;
            // Ratzilla reports browser pointer positions in terminal cells.
            terminal
                .on_mouse_event({
                    let host = Rc::clone(&host);
                    move |mouse| {
                        host.on_event(mouse);
                    }
                })
                .map_err(|error| io::Error::other(error.to_string()))?;
        }

        if D::PASTE {
            let listener = BrowserPasteListener::install({
                let host = Rc::clone(&host);
                move |paste| host.on_event(paste)
            })?;
            *host._paste.borrow_mut() = Some(listener);
        }

        host.watch_resize()?;
        host.request_frame();
        Ok(())
    }

    impl<D: Demo + 'static> Host<D> {
        /// Paint one frame and decide when the next one is due.
        fn render(self: &Rc<Self>) {
            self.requested.set(false);
            let outgrew = super::draw_frame(
                &mut *self.demo.borrow_mut(),
                &mut self.terminal.borrow_mut(),
            )
            .expect("the canvas backend accepts every frame");
            self.rendered.set(true);
            // The canvas adopted a resize while this frame was flushed, so the
            // frame that settles on the new grid is the next one.
            if outgrew {
                self.request_frame();
            }
            self.arm_wake();
        }

        /// Render on the next animation frame. Asking twice costs one frame:
        /// the frame answers every request made before it.
        fn request_frame(self: &Rc<Self>) {
            if self.requested.replace(true) {
                return;
            }
            let frame = self.frame.borrow();
            let callback = frame
                .as_ref()
                .expect("the frame callback is installed before the first request");
            web_sys::window()
                .expect("a demo runs in a browser window")
                .request_animation_frame(callback.as_ref().unchecked_ref())
                .expect("the browser schedules animation frames");
        }

        /// Arm the frame the demo asked the clock for, replacing any deadline
        /// the frame just drawn has made obsolete.
        fn arm_wake(self: &Rc<Self>) {
            let wake = self.demo.borrow().wake();
            // Dropping the pending timeout cancels it.
            self.timer.borrow_mut().take();
            match wake {
                None => {}
                // A deadline within one frame is just the next frame.
                Some(delay) if delay <= ANIMATION_FRAME => self.request_frame(),
                Some(delay) => {
                    let millis = u32::try_from(delay.as_millis()).unwrap_or(u32::MAX);
                    let host = Rc::clone(self);
                    let timeout = Timeout::new(millis, move || host.request_frame());
                    *self.timer.borrow_mut() = Some(timeout);
                }
            }
        }

        /// Route one browser event, and report whether the demo took it — which
        /// is what a paste listener needs to know to leave the page's own
        /// handling alone.
        fn on_event(self: &Rc<Self>, event: impl TryInto<Event>) -> bool {
            // Before the first frame there is no surface to route through: the
            // runtime would ignore the event, so claiming it would be a lie.
            if !self.rendered.get() {
                return false;
            }
            let Ok(event) = event.try_into() else {
                return false;
            };
            let handled = self.demo.borrow_mut().handle_event(event);
            if handled {
                self.request_frame();
            }
            handled
        }

        /// Redraw when the window resizes.
        ///
        /// No ratzilla callback reports a resize, and the canvas only adopts a
        /// new size while a frame is flushed — so without this the grid would
        /// keep the size it booted at until some unrelated event happened to
        /// ask for a frame.
        fn watch_resize(self: &Rc<Self>) -> io::Result<()> {
            let window = web_sys::window().ok_or_else(|| io::Error::other("no window"))?;
            let callback = Closure::<dyn FnMut()>::new({
                let host = Rc::clone(self);
                move || host.request_frame()
            });
            window
                .add_event_listener_with_callback("resize", callback.as_ref().unchecked_ref())
                .map_err(|error| io::Error::other(format!("resize listener: {error:?}")))?;
            *self._resize.borrow_mut() = Some(callback);
            Ok(())
        }
    }

    impl<D> Drop for Host<D> {
        /// Take the resize listener off the window with the host. The paste
        /// listener is a guard that removes itself; a bare [`Closure`] cannot.
        fn drop(&mut self) {
            if let Some(callback) = self._resize.borrow().as_ref()
                && let Some(window) = web_sys::window()
            {
                let _ = window.remove_event_listener_with_callback(
                    "resize",
                    callback.as_ref().unchecked_ref(),
                );
            }
        }
    }
}

/// A `WebGl2Backend` whose canvas padding matches the demo's background.
#[cfg(target_arch = "wasm32")]
fn web_backend(background: Color) -> Result<ratzilla::WebGl2Backend, io::Error> {
    ratzilla::backend::webgl2::WebGl2Backend::new_with_options(
        ratzilla::backend::webgl2::WebGl2BackendOptions::new().canvas_padding_color(background),
    )
    .map_err(|error| io::Error::other(error.to_string()))
}

/// Browser paste listener, installed for the guard's lifetime.
#[cfg(target_arch = "wasm32")]
mod browser_paste {
    use std::io;

    use ratcn::runtime::Event;
    use ratzilla::web_sys::{
        ClipboardEvent, Document,
        wasm_bindgen::{JsCast, prelude::Closure},
    };

    #[must_use = "dropping the listener removes its browser paste handler"]
    pub struct BrowserPasteListener {
        document: Document,
        callback: Closure<dyn FnMut(ClipboardEvent)>,
    }

    impl BrowserPasteListener {
        /// Forward `text/plain` clipboard data as [`Event::Paste`]. Ratzilla has
        /// no callback for it, so the listener goes on the document itself.
        ///
        /// `on_paste` reports whether the demo took the text; only then is the
        /// page's own paste handling suppressed.
        ///
        /// # Errors
        ///
        /// Returns an I/O error if there is no document, or if it refuses the
        /// listener.
        pub fn install(on_paste: impl FnMut(Event) -> bool + 'static) -> io::Result<Self> {
            let document = ratzilla::web_sys::window()
                .and_then(|window| window.document())
                .ok_or_else(|| io::Error::other("no document"))?;
            let mut on_paste = on_paste;
            let callback = Closure::new(move |event: ClipboardEvent| {
                let Ok(paste) = Event::try_from(&event) else {
                    return;
                };
                if on_paste(paste) {
                    event.prevent_default();
                }
            });
            document
                .add_event_listener_with_callback("paste", callback.as_ref().unchecked_ref())
                .map_err(|error| io::Error::other(format!("paste listener: {error:?}")))?;
            Ok(Self { document, callback })
        }
    }

    impl Drop for BrowserPasteListener {
        fn drop(&mut self) {
            let _ = self.document.remove_event_listener_with_callback(
                "paste",
                self.callback.as_ref().unchecked_ref(),
            );
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use browser_paste::BrowserPasteListener;

/// Elapsed time since the demo started, from the platform's monotonic clock.
#[must_use]
pub fn monotonic_time() -> Duration {
    #[cfg(target_arch = "wasm32")]
    {
        let millis = ratzilla::web_sys::window()
            .and_then(|window| window.performance())
            .map_or(0.0, |performance| performance.now())
            .max(0.0);
        Duration::from_secs_f64(millis / 1_000.0)
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::{sync::OnceLock, time::Instant};

        static START: OnceLock<Instant> = OnceLock::new();
        START.get_or_init(Instant::now).elapsed()
    }
}

/// The redraw policy, driven from a scripted terminal.
///
/// The policy is the whole point of this host, and every claim it makes is
/// observable natively: the frames a demo is asked to draw, and the waits the
/// host performs between them.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::{collections::VecDeque, convert::Infallible};

    use ratatui::{
        backend::{ClearType, TestBackend, WindowSize},
        buffer::Cell,
        crossterm::event::{KeyCode, KeyEvent as CrosstermKeyEvent, KeyModifiers},
        layout::{Position, Size},
    };

    use super::{
        ANIMATION_FRAME, Backend, Color, CrosstermEvent, Demo, Duration, Event, Events, Frame,
        Terminal, draw_frame, drive, io,
    };

    /// A [`TestBackend`] that reports a native host's error type, and that can
    /// adopt a resize while a frame is flushed — the one habit of ratzilla's
    /// canvas the policy has to answer for.
    struct HostBackend {
        inner: TestBackend,
        /// Adopted during the next flush, once.
        resize_on_flush: Option<Size>,
    }

    impl HostBackend {
        fn new(width: u16, height: u16) -> Self {
            Self {
                inner: TestBackend::new(width, height),
                resize_on_flush: None,
            }
        }

        /// Change the grid size during the next flush, the way a canvas does.
        fn resizing(mut self, width: u16, height: u16) -> Self {
            self.resize_on_flush = Some(Size::new(width, height));
            self
        }
    }

    /// A `TestBackend` cannot fail; a native host's backend can.
    fn native<T>(result: Result<T, Infallible>) -> io::Result<T> {
        Ok(result.expect("a TestBackend is infallible"))
    }

    impl Backend for HostBackend {
        type Error = io::Error;

        fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
        where
            I: Iterator<Item = (u16, u16, &'a Cell)>,
        {
            native(self.inner.draw(content))
        }

        fn hide_cursor(&mut self) -> io::Result<()> {
            native(self.inner.hide_cursor())
        }

        fn show_cursor(&mut self) -> io::Result<()> {
            native(self.inner.show_cursor())
        }

        fn get_cursor_position(&mut self) -> io::Result<Position> {
            native(self.inner.get_cursor_position())
        }

        fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
            native(self.inner.set_cursor_position(position))
        }

        fn clear(&mut self) -> io::Result<()> {
            native(self.inner.clear())
        }

        fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
            native(self.inner.clear_region(clear_type))
        }

        fn size(&self) -> io::Result<Size> {
            native(self.inner.size())
        }

        fn window_size(&mut self) -> io::Result<WindowSize> {
            native(self.inner.window_size())
        }

        fn flush(&mut self) -> io::Result<()> {
            if let Some(size) = self.resize_on_flush.take() {
                self.inner.resize(size.width, size.height);
            }
            native(self.inner.flush())
        }
    }

    /// A demo that records what the host asked of it, and answers what the test
    /// needs it to.
    #[derive(Default)]
    struct Probe {
        /// Frames drawn.
        frames: usize,
        /// Events routed, which a resize must never appear in.
        routed: Vec<Event>,
        /// What `handle_event` reports, per event in order. A spent script
        /// reports `false`, the answer that must not cause a frame.
        handled: VecDeque<bool>,
        /// The deadline to report once `frames` frames have been drawn, so a
        /// test can watch the host re-read it. Missing entries are no deadline.
        deadlines: Vec<Option<Duration>>,
    }

    impl Demo for Probe {
        fn background(&self) -> Color {
            Color::Reset
        }

        fn draw(&mut self, frame: &mut Frame) {
            self.frames += 1;
            let area = frame.area();
            frame.render_widget("probe", area);
        }

        fn handle_event(&mut self, event: Event) -> bool {
            self.routed.push(event);
            self.handled.pop_front().unwrap_or(false)
        }

        fn wake(&self) -> Option<Duration> {
            self.deadlines.get(self.frames).copied().flatten()
        }
    }

    /// The waits the host performs, scripted: one entry answers one wait.
    struct Script {
        /// `Some(event)` hands an event over; `None` is a wait that ran out.
        steps: VecDeque<Option<CrosstermEvent>>,
        /// The timeout the host asked for, per wait, in order.
        waits: Vec<Option<Duration>>,
    }

    impl Script {
        fn new(steps: impl IntoIterator<Item = Option<CrosstermEvent>>) -> Self {
            Self {
                steps: steps.into_iter().collect(),
                waits: Vec::new(),
            }
        }
    }

    impl Events for Script {
        fn next(&mut self, timeout: Option<Duration>) -> io::Result<Option<CrosstermEvent>> {
            self.waits.push(timeout);
            // A spent script quits, so the loop returns instead of running on.
            Ok(self.steps.pop_front().unwrap_or_else(|| Some(quit())))
        }
    }

    fn key(code: char) -> CrosstermEvent {
        CrosstermEvent::Key(CrosstermKeyEvent::new(
            KeyCode::Char(code),
            KeyModifiers::NONE,
        ))
    }

    fn quit() -> CrosstermEvent {
        CrosstermEvent::Key(CrosstermKeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        ))
    }

    /// Run the host over `script` and hand back the terminal it drew on.
    fn run_scripted(
        probe: &mut Probe,
        backend: HostBackend,
        script: &mut Script,
    ) -> Terminal<HostBackend> {
        let mut terminal = Terminal::new(backend).expect("the test backend opens");
        drive(probe, &mut terminal, script).expect("the scripted host reaches the quit key");
        terminal
    }

    #[test]
    fn the_first_frame_is_drawn_before_any_event_arrives() {
        let mut probe = Probe::default();
        let mut script = Script::new([]);

        let terminal = run_scripted(&mut probe, HostBackend::new(20, 5), &mut script);

        assert_eq!(probe.frames, 1);
        assert_eq!(
            script.waits,
            vec![None],
            "a demo with no deadline waits for input, with no timeout"
        );
        let painted: String = terminal
            .backend()
            .inner
            .buffer()
            .content
            .iter()
            .map(Cell::symbol)
            .collect();
        assert!(
            painted.contains("probe"),
            "the frame reached the backend: {painted:?}"
        );
    }

    #[test]
    fn an_event_the_demo_handled_draws_the_next_frame() {
        let mut probe = Probe {
            handled: VecDeque::from([true]),
            ..Probe::default()
        };
        let mut script = Script::new([Some(key('y'))]);

        run_scripted(&mut probe, HostBackend::new(20, 5), &mut script);

        assert_eq!(probe.routed.len(), 1, "the event reached the demo");
        assert_eq!(probe.frames, 2, "the first frame, and one for the event");
    }

    #[test]
    fn an_event_the_demo_ignored_draws_nothing() {
        let mut probe = Probe {
            handled: VecDeque::from([false]),
            ..Probe::default()
        };
        let mut script = Script::new([Some(key('n'))]);

        run_scripted(&mut probe, HostBackend::new(20, 5), &mut script);

        assert_eq!(probe.routed.len(), 1, "the event reached the demo");
        assert_eq!(probe.frames, 1, "only the first frame");
    }

    #[test]
    fn a_resize_draws_even_though_the_demo_never_sees_it() {
        let mut probe = Probe::default();
        let mut script = Script::new([Some(CrosstermEvent::Resize(30, 10))]);

        run_scripted(&mut probe, HostBackend::new(20, 5), &mut script);

        assert!(
            probe.routed.is_empty(),
            "a resize is not an app event: {:?}",
            probe.routed
        );
        assert_eq!(probe.frames, 2, "the new size reaches the demo as an area");
    }

    #[test]
    fn a_wait_that_runs_out_draws_the_frame_the_deadline_asked_for() {
        let deadline = Duration::from_secs(2);
        let mut probe = Probe {
            deadlines: vec![None, Some(deadline)],
            ..Probe::default()
        };
        let mut script = Script::new([None]);

        run_scripted(&mut probe, HostBackend::new(20, 5), &mut script);

        assert_eq!(probe.frames, 2, "the deadline caused a frame of its own");
        assert!(
            probe.routed.is_empty(),
            "nothing was invented to carry the wake-up"
        );
        assert_eq!(script.waits.first(), Some(&Some(deadline)));
    }

    #[test]
    fn the_deadline_is_re_read_after_a_frame_and_left_alone_after_an_ignored_event() {
        let first = Duration::from_secs(2);
        let second = Duration::from_secs(3);
        let mut probe = Probe {
            handled: VecDeque::from([false]),
            deadlines: vec![None, Some(first), Some(second)],
            ..Probe::default()
        };
        // An ignored event, then a wait that runs out.
        let mut script = Script::new([Some(key('n')), None]);

        run_scripted(&mut probe, HostBackend::new(20, 5), &mut script);

        assert_eq!(
            script.waits,
            vec![Some(first), Some(first), Some(second)],
            "the deadline follows the frames: unchanged while the ignored event \
             drew none, re-read once the wake-up drew one"
        );
    }

    #[test]
    fn a_deadline_inside_one_frame_still_waits_a_whole_frame() {
        let mut probe = Probe {
            deadlines: vec![None, Some(Duration::ZERO)],
            ..Probe::default()
        };
        let mut script = Script::new([]);

        run_scripted(&mut probe, HostBackend::new(20, 5), &mut script);

        assert_eq!(
            script.waits,
            vec![Some(ANIMATION_FRAME)],
            "a zero wait would spin the CPU for nothing new to show"
        );
    }

    #[test]
    fn the_quit_key_ends_the_loop_without_routing_it() {
        let mut probe = Probe::default();
        let mut script = Script::new([Some(quit()), Some(key('y'))]);

        run_scripted(&mut probe, HostBackend::new(20, 5), &mut script);

        assert!(
            probe.routed.is_empty(),
            "the quit key is the host's, not the demo's: {:?}",
            probe.routed
        );
        assert_eq!(probe.frames, 1, "nothing after the quit key ran");
    }

    #[test]
    fn a_frame_the_grid_outgrew_reports_it() {
        let mut probe = Probe::default();
        let mut steady = Terminal::new(HostBackend::new(20, 5)).expect("the test backend opens");
        let mut resizing = Terminal::new(HostBackend::new(20, 5).resizing(30, 10))
            .expect("the test backend opens");

        assert!(
            !draw_frame(&mut probe, &mut steady).expect("the test backend draws"),
            "a grid that did not move needs no second frame"
        );
        assert!(
            draw_frame(&mut probe, &mut resizing).expect("the test backend draws"),
            "the flush adopted a new size, so the frame just drawn is stale"
        );
    }

    #[test]
    fn the_loop_draws_the_frame_that_settles_a_grid_it_outgrew() {
        let script = || Script::new([Some(key('n'))]);
        let probe = || Probe {
            handled: VecDeque::from([false]),
            ..Probe::default()
        };

        let mut steady_probe = probe();
        run_scripted(&mut steady_probe, HostBackend::new(20, 5), &mut script());

        let mut resized_probe = probe();
        run_scripted(
            &mut resized_probe,
            HostBackend::new(20, 5).resizing(30, 10),
            &mut script(),
        );

        assert_eq!(steady_probe.frames, 1, "nothing asked for a second frame");
        assert_eq!(
            resized_probe.frames, 2,
            "the frame the canvas resized under is followed by the one that fits"
        );
    }
}
