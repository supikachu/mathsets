import { ref, readonly } from 'vue'

// Module-level singleton state — shared across the entire app
const selectedKpId = ref<string | null>(null)
const selectedKpName = ref<string | null>(null)

// 知识点学段（与 KpTreePanel 共享）
const kpLevel = ref<'junior' | 'senior'>('junior')

export function useSelectedKp() {
  function select(id: string | null, name?: string | null) {
    selectedKpId.value = id
    selectedKpName.value = name ?? null
  }

  function clear() {
    selectedKpId.value = null
    selectedKpName.value = null
  }

  function setLevel(lv: 'junior' | 'senior') {
    kpLevel.value = lv
  }

  return {
    selectedKpId: readonly(selectedKpId),
    selectedKpName: readonly(selectedKpName),
    kpLevel: readonly(kpLevel),
    select,
    clear,
    setLevel,
  }
}
