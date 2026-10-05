// The landing initializer measures its font and sizes the iframe before wasm
// starts. Only width changes need a reboot: DomBackend replaces its grid on
// resize but leaves input listeners attached to the old element.
function wire(iframe: HTMLIFrameElement): () => void {
  let width = iframe.clientWidth
  let timer = 0

  const onResize = () => {
    clearTimeout(timer)
    timer = window.setTimeout(() => {
      const next = iframe.clientWidth
      if (!next || next === width) return
      width = next
      iframe.contentWindow?.location.reload()
    }, 150)
  }

  const observer = new ResizeObserver(onResize)
  observer.observe(iframe)
  return () => {
    clearTimeout(timer)
    observer.disconnect()
  }
}

export function initPreviewAutoSize(): () => void {
  const iframe = document.querySelector<HTMLIFrameElement>('.ratcn-landing-preview-frame')
  return iframe ? wire(iframe) : () => {}
}
