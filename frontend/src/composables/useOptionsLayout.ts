import { ref, onMounted, onBeforeUnmount, nextTick, watch, type Ref } from 'vue'

export function useOptionsLayout(
  containerRef: Ref<HTMLElement | null>,
  optionsListRef: Ref<any[]>,
  optionsSelector = '.paper-opt',
  gap = 16
) {
  const layout = ref<'grid-4' | 'grid-2' | 'grid-1'>('grid-2')
  let resizeObserver: ResizeObserver | null = null
  let layoutTimer: ReturnType<typeof setTimeout> | null = null

  function computeLayout() {
    const container = containerRef.value
    if (!container) return
    const containerWidth = container.clientWidth
    if (containerWidth === 0) return

    const optionEls = container.querySelectorAll<HTMLElement>(optionsSelector)
    if (optionEls.length === 0) return

    // 临时切换为 block 布局测量真实宽度
    const prevDisplay = container.style.display
    const prevCols = container.style.gridTemplateColumns
    container.style.display = 'block'
    container.style.gridTemplateColumns = ''

    let maxWidth = 0
    const prevStyles: { el: HTMLElement; display: string; width: string }[] = []
    optionEls.forEach(el => {
      prevStyles.push({ el, display: el.style.display, width: el.style.width })
      el.style.display = 'inline-flex'
      el.style.width = 'auto'
      el.style.whiteSpace = 'nowrap'
      const w = el.scrollWidth
      if (w > maxWidth) maxWidth = w
      el.style.whiteSpace = ''
    })

    // 恢复原有样式
    prevStyles.forEach(({ el, display, width }) => {
      el.style.display = display
      el.style.width = width
    })
    container.style.display = prevDisplay
    container.style.gridTemplateColumns = prevCols

    if (maxWidth === 0) return

    const slot = maxWidth + gap
    if (slot * 4 <= containerWidth) {
      layout.value = 'grid-4'
    } else if (slot * 2 <= containerWidth) {
      layout.value = 'grid-2'
    } else {
      layout.value = 'grid-1'
    }
  }

  function scheduleCompute() {
    if (layoutTimer) clearTimeout(layoutTimer)
    layoutTimer = setTimeout(() => computeLayout(), 50)
  }

  watch(optionsListRef, () => {
    nextTick(() => {
      setTimeout(() => computeLayout(), 120)
    })
  }, { deep: true })

  onMounted(() => {
    nextTick(() => {
      setTimeout(() => computeLayout(), 150)
      if (containerRef.value) {
        resizeObserver = new ResizeObserver(() => scheduleCompute())
        resizeObserver.observe(containerRef.value)
      }
    })
  })

  onBeforeUnmount(() => {
    if (resizeObserver) {
      resizeObserver.disconnect()
      resizeObserver = null
    }
    if (layoutTimer) clearTimeout(layoutTimer)
  })

  return {
    layout,
    recompute: computeLayout,
  }
}
