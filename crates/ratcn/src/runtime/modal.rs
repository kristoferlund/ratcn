use std::{error::Error, fmt};

use super::{ChildId, FocusState};

/// Which modals are open, innermost last, plus the focus to restore on close.
///
/// A modal is a layer that takes over input while it is open — a dialog, a
/// confirmation prompt. They stack, because one modal can open another, so this
/// holds ids in opening order rather than a single "current modal". It lives in
/// app state like focus and hover: [`open`](Self::open) and
/// [`close`](Self::close) give you the stack and the focus save/restore, but
/// deciding *when* a modal opens stays yours.
///
/// Opening a modal also stashes the focus path that was current at the time, so
/// closing it can put focus back exactly where the user left it rather than
/// resetting to the first control.
///
/// # Why bind it to the runtime
///
/// Declaring a modal with [`DeclareCtx::modal`](super::DeclareCtx::modal) is
/// enough to draw and route one. Binding this state with
/// [`Ratcn::modals`](super::Ratcn::modals) additionally closes a timing gap:
/// between the message that opens or closes a modal and the redraw that
/// declares it, the retained surface still describes the *old* layer. With the
/// binding in place the runtime notices that mismatch and swallows events until
/// the two agree, so a keypress can't land on a dialog the app already
/// considers closed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModalState {
    /// One entry per open modal, innermost last: its id and the focus that was
    /// current when it opened.
    open: Vec<OpenModal>,
}

/// One open modal: its id, and the focus to restore when it closes.
#[derive(Debug, Clone, PartialEq)]
struct OpenModal {
    id: ChildId,
    return_focus: FocusState,
}

impl ModalState {
    /// Push `id` onto the stack and move focus into it.
    ///
    /// `focus` is your app's focus state, taken by `&mut` because this does two
    /// things with it: it copies the current path into this modal's saved
    /// history, then clears it, so the newly opened layer resolves focus to
    /// its own first focusable leaf. [`close`](Self::close) reverses both.
    ///
    /// Re-opening whatever is already on top is a no-op — it will not overwrite
    /// the focus that modal saved when it first opened.
    ///
    /// # Errors
    ///
    /// Returns [`ModalOpenError`] if `id` is already open *below* another
    /// modal: one id holds one place in the stack, with one saved focus
    /// snapshot.
    pub fn open(
        &mut self,
        id: impl Into<ChildId>,
        focus: &mut FocusState,
    ) -> Result<(), ModalOpenError> {
        let id = id.into();
        if self.top() == Some(&id) {
            return Ok(());
        }
        if self.is_open(&id) {
            return Err(ModalOpenError { id });
        }

        self.open.push(OpenModal {
            return_focus: std::mem::take(focus),
            id,
        });
        Ok(())
    }

    /// Pop the top modal and write its saved focus snapshot back into `focus`.
    ///
    /// The restored path is the exact one that was current when that modal
    /// opened, including a path whose component is not declared — focus parks
    /// there.
    ///
    /// Returns the closed modal id, or `None` when the stack is already empty,
    /// in which case `focus` is left alone.
    pub fn close(&mut self, focus: &mut FocusState) -> Option<ChildId> {
        let closed = self.open.pop()?;
        *focus = closed.return_focus;
        Some(closed.id)
    }

    /// Whether `id` is present anywhere in the modal stack.
    #[must_use]
    pub fn is_open(&self, id: impl AsRef<str>) -> bool {
        let id = id.as_ref();
        self.open.iter().any(|open| open.id.as_str() == id)
    }

    /// The top modal id, if any.
    #[must_use]
    pub fn top(&self) -> Option<&ChildId> {
        Some(&self.open.last()?.id)
    }

    /// Modal ids in declaration order, from the lowest layer to the top.
    #[must_use]
    pub fn ids(&self) -> impl ExactSizeIterator<Item = &ChildId> + Clone {
        self.open.iter().map(|open| &open.id)
    }
}

/// Error returned when opening a modal that is already below the stack top.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModalOpenError {
    id: ChildId,
}

impl ModalOpenError {
    /// The duplicate modal id.
    #[must_use]
    pub fn id(&self) -> &ChildId {
        &self.id
    }
}

impl fmt::Display for ModalOpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "modal `{}` is already open below the top of the stack",
            self.id
        )
    }
}

impl Error for ModalOpenError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_and_close_restore_the_exact_focus_snapshot() {
        let original = FocusState::intent([
            ChildId::Static("pane"),
            ChildId::Static("temporarily-absent"),
        ]);
        let mut focus = original.clone();
        let mut modals = ModalState::default();

        modals
            .open(ChildId::Static("dialog"), &mut focus)
            .expect("open dialog");
        assert_eq!(focus, FocusState::default());
        assert_eq!(modals.close(&mut focus), Some(ChildId::Static("dialog")));
        assert_eq!(focus, original);
    }

    #[test]
    fn nested_modals_restore_each_stack_edge() {
        let base = FocusState::intent([ChildId::Static("base")]);
        let lower = FocusState::intent([ChildId::Static("lower"), ChildId::Static("field")]);
        let mut focus = base.clone();
        let mut modals = ModalState::default();

        modals.open("lower", &mut focus).expect("open lower");
        focus = lower.clone();
        modals.open("top", &mut focus).expect("open top");
        assert_eq!(
            modals.ids().collect::<Vec<_>>(),
            [&ChildId::Static("lower"), &ChildId::Static("top")]
        );

        assert_eq!(modals.close(&mut focus), Some(ChildId::Static("top")));
        assert_eq!(focus, lower);
        assert_eq!(modals.close(&mut focus), Some(ChildId::Static("lower")));
        assert_eq!(focus, base);
    }

    #[test]
    fn same_top_is_idempotent_but_duplicate_nesting_is_rejected() {
        let base = FocusState::intent([ChildId::Static("base")]);
        let mut focus = base.clone();
        let mut modals = ModalState::default();

        modals.open("lower", &mut focus).expect("open lower");
        focus = FocusState::intent([ChildId::Static("lower"), ChildId::Static("field")]);
        modals.open("lower", &mut focus).expect("same top is valid");
        assert_eq!(
            modals.ids().collect::<Vec<_>>(),
            [&ChildId::Static("lower")]
        );
        modals.open("top", &mut focus).expect("open top");

        let error = modals
            .open("lower", &mut focus)
            .expect_err("duplicate nesting");
        assert_eq!(error.id(), &ChildId::Static("lower"));
        assert_eq!(
            modals.ids().collect::<Vec<_>>(),
            [&ChildId::Static("lower"), &ChildId::Static("top")]
        );

        let _ = modals.close(&mut focus);
        let _ = modals.close(&mut focus);
        assert_eq!(focus, base, "same-top open must not overwrite return focus");
    }
}
