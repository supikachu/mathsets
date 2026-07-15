<script setup lang="ts">
import { computed } from 'vue'
import { AppIcon } from '@/components/ui'

const props = defineProps<{
  modelValue: boolean
  title?: string
  size?: 'sm' | 'md' | 'lg' | 'xl'
}>()

const emit = defineEmits<{ 'update:modelValue': [value: boolean] }>()

const maxWidth = computed(() => {
  switch (props.size) {
    case 'sm': return '400px'
    case 'lg': return '800px'
    case 'xl': return '960px'
    default: return '600px'
  }
})

function close() {
  emit('update:modelValue', false)
}
</script>

<template>
  <Teleport to="body">
    <div v-if="modelValue" class="modal-overlay" @click.self="close">
      <div class="modal" :style="{ maxWidth }">
        <div v-if="title || $slots.header" class="modal-header">
          <slot name="header">
            <span>{{ title }}</span>
          </slot>
          <button type="button" class="modal-close" @click="close">
            <AppIcon name="x" :size="16" />
          </button>
        </div>
        <slot />
      </div>
    </div>
  </Teleport>
</template>
