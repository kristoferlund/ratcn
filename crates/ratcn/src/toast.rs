//! App-owned toast state: [`Toast`], [`ToastEntry`], and [`ToasterState`] —
//! what your app stores for the toast stack.
//! A component reads this state and never mutates it; the app's
//! `update` persists what [`ToasterState`] returns. The library never reads a
//! clock — time arrives as [`Duration`]s the app supplies, so pushing and
//! pruning are driven by whatever timestamp source the app already has.
//!
//! [`ToasterStyle`](crate::ToasterStyle) and
//! [`ToasterWidget`](crate::ToasterWidget) — the paint half — stay
//! in the copyable `components::toast` module; this state travels with the
//! library so `ratcn::toast::ToasterState` stays one type across copies.

use std::borrow::Cow;
use std::time::Duration;

const DEFAULT_DURATION: Duration = Duration::from_secs(4);

/// What a toast is telling the user, which picks its accent color and icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToastKind {
    /// Neutral. No particular good or bad news.
    #[default]
    Default,
    /// Something worked.
    Success,
    /// Something failed. Consider [`Toast::persistent`] so it is not missed.
    Error,
    /// Something needs attention but did not fail.
    Warning,
    /// Neutral information, accented to stand out from `Default`.
    Info,
    /// Something is in progress. Usually paired with [`Toast::persistent`].
    Loading,
}

/// One toast's content and lifetime: a title, an optional description, a
/// [`ToastKind`] accent, and a duration (or [`persistent`](Toast::persistent)).
///
/// A toast has no width-independent height: title and description wrap at the
/// stack width, so [`ToasterWidget`](crate::ToasterWidget) measures each toast
/// against the width it paints at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast<'a> {
    id: Option<Cow<'a, str>>,
    title: Cow<'a, str>,
    description: Option<Cow<'a, str>>,
    kind: ToastKind,
    duration: Option<Duration>,
    border: bool,
}

impl<'a> Toast<'a> {
    /// A neutral toast with the default 4-second lifetime.
    #[must_use]
    pub fn new(title: impl Into<Cow<'a, str>>) -> Self {
        Self {
            id: None,
            title: title.into(),
            description: None,
            kind: ToastKind::Default,
            duration: Some(DEFAULT_DURATION),
            border: true,
        }
    }

    /// Shorthand for [`ToastKind::Success`].
    #[must_use]
    pub fn success(title: impl Into<Cow<'a, str>>) -> Self {
        Self::new(title).with_kind(ToastKind::Success)
    }

    /// Shorthand for [`ToastKind::Error`].
    #[must_use]
    pub fn error(title: impl Into<Cow<'a, str>>) -> Self {
        Self::new(title).with_kind(ToastKind::Error)
    }

    /// Shorthand for [`ToastKind::Warning`].
    #[must_use]
    pub fn warning(title: impl Into<Cow<'a, str>>) -> Self {
        Self::new(title).with_kind(ToastKind::Warning)
    }

    /// Shorthand for [`ToastKind::Info`].
    #[must_use]
    pub fn info(title: impl Into<Cow<'a, str>>) -> Self {
        Self::new(title).with_kind(ToastKind::Info)
    }

    /// Shorthand for [`ToastKind::Loading`].
    #[must_use]
    pub fn loading(title: impl Into<Cow<'a, str>>) -> Self {
        Self::new(title).with_kind(ToastKind::Loading)
    }

    /// A second line of detail under the title. Wraps at the stack width.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<Cow<'a, str>>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the kind directly. See [`ToastKind`].
    #[must_use]
    pub const fn with_kind(mut self, kind: ToastKind) -> Self {
        self.kind = kind;
        self
    }

    /// How long the toast lives before
    /// [`ToasterState::prune_expired`] removes it.
    ///
    /// The clock is the app's: this is measured against the timestamp handed to
    /// `prune_expired`, so nothing expires unless the app tells the stack that
    /// time has passed.
    #[must_use]
    pub const fn duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Never expire. In [`ToasterState`], the toast stays until the app removes
    /// it through an [`id`](Self::with_id) with [`ToasterState::dismiss`] or
    /// [`ToasterState::replace`].
    ///
    /// Right for errors the user must acknowledge, and for `Loading` toasts
    /// whose work has no predictable length — give those an id so the finished
    /// work can dismiss or replace them.
    #[must_use]
    pub const fn persistent(mut self) -> Self {
        self.duration = None;
        self
    }

    /// Whether to draw a border around this toast. On by default.
    #[must_use]
    pub const fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    /// An app-chosen identity for this toast, so
    /// [`ToasterState::dismiss`] and [`ToasterState::replace`] can address it
    /// later — a "saving…" toast replaced by "saved", say. Toasts without an id
    /// (the default) leave the stack by expiring or via
    /// [`ToasterState::pop_newest`].
    ///
    /// Ids are not deduplicated: pushing two toasts with the same id shows
    /// both, and id-keyed operations then affect the oldest match.
    #[must_use]
    pub fn with_id(mut self, id: impl Into<Cow<'a, str>>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Whether a toast this old has outlived its duration. Always false for a
    /// [`persistent`](Self::persistent) toast.
    #[must_use]
    pub fn is_expired_after(&self, age: Duration) -> bool {
        match self.duration {
            Some(duration) => age >= duration,
            None => false,
        }
    }

    /// The toast's title text — a render-facing accessor so
    /// [`ToasterWidget`](crate::ToasterWidget) can paint a toast
    /// without reaching into its private fields.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The toast's optional description text (see [`title`](Self::title)).
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// The toast's [`ToastKind`] accent (see [`title`](Self::title)).
    #[must_use]
    pub const fn kind(&self) -> ToastKind {
        self.kind
    }

    /// Whether this toast draws a border (see [`title`](Self::title)).
    #[must_use]
    pub const fn is_bordered(&self) -> bool {
        self.border
    }

    /// The identity given to [`with_id`](Self::with_id), or `None` for an
    /// anonymous toast.
    #[must_use]
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }
}

/// A toast in the stack, together with when it was pushed.
///
/// [`Toast`] describes the message; this adds the one fact that only exists once
/// it is on screen — its creation time, from which expiry is computed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToastEntry<'a> {
    toast: Toast<'a>,
    created_at: Duration,
}

impl<'a> ToastEntry<'a> {
    /// Pair a toast with the timestamp it was created at.
    ///
    /// `created_at` is a reading from whatever clock the app uses, expressed as
    /// elapsed time since that clock's origin — not a wall-clock time.
    #[must_use]
    pub const fn new(toast: Toast<'a>, created_at: Duration) -> Self {
        Self { toast, created_at }
    }

    /// The message this entry is showing.
    #[must_use]
    pub const fn toast(&self) -> &Toast<'a> {
        &self.toast
    }

    /// The timestamp this entry was pushed at.
    #[must_use]
    pub const fn created_at(&self) -> Duration {
        self.created_at
    }

    /// How long this entry has been on screen as of `now`. Saturates at zero if
    /// `now` is somehow earlier than its creation.
    #[must_use]
    pub fn age(&self, now: Duration) -> Duration {
        now.saturating_sub(self.created_at)
    }

    /// Whether this entry has outlived its duration as of `now`.
    #[must_use]
    pub fn is_expired(&self, now: Duration) -> bool {
        self.toast.is_expired_after(self.age(now))
    }
}

/// The stack of toasts currently showing, owned by the app.
///
/// This holds the toasts; [`ToasterWidget`](crate::ToasterWidget) paints them.
/// Beyond [`prune_expired`](Self::prune_expired),
/// [`pop_newest`](Self::pop_newest) removes the most recent toast, and a toast
/// given an id with [`Toast::with_id`] is individually
/// [`dismiss`](Self::dismiss)ed or [`replace`](Self::replace)d. Apps that need a
/// different lifecycle entirely can own a custom [`ToastEntry`] collection and paint it with
/// [`ToasterWidget::from_entries`](crate::ToasterWidget::from_entries).
///
/// # The library never reads a clock
///
/// Toasts expire with time, but nothing here calls `Instant::now`. Every method
/// that cares about time takes a [`Duration`] from you. That keeps the crate
/// usable in the browser, where there is no `Instant`, and keeps expiry
/// testable without sleeping.
///
/// The loop that follows from it:
///
/// 1. [`push`](Self::push) a toast with the current reading from your clock.
/// 2. Ask [`time_until_next_expiry`](Self::time_until_next_expiry) when to wake
///    up next, and set a timer for it.
/// 3. When the timer fires, call [`prune_expired`](Self::prune_expired) with the
///    new reading and redraw if it returns `true`.
///
/// Skip step 2 and expired entries remain in this state. The
/// [`ToasterWidget`](crate::ToasterWidget) still hides them when painting.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToasterState<'a> {
    toasts: Vec<ToastEntry<'a>>,
}

impl<'a> ToasterState<'a> {
    /// An empty stack.
    #[must_use]
    pub const fn new() -> Self {
        Self { toasts: Vec::new() }
    }

    /// Add a toast, stamped with the current reading from your clock.
    ///
    /// Newest goes last. Nothing is deduplicated, so pushing the same message
    /// twice shows it twice.
    pub fn push(&mut self, toast: Toast<'a>, created_at: Duration) {
        self.toasts.push(ToastEntry::new(toast, created_at));
    }

    /// Drop every toast that has outlived its duration as of `now`.
    ///
    /// Returns whether anything was removed, so the caller can skip a redraw
    /// when nothing changed. Persistent toasts are never removed.
    #[must_use]
    pub fn prune_expired(&mut self, now: Duration) -> bool {
        let previous_len = self.toasts.len();
        self.toasts.retain(|toast| !toast.is_expired(now));
        self.toasts.len() != previous_len
    }

    /// How long until the next toast expires — how long to wait before calling
    /// [`prune_expired`](Self::prune_expired) again.
    ///
    /// [`Duration::ZERO`] means something has already expired, so prune now.
    /// [`None`] means there is nothing to wait for: the stack is empty or every
    /// toast in it is persistent. Persistent toasts are otherwise ignored.
    #[must_use]
    pub fn time_until_next_expiry(&self, now: Duration) -> Option<Duration> {
        self.toasts
            .iter()
            .filter_map(|entry| {
                entry
                    .toast
                    .duration
                    .map(|duration| duration.saturating_sub(entry.age(now)))
            })
            .min()
    }

    /// Remove the oldest toast whose [`Toast::id`] equals `id`.
    ///
    /// Returns whether a toast was removed; `false` means no toast carries that
    /// id, so the caller can skip a redraw. The oldest matching entry is removed
    /// whether it has expired or not; call [`prune_expired`](Self::prune_expired)
    /// first when expired entries should not take precedence. When several
    /// toasts share the id, only the oldest goes — call again to remove the next.
    /// This works on persistent toasts too, and is the intended way to end one
    /// whose work has finished.
    #[must_use = "redraw when a toast was dismissed"]
    pub fn dismiss(&mut self, id: &str) -> bool {
        match self.position_of(id) {
            Some(index) => {
                self.toasts.remove(index);
                true
            }
            None => false,
        }
    }

    /// Replace the oldest toast whose [`Toast::id`] equals `id` with `toast`, in
    /// place, restarting its lifetime.
    ///
    /// The entry keeps its position in the stack but is re-stamped with `now` —
    /// a reading from your clock, exactly as passed to [`push`](Self::push) —
    /// so the replacement's duration counts from the replacement, not from the
    /// original push. The replacement's identity is whatever id `toast`
    /// carries; give it the same id if you plan to address it again. Returns
    /// `false`, changing nothing, when no toast carries `id`. The oldest
    /// matching entry is replaced whether it has expired or not; call
    /// [`prune_expired`](Self::prune_expired) first when expired entries should
    /// not take precedence.
    #[must_use = "redraw when a toast was replaced"]
    pub fn replace(&mut self, id: &str, toast: Toast<'a>, now: Duration) -> bool {
        match self.position_of(id) {
            Some(index) => {
                self.toasts[index] = ToastEntry::new(toast, now);
                true
            }
            None => false,
        }
    }

    /// The index of the oldest entry whose toast carries `id`.
    fn position_of(&self, id: &str) -> Option<usize> {
        self.toasts
            .iter()
            .position(|entry| entry.toast.id() == Some(id))
    }

    /// Remove and return the newest toast, regardless of whether it has an id.
    ///
    /// Returns `None` when the stack is empty. This is useful for a global
    /// dismiss-last action such as an Escape key binding.
    #[must_use]
    pub fn pop_newest(&mut self) -> Option<Toast<'a>> {
        self.toasts.pop().map(|entry| entry.toast)
    }

    /// Whether the stack is empty — worth checking before reserving screen space
    /// for it.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }

    /// How many toasts are in the stack.
    #[must_use]
    pub fn len(&self) -> usize {
        self.toasts.len()
    }

    /// The toasts, oldest first — what
    /// [`ToasterWidget`](crate::ToasterWidget) paints.
    #[must_use]
    pub fn entries(&self) -> &[ToastEntry<'a>] {
        &self.toasts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persistent_toasts_do_not_expire() {
        let toast = Toast::new("saved").persistent();

        assert!(!toast.is_expired_after(Duration::from_secs(60)));
    }

    #[test]
    fn zero_duration_toast_expires_immediately() {
        let toast = Toast::new("done").duration(Duration::ZERO);

        assert!(toast.is_expired_after(Duration::ZERO));
    }

    #[test]
    fn toaster_state_prunes_expired_toasts() {
        let mut toasts = ToasterState::new();
        toasts.push(Toast::new("old"), Duration::ZERO);
        toasts.push(Toast::new("new"), Duration::from_millis(10));

        assert!(toasts.prune_expired(DEFAULT_DURATION));

        assert_eq!(toasts.len(), 1);
        assert_eq!(toasts.entries()[0].toast().title(), "new");
        assert!(!toasts.prune_expired(DEFAULT_DURATION));
    }

    #[test]
    fn toaster_state_schedules_the_earliest_expiring_toast() {
        let mut toasts = ToasterState::new();
        toasts.push(Toast::new("persistent").persistent(), Duration::ZERO);
        toasts.push(
            Toast::new("later").duration(Duration::from_secs(10)),
            Duration::from_secs(2),
        );
        toasts.push(
            Toast::new("next").duration(Duration::from_secs(4)),
            Duration::from_secs(3),
        );

        assert_eq!(
            toasts.time_until_next_expiry(Duration::from_secs(5)),
            Some(Duration::from_secs(2))
        );
    }

    #[test]
    fn toaster_state_schedules_immediate_cleanup_for_expired_toasts() {
        let mut toasts = ToasterState::new();
        toasts.push(Toast::new("expired"), Duration::ZERO);

        assert_eq!(
            toasts.time_until_next_expiry(DEFAULT_DURATION),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn dismiss_removes_the_oldest_match_by_id() {
        let mut toasts = ToasterState::new();
        toasts.push(
            Toast::loading("saving A").persistent().with_id("save"),
            Duration::ZERO,
        );
        toasts.push(
            Toast::loading("saving B").persistent().with_id("save"),
            Duration::ZERO,
        );
        toasts.push(Toast::new("other"), Duration::ZERO);

        assert!(toasts.dismiss("save"));
        assert_eq!(toasts.len(), 2);
        assert_eq!(toasts.entries()[0].toast().title(), "saving B");

        assert!(toasts.dismiss("save"));
        assert!(!toasts.dismiss("save"), "no toast carries the id anymore");
        assert_eq!(toasts.entries()[0].toast().title(), "other");
    }

    #[test]
    fn dismiss_with_an_unknown_id_returns_false_and_changes_nothing() {
        let mut toasts = ToasterState::new();
        toasts.push(Toast::new("saved").with_id("save"), Duration::ZERO);

        assert!(!toasts.dismiss("missing"));
        assert_eq!(toasts.len(), 1);
    }

    #[test]
    fn replace_swaps_in_place_and_restarts_the_duration() {
        let mut toasts = ToasterState::new();
        toasts.push(
            Toast::loading("saving").persistent().with_id("save"),
            Duration::ZERO,
        );
        toasts.push(Toast::new("other").persistent(), Duration::from_secs(1));

        assert!(toasts.replace(
            "save",
            Toast::success("saved").with_id("save"),
            Duration::from_secs(10),
        ));

        // The entry keeps its stack position but counts its lifetime from the
        // replacement timestamp, not the original push.
        let entry = &toasts.entries()[0];
        assert_eq!(entry.toast().title(), "saved");
        assert_eq!(entry.created_at(), Duration::from_secs(10));
        let just_before = (Duration::from_secs(10) + DEFAULT_DURATION)
            .checked_sub(Duration::from_millis(1))
            .expect("the deadline is past one millisecond");
        assert!(!entry.is_expired(just_before));
        assert!(entry.is_expired(Duration::from_secs(10) + DEFAULT_DURATION));
        assert_eq!(
            toasts.time_until_next_expiry(Duration::from_secs(11)),
            Some(Duration::from_secs(3))
        );
    }

    #[test]
    fn replace_with_an_unknown_id_returns_false_and_changes_nothing() {
        let mut toasts = ToasterState::new();
        toasts.push(Toast::new("saved").with_id("save"), Duration::ZERO);

        assert!(!toasts.replace("missing", Toast::new("nope"), Duration::ZERO));
        assert_eq!(toasts.entries()[0].toast().title(), "saved");
    }

    #[test]
    fn toaster_state_does_not_schedule_persistent_toasts() {
        let mut toasts = ToasterState::new();
        toasts.push(Toast::new("persistent").persistent(), Duration::ZERO);

        assert_eq!(toasts.time_until_next_expiry(Duration::from_secs(60)), None);
    }

    #[test]
    fn pop_newest_removes_identified_or_anonymous_toasts() {
        let mut toasts = ToasterState::new();
        toasts.push(Toast::new("anonymous"), Duration::ZERO);
        toasts.push(Toast::new("identified").with_id("latest"), Duration::ZERO);

        assert_eq!(
            toasts.pop_newest().as_ref().map(Toast::title),
            Some("identified")
        );
        assert_eq!(
            toasts.pop_newest().as_ref().map(Toast::title),
            Some("anonymous")
        );
        assert_eq!(toasts.pop_newest(), None);
    }
}
