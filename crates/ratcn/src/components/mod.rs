// The component library. Each component lives in its own module and is
// self-contained — paint and, where interactive, behavior together — so it can
// be copied into someone else's project as one file.
//
// A component module builds on the crate's core (runtime, theme, color, toast)
// and on the copy-support modules listed at the crate root. It never builds on a
// sibling component: a component that needs a border constructs a ratatui
// `Block` itself. The crate root re-exports each component's types straight
// from its module, so every public name is written down once, and nothing else
// in the crate reaches in here.
//
// CONTRIBUTING.md holds the section order these modules are written to and the
// shape their `handle_event` implementations share.
pub(crate) mod barchart;
pub(crate) mod button;
pub(crate) mod checkbox;
pub(crate) mod cycle;
pub(crate) mod dialog;
pub(crate) mod list;
pub(crate) mod progress;
pub(crate) mod scroll_area;
pub(crate) mod select;
pub(crate) mod tabs;
pub(crate) mod toast;
pub(crate) mod tooltip;
