//! Smoke tests over the crate's public surface, compiled as a downstream user would see it.

#[cfg(any(feature = "crossterm", feature = "ratzilla"))]
use ratcn::runtime::Event;
use ratcn::{Button, Theme, Toast, ToasterState};

#[test]
fn documented_root_imports_are_available_to_external_crates() {
    let _ = Button::<()>::new("Save");
    let _ = Theme::default_dark();
    let _ = Toast::new("Saved");
    let _ = ToasterState::default();
}

#[cfg(any(feature = "crossterm", feature = "ratzilla"))]
fn assert_backend_event<T>()
where
    T: TryInto<Event>,
{
}

#[cfg(feature = "crossterm")]
#[test]
fn crossterm_feature_exposes_host_helpers_and_event_conversion() {
    use ratcn::crossterm::{InputModeGuard, InputModes};

    let _ = InputModes::new().mouse_capture().bracketed_paste();
    let _: Option<InputModeGuard> = None;
    assert_backend_event::<ratatui::crossterm::event::Event>();
}

#[cfg(feature = "ratzilla")]
#[test]
fn ratzilla_feature_exposes_event_conversion() {
    assert_backend_event::<ratzilla::event::KeyEvent>();
    assert_backend_event::<ratzilla::event::MouseEvent>();
}
