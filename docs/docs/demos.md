---
description: "Every ratcn demo, what it shows, and how to run it — from single-component examples to three full applications you can read end to end."
---

# Demos

Every demo in the repository runs in your terminal and in the browser from the
same source. Start with the live showcase, then explore the code behind it.

## Try the live showcase

Open your terminal and run:

```sh
ssh ratcn.com
```

No Rust installation or account needed. Browse the component catalog, switch
themes, drag cards across the Kanban board, and try the larger demo apps with
your keyboard and mouse. The showcase runs real Ratatui interfaces remotely
and displays them in your terminal.

The hosted sandbox leaves out the network-dependent Effects demo. Run the
showcase locally or use the browser demos to explore the complete collection.

## Run a demo locally

Clone the repository and run any demo by name:

```sh
git clone https://github.com/kristoferlund/ratcn
cd ratcn
cargo run -p ledger93
```

The live previews on the component pages are these same demos, compiled to
WebAssembly.

## Browsing them all

```sh
cargo run -p showcase
```

Three views bring the site into your terminal: the landing page, a scrolling
Getting started guide, and a catalog with the selected demo running beside its
navigation list. Showcase embeds the demo crates rather than reimplementing them.

### Reading the source

- Start with a small demo such as [`select/src/lib.rs`](https://github.com/kristoferlund/ratcn/blob/main/demos/select/src/lib.rs) for state, messages, and component declarations.
- [`demos/shared/src/lib.rs`](https://github.com/kristoferlund/ratcn/blob/main/demos/shared/src/lib.rs) defines the single `Demo::draw(&mut Buffer, area, theme)` contract and the shared native/browser host. The host supplies the frame's buffer and area, routes events, and schedules redraws and wakeups.
- [`showcase/src/main.rs`](https://github.com/kristoferlund/ratcn/blob/main/crates/showcase/src/main.rs) owns view navigation, scroll offsets, chrome focus, and input ownership. [`catalog.rs`](https://github.com/kristoferlund/ratcn/blob/main/crates/showcase/src/catalog.rs) adapts demos for hosting; instances are constructed lazily and keep their own state and runtime.
- The Getting started view renders [`getting_started.md`](https://github.com/kristoferlund/ratcn/blob/main/crates/showcase/getting_started.md) with `tui-markdown` inside a ScrollArea. Edit the document, not a collection of Rust widgets; code highlighting is disabled.

Catalog demos draw directly into their pane. Interactive demos pass that area to
`Ratcn::render_into`; paint-only demos use ordinary widgets. Pointer coordinates
remain screen-absolute. Only the scrolling landing preview uses an
offscreen buffer, copies visible rows, and translates pointer coordinates.
The host owns allocation and clearing; the buffer contract carries no cursor
metadata and does not sandbox arbitrary base paint.

Showcase routes input to the active demo, saves and restores chrome focus, and
cancels pointer interaction when a demo loses its input session or painted area.
Its small focus-reveal helper keeps the separately painted landing preview aligned:
[`ScrollArea` focus reveal](./components/scroll-area#focus) does not emit its
effective offset to the app.

## Full applications

Start here if you want to see how the pieces fit together at real size. All
three are worth reading as source, not just running.

| Demo | Shows |
|---|---|
| `ledger93` | A nineties double-entry bookkeeping terminal. The largest example: several screens, per-screen state and messages, and a keyboard-first workflow. |
| `landing` | A responsive grid of live components, and how one app splits state and messages across independent tiles. This is the demo embedded on the [home page](/). |
| `wizard` | A four-step wizard that walks through starting a ratcn app. Buttons that move between steps, a select and a list that record choices, and one screen module per step. This is the demo embedded on [Getting started](./getting-started). |

## Patterns

Each of these shows one technique end to end.

| Demo | Shows |
|---|---|
| `kanban` | Drag and drop between columns, with cards keeping their identity as they move. See [Dragging](./concepts/dragging). |
| `drag` | The smallest possible drag: one block, moved anywhere inside the frame. |
| `panels` | Two focusable panels, each grouping its own children — scopes and Tab boundaries. See [Structuring a larger app](./concepts/composition). |
| `effects` | Fetching data without blocking the UI, and feeding the result back in as a message. See [State and messages](./concepts/state-and-messages#effects-and-result-messages). |

## Components

One demo per feature, so a docs page can show several side by side. These are
the previews embedded on the [component pages](./components/button).

| Component | Demos |
|---|---|
| [Button](./components/button) | `button-small`, `button-large` — the five variants at each size |
| [List](./components/list) | `list` (cursor and selection kept separate), `list-multi` (checkbox multi-selection), `list-people` (two-line custom rows) |
| [ScrollArea](./components/scroll-area) | `scroll-area` — ten buttons in a viewport three of them tall |
| [Select](./components/select) | `select` — the dropdown panel |
| [Tabs](./components/tabs) | `tabs-basic` (manual activation), `tabs-automatic` (focus selects), `tabs-disabled` (skipped by traversal), `tabs-large` |
| [Dialog](./components/dialog) | `dialog` — a modal layer with actions, draggable by its border |
| [Toast](./components/toast) | `toast` — transient notifications your app owns |
| [Tooltip](./components/tooltip) | `tooltip` — hover or Tab to a button and its bubble floats above |
| [Checkbox](./components/checkbox) | `checkbox` — one component as checkbox, ASCII checklist, and switch |
| [Cycle](./components/cycle) | `cycle` — settings rows with the value cycling in place |
| [Progress](./components/progress) | `progress` — a bare bar, a downloading label-and-percentage pair, and a finished one |
| [BarChart](./components/barchart) | `barchart`, `barchart-horizontal` — a paint-only widget, no runtime needed |

## Running them in the browser

The demos are also the source of the previews on this site. Building them needs
[Trunk](https://trunkrs.dev):

```sh
cd demos/ledger93
trunk serve
```

Each demo's `lib.rs` is its reusable entry point; larger demos split their
implementation across modules. The `main.rs` Trunk builds is the same one that
runs natively. Each demo's `Cargo.toml` adds ratcn's `ratzilla`
feature for `wasm32`, and the host they all share, `demos/shared`, enables
ratcn's `termina` feature for the native build. See
[Host integration](./concepts/host-integration) for how that wiring works.
