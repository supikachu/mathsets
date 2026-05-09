<template>
  <div ref="container" class="latex-render" :class="{ 'latex-inline': inline }" />
</template>

<script setup lang="ts">
import { ref, onMounted, watch } from 'vue'
import katex from 'katex'

const props = defineProps<{
  text: string
  inline?: boolean
}>()

const container = ref<HTMLElement>()

function render() {
  if (!container.value) return
  const text = props.text || ''

  // 将文本中的 $...$ 或 $$...$$ 替换为 KaTeX 渲染
  // 先处理块级公式 $$...$$
  let html = text.replace(/\$\$(.+?)\$\$/gs, (_, formula) => {
    try {
      return katex.renderToString(formula.trim(), { displayMode: true, throwOnError: false })
    } catch {
      return `<span class="katex-error">${formula}</span>`
    }
  })
  // 再处理行内公式 $...$
  html = html.replace(/\$(.+?)\$/g, (_, formula) => {
    try {
      return katex.renderToString(formula.trim(), { displayMode: false, throwOnError: false })
    } catch {
      return `<span class="katex-error">${formula}</span>`
    }
  })
  // 处理换行
  html = html.replace(/\n/g, '<br>')

  container.value.innerHTML = html
}

onMounted(render)
watch(() => props.text, render)
</script>

<style>
.latex-render {
  line-height: 1.8;
}
.latex-render.latex-inline {
  display: inline;
}
.latex-render .katex-error {
  color: #e74c3c;
  border-bottom: 1px dashed #e74c3c;
}
</style>
