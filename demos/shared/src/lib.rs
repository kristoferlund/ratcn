//! The host every demo runs on: one [`Demo`] implementation, two platforms.
//!
//! A demo says what it paints, what it does with an event, and when the clock
//! should wake it. [`run`] owns everything else — the terminal or canvas, the
//! event listeners, and the redraw policy.
//!
//! Everything a terminal app needs beyond that is
//! [`ratcn::terminal`]: opening the terminal, putting it back,
//! and following its colors. This crate is the demos' glue and nothing more, so
//! that a demo can be lifted out of this repository with it and still run.
//!
//! # Frames happen for a reason
//!
//! The host renders when something says the last frame is stale: the first
//! frame, an event the demo reports as handled, a [`Demo::wake`] deadline, or a
//! resize. Nothing else. An idle demo costs one blocked read natively, and
//! nothing at all in the browser.

use std::{io, time::Duration};

#[cfg(target_arch = "wasm32")]
use ratatui::style::Color;
use ratatui::{Frame, Terminal, backend::Backend};
#[cfg(not(target_arch = "wasm32"))]
use ratcn::terminal::{Session, SessionEvent, SessionOptions, termina};
use ratcn::{Theme, runtime::Event};

/// The shortest wait worth honoring: a wake sooner than the display refreshes
/// cannot show anything new. [`Demo::wake`] values this small or smaller mean
/// "keep rendering", which is how a demo animates.
pub const ANIMATION_FRAME: Duration = Duration::from_millis(16);

/// One demo, in the shape a host can drive.
pub trait Demo {
    /// Whether the demo reads input. Turning it on installs the browser's
    /// listeners and takes over the terminal's mouse.
    const INPUT: bool = true;

    /// Whether the host delivers clipboard pastes — bracketed paste natively,
    /// the browser's `paste` event on the web.
    const PASTE: bool = false;

    /// The theme this demo paints with, and what it falls back to under
    /// [`ADAPTIVE`](Self::ADAPTIVE).
    const THEME: Theme = Theme::default_dark();

    /// Turn this on to paint with the terminal's own colors, following them as
    /// the user changes them. They reach [`draw`](Self::draw) each frame.
    const ADAPTIVE: bool = false;

    /// Paint one frame with `theme`.
    fn draw(&mut self, frame: &mut Frame, theme: &Theme);

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
    /// This is how a demo whose screen depends on the clock — a toast that
    /// expires, an animation, a background fetch still in flight — gets the
    /// frame it needs between input events.
    ///
    /// It is a deadline: a demo reads the clock in [`draw`](Self::draw) and
    /// works from that reading, so any frame serves it. Under sustained input
    /// every frame is drawn from a fresh reading, which is what keeps the
    /// deadline's work happening.
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
    let mut session = Session::open(options_for::<D>())?;
    drive(&mut demo, &mut session)
}

/// What a demo asks of its terminal.
#[cfg(not(target_arch = "wasm32"))]
fn options_for<D: Demo>() -> SessionOptions {
    let mut options = SessionOptions::new();
    if D::INPUT {
        options = options.mouse();
    }
    if D::PASTE {
        options = options.paste();
    }
    if D::ADAPTIVE {
        options = options.adaptive();
    }
    options
}

/// What the loop needs of its host: a frame to draw on, and the next thing to
/// happen.
///
/// [`Session`] is the one that matters; the tests supply a scripted one, which
/// is how the redraw policy below is checked without a terminal.
#[cfg(not(target_arch = "wasm32"))]
trait Host {
    type Backend: Backend<Error = io::Error>;

    fn terminal(&mut self) -> &mut Terminal<Self::Backend>;

    /// What the terminal says it looks like, or `fallback`.
    fn theme(&self, fallback: Theme) -> Theme;

    /// Wait for at most `timeout`, or indefinitely when it is [`None`].
    fn next(&mut self, timeout: Option<Duration>) -> io::Result<Option<SessionEvent>>;
}

#[cfg(not(target_arch = "wasm32"))]
impl Host for Session {
    type Backend = ratcn::terminal::SessionBackend;

    fn terminal(&mut self) -> &mut Terminal<Self::Backend> {
        self.terminal_mut()
    }

    fn theme(&self, fallback: Theme) -> Theme {
        self.theme_with_fallback(fallback)
    }

    fn next(&mut self, timeout: Option<Duration>) -> io::Result<Option<SessionEvent>> {
        Session::next(self, timeout)
    }
}

/// Draw, wait, route, repeat, until the quit key.
#[cfg(not(target_arch = "wasm32"))]
fn drive<D: Demo, H: Host>(demo: &mut D, host: &mut H) -> io::Result<()> {
    let mut stale = true;
    loop {
        if stale {
            // Read every frame: this is what follows a terminal that re-themes.
            let theme = host.theme(D::THEME);
            stale = draw_frame(demo, host.terminal(), &theme)?;
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
        let Some(event) = host.next(timeout)? else {
            // The wait ran out: the deadline the demo named has arrived, and no
            // event needs inventing for it.
            stale = true;
            continue;
        };

        let event = match event {
            // The terminal re-themed. Nothing was asked of this loop to make it
            // happen; it arrives like any other event.
            // The frame above reads the theme, so this owes only the frame.
            SessionEvent::ThemeChanged(_) => {
                stale = true;
                continue;
            }
            SessionEvent::Input(event) => event,
        };

        if is_quit(&event) {
            return Ok(());
        }
        // A resize is not an app event: the new size reaches the demo as the
        // next frame's area, so all the host owes it is that frame.
        stale |= matches!(event, termina::Event::WindowResized(..));
        if let Ok(event) = Event::try_from(event) {
            stale |= demo.handle_event(event);
        }
    }
}

/// Ctrl+C, the demos' quit key.
#[cfg(not(target_arch = "wasm32"))]
fn is_quit(event: &termina::Event) -> bool {
    use termina::event::{KeyCode, KeyEventKind, Modifiers};

    matches!(
        event,
        termina::Event::Key(key)
            if key.kind == KeyEventKind::Press
                && key.code == KeyCode::Char('c')
                && key.modifiers.contains(Modifiers::CONTROL)
    )
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
fn draw_frame<D, B>(demo: &mut D, terminal: &mut Terminal<B>, theme: &Theme) -> io::Result<bool>
where
    D: Demo,
    B: Backend<Error = io::Error>,
{
    let drawn = terminal
        .draw(|frame| demo.draw(frame, theme))?
        .area
        .as_size();
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
    /// Ratzilla's `draw_web` re-arms `requestAnimationFrame` unconditionally,
    /// so an idle demo would render sixty frames a second. This host keeps the
    /// [`Terminal`] and asks for a frame only when there is something new.
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
        let backend = super::web_backend(D::THEME.background)?;
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
                &D::THEME,
            )
            .expect("the canvas backend refused a frame");
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
                .expect("a frame was requested before the host was wired up");
            web_sys::window()
                .expect("no browser window to draw in")
                .request_animation_frame(callback.as_ref().unchecked_ref())
                .expect("the browser refused an animation frame");
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

/// Elapsed time since this was first called, from the platform's monotonic
/// clock. Demos call it every frame, so the first call is the first frame.
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
        // A process-start instant has to be remembered somewhere, and `Instant`
        // has no const constructor.
        use std::{sync::OnceLock, time::Instant};

        static START: OnceLock<Instant> = OnceLock::new();
        START.get_or_init(Instant::now).elapsed()
    }
}

/// The redraw policy, driven from a scripted host.
///
/// The policy is the whole point of this crate, and every claim it makes is
/// observable: the frames a demo is asked to draw, and the waits the loop asks
/// for between them.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::{collections::VecDeque, convert::Infallible};

    use ratatui::{
        backend::{ClearType, TestBackend, WindowSize},
        buffer::Cell,
        layout::{Position, Size},
    };
    use ratcn::Theme;
    use ratcn::terminal::termina::{
        self, WindowSize as TerminalSize,
        event::{KeyCode, KeyEvent as TerminalKeyEvent, Modifiers},
    };

    use super::{
        ANIMATION_FRAME, Backend, Demo, Duration, Event, Frame, Host, SessionEvent, SessionOptions,
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
        Ok(result.expect("a TestBackend cannot fail"))
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
        /// The theme each frame was painted with, in order.
        themes: Vec<Theme>,
    }

    impl Demo for Probe {
        fn draw(&mut self, frame: &mut Frame, theme: &Theme) {
            self.frames += 1;
            self.themes.push(*theme);
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

    /// The host, scripted: one entry answers one wait.
    struct Script {
        terminal: Terminal<HostBackend>,
        /// What the terminal says it looks like, if it says anything.
        theme: Option<Theme>,
        /// `Some(event)` hands an event over; `None` is a wait that ran out.
        steps: VecDeque<Option<SessionEvent>>,
        /// The timeout the loop asked for, per wait, in order.
        waits: Vec<Option<Duration>>,
    }

    impl Script {
        fn new(
            backend: HostBackend,
            steps: impl IntoIterator<Item = Option<SessionEvent>>,
        ) -> Self {
            Self {
                terminal: Terminal::new(backend).expect("the test backend opens"),
                theme: None,
                steps: steps.into_iter().collect(),
                waits: Vec::new(),
            }
        }
    }

    impl Host for Script {
        type Backend = HostBackend;

        fn terminal(&mut self) -> &mut Terminal<HostBackend> {
            &mut self.terminal
        }

        fn theme(&self, fallback: Theme) -> Theme {
            self.theme.unwrap_or(fallback)
        }

        fn next(&mut self, timeout: Option<Duration>) -> io::Result<Option<SessionEvent>> {
            self.waits.push(timeout);
            // A spent script quits, so the loop returns instead of running on.
            let step = self.steps.pop_front().unwrap_or_else(|| Some(quit()));
            // The terminal wears a theme from the moment it reports it, the way
            // a real one does.
            if let Some(SessionEvent::ThemeChanged(theme)) = &step {
                self.theme = Some(*theme);
            }
            Ok(step)
        }
    }

    fn input(event: termina::Event) -> SessionEvent {
        SessionEvent::Input(event)
    }

    fn key(code: char) -> SessionEvent {
        input(termina::Event::Key(TerminalKeyEvent::new(
            KeyCode::Char(code),
            Modifiers::NONE,
        )))
    }

    fn quit() -> SessionEvent {
        input(termina::Event::Key(TerminalKeyEvent::new(
            KeyCode::Char('c'),
            Modifiers::CONTROL,
        )))
    }

    fn resize(cols: u16, rows: u16) -> SessionEvent {
        input(termina::Event::WindowResized(TerminalSize {
            cols,
            rows,
            pixel_width: None,
            pixel_height: None,
        }))
    }

    /// Run the loop over `script` and hand back what it drew on.
    fn run_scripted(probe: &mut Probe, script: &mut Script) {
        drive(probe, script).expect("the scripted host reaches the quit key");
    }

    /// A demo that reads no input keeps the terminal's own mouse, and one that
    /// never pastes leaves bracketed paste alone.
    #[test]
    fn a_demo_gets_the_terminal_modes_it_asked_for_and_no_others() {
        struct Quiet;
        struct Everything;
        impl Demo for Quiet {
            const INPUT: bool = false;
            fn draw(&mut self, _frame: &mut Frame, _theme: &Theme) {}
        }
        impl Demo for Everything {
            const PASTE: bool = true;
            const ADAPTIVE: bool = true;
            fn draw(&mut self, _frame: &mut Frame, _theme: &Theme) {}
        }

        assert_eq!(
            super::options_for::<Quiet>(),
            SessionOptions::new(),
            "a demo that asks for nothing gets the alternate screen alone, and \
             its terminal is never queried"
        );
        assert_eq!(
            super::options_for::<Probe>(),
            SessionOptions::new().mouse(),
            "reading input is what asks for the mouse"
        );
        assert_eq!(
            super::options_for::<Everything>(),
            SessionOptions::new().mouse().paste().adaptive(),
            "and following the terminal is what asks for the query"
        );
    }

    #[test]
    fn the_first_frame_is_drawn_before_any_event_arrives() {
        let mut probe = Probe::default();
        let mut script = Script::new(HostBackend::new(20, 5), []);

        run_scripted(&mut probe, &mut script);

        assert_eq!(probe.frames, 1);
        assert_eq!(
            script.waits,
            vec![None],
            "a demo with no deadline waits for input, with no timeout"
        );
        let painted: String = script
            .terminal
            .backend()
            .inner
            .buffer()
            .content
            .iter()
            .map(Cell::symbol)
            .collect();
        assert!(painted.contains("probe"), "the frame reached the backend");
    }

    #[test]
    fn an_event_the_demo_handled_draws_the_next_frame() {
        let mut probe = Probe {
            handled: VecDeque::from([true]),
            ..Probe::default()
        };
        let mut script = Script::new(HostBackend::new(20, 5), [Some(key('y'))]);

        run_scripted(&mut probe, &mut script);

        assert_eq!(probe.routed.len(), 1, "the event reached the demo");
        assert_eq!(probe.frames, 2, "the first frame, and one for the event");
    }

    #[test]
    fn an_event_the_demo_ignored_draws_nothing() {
        let mut probe = Probe {
            handled: VecDeque::from([false]),
            ..Probe::default()
        };
        let mut script = Script::new(HostBackend::new(20, 5), [Some(key('n'))]);

        run_scripted(&mut probe, &mut script);

        assert_eq!(probe.routed.len(), 1, "the event reached the demo");
        assert_eq!(probe.frames, 1, "only the first frame");
    }

    #[test]
    fn a_resize_draws_even_though_the_demo_never_sees_it() {
        let mut probe = Probe::default();
        let mut script = Script::new(HostBackend::new(20, 5), [Some(resize(30, 10))]);

        run_scripted(&mut probe, &mut script);

        assert!(probe.routed.is_empty(), "a resize is not an app event");
        assert_eq!(probe.frames, 2, "the new size reaches the demo as an area");
    }

    /// Each frame is painted with the theme read from the terminal for that
    /// frame, so a change reaches the screen through `draw` alone.
    #[test]
    fn a_re_theme_reaches_the_next_frame_without_the_demo_matching_it() {
        let flipped = Theme::catppuccin();
        let mut probe = Probe::default();
        let mut script = Script::new(
            HostBackend::new(20, 5),
            [Some(SessionEvent::ThemeChanged(flipped))],
        );

        run_scripted(&mut probe, &mut script);

        assert!(
            probe.routed.is_empty(),
            "a theme change is not routed at the components"
        );
        assert_eq!(probe.frames, 2, "the frame that wears it was drawn");
        assert_eq!(
            probe.themes,
            vec![<Probe as Demo>::THEME, flipped],
            "the first frame wears the preset, and the frame after the change \
             wears what the terminal now says"
        );
    }

    #[test]
    fn a_demo_paints_with_its_own_preset_when_the_terminal_says_nothing() {
        let mut probe = Probe::default();
        let mut script = Script::new(HostBackend::new(20, 5), []);

        run_scripted(&mut probe, &mut script);

        assert_eq!(
            probe.themes,
            vec![<Probe as Demo>::THEME],
            "the fallback is the demo's own const, not the crate's"
        );
    }

    #[test]
    fn a_wait_that_runs_out_draws_the_frame_the_deadline_asked_for() {
        let deadline = Duration::from_secs(2);
        let mut probe = Probe {
            deadlines: vec![None, Some(deadline)],
            ..Probe::default()
        };
        let mut script = Script::new(HostBackend::new(20, 5), [None]);

        run_scripted(&mut probe, &mut script);

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
        let mut script = Script::new(HostBackend::new(20, 5), [Some(key('n')), None]);

        run_scripted(&mut probe, &mut script);

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
        let mut script = Script::new(HostBackend::new(20, 5), []);

        run_scripted(&mut probe, &mut script);

        assert_eq!(
            script.waits,
            vec![Some(ANIMATION_FRAME)],
            "a zero wait would spin the CPU for nothing new to show"
        );
    }

    #[test]
    fn the_quit_key_ends_the_loop_without_routing_it() {
        let mut probe = Probe::default();
        let mut script = Script::new(HostBackend::new(20, 5), [Some(quit()), Some(key('y'))]);

        run_scripted(&mut probe, &mut script);

        assert!(probe.routed.is_empty(), "the quit key is the host's");
        assert_eq!(probe.frames, 1, "nothing after the quit key ran");
    }

    #[test]
    fn a_frame_the_grid_outgrew_reports_it() {
        let mut probe = Probe::default();
        let mut steady = Terminal::new(HostBackend::new(20, 5)).expect("the test backend opens");
        let mut resizing = Terminal::new(HostBackend::new(20, 5).resizing(30, 10))
            .expect("the test backend opens");

        assert!(
            !draw_frame(&mut probe, &mut steady, &Theme::default_dark())
                .expect("the test backend draws"),
            "a grid that did not move needs no second frame"
        );
        assert!(
            draw_frame(&mut probe, &mut resizing, &Theme::default_dark())
                .expect("the test backend draws"),
            "the flush adopted a new size, so the frame just drawn is stale"
        );
    }

    #[test]
    fn the_loop_draws_the_frame_that_settles_a_grid_it_outgrew() {
        let probe = || Probe {
            handled: VecDeque::from([false]),
            ..Probe::default()
        };

        let mut steady_probe = probe();
        run_scripted(
            &mut steady_probe,
            &mut Script::new(HostBackend::new(20, 5), [Some(key('n'))]),
        );

        let mut resized_probe = probe();
        run_scripted(
            &mut resized_probe,
            &mut Script::new(HostBackend::new(20, 5).resizing(30, 10), [Some(key('n'))]),
        );

        assert_eq!(steady_probe.frames, 1, "nothing asked for a second frame");
        assert_eq!(
            resized_probe.frames, 2,
            "the frame the canvas resized under is followed by the one that fits"
        );
    }
}
