<script setup lang="ts">
withDefaults(
  defineProps<{
    title?: string
    noHover?: boolean
  }>(),
  { noHover: false },
)
</script>

<template>
  <div class="card" :class="{ 'card-static': noHover }">
    <div v-if="$slots.header || title" class="card-header">
      <slot name="header">
        <span class="card-title">{{ title }}</span>
      </slot>
    </div>
    <slot />
  </div>
</template>

<style scoped>
.card-static:hover {
  transform: none;
  box-shadow: var(--shadow-sm);
}

.card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 16px;
  padding-bottom: 12px;
  border-bottom: 1px solid var(--border-color);
}

.card-title {
  font-size: 15px;
  font-weight: 700;
  color: var(--text-primary);
}
</style>
