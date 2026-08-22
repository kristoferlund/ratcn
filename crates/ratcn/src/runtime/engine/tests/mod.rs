//! The engine's own tests, and the fixtures they share.

use std::{
    cell::RefCell,
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use super::*;
use crate::runtime::PopupOptions;
use crate::test_support::{Driver, mouse};
use crate::{
    Button, Dialog,
    runtime::{CellOffset, DragOptions, DragPhase, KeyChord, Modifiers},
};

mod declaration;
mod focus;
mod hover;
mod modal;
mod paint;
mod pointer;
mod popup;
mod viewport;

struct Leaf;

impl Component<(), ()> for Leaf {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, (), ()>) {}

    fn handle_event(
        &mut self,
        _event: &Event,
        _state: &(),
        _ctx: &mut EventCtx<'_>,
    ) -> EventResult<()> {
        EventResult::Ignored
    }
}

#[derive(Default)]
struct FocusTestState {
    focus: FocusState,
}

#[derive(Default)]
struct ModalTestState {
    focus: FocusState,
    modals: ModalState,
}

#[derive(Debug, Clone, PartialEq)]
enum ModalTestMsg {
    Routed(&'static str),
    Focus(FocusState),
}

#[derive(Default)]
struct ButtonTimingState {
    focus: FocusState,
    saving: bool,
    accepted_saves: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum ButtonTimingMsg {
    Focus(FocusState),
    Save,
    Replacement,
}

fn update_button_timing(state: &mut ButtonTimingState, msg: ButtonTimingMsg) -> bool {
    match msg {
        ButtonTimingMsg::Focus(focus) => {
            state.focus = focus;
            true
        }
        ButtonTimingMsg::Save if !state.saving => {
            state.saving = true;
            state.accepted_saves += 1;
            true
        }
        ButtonTimingMsg::Save | ButtonTimingMsg::Replacement => false,
    }
}

#[derive(Debug, PartialEq)]
enum FocusTestMsg {
    Focus(FocusState),
    Activated(Vec<ChildId>),
    Parent(Vec<ChildId>),
}

/// A driver whose focus lives in [`FocusTestState`].
fn focus_driver(width: u16, height: u16) -> Driver<FocusTestState, FocusTestMsg> {
    Driver::with(
        Ratcn::new().focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus),
        width,
        height,
    )
}

/// The same, with the root scope wrapping Tab at its ends.
fn wrapping_focus_driver(width: u16, height: u16) -> Driver<FocusTestState, FocusTestMsg> {
    Driver::with(
        Ratcn::new()
            .focus(|state: &FocusTestState| &state.focus, FocusTestMsg::Focus)
            .tab_wrap(TabWrap::Wrap),
        width,
        height,
    )
}

type FocusRenderLog = Rc<RefCell<Vec<(bool, bool)>>>;

struct FocusLeaf {
    enabled: bool,
    consume_focus_key: bool,
    rendered: Option<FocusRenderLog>,
}

impl FocusLeaf {
    fn enabled() -> Self {
        Self {
            enabled: true,
            consume_focus_key: false,
            rendered: None,
        }
    }

    fn disabled() -> Self {
        Self {
            enabled: false,
            consume_focus_key: false,
            rendered: None,
        }
    }

    fn recording(rendered: FocusRenderLog) -> Self {
        Self {
            enabled: true,
            consume_focus_key: false,
            rendered: Some(rendered),
        }
    }

    fn consuming_focus_key() -> Self {
        Self {
            enabled: true,
            consume_focus_key: true,
            rendered: None,
        }
    }
}

impl Component<FocusTestState, FocusTestMsg> for FocusLeaf {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, FocusTestState, FocusTestMsg>) {}

    fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, FocusTestState>) {
        if let Some(rendered) = &self.rendered {
            rendered
                .borrow_mut()
                .push((ctx.focused(), ctx.contains_focus()));
        }
    }

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &FocusTestState,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<FocusTestMsg> {
        if !self.enabled {
            return EventResult::Ignored;
        }
        match event {
            Event::Key(key) if self.consume_focus_key && key.code == KeyCode::Char('x') => {
                EventResult::Consumed
            }
            Event::Key(key) if key.code == KeyCode::Enter => {
                EventResult::Emit(FocusTestMsg::Activated(ctx.path().to_vec()))
            }
            _ => EventResult::Ignored,
        }
    }

    fn is_focusable(&self) -> bool {
        self.enabled
    }
}

fn render_leaf(driver: &mut Driver<(), ()>, id: &ChildId) {
    let area = driver.area();
    driver.render(&(), |ctx| {
        ctx.component(id.clone(), Leaf, area);
    });
}

/// The pointer tests need no app state at all: hover is the runtime's.
#[derive(Debug, Default)]
struct PointerState;

#[derive(Debug, Clone, PartialEq)]
enum PointerMsg {
    Routed(&'static str, MouseKind, usize),
    Transient(usize),
    Drag(DragPhase),
    Dismissed,
}

#[derive(Debug, Default)]
struct HoverFocusState {
    focus: FocusState,
}

#[derive(Debug, Clone, PartialEq)]
enum HoverFocusMsg {
    Focus(FocusState),
}

struct HoverFocusLeaf {
    enabled: bool,
}

impl HoverFocusLeaf {
    fn enabled() -> Self {
        Self { enabled: true }
    }

    fn disabled() -> Self {
        Self { enabled: false }
    }
}

impl Component<HoverFocusState, HoverFocusMsg> for HoverFocusLeaf {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, HoverFocusState, HoverFocusMsg>) {}

    fn is_focusable(&self) -> bool {
        self.enabled
    }
}

#[derive(Default)]
struct DragTransient {
    events: usize,
}

struct Draggable {
    name: &'static str,
}

impl Component<PointerState, PointerMsg> for Draggable {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, PointerState, PointerMsg>) {}

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &PointerState,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<PointerMsg> {
        let Event::Mouse(mouse) = event else {
            return EventResult::Ignored;
        };
        match mouse.kind {
            MouseKind::Down(MouseButton::Left) => {
                ctx.capture_pointer(MouseButton::Left);
                ctx.transient::<DragTransient>().events += 1;
                EventResult::Consumed
            }
            MouseKind::Drag(MouseButton::Left)
            | MouseKind::Up(MouseButton::Left)
            | MouseKind::Click(MouseButton::Left)
            | MouseKind::DragEnd(MouseButton::Left) => {
                let transient = ctx.transient::<DragTransient>();
                transient.events += 1;
                EventResult::Emit(PointerMsg::Routed(self.name, mouse.kind, transient.events))
            }
            _ => EventResult::Ignored,
        }
    }
}

struct HoverLeaf {
    consume_move: bool,
    rendered: Option<HoverRenderLog>,
}

impl Component<PointerState, PointerMsg> for HoverLeaf {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, PointerState, PointerMsg>) {}

    fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, PointerState>) {
        if let Some(rendered) = &self.rendered {
            rendered
                .borrow_mut()
                .push((ctx.hovered(), ctx.contains_hover()));
        }
    }

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &PointerState,
        _ctx: &mut EventCtx<'_>,
    ) -> EventResult<PointerMsg> {
        if self.consume_move
            && matches!(
                event,
                Event::Mouse(MouseEvent {
                    kind: MouseKind::Moved,
                    ..
                })
            )
        {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }
}

type HoverRenderLog = Rc<RefCell<Vec<(bool, bool)>>>;

struct RouteLeaf(&'static str);

impl Component<PointerState, PointerMsg> for RouteLeaf {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, PointerState, PointerMsg>) {}

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &PointerState,
        _ctx: &mut EventCtx<'_>,
    ) -> EventResult<PointerMsg> {
        match event {
            Event::Mouse(mouse) => EventResult::Emit(PointerMsg::Routed(self.0, mouse.kind, 0)),
            _ => EventResult::Ignored,
        }
    }
}

fn render_drag_surface(
    driver: &mut Driver<PointerState, PointerMsg>,
    state: &PointerState,
    ids: &[(&'static str, &'static str, Rect)],
) {
    driver.render(state, |ctx| {
        for &(id, name, area) in ids {
            ctx.component(ChildId::Static(id), Draggable { name }, area);
        }
    });
}

struct LoggingComponent {
    name: &'static str,
    log: Rc<RefCell<Vec<&'static str>>>,
    focusable: bool,
}

impl Component<FocusTestState, FocusTestMsg> for LoggingComponent {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, FocusTestState, FocusTestMsg>) {}

    fn paint(&mut self, _ctx: &mut PaintCtx<'_, '_, FocusTestState>) {
        self.log.borrow_mut().push(self.name);
    }

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &FocusTestState,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<FocusTestMsg> {
        if matches!(event, Event::Key(_)) {
            EventResult::Emit(FocusTestMsg::Activated(ctx.path().to_vec()))
        } else {
            EventResult::Ignored
        }
    }

    fn is_focusable(&self) -> bool {
        self.focusable
    }
}
