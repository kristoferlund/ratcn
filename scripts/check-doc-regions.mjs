// Every `<<< path#region` in the docs must point at a region that exists.
//
// VitePress fails the build for a missing *file*, but a missing or renamed
// *region* silently falls back to including the whole file — the page still
// builds, and quietly shows a few hundred lines of source. This runs before
// vitepress and fails loudly instead.
//
// No dependencies: node's standard library only.

import { readdirSync, readFileSync, statSync } from 'node:fs'
import { dirname, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const docsDir = join(root, 'docs')
const skip = new Set(['node_modules', 'dist', 'cache', 'public'])

/** Every markdown file under `docs/`, ignoring build output. */
function markdownFiles(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name)
    if (entry.isDirectory()) return skip.has(entry.name) ? [] : markdownFiles(path)
    return entry.name.endsWith('.md') ? [path] : []
  })
}

/**
 * The file path and region name out of a snippet line's argument, which may
 * carry `#region`, a `{line-range or lang}` block, and a `[title]`.
 */
function parseSnippet(argument) {
  const withoutTitle = argument.replace(/\[[^\]]*\]\s*$/, '')
  const withoutMeta = withoutTitle.replace(/\{[^}]*\}\s*$/, '')
  const [path, region] = withoutMeta.trim().split('#')
  return { path, region }
}

/**
 * Matches the region markers VitePress recognises, in a comment of any flavor.
 * The leading `[^a-zA-Z]` guard is what keeps `endregion` from reading as a
 * `region` marker.
 */
function marker(line, keyword) {
  const pattern = `(?:^|[^a-zA-Z])#?${keyword}\\b\\s*(.*?)\\s*(?:-->|\\*/|\\*\\))?$`
  return new RegExp(pattern).exec(line)?.[1]
}

const problems = []
let checked = 0

for (const file of markdownFiles(docsDir)) {
  const lines = readFileSync(file, 'utf8').split('\n')
  lines.forEach((line, index) => {
    if (!line.startsWith('<<<')) return
    const where = `${relative(root, file)}:${index + 1}`
    const { path, region } = parseSnippet(line.slice(3))
    if (!path) {
      problems.push(`${where}: snippet has no path`)
      return
    }
    const target = path.startsWith('@')
      ? join(docsDir, path.replace(/^@\/?/, ''))
      : resolve(dirname(file), path)
    let source
    try {
      if (!statSync(target).isFile()) throw new Error('not a file')
      source = readFileSync(target, 'utf8').split('\n')
    } catch {
      problems.push(`${where}: no such file: ${relative(root, target)}`)
      return
    }
    checked += 1
    if (!region) return
    const named = (keyword) => source.some((line) => marker(line, keyword) === region)
    if (!named('region')) {
      problems.push(
        `${where}: ${relative(root, target)} has no region "${region}" ` +
          `(VitePress would silently include the whole file)`
      )
    } else if (!named('endregion')) {
      problems.push(`${where}: ${relative(root, target)} never closes region "${region}"`)
    }
  })
}

if (problems.length > 0) {
  console.error(`doc snippets: ${problems.length} problem(s)`)
  for (const problem of problems) console.error(`  ${problem}`)
  process.exit(1)
}

console.log(`doc snippets: ${checked} resolve`)
