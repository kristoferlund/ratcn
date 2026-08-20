---
layout: false
description: "ratcn is a component library for Ratatui apps: beautifully designed terminal UI components that you can copy, paste, theme, and own in your application code."
---

<script setup>
import { ref, onMounted } from 'vue'

// Baked in at build time so the rendered HTML already carries a real number —
// no flash of a placeholder, and it stays right for crawlers that do not run
// scripts. Refreshed on mount so a deployment that sits for months does not
// drift. A failed or rate-limited request simply keeps the build-time value.
const stars = ref(__GITHUB_STARS__)

onMounted(async () => {
  try {
    const response = await fetch(`https://api.github.com/repos/${__GITHUB_REPO__}`, {
      headers: { Accept: 'application/vnd.github+json' }
    })
    if (!response.ok) return
    const data = await response.json()
    if (typeof data.stargazers_count === 'number') stars.value = data.stargazers_count
  } catch {
    // Offline or rate limited: the build-time number stands.
  }
})
</script>

<div class="ratcn-preview-notice">
  <span class="ratcn-preview-notice-dot"></span>
  <span>Preview release: the API will break, there is no install CLI yet, and more components are coming — text input, text area, and scroll area. Want something specific? Open an issue.</span>
  <a href="https://github.com/kristoferlund/ratcn/issues" aria-label="Open an issue on GitHub">
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><path fill="currentColor" d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z"/></svg>
    GitHub
  </a>
</div>

<header class="ratcn-site-header">
  <div class="ratcn-header-inner">
    <div class="ratcn-header-left">
      <a href="/" class="ratcn-header-logo" aria-label="ratcn home">ratcn</a>
      <nav class="ratcn-header-nav" aria-label="Main navigation">
        <a href="/docs/introduction">Docs</a>
        <a href="/docs/components/button">Components</a>
      </nav>
    </div>
    <div class="ratcn-header-right">
      <a class="ratcn-header-github" href="https://github.com/kristoferlund/ratcn" aria-label="GitHub repository">
        <svg viewBox="0 0 16 16" width="15" height="15" aria-hidden="true"><path fill="currentColor" d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z"/></svg>
        <span>{{ stars }}</span>
      </a>
      <a class="ratcn-header-cta" href="/docs/getting-started">Get Started</a>
    </div>
  </div>
</header>

<main class="ratcn-home">
  <section class="ratcn-hero" aria-labelledby="ratcn-title">
    <h1 id="ratcn-title">The Foundation for your Terminal UI</h1>
    <p class="ratcn-lede">
      A set of beautifully designed components that you can copy and paste into
      your Ratatui apps. Themeable. App-owned state. Open Source. Open Code.
    </p>
    <div class="ratcn-actions" aria-label="Primary links">
      <a class="ratcn-button ratcn-button-primary" href="/docs/getting-started">Build Your Own <span aria-hidden="true">→</span></a>
      <a class="ratcn-button ratcn-button-secondary" href="https://github.com/kristoferlund/ratcn">GitHub</a>
    </div>
  </section>

  <section class="ratcn-preview" aria-labelledby="ratcn-preview-title">
    <h2 id="ratcn-preview-title" class="ratcn-sr-only">Live component preview</h2>
    <!-- Matches preview-resize.ts heightFor(3), the wide three-column layout,
         so a hard reload on a wide screen boots the wasm at its final height
         and needs no corrective reload. -->
    <div class="ratcn-preview-window" style="--ratcn-preview-height: 909px">
      <div class="ratcn-preview-chrome" aria-hidden="true">
        <span class="ratcn-dot"></span>
        <span class="ratcn-dot"></span>
        <span class="ratcn-dot"></span>
        <span class="ratcn-preview-url">cargo run -p landing</span>
      </div>
      <div class="ratcn-preview-body">
        <iframe
          class="ratcn-landing-preview-frame"
          src="./demos/landing-grid/index.html"
          title="ratcn responsive WebAssembly component grid"
          loading="lazy"
        ></iframe>
      </div>
    </div>
    <p class="ratcn-preview-caption">
      Every component above is real Ratatui, compiled to WebAssembly and rendered
      live in your browser. The exact same code runs in your terminal.
    </p>
  </section>
</main>
