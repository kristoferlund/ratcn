#[cfg(target_arch = "wasm32")]
use std::io;
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
use ratatui::crossterm::event::{
    Event as CrosstermEvent, KeyCode as CrosstermKeyCode, KeyEventKind, KeyModifiers,
};

#[cfg(not(target_arch = "wasm32"))]
use std::{sync::OnceLock, time::Instant};

/// A `WebGl2Backend` whose canvas padding matches the demo's theme background.
///
/// The terminal grid covers a whole number of cells, so the canvas is usually a
/// few pixels wider and taller than the grid. That leftover strip is painted
/// with the *canvas padding color*, which defaults to black — visible as dark
/// bars along the right and bottom edges once the demos stopped overriding the
/// theme background to pure black.
#[cfg(target_arch = "wasm32")]
pub fn web_backend(
    background: ratzilla::ratatui::style::Color,
) -> Result<ratzilla::WebGl2Backend, io::Error> {
    ratzilla::backend::webgl2::WebGl2Backend::new_with_options(
        ratzilla::backend::webgl2::WebGl2BackendOptions::new().canvas_padding_color(background),
    )
    .map_err(|error| io::Error::other(error.to_string()))
}

/// Browser paste listener, installed for the guard's lifetime.
#[cfg(target_arch = "wasm32")]
mod browser_paste {
    use std::io;

    use ratzilla::web_sys::{
        ClipboardEvent, Document,
        wasm_bindgen::{JsCast, prelude::Closure},
    };

    #[must_use = "dropping the listener removes its browser paste handler"]
    pub struct BrowserPasteListener {
        document: Document,
        callback: Closure<dyn FnMut(ClipboardEvent)>,
    }

    impl BrowserPasteListener {
        pub fn install(on_paste: impl FnMut(String) -> bool + 'static) -> io::Result<Self> {
            let document = ratzilla::web_sys::window()
                .and_then(|window| window.document())
                .ok_or_else(|| io::Error::other("no document"))?;
            let mut on_paste = on_paste;
            let callback = Closure::new(move |event: ClipboardEvent| {
                let Some(data) = event.clipboard_data() else {
                    return;
                };
                let Ok(text) = data.get_data("text/plain") else {
                    return;
                };
                if on_paste(text) {
                    event.prevent_default();
                }
            });
            document
                .add_event_listener_with_callback("paste", callback.as_ref().unchecked_ref())
                .map_err(|error| io::Error::other(format!("paste listener: {error:?}")))?;
            Ok(Self { document, callback })
        }
    }

    impl Drop for BrowserPasteListener {
        fn drop(&mut self) {
            let _ = self.document.remove_event_listener_with_callback(
                "paste",
                self.callback.as_ref().unchecked_ref(),
            );
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use browser_paste::BrowserPasteListener;

#[cfg(not(target_arch = "wasm32"))]
pub fn is_quit(event: &CrosstermEvent) -> bool {
    matches!(
        event,
        CrosstermEvent::Key(key)
            if key.kind == KeyEventKind::Press
                && (key.code == CrosstermKeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL))
    )
}

#[must_use]
pub fn monotonic_time() -> Duration {
    #[cfg(target_arch = "wasm32")]
    {
        let millis = ratzilla::web_sys::window()
            .and_then(|window| window.performance())
            .map_or(0.0, |performance| performance.now())
            .max(0.0);
        Duration::from_secs_f64(millis / 1_000.0)
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        static START: OnceLock<Instant> = OnceLock::new();
        START.get_or_init(Instant::now).elapsed()
    }
}
