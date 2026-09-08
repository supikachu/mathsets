import { ref, readonly, computed } from 'vue'

/** 试题篮 ID 列表（保序）；刷新 / 重开标签页后恢复 */
const STORAGE_KEY = 'mathset_question_basket'

function loadIds(): Set<string> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return new Set()
    const parsed = JSON.parse(raw) as unknown
    if (!Array.isArray(parsed)) return new Set()
    return new Set(
      parsed.filter((id): id is string => typeof id === 'string' && id.length > 0),
    )
  } catch {
    return new Set()
  }
}

function persistIds(ids: Set<string>) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(Array.from(ids)))
  } catch {
    /* quota / 隐私模式：忽略，内存态仍可用 */
  }
}

// Module-level singleton — shared across the entire app
const basketIds = ref<Set<string>>(loadIds())

function commit(next: Set<string>) {
  basketIds.value = next
  persistIds(next)
}

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
    commit(next)
  }

  function add(id: string) {
    if (!basketIds.value.has(id)) {
      const next = new Set(basketIds.value)
      next.add(id)
      commit(next)
    }
  }

  function remove(id: string) {
    if (basketIds.value.has(id)) {
      const next = new Set(basketIds.value)
      next.delete(id)
      commit(next)
    }
  }

  function clear() {
    commit(new Set())
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

/** 登出时清空，避免同机换账号仍看到上一用户的选题 */
export function clearQuestionBasketStorage() {
  commit(new Set())
}
