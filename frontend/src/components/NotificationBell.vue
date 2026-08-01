<template>
  <button
    type="button"
    class="notif-bell"
    :class="{ active: open }"
    @click="$emit('toggle')"
  >
    <AppIcon name="bell" :size="20" />
    <span v-if="unreadCount > 0" class="notif-badge">
      {{ unreadCount > 99 ? '99+' : unreadCount }}
    </span>
  </button>
</template>

<script setup lang="ts">
import { AppIcon } from '@/components/ui'

defineProps<{
  unreadCount: number
  open: boolean
}>()

defineEmits<{
  toggle: []
}>()
</script>

<style scoped>
.notif-bell {
  position: relative;
  width: 36px;
  height: 36px;
  border-radius: var(--radius-xs);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-secondary);
  background: transparent;
  transition: var(--transition-fast);
}

.notif-bell:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.notif-bell.active {
  background: var(--accent-light);
  color: var(--accent);
}

/* 未读红点角标 */
.notif-badge {
  position: absolute;
  top: 4px;
  right: 4px;
  min-width: 16px;
  height: 16px;
  padding: 0 4px;
  border-radius: 8px;
  background: #ff3b30;
  color: #fff;
  font-size: 10px;
  font-weight: 700;
  line-height: 16px;
  text-align: center;
  border: 2px solid var(--bg-card);
  box-sizing: content-box;
}
</style>
