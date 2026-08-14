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
      <!-- 选项标号独立成元素，与内容分离，由父级 flex align-items:center 实现垂直居中 -->
      <!-- 标号用 LatexRender 渲染 $\mathrm{A.}$ 保持数学罗马体字体样式 -->
      <span class="q-option-label"><LatexRender :text="`$\\mathrm{${opt.label}.}$`" :inline="true" /></span>
      <div class="q-option-content">
        <LatexRender :text="opt.content" :inline="true" />
      </div>
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
  border-radius: 4px; /* 微积木小圆角，区别于卡片大圆角 */
  background: #f7f8fa; /* 淡灰底，让选项成为独立“微积木” */
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

/* 暗色模式：选项微积木改用主题输入态底色，避免 #f7f8fa 过亮 */
[data-theme='dark'] .q-option {
  background: var(--bg-input);
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
  flex: 1;
  min-width: 0;
}

/* 穿透清除 LatexRender 最后元素 margin-bottom，避免 flex 居中视觉偏移 */
.q-option-content :deep(.latex-render > p:last-child),
.q-option-content :deep(.latex-render p) {
  margin-bottom: 0 !important;
  margin-top: 0 !important;
}

.q-option-content :deep(.latex-render img) {
  margin-bottom: 0 !important;
  vertical-align: middle;
}
</style>
