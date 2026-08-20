//! A terminal that records what was written to it and answers from a script.
//!
//! `termina::Terminal` is an ordinary public trait, so the whole exchange — and
//! everything built on it — can be driven with no terminal anywhere.
//! `event_reader` is the one method whose return type this crate cannot build,
//! and nothing under test calls it.

use std::{cell::RefCell, collections::VecDeque, io, time::Duration};

use termina::{Event, Parser, Terminal};

/// A terminal that records what was written to it and answers from a script.
///
/// Only what the exchange touches is real: the writes, the filtered polls
/// and reads, and the timeout each wait was given. `event_reader` is the one
/// method whose return type this crate cannot build, and the exchange never
/// calls it — it reads through the trait, which is what lets the whole
/// exchange be driven with no terminal anywhere.
pub(super) struct FakeTerminal {
    pub(super) written: Vec<u8>,
    /// Replies not yet handed over, oldest first.
    pub(super) pending: RefCell<VecDeque<Event>>,
    /// The timeout each `poll` was given, in order.
    pub(super) polls: RefCell<Vec<Option<Duration>>>,
}

impl FakeTerminal {
    /// A terminal that will answer with whatever `script` parses to.
    pub(super) fn answering(script: &str) -> Self {
        let mut parser = Parser::default();
        parser.parse(script.as_bytes(), false);
        let mut pending = VecDeque::new();
        while let Some(event) = parser.pop() {
            pending.push_back(event);
        }
        Self {
            written: Vec::new(),
            pending: RefCell::new(pending),
            polls: RefCell::new(Vec::new()),
        }
    }

    pub(super) fn still_pending(&self) -> Vec<Event> {
        self.pending.borrow().iter().cloned().collect()
    }
}

impl io::Write for FakeTerminal {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.written.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Terminal for FakeTerminal {
    fn enter_raw_mode(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn enter_cooked_mode(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn get_dimensions(&self) -> io::Result<termina::WindowSize> {
        Ok(termina::WindowSize {
            cols: 80,
            rows: 24,
            pixel_width: None,
            pixel_height: None,
        })
    }

    fn event_reader(&self) -> termina::EventReader {
        unimplemented!("the exchange reads through `poll` and `read`, never through a reader")
    }

    fn poll<F: Fn(&Event) -> bool>(
        &self,
        filter: F,
        timeout: Option<Duration>,
    ) -> io::Result<bool> {
        self.polls.borrow_mut().push(timeout);
        Ok(self.pending.borrow().iter().any(filter))
    }

    fn read<F: Fn(&Event) -> bool>(&self, filter: F) -> io::Result<Event> {
        let mut pending = self.pending.borrow_mut();
        match pending.iter().position(filter) {
            Some(index) => Ok(pending
                .remove(index)
                .expect("the index came from this queue")),
            // The real reader blocks here until something matches. Failing
            // instead is what makes a read let through on the strength of
            // the wrong filter show up as a failure rather than a hang.
            None => Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "nothing pending matches the filter",
            )),
        }
    }

    fn set_panic_hook(
        &mut self,
        _hook: impl Fn(&mut termina::PlatformHandle) + Send + Sync + 'static,
    ) {
    }
}
