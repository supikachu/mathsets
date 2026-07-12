import { ref, readonly, computed } from 'vue'

// Module-level singleton — shared across the entire app
const basketIds = ref<Set<string>>(new Set())

export function useQuestionBasket() {
  const count = computed(() => basketIds.value.size)
  const isEmpty = computed(() => basketIds.value.size === 0)

  function isInBasket(id: string): boolean {
    return basketIds.value.has(id)
  }

  function toggle(id: string) {
    const next = new Set(basketIds.value)
    if (next.has(id)) {
      next.delete(id)
    } else {
      next.add(id)
    }
    basketIds.value = next
  }

  function add(id: string) {
    if (!basketIds.value.has(id)) {
      const next = new Set(basketIds.value)
      next.add(id)
      basketIds.value = next
    }
  }

  function remove(id: string) {
    if (basketIds.value.has(id)) {
      const next = new Set(basketIds.value)
      next.delete(id)
      basketIds.value = next
    }
  }

  function clear() {
    basketIds.value = new Set()
  }

  function getAll(): string[] {
    return Array.from(basketIds.value)
  }

  return {
    basketIds: readonly(basketIds),
    count,
    isEmpty,
    isInBasket,
    toggle,
    add,
    remove,
    clear,
    getAll,
  }
}
