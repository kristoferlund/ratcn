---
description: "Why ratcn panics on declaration mistakes and defers every paint to a replay after declaring: what each decision buys, what it costs, and which alternatives were rejected."
---

# Design decisions

Two things about ratcn surprise people the first time they read the code:
mistakes while declaring the UI panic rather than return an error, and
declaring a frame draws none of it. Both are deliberate. This page explains
why, and what each one costs.

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

- Declaring is where these mistakes are found, and declaring does not paint:
  every mistake is detected while the frame is still only a description of
  itself. An error value would therefore have to be threaded back out through
  every nested declaration to reach a caller that could act on it, for a
  condition the caller cannot act on. A panic reaches the same place in one
  step.
- Anything that returns a `Result` invites `?` and `let _ =`. A bug that can be
  passed along quietly will be, and it would resurface later as misrouted
  events, far from where it started. A panic points at the exact call, the
  first time the mistake is reached.

### What a panic guarantees

Replacing the retained surface is the very last step of a successful render, and
the whole pass runs under unwind protection. So a panic anywhere along the way
leaves the last good surface untouched: events keep routing through it, and the
next successful render replaces it as usual.

A rejected pass paints nothing at all: declaration, validation, and the modal
check all complete before the first cell is written, so the previous frame
stays on screen intact. What also cannot happen is a half-routable surface —
there is no state in which part of a declaration receives events.

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

## Declare once, then draw

Every frame, `Ratcn::render` runs the app's declaration closure exactly once.
That run draws nothing: it builds the tree — identity, geometry, which
declarations can take focus — and queues the paint each declaration owes, in
the order it reached them. Focus then resolves once, against that complete
tree. Only then does the queue run, and every interaction flag a paint reads
is derived at that moment, from the node it belongs to against the focus that
just resolved.

The reason for the split is that getting focus right on the very first frame
needs the whole tree. Startup focus, highlighting a pane that contains focus,
and a newly opened modal claiming focus all depend on one question: *does this
subtree contain anything focusable?* A parent is reached before its children
have declared, so at the moment a container is declared that question has no
answer yet.

Earlier versions had components promise the answer up front, through a
`focusable_descendants()` method. It worked, but every composite had to carry
it, and the promise repeated something the children already said for
themselves. Deferring paint removes the question entirely — by the time
anything needs a flag, the tree is complete and the runtime just looks.

Deferring is also what removed the second declaration. An earlier design ran
the closure twice, once to learn the tree and once to paint it with the
resolved focus, and paid for it with an idempotency contract: the two runs had
to declare the same tree, checked declaration-by-declaration, and any impurity
in the closure was a panic. Separating *when a paint is decided* from *when it
happens* buys the same ordering with one run and no contract.

The same mechanism is what makes painting and routing agree. Focus is resolved
by one function, over one tree, and it is the same function event routing uses
later. There is no second answer to drift from.

The obvious alternative — reusing last frame's answer — is accurate except
exactly when it matters: the first frame, and any frame where focusability
changed. Both leave a window where focus paints in one place and events go to
another.

### What this costs the closure

Almost nothing. The closure runs once, so it may have side effects, consume
what it captures, and move owned values into the components it declares — it
is `FnOnce`. What it cannot do is read an interaction flag while declaring,
because none exists yet: the flags are only offered to `PaintCtx`, where
styling uses them freely.

The one thing paint owes in return is that it cannot decide *structure*. A
paint closure draws; it does not declare. That is what keeps "the tree is
complete before any flag is read" true rather than merely usual.
