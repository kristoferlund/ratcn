//! A terminal that records what was written to it and answers from a script.
//!
//! `termina::Terminal` is an ordinary public trait, so the whole exchange — and
//! everything built on it — can be driven with no terminal anywhere.
//! `event_reader` is the one method whose return type this crate cannot build,
//! and nothing under test calls it.

use std::{cell::RefCell, collections::VecDeque, io, time::Duration};

use termina::{Event, Parser, Terminal};

use super::Source;

/// How many times a pump may ask a [`Source`] before the loop it is in counts
/// as a spin.
///
/// Every script is a handful of steps, and every pass through that loop spends
/// one: a pump still asking after this many has stopped making progress through
/// anything. Counting `polls` serves this because one instance answers one
/// reader, so nothing else is adding to it.
const POLLS: usize = 64;

/// A terminal that records what was written to it and answers from a script.
///
/// Only what an exchange touches is real: the writes, the polls and reads, and
/// the timeout each wait was given. `event_reader` is the one method whose
/// return type this crate cannot build, and nothing here calls it — the reading
/// goes through the trait, which is what lets the whole exchange be driven with
/// no terminal anywhere.
///
/// One queue of steps serves two readers, and an instance answers one of them:
///
/// - [`answering`](Self::answering) makes the [`Terminal`] the opening query
///   exchange reads. Polls and reads carry the exchange's filter, and both
///   answer straight away.
/// - [`scripted`](Self::scripted) makes the [`Source`] a session's pump reads.
///   Steps come in the order they were written, and a wait step is sat out for
///   real, so the pump's deadline arithmetic is measured against a clock that
///   moved.
pub(super) struct FakeTerminal {
    pub(super) written: Vec<u8>,
    /// Steps not yet handed over, oldest first. [`None`] is a wait to sit out,
    /// which only a [`Source`] reaches.
    steps: RefCell<VecDeque<Option<Event>>>,
    /// The timeout each `poll` was given, in order.
    pub(super) polls: RefCell<Vec<Option<Duration>>>,
}

impl FakeTerminal {
    /// A [`Terminal`] that answers the query exchange with whatever `script`
    /// parses to.
    pub(super) fn answering(script: &str) -> Self {
        Self::scripted(script, &[])
    }

    /// A [`Source`] that hands a pump whatever `script` parses to, sitting out
    /// the wait it is given at each index `waits` names.
    pub(super) fn scripted(script: &str, waits: &[usize]) -> Self {
        let mut parser = Parser::default();
        parser.parse(script.as_bytes(), false);
        let mut events = VecDeque::new();
        while let Some(event) = parser.pop() {
            events.push_back(event);
        }
        let mut steps: VecDeque<Option<Event>> = VecDeque::new();
        for (index, event) in events.into_iter().enumerate() {
            if waits.contains(&index) {
                steps.push_back(None);
            }
            steps.push_back(Some(event));
        }
        if waits.contains(&steps.len()) {
            steps.push_back(None);
        }
        Self {
            written: Vec::new(),
            steps: RefCell::new(steps),
            polls: RefCell::new(Vec::new()),
        }
    }

    pub(super) fn still_pending(&self) -> Vec<Event> {
        self.steps.borrow().iter().flatten().cloned().collect()
    }

    /// The timeout the `index`th poll was given, if it was bounded.
    pub(super) fn poll_at(&self, index: usize) -> Option<Duration> {
        self.polls.borrow().get(index).copied().flatten()
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
        Ok(self.steps.borrow().iter().flatten().any(filter))
    }

    fn read<F: Fn(&Event) -> bool>(&self, filter: F) -> io::Result<Event> {
        let mut steps = self.steps.borrow_mut();
        match steps
            .iter()
            .position(|step| step.as_ref().is_some_and(&filter))
        {
            Some(index) => Ok(steps
                .remove(index)
                .flatten()
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

impl Source for FakeTerminal {
    fn poll(&self, timeout: Option<Duration>) -> io::Result<bool> {
        self.polls.borrow_mut().push(timeout);
        let asked = self.polls.borrow().len();
        if asked > POLLS {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                format!("the source was asked {asked} times: the loop is spinning"),
            ));
        }
        match self.steps.borrow().front() {
            Some(Some(_)) => return Ok(true),
            Some(None) => {}
            // A spent script is a terminal with nothing more to say: it sits
            // out whatever wait it was given, and answers an unbounded one the
            // way a silent terminal does, by staying silent.
            None => {
                if let Some(timeout) = timeout {
                    std::thread::sleep(timeout);
                }
                return Ok(false);
            }
        }
        self.steps.borrow_mut().pop_front();
        if let Some(timeout) = timeout {
            std::thread::sleep(timeout);
        }
        Ok(false)
    }

    fn read(&self) -> io::Result<Event> {
        self.steps
            .borrow_mut()
            .pop_front()
            .flatten()
            .ok_or_else(|| io::Error::new(io::ErrorKind::WouldBlock, "nothing pending"))
    }
}
