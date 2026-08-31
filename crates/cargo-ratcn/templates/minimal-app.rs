use std::io;

use ratatui::{
    style::Style,
    widgets::{Block, Paragraph},
};
use ratcn::{
    Theme,
    terminal::{Session, SessionEvent, SessionOptions, termina},
};

fn main() -> io::Result<()> {
    let mut session = Session::open(SessionOptions::new().adaptive())?;

    loop {
        let theme = session.theme();
        session.terminal_mut().draw(|frame| draw(frame, &theme))?;

        if let Some(SessionEvent::Input(event)) = session.next(None)?
            && is_quit(&event)
        {
            return Ok(());
        }
    }
}

fn draw(frame: &mut ratatui::Frame, theme: &Theme) {
    let message = Paragraph::new("Ratcn is ready.\n\nPress Ctrl+C to exit.")
        .style(Style::default().fg(theme.foreground).bg(theme.background))
        .block(Block::bordered().title("Ratcn"));
    frame.render_widget(message, frame.area());
}

fn is_quit(event: &termina::Event) -> bool {
    use termina::event::{KeyCode, KeyEventKind, Modifiers};

    matches!(
        event,
        termina::Event::Key(key)
            if key.kind == KeyEventKind::Press
                && key.code == KeyCode::Char('c')
                && key.modifiers.contains(Modifiers::CONTROL)
    )
}
