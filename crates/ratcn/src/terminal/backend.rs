//! The backend a [`Session`](super::Session) draws through.
//!
//! It is termina's, with two things added on the way out: every color brought
//! within what the terminal shows (see `depth`), and every frame bracketed in
//! synchronized output (DEC private mode 2026), so the terminal shows a frame
//! only once it has arrived whole. Ratatui has no hook for the bracket, and an
//! app draws through [`Terminal::draw`](ratatui::Terminal::draw), which the
//! session never sees, so the backend is where it can go.
//!
//! A frame opens with its cells or a clear — the clear of a resize included,
//! which is where tearing shows most — and closes at the flush that ends it.
//! The cursor calls open nothing: inside a draw they follow the cells, and
//! ratatui's `Terminal` shows the cursor again as it drops, after the session
//! has given the terminal back, where an opened frame would never close. A
//! terminal without the mode ignores both.

use std::io::{self, Write};

use ratatui::{
    backend::{Backend, ClearType, WindowSize},
    buffer::Cell,
    layout::{Position, Size},
};
use ratatui_termina::TerminaBackend;
use termina::{PlatformTerminal, Terminal};

use super::depth::ColorDepth;

/// `CSI ? 2026 h`: hold what follows until the frame is whole.
pub(super) const BEGIN_FRAME: &str = "\x1b[?2026h";

/// `CSI ? 2026 l`: show the frame.
pub(super) const END_FRAME: &str = "\x1b[?2026l";

/// The backend a [`Session`](super::Session) draws through: the terminal's
/// own, with every color brought within what the terminal shows, and every
/// frame shown at once.
#[derive(Debug)]
pub struct SessionBackend<T: Terminal = PlatformTerminal> {
    inner: TerminaBackend<T>,
    depth: ColorDepth,
    /// Whether a frame's opening bracket went out and its closing one has not.
    framing: bool,
}

impl<T: Terminal> SessionBackend<T> {
    pub(super) const fn new(inner: TerminaBackend<T>, depth: ColorDepth) -> Self {
        Self {
            inner,
            depth,
            framing: false,
        }
    }

    /// Open the frame, once, before its first write.
    fn frame(&mut self) -> io::Result<()> {
        if !self.framing {
            self.inner.write_all(BEGIN_FRAME.as_bytes())?;
            self.framing = true;
        }
        Ok(())
    }
}

impl<T: Terminal> Write for SessionBackend<T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Write::flush(&mut self.inner)
    }
}

impl<T: Terminal> Backend for SessionBackend<T> {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        self.frame()?;
        if self.depth == ColorDepth::TrueColor {
            return self.inner.draw(content);
        }
        let reduced: Vec<(u16, u16, Cell)> = content
            .map(|(x, y, cell)| (x, y, self.depth.reduce_cell(cell)))
            .collect();
        self.inner
            .draw(reduced.iter().map(|(x, y, cell)| (*x, *y, cell)))
    }

    fn append_lines(&mut self, n: u16) -> io::Result<()> {
        self.frame()?;
        self.inner.append_lines(n)
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        self.inner.get_cursor_position()
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.inner.set_cursor_position(position)
    }

    fn clear(&mut self) -> io::Result<()> {
        self.frame()?;
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        self.frame()?;
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> io::Result<Size> {
        self.inner.size()
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.framing {
            self.inner.write_all(END_FRAME.as_bytes())?;
            self.framing = false;
        }
        Backend::flush(&mut self.inner)
    }
}

#[cfg(test)]
mod tests {
    use super::{BEGIN_FRAME, ColorDepth, END_FRAME, SessionBackend};
    use crate::terminal::fake::FakeTerminal;
    use ratatui::{Terminal, layout::Rect, style::Color, widgets::Paragraph};
    use ratatui_termina::TerminaBackend;

    fn terminal(depth: ColorDepth) -> Terminal<SessionBackend<FakeTerminal>> {
        let backend = SessionBackend::new(TerminaBackend::new(FakeTerminal::scripted("")), depth);
        Terminal::new(backend).expect("a fake terminal has a size")
    }

    fn written(terminal: &mut Terminal<SessionBackend<FakeTerminal>>) -> String {
        let fake = terminal.backend_mut().inner.terminal_mut();
        String::from_utf8(std::mem::take(&mut fake.written)).expect("a frame is UTF-8")
    }

    /// Ratatui shows a hidden cursor again as the `Terminal` drops, after the
    /// session has restored the terminal: that must not open a frame nothing
    /// will close, which would hold the user's shell.
    #[test]
    fn the_cursor_alone_opens_no_frame() {
        let mut terminal = terminal(ColorDepth::TrueColor);
        terminal
            .hide_cursor()
            .expect("a fake terminal takes writes");
        terminal
            .show_cursor()
            .expect("a fake terminal takes writes");
        terminal
            .set_cursor_position((1, 1))
            .expect("a fake terminal takes writes");

        let written = written(&mut terminal);
        assert!(!written.contains(BEGIN_FRAME), "{written:?}");
    }

    /// The terminal holds a frame until it is whole: the bracket opens before
    /// the first cell and closes after the cursor, at the flush that ends the
    /// frame, and a frame with nothing to change is still one frame.
    #[test]
    fn every_frame_is_bracketed_in_synchronized_output() {
        let mut terminal = terminal(ColorDepth::TrueColor);

        for _ in 0..2 {
            terminal
                .draw(|frame| frame.render_widget(Paragraph::new("hi"), frame.area()))
                .expect("a fake terminal takes writes");
            let frame = written(&mut terminal);

            assert!(frame.starts_with(BEGIN_FRAME), "{frame:?}");
            assert!(frame.ends_with(END_FRAME), "{frame:?}");
            assert_eq!(frame.matches(BEGIN_FRAME).count(), 1, "{frame:?}");
        }
    }

    /// A resize clears the screen before the frame redraws it; inside the
    /// bracket, the blank screen in between is never shown.
    #[test]
    fn a_resize_clears_inside_the_frame() {
        let mut terminal = terminal(ColorDepth::TrueColor);
        terminal
            .resize(Rect::new(0, 0, 40, 10))
            .expect("a fake terminal takes writes");
        terminal
            .draw(|frame| frame.render_widget(Paragraph::new("hi"), frame.area()))
            .expect("a fake terminal takes writes");
        let frame = written(&mut terminal);

        assert!(frame.starts_with(BEGIN_FRAME), "{frame:?}");
        assert!(frame.contains("\x1b[2J"), "the clear went out: {frame:?}");
        assert_eq!(frame.matches(BEGIN_FRAME).count(), 1, "{frame:?}");
        assert!(frame.ends_with(END_FRAME), "{frame:?}");
    }

    /// The colors are brought within the terminal's depth on the same path.
    #[test]
    fn a_frame_reaches_a_256_color_terminal_without_rgb() {
        let mut terminal = terminal(ColorDepth::Ansi256);
        terminal
            .draw(|frame| {
                frame.render_widget(
                    Paragraph::new("hi")
                        .style(ratatui::style::Style::new().bg(Color::Rgb(10, 10, 10))),
                    frame.area(),
                );
            })
            .expect("a fake terminal takes writes");
        let frame = written(&mut terminal);

        assert!(!frame.contains("48;2;"), "{frame:?}");
        assert!(frame.contains("48;5;"), "{frame:?}");
    }
}
