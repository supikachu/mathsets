<template>
  <div
    ref="containerRef"
    class="q-options"
    :class="layoutClass"
  >
    <div
      v-for="opt in options"
      :key="opt.label"
      class="q-option w-fit flex items-center gap-2 whitespace-nowrap"
    >
      <!-- 选项前缀与内容统一拼接后送入 KaTeX 渲染 -->
      <LatexRender :text="`$\\mathrm{${opt.label}.}$ ` + opt.content" :inline="true" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, toRef } from 'vue'
import LatexRender from '@/components/LatexRender.vue'
import { useOptionLayout } from '@/composables/useOptionLayout'

const props = defineProps<{
  options: { label: string; content: string }[]
}>()

const containerRef = ref<HTMLElement | null>(null)
const optionsRef = toRef(props, 'options')

const { layoutClass, layoutMode, computeLayout } = useOptionLayout(
  containerRef,
  optionsRef,
  '.q-option'
)

defineExpose({
  containerRef,
  layoutMode,
  layoutClass,
  computeLayout,
})
</script>

<style scoped>
.q-options {
  margin-top: 14px;
}

.q-option {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 6px 12px;
  border-radius: var(--radius-sm);
  background: var(--bg-input);
  border: 1px solid transparent;
  font-size: 13.5px;
  line-height: 1.6;
  white-space: nowrap;
  width: fit-content;
  max-width: 100%;
  transition: var(--transition-fast);
}

.q-option:hover {
  background: var(--bg-hover);
}

.q-option-label {
  font-weight: 600;
  color: var(--text-secondary);
  flex-shrink: 0;
}

.q-option-content {
  display: inline-flex;
  align-items: center;
  white-space: nowrap;
}
</style>
