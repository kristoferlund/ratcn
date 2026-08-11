---
description: "Install ratcn, pick the feature for your backend, and get a focusable button on screen — the smallest complete Ratatui app using the runtime."
---

# Getting started

The wizard below is itself a ratcn app — buttons, a select, and a list. Press `Enter` to move through it, or `Tab` into a step to make its choice. Its source is [`demos/wizard`](https://github.com/kristoferlund/ratcn/tree/main/demos/wizard.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 460px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p wizard</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/wizard-demo/index.html" title="ratcn getting started demo"></iframe>
  </div>
</div>

## Install

```sh
cargo add ratcn --features crossterm
```

`ratcn` builds on [Ratatui](https://ratatui.rs), so you need that too:

```sh
cargo add ratatui
```

### Pick a feature for your backend

| Feature | For |
|---|---|
| `crossterm` | Terminal apps — the usual choice |
| `ratzilla` | Running in the browser through [Ratzilla](https://github.com/orhun/ratzilla) |
| *(none)* | Paint-only widgets, or your own backend |

Neither feature is on by default, so you only pay for the one you use.

## A first app

The smallest complete shape: state, messages, a runtime, one function that
draws, and one that handles events.

```rust
use ratcn::{Button, Theme};
use ratcn::runtime::{EventResult, FocusState, Ratcn, TabWrap};

struct AppState {
    focus: FocusState,
    theme: Theme,
    saved: bool,
}

#[derive(Clone)]
enum Msg {
    FocusChanged(FocusState),
    Save,
}

impl AppState {
    /// The only place app state changes. A plain function of state and
    /// message: testable without a terminal, an event loop, or the runtime.
    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::FocusChanged(focus) => self.focus = focus,
            Msg::Save => self.saved = true,
        }
    }
}

struct App {
    state: AppState,
    ratcn: Ratcn<AppState, Msg>,
}

impl App {
    fn new(state: AppState) -> Self {
        let ratcn = Ratcn::new()
            .focus(|state: &AppState| &state.focus, Msg::FocusChanged)
            .tab_wrap(TabWrap::Wrap);
        Self { state, ratcn }
    }

    /// Route one event; apply whatever it produced.
    fn handle_event(&mut self, event: impl TryInto<ratcn::runtime::Event>) {
        if let EventResult::Emit(msg) = self.ratcn.handle_event(event, &self.state) {
            self.state.update(msg);
        }
    }

    fn draw(&mut self, frame: &mut ratatui::Frame) {
        let saved = self.state.saved;
        let area = frame.area();
        self.ratcn.render(frame, &self.state, &self.state.theme, |ctx| {
            let save = Button::new("Save")
                .disabled(saved)
                .on_press(|| Msg::Save);
            ctx.render_component("save", save, area);
        });
    }
}
```

Two calls do the work. `render` declares what is on screen this frame;
`handle_event` routes one input event and hands back a message if something
happened. Everything else — the loop, the terminal setup, `update` — stays
yours.

Keeping `update` in its own function means every state change is a plain call
you can test without a terminal, and messages from elsewhere (a timer, a
background task) get the same single path into state.

Wiring this into a real event loop, native or browser, is covered in
[Host integration](./concepts/host-integration).

## Just want the look?

You do not have to adopt the runtime. Every interactive component paints
through a plain Ratatui widget that you can use on its own:

```rust
frame.render_widget(
    ButtonWidget::new("Save").themed(&theme).focused(is_focused),
    area,
);
```

Give it a theme and a couple of bools and that is the whole integration. If you
already have focus and event handling you like, keep it — and adopt the runtime
later, one component at a time, if you want to.

## Try it before you build it

Every demo runs in your terminal from a checkout of the repository:

```sh
git clone https://github.com/kristoferlund/ratcn
cd ratcn
cargo run -p ledger93
```

See [Demos](./demos) for what each one shows.

## Where to go next

- **[Demos](./demos)** — run something and read its source.
- **[Components](./components/button)** — what each built-in can do, with live
  previews.
- **[State and messages](./concepts/state-and-messages)** — the ownership rules
  everything else builds on. The best next read if you plan to build something
  real.
- **[Themes](./concepts/themes)** — presets, and writing your own palette.
