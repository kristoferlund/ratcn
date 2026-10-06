//! Four [`Input`]s, each with a different adornment: muted text painted
//! inside the field, before or after what is typed.
//!
//! An adornment is a `prefix` or a `suffix`: a string, or a styled `Line`.
//! The text starts a cell after the prefix and ends a cell before the suffix,
//! and a click on either puts the cursor at that end of the text. Tab moves
//! between the fields.

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Rect},
    style::Style,
};
use ratcn::{
    Input, InputState, Theme,
    runtime::{Event, EventResult, FocusState, PointerShape, Ratcn, TabWrap},
};

#[derive(Default)]
struct State {
    focus: FocusState,
    website: InputState,
    search: InputState,
    price: InputState,
    weight: InputState,
}

#[derive(Clone)]
enum Msg {
    Focus(FocusState),
    Website(InputState),
    Search(InputState),
    Price(InputState),
    Weight(InputState),
}

pub struct App {
    state: State,
    ratcn: Ratcn<State, Msg>,
}

impl App {
    pub fn new() -> Self {
        Self {
            state: State::default(),
            ratcn: Ratcn::new()
                .focus(|s: &State| &s.focus, Msg::Focus)
                .tab_wrap(TabWrap::Wrap),
        }
    }

    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Focus(focus) => self.state.focus = focus,
            Msg::Website(website) => self.state.website = website,
            Msg::Search(search) => self.state.search = search,
            Msg::Price(price) => self.state.price = price,
            Msg::Weight(weight) => self.state.weight = weight,
        }
    }
}

impl demo_shared::Demo for App {
    /// Pastes, and a browser's copy and cut, reach the focused field.
    const CLIPBOARD: bool = true;

    fn handle_event(&mut self, event: Event) -> bool {
        match self.ratcn.handle_event(event, &self.state) {
            EventResult::Emit(msg) => {
                self.update(msg);
                true
            }
            EventResult::Consumed => true,
            EventResult::Ignored => false,
        }
    }

    /// What a copy or cut in a field put on the clipboard, for the demo host
    /// to write out. To hook up the clipboard in an app of your own, see
    /// <https://ratcn.com/docs/concepts/host-integration#the-clipboard>.
    fn take_clipboard(&mut self) -> Option<String> {
        self.ratcn.take_clipboard()
    }

    fn pointer_shape(&self) -> PointerShape {
        self.ratcn.pointer_shape()
    }

    fn draw(&mut self, buffer: &mut Buffer, area: Rect, theme: &Theme) {
        buffer.set_style(area, Style::default().bg(theme.background));
        let [column] = Layout::horizontal([Constraint::Length(40)])
            .flex(Flex::Center)
            .areas(area);
        let [website_area, search_area, price_area, weight_area] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .spacing(1)
        .flex(Flex::Center)
        .areas(column);

        self.ratcn
            .render_into(buffer, area, &self.state, theme, |ctx| {
                ctx.component(
                    "website",
                    Input::new()
                        .value(|s: &State| &s.website, Msg::Website)
                        .title("Website")
                        .prefix("https://")
                        .placeholder("ratcn.com"),
                    website_area,
                );
                ctx.component(
                    "search",
                    Input::new()
                        .value(|s: &State| &s.search, Msg::Search)
                        .prefix("🔍")
                        .placeholder("Search"),
                    search_area,
                );
                ctx.component(
                    "price",
                    Input::new()
                        .value(|s: &State| &s.price, Msg::Price)
                        .title("Price")
                        .prefix("$")
                        .suffix("USD")
                        .placeholder("0.00"),
                    price_area,
                );
                ctx.component(
                    "weight",
                    Input::new()
                        .value(|s: &State| &s.weight, Msg::Weight)
                        .title("Weight")
                        .suffix("kg")
                        .placeholder("0"),
                    weight_area,
                );
            });
    }
}

#[cfg(test)]
mod tests {
    use demo_shared::Demo as _;
    use ratcn::runtime::{KeyCode, KeyEvent};

    use super::*;

    /// The app with a frame drawn: the runtime routes nothing before one.
    fn draw(app: &mut App) -> String {
        let area = Rect::new(0, 0, 60, 20);
        let mut buffer = Buffer::empty(area);
        app.draw(&mut buffer, area, &Theme::default_dark());
        buffer.content.iter().map(|cell| cell.symbol()).collect()
    }

    fn press(app: &mut App, code: KeyCode) {
        assert!(app.handle_event(Event::Key(KeyEvent::new(code))));
        draw(app);
    }

    fn type_text(app: &mut App, text: &str) {
        text.chars()
            .for_each(|char| press(app, KeyCode::Char(char)));
    }

    /// The adornments are painted, not typed: the value holds only what the
    /// user typed, and it shows a cell after the prefix.
    #[test]
    fn typing_lands_after_the_prefix_and_stays_out_of_the_value() {
        let mut app = App::new();
        assert!(draw(&mut app).contains("https:// ratcn.com"));

        type_text(&mut app, "example.org");

        assert_eq!(app.state.website.value(), "example.org");
        assert!(draw(&mut app).contains("https:// example.org"));
    }

    #[test]
    fn tab_moves_between_the_fields_and_every_adornment_paints() {
        let mut app = App::new();
        draw(&mut app);
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "12.50");

        assert_eq!(app.state.price.value(), "12.50");
        let screen = draw(&mut app);
        for adornment in ["🔍", "$ 12.50", "USD", "kg"] {
            assert!(screen.contains(adornment), "{adornment}");
        }
    }
}
