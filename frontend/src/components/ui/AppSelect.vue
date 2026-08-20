<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount, nextTick, watch } from 'vue'
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
// 下拉面板的 fixed 定位坐标（基于 viewport）
const popoverStyle = ref({ top: '0px', left: '0px', minWidth: '0px' })

const selectedLabel = computed(() => {
  if (!props.modelValue) return ''
  const opt = props.options.find(o => o.value === props.modelValue)
  return opt?.label ?? ''
})

const displayText = computed(() => selectedLabel.value || props.placeholder || '请选择')
const hasValue = computed(() => !!props.modelValue)

function updatePopoverPosition() {
  if (!triggerRef.value) return
  const rect = triggerRef.value.getBoundingClientRect()
  popoverStyle.value = {
    top: `${rect.bottom + 4}px`,
    left: `${rect.left}px`,
    minWidth: `${rect.width}px`,
  }
}

function toggle() {
  if (props.disabled) return
  open.value = !open.value
  if (open.value) {
    nextTick(() => updatePopoverPosition())
  }
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

// 下拉打开时监听滚动和缩放，实时更新位置
function onScrollOrResize() {
  if (open.value) updatePopoverPosition()
}

watch(open, (val) => {
  if (val) {
    window.addEventListener('scroll', onScrollOrResize, true)
    window.addEventListener('resize', onScrollOrResize)
  } else {
    window.removeEventListener('scroll', onScrollOrResize, true)
    window.removeEventListener('resize', onScrollOrResize)
  }
})

onMounted(() => {
  document.addEventListener('click', onClickOutside)
  document.addEventListener('keydown', onEscape)
})
onBeforeUnmount(() => {
  document.removeEventListener('click', onClickOutside)
  document.removeEventListener('keydown', onEscape)
  window.removeEventListener('scroll', onScrollOrResize, true)
  window.removeEventListener('resize', onScrollOrResize)
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

    <Teleport to="body">
      <Transition name="select-pop">
        <div v-if="open" ref="popoverRef" class="app-select-popover" :style="popoverStyle">
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
    </Teleport>
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
  padding: 0 11px;
  border: 1px solid color-mix(in srgb, var(--text-primary, #1d1d1f) 9%, transparent);
  border-radius: 9px;
  background: var(--bg-card, #fff);
  color: var(--text-primary);
  font-size: 13px;
  font-family: inherit;
  line-height: 1.4;
  cursor: pointer;
  transition: border-color 0.18s ease, box-shadow 0.18s ease, background 0.18s ease;
  text-align: left;
  box-sizing: border-box;
  min-height: 38px;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.025);
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

/* Popover — Apple style floating card（Teleport 到 body，position: fixed 脱离父容器裁剪） */
.app-select-popover {
  position: fixed;
  z-index: 10000;
  min-width: max-content;
  max-height: 280px;
  overflow-y: auto;
  padding: 6px;
  color: var(--text-primary, #1d1d1f);
  background: color-mix(in srgb, var(--bg-card, #fff) 86%, transparent);
  border: 1px solid color-mix(in srgb, var(--text-primary, #1d1d1f) 8%, transparent);
  border-radius: 12px;
  box-shadow:
    0 0 0 0.5px rgba(0, 0, 0, 0.04),
    0 10px 40px rgba(0, 0, 0, 0.14),
    0 2px 8px rgba(0, 0, 0, 0.06);
  backdrop-filter: saturate(180%) blur(28px);
  -webkit-backdrop-filter: saturate(180%) blur(28px);
  font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif;
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
  min-height: 32px;
  padding: 7px 10px;
  border: none;
  border-radius: 8px;
  background: transparent;
  color: var(--text-primary);
  font-size: 13px;
  font-family: inherit;
  line-height: 1.3;
  cursor: pointer;
  transition: background 0.12s ease, color 0.12s ease;
  text-align: left;
  white-space: nowrap;
}

.app-select-option:hover {
  background: color-mix(in srgb, var(--text-primary, #1d1d1f) 6%, transparent);
}

[data-theme='dark'] .app-select-option:hover {
  background: rgba(255, 255, 255, 0.08);
}

.app-select-option.selected {
  color: var(--accent, #0071e3);
  font-weight: 600;
  background: color-mix(in srgb, var(--accent, #0071e3) 10%, transparent);
}

.app-select-option.selected:hover {
  background: color-mix(in srgb, var(--accent, #0071e3) 16%, transparent);
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
