import { ref, onMounted, onBeforeUnmount, nextTick, watch, type Ref } from 'vue'
import { useOptionLayout, type LayoutMode } from './useOptionLayout'

export { useOptionLayout, type LayoutMode }

export function useOptionsLayout(
  containerRef: Ref<HTMLElement | null>,
  optionsListRef: Ref<any[]>,
  optionsSelector = '.paper-opt',
  gap = 16
) {
  const { layoutMode, layoutClass, computeLayout } = useOptionLayout(
    containerRef,
    optionsListRef,
    optionsSelector
  )

  const layout = ref<'grid-4' | 'grid-2' | 'grid-1'>('grid-2')

  watch(layoutMode, (newMode) => {
    if (newMode === '4-col') layout.value = 'grid-4'
    else if (newMode === '2-col') layout.value = 'grid-2'
    else layout.value = 'grid-1'
  }, { immediate: true })

  return {
    layout,
    layoutMode,
    layoutClass,
    recompute: computeLayout,
    computeLayout,
  }
}
