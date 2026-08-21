---
description: "The ownership rules in ratcn: your app owns state, components read it and return messages, and your update function is the only writer."
---

# State and messages

Your app owns everything with durable meaning: domain data, form values,
selected rows, focus, open modals, theme selection, toasts. Components read
that state and return messages; they never mutate it. Hover is the one
interaction path the runtime keeps for itself — see
[Focus, hover, and identity](./focus-hover-identity).

The pattern is one app-specific `Msg` enum and one `update` function that is
the only place state changes. When `handle_event` returns
`EventResult::Emit(msg)`, apply it through `update` and redraw:

```rust
struct AppState {
    focus: FocusState,
    fruit: Option<&'static str>,
    saved: bool,
}

enum Msg {
    FocusChanged(FocusState),
    FruitSelected(&'static str),
    Save,
}

fn update(state: &mut AppState, msg: Msg) {
    match msg {
        Msg::FocusChanged(focus) => state.focus = focus,
        Msg::FruitSelected(fruit) => state.fruit = Some(fruit),
        Msg::Save => state.saved = true,
    }
}

let ratcn = Ratcn::new()
    .focus(|state: &AppState| &state.focus, Msg::FocusChanged);
let fruits = List::new(["Mango", "Papaya"].map(|name| ListItem::new(name, name)))
    .selection(|state: &AppState| state.fruit, Msg::FruitSelected);
```

Because `update` is a plain function of state and message, every state
transition is testable without a terminal, and messages from other sources — a
background task, a timer — take the same single path into state.

## Controlled state

Values the user edits — list cursors, scroll positions, tab selection — are
**controlled**: the component gets a read accessor that
supplies the current value, and an `on_*` handler that wraps the next value in
a message. Your `update` stores the new value; the component never does.

The read side is a closure over your state rather than a plain value so that
fast consecutive edits compose correctly — each edit starts from the state the
previous edit produced, even before a redraw happens.
[Rendering and event routing](./rendering-and-events#what-an-event-sees)
explains the mechanics.

When two controlled values move together, persist both with one message. A
scroll-bound `List` passes both the newly focused item and the resulting scroll
offset, so one `update` arm keeps them in step:

```rust
List::new(items).item_focus(
    |state: &AppState| state.focused_item,
    |item, offset| Msg::ListFocused { item, offset },
);

Msg::ListFocused { item, offset } => {
    state.focused_item = Some(item);
    state.list_scroll = offset;
}
```

Focus follows the same rule: `Ratcn::focus(read, on_change)` reads from your
state and emits messages for your `update` to store. The runtime computes the
next path but never writes your state behind your back.

## Effects and result messages

Keep I/O out of `update`. A clean pattern is to have `update` return an
app-specific effect value, let the host execute it, and feed the result back
through the same message path:

```rust
enum Msg {
    RefreshRequested,
    JokeFetchCompleted(Result<String, String>),
}

enum Effect {
    FetchJoke,
}

fn update(state: &mut AppState, msg: Msg) -> Option<Effect> {
    match msg {
        Msg::RefreshRequested if !state.joke.is_loading() => {
            state.joke = JokeState::Loading;
            Some(Effect::FetchJoke)
        }
        Msg::JokeFetchCompleted(Ok(joke)) => {
            state.joke = JokeState::Ready(joke);
            None
        }
        Msg::JokeFetchCompleted(Err(error)) => {
            state.joke = JokeState::Failed(error);
            None
        }
        _ => None,
    }
}
```

The app applies `update`, then executes any returned effect; the completion
callback sends its result message through a channel instead of touching state
directly. Ratcn defines none of this — `Effect`, the queue, and the executor
are ordinary application code, which means they are also yours to shape.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 340px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p effects</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/effects-demo/index.html" title="ratcn effects demo"></iframe>
  </div>
</div>

The effects demo requests one joke at startup and another when its button emits
`RefreshRequested`. Its `update` function, HTTP executor, and native/browser
completion queues are in
[`demos/effects`](https://github.com/kristoferlund/ratcn/tree/main/demos/effects).
Jokes are supplied by [icanhazdadjoke.com](https://icanhazdadjoke.com/).

## Splitting up a larger app

One `Ratcn<State, Msg>` uses one app-wide state and message type, but that does
not mean one flat struct and one giant match. A screen can own its own `State`,
local `Msg`, and `State::update`; the app message wraps the local message and
the shell delegates:

```rust
enum AppMsg {
    FocusChanged(FocusState),
    Toast(Toast<'static>),
    Settings(settings::Msg),
}

match msg {
    AppMsg::FocusChanged(focus) => state.focus = focus,
    AppMsg::Toast(toast) => state.toasts.push(toast, now),
    AppMsg::Settings(msg) => state.settings.update(msg),
}
```

Components still emit the app-wide type; lift the local message at the
component callback:

```rust
List::new(theme_names).selection(
    |state: &AppState| state.settings.theme,
    |theme| AppMsg::Settings(settings::Msg::ThemeSelected(theme)),
)
```

Split by state ownership, not by visual containers, and keep cross-cutting
state — focus, modals, shared preferences — with its actual owner.
[Structuring a larger app](./composition) develops this into a full multi-screen
layout.

## What Ratcn keeps

Ratcn retains one thing between frames: the retained surface from the last
successful render — component instances, identities, geometry — so events have
something to route through. It also holds short-lived gesture state, such as a
drag in progress. Neither is a second copy of your application model: nothing
durable lives inside the runtime, and everything with lasting meaning stays in
your `AppState` where you can read, test, and persist it.

One practical consequence: not every mutation needs a message. Messages are for
component output and app decisions. Deterministic host housekeeping — say, a
timed wakeup pruning expired toasts with `ToasterState::prune_expired(now)` —
can act directly in the loop that owns the trigger. See
[Host integration](./host-integration) for loop shapes.
