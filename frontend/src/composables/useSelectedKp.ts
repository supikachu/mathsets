import { ref, readonly } from 'vue'

// Module-level singleton state — shared across the entire app
const selectedKpId = ref<string | null>(null)
const selectedKpName = ref<string | null>(null)

export function useSelectedKp() {
  function select(id: string | null, name?: string | null) {
    selectedKpId.value = id
    selectedKpName.value = name ?? null
  }

  function clear() {
    selectedKpId.value = null
    selectedKpName.value = null
  }

  return {
    selectedKpId: readonly(selectedKpId),
    selectedKpName: readonly(selectedKpName),
    select,
    clear,
  }
}
