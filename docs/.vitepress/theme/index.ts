import { onContentUpdated } from 'vitepress'
import DefaultTheme from 'vitepress/theme'
import './style.css'
import { initPreviewAutoSize } from './preview-resize'

export default {
  extends: DefaultTheme,
  enhanceApp() {
    if (typeof window === 'undefined') return
    // `onContentUpdated` runs once the page component's DOM is in place — on
    // mount, on update, and on unmount alike — which is both the moment the
    // preview iframe can be measured and the moment the previous page's wiring
    // stops having anything to observe. One registration for the life of the
    // app, one live wiring at a time.
    let dispose = () => {}
    onContentUpdated(() => {
      dispose()
      dispose = initPreviewAutoSize()
    })
  }
}
