<script setup lang="ts">
import { AppIcon } from '@/components/ui'

const props = defineProps<{
  modelValue: boolean
  title?: string
}>()

const emit = defineEmits<{ 'update:modelValue': [value: boolean] }>()

function close() {
  emit('update:modelValue', false)
}
</script>

<template>
  <Teleport to="body">
    <div v-if="modelValue" class="modal-overlay" @click.self="close">
      <div class="modal">
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
