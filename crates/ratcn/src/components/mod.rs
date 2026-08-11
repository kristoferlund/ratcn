//! The component library: the user-facing pieces you compose an app from.
//!
//! Each component lives in its own module and is self-contained — paint and
//! (where interactive) behavior together — so it reads, and eventually
//! installs, as one unit. Components build on the [`runtime`](crate::runtime) engine
//! (focus, events, rendering); the engine never depends on them.
//!
//! Interactive components (focusable, event-handling): [`Button`], [`List`],
//! [`Select`], [`Tabs`], [`Dialog`]. Event-handling but never focusable:
//! [`Tooltip`], which explains the trigger it wraps rather than acting itself.
//! Paint-only, with no interactive half:
//! [`ToasterWidget`], and [`BarChartWidget`] — which is a themed adapter over
//! ratatui's own `BarChart` rather than a component in the sense used here,
//! adding theme colors, grouping, and a value-display switch to a chart ratatui
//! draws.
//!
//! # What a component depends on
//!
//! A component module builds on the crate's core — [`runtime`](crate::runtime),
//! [`theme`](crate::theme), [`color`](crate::color) — and on the shared helper
//! modules, which are the pieces more than one component would otherwise
//! duplicate:
//!
//! - [`button_shape`](crate::button_shape) — the cap and fill rows of the
//!   button idiom, which `Tabs` paints a large tab with too
//! - [`linear_nav`](crate::linear_nav) — index arithmetic for moving through an
//!   ordered set of items
//! - [`list_core`](crate::list_core) — value-keyed items, the uniform-row
//!   viewport, and the wheel park
//! - [`selection_indicator`](crate::selection_indicator) — the radio and
//!   checkbox markers
//! - [`text_width`](crate::text_width) — display-width measurement and wrapping
//! - [`toast`](crate::toast) — the app-owned toast queue [`ToasterWidget`]
//!   paints
//!
//! What a component module never builds on is a *sibling component*: a
//! component that draws a border constructs a plain ratatui `Block` itself. So
//! replacing or removing one component never breaks another, and a copied
//! module carries only the core and helper modules with it.
//!
//! # Module layout
//!
//! Component modules are written to one section order, so a reader — or a
//! developer who has copied a module into their project — knows where to look:
//!
//! 1. imports
//! 2. constants
//! 3. variant enums
//! 4. the style struct — `fallback()`, `from_theme()`, and its `resolve_*`
//!    methods: where a *component's own* interaction state (focused, hovered,
//!    selected, disabled) picks colors. A component whose surfaces vary
//!    independently has one resolver per surface, named for it: `ListStyle` and
//!    `SelectStyle` each have `resolve_surface` (the backdrop behind the rows)
//!    and `resolve_row` (one row). `ButtonStyle` and `TabsStyle` have a single
//!    `resolve`. `ToasterStyle::resolve` is not an interaction resolver: toasts
//!    take no focus, and it maps a `ToastKind` to that kind's accent color and
//!    title style. `DialogStyle` and `BarChartStyle` have no resolver at all:
//!    their colors do not vary. Visual rules two components share sit in a
//!    helper module rather than in either style struct —
//!    [`selection_indicator::color`](crate::selection_indicator::color) picks a
//!    marker color from disabled and selected for `List` and `Select` alike.
//! 5. the paint widget (`XWidget`) — builders, `Widget` impl, private paint
//!    helpers
//! 6. closure type aliases
//! 7. the interactive component (`X<S, M>`) — builders and wiring
//! 8. its `Component` impl
//! 9. private helpers and free functions
//! 10. tests
//!
//! Modules vary within that order where their own reading order won, so expect
//! the shape rather than the exact sequence: `list.rs` and `toast.rs` follow it
//! as written, while `dialog.rs` declares its closure aliases before its
//! constants and keeps the private geometry functions next to the style struct
//! they serve, `tabs.rs` places its constants after the variant enums,
//! `button.rs` has no constants and places `ButtonRenderMode` after
//! `ButtonStyle`, `select.rs` puts a few private free functions ahead of the
//! component, and `barchart.rs` declares its private data enum after the paint
//! widget.
//!
//! The `handle_event` implementations of the components listed above share one
//! silhouette. `Button`, `List`, `Select`, and `Tabs` open with an early guard
//! returning `Ignored` when the component cannot act at all — `disabled`, no
//! items, or (for `Button`) no `on_press` wired. `Dialog` has no early guard: it
//! has no disabled state and is always able to act, so it opens directly on the
//! dismiss-key check. Only `Event::Mouse` and `Event::Key` are handled anywhere;
//! no component reads `Event::Paste`.
//!
//! After the guard, `List`, `Select`, and `Tabs` `match` on the event kind,
//! mouse arm first and key arm second, falling through to `Ignored` for kinds
//! they do not handle. The other two differ in shape. `Button` matches the event
//! kind but the match yields a `bool`, because one `match` resolves whether
//! either kind was a press. `Dialog` checks the key first — the dismiss key must
//! answer before the border drag hit-test — in an `if let Event::Key(..)` chain,
//! then narrows with a `let Event::Mouse(mouse) = event else { return Ignored }`
//! and dispatches on the drag phase rather than on the event kind. The private
//! option-list component behind `Select`'s popup uses that same let-else, after
//! a guard on whether the popup is open, and then matches `mouse.kind`; it is
//! not public, so it is the one `Component` impl outside the five named above.
//!
//! `Dialog` deviates from the widget/component split: it has no paint widget —
//! its visual frame is pure geometry functions so event handling can re-derive the
//! box for hit-testing (see the module).

mod barchart;
mod button;
mod dialog;
mod list;
mod select;
mod tabs;
mod toast;
mod tooltip;

pub use barchart::{BarChartGroup, BarChartStyle, BarChartWidget};
pub use button::{Button, ButtonRenderMode, ButtonSize, ButtonStyle, ButtonVariant, ButtonWidget};
pub use dialog::{Dialog, DialogStyle};
pub use list::{List, ListStyle, ListWidget};
pub use select::{Select, SelectStyle, SelectWidget};
pub use tabs::{Tab, Tabs, TabsActivation, TabsSize, TabsStyle, TabsWidget};
pub use toast::{ToastPosition, ToasterStyle, ToasterWidget};
pub use tooltip::{Tooltip, TooltipSide, TooltipStyle, TooltipWidget};
