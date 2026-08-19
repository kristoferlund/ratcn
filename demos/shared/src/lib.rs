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

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
use std::{io, sync::RwLock, time::Duration};

use ratatui::{Frame, Terminal, backend::Backend, style::Color};
#[cfg(not(target_arch = "wasm32"))]
use ratatui_termina::{
    TerminaBackend,
    termina::{
        Event as TerminalEvent, EventReader, PlatformTerminal, Terminal as _,
        escape::csi::{Csi, DecPrivateMode, DecPrivateModeCode, Mode},
    },
};
#[cfg(not(target_arch = "wasm32"))]
use ratcn::terminal_query::ThemeWatch;
use ratcn::{Theme, runtime::Event};

/// The theme every demo paints with.
///
/// Natively it is solved from the colors this terminal reported about itself,
/// so the demos sit in the user's own palette rather than next to it. In the
/// browser, and on a terminal that could not be asked, it is a preset.
///
/// It is resolved before the demo is built, and on a terminal that reports its
/// own theme changes it is resolved *again* whenever the user flips theirs — so
/// read it per frame and paint from what it says. A demo that copies it into
/// its own state stops following the terminal at that moment. A demo that has
/// to read it while being built is built by [`run_lazy`], after the first
/// answer is in.
#[must_use]
pub fn theme() -> Theme {
    *THEME
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Publish a newly resolved theme, and report whether it is a change — which is
/// what the next frame depends on being told.
#[cfg(not(target_arch = "wasm32"))]
fn publish_theme(theme: Theme) -> bool {
    let mut published = THEME
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let changed = *published != theme;
    *published = theme;
    changed
}

/// Resolved by the native [`run_lazy`] and re-resolved by its event loop; in
/// the browser it keeps the value it was born with.
static THEME: RwLock<Theme> = RwLock::new(fallback_theme());

/// The theme to paint with when the terminal's own colors are unknown.
///
/// A preset that names every color, rather than [`Theme::terminal`]: the
/// fallback runs exactly when the polarity of the screen is unknown, and the
/// `terminal` preset pairs [`Color::Reset`] text with concrete dark wells —
/// which on a light terminal is near-black text on a near-black fill. A theme
/// that names its colors is at least legible on every terminal, whichever way
/// round that terminal is.
const fn fallback_theme() -> Theme {
    Theme::default_dark()
}

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
/// The demo already exists, so it was built before the terminal was asked about
/// its colors. That is fine for a demo whose theme is a constant. A demo that
/// reads [`theme`] must read it *per frame* — a copy taken while the demo was
/// being built is not only the pre-query theme, it also stops following the
/// terminal from then on. A demo that has to read the theme while being built
/// uses [`run_lazy`], which builds it after the query.
///
/// # Errors
///
/// Returns an I/O error if the terminal or the browser canvas cannot be set up,
/// and natively if drawing or reading input fails.
pub fn run<D: Demo + 'static>(demo: D) -> io::Result<()> {
    run_lazy(move || demo)
}

/// Run the demo `build` returns, building it only once [`theme`] can answer.
///
/// The theme is resolved from the terminal inside this call, and a demo that
/// reads it while being built has to be built after that — a picker seeding its
/// selection from the theme list, a widget caching a color. `build` runs at the
/// one moment where the terminal is open, the query has been answered, and no
/// frame has been drawn.
///
/// # Errors
///
/// The same as [`run`].
#[cfg(not(target_arch = "wasm32"))]
pub fn run_lazy<D: Demo + 'static>(build: impl FnOnce() -> D) -> io::Result<()> {
    let mut output = PlatformTerminal::new()?;
    // Raw mode first: nothing below reads a reply that the line discipline
    // would otherwise hold back until Enter.
    output.enter_raw_mode()?;
    // The one window the query has: raw mode is on and no event reader has
    // taken a byte yet, so the terminal's answers are the only thing in the
    // stream that is not the user typing. It happens before the alternate
    // screen so a slow terminal is answered against the shell the user can
    // still see, rather than behind a blank screen.
    //
    // The panic hook has to know the mode list, and whether the subscription
    // joins it is the query's answer — so the query goes first and the hook is
    // installed against the list it produced.
    let (theme, subscribe) = queried(&mut output);
    publish_theme(theme);

    let modes = modes(D::INPUT, D::PASTE, subscribe);
    // Termina restores raw mode after this hook runs, so the hook owes only the
    // modes this host turns on below. It writes through a handle of its own,
    // which is why it can run while the session is being unwound. Nothing is on
    // yet, so the query above had nothing for a hook to put back.
    output.set_panic_hook({
        let modes = modes.clone();
        move |handle| {
            let _ = restore_modes(handle, &modes);
        }
    });

    let events = output.event_reader();
    // `Terminal::new` measures the grid and can fail. It runs while the modes
    // are still off, so there is nothing to restore if it does — which is why
    // the session takes them on afterwards and not before.
    let session = Session {
        terminal: Terminal::new(TerminaBackend::new(output))?,
        modes,
    };
    let mut session = session.enable()?;

    let mut demo = build();
    drive(
        &mut demo,
        &mut session.terminal,
        &mut TerminalEvents(events),
    )
}

/// The native host's terminal, holding the modes it switched on.
///
/// Dropping it switches them back off. That is what covers every path out of
/// [`run_lazy`] once the modes are on — a `?`, the quit key, or an unwinding
/// panic — so the user never gets their shell back inside the alternate screen
/// with the mouse still captured. Errors while restoring are dropped: `Drop`
/// has nowhere to report them and the process is on its way out anyway.
#[cfg(not(target_arch = "wasm32"))]
struct Session {
    terminal: Terminal<TerminaBackend<PlatformTerminal>>,
    modes: Vec<DecPrivateModeCode>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Session {
    /// Switch the modes on. A failure part-way through still restores, because
    /// the guard already owns them: resetting a mode that never went on is
    /// what the terminal does with any mode it does not know.
    fn enable(mut self) -> io::Result<Self> {
        set_modes(self.terminal.backend_mut(), &self.modes)?;
        Ok(self)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Session {
    fn drop(&mut self) {
        let _ = restore_modes(self.terminal.backend_mut(), &self.modes);
    }
}

/// Solve a theme from the colors the terminal reports about itself, and say
/// whether to subscribe to changes in them.
///
/// Any terminal that answered is subscribed to. There is no way to ask mode
/// 2031 whether it is supported, and switching it on where it is not costs
/// nothing — the notification that never comes is the same silence as not
/// asking. The light/dark verdict is deliberately *not* the signal: it is a
/// separate capability with wider support than the mode, and a verdict that
/// merely arrived after the startup fence would write off a terminal that
/// supports everything.
///
/// A terminal that cannot be asked, will not answer, or answers only in part
/// gets the [`fallback_theme`] and no subscription. So does one whose reply
/// could not be read: the same terminal is about to be drawn on, and if it is
/// broken enough to fail here the frame will say so.
///
/// Neither branch is reachable without a terminal, so both are covered by the
/// pty scenarios rather than by a test here: an answering terminal is sent
/// `?2031h` and follows a flip, a silent one is sent neither.
#[cfg(not(target_arch = "wasm32"))]
fn queried(terminal: &mut PlatformTerminal) -> (Theme, bool) {
    match ratcn::terminal_query::query(terminal) {
        Ok(Some(colors)) => (
            Theme::adaptive(
                colors.background,
                colors.foreground,
                colors.palette16.as_ref(),
            ),
            true,
        ),
        Ok(None) | Err(_) => (fallback_theme(), false),
    }
}

/// The terminal modes the host switches on, in the order it switches them on.
///
/// Termina deliberately manages none of these: they are protocol, and the
/// application decides. Restoring walks the list back off in reverse, which is
/// why the theme subscription goes on last — it comes off first, and no change
/// gets reported into a terminal that has started being put back.
#[cfg(not(target_arch = "wasm32"))]
fn modes(input: bool, paste: bool, subscribe: bool) -> Vec<DecPrivateModeCode> {
    use DecPrivateModeCode as M;

    let mut modes = vec![M::ClearAndEnableAlternateScreen];
    if input {
        // Presses, motion with a button held, motion without one, and SGR
        // coordinates — without the last, a click past column 223 cannot be
        // encoded at all.
        modes.extend([
            M::MouseTracking,
            M::ButtonEventMouse,
            M::AnyEventMouse,
            M::SGRMouse,
        ]);
    }
    if paste {
        modes.push(M::BracketedPaste);
    }
    if subscribe {
        modes.push(M::Theme);
    }
    modes
}

#[cfg(not(target_arch = "wasm32"))]
fn set_modes(out: &mut impl io::Write, modes: &[DecPrivateModeCode]) -> io::Result<()> {
    for &code in modes {
        let mode = DecPrivateMode::Code(code);
        write!(out, "{}", Csi::Mode(Mode::SetDecPrivateMode(mode)))?;
    }
    out.flush()
}

/// Switch the modes back off, newest first, and show the cursor.
///
/// A frame interrupted between hiding the cursor and showing it again leaves it
/// hidden, and a hidden cursor is invisible in the shell the user comes back to.
#[cfg(not(target_arch = "wasm32"))]
fn restore_modes(out: &mut impl io::Write, modes: &[DecPrivateModeCode]) -> io::Result<()> {
    for &code in modes.iter().rev() {
        let mode = DecPrivateMode::Code(code);
        write!(out, "{}", Csi::Mode(Mode::ResetDecPrivateMode(mode)))?;
    }
    let cursor = DecPrivateMode::Code(DecPrivateModeCode::ShowCursor);
    write!(out, "{}", Csi::Mode(Mode::SetDecPrivateMode(cursor)))?;
    out.flush()
}

/// Where the native host's waiting happens.
///
/// A demo waits on the terminal. The tests drive the same loop from a scripted
/// source, which is also how they see the waits the host asks for.
#[cfg(not(target_arch = "wasm32"))]
trait Events {
    /// Wait for the next event, for at most `timeout` — indefinitely when it is
    /// [`None`]. `Ok(None)` means the wait ran out with nothing to route.
    fn next(&mut self, timeout: Option<Duration>) -> io::Result<Option<TerminalEvent>>;
}

/// The terminal itself: poll when there is a deadline, block when there is not.
#[cfg(not(target_arch = "wasm32"))]
struct TerminalEvents(EventReader);

#[cfg(not(target_arch = "wasm32"))]
impl Events for TerminalEvents {
    fn next(&mut self, timeout: Option<Duration>) -> io::Result<Option<TerminalEvent>> {
        // Everything is taken, escape replies included: a reply that outran the
        // startup query's fence is one this host has no use for, and leaving it
        // buffered would only hand it to the next read.
        if let Some(timeout) = timeout
            && !self.0.poll(Some(timeout), |_| true)?
        {
            return Ok(None);
        }
        self.0.read(|_| true).map(Some)
    }
}

/// Draw, wait, route, repeat, until the quit key.
#[cfg(not(target_arch = "wasm32"))]
fn drive<D, B, E>(demo: &mut D, terminal: &mut Terminal<B>, events: &mut E) -> io::Result<()>
where
    D: Demo,
    B: Backend<Error = io::Error> + io::Write,
    E: Events,
{
    // Idle unless the terminal was subscribed to, and free either way: a
    // terminal that never reports a change never moves it out of `Idle`.
    //
    // A process that can be suspended would re-query on SIGCONT as well — the
    // terminal may have been re-themed while it was stopped. These demos do not
    // handle suspension, so there is no handler for it to hang off.
    let mut watch = ThemeWatch::new();
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
        let wanted = demo.wake().map(|delay| delay.max(ANIMATION_FRAME));
        let timeout = soonest(wanted, watch.wake(Instant::now()));
        let event = events.next(timeout)?;

        // The watch sees the event before the demo does, and takes nothing away
        // from it: a reply is not an app event, and a keystroke that arrived
        // mid-exchange is not a reply.
        let now = Instant::now();
        if let Some(event) = &event {
            watch.absorb(event, now);
        }
        if let Some(colors) = watch.step(terminal.backend_mut(), now)? {
            stale |= publish_theme(Theme::adaptive(
                colors.background,
                colors.foreground,
                colors.palette16.as_ref(),
            ));
        }

        let Some(event) = event else {
            // The wait ran out. Either the demo's deadline arrived or the
            // watch's did; a frame answers both, and the watch's costs one
            // frame per theme change.
            stale = true;
            continue;
        };

        if is_quit(&event) {
            return Ok(());
        }
        // A resize is not an app event: the new size reaches the demo as the
        // next frame's area, so all the host owes it is that frame.
        stale |= matches!(event, TerminalEvent::WindowResized(..));
        if let Ok(event) = Event::try_from(event) {
            stale |= demo.handle_event(event);
        }
    }
}

/// The nearer of two waits, where [`None`] is "no deadline of my own".
#[cfg(not(target_arch = "wasm32"))]
fn soonest(one: Option<Duration>, other: Option<Duration>) -> Option<Duration> {
    match (one, other) {
        (Some(one), Some(other)) => Some(one.min(other)),
        (only, None) | (None, only) => only,
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

/// Run the demo `build` returns in the browser. Returns once the host is wired
/// up; the demo keeps running on browser events and animation frames.
///
/// There is no terminal to ask here, so [`theme`] already answers and `build`
/// runs immediately.
///
/// # Errors
///
/// Returns an I/O error if the canvas backend or one of the listeners cannot be
/// installed.
#[cfg(target_arch = "wasm32")]
pub fn run_lazy<D: Demo + 'static>(build: impl FnOnce() -> D) -> io::Result<()> {
    web_host::start(build())
}

/// Ctrl+C, the demos' quit key.
#[cfg(not(target_arch = "wasm32"))]
fn is_quit(event: &TerminalEvent) -> bool {
    use ratatui_termina::termina::event::{KeyCode, KeyEventKind, Modifiers};

    matches!(
        event,
        TerminalEvent::Key(key)
            if key.kind == KeyEventKind::Press
                && key.code == KeyCode::Char('c')
                && key.modifiers.contains(Modifiers::CONTROL)
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
        layout::{Position, Size},
    };
    use ratatui_termina::termina::{
        WindowSize as TerminalSize,
        event::{KeyCode, KeyEvent as TerminalKeyEvent, Modifiers},
    };

    use super::{
        ANIMATION_FRAME, Backend, Color, Demo, Duration, Event, Events, Frame, Terminal,
        TerminalEvent, draw_frame, drive, io,
    };

    /// A [`TestBackend`] that reports a native host's error type, and that can
    /// adopt a resize while a frame is flushed — the one habit of ratzilla's
    /// canvas the policy has to answer for.
    struct HostBackend {
        inner: TestBackend,
        /// Adopted during the next flush, once.
        resize_on_flush: Option<Size>,
        /// Bytes the host wrote straight through, rather than as cells: the
        /// theme watch's re-query is the only thing that does.
        written: Vec<u8>,
    }

    impl HostBackend {
        fn new(width: u16, height: u16) -> Self {
            Self {
                inner: TestBackend::new(width, height),
                resize_on_flush: None,
                written: Vec::new(),
            }
        }

        /// Change the grid size during the next flush, the way a canvas does.
        fn resizing(mut self, width: u16, height: u16) -> Self {
            self.resize_on_flush = Some(Size::new(width, height));
            self
        }
    }

    /// The re-query the theme watch writes goes through the backend, which the
    /// real one is: `TerminaBackend` is the terminal's own writer. Discarding
    /// it here would let the watch write into a hole and no test would notice.
    impl io::Write for HostBackend {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.written.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
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
        steps: VecDeque<Option<TerminalEvent>>,
        /// The timeout the host asked for, per wait, in order.
        waits: Vec<Option<Duration>>,
        /// Whether a wait that runs out really takes that long. Off by default,
        /// because a scripted deadline is usually seconds the test has no
        /// reason to spend; on where the watch's own deadlines are what is
        /// being driven, and those are wall-clock.
        sleeps: bool,
    }

    impl Script {
        fn new(steps: impl IntoIterator<Item = Option<TerminalEvent>>) -> Self {
            Self {
                steps: steps.into_iter().collect(),
                waits: Vec::new(),
                sleeps: false,
            }
        }

        fn sleeping(mut self) -> Self {
            self.sleeps = true;
            self
        }
    }

    impl Events for Script {
        fn next(&mut self, timeout: Option<Duration>) -> io::Result<Option<TerminalEvent>> {
            self.waits.push(timeout);
            // A spent script quits, so the loop returns instead of running on.
            let step = self.steps.pop_front().unwrap_or_else(|| Some(quit()));
            if self.sleeps
                && step.is_none()
                && let Some(timeout) = timeout
            {
                std::thread::sleep(timeout);
            }
            Ok(step)
        }
    }

    fn key(code: char) -> TerminalEvent {
        TerminalEvent::Key(TerminalKeyEvent::new(KeyCode::Char(code), Modifiers::NONE))
    }

    fn quit() -> TerminalEvent {
        TerminalEvent::Key(TerminalKeyEvent::new(
            KeyCode::Char('c'),
            Modifiers::CONTROL,
        ))
    }

    fn resize(cols: u16, rows: u16) -> TerminalEvent {
        TerminalEvent::WindowResized(TerminalSize {
            cols,
            rows,
            pixel_width: None,
            pixel_height: None,
        })
    }

    /// The terminal reporting that its theme changed.
    fn re_themed() -> TerminalEvent {
        use ratatui_termina::termina::escape::csi::{Csi, Mode, ThemeMode};

        TerminalEvent::Csi(Csi::Mode(Mode::ReportTheme(ThemeMode::Light)))
    }

    /// The terminal answering one half of a colour re-query.
    fn color_reply(background: bool, red: u8, green: u8, blue: u8) -> TerminalEvent {
        use ratatui_termina::termina::{
            escape::osc::{ColorOrQuery, DynamicColorNumber, Osc},
            style::RgbColor,
        };

        let slot = if background {
            DynamicColorNumber::TextBackgroundColor
        } else {
            DynamicColorNumber::TextForegroundColor
        };
        TerminalEvent::Osc(Osc::ChangeDynamicColors(
            slot,
            vec![ColorOrQuery::Color(RgbColor::new(red, green, blue))],
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
    fn the_fallback_theme_leaves_no_color_to_the_terminal() {
        // It is chosen exactly when the terminal's polarity is unknown, so a
        // `Color::Reset` in it would be a color picked by a terminal the rest
        // of the theme was not solved for: `Theme::terminal`'s Reset text on
        // its concrete dark wells is near-black on near-black once the screen
        // turns out to be light.
        //
        // What an unasked terminal actually gets is pinned by
        // `the_published_theme_is_what_the_next_frame_reads`, which owns the
        // published theme — this one must not touch it.
        let theme = super::fallback_theme();

        for (role, color) in [
            ("foreground", theme.foreground),
            ("background", theme.background),
            ("surface", theme.surface),
            ("field", theme.field),
        ] {
            assert_ne!(color, Color::Reset, "{role} is named, not inherited");
        }
    }

    /// The published theme is one value for the whole process. Tests that move
    /// it take this in turn rather than racing over it, and put the fallback
    /// back when they are done.
    fn published_theme() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

        LOCK.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn a_solved_theme_differs_from_another_by_more_than_its_name() {
        // Every change this feature produces is `Adaptive` following `Adaptive`,
        // so a comparison that stopped at the name would report no change ever
        // and quietly switch live re-theming off.
        let _guard = published_theme();

        let light =
            ratcn::Theme::adaptive(Color::Rgb(253, 246, 227), Color::Rgb(101, 123, 131), None);
        let dark = ratcn::Theme::adaptive(Color::Rgb(26, 27, 38), Color::Rgb(192, 202, 245), None);
        assert_eq!(
            light.name, dark.name,
            "both are solved, so both are Adaptive"
        );

        assert!(super::publish_theme(light), "the first is a change");
        assert!(
            super::publish_theme(dark),
            "and so is the second, name for name identical though it is"
        );

        super::publish_theme(super::fallback_theme());
    }

    #[test]
    fn the_published_theme_is_what_the_next_frame_reads() {
        let _guard = published_theme();
        // No query has run here, so this is what an unasked terminal gets.
        assert_eq!(super::theme(), super::fallback_theme());

        let flipped = ratcn::Theme::catppuccin();
        assert!(
            super::publish_theme(flipped),
            "a different theme is a change, and the frame that shows it is owed"
        );
        assert_eq!(super::theme(), flipped, "a demo reads it on the next frame");
        assert!(
            !super::publish_theme(flipped),
            "re-publishing the same theme owes no frame: a terminal that \
             reports a change to the colors it already had costs no repaint"
        );

        assert!(super::publish_theme(super::fallback_theme()));
        assert_eq!(super::theme(), super::fallback_theme());
    }

    #[test]
    fn the_subscription_goes_on_last_so_it_comes_off_first() {
        use super::DecPrivateModeCode as M;

        let quiet = super::modes(false, false, false);
        let subscribed = super::modes(true, true, true);

        assert!(
            !quiet.contains(&M::Theme),
            "a terminal that cannot report changes is not subscribed to"
        );
        assert_eq!(
            subscribed.last(),
            Some(&M::Theme),
            "restoring walks the list backwards, so last on is first off"
        );
        assert_eq!(
            subscribed.first(),
            Some(&M::ClearAndEnableAlternateScreen),
            "and the screen the user came from is the last thing given back"
        );
    }

    #[test]
    fn restoring_switches_the_subscription_off_before_anything_else() {
        let modes = super::modes(true, true, true);
        let mut written = Vec::new();

        super::restore_modes(&mut written, &modes).expect("a vector accepts every write");

        let written = String::from_utf8(written).expect("escape sequences are ASCII");
        let subscription_off = written
            .find("\x1b[?2031l")
            .expect("the subscription is switched off");
        let screen_back = written
            .find("\x1b[?1049l")
            .expect("the alternate screen is left");

        assert!(
            subscription_off < screen_back,
            "no change may be reported into a terminal that has started being \
             put back: {written:?}"
        );
        assert!(
            written.ends_with("\x1b[?25h"),
            "and the cursor is visible again at the end: {written:?}"
        );
    }

    #[test]
    fn the_nearer_of_two_waits_wins_and_no_wait_at_all_yields() {
        let short = Duration::from_millis(10);
        let long = Duration::from_millis(500);

        assert_eq!(super::soonest(Some(long), Some(short)), Some(short));
        assert_eq!(super::soonest(Some(short), Some(long)), Some(short));
        assert_eq!(super::soonest(Some(short), None), Some(short));
        assert_eq!(super::soonest(None, Some(long)), Some(long));
        assert_eq!(super::soonest(None, None), None);
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
        let mut script = Script::new([Some(resize(30, 10))]);

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
    fn a_reported_theme_change_shortens_the_wait_a_demo_asked_to_be_endless() {
        // The demo names no deadline, so before the notification the host waits
        // for input with no timeout at all. Afterwards it owes the watch a
        // visit, and the wait it asks for has to be short enough to make it.
        let mut probe = Probe::default();
        let mut script = Script::new([Some(re_themed())]);

        run_scripted(&mut probe, HostBackend::new(20, 5), &mut script);

        assert!(
            probe.routed.is_empty(),
            "a terminal's report is not an app event: {:?}",
            probe.routed
        );
        assert_eq!(script.waits.first(), Some(&None), "endless, until the news");
        let after = script.waits.get(1).copied().flatten();
        let waited = after.expect("the watch put a deadline on the next wait");
        assert!(
            waited <= Duration::from_millis(100),
            "the re-query is due within the collapse window: {waited:?}"
        );
    }

    #[test]
    fn a_reported_change_is_re_queried_and_the_answer_reaches_the_next_frame() {
        // The whole pipeline through the host: notification in, re-query out on
        // the wire, replies in, new theme published, frame drawn. Costs the one
        // debounce window in real time, which is what the sleeping script is
        // for — the watch's deadlines are wall-clock.
        let _guard = published_theme();
        super::publish_theme(super::fallback_theme());

        let mut probe = Probe::default();
        let mut script = Script::new([
            Some(re_themed()),
            // The wait that lets the collapse window close.
            None,
            Some(color_reply(false, 101, 123, 131)),
            Some(color_reply(true, 253, 246, 227)),
        ])
        .sleeping();

        let terminal = run_scripted(&mut probe, HostBackend::new(20, 5), &mut script);

        let written = String::from_utf8(terminal.backend().written.clone())
            .expect("escape sequences are ASCII");
        assert_eq!(
            written, "\x1b]10;?\x07\x1b]11;?\x07",
            "the re-query went out through the terminal's own writer, once"
        );
        assert_eq!(
            super::theme(),
            ratcn::Theme::adaptive(Color::Rgb(253, 246, 227), Color::Rgb(101, 123, 131), None),
            "the colours the terminal answered with are what the app now wears"
        );
        assert_eq!(
            probe.frames, 3,
            "the first frame, the one the closing window asked for, and the one \
             the new theme did"
        );

        super::publish_theme(super::fallback_theme());
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
