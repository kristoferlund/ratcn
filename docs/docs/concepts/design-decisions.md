---
description: "Why ratcn panics on declaration mistakes and declares every frame twice: what each decision buys, what it costs, and which alternatives were rejected."
---

# Design decisions

Two things about ratcn surprise people the first time they read the code:
mistakes while declaring the UI panic rather than return an error, and every
frame is declared twice. Both are deliberate. This page explains why, and what
each one costs.

## Why mistakes panic

Declaration mistakes panic. Duplicate sibling ids, a declaration closure that
declares different trees on its two runs, modal root ids that differ from the
bound `ModalState` — all of these unwind the declaration pass instead of returning
an error value.

Each of these is a bug in your code, not something an app could sensibly
recover from at runtime. There is no useful branch to write for "two children
share an id" — the fix is editing the declaration. Rust treats indexing past the
end of a slice the same way, and for the same reason.

Returning a `Result` instead was considered and rejected, for two reasons:

- Components paint as they are declared. By the time a duplicate id can be
  detected, earlier pixels are already in the frame, and returning an error
  could not undo them. It would report a broken frame while looking
  recoverable.
- Anything that returns a `Result` invites `?` and `let _ =`. A bug that can be
  passed along quietly will be, and it would resurface later as misrouted
  events, far from where it started. A panic points at the exact call, the
  first time the mistake is reached.

### What a panic guarantees

Replacing the retained surface is the very last step of a successful render, and
the whole pass runs under unwind protection. So a panic anywhere along the way
leaves the last good surface untouched: events keep routing through it, and the
next successful render replaces it as usual.

Pixels already written to the Ratatui frame are not rolled back, so a failed
frame can look half-drawn. What cannot happen is a half-routable surface — there
is no state in which part of a declaration receives events.

Hosts that want to keep running through a declaration bug can catch the unwind
around the draw call and keep dispatching events; the retained surface makes
that safe. See
[Rendering and event routing](./rendering-and-events#what-an-event-sees)
for the full timing contract.

### The same stance, without panics

The same refusal to guess shows up outside validation. If a focused component
becomes unfocusable, focus stays where it is — *parked* — rather than jumping to
a neighbour. Disabled controls simply ignore input, and Tab moves past them. A
focus path naming nothing at all stays parked too.

Quietly repairing focus was rejected because a fallback applied while painting
but not while routing, or the reverse, produces the worst failure this design
can have: pixels that disagree with where events go. Parked focus is visible and
easy to fix; a guess is neither.

An open modal is an explicit focus boundary, not a silent repair — and it
follows the same parking rule. A stored path the surface *did* declare outside
the top modal resolves into the modal (the modal owns input); a path the
surface never declared stays parked even then, with keys nothing owns falling
back to the modal root. Parked paths inside the modal remain exact. None of
this depends on the `Ratcn::modals` binding, whose jobs are stack validation
and covering the open/close gap.

## Why every frame declares twice

Every frame, `Ratcn::render` runs the app's declaration closure twice. The
first run is the *structure pass*: the same declarations with every paint call
suppressed, so the runtime learns the full tree — identity, geometry, which
declarations can take focus — before anything is drawn. Focus then resolves
once, against that complete tree, and the second run — the *paint pass* —
paints with the resolved focus feeding every `ctx.focused` flag. The paint
pass is validated declaration-by-declaration against the structure pass, so a
closure that declares different trees on its two runs panics naming the first
divergent path.

The reason is that getting focus right on the very first frame needs the whole
tree. Startup focus, highlighting a pane that contains focus, and a newly opened
modal claiming focus all depend on one question: *does this subtree contain
anything focusable?* A parent has to paint before its children have declared, so
in a single pass that question has no answer yet.

Earlier versions had components promise the answer up front, through a
`focusable_descendants()` method. It worked, but every composite had to carry
it, and the promise repeated something the children already said for
themselves. Declaring twice removes the question entirely — the structure pass
just looks.

The same mechanism is what makes painting and routing agree. Focus is resolved
by one function, over one tree, and it is the same function event routing uses
later. There is no second answer to drift from.

The obvious alternative — reusing last frame's answer — is accurate except
exactly when it matters: the first frame, and any frame where focusability
changed. Both leave a window where focus paints in one place and events go to
another.

### The contract, and its cost

The closure has to declare the same tree both times. It may branch on anything
in app state, focus included — what it must not branch on is the `ctx.focused`
and `ctx.contains_focus` flags, which are only provisional during the structure
pass. Styling and painting may use those flags freely; that is what they are
for.

The runtime checks this every frame, so a closure that is not repeatable fails
the first time it runs rather than going quietly out of step.

The cost is that declaring happens twice per frame, though painting still
happens once. At terminal scale that is cheap: building a frame's components is
arithmetic and a few small allocations, and the structure pass writes no cells
at all. Because components are built twice, anything moved into one has to be
constructible twice — the compiler enforces that for you, since the closure is
`FnMut`.
