<script setup lang="ts">
export type ButtonVariant = 'primary' | 'success' | 'danger' | 'outline' | 'ghost'
export type ButtonSize = 'md' | 'sm'

withDefaults(
  defineProps<{
    variant?: ButtonVariant
    size?: ButtonSize
    block?: boolean
    loading?: boolean
    disabled?: boolean
    nativeType?: 'button' | 'submit' | 'reset'
  }>(),
  {
    variant: 'primary',
    size: 'md',
    block: false,
    loading: false,
    disabled: false,
    nativeType: 'button',
  },
)
</script>

<template>
  <button
    :type="nativeType"
    class="btn"
    :class="[
      `btn-${variant}`,
      size === 'sm' ? 'btn-sm' : '',
      { 'btn-block': block },
    ]"
    :disabled="disabled || loading"
  >
    <span v-if="loading" class="btn-loading" />
    <slot />
  </button>
</template>

<style scoped>
.btn-loading {
  margin-right: 4px;
}
</style>
