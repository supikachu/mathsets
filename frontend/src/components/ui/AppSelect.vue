<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount, nextTick } from 'vue'
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

const open = ref(false)
const triggerRef = ref<HTMLElement | null>(null)
const popoverRef = ref<HTMLElement | null>(null)

const selectedLabel = computed(() => {
  if (!props.modelValue) return ''
  const opt = props.options.find(o => o.value === props.modelValue)
  return opt?.label ?? ''
})

const displayText = computed(() => selectedLabel.value || props.placeholder || '请选择')
const hasValue = computed(() => !!props.modelValue)

function toggle() {
  if (props.disabled) return
  open.value = !open.value
}

function select(val: string) {
  emit('update:modelValue', val)
  open.value = false
}

function clearValue(e: Event) {
  e.stopPropagation()
  emit('update:modelValue', undefined)
}

function onClickOutside(e: MouseEvent) {
  const target = e.target as Node
  if (triggerRef.value?.contains(target)) return
  if (popoverRef.value?.contains(target)) return
  open.value = false
}

function onEscape(e: KeyboardEvent) {
  if (e.key === 'Escape' && open.value) {
    open.value = false
    triggerRef.value?.focus()
  }
}

onMounted(() => {
  document.addEventListener('click', onClickOutside)
  document.addEventListener('keydown', onEscape)
})
onBeforeUnmount(() => {
  document.removeEventListener('click', onClickOutside)
  document.removeEventListener('keydown', onEscape)
})
</script>

<template>
  <div class="app-select-wrapper" :class="{ disabled }">
    <button
      ref="triggerRef"
      type="button"
      class="app-select-trigger"
      :class="{ open, 'has-value': hasValue }"
      :disabled="disabled"
      @click="toggle"
    >
      <span class="app-select-text" :class="{ placeholder: !hasValue }">{{ displayText }}</span>
      <div class="app-select-icons">
        <button
          v-if="clearable && hasValue && !disabled"
          type="button"
          class="app-select-clear"
          @click="clearValue"
        ><AppIcon name="x" :size="13" /></button>
        <svg class="app-select-chevron" :class="{ rotated: open }" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
          <path d="m6 9 6 6 6-6" />
        </svg>
      </div>
    </button>

    <Transition name="select-pop">
      <div v-if="open" ref="popoverRef" class="app-select-popover">
        <button
          type="button"
          class="app-select-option"
          :class="{ selected: !modelValue }"
          @click="select('')"
        >
          <span>{{ placeholder || '请选择' }}</span>
          <AppIcon v-if="!modelValue" name="check" :size="15" class="option-check" />
        </button>
        <button
          v-for="opt in options"
          :key="opt.value"
          type="button"
          class="app-select-option"
          :class="{ selected: modelValue === opt.value }"
          @click="select(opt.value)"
        >
          <span>{{ opt.label }}</span>
          <AppIcon v-if="modelValue === opt.value" name="check" :size="15" class="option-check" />
        </button>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.app-select-wrapper {
  position: relative;
  display: block;
  width: 100%;
}

/* Trigger button — looks like a text field */
.app-select-trigger {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
  width: 100%;
  padding: 7px 12px;
  border: 1px solid var(--border-color);
  border-radius: 8px;
  background: var(--bg-input);
  color: var(--text-primary);
  font-size: 13px;
  font-family: inherit;
  line-height: 1.4;
  cursor: pointer;
  transition: border-color 0.2s, box-shadow 0.2s, background 0.2s;
  text-align: left;
}

.app-select-trigger:hover:not(.disabled) {
  border-color: var(--text-muted);
}

.app-select-trigger.open {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
  background: var(--bg-card);
}

.app-select-trigger:disabled,
.app-select-wrapper.disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.app-select-text {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.app-select-text.placeholder {
  color: var(--text-muted);
}

.app-select-icons {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
}

.app-select-clear {
  display: flex;
  align-items: center;
  justify-content: center;
  background: none;
  border: none;
  padding: 0;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  color: var(--text-muted);
  cursor: pointer;
  transition: color 0.15s, background 0.15s;
}

.app-select-clear:hover {
  color: var(--text-primary);
  background: var(--bg-hover);
}

.app-select-chevron {
  color: var(--text-muted);
  transition: transform 0.2s ease;
  flex-shrink: 0;
}

.app-select-chevron.rotated {
  transform: rotate(180deg);
}

/* Popover — Apple style floating card */
.app-select-popover {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  right: 0;
  z-index: 100;
  max-height: 240px;
  overflow-y: auto;
  padding: 4px;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: 10px;
  box-shadow:
    0 4px 24px rgba(0, 0, 0, 0.08),
    0 1px 4px rgba(0, 0, 0, 0.06);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
}

[data-theme='dark'] .app-select-popover {
  background: rgba(44, 44, 46, 0.95);
  border-color: rgba(84, 84, 88, 0.6);
  box-shadow:
    0 4px 24px rgba(0, 0, 0, 0.4),
    0 1px 4px rgba(0, 0, 0, 0.3);
}

.app-select-option {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
  padding: 7px 10px;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--text-primary);
  font-size: 13px;
  font-family: inherit;
  line-height: 1.4;
  cursor: pointer;
  transition: background 0.12s, color 0.12s;
  text-align: left;
}

.app-select-option:hover {
  background: var(--bg-hover);
}

.app-select-option.selected {
  color: var(--accent);
  font-weight: 600;
}

.app-select-option.selected:hover {
  background: var(--accent-light);
}

.option-check {
  color: var(--accent);
  flex-shrink: 0;
}

/* Scrollbar */
.app-select-popover::-webkit-scrollbar {
  width: 5px;
}
.app-select-popover::-webkit-scrollbar-track {
  background: transparent;
}
.app-select-popover::-webkit-scrollbar-thumb {
  background: var(--border-color);
  border-radius: 3px;
}

/* Transition */
.select-pop-enter-active {
  transition: opacity 0.15s ease, transform 0.15s ease;
}
.select-pop-leave-active {
  transition: opacity 0.1s ease, transform 0.1s ease;
}
.select-pop-enter-from {
  opacity: 0;
  transform: translateY(-4px) scale(0.98);
}
.select-pop-leave-to {
  opacity: 0;
  transform: translateY(-2px) scale(0.98);
}
</style>
