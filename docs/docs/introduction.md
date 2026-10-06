---
description: "ratcn is an opinionated library of beautifully designed terminal UI components for Ratatui that you copy into your app and own, plus an engine for focus, mouse, hover, and layers."
---

# Introduction

ratcn is an opinionated library of beautifully designed terminal UI components
for [Ratatui](https://ratatui.rs). You copy them into your application code,
theme them, and make them your own.

Every component paints through a plain Ratatui widget, so it fits into the app
you already have. Add the ratcn engine and the components come alive: it handles
focus, keyboard and mouse input, hover, and layers, and tells your app what the
user did.

<figure class="ratcn-diagram">
<svg viewBox="0 0 640 230" role="img" aria-labelledby="ratcn-uifn-title" xmlns="http://www.w3.org/2000/svg">
<title id="ratcn-uifn-title">ui = f(state): your app state is rendered into the UI, user input becomes a message, and update applies the message to the state.</title>
<defs><marker id="ratcn-uifn-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" orient="auto-start-reverse"><path class="d-head" d="M0 0 L10 5 L0 10 z"/></marker></defs>
<text class="d-formula" x="320" y="30" text-anchor="middle">ui = f(state)</text>
<rect class="d-key" x="30" y="62" width="150" height="64" rx="10"/>
<text class="d-name" x="105" y="91" text-anchor="middle">State</text>
<text class="d-sub" x="105" y="111" text-anchor="middle">owned by your app</text>
<rect class="d-box" x="245" y="62" width="150" height="64" rx="10"/>
<text class="d-name" x="320" y="91" text-anchor="middle">UI</text>
<text class="d-sub" x="320" y="111" text-anchor="middle">what is on screen</text>
<rect class="d-box" x="460" y="62" width="150" height="64" rx="10"/>
<text class="d-name" x="535" y="91" text-anchor="middle">Message</text>
<text class="d-sub" x="535" y="111" text-anchor="middle">what the user did</text>
<path class="d-line" d="M182 94 H241" marker-end="url(#ratcn-uifn-arrow)"/>
<text class="d-verb" x="212" y="84" text-anchor="middle">render</text>
<path class="d-line" d="M397 94 H456" marker-end="url(#ratcn-uifn-arrow)"/>
<text class="d-verb" x="427" y="84" text-anchor="middle">input</text>
<path class="d-line" d="M535 128 V176 Q535 186 525 186 H115 Q105 186 105 176 V132" marker-end="url(#ratcn-uifn-arrow)"/>
<text class="d-verb" x="320" y="208" text-anchor="middle">update</text>
</svg>
</figure>

Your app stays in charge. The state lives in your app, and the screen is a
function of it: each frame describes the UI from that state, and components
report what happened as messages for your app to apply. The components are yours
as well. When one doesn't do what you need, copy its source into your project
and change it.

Terminal apps can be as pleasant to use as anything on the web. Buttons light up
under the pointer, fields take clicks and selections, and the patterns you know
from the web are here too: toasts, tooltips, dropdowns, and modal dialogs that
float above the rest of your app.

## See it live

The components run in your terminal right now, with nothing to install:

```sh
ssh ratcn.com
```

The showcase lets you browse every component, switch themes, drag cards across a
Kanban board, and try full demo apps with your keyboard and mouse. The previews
on the component pages run live in your browser as well.

## Widgets, the engine, or both

Each component comes in up to two parts, and each part works on its own:

- **Paint-only widgets**, such as `ButtonWidget` and `BarChartWidget`, are
  ordinary Ratatui widgets. They drop into any Ratatui app with
  `frame.render_widget(...)`: no engine, no message type, no change to how your
  app already works.
- **Interactive components**, such as `Button`, `List`, `Tabs`, and `Dialog`,
  add focus, keyboard and mouse handling, and messages on top. You declare them
  through the engine, the `Ratcn` runtime.

If you already have focus and event handling you like, use the widgets alone and
keep it. Nothing built that way is second-class: most interactive components
paint through these same widgets, and you can adopt the engine later, one
component at a time.

## Your app stays in charge

The engine enters your app at two calls, and leaves the rest of your loop alone:

- `Ratcn::render(frame, area, state, theme, declare)` declares which components
  are on screen this frame, and paints them.
- `Ratcn::handle_event(event, state)` routes one input event, and may hand you a
  message back.

Components never write your state. A `Button` emits a message when pressed; a
`List` reads its selection from your state and emits the item the user chose.
Your `update` function applies the message, and it is the only place state
changes. [State and messages](./concepts/state-and-messages) covers the rules
everything else builds on.

## Layers

Some things belong above the rest of the screen. The engine paints them on
layers, each with its own rules:

- **Hints** explain, like a tooltip. They paint on top and never get in the way
  of the pointer.
- **Popups** offer a choice, like a dropdown or a menu. They take the clicks that
  land on them, without dimming anything or stealing focus.
- **Modals** take over, like a dialog. They dim what is beneath, hold focus, and
  keep keys and clicks to themselves until they close.

A layer can be declared from anywhere in your UI. See
[Layers and modals](./concepts/layers-and-modals).

## Components

[Button](./components/button), [Input](./components/input),
[TextArea](./components/textarea), [List](./components/list),
[ScrollArea](./components/scroll-area), [Select](./components/select),
[Tabs](./components/tabs), [Dialog](./components/dialog),
[Toast](./components/toast), [BarChart](./components/barchart),
[Tooltip](./components/tooltip), [Checkbox](./components/checkbox),
[Cycle](./components/cycle), and [Progress](./components/progress). Each page
has a live preview. If there is a component or pattern you would like to see,
please [open an issue](https://github.com/kristoferlund/ratcn/issues).

## The concepts

Each concept page covers one idea in depth. Roughly in reading order:

- [State and messages](./concepts/state-and-messages): the ownership rules.
  Your app owns state, components read it and emit messages, and `update` is
  the only writer.
- [Rendering and event routing](./concepts/rendering-and-events): how a frame
  is declared, how the engine remembers it, and how events find the right
  component.
- [Focus, hover, and identity](./concepts/focus-hover-identity): how components
  get stable identities, how Tab traversal works, and why focus lives in your
  state while hover lives in the engine.
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

[Getting started](./getting-started) sets up a project and puts a first app on
screen, and [Demos](./demos) lists every runnable example, including three full
applications.
