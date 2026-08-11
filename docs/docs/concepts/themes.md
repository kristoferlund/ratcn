---
description: "A ratcn Theme is a palette of semantic color roles such as background, primary, destructive, border, and ring, with presets and support for authoring your own."
---

# Themes

A `Theme` is a palette of colors named for what they are *for* rather than what
they look like, and it is the only styling most apps ever touch. Every component
picks its default look from these roles, so switching the theme restyles the
whole app.

| Role | Used for |
|---|---|
| `background`, `foreground` | The app canvas |
| `surface` | Raised containers, such as a dialog |
| `field` | Controls that hold a value, such as a list or a select panel |
| `primary`, `secondary`, `accent` | Emphasis, in decreasing weight |
| `destructive`, `warning` | Actions and messages that need caution |
| `border` | Ordinary container edges |
| `ring` | The focus accent — a focused pane's border, the dialog frame |
| `cursor` | A text caret |

Most roles have a foreground companion chosen to contrast with the fill, so text
stays readable whichever role paints behind it.

## Using a preset

Choose one built-in preset directly:

```rust
use ratcn::Theme;

let theme = Theme::catppuccin();
ratcn.render(frame, &state, &theme, |ctx| {
    // Declare components.
});
```

The presets are `default_dark`, `terminal`, `catppuccin`, `gruvbox`, `nord`,
`tokyo_night`, and `solarized`. `Theme::presets()` returns them as a
`&'static [Theme]` in stable picker order — iterate or index it, but do not
depend on the count, which grows as presets are added.

## Writing your own

To author a palette, start from a preset and assign the roles you care about.
`Theme` is `#[non_exhaustive]`, so a struct literal is not available; the fields
are public and the presets are `const`, so a `const fn` gives the same result.
(The display name is a `&'static str`, so authored names must be static in the
current API.)

```rust
use ratatui::style::Color;
use ratcn::Theme;

const fn operations() -> Theme {
    let mut theme = Theme::default_dark();
    theme.name = "Operations";
    theme.accent = Color::LightCyan;
    theme.warning = Color::LightYellow;
    theme
}

const THEME: Theme = operations();
```

Pass the same theme to runtime rendering and paint-only widgets so both halves
of the library agree. To recolor a single component, use that component's
`.style(|theme| ...)` override — see each component page's Styling section —
rather than adding a palette role.
