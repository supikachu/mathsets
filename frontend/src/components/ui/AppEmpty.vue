<script setup lang="ts">
import { AppIcon } from '@/components/ui'

defineProps<{ description?: string; icon?: string }>()
</script>

<template>
  <div class="empty-state">
    <AppIcon :name="icon || 'inbox'" :size="40" class="empty-icon" />
    <p>{{ description || '暂无数据' }}</p>
  </div>
</template>

<style scoped>
.empty-icon {
  color: var(--text-muted);
  opacity: 0.4;
  margin-bottom: 12px;
}
</style>
