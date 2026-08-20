---
description: "Splitting state, messages, and rendering per screen once one module is not enough, without one enormous AppState and a forty-variant message enum."
---

# Structuring a larger app

One screen is easy. This page is about what happens after that — when an app has
several screens, each with its own state and messages, and you would rather not
end up with one enormous `AppState`, one `Msg` enum with forty variants, and one
`update` nobody wants to open.

Ratcn has no opinion about your module layout. It gives you two things that make
a layered structure possible: **scopes**, which group declarations without needing
a container component, and the fact that **state and messages are yours**, so
they can nest however you like.

## Grouping with scopes

A scope is a named grouping around some children. It gives them a shared path
segment, their own Tab boundary, and a focus target — with no component written
for it.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 420px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p panels</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/panels-demo/index.html" title="ratcn composition demo"></iframe>
  </div>
</div>

Two panels, each a scope. `a` and `b` jump between them, Tab cycles inside the
focused one, and Enter or Space presses a button.

```rust
ratcn.render(frame, state, &state.theme, |ctx| {
    ctx.scope(
        "panel_a",
        panel_a_area,
        ScopeOptions::default().tab_wrap(TabWrap::Wrap),
        declare_panel_a,
    );
});

fn declare_panel_a(ctx: &mut DeclareCtx<'_, AppState, Msg>) {
    ctx.component("save", Button::new("Save").on_press(|| Msg::Save), ctx.area());
}
```

The runtime discovers the button on its own — focusability needs no
announcement. A scope with nothing focusable inside — a chart, a read-out —
uses `ScopeOptions::default().focusable()`, which makes the scope itself the
Tab stop. [Focus, hover, and identity](./focus-hover-identity) covers this.

The panel border is a plain Ratatui `Block`, drawn from a `ctx.paint` closure
whose `PaintCtx::contains_focus` says whether focus is inside, so the accent
follows the user. Layout stays ordinary Ratatui code throughout.

The rest of this page is what to do with that once there is more than one
screen.

## Splitting state and messages

<div class="ratcn-preview-window" style="--ratcn-preview-height: 420px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p ledger93</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/ledger93-demo/index.html" title="ratcn LEDGER-93 demo"></iframe>
  </div>
</div>

**LEDGER-93** is a small bookkeeping app with three screens — Ledger, Report,
Settings. One module per screen, each owning its own state, messages, update, and
rendering:

```
src/
  app.rs          the shell: composes state, routes messages, draws the frame
  nav.rs          which screen is selected
  shared.rs       state more than one screen needs
  screens/
    ledger.rs     State, Msg, update, declare
    report.rs     State, Msg, update, declare
    settings.rs   State, Msg, update, declare
```

A screen module is self-contained and stays small:

```rust
// screens/ledger.rs
pub struct State { pub row: Option<&'static str>, pub list_scroll: usize }

pub enum Msg { RowFocused(&'static str, usize), ListScrolled(usize) }

impl State {
    pub fn update(&mut self, msg: Msg) { /* only this screen's concerns */ }
}

pub fn declare(ctx: &mut DeclareCtx<'_, AppState, AppMsg>) { /* ... */ }
```

The shell composes them by nesting rather than flattening:

```rust
// app.rs
pub struct AppState {
    pub focus: FocusState,
    pub nav: Nav,
    pub shared: Shared,
    pub ledger: screens::ledger::State,
    pub report: screens::report::State,
    pub settings: screens::settings::State,
}

pub enum Msg {
    Focus(FocusState),
    Nav(NavMsg),
    Ledger(screens::ledger::Msg),
    Report(screens::report::Msg),
    Settings(screens::settings::Msg),
}
```

Which turns the shell's update into a router rather than a monolith — each arm
hands the message to its owner:

```rust
match msg {
    Msg::Focus(focus) => self.state.focus = focus,
    Msg::Nav(msg) => self.state.nav.update(msg),
    Msg::Ledger(msg) => self.state.ledger.update(msg),
    Msg::Report(msg) => self.state.report.update(msg),
    Msg::Settings(msg) => self.state.settings.update(msg),
}
```

Adding a fourth screen adds one field, one variant, one arm, and one module. It
makes no existing function longer.

## Reaching app state from a screen

Component bindings are closures over the *whole* `AppState`, so a screen's
declaration function reaches through the shell:

```rust
List::new(entries)
    .item_focus(
        |s: &AppState| s.ledger.row,
        |row, offset| AppMsg::Ledger(Msg::RowFocused(row, offset)),
    )
```

The reader dives into the screen's slice; the message constructor wraps the
screen's `Msg` back into the app's. Those two closures are the only place that
knows where the screen sits inside the app — everything else in the module names
only its own types.

## Sharing state between screens

Some state belongs to no single screen. LEDGER-93 keeps a currency preference in
`shared.rs`: Settings changes it, Ledger and Report declare with it.

Give it its own module, and let the shell keep dependent screens in step when a
shared value changes. Resist reaching from one screen module into another — a
screen reading `state.settings.currency` has quietly coupled itself to Settings'
internals, where `state.shared.prefs.currency` is a contract both can depend on.

## Where focus goes on a screen change

Selecting a tab moves focus into that screen's scope:

```rust
Msg::Nav(NavMsg::Selected(screen)) => {
    self.state.nav.update(NavMsg::Selected(screen));
    self.state.focus = FocusState::intent([screen_id(screen)]);
}
```

An intent path naming just the scope is enough — the runtime descends to that
scope's first focusable child. There is no per-screen focus memory, so switching
back starts at the top of the screen again.

## See also

- [State and messages](./state-and-messages) — the ownership rules this builds on.
- [Focus, hover, and identity](./focus-hover-identity) — scopes, traversal, and
  the identity paths scopes create.
- [Rendering and event routing](./rendering-and-events) — the per-frame contract.
- [`DeclareCtx::scope`](https://docs.rs/ratcn/latest/ratcn/runtime/struct.DeclareCtx.html#method.scope)
  and [`ScopeOptions`](https://docs.rs/ratcn/latest/ratcn/runtime/struct.ScopeOptions.html)
  for every scope option.
