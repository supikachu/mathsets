<script setup lang="ts">
import AppModal from './AppModal.vue'
import AppButton from './AppButton.vue'

const props = withDefaults(
  defineProps<{
    modelValue: boolean
    title?: string
    message: string
    confirmText?: string
    cancelText?: string
    danger?: boolean
    loading?: boolean
  }>(),
  { loading: false },
)

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  confirm: []
}>()

function close() {
  if (props.loading) return
  emit('update:modelValue', false)
}

function onConfirm() {
  emit('confirm')
}
</script>

<template>
  <AppModal
    :model-value="modelValue"
    :title="title"
    @update:model-value="$emit('update:modelValue', $event)"
  >
    <div class="confirm-content">
      <p>{{ message }}</p>
      <div class="confirm-actions">
        <AppButton variant="ghost" :disabled="loading" @click="close">{{ cancelText || '取消' }}</AppButton>
        <AppButton :variant="danger ? 'danger' : 'primary'" :loading="loading" @click="onConfirm">
          {{ confirmText || '确定' }}
        </AppButton>
      </div>
    </div>
  </AppModal>
</template>
