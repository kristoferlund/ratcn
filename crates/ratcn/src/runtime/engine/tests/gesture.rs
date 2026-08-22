//! What one raw mouse event turns into, traced end to end.
//!
//! Every other pointer test asserts one rule; these record the whole
//! machine's output — which normalized events each raw event produced, where
//! they were delivered, and what the runtime answered — so that a change to
//! how gestures are tracked has to reproduce the trace exactly.

use super::*;
use crate::runtime::ScrollDirection;

#[derive(Default)]
struct GestureState {
    focus: FocusState,
    modals: ModalState,
}

#[derive(Debug, Clone, PartialEq)]
enum GestureMsg {
    Focus,
}

type Log = Rc<RefCell<Vec<String>>>;

/// Writes every mouse event it is handed into the shared log, and optionally
/// claims the gesture on the press. One that does not handle what it logs
/// lets the event bubble on to its ancestors.
#[derive(Clone)]
struct Recorder {
    name: &'static str,
    log: Log,
    capture: bool,
    handles: bool,
}

impl Component<GestureState, GestureMsg> for Recorder {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, GestureState, GestureMsg>) {}

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &GestureState,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<GestureMsg> {
        let Event::Mouse(mouse) = event else {
            return EventResult::Ignored;
        };
        self.log.borrow_mut().push(format!(
            "  -> {} {:?} @{},{}",
            self.name, mouse.kind, mouse.column, mouse.row
        ));
        if self.capture
            && let MouseKind::Down(button) = mouse.kind
        {
            ctx.capture_pointer(button);
        }
        if self.handles {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }
}

/// A [`Recorder`] that declares a child of its own, so a press the child
/// leaves alone bubbles up to it and it claims the gesture from above the
/// node the pointer actually hit.
struct Ancestor {
    outer: Recorder,
    inner: Recorder,
}

impl Component<GestureState, GestureMsg> for Ancestor {
    fn declare(&mut self, ctx: &mut DeclareCtx<'_, GestureState, GestureMsg>) {
        ctx.component(ChildId::Static("inner"), self.inner.clone(), ctx.area());
    }

    fn handle_event(
        &mut self,
        event: &Event,
        state: &GestureState,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<GestureMsg> {
        self.outer.handle_event(event, state, ctx)
    }
}

struct Probe {
    driver: Driver<GestureState, GestureMsg>,
    log: Log,
    trace: Vec<String>,
}

impl Probe {
    fn new() -> Self {
        Self {
            driver: Driver::with(
                Ratcn::new()
                    .focus(|state: &GestureState| &state.focus, |_| GestureMsg::Focus)
                    .modals(|state: &GestureState| &state.modals),
                20,
                3,
            ),
            log: Rc::new(RefCell::new(Vec::new())),
            trace: Vec::new(),
        }
    }

    /// Declare `left` at columns 0..10 and `right` at 10..20, either of them
    /// dropped by passing `false`.
    fn render(&mut self, state: &GestureState, left: bool, right: bool, capture: bool) {
        let log = Rc::clone(&self.log);
        self.driver.render(state, |ctx| {
            if left {
                ctx.component(
                    ChildId::Static("left"),
                    Recorder {
                        name: "left",
                        log: Rc::clone(&log),
                        capture,
                        handles: true,
                    },
                    Rect::new(0, 0, 10, 3),
                );
            }
            if right {
                ctx.component(
                    ChildId::Static("right"),
                    Recorder {
                        name: "right",
                        log: Rc::clone(&log),
                        capture,
                        handles: true,
                    },
                    Rect::new(10, 0, 10, 3),
                );
            }
        });
    }

    /// Feed one raw event and record the answer plus everything it delivered.
    fn step(&mut self, state: &GestureState, kind: MouseKind, column: u16, row: u16) {
        let result = self.driver.event(mouse(kind, column, row), state);
        self.trace
            .push(format!("{kind:?} @{column},{row} => {result:?}"));
        self.trace.extend(self.log.borrow_mut().drain(..));
    }

    /// Declare "outer" at columns 0..10 with a child filling it, so a press
    /// there hits the child and bubbles to the parent, and "right" at 10..20.
    fn render_nested(&mut self, state: &GestureState, capture: bool) {
        let log = Rc::clone(&self.log);
        self.driver.render(state, |ctx| {
            ctx.component(
                ChildId::Static("outer"),
                Ancestor {
                    outer: Recorder {
                        name: "outer",
                        log: Rc::clone(&log),
                        capture,
                        handles: true,
                    },
                    inner: Recorder {
                        name: "inner",
                        log: Rc::clone(&log),
                        capture: false,
                        handles: false,
                    },
                },
                Rect::new(0, 0, 10, 3),
            );
            ctx.component(
                ChildId::Static("right"),
                Recorder {
                    name: "right",
                    log: Rc::clone(&log),
                    capture: false,
                    handles: true,
                },
                Rect::new(10, 0, 10, 3),
            );
        });
    }

    fn note(&mut self, note: &str) {
        self.trace.push(format!("[{note}]"));
    }

    fn trace(&self) -> String {
        self.trace.join("\n")
    }
}

const LEFT: MouseButton = MouseButton::Left;

#[test]
fn hit_tested_gesture_trace() {
    let state = GestureState::default();
    let mut probe = Probe::new();
    probe.render(&state, true, true, false);

    probe.note("motion with nothing held is hover");
    probe.step(&state, MouseKind::Moved, 2, 1);
    probe.note("a press, motion inside its cell, then motion that leaves it");
    probe.step(&state, MouseKind::Down(LEFT), 2, 1);
    probe.step(&state, MouseKind::Moved, 2, 1);
    probe.step(&state, MouseKind::Drag(LEFT), 4, 1);
    probe.note("released on the press target: drift does not eat the click");
    probe.step(&state, MouseKind::Up(LEFT), 4, 1);

    probe.note("pressed left, released over right: no click, and it moved");
    probe.step(&state, MouseKind::Down(LEFT), 2, 1);
    probe.step(&state, MouseKind::Moved, 12, 1);
    probe.step(&state, MouseKind::Up(LEFT), 12, 1);

    probe.note("left the press target and came back: the click revives");
    probe.step(&state, MouseKind::Down(LEFT), 2, 1);
    probe.step(&state, MouseKind::Moved, 12, 1);
    probe.step(&state, MouseKind::Moved, 3, 1);
    probe.step(&state, MouseKind::Up(LEFT), 3, 1);

    probe.note("released elsewhere without ever moving: nothing follows the up");
    probe.step(&state, MouseKind::Down(LEFT), 2, 1);
    probe.step(&state, MouseKind::Up(LEFT), 12, 1);

    probe.note("the wheel passes through untouched");
    probe.step(&state, MouseKind::Scroll(ScrollDirection::Down), 2, 1);

    probe.note("the pointer leaves the grid");
    probe.step(&state, MouseKind::Exited, 0, 0);

    compare_trace(&probe.trace(), HIT_TESTED);
}

const HIT_TESTED: &str = "\
[motion with nothing held is hover]
Moved @2,1 => Consumed
  -> left Moved @2,1
[a press, motion inside its cell, then motion that leaves it]
Down(Left) @2,1 => Consumed
  -> left Down(Left) @2,1
Moved @2,1 => Consumed
Drag(Left) @4,1 => Consumed
  -> left Drag(Left) @4,1
[released on the press target: drift does not eat the click]
Up(Left) @4,1 => Consumed
  -> left Up(Left) @4,1
  -> left Click(Left) @4,1
[pressed left, released over right: no click, and it moved]
Down(Left) @2,1 => Consumed
  -> left Down(Left) @2,1
Moved @12,1 => Consumed
  -> right Drag(Left) @12,1
Up(Left) @12,1 => Consumed
  -> right Up(Left) @12,1
  -> right DragEnd(Left) @12,1
[left the press target and came back: the click revives]
Down(Left) @2,1 => Consumed
  -> left Down(Left) @2,1
Moved @12,1 => Consumed
  -> right Drag(Left) @12,1
Moved @3,1 => Consumed
  -> left Drag(Left) @3,1
Up(Left) @3,1 => Consumed
  -> left Up(Left) @3,1
  -> left Click(Left) @3,1
[released elsewhere without ever moving: nothing follows the up]
Down(Left) @2,1 => Consumed
  -> left Down(Left) @2,1
Up(Left) @12,1 => Consumed
  -> right Up(Left) @12,1
[the wheel passes through untouched]
Scroll(Down) @2,1 => Consumed
  -> left Scroll(Down) @2,1
[the pointer leaves the grid]
Exited @0,0 => Consumed";

#[test]
fn captured_gesture_trace() {
    let state = GestureState::default();
    let mut probe = Probe::new();
    probe.render(&state, true, true, true);

    probe.note("a claimed press owns the pointer wherever it goes");
    probe.step(&state, MouseKind::Down(LEFT), 2, 1);
    probe.step(&state, MouseKind::Moved, 12, 1);
    probe.step(&state, MouseKind::Up(LEFT), 12, 1);

    probe.note("a claim that never moved is still a click");
    probe.step(&state, MouseKind::Down(LEFT), 2, 1);
    probe.step(&state, MouseKind::Up(LEFT), 2, 1);

    probe.note("a claim that moved and came back ends as a drag, not a click");
    probe.step(&state, MouseKind::Down(LEFT), 2, 1);
    probe.step(&state, MouseKind::Moved, 4, 1);
    probe.step(&state, MouseKind::Up(LEFT), 2, 1);

    compare_trace(&probe.trace(), CAPTURED);
}

const CAPTURED: &str = "\
[a claimed press owns the pointer wherever it goes]
Down(Left) @2,1 => Consumed
  -> left Down(Left) @2,1
Moved @12,1 => Consumed
  -> left Drag(Left) @12,1
Up(Left) @12,1 => Consumed
  -> left Up(Left) @12,1
  -> left DragEnd(Left) @12,1
[a claim that never moved is still a click]
Down(Left) @2,1 => Consumed
  -> left Down(Left) @2,1
Up(Left) @2,1 => Consumed
  -> left Up(Left) @2,1
  -> left Click(Left) @2,1
[a claim that moved and came back ends as a drag, not a click]
Down(Left) @2,1 => Consumed
  -> left Down(Left) @2,1
Moved @4,1 => Consumed
  -> left Drag(Left) @4,1
Up(Left) @2,1 => Consumed
  -> left Up(Left) @2,1
  -> left DragEnd(Left) @2,1";

#[test]
fn suppressed_gesture_trace() {
    let state = GestureState::default();
    let mut probe = Probe::new();
    probe.render(&state, true, true, true);

    probe.note("a claimed press whose component the next frame drops");
    probe.step(&state, MouseKind::Down(LEFT), 2, 1);
    probe.render(&state, false, true, true);
    probe.note("every event for that button is swallowed until its release");
    probe.step(&state, MouseKind::Moved, 12, 1);
    probe.step(&state, MouseKind::Up(LEFT), 12, 1);
    probe.note("the next press starts clean");
    probe.step(&state, MouseKind::Down(LEFT), 12, 1);
    probe.step(&state, MouseKind::Up(LEFT), 12, 1);

    compare_trace(&probe.trace(), SUPPRESSED);
}

const SUPPRESSED: &str = "\
[a claimed press whose component the next frame drops]
Down(Left) @2,1 => Consumed
  -> left Down(Left) @2,1
[every event for that button is swallowed until its release]
Moved @12,1 => Consumed
Up(Left) @12,1 => Consumed
[the next press starts clean]
Down(Left) @12,1 => Consumed
  -> right Down(Left) @12,1
Up(Left) @12,1 => Consumed
  -> right Up(Left) @12,1
  -> right Click(Left) @12,1";

#[test]
fn stale_modal_window_gesture_trace() {
    let mut state = GestureState::default();
    let mut probe = Probe::new();
    probe.render(&state, true, true, false);

    probe.note("the app opens a modal the retained surface has not painted yet");
    state
        .modals
        .open("dialog", &mut state.focus)
        .expect("open dialog");
    probe.step(&state, MouseKind::Down(LEFT), 2, 1);
    probe.step(&state, MouseKind::Moved, 4, 1);
    probe.step(&state, MouseKind::Up(LEFT), 4, 1);
    probe.note("a press that begins inside the gap is suppressed through it");
    probe.step(&state, MouseKind::Down(LEFT), 2, 1);
    let _ = state.modals.close(&mut state.focus);
    probe.step(&state, MouseKind::Up(LEFT), 2, 1);
    probe.note("and the gesture after the gap routes normally");
    probe.step(&state, MouseKind::Down(LEFT), 2, 1);
    probe.step(&state, MouseKind::Up(LEFT), 2, 1);

    compare_trace(&probe.trace(), STALE_MODAL);
}

const STALE_MODAL: &str = "\
[the app opens a modal the retained surface has not painted yet]
Down(Left) @2,1 => Consumed
Moved @4,1 => Consumed
Up(Left) @4,1 => Consumed
[a press that begins inside the gap is suppressed through it]
Down(Left) @2,1 => Consumed
Up(Left) @2,1 => Consumed
[and the gesture after the gap routes normally]
Down(Left) @2,1 => Consumed
  -> left Down(Left) @2,1
Up(Left) @2,1 => Consumed
  -> left Up(Left) @2,1
  -> left Click(Left) @2,1";

#[test]
fn bubbled_press_gesture_trace() {
    let state = GestureState::default();
    let mut probe = Probe::new();

    probe.render_nested(&state, false);
    probe.note("a press the child leaves alone is still the child's press target");
    probe.step(&state, MouseKind::Down(LEFT), 2, 1);
    probe.step(&state, MouseKind::Up(LEFT), 2, 1);

    probe.render_nested(&state, true);
    probe.note("claimed from above the hit node, the claim is the press target");
    probe.step(&state, MouseKind::Down(LEFT), 2, 1);
    probe.note("so a release on the child lands somewhere else, and never clicks");
    probe.step(&state, MouseKind::Up(LEFT), 2, 1);
    probe.note("nor does one that leaves the claimant altogether");
    probe.step(&state, MouseKind::Down(LEFT), 2, 1);
    probe.step(&state, MouseKind::Up(LEFT), 12, 1);

    compare_trace(&probe.trace(), BUBBLED_PRESS);
}

const BUBBLED_PRESS: &str = "\
[a press the child leaves alone is still the child's press target]
Down(Left) @2,1 => Consumed
  -> inner Down(Left) @2,1
  -> outer Down(Left) @2,1
Up(Left) @2,1 => Consumed
  -> inner Up(Left) @2,1
  -> outer Up(Left) @2,1
  -> inner Click(Left) @2,1
  -> outer Click(Left) @2,1
[claimed from above the hit node, the claim is the press target]
Down(Left) @2,1 => Consumed
  -> inner Down(Left) @2,1
  -> outer Down(Left) @2,1
[so a release on the child lands somewhere else, and never clicks]
Up(Left) @2,1 => Consumed
  -> outer Up(Left) @2,1
[nor does one that leaves the claimant altogether]
Down(Left) @2,1 => Consumed
  -> inner Down(Left) @2,1
  -> outer Down(Left) @2,1
Up(Left) @12,1 => Consumed
  -> outer Up(Left) @12,1";

/// Compare two traces line by line, so a mismatch names the line that moved.
fn compare_trace(actual: &str, expected: &str) {
    if actual != expected {
        for (index, (actual, expected)) in actual.lines().zip(expected.lines()).enumerate() {
            assert_eq!(actual, expected, "trace diverges at line {index}");
        }
        assert_eq!(actual, expected);
    }
}
