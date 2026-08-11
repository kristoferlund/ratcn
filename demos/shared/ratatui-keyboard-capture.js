// Keyboard capture for the WebAssembly demos.
//
// ratzilla delivers key events through a listener on its own canvas element
// (since 0.3.1 — it used to listen on `document`), and browsers only send
// keydown to the focused element. This script makes the whole page act as the
// keyboard surface instead: while focus is anywhere in the page, keys the demo
// understands are intercepted before the browser acts on them (Tab must move
// terminal focus, not leave the iframe) and re-dispatched onto the canvas.
//
// Forwarding also normalizes Alt chords by physical key: macOS composes
// Option+letter into a symbol before the page sees it (Option+D arrives as
// "∂", Option+1 as "¡"), so matching on the produced character would never
// fire. The physical key survives only in `event.code`.
//
// `data-keyboard-active` on the root drives the visible capture state — the
// hint pill and the thin border marking that the demo holds the keyboard.

const CAPTURED_KEYS = new Set([
  "Tab",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "Enter",
  "Escape",
  "Backspace",
  "Delete",
  "Home",
  "End",
  "PageUp",
  "PageDown",
]);

function shouldLetBrowserHandle(event) {
  if (event.key === "Tab" && event.ctrlKey) return true;
  if (event.metaKey) return true;
  if (event.ctrlKey && event.key.toLowerCase() === "f") return true;

  return false;
}

function shouldCapture(event) {
  if (event.key.length === 1) return true;

  return CAPTURED_KEYS.has(event.key);
}

// Letters always normalize: their Option compositions (∂, ƒ, ©) are not
// realistic text input. Digits stop at 6 — the landing tile hotkeys — because
// Option+7/8/9 produce | [ ] on Nordic layouts, which are.
function forwardedKey(event) {
  if (event.altKey) {
    const letter = event.code.match(/^Key([A-Z])$/);
    if (letter) return letter[1].toLowerCase();
    const digit = event.code.match(/^(?:Digit|Numpad)([1-6])$/);
    if (digit) return digit[1];
  }

  return event.key;
}

function installKeyboardCapture(root = document.body) {
  if (!root) return;

  const forwardedEvents = new WeakSet();

  root.tabIndex = root.tabIndex >= 0 ? root.tabIndex : 0;
  root.classList.add("ratatui-demo-root");
  root.dataset.keyboardActive = "false";

  const hint = document.createElement("div");
  hint.className = "keyboard-capture-hint";
  hint.textContent = "Keyboard captured - press Esc to release";
  root.appendChild(hint);

  // ratzilla's canvas exists once the wasm module has booted; look it up per
  // event rather than at install time.
  const target = () => root.querySelector("canvas") ?? root;

  // Focus anywhere in the page counts as capture: clicking the demo focuses
  // the canvas (ratzilla makes it focusable), tabbing into the iframe lands
  // on the body. `:focus` distinguishes a really-focused body from
  // `document.activeElement`'s default fallback (also the body).
  const isActive = () =>
    root.matches(":focus") || root.querySelector(":focus") !== null;

  const syncActive = () => {
    root.dataset.keyboardActive = String(isActive());
  };

  // focusin/focusout bubble (focus/blur don't). focusout fires before focus
  // has actually moved, so re-read the state a tick later.
  document.addEventListener("focusin", syncActive);
  document.addEventListener("focusout", () => window.setTimeout(syncActive, 0));

  root.addEventListener("pointerdown", () => root.focus());

  document.addEventListener(
    "keydown",
    (event) => {
      if (forwardedEvents.has(event)) return;
      if (!isActive()) return;
      if (shouldLetBrowserHandle(event)) return;
      if (!shouldCapture(event)) return;

      // Stop the original here in the capture phase — before it reaches the
      // canvas — so ratzilla only ever sees the normalized clone.
      event.preventDefault();
      event.stopPropagation();
      event.stopImmediatePropagation();

      const forwardedEvent = new KeyboardEvent("keydown", {
        key: forwardedKey(event),
        code: event.code,
        ctrlKey: event.ctrlKey,
        altKey: event.altKey,
        shiftKey: event.shiftKey,
        metaKey: event.metaKey,
        repeat: event.repeat,
        bubbles: true,
      });
      forwardedEvents.add(forwardedEvent);
      target().dispatchEvent(forwardedEvent);

      if (event.key === "Escape") {
        window.setTimeout(() => releaseFocus(root), 0);
      }
    },
    { capture: true },
  );
}

function releaseFocus(root) {
  const active = document.activeElement;
  if (active instanceof HTMLElement && root.contains(active)) {
    active.blur();
  }
  root.dataset.keyboardActive = "false";
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", () => installKeyboardCapture());
} else {
  installKeyboardCapture();
}
