<script setup lang="ts">
import { computed } from 'vue'
import { useToast } from '@/composables/useToast'
import { AppIcon } from '@/components/ui'

const { toasts } = useToast()

const iconFor = computed(() => (type: string) => {
  const map: Record<string, string> = {
    success: 'check-circle',
    error: 'x-circle',
    warning: 'alert',
    info: 'info',
  }
  return map[type] || 'info'
})
</script>

<template>
  <div class="toast-container">
    <div v-for="t in toasts" :key="t.id" class="toast" :class="t.type">
      <AppIcon :name="iconFor(t.type)" :size="18" class="toast-icon" />
      <span>{{ t.message }}</span>
    </div>
  </div>
</template>

<style scoped>
.toast-icon {
  flex-shrink: 0;
}

.toast.success .toast-icon {
  color: var(--success);
}
.toast.error .toast-icon {
  color: var(--danger);
}
.toast.warning .toast-icon {
  color: var(--warning);
}
</style>
