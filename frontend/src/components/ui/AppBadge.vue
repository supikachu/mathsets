<script setup lang="ts">
export type BadgeColor = 'blue' | 'green' | 'yellow' | 'red' | 'purple' | 'teal' | 'gray'

withDefaults(
  defineProps<{ color?: BadgeColor }>(),
  { color: 'blue' },
)
</script>

<template>
  <span class="badge" :class="`badge-${color}`">
    <slot />
  </span>
</template>
