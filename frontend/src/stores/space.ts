import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { spaceApi, type SpaceSummary } from '@/api/client'

export const useSpaceStore = defineStore('space', () => {
  const spaces = ref<SpaceSummary[]>([])
  const currentSpaceId = ref(localStorage.getItem('currentSpaceId') || '')
  const loading = ref(false)
  const spacesLoaded = ref(false)

  const currentSpace = computed(
    () => spaces.value.find((s) => s.id === currentSpaceId.value) || null,
  )

  const personalSpace = computed(
    () => spaces.value.find((s) => s.kind === 'personal') || null,
  )

  const publicSpace = computed(
    () => spaces.value.find((s) => s.kind === 'public') || null,
  )

  async function fetchSpaces() {
    loading.value = true
    try {
      const res = await spaceApi.list()
      spaces.value = res.data || []
      // 默认选中个人空间
      if (!currentSpaceId.value || !spaces.value.some((s) => s.id === currentSpaceId.value)) {
        const personal = spaces.value.find((s) => s.kind === 'personal')
        if (personal) {
          currentSpaceId.value = personal.id
          localStorage.setItem('currentSpaceId', personal.id)
        } else if (spaces.value[0]) {
          currentSpaceId.value = spaces.value[0].id
          localStorage.setItem('currentSpaceId', spaces.value[0].id)
        }
      }
    } catch {
      /* handled by interceptor */
    } finally {
      loading.value = false
      spacesLoaded.value = true
    }
  }

  function setCurrentSpace(id: string) {
    currentSpaceId.value = id
    localStorage.setItem('currentSpaceId', id)
  }

  return {
    spaces,
    currentSpaceId,
    currentSpace,
    personalSpace,
    publicSpace,
    loading,
    spacesLoaded,
    fetchSpaces,
    setCurrentSpace,
  }
})
