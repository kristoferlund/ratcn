//! Composition demo: two focusable panels, each grouping its own children.
//!
//! Each panel is a `ctx.scope(..)` scope — a named grouping with its own Tab
//! boundary and focus hotkey. No container component is involved.
//!
//! Panel A holds two buttons; Panel B holds three.
//!
//! - `a` / `b`: jump focus between the panels (app-level panel-switch hotkeys)
//! - `Tab` / `Shift+Tab`: move between the focused panel's buttons
//! - `Enter` / `Space`: press the focused button

use std::io;

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Margin},
    style::{Color, Style},
    widgets::Block,
};
use ratcn::{
    Button, Theme,
    runtime::{DeclareCtx, Event, EventResult, FocusState, Ratcn, ScopeOptions, TabWrap},
};

const THEME: Theme = Theme::default_dark();
const PADDING_X: u16 = 2;
const PADDING_Y: u16 = 2;

/// Child ids, named once so declarations and focus paths can't drift apart on a
/// typo.
mod ids {
    pub const PANEL_A: &str = "panel_a";
    pub const PANEL_B: &str = "panel_b";
    pub const A1: &str = "a1";
    pub const A2: &str = "a2";
    pub const B1: &str = "b1";
    pub const B2: &str = "b2";
    pub const B3: &str = "b3";
}

/// The two panels, named so a message cannot reference a panel that does not
/// exist.
#[derive(Clone, Copy)]
enum PanelId {
    A,
    B,
}

impl PanelId {
    const fn letter(self) -> char {
        match self {
            Self::A => 'A',
            Self::B => 'B',
        }
    }
}

/// All mutable UI state lives here: button results and focus.
#[derive(Default)]
struct AppState {
    focus: FocusState,
    pressed_a: Option<u8>,
    pressed_b: Option<u8>,
}

impl AppState {
    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::FocusChanged(focus) => self.focus = focus,
            Msg::ButtonPressed {
                panel: PanelId::A,
                button,
            } => self.pressed_a = Some(button),
            Msg::ButtonPressed {
                panel: PanelId::B,
                button,
            } => self.pressed_b = Some(button),
        }
    }

    fn pressed(&self, panel: PanelId) -> Option<u8> {
        match panel {
            PanelId::A => self.pressed_a,
            PanelId::B => self.pressed_b,
        }
    }
}

#[derive(Clone)]
enum Msg {
    ButtonPressed { panel: PanelId, button: u8 },
    FocusChanged(FocusState),
}

struct App {
    state: AppState,
    ratcn: Ratcn<AppState, Msg>,
}

impl App {
    fn new() -> Self {
        Self {
            state: AppState::default(),
            ratcn: Ratcn::new()
                .focus(|s: &AppState| &s.focus, Msg::FocusChanged)
                .focus_key('a', [ids::PANEL_A])
                .focus_key('b', [ids::PANEL_B]),
        }
    }
}

impl demo_shared::Demo for App {
    fn background(&self) -> Color {
        THEME.background
    }

    fn handle_event(&mut self, event: Event) -> bool {
        match self.ratcn.handle_event(event, &self.state) {
            EventResult::Emit(msg) => {
                self.state.update(msg);
                true
            }
            EventResult::Consumed => true,
            EventResult::Ignored => false,
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        frame
            .buffer_mut()
            .set_style(area, Style::default().bg(THEME.background));
        let area = area.inner(Margin::new(PADDING_X, PADDING_Y));
        let state = &self.state;
        let panels_layout = Layout::vertical([Constraint::Fill(1); 2]).spacing(1);
        self.ratcn.render(frame, state, &THEME, |ctx| {
            let [panel_a_area, panel_b_area] = area.layout(&panels_layout);
            ctx.scope(ids::PANEL_A, panel_a_area, Self::panel_options(), |ctx| {
                Self::panel(ctx, PanelId::A, &[ids::A1, ids::A2]);
            });
            ctx.scope(ids::PANEL_B, panel_b_area, Self::panel_options(), |ctx| {
                Self::panel(ctx, PanelId::B, &[ids::B1, ids::B2, ids::B3]);
            });
        });
    }
}

impl App {
    /// Both panel scopes get the same options, so they cannot disagree about
    /// Tab behavior.
    fn panel_options() -> ScopeOptions {
        ScopeOptions::default().tab_wrap(TabWrap::Wrap)
    }

    /// One panel: a border around a centered row of buttons, the first primary
    /// and the rest secondary. Both panels render through here, so they can
    /// only differ by the data passed in.
    fn panel(
        ctx: &mut DeclareCtx<'_, AppState, Msg>,
        panel: PanelId,
        button_ids: &'static [&'static str],
    ) {
        let panel_area = ctx.area();
        let pressed = ctx.state().pressed(panel);
        let buttons: Vec<Button<Msg>> = button_ids
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let number = index as u8 + 1;
                let button =
                    Button::new(format!("Button {number}")).on_press(move || Msg::ButtonPressed {
                        panel,
                        button: number,
                    });
                if index == 0 {
                    button
                } else {
                    button.secondary()
                }
            })
            .collect();

        // The inner rect follows from the borders alone, so it is available
        // before the border color is: that depends on focus, which only
        // settles once the whole tree is declared.
        let inner_area = Block::bordered().inner(panel_area);
        ctx.paint(move |ctx| {
            let border = Self::panel_border(panel, ctx.contains_focus, pressed, ctx.theme);
            debug_assert_eq!(
                border.inner(panel_area),
                inner_area,
                "the painted block's inner rect must match the one the layout used"
            );
            ctx.widget(border, panel_area);
        });

        let [buttons_area] = inner_area.layout(
            &Layout::vertical([Constraint::Length(ratcn::ButtonSize::Small.height())])
                .flex(Flex::Center),
        );
        let button_areas = buttons_area.layout_vec(
            &Layout::horizontal(
                buttons
                    .iter()
                    .map(|button| Constraint::Length(button.width())),
            )
            .flex(Flex::Center)
            .spacing(2),
        );
        for ((id, button), button_area) in button_ids.iter().zip(buttons).zip(button_areas) {
            ctx.component(*id, button, button_area);
        }
    }

    /// The border drawn around a panel's area: it lights up while the panel
    /// contains focus, and the last press is printed on its bottom edge.
    fn panel_border(
        panel: PanelId,
        panel_contains_focus: bool,
        pressed_button: Option<u8>,
        theme: &Theme,
    ) -> Block<'static> {
        // Both panels go through here, so they cannot disagree about what a
        // focused panel looks like.
        let border = if panel_contains_focus {
            theme.ring
        } else {
            theme.border
        };
        let letter = panel.letter();
        let mut block = Block::bordered()
            .border_style(Style::default().fg(border))
            .style(Style::default().fg(theme.foreground).bg(theme.background))
            .title(format!(" Panel {letter}, press {letter} to select "));
        if let Some(button_number) = pressed_button {
            block = block.title_bottom(format!(" Panel {letter} button {button_number} pressed "));
        }
        block
    }
}

fn main() -> io::Result<()> {
    demo_shared::run(App::new())
}
