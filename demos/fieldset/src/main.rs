//! Fieldset demo: a composite component of the app's own, used twice.
//!
//! `fieldset.rs` is the component — a labeled group box with a caller-supplied
//! body, a measured action beside its label, a collapse the app owns, and a
//! disabled state that dims the whole group and takes it out of interaction.
//! It is the worked example for
//! [Building a composite](https://ratcn.kristoferlund.se/docs/concepts/building-a-composite),
//! which walks through both files piece by piece.
//!
//! This file is the caller: it stacks two fieldsets, fills their bodies, and
//! owns every flag they read.
//!
//! Controls:
//! - `Tab` / `Shift+Tab`: move focus through the groups and their controls
//! - `Enter` / `Space`: press the focused control, or toggle the focused group
//! - `←` / `→`: collapse and expand the group focus is inside

use std::io;

use fieldset::Fieldset;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use ratcn::{
    Button, ButtonSize, Theme,
    runtime::{DeclareCtx, Event, EventResult, FocusState, Ratcn},
};

mod fieldset;

const THEME: Theme = Theme::default_dark();
const DEMO_WIDTH: u16 = 54;
const DEMO_HEIGHT: u16 = 17;
const PADDING_X: u16 = 3;
const PADDING_Y: u16 = 1;
/// One row of switches, and one row of billing facts.
const BODY_HEIGHT: u16 = 1;
const GROUP_SPACING: u16 = 1;

struct AppState {
    focus: FocusState,
    notifications_collapsed: bool,
    billing_collapsed: bool,
    email: bool,
    push: bool,
    muted: bool,
    pro: bool,
    seats: u16,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            focus: FocusState::default(),
            notifications_collapsed: false,
            billing_collapsed: false,
            email: true,
            push: false,
            muted: false,
            pro: false,
            seats: 3,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Notifications,
    Billing,
}

enum Msg {
    Focus(FocusState),
    Collapse(Section, bool),
    ToggleEmail,
    TogglePush,
    ToggleMute,
    TogglePlan,
    AddSeat,
}

fn main() -> io::Result<()> {
    demo_shared::run(App::new())
}

struct App {
    state: AppState,
    ratcn: Ratcn<AppState, Msg>,
}

impl App {
    fn new() -> Self {
        Self {
            state: AppState::default(),
            ratcn: Ratcn::new().focus(|state: &AppState| &state.focus, Msg::Focus),
        }
    }
}

/// The only place app state changes. Every flag the fieldsets read is set here,
/// including the collapse they ask for — a composite owns none of it.
fn update(state: &mut AppState, msg: Msg) {
    match msg {
        Msg::Focus(focus) => state.focus = focus,
        Msg::Collapse(Section::Notifications, collapsed) => {
            state.notifications_collapsed = collapsed;
        }
        Msg::Collapse(Section::Billing, collapsed) => state.billing_collapsed = collapsed,
        Msg::ToggleEmail => state.email = !state.email,
        Msg::TogglePush => state.push = !state.push,
        Msg::ToggleMute => state.muted = !state.muted,
        Msg::TogglePlan => state.pro = !state.pro,
        Msg::AddSeat => state.seats = state.seats.saturating_add(1),
    }
}

impl demo_shared::Demo for App {
    fn background(&self) -> Color {
        THEME.background
    }

    fn handle_event(&mut self, event: Event) -> bool {
        match self.ratcn.handle_event(event, &self.state) {
            EventResult::Emit(msg) => {
                update(&mut self.state, msg);
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
        let panel = area
            .centered(
                Constraint::Length(DEMO_WIDTH),
                Constraint::Length(DEMO_HEIGHT),
            )
            .inner(Margin::new(PADDING_X, PADDING_Y));
        let state = &self.state;
        self.ratcn.render(frame, state, &THEME, |ctx| {
            let [title_area, groups_area, footer_area] = Layout::vertical([
                Constraint::Length(2),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .areas(panel);

            ctx.paint_widget(
                Paragraph::new("Project settings").style(
                    Style::default()
                        .fg(ctx.theme.foreground)
                        .add_modifier(Modifier::BOLD),
                ),
                title_area,
            );

            // #region stacking
            // Each group is handed exactly the rows it says it needs, which is
            // why the collapse can change the layout of the ones below it.
            let notifications = notifications(state);
            let billing = billing(state);
            let [notifications_area, billing_area, _rest] = Layout::vertical([
                Constraint::Length(notifications.height(state)),
                Constraint::Length(billing.height(state)),
                Constraint::Min(0),
            ])
            .spacing(GROUP_SPACING)
            .areas(groups_area);

            ctx.component("notifications", notifications, notifications_area);
            ctx.component("billing", billing, billing_area);
            // #endregion stacking

            footer(ctx, state, footer_area);
        });
    }
}

// #region caller
/// A group whose body declares components: two switches the caller wires and
/// the fieldset knows nothing about.
fn notifications(state: &AppState) -> Fieldset<AppState, Msg> {
    let (email, push) = (state.email, state.push);
    Fieldset::new("Notifications")
        .collapsed(|state: &AppState| state.notifications_collapsed)
        .on_toggle(|collapsed| Msg::Collapse(Section::Notifications, collapsed))
        .action(
            "mute",
            Button::new(if state.muted { "Unmute" } else { "Mute" })
                .ghost()
                .on_press(|| Msg::ToggleMute),
        )
        .body(BODY_HEIGHT, move |ctx| {
            let [email_area, push_area, _rest] = Layout::horizontal([
                Constraint::Length(12),
                Constraint::Length(12),
                Constraint::Min(0),
            ])
            .areas(ctx.area());
            ctx.component(
                "email",
                switch("Email", email, || Msg::ToggleEmail),
                email_area,
            );
            ctx.component("push", switch("Push", push, || Msg::TogglePush), push_area);
        })
}
// #endregion caller

/// A group whose body only paints, and which goes inert with the plan: dimmed,
/// unfocusable, and unclickable, action button included.
fn billing(state: &AppState) -> Fieldset<AppState, Msg> {
    let seats = state.seats;
    Fieldset::new("Billing")
        .collapsed(|state: &AppState| state.billing_collapsed)
        .on_toggle(|collapsed| Msg::Collapse(Section::Billing, collapsed))
        .disabled(!state.pro)
        // A three-row action, which the header sizes itself to: the fieldset
        // measures what it is given rather than assuming a one-row button.
        .action(
            "seat",
            Button::new("+ Seat")
                .secondary()
                .size(ButtonSize::Large)
                .on_press(|| Msg::AddSeat),
        )
        .body(BODY_HEIGHT, move |ctx| {
            // Indented to line up with the label above it, which the button
            // idiom's own padding puts one cell in.
            let area = Rect {
                x: ctx.area().x + 1,
                ..ctx.area()
            };
            ctx.paint_widget(
                Paragraph::new(format!("{seats} seats \u{b7} billed monthly"))
                    .style(Style::default().fg(ctx.theme.muted_foreground)),
                area,
            );
        })
}

fn switch(label: &str, on: bool, msg: fn() -> Msg) -> Button<Msg> {
    let mark = if on { "\u{25cf}" } else { "\u{25cb}" };
    Button::new(format!("{mark} {label}")).ghost().on_press(msg)
}

fn footer(ctx: &mut DeclareCtx<'_, AppState, Msg>, state: &AppState, area: Rect) {
    let [plan_area, hint_area] =
        Layout::horizontal([Constraint::Length(16), Constraint::Min(0)]).areas(area);
    ctx.component(
        "plan",
        Button::new(if state.pro {
            "Downgrade"
        } else {
            "Upgrade to Pro"
        })
        .outline()
        .on_press(|| Msg::TogglePlan),
        plan_area,
    );
    let plan = if state.pro { "pro" } else { "free" };
    ctx.paint_widget(
        Paragraph::new(Line::from(vec![Span::raw(format!("  plan: {plan}"))]))
            .style(Style::default().fg(ctx.theme.muted_foreground)),
        hint_area,
    );
}

#[cfg(test)]
mod tests {
    use demo_shared::Demo as _;
    use ratatui::{Terminal, backend::TestBackend};
    use ratcn::runtime::{ChildId, KeyCode, KeyEvent};

    use super::*;

    fn app() -> (App, Terminal<TestBackend>) {
        (
            App::new(),
            Terminal::new(TestBackend::new(DEMO_WIDTH, DEMO_HEIGHT)).expect("terminal"),
        )
    }

    fn draw(app: &mut App, terminal: &mut Terminal<TestBackend>) {
        terminal.draw(|frame| app.draw(frame)).expect("draw");
    }

    fn press(app: &mut App, terminal: &mut Terminal<TestBackend>, code: KeyCode) {
        draw(app, terminal);
        app.handle_event(Event::Key(KeyEvent::new(code)));
    }

    fn rendered(terminal: &Terminal<TestBackend>) -> String {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .flat_map(|row| {
                (0..buffer.area.width)
                    .map(move |column| (column, row))
                    .map(|position| buffer.cell(position).expect("cell").symbol().to_owned())
            })
            .collect()
    }

    /// Collapsing the first group moves the second one up: the caller sizes each
    /// slot from the fieldset's own answer, so nothing here duplicates the
    /// component's layout arithmetic.
    #[test]
    fn collapsing_a_group_reflows_the_one_below_it() {
        let (mut app, mut terminal) = app();
        draw(&mut app, &mut terminal);
        let expanded = billing(&app.state).height(&app.state);

        update(&mut app.state, Msg::Collapse(Section::Notifications, true));
        draw(&mut app, &mut terminal);

        let rows = rendered(&terminal);
        assert!(rows.contains("Notifications"), "the group is still there");
        assert!(
            !rows.contains("Email"),
            "a collapsed group declares no body: {rows}"
        );
        assert_eq!(
            billing(&app.state).height(&app.state),
            expanded,
            "the second group did not change size, only position"
        );
    }

    /// The disabled group is inert including its action: the runtime cannot
    /// focus into an empty interaction area, so Tab skips the whole section and
    /// the plan button is the next stop after the notification switches.
    #[test]
    fn tab_skips_a_disabled_group_entirely() {
        let (mut app, mut terminal) = app();
        assert!(!app.state.pro, "billing starts disabled");

        for _ in 0..4 {
            press(&mut app, &mut terminal, KeyCode::Tab);
        }

        assert_eq!(
            app.state.focus.path(),
            &[ChildId::Static("plan")],
            "focus walked mute, email, push, then straight to the plan button"
        );
    }

    /// Enabling the plan brings the group back without any of its own state
    /// having been kept: the fieldset reads the flag, it does not remember it.
    #[test]
    fn the_plan_button_enables_the_billing_group_and_its_action() {
        let (mut app, mut terminal) = app();
        app.state.focus = FocusState::intent([ChildId::Static("plan")]);

        press(&mut app, &mut terminal, KeyCode::Enter);
        assert!(app.state.pro);

        let seats = app.state.seats;
        app.state.focus = FocusState::intent([ChildId::Static("billing"), ChildId::Static("seat")]);
        press(&mut app, &mut terminal, KeyCode::Enter);

        assert_eq!(app.state.seats, seats + 1, "the action is live again");
    }
}
