---
description: "What ratcn is: a component library for Ratatui apps, with paint-only widgets you can use alone and interactive components declared through a small runtime."
---

# Introduction

The `ratcn` crate is a component library for Ratatui apps: beautifully designed terminal UI
components that you can copy, paste, theme, and own in your application code.

## Getting started

The recommended way to set up a terminal project is with the `cargo-ratcn` CLI:

```sh
cargo install cargo-ratcn
cargo new my-app
cd my-app
cargo ratcn init
```

::: note Arch Linux
The CLI is available from the official
[`cargo-ratcn` package](https://archlinux.org/packages/extra/x86_64/cargo-ratcn/):

```sh
pacman -S cargo-ratcn
```
:::

See [Getting started](./getting-started) for the starter apps, and for copying
components into your project with `cargo ratcn add`.

## Preview status

This is a preview release. It works, and it is documented, but three things are
worth knowing before you build on it:

- **The API is unstable.** The public surface is still moving. Pin an exact
  version and expect to edit when you upgrade.
- **The CLI covers terminal apps.** `cargo ratcn init` configures terminal Cargo
  packages, and offers a starter app only when `main.rs` is still Cargo's
  untouched default. `cargo ratcn add` copies a built-in component into your
  project when you want to own its source.
- **Fourteen components are available:**
  [Button](./components/button), [Input](./components/input),
  [TextArea](./components/textarea), [List](./components/list),
  [ScrollArea](./components/scroll-area), [Select](./components/select),
  [Tabs](./components/tabs), [Dialog](./components/dialog),
  [Toast](./components/toast), [BarChart](./components/barchart),
  [Tooltip](./components/tooltip), [Checkbox](./components/checkbox),
  [Cycle](./components/cycle), and
  [Progress](./components/progress).

If there are components, patterns, or features you would like to see, please
[open an issue](https://github.com/kristoferlund/ratcn/issues).

## Two layers, use either one

The library has two layers, and each works on its own:

- **Paint-only widgets**, such as `ButtonWidget` and `BarChartWidget`, are
  ordinary Ratatui widgets that only paint. They drop into any Ratatui app with
  `frame.render_widget(...)`: no runtime, no message type, no change to how your
  app already works.
- **Interactive components**, such as `Button`, `List`, `Tabs`, and `Dialog`,
  add focus, keyboard and mouse handling, and messages on top. You declare them
  through the `Ratcn` runtime.

If you already have focus and event handling you like, use the widgets alone and
keep it. Nothing built that way is second-class: most interactive components
paint through these same widgets, and you can adopt the runtime later, one
component at a time.

## Your app stays in charge

The library does not own your app loop or your state. Your app owns state,
events, and updates; the library reads state while rendering and returns
messages when something happens. It enters your app at exactly two call sites,
and removing them leaves the rest of the loop untouched:

- `Ratcn::render(frame, area, state, theme, declare)` declares which components
  are on screen this frame, and paints them.
- `Ratcn::handle_event(event, state)` routes one input event and may give you a
  message back.

A typical app has three pieces:

| Piece | Role |
| --- | --- |
| `AppState` | Your state: domain data, form values, selected rows, `FocusState`, theme, open dialogs. |
| `Msg` | Your message enum. Components emit these; your `update` function applies them. |
| `Ratcn` | The runtime: remembers what was on screen last frame and routes events to it. |

Each frame, the closure you pass to `Ratcn::render` **declares** the UI. It
builds components from current state, splits areas with ordinary Ratatui
layouts, and places each interactive component where it is painted. Decorative
widgets are painted directly and need no id. Styling comes from the theme you
pass to `render`, and that is the only styling most apps touch. See
[Themes](./concepts/themes) for presets and authored palettes.

Components never write your state. A `Button` emits a message when pressed. A
`List` reads its selection from your state and emits the chosen item for you to
store. Focus works the same way: a `FocusState` lives in your `AppState`, and
focus changes come back as a message. Your `update` function is the only place
state changes.

When an event arrives, hand it to `handle_event`. The result tells you what to
do:

| Result | Meaning |
| --- | --- |
| `Emit(msg)` | A component handled the event and produced an app message. Apply it. |
| `Consumed` | A component handled the event; nothing for you to do. |
| `Ignored` | No component wanted it; your own shortcuts can have it. |

[Getting started](./getting-started) shows all of this in the smallest complete
app.

## The concepts

Each concept page covers one idea in depth. Roughly in reading order:

- [State and messages](./concepts/state-and-messages): the ownership rules.
  Your app owns state, components read it and emit messages, and `update` is
  the only writer.
- [Rendering and event routing](./concepts/rendering-and-events): how a frame
  is declared, how the runtime remembers it, and how events find the right
  component.
- [Focus, hover, and identity](./concepts/focus-hover-identity): how components
  get stable identities, how Tab traversal works, and why focus lives in your
  state while hover lives in the runtime.
- [Keyboard](./concepts/keyboard): every key the components respond to, and
  the rules that decide which component claims a key.
- [Layers and modals](./concepts/layers-and-modals): dialogs, overlays, and
  paint ordering.
- [Themes](./concepts/themes): built-in presets and authoring your own palette.
- [Host integration](./concepts/host-integration): opening and restoring the
  terminal with a `Session`, the shape of the event loop, and running in the
  browser with Ratzilla.
- [Mouse input](./concepts/mouse) and [Dragging](./concepts/dragging): enabling
  mouse support, and how clicks, hover, and drags reach components.
- [Structuring a larger app](./concepts/composition): splitting state,
  messages, and rendering per screen once one module is not enough.
- [Custom components](./concepts/custom-components): writing your own
  components, composites included, with the same powers as the built-ins.

The [component pages](./components/button) cover each built-in component's
features with live previews, and [Demos](./demos) lists every runnable example
in the repository, including three full applications.
