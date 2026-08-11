//! The transport half of [`Effect::FetchJoke`](crate::Effect): fetch a joke in
//! the background and report the outcome through the channel as a
//! [`Msg::JokeFetchCompleted`](crate::Msg). Nothing here touches app state.

use std::{sync::mpsc::Sender, time::Duration};

use serde::Deserialize;

use crate::Msg;

const API_URL: &str = "https://icanhazdadjoke.com/";
#[cfg(not(target_arch = "wasm32"))]
const USER_AGENT: &str = "ratcn effects demo (https://github.com/kristoferlund/ratcn)";
// The API often answers in well under a second; hold the loading state for at
// least this long so the "Fetching..." feedback is visible rather than a flicker.
const LOADING_DELAY: Duration = Duration::from_secs(1);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
struct JokeResponse {
    joke: String,
}

/// Native: fetch on a worker thread so the event loop keeps running. The
/// thread pads fast responses up to `LOADING_DELAY`, then reports through the
/// channel; sleeping here blocks only the worker, never the UI.
#[cfg(not(target_arch = "wasm32"))]
pub fn fetch_joke(sender: Sender<Msg>) {
    let worker_sender = sender.clone();
    let worker = std::thread::Builder::new()
        .name("dad-joke".to_owned())
        .spawn(move || {
            let started = std::time::Instant::now();
            let result = fetch_joke_native();
            std::thread::sleep(LOADING_DELAY.saturating_sub(started.elapsed()));
            let _ = worker_sender.send(Msg::JokeFetchCompleted(result));
        });
    if let Err(error) = worker {
        let _ = sender.send(Msg::JokeFetchCompleted(Err(format!(
            "could not start joke request: {error}"
        ))));
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_joke_native() -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent
        .get(API_URL)
        .header("Accept", "application/json")
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|error| format!("request failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("server returned HTTP {}", response.status()));
    }
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|error| format!("could not read response: {error}"))?;
    parse_joke(&text)
}

/// Wasm: no threads in the browser, so spawn a future instead. Joining the
/// fetch with a timer enforces the same minimum loading time as the native
/// worker's sleep.
#[cfg(target_arch = "wasm32")]
pub fn fetch_joke(sender: Sender<Msg>) {
    use futures_util::future::join;
    use gloo_timers::future::TimeoutFuture;

    wasm_bindgen_futures::spawn_local(async move {
        let (result, ()) = join(
            fetch_joke_browser(),
            TimeoutFuture::new(LOADING_DELAY.as_millis() as u32),
        )
        .await;
        let _ = sender.send(Msg::JokeFetchCompleted(result));
    });
}

#[cfg(target_arch = "wasm32")]
async fn fetch_joke_browser() -> Result<String, String> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    // The browser enforces the timeout: an expired signal rejects the fetch,
    // and the error surfaces through `browser_error` like any other failure.
    let timeout = web_sys::AbortSignal::timeout_with_u32(REQUEST_TIMEOUT.as_millis() as u32);
    let init = web_sys::RequestInit::new();
    init.set_method("GET");
    init.set_signal(Some(&timeout));
    let request = web_sys::Request::new_with_str_and_init(API_URL, &init).map_err(browser_error)?;
    request
        .headers()
        .set("Accept", "application/json")
        .map_err(browser_error)?;
    let window = web_sys::window().ok_or_else(|| "no browser window".to_owned())?;
    let response = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(browser_error)?
        .dyn_into::<web_sys::Response>()
        .map_err(browser_error)?;
    if !response.ok() {
        return Err(format!("server returned HTTP {}", response.status()));
    }
    let text = JsFuture::from(response.text().map_err(browser_error)?)
        .await
        .map_err(browser_error)?
        .as_string()
        .ok_or_else(|| "server returned non-text data".to_owned())?;
    parse_joke(&text)
}

#[cfg(target_arch = "wasm32")]
fn browser_error(error: wasm_bindgen::JsValue) -> String {
    error
        .as_string()
        .unwrap_or_else(|| format!("browser request failed: {error:?}"))
}

fn parse_joke(text: &str) -> Result<String, String> {
    let response: JokeResponse =
        serde_json::from_str(text).map_err(|error| format!("invalid JSON response: {error}"))?;
    if response.joke.trim().is_empty() {
        return Err("API returned an empty joke".to_owned());
    }
    Ok(response.joke)
}
