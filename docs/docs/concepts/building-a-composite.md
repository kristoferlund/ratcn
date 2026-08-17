---
description: "A composite component built end to end: a caller-supplied body slot, a measured child, one geometry function, focus-aware paint, and event handling that bubbles — every piece shown from a compiled, tested example."
---

# Building a composite

[Custom components](./custom-components) states the contract: a composite is an
ordinary `Component`, there is no composite trait, and what it needs is
somewhere to keep what its builders were handed and a way to keep answering
geometry questions once that is gone. This page is that contract as a working
component, taken apart piece by piece.

The component is `Fieldset` — a labeled group box with a body the caller fills,
one measured action beside the label, a collapse the app owns, and a disabled
state that dims the whole group and takes it out of interaction. It was chosen
because it needs all five things a composite generally needs, and nothing else:
a caller-supplied body, a measured child, retained layout facts, paint that
follows focus, and keys that bubble.

Tab through the groups below and press `Enter` on a switch; collapse a group
with `←` and reopen it with `→`; click a header. `Billing` starts dim and inert
on the free plan — nothing in it can be focused, hovered, or clicked, its own
action button included — until `Upgrade to Pro` turns it on; downgrade again to
watch the whole section drop back out.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 420px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p fieldset</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/fieldset-demo/index.html" title="ratcn fieldset demo"></iframe>
  </div>
</div>

Every snippet on this page is quoted out of
[`demos/fieldset`](https://github.com/kristoferlund/ratcn/tree/main/demos/fieldset)
at build time, so what you read is what compiles: `src/fieldset.rs` is the
component, `src/main.rs` is the app using it. The crate depends on `ratcn` the
way yours does — nothing in it reaches inside the library — which is the same
promise the `copy-fixture` crate makes for the built-ins. A composite of your
own can do everything `Dialog` does.

## What the caller writes

The whole component is configured by builders, and the two interesting ones hand
over a closure and a child:

<<< ../../../demos/fieldset/src/main.rs#caller{rust}

Read that as four kinds of argument, which is the taxonomy from
[What a component may hold](./custom-components#what-a-component-may-hold):
`"Notifications"` is a declaration prop, `collapsed` is a controlled binding,
`on_toggle` is event wiring, and `body` is a region of the screen the caller
owns. The fieldset knows nothing about email or push notifications, and cannot:
they exist only inside a closure it will hand an area to.

## The fields, and why they are split

<<< ../../../demos/fieldset/src/fieldset.rs#struct{rust}

Two pairs of fields carry what would naively be one thing each.
`body_height`/`body` and `action_size`/`action` are split because `declare`
*takes* the closures — they are `FnOnce`, they run once, and afterwards the
`Option` is empty — while the sizes are still needed a frame later, when a click
arrives and the box has to be re-derived. The rule from the contract, made
concrete: **keep the layout facts in fields that taking a closure does not
empty.**

`collapsed_now` and `paint_area` are the other half of the same idea. They are
declaration-derived caches, written while declaring and read afterwards, and
neither is a second copy of app state: one is a pinned reading of it, the other
is geometry.

One method is the hinge the whole pattern turns on — it names, in one place,
exactly which fields the geometry is allowed to depend on:

<<< ../../../demos/fieldset/src/fieldset.rs#mapping{rust}

No geometry reads `self.body` or `self.action`. `layout` below takes `Facts` and
nothing else — it is a free function, not a method — so a later edit cannot
quietly derive a rect from a closure that may already have been taken.

## One geometry function

Everything about where the fieldset's parts are lives in one place:

<<< ../../../demos/fieldset/src/fieldset.rs#facts{rust}

`layout` is called from four places — `declare` to place the body and the action,
`paint` to draw the border and label, `interaction_area` to report the box, and
`handle_event` to hit-test the header — and `height`, below, answers from the
same two helpers. None of them re-derives anything itself. That is the whole
discipline, and the reason for it is that hit-testing is where a disagreement
surfaces: a header the pointer misses by one row is a bug you find by clicking,
not by reading.

Two details in there are worth naming. The inset comes from a **plain** block —
`Block::bordered().padding(…).inner(area)` depends only on borders and padding,
so the styled block `paint` builds from the theme cannot move the layout out
from under the rects. And `header_height` takes the action's measured height
rather than assuming one row, which is what measuring the child is *for*.

## The body slot

<<< ../../../demos/fieldset/src/fieldset.rs#body{rust}

A region the caller fills is a closure, and it should be `FnOnce` so the caller
can move owned values into it — the demo moves two `bool`s in, and a real app
moves owned rows, strings, and handles. It is stored on the component until
`declare`, which is why it may only capture `'static` values.

`ctx.in_area(area, body)` is what runs it: the callback sees the strip as its
own `ctx.area()`, while the identity scope stays the fieldset's. So the body's
children are ordinary siblings of the action, in **one** id namespace — `"email"`
and `"push"` must not collide with `"mute"`, and the runtime panics if they do.
Nothing about a body slot is a scope; if a body needs its own Tab boundary, the
caller opens a `ctx.scope` inside it.

## The measured action

<<< ../../../demos/fieldset/src/fieldset.rs#action{rust}

`measure()` is called **here, at push time**, not in `declare`. By the time
`declare` runs, the component has been moved into the closure and is out of
reach, and the header cannot be laid out without its width. So the size is taken
while the component is still in hand and kept beside the closure.

What gets boxed is the *declaration*, not the child: `Box<dyn Component>` does
not itself implement `Component`, so the id and the component are captured
inside a closure that calls `ctx.component`. Everything that implements
[`MeasuredComponent`](https://docs.rs/ratcn/latest/ratcn/runtime/trait.MeasuredComponent.html)
can go in the slot; anything else has no size to place, and belongs in the body
where the caller does the placing.

## Declaring

<<< ../../../demos/fieldset/src/fieldset.rs#impl-declare{rust}

`prepare` runs first, before the runtime reads `scope_options`,
`is_focusable`, or `interaction_area`, and that is the only place the collapsed
binding can be pinned: `interaction_area(&self, area)` is handed no state, and
the box it reports must be the box this frame will paint. Not every answer needs
the pin — `Component::is_focusable(&self, state)` takes state, so a leaf whose
focusability follows a `disabled` prop just reads it — but `scope_options` and
`interaction_area` do not, and whatever they depend on has to be settled here.

`declare` paints nothing. It records `paint_area`, computes the layout once,
declares the action, then the body — declaration order is Tab order, and here it
is also reading order — and each closure is `take`n so it can never run twice.
A collapsed fieldset simply does not declare its body: there is no hiding
mechanism to maintain, because a declaration that does not happen leaves nothing
behind.

The `disabled` wash is the interesting part, because it has three plausible
spellings and only one of them is right here.

Paint is a queue, filled while declaring and replayed in the order it was
filled. **`Component::paint` is queued where the declaration opens** — before
everything the composite declares inside itself. That is exactly what a
background and a border want, and exactly wrong for a wash: the body's
components are queued after it and would paint straight over it.

**`ctx.paint` is queued where it is reached.** Called at the end of `declare`,
after the action and the body have been declared, it lands after *their* paint in
the same queue and on the same layer, and its `PaintCtx` still reports this
declaration's area and interaction flags. That is what the fieldset uses, and it
is the answer whenever the decoration has to cover only what the composite
itself declared.

**`ctx.defer_paint` is the last resort.** It defers until the current layer has
finished declaring, so it outranks even *later siblings* — a drag ghost floating
over cards declared after the one being dragged, as the kanban demo does. The
price is real: deferred paint has no identity, no geometry, and no hit target,
and paint deferred from the base declaration flushes after every layer has
composited, so a wash registered there would draw over a modal opened on top of
it. Use it to outrank siblings; use `ctx.paint` to outrank your own children.

## Painting

<<< ../../../demos/fieldset/src/fieldset.rs#impl-paint{rust}

`paint` runs after the whole tree is declared and focus has resolved, which is
why the four interaction flags live on `PaintCtx` and not on `DeclareCtx`. The
fieldset uses three of them, and the distinctions matter:

- `contains_focus` for the border, because a group with a focused control inside
  it is the thing the user is working in — `focused` alone would leave the
  border dull while the user typed in the body.
- `focused` for the marker, which is the fieldset's own affordance: it lights up
  when the toggle key would reach *here*.
- `hovered`, not `contains_hover`, for the pointer, because the pointer resting
  on the action button is not the pointer resting on the header. The header is
  what a click would toggle, so the header is what the marker reports on.

This is also the answer to a question worth asking once:
`DeclareCtx::pointer_within()` reads hover while declaring, so why not style
from it? Because it answers with the *last resolved* hover — the previous frame's,
against the previous surface — while the paint flags are this frame's. Where the
two disagree, structure lags by one frame. Appearance is never a good enough
reason to accept that; keep `pointer_within` for the rare case where hover
changes what is *declared*, as `Tooltip` uses it to decide whether its bubble
exists at all.

## Being interacted with

<<< ../../../demos/fieldset/src/fieldset.rs#impl-interaction{rust}

**`scope_options`.** Wiring `on_toggle` makes the fieldset itself focusable,
which sounds like it would steal Tab stops from the body and does not: focus
prefers a focusable descendant and falls back to the scope only when there is
none. A collapsed group with no action has nothing inside it, so without this it
would be a keyboard dead end — reachable by mouse, impossible to reopen with
`→`. Tab wrapping is left at its default, because a fieldset is a group on a
page, not a trap like a dialog.

**`interaction_area`.** The box is usually shorter than the allocation, and rows
the fieldset never painted must not take clicks — that is the ordinary case.
`disabled` is the sharp one: returning an empty rect keeps the identity and the
paint but removes the fieldset *and everything declared inside it* from focus
traversal, hit-testing, pointer capture, and event routing. That is how one
builder call makes a whole section inert without the caller disabling each
control in it, and it is why the demo's plan button lives outside the group it
enables.

**`handle_event`.** Two things are happening here, and they read the same value
at two different ages on purpose:

- The collapsed flag is read from *current* state, because two keys can reach
  the retained instance before the next frame is drawn and the second has to
  compose on what the first asked for. Copied in at declaration time, the second
  `Enter` would re-send the first one's request and the group would look stuck.
- The geometry is derived from the *retained* facts and the retained allocation,
  because the click has to be tested against the box that was on screen when the
  user aimed at it.

Everything the fieldset has no answer for returns `Ignored`, and that is not
politeness — it is what makes the accordion idiom work at all. A `←` pressed
while a switch inside the body has focus is declined by the switch, bubbles to
the fieldset, and collapses the group. Consume more than you handle and you
break the parent; consume less and keys land twice.

## Sizing from outside

<<< ../../../demos/fieldset/src/fieldset.rs#height{rust}

The caller stacks the groups by asking each one how tall it wants to be, so
collapsing the first one reflows the second:

<<< ../../../demos/fieldset/src/main.rs#stacking{rust}

`MeasuredComponent` cannot answer that question here, and the reason is
instructive. Its `measure(&self)` takes no state, because a container calls it
while *pushing* the child — before `prepare`, before any state is in hand. That
fits a `Button`, whose size is a property of its label. It does not fit a
fieldset, whose height depends on a flag that lives in the app. So the question
becomes a method that takes what it depends on, answered by the same arithmetic
`layout` uses rather than a second copy of it.

## What is deliberately not here

**Transients.** `ctx.transient` exists for gesture mechanics that must outlive
the component instance — a drag anchor, a wheel-parked offset. The fieldset has
none: a collapse is semantic state the app must persist, so it belongs in app
state and travels by message. Reaching for a transient to hold it would put UI
state somewhere nothing warns you about when its path stops being declared. See
[Dragging](./dragging) for the case that genuinely needs one.

**A paint widget.** Reusable components are usually worth splitting into a
stateless widget that draws and a component that behaves. `Fieldset` skips the
split for the same reason `Dialog` does: its frame is pure geometry, and
`handle_event` needs to re-derive that geometry between frames without a
`Frame` to draw into.

## The tests, and what they pin

`cargo test -p fieldset` runs ten tests, and every one of them exists because
some part of the pattern above could plausibly be written the other way:

| Test | The mistake it catches |
|---|---|
| `two_toggles_without_a_frame_between_them_expand_and_then_collapse` | reading the binding at declaration time instead of event time |
| `a_group_with_nothing_focusable_inside_is_the_focus_target_itself` | leaving `scope_options().focusable()` off, making a collapsed group a dead end — and returning anything but `Ignored` for a key the group has no answer for |
| `left_collapses_the_group_from_a_control_inside_its_body` | handling the toggle only while the group itself holds focus, so the accordion cannot be driven from a control inside it |
| `the_header_toggles_on_click_and_the_rows_below_a_collapsed_box_do_not` | hit-testing against something other than the geometry that was painted |
| `a_disabled_fieldset_ignores_events_meant_for_its_own_children` | dimming a section without making it inert |
| `the_disabled_wash_covers_what_the_body_declared` | drawing over-the-descendants decoration from `paint`, where the descendants would cover it |
| `the_height_asked_for_is_the_height_the_box_lays_itself_out_to` | a second copy of the layout arithmetic |
| `collapsing_a_group_reflows_the_one_below_it` | the caller duplicating the component's sizing |
| `tab_skips_a_disabled_group_entirely` | a dimmed section that still collects Tab stops |
| `the_plan_button_enables_the_billing_group_and_its_action` | a composite that remembers state it should be re-reading |

The component ones drive a real `Ratcn` surface — declare a frame, then hand
events to the retained instance — which is the only way to test what a composite
does *with* the runtime rather than in isolation. The app ones drive the demo's
own `draw` and `handle_event`.

## Checklist

- Layout facts get fields of their own; the closures they describe are `Option`s
  that `declare` takes.
- Every rect comes from one function, called from `declare`, `paint`,
  `interaction_area`, and `handle_event`.
- A caller's body is `FnOnce`, stored, `take`n in `declare`, and run with
  `ctx.in_area`.
- A placed child is measured in the builder; the *declaration* is what gets
  boxed, and declaration order is Tab order.
- Ids stay unique across every slot: a body opens no scope of its own.
- Anything `interaction_area` or `scope_options` depends on is pinned in
  `prepare` — they are asked without state.
- Controlled bindings are read at event time; geometry is hit-tested against
  what was painted.
- Paint styles from `PaintCtx`'s flags; decoration that must cover the
  composite's own descendants is a late `ctx.paint`, and `ctx.defer_paint` only
  when it must cover later siblings too.
- Everything the composite does not handle returns `Ignored`.

## Copying this

`demos/fieldset/src/fieldset.rs` is written to be lifted. It has no dependency
on the demo around it: it is generic over `S` and `M`, it imports only from
`ratcn` and `ratatui`, and it holds no state of its own. Copy it, rename it, and
start deleting the parts you do not need — the collapse, the action slot, and
the disabled state are independent of each other.

For a second reading of the same pattern under more pressure, read
[`components/dialog.rs`](https://github.com/kristoferlund/ratcn/blob/main/crates/ratcn/src/components/dialog.rs):
two body slots instead of one, a row of measured actions instead of a single
one, and a box that moves under a drag, so its retained allocation is genuinely
load-bearing rather than merely correct.
