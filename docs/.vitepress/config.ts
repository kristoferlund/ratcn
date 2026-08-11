import { writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { defineConfig, type HeadConfig } from 'vitepress'

const siteUrl = 'https://ratcn.kristoferlund.se'
const siteTitle = 'ratcn - themeable terminal UI components for Ratatui'
const siteDescription =
  'ratcn is a component library for Ratatui apps: beautifully designed terminal UI components that you can copy, paste, theme, and own in your application code.'
// Absolute on purpose: Open Graph and Twitter crawlers do not resolve relative
// image paths.
const shareImage = `${siteUrl}/og-image.png`

const repo = 'kristoferlund/ratcn'

/// The star count baked into the build, so the first paint carries a real
/// number instead of a placeholder. The landing page refreshes it on mount;
/// this only has to be right at deploy time.
///
/// Every failure path — offline builder, rate limit, changed payload — falls
/// back to a plausible number rather than failing the build. A docs build must
/// not depend on GitHub being reachable.
async function fetchStars(): Promise<number> {
  const fallback = 92
  try {
    const response = await fetch(`https://api.github.com/repos/${repo}`, {
      headers: { Accept: 'application/vnd.github+json' },
      signal: AbortSignal.timeout(5000)
    })
    if (!response.ok) return fallback
    const data = (await response.json()) as { stargazers_count?: unknown }
    return typeof data.stargazers_count === 'number' ? data.stargazers_count : fallback
  } catch {
    return fallback
  }
}

const stars = await fetchStars()

/// The public URL of a page, from its source path. Mirrors `cleanUrls`: the
/// `.md` extension goes, and an `index` segment collapses into its directory,
/// so the sitemap and the canonical tag agree with the links VitePress emits.
function pageUrl(relativePath: string): string {
  const clean = relativePath
    .replace(/\.md$/, '')
    .replace(/\/index$/, '')
    .replace(/^index$/, '')
  return clean ? `${siteUrl}/${clean}` : siteUrl
}

function softwareSourceCodeSchema(): string {
  return JSON.stringify({
    '@context': 'https://schema.org',
    '@type': 'SoftwareSourceCode',
    name: 'ratcn',
    description: siteDescription,
    url: siteUrl,
    codeRepository: 'https://github.com/kristoferlund/ratcn',
    programmingLanguage: 'Rust',
    runtimePlatform: 'Rust',
    license: 'https://github.com/kristoferlund/ratcn/blob/main/LICENSE',
    author: { '@type': 'Person', name: 'Kristofer Lund' }
  })
}

function breadcrumbSchema(title: string, url: string): string {
  return JSON.stringify({
    '@context': 'https://schema.org',
    '@type': 'BreadcrumbList',
    itemListElement: [
      { '@type': 'ListItem', position: 1, name: 'ratcn', item: siteUrl },
      { '@type': 'ListItem', position: 2, name: title, item: url }
    ]
  })
}

export default defineConfig({
  title: 'ratcn',
  description: siteDescription,
  lang: 'en-US',
  appearance: 'dark',
  // Serve /docs/introduction rather than /docs/introduction.html. The host
  // resolves the extensionless path; `public/_redirects` sends the old
  // `.html` URLs on with a 301 so nothing already indexed dead-ends.
  cleanUrls: true,
  transformHtml(code) {
    return code
      .replace('<html ', '<html class="dark" ')
      .replace(/<meta name="generator"[^>]*>\s*/g, '')
  },
  transformHead({ pageData }) {
    const head: HeadConfig[] = []
    const url = pageUrl(pageData.relativePath)
    const isHome = pageData.relativePath === 'index.md'
    const title = isHome ? siteTitle : `${pageData.title} — ratcn`
    const description =
      (pageData.frontmatter?.description as string | undefined) || siteDescription

    head.push(['link', { rel: 'canonical', href: url }])
    head.push(['meta', { property: 'og:url', content: url }])
    head.push(['meta', { property: 'og:title', content: title }])
    head.push(['meta', { property: 'og:description', content: description }])
    head.push(['meta', { name: 'twitter:title', content: title }])
    head.push(['meta', { name: 'twitter:description', content: description }])
    head.push([
      'script',
      { type: 'application/ld+json' },
      isHome ? softwareSourceCodeSchema() : breadcrumbSchema(pageData.title, url)
    ])

    return head
  },
  async buildEnd(siteConfig) {
    const urls = siteConfig.pages.map((page: string) => pageUrl(page)).sort()
    const sitemap = [
      '<?xml version="1.0" encoding="UTF-8"?>',
      '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">',
      ...urls.map((url: string) => `  <url><loc>${url}</loc></url>`),
      '</urlset>'
    ].join('\n')
    writeFileSync(join(siteConfig.outDir, 'sitemap.xml'), sitemap)
  },
  head: [
    ['link', { rel: 'icon', type: 'image/png', href: '/icon.png' }],
    ['link', { rel: 'apple-touch-icon', href: '/icon.png' }],
    ['meta', { name: 'theme-color', content: '#0a0a0a' }],
    ['meta', { property: 'og:type', content: 'website' }],
    ['meta', { property: 'og:site_name', content: 'ratcn' }],
    // og:title, og:description, og:url are injected per page in transformHead.
    ['meta', { property: 'og:image', content: shareImage }],
    ['meta', { property: 'og:image:width', content: '1200' }],
    ['meta', { property: 'og:image:height', content: '630' }],
    ['meta', { property: 'og:image:alt', content: siteTitle }],
    ['meta', { name: 'twitter:card', content: 'summary_large_image' }],
    ['meta', { name: 'twitter:site', content: '@kristoferlund' }],
    // twitter:title and twitter:description are injected per page.
    ['meta', { name: 'twitter:image', content: shareImage }],
    ['meta', { name: 'twitter:image:alt', content: siteTitle }],
    [
      'script',
      {
        defer: '',
        'data-domain': 'ratcn.kristoferlund.se',
        src: '/js/script.js'
      }
    ]
  ],
  vite: {
    define: {
      __GITHUB_REPO__: JSON.stringify(repo),
      __GITHUB_STARS__: JSON.stringify(stars)
    },
    build: {
      rollupOptions: {
        onwarn(warning, warn) {
          if (
            warning.code === 'INVALID_ANNOTATION' &&
            warning.message.includes('@vueuse/core') &&
            warning.message.includes('#__PURE__')
          ) {
            return
          }

          warn(warning)
        }
      }
    }
  },
  themeConfig: {
    search: {
      provider: 'local'
    },
    nav: [
      { text: 'Home', link: '/' },
      { text: 'Docs', link: '/docs/introduction' }
    ],
    socialLinks: [
      { icon: 'github', link: 'https://github.com/kristoferlund/ratcn' }
    ],
    sidebar: {
      '/docs/': [
        {
          text: 'Docs',
          items: [
            { text: 'Introduction', link: '/docs/introduction' },
            { text: 'Getting started', link: '/docs/getting-started' },
            { text: 'Demos', link: '/docs/demos' }
          ]
        },
        {
          text: 'Concepts',
          items: [
            { text: 'State and messages', link: '/docs/concepts/state-and-messages' },
            { text: 'Rendering and event routing', link: '/docs/concepts/rendering-and-events' },
            { text: 'Focus, hover, and identity', link: '/docs/concepts/focus-hover-identity' },
            { text: 'Keyboard', link: '/docs/concepts/keyboard' },
            { text: 'Layers and modals', link: '/docs/concepts/layers-and-modals' },
            { text: 'Themes', link: '/docs/concepts/themes' },
            { text: 'Host integration', link: '/docs/concepts/host-integration' },
            { text: 'Mouse input', link: '/docs/concepts/mouse' },
            { text: 'Dragging', link: '/docs/concepts/dragging' },
            { text: 'Structuring a larger app', link: '/docs/concepts/composition' },
            { text: 'Custom components', link: '/docs/concepts/custom-components' },
            { text: 'Design decisions', link: '/docs/concepts/design-decisions' }
          ]
        },
        {
          text: 'Components',
          items: [
            { text: 'BarChartWidget', link: '/docs/components/barchart' },
            { text: 'Button', link: '/docs/components/button' },
            { text: 'Dialog', link: '/docs/components/dialog' },
            { text: 'List', link: '/docs/components/list' },
            { text: 'Select', link: '/docs/components/select' },
            { text: 'Tabs', link: '/docs/components/tabs' },
            { text: 'Toast', link: '/docs/components/toast' },
            { text: 'Tooltip', link: '/docs/components/tooltip' }
          ]
        }
      ]
    }
  }
})
