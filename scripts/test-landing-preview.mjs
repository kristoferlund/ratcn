// Build and serve the docs first, then: node scripts/test-landing-preview.mjs URL
// Uses agent-browser through npx; AGENT_BROWSER_EXECUTABLE_PATH selects Chromium.
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'

const url = process.argv[2] ?? 'http://localhost:4173'
const session = `landing-preview-test-${process.pid}`
function browser(...args) {
  const output = execFileSync('npx', ['--yes', 'agent-browser', '--session', session, '--json', ...args], {
    encoding: 'utf8', timeout: 60000,
  })
  const response = JSON.parse(output)
  assert.ok(response.success, response.error)
  return response.data
}
const evaluate = code => browser('eval', `(() => { ${code} })()`).result

function checkGrid(columns) {
  browser('wait', '--fn', `(() => {
    const f = document.querySelector('iframe')
    const rows = [...(f?.contentDocument?.querySelectorAll('#grid > pre') ?? [])]
    const cellWidth = rows[0]?.children[0]?.getBoundingClientRect().width
    return rows.some(r => r.textContent.includes('alt+8')) && rows.length === ${columns === 1 ? 181 : columns === 2 ? 93 : 71}
      && rows[0].children.length === Math.floor(f.clientWidth / cellWidth)
  })()`)
  const grid = evaluate(`
    const f = document.querySelector('iframe')
    const rows = [...f.contentDocument.querySelectorAll('#grid > pre')]
    const labels = rows.flatMap((row, y) => [...row.children].flatMap((cell, x) =>
      /^alt[+][1-8]/.test(row.textContent.slice(x)) ? [{ y, x, bottom: rows[y + 19]?.children[x - 2]?.textContent }] : []))
    return { labels, height: f.clientHeight, rowHeight: rows[0].getBoundingClientRect().height,
      canvas: !!f.contentDocument.querySelector('canvas') }
  `)
  assert.equal(grid.canvas, false, 'A tall HiDPI canvas exceeds mobile GPU limits')
  assert.equal(grid.labels.length, 8, 'Every tile must have its full top border')
  assert.equal(new Set(grid.labels.map(label => label.x)).size, columns)
  for (const label of grid.labels) {
    assert.equal(label.bottom, '└', 'Tiles must remain 20 rows high, not squeezed')
    assert.ok((label.y + 20) * grid.rowHeight <= grid.height, 'Last tile must fit inside the iframe')
  }
}

function clickText(text) {
  const point = evaluate(`
    const f = document.querySelector('iframe')
    const rows = [...f.contentDocument.querySelectorAll('#grid > pre')]
    const row = rows.find(r => r.textContent.includes(${JSON.stringify(text)}))
    const cell = row.children[row.textContent.indexOf(${JSON.stringify(text)})]
    const rect = cell.getBoundingClientRect()
    window.scrollTo(0, scrollY + f.getBoundingClientRect().top + rect.top - 150)
    const frame = f.getBoundingClientRect()
    return { x: frame.left + rect.left + rect.width / 2, y: frame.top + rect.top + rect.height / 2, scroll: scrollY }
  `)
  browser('mouse', 'move', String(Math.round(point.x)), String(Math.round(point.y)))
  browser('mouse', 'down', 'left')
  browser('mouse', 'up', 'left')
  assert.equal(evaluate('return scrollY'), point.scroll, 'Focusing must not scroll the target away between down and up')
}

function checkClicks() {
  clickText('Catppuccin')
  browser('wait', '--fn', `(() => {
    const rows = document.querySelector('iframe').contentDocument.querySelectorAll('#grid > pre')
    return [...rows].some(row => row.textContent.includes('● Catppuccin'))
  })()`)
  // A bottom-tile select catches offsets that accumulate down the tall preview.
  clickText('USD -')
  browser('wait', '--fn', `document.querySelector('iframe').contentDocument.querySelector('#grid').textContent.includes('EUR - Euro')`)
  browser('press', 'Escape')
}

try {
  browser('set', 'device', 'Pixel 7')
  // First visit, before any cache-warming reload. The demo must boot at its
  // final size even if the parent documentation app has not started yet.
  browser('network', 'route', '**/assets/app.*.js', '--abort')
  browser('open', url)
  checkGrid(1)
  checkClicks()
  console.log('PASS: uncached first visit, without parent JavaScript sizing')
  browser('network', 'unroute', '**/assets/app.*.js')
  browser('reload')
  checkGrid(1)
  checkClicks()
  clickText('Re:')
  browser('press', 'Alt+2')
  browser('press', 'x')
  browser('wait', '--fn', `document.querySelector('iframe').contentDocument.querySelector('#grid').textContent.includes('│x ')`)
  console.log('PASS: Android portrait, full tiles, first/last-tile clicks and keyboard input')

  browser('set', 'viewport', '915', '412', '2.625')
  checkGrid(2)
  checkClicks()
  console.log('PASS: rotate to landscape, full tiles and clicks')

  browser('reload')
  checkGrid(2)
  checkClicks()
  console.log('PASS: cold landscape load')

  browser('set', 'viewport', '393', '851', '3')
  checkGrid(1)
  checkClicks()
  browser('set', 'viewport', '375', '812', '3')
  checkGrid(1)
  checkClicks()
  console.log('PASS: 3x DPR and same-column width changes')

  browser('set', 'viewport', '1440', '900', '1')
  checkGrid(3)
  checkClicks()
  console.log('PASS: desktop, full tiles and clicks')

  browser('click', '.ratcn-header-nav a[href="/docs/introduction"]')
  browser('wait', '--url', '**/docs/introduction')
  browser('back')
  checkGrid(3)
  checkClicks()
  console.log('PASS: navigate away and back without stale sizing/listeners')
} finally {
  browser('close')
}
