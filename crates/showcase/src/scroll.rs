//! The page-key arithmetic both scrolling views answer from.
//!
//! A [`ScrollArea`] answers Page keys only while focus is inside it, and both
//! scrolling views open with focus still in the header — so the app answers
//! them instead, out of the measurement the view just made. One type rather
//! than one per view: the rule is the same on both pages, and two copies of it
//! would drift.
//!
//! [`ScrollArea`]: ratcn::ScrollArea

use ratcn::runtime::KeyCode;

/// A scrolling page's vertical extent, as the frame that drew it measured it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scroll {
    /// Rows the page occupies: what the scroll area is told.
    pub content: u16,
    /// Rows the viewport shows.
    pub viewport: u16,
    /// The offset in force, clamped to the travel the content has. The area
    /// clamps the offset it lays out from the same way, so reading the app's
    /// raw value anywhere else would draw the page and anything blitted into
    /// it at two different rows.
    pub offset: u16,
}

impl Scroll {
    /// A page `content` rows tall in a `viewport`-row window, asked for
    /// `offset` and given what the travel allows.
    #[must_use]
    pub const fn new(content: u16, viewport: u16, offset: u16) -> Self {
        let furthest = content.saturating_sub(viewport);
        Self {
            content,
            viewport,
            offset: if offset < furthest { offset } else { furthest },
        }
    }

    /// Where a page key leaves the scroll offset, or [`None`] when it is not
    /// one of them or the page is already there.
    ///
    /// A scrolling page is the whole view, so these keys are the app's the way
    /// an embedded demo's own chords are its. The scroll area answers them
    /// first whenever focus is inside it; this is what happens when it is not,
    /// and nothing here reaches into the area to find out.
    #[must_use]
    pub fn scrolled(&self, key: KeyCode) -> Option<u16> {
        let next = match key {
            KeyCode::PageDown => self.offset.saturating_add(self.viewport),
            KeyCode::PageUp => self.offset.saturating_sub(self.viewport),
            KeyCode::Home => 0,
            KeyCode::End => self.furthest(),
            _ => return None,
        };
        self.moved_to(next)
    }

    /// The last row the page can be scrolled to.
    #[must_use]
    pub const fn furthest(&self) -> u16 {
        self.content.saturating_sub(self.viewport)
    }

    /// `next`, clamped and reported only if it is somewhere new.
    #[must_use]
    pub fn moved_to(&self, next: u16) -> Option<u16> {
        let next = next.min(self.furthest());
        (next != self.offset).then_some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The page keys are the app's on both scrolling views, so their
    /// arithmetic is the app's to get right: neither end may be overshot, and
    /// a page that cannot scroll must not claim it did.
    #[test]
    fn page_keys_step_by_a_viewport_and_stop_at_both_ends() {
        // Forty rows of page in a ten-row viewport: thirty rows of travel.
        let scroll = |key, offset| Scroll::new(40, 10, offset).scrolled(key);

        assert_eq!(
            scroll(KeyCode::PageDown, 0),
            Some(10),
            "a page down from the top"
        );
        assert_eq!(
            scroll(KeyCode::PageDown, 25),
            Some(30),
            "the last page down is a short one"
        );
        assert_eq!(
            scroll(KeyCode::PageDown, 30),
            None,
            "a page down at the bottom moves nothing"
        );
        assert_eq!(
            scroll(KeyCode::PageUp, 0),
            None,
            "and a page up at the top moves nothing"
        );
        assert_eq!(
            scroll(KeyCode::PageUp, 25),
            Some(15),
            "a page up from the middle"
        );
        assert_eq!(scroll(KeyCode::End, 0), Some(30), "End is the furthest row");
        assert_eq!(scroll(KeyCode::Home, 30), Some(0), "Home is the first");
        assert_eq!(
            scroll(KeyCode::Enter, 0),
            None,
            "and Enter is not a page key"
        );

        for key in [
            KeyCode::PageDown,
            KeyCode::PageUp,
            KeyCode::Home,
            KeyCode::End,
        ] {
            assert_eq!(
                Scroll::new(8, 20, 0).scrolled(key),
                None,
                "{key:?} on a page shorter than its viewport"
            );
        }
    }

    /// An offset past the travel is not a scroll position anyone can be at.
    #[test]
    fn an_offset_past_the_end_is_clamped_on_the_way_in() {
        assert_eq!(Scroll::new(40, 10, 99).offset, 30);
        assert_eq!(Scroll::new(8, 20, 5).offset, 0, "a page that cannot scroll");
    }
}
