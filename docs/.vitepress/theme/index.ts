import DefaultTheme from 'vitepress/theme'
import './style.css'
import { initPreviewAutoSize } from './preview-resize'

export default {
  extends: DefaultTheme,
  enhanceApp({ router }) {
    if (typeof window === 'undefined') return
    const prev = router.onAfterRouteChanged
    router.onAfterRouteChanged = (to: string) => {
      prev?.(to)
      initPreviewAutoSize()
    }
    initPreviewAutoSize()
  }
}
