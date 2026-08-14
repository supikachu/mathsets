import { ref, computed, onMounted, onBeforeUnmount, nextTick, watch, type Ref } from 'vue'

export type LayoutMode = '4-col' | '2-col' | '1-col'

/**
 * 智能选项布局 Composable:
 * 根据 4 个选项中最大 DOM 物理宽度与容器可用宽度，
 * 动态计算最佳排版模式（4列 / 2列 / 1列）。
 */
export function useOptionLayout(
  containerRef: Ref<HTMLElement | null>,
  optionsListRef: Ref<any[]>,
  optionsSelector = '.q-option'
) {
  const layoutMode = ref<LayoutMode>('2-col')
  let resizeObserver: ResizeObserver | null = null
  let layoutTimer: ReturnType<typeof setTimeout> | null = null

  // 映射对应的 Tailwind 均分网格类名（实现制表位垂直对齐）
  const layoutClass = computed(() => {
    switch (layoutMode.value) {
      case '4-col':
        return 'grid grid-cols-4 gap-x-4 gap-y-4 w-full'
      case '1-col':
        return 'grid grid-cols-1 gap-y-4 w-full'
      case '2-col':
      default:
        return 'grid grid-cols-2 gap-x-8 gap-y-4 w-full'
    }
  })

  function computeLayout() {
    const container = containerRef.value
    if (!container) return

    const optionEls = container.querySelectorAll<HTMLElement>(optionsSelector)
    if (optionEls.length === 0) return

    // 1. 临时解锁容器与选项尺寸，测量真实尺寸
    const prevContainerDisplay = container.style.display
    const prevContainerWidth = container.style.width
    container.style.display = 'block'
    container.style.width = '100%'

    // 父级容器可用宽度
    const containerWidth = container.clientWidth || container.parentElement?.clientWidth || 0

    if (containerWidth === 0) {
      container.style.display = prevContainerDisplay
      container.style.width = prevContainerWidth
      return
    }

    let maxOptionWidth = 0
    const prevOptionStyles: { el: HTMLElement; display: string; width: string; whiteSpace: string }[] = []

    optionEls.forEach((el) => {
      prevOptionStyles.push({
        el,
        display: el.style.display,
        width: el.style.width,
        whiteSpace: el.style.whiteSpace,
      })
      el.style.display = 'inline-flex'
      el.style.width = 'auto'
      el.style.whiteSpace = 'nowrap'

      const w = Math.max(el.offsetWidth, el.scrollWidth, el.getBoundingClientRect().width)
      if (w > maxOptionWidth) {
        maxOptionWidth = w
      }
    })

    // 恢复原有样式
    prevOptionStyles.forEach(({ el, display, width, whiteSpace }) => {
      el.style.display = display
      el.style.width = width
      el.style.whiteSpace = whiteSpace
    })
    container.style.display = prevContainerDisplay
    container.style.width = prevContainerWidth

    if (maxOptionWidth === 0) return

    // 2. 判断 4列 / 2列 / 1列 降级逻辑
    // 1x4 (grid-cols-4): gap-x-4 (16px)，3个 gap => 48px
    // 2x2 (grid-cols-2): gap-x-8 (32px)，1个 gap => 32px
    const gapX4Col = 16
    const gapX2Col = 32

    const requiredWidth4Col = maxOptionWidth * 4 + gapX4Col * 3
    const requiredWidth2Col = maxOptionWidth * 2 + gapX2Col

    if (requiredWidth4Col <= containerWidth) {
      layoutMode.value = '4-col'
    } else if (requiredWidth2Col <= containerWidth) {
      layoutMode.value = '2-col'
    } else {
      layoutMode.value = '1-col'
    }
  }

  function scheduleCompute() {
    if (layoutTimer) clearTimeout(layoutTimer)
    layoutTimer = setTimeout(() => computeLayout(), 40)
  }

  watch(
    optionsListRef,
    () => {
      nextTick(() => {
        setTimeout(() => computeLayout(), 100)
      })
    },
    { deep: true }
  )

  onMounted(() => {
    nextTick(() => {
      setTimeout(() => computeLayout(), 150)
      if (containerRef.value) {
        resizeObserver = new ResizeObserver(() => scheduleCompute())
        resizeObserver.observe(containerRef.value)
        if (containerRef.value.parentElement) {
          resizeObserver.observe(containerRef.value.parentElement)
        }
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
    layoutMode,
    layoutClass,
    computeLayout,
  }
}
