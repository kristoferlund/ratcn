---
description: "Every ratcn demo, what it shows, and how to run it — from single-component examples to three full applications you can read end to end."
---

# Demos

Every demo in the repository runs in your terminal and in the browser from the
same source. Clone the repository and run any of them by name:

```sh
git clone https://github.com/kristoferlund/ratcn
cd ratcn
cargo run -p ledger93
```

The live previews on the component pages are these same demos, compiled to
WebAssembly.

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

The same `main.rs` covers both targets. Each demo's `Cargo.toml` adds ratcn's
`ratzilla` feature for `wasm32`, and the host they all share, `demos/shared`,
enables ratcn's `termina` feature for the native build. See
[Host integration](./concepts/host-integration) for how that wiring works.
