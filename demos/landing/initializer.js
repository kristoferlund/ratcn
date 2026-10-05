// Trunk imports this before starting wasm. Await here: its onStart hook is not
// awaited, and measuring a fallback font would misalign the grid and pointer.
await document.fonts.load('14px "JetBrainsMono Nerd Font Mono"');

// Size the embed before DomBackend measures it or installs resize listeners.
// A corrective resize after its first paint can replace the grid underneath
// those listeners and leave a partly repainted, squeezed preview on cold loads.
const iframe = window.frameElement;
if (iframe) {
  // The same probe as DomBackend::measure_cell_size, using the loaded font.
  const pre = document.createElement("pre");
  pre.style.cssText = "margin: 0; padding: 0; border: 0; line-height: normal;";
  const span = document.createElement("span");
  span.textContent = "█";
  span.style.cssText = "display: inline-block; width: 1ch;";
  pre.appendChild(span);
  document.body.appendChild(pre);
  const cell = span.getBoundingClientRect();
  pre.remove();

  // Mirrors column_count/grid_height in demos/landing/src/lib.rs.
  const cols = Math.floor(document.body.getBoundingClientRect().width / cell.width);
  const columns = Math.min(Math.max(Math.floor((cols + 2) / (42 + 2)), 1), 4);
  const rows = Math.ceil(8 / columns);
  const cells = 2 * 3 + rows * 20 + (rows - 1) * 2;
  iframe.style.height = `${Math.ceil((cells + 1) * cell.height)}px`;

  // Flush layout and let its resize event settle before wasm registers listeners.
  document.body.getBoundingClientRect();
  await new Promise(requestAnimationFrame);
}
export default () => ({});
