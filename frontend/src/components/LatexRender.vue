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

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

function render() {
  if (!container.value) return
  const text = props.text || ''

  // 1. 先转义整个文本，防止 XSS
  let html = escapeHtml(text)

  // 2. 处理换行（在 KaTeX 渲染之前，此时文本已转义，不会破坏 KaTeX 输出）
  html = html.replace(/\n/g, '<br>')

  // 3. 处理块级公式 $$...$$
  html = html.replace(/\$\$(.+?)\$\$/gs, (_, formula) => {
    try {
      // 反转义公式内容中的 HTML 实体，让 KaTeX 正确解析
      const raw = formula
        .replace(/&amp;/g, '&')
        .replace(/&lt;/g, '<')
        .replace(/&gt;/g, '>')
        .replace(/&quot;/g, '"')
        .replace(/&#39;/g, "'")
      return katex.renderToString(raw.trim(), { displayMode: true, throwOnError: false })
    } catch {
      return `<span class="katex-error">${formula}</span>`
    }
  })

  // 4. 处理行内公式 $...$
  html = html.replace(/\$(.+?)\$/g, (_, formula) => {
    try {
      const raw = formula
        .replace(/&amp;/g, '&')
        .replace(/&lt;/g, '<')
        .replace(/&gt;/g, '>')
        .replace(/&quot;/g, '"')
        .replace(/&#39;/g, "'")
      return katex.renderToString(raw.trim(), { displayMode: false, throwOnError: false })
    } catch {
      return `<span class="katex-error">${formula}</span>`
    }
  })

  container.value.innerHTML = html
}

onMounted(render)
watch(() => [props.text, props.inline], render)
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
