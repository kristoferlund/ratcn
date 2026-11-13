// Keyboard capture for the WebAssembly demos.
//
// ratzilla delivers key events through a listener on its own canvas/grid element
// (since 0.3.1 — it used to listen on `document`), and browsers only send
// keydown to the focused element. This script makes the whole page act as the
// keyboard surface instead: while focus is anywhere in the page, keys the demo
// understands are intercepted before the browser acts on them (Tab must move
// terminal focus, not leave the iframe) and re-dispatched onto the canvas.
//
// Forwarding keeps what the key typed. A character composed with Option
// (macOS) or AltGr (Ctrl+Alt on Windows), `@` from Option+2 on a Swedish Mac
// or `€` from AltGr+E, is forwarded without those modifiers, so a text field
// types it. Only a page's own Alt hotkeys go by physical key, named on the
// root as `data-alt-hotkeys` (say "1 2 d"): macOS composes Option+D into "∂",
// so matching on the produced character would never fire. Text wins over a
// hotkey: Option+2 typing `@` stays `@`.
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

// Told the way the clipboard listener in ratcn tells it.
const MAC = navigator.platform.startsWith("Mac");

function shouldLetBrowserHandle(event) {
  if (event.key === "Tab" && event.ctrlKey) return true;
  if (event.metaKey) return true;
  if (event.ctrlKey && event.key.toLowerCase() === "f") return true;

  return false;
}

// One character, counted as Rust does: `key.length` counts UTF-16 units.
function isCharacter(key) {
  return [...key].length === 1;
}

function shouldCapture(event) {
  if (isCharacter(event.key)) return true;

  return CAPTURED_KEYS.has(event.key);
}

// The character a letter or digit key types unmodified. Other keys differ
// by layout, so they have none here.
function physicalKey(event) {
  const letter = event.code.match(/^Key([A-Z])$/);
  if (letter) return letter[1].toLowerCase();
  const digit = event.code.match(/^(?:Digit|Numpad)([0-9])$/);
  if (digit && !event.shiftKey) return digit[1];
  return null;
}

// Whether Option or AltGr composed the character rather than chording it.
// Off a Mac, Alt alone is always a chord, whatever the layout types (Alt+B
// on Dvorak, Alt+F on a Russian layout): AltGr reports as Ctrl+Alt on
// Windows and as neither on Linux. An ASCII letter or digit is a chord on
// any layout, Ctrl+Alt+B say.
function isComposed(event) {
  if (!event.altKey || !isCharacter(event.key)) return false;
  if (!MAC && !event.ctrlKey) return false;
  return !/^[A-Za-z0-9]$/.test(event.key);
}

// ASCII punctuation, which a field needs typed: `@` from Option+2 and `$`
// from Option+4 on a Swedish Mac.
function isPunctuation(key) {
  return /^[\x21-\x2f\x3a-\x40\x5b-\x60\x7b-\x7e]$/.test(key);
}

// What to forward: a hotkey by physical key, unless the key typed
// punctuation; a composed character as plain text; anything else as it came.
function forwarded(event, hotkeys) {
  const physical = physicalKey(event);
  if (event.altKey && !event.ctrlKey && hotkeys.has(physical) && !isPunctuation(event.key)) {
    return { key: physical, ctrlKey: false, altKey: true };
  }
  if (isComposed(event)) {
    return { key: event.key, ctrlKey: false, altKey: false };
  }
  return { key: event.key, ctrlKey: event.ctrlKey, altKey: event.altKey };
}

function installKeyboardCapture(root = document.body) {
  if (!root) return;

  const forwardedEvents = new WeakSet();
  const hotkeys = new Set((root.dataset.altHotkeys ?? "").split(/\s+/).filter(Boolean));

  root.tabIndex = root.tabIndex >= 0 ? root.tabIndex : 0;
  root.classList.add("ratatui-demo-root");
  root.dataset.keyboardActive = "false";

  const hint = document.createElement("div");
  hint.className = "keyboard-capture-hint";
  hint.textContent = "Keyboard captured - press Esc to release";
  root.appendChild(hint);

  // ratzilla's canvas/grid exists once the wasm module has booted; look it up per
  // event rather than at install time.
  const target = () => root.querySelector("canvas, #grid") ?? root;

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

  root.addEventListener("pointerdown", () => root.focus({ preventScroll: true }));
  // The browser's default mousedown focus would scroll a tall grid/canvas to
  // its top between down and up, moving the control away from the pointer.
  root.addEventListener("mousedown", (event) => {
    event.preventDefault();
    root.focus({ preventScroll: true });
  });

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
        ...forwarded(event, hotkeys),
        code: event.code,
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
