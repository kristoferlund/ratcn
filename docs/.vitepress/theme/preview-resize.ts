// Sizes the landing WebAssembly preview iframe to one of three fixed layouts.
//
// The landing demo (demos/landing/) runs on ratzilla's WebGl2 (canvas) backend,
// which picks its tile column count from the iframe's rendered width using the
// `column_count` math in demos/landing/src/main.rs:
//
//   wide screen   -> 3 columns x 2 rows
//   medium screen -> 2 columns x 3 rows
//   narrow screen -> 1 column  x 6 rows
//
// We mirror that math here to choose the matching iframe HEIGHT, so every tile
// fits at full height with no centering gap and no clipping. The demo sizes its
// canvas exactly once, at wasm boot: its host does redraw on a window resize,
// but beamterm pins the canvas CSS to the pixel size it booted at, so the
// flush-time check ratzilla resizes on never sees the canvas change. Whenever
// the right height differs from the height the demo booted at, the iframe must
// be reloaded to re-render the WebAssembly at the new size.

const TILE_COUNT = 6 // main.rs: TILE_COUNT
const TILE_WIDTH = 42 // main.rs: TILE_WIDTH (cells)
const TILE_HEIGHT = 20 // main.rs: TILE_HEIGHT (cells)
const TILE_GAP = 2 // main.rs: TILE_GAP (cells)

// beamterm's embedded-atlas cell size, in CSS px. The backend sets the canvas
// buffer to the CSS size (devicePixelRatio aside), so a cell is a fixed pixel
// box; these are measured from the rendered grid (~8.9 x 15.8). Both are rounded
// *up*: a slightly wide cell makes our column flip land a hair after the demo's,
// so at a breakpoint we pick the taller layout (a little margin) rather than the
// shorter one (a clipped tile); a slightly tall cell keeps the height generous.
const CELL_W = 9
const CELL_H = 16

// Mirrors `column_count` in main.rs.
function columnCount(cols: number): number {
  const columns = Math.floor((cols + TILE_GAP) / (TILE_WIDTH + TILE_GAP))
  return Math.min(Math.max(columns, 1), 4)
}

// Fixed iframe height (px) for a grid `columns` wide: enough rows to show all six
// tiles at full height, plus one cell of slack so rounding never clips a tile.
// The landing page gives the preview 20% extra vertical room around the padded
// grid so it breathes in the surrounding page layout; the 2- and 3-column
// layouts get another 10% — at 1.2 the demo comes up a couple of terminal rows
// short there and squeezes the tiles below their full height.
function heightFor(columns: number): number {
  const rows = Math.ceil(TILE_COUNT / columns)
  const cells = rows * TILE_HEIGHT + (rows - 1) * TILE_GAP
  const slack = columns >= 2 ? 1.2 * 1.1 : 1.2
  return Math.ceil((cells + 1) * CELL_H * slack)
}

// Sizes `iframe` now and keeps it sized. Returns a disposer that takes the
// observer, the listener, and any pending debounce back off again.
function wire(iframe: HTMLIFrameElement): () => void {
  // The column count currently applied; 0 until the first measurement.
  let columns = 0

  const apply = () => {
    const width = iframe.clientWidth
    if (!width) return
    const next = columnCount(Math.floor(width / CELL_W))
    // Within a layout the height is fixed — nothing to do until the column
    // count actually changes.
    if (next === columns) return
    columns = next
    const height = heightFor(next)
    const changed = Math.round(iframe.getBoundingClientRect().height) !== height
    iframe.style.height = `${height}px`
    // If the demo already booted, it laid its canvas out against the old
    // height and will not adapt (see above) — re-boot it at the corrected
    // one. This covers both crossing a breakpoint and a hard reload, where
    // the statically-loaded iframe boots before this code runs (soft
    // navigation sizes the iframe before its lazy load starts). A missing
    // canvas means the demo has not measured anything yet: the measurement
    // and canvas creation are one synchronous wasm-init step, so setting the
    // height first is race-free.
    if (changed && iframe.contentDocument?.querySelector('canvas')) {
      iframe.contentWindow?.location.reload()
    }
  }

  // Debounce so dragging the window across a breakpoint reloads once, not per frame.
  let timer = 0
  const onResize = () => {
    clearTimeout(timer)
    timer = window.setTimeout(apply, 150)
  }

  apply()
  // Observing the iframe catches container resizes too; a height change we make
  // re-fires it, but `apply` no-ops when the column count is unchanged.
  const observer = new ResizeObserver(onResize)
  observer.observe(iframe)
  window.addEventListener('resize', onResize)

  return () => {
    clearTimeout(timer)
    observer.disconnect()
    window.removeEventListener('resize', onResize)
  }
}

// Wires the landing preview iframe, if the page in the DOM has one. The caller
// owns the returned disposer and must run it before wiring again: one wiring is
// live at a time, so navigating between the home page and the docs cannot
// strand an observer, a listener, or a timer on a page that is gone.
export function initPreviewAutoSize(): () => void {
  const iframe = document.querySelector<HTMLIFrameElement>('.ratcn-landing-preview-frame')
  return iframe ? wire(iframe) : () => {}
}
