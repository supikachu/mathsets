<script setup lang="ts">
import AppIcon from './AppIcon.vue'

export interface SelectOption {
  label: string
  value: string
}

const props = withDefaults(
  defineProps<{
    modelValue?: string
    options: SelectOption[]
    placeholder?: string
    clearable?: boolean
    disabled?: boolean
  }>(),
  { clearable: false, disabled: false },
)

const emit = defineEmits<{ 'update:modelValue': [value: string | undefined] }>()

function onChange(e: Event) {
  const val = (e.target as HTMLSelectElement).value
  emit('update:modelValue', val === '' ? undefined : val)
}

function clearValue() {
  emit('update:modelValue', undefined)
}
</script>

<template>
  <div class="app-select-wrapper">
    <select :value="modelValue ?? ''" :disabled="disabled" @change="onChange">
      <option value="">{{ placeholder || '请选择' }}</option>
      <option v-for="opt in options" :key="opt.value" :value="opt.value">
        {{ opt.label }}
      </option>
    </select>
    <button
      v-if="clearable && modelValue && !disabled"
      type="button"
      class="app-select-clear"
      @click="clearValue"
    ><AppIcon name="x" :size="14" /></button>
  </div>
</template>

<style scoped>
.app-select-wrapper {
  position: relative;
  display: inline-block;
  width: 100%;
}

.app-select-wrapper select {
  width: 100%;
}

.app-select-clear {
  position: absolute;
  right: 8px;
  top: 50%;
  transform: translateY(-50%);
  background: none;
  border: none;
  cursor: pointer;
  font-size: 18px;
  line-height: 1;
  color: var(--text-muted);
  padding: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  transition: var(--transition);
}

.app-select-clear:hover {
  color: var(--text-primary);
  background: var(--bg-hover);
}
</style>
