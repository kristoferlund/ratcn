---
description: "What ratcn is: a component library for Ratatui apps, with paint-only widgets you can use alone and interactive components declared through a small runtime."
---

# Introduction

`ratcn` is a component library for Ratatui apps: beautifully designed terminal UI
components that you can copy, paste, theme, and own in your application code.

## Preview status

ratcn is a preview release. It works, and it is documented, but three things
are worth knowing before you build on it:

- **The API will break.** The public surface is still moving — recent work has
  renamed methods, changed signatures, and removed components outright. Pin an
  exact version and expect to edit when you upgrade.
- **There is no install command.** The shadcn resemblance is in how the code is
  structured, not yet in tooling. Copying a component into your project is a
  manual file copy today. A CLI is intended, but it does not exist.
- **The component set is small and growing.** Eight components ship today:
  [Button](./components/button), [List](./components/list),
  [Select](./components/select), [Tabs](./components/tabs),
  [Dialog](./components/dialog), [Toast](./components/toast),
  [BarChart](./components/barchart), and [Tooltip](./components/tooltip).
  Notably missing and planned next are **text input**, **multi-line text
  area**, and a **scroll area** for content taller than its viewport. Input and
  TextArea existed in an earlier preview and were withdrawn pending upstream
  fixes in the text-editing crate they wrap.

If there are specific components, patterns, or features you would like to see
included, please [open an issue](https://github.com/kristoferlund/ratcn/issues).

## Two layers, use either one

The library has two layers, and each works on its own:

- **Paint-only widgets** — `ButtonWidget`, `BarChartWidget`, and friends.
  Ordinary Ratatui widgets that just draw. They drop into any Ratatui app with
  `frame.render_widget(...)`: no runtime, no message type, no change to how your
  app already works.
- **Interactive components** — `Button`, `List`, `Tabs`, `Dialog`, and more.
  These add focus, keyboard and mouse handling, and messages on top, and are
  declared through the `Ratcn` runtime.

If you already have focus and event handling you like, use the widgets alone and
keep it. Nothing built that way is second-class — the interactive components
paint through the very same widgets — and you can adopt the runtime later, one
component at a time.

## Your app stays in charge

`ratcn` does not own your app loop or your state. Your app owns state, events,
and updates; the library reads state while rendering and returns messages when
something happens. It enters your app at exactly two call sites — remove them
and the rest of the loop is untouched:

- `Ratcn::render(frame, state, theme, declare)` — paint one frame and declare
  which components are on screen.
- `Ratcn::handle_event(event, state)` — route one input event and maybe get a
  message back.

A typical app has three pieces:

| Piece | Role |
| --- | --- |
| `AppState` | Your state: domain data, form values, selected rows, `FocusState`, theme, open dialogs. |
| `Msg` | Your message enum. Components emit these; your `update` function applies them. |
| `Ratcn` | The runtime: remembers what was on screen last frame and routes events to it. |

Each frame, the closure you pass to `Ratcn::render` **declares** the UI: build
components from current state, split areas with ordinary Ratatui layouts, and
place each interactive component where it is painted. Decorative widgets are
painted directly and need no ID or registration.

Components never write your state. A `Button` emits a message when pressed. A
`List` reads its selection from your state and emits the chosen item for you to
store. Focus works the same way: a `FocusState` lives in your `AppState`, and
focus changes come back as a message. Your `update` function is the only place
state changes.

When an event arrives, hand it to `handle_event`. The result tells you what to
do:

| Result | Meaning |
| --- | --- |
| `Emit(msg)` | A component handled the event and produced an app message — apply it. |
| `Consumed` | A component handled the event; nothing for you to do. |
| `Ignored` | No component wanted it; your own shortcuts can have it. |

## A first app

See [Getting started](./getting-started) for installation, the feature flag to
pick for your backend, and the smallest complete app using the runtime.

Styling comes from the theme passed to `Ratcn::render`, and that is the only
styling most apps touch. See [Themes](./concepts/themes) for presets and
authored palettes.

## The concepts

The concept pages each cover one idea in depth. Roughly in reading order:

- [State and messages](./concepts/state-and-messages) — the ownership rules:
  your app owns state, components read it and emit messages, `update` is the
  only writer.
- [Rendering and event routing](./concepts/rendering-and-events) — how a frame
  is declared, how the runtime remembers it, and how events find the right
  component.
- [Focus, hover, and identity](./concepts/focus-hover-identity) — how components
  get stable identities, how Tab traversal works, why focus lives in your state
  and hover lives in the runtime.
- [Layers and modals](./concepts/layers-and-modals) — dialogs, overlays, and
  paint ordering.
- [Themes](./concepts/themes) — built-in presets and authoring your own palette.
- [Host integration](./concepts/host-integration) — wiring the runtime into a
  native crossterm loop or a browser app with ratzilla.
- [Mouse Input](./concepts/mouse) and [Dragging](./concepts/dragging) — enabling
  mouse support, and how clicks, hover, and drags reach components.
- [Structuring a larger app](./concepts/composition) — splitting state,
  messages, and rendering per screen once one module is not enough.
- [Custom components](./concepts/custom-components) — writing your own
  components with the same powers as the built-ins.
- [Design decisions](./concepts/design-decisions) — why declaration mistakes
  panic, and other deliberate choices, for readers evaluating the library.

Component pages under [Components](./components/button) cover each built-in
component's features with live previews, and [Demos](./demos) lists every
runnable example in the repository — including two full applications.
