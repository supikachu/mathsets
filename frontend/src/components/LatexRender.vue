<template>
  <div ref="container" class="latex-render" :class="{ 'latex-inline': inline }" />
</template>

<script setup lang="ts">
import { ref, onMounted, watch } from 'vue'
import katex from 'katex'

const props = defineProps<{
  text: string
  inline?: boolean
  subQuestionBadge?: boolean
}>()

const container = ref<HTMLElement>()

// 全局 KaTeX 宏：将 \emptyset 映射为 \varnothing，符合国内教材椭圆空集符号
const katexMacros = {
  '\\emptyset': '\\varnothing',
}

// 将公式中的 Unicode 空集符号 ∅ (U+2205) 替换为 \varnothing
// KaTeX macros 只对 LaTeX 命令生效，Unicode 字符需预处理
function normalizeEmptyset(s: string): string {
  return s.replace(/\u2205/g, '\\varnothing')
}

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

  // 1.5. 小问题号徽章：保护数学公式 → 替换题号 → 恢复公式
  if (props.subQuestionBadge) {
    const mathStore: string[] = []
    // 暂存块级公式 $$...$$
    html = html.replace(/\$\$(.+?)\$\$/gs, (m) => {
      const i = mathStore.length
      mathStore.push(m)
      return `\x00MATH${i}\x00`
    })
    // 暂存行内公式 $...$
    html = html.replace(/\$(.+?)\$/g, (m) => {
      const i = mathStore.length
      mathStore.push(m)
      return `\x00MATH${i}\x00`
    })
    // 替换全角/半角括号题号为徽章
    html = html.replace(/\((\d+)\)|（(\d+)）/g, (_, half, full) => {
      return `<span class="sub-question-badge">${half || full}</span>`
    })
    // 恢复数学公式
    html = html.replace(/\x00MATH(\d+)\x00/g, (_, i) => mathStore[parseInt(i)])
  }

  // 2. 处理块级公式 $$...$$（在换行替换之前，避免公式内的 \n 被转为 <br>）
  html = html.replace(/\$\$(.+?)\$\$/gs, (_, formula) => {
    try {
      const raw = formula
        .replace(/&amp;/g, '&')
        .replace(/&lt;/g, '<')
        .replace(/&gt;/g, '>')
        .replace(/&quot;/g, '"')
        .replace(/&#39;/g, "'")
      return katex.renderToString(normalizeEmptyset(raw.trim()), { displayMode: true, throwOnError: false, macros: katexMacros })
    } catch {
      return `<span class="katex-error">${formula}</span>`
    }
  })

  // 3. 处理行内公式 $...$
  html = html.replace(/\$(.+?)\$/g, (_, formula) => {
    try {
      const raw = formula
        .replace(/&amp;/g, '&')
        .replace(/&lt;/g, '<')
        .replace(/&gt;/g, '>')
        .replace(/&quot;/g, '"')
        .replace(/&#39;/g, "'")
      return katex.renderToString(normalizeEmptyset(raw.trim()), { displayMode: false, throwOnError: false, macros: katexMacros })
    } catch {
      return `<span class="katex-error">${formula}</span>`
    }
  })

  // 4. 处理 Markdown 图片语法 ![alt](url)
  html = html.replace(/!\[([^\]]*)\]\(([^)]+)\)/g, (match, alt, url) => {
    const decodedUrl = url
      .replace(/&amp;/g, '&')
      .replace(/&lt;/g, '<')
      .replace(/&gt;/g, '>')
      .replace(/&quot;/g, '"')
      .replace(/&#39;/g, "'")
    const decodedAlt = alt
      .replace(/&amp;/g, '&')
      .replace(/&lt;/g, '<')
      .replace(/&gt;/g, '>')
      .replace(/&quot;/g, '"')
      .replace(/&#39;/g, "'")
    return `<img src="${decodedUrl}" alt="${decodedAlt}" class="latex-img" loading="lazy" />`
  })

  // 5. 处理换行 — 小问徽章模式使用 <p> 段落包裹以拉开间距
  if (props.subQuestionBadge && !props.inline) {
    // 保护已渲染的 KaTeX HTML — 其输出可能包含 \n，直接替换会截断 DOM 导致乱码
    const katexStore: string[] = []
    html = html.replace(/<span class="katex[^"]*">[\s\S]*?<\/span>/g, (m) => {
      const i = katexStore.length
      katexStore.push(m)
      return `\x00KATEX${i}\x00`
    })
    // 同样保护 img 标签
    html = html.replace(/<img[^>]*>/g, (m) => {
      const i = katexStore.length
      katexStore.push(m)
      return `\x00KATEX${i}\x00`
    })
    // 按换行分割段落
    html = `<p>${html.replace(/\n/g, '</p><p>')}</p>`
    html = html.replace(/<p>\s*<\/p>/g, '')
    // 恢复 KaTeX HTML 和 img 标签
    html = html.replace(/\x00KATEX(\d+)\x00/g, (_, i) => katexStore[parseInt(i)])
  } else {
    html = html.replace(/\n/g, '<br>')
  }

  // 6. 行内图片修正：如果 img 前后是 <br> 或字符串首尾，则保持 block 样式；
  //    否则添加 inline 类。需要在 DOM 插入后处理。
  container.value.innerHTML = html

  // 7. 后处理：区分块级和行内图片
  const imgs = container.value.querySelectorAll('img.latex-img')
  imgs.forEach((img) => {
    const prev = img.previousSibling
    const next = img.nextSibling
    const isBlock =
      (!prev || (prev.nodeName === 'BR')) &&
      (!next || (next.nodeName === 'BR'))
    if (isBlock) {
      img.classList.add('img-block')
      // 清除图片前后的 <br>（块级图片自带 margin，不需要额外换行）
      if (prev?.nodeName === 'BR') prev.remove()
      if (next?.nodeName === 'BR') next.remove()
    } else {
      img.classList.add('img-inline')
    }
  })
}

onMounted(render)
watch(() => [props.text, props.inline, props.subQuestionBadge], render)
</script>

<style>
.latex-render {
  line-height: 1.8;
  font-family: var(--font-cn-isolated);
}
.latex-render.latex-inline {
  display: inline;
}
.latex-render .katex-error {
  color: #e74c3c;
  border-bottom: 1px dashed #e74c3c;
}

/* 行间公式（$$...$$）：左对齐+缩进，提升长篇推导的阅读连贯性 */
.latex-render .katex-display {
  margin: 12px 0 !important;
  line-height: 1;
  overflow-x: auto;
  padding: 4px 0 4px 32px;
  text-align: left !important;
}
/* 行间公式自带上下 margin，隐藏公式前后的 <br> 避免额外空行。
   br:has(+ .katex-display) 隐藏公式前的 <br>，
   .katex-display + br 隐藏公式后的 <br> */
.latex-render .katex-display + br,
.latex-render br:has(+ .katex-display) {
  display: none;
}

/* 块级图片：苹果级极简视觉 */
.latex-render img.latex-img.img-block {
  max-width: 80%;
  max-height: 220px;
  display: block;
  margin: 12px auto;
  border-radius: 6px;
  border: 1px solid #f0f0f0;
}

/* 行内图片 */
.latex-render img.latex-img.img-inline {
  display: inline-block;
  vertical-align: middle;
  margin: 0 4px;
  max-height: 1.5em;
  border-radius: 3px;
}

[data-theme='dark'] .latex-render img.latex-img {
  border-color: rgba(255, 255, 255, 0.08);
}

/* ============ 小问数字徽章 ============ */
.latex-render .sub-question-badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  background: #0071e3;
  color: #ffffff;
  border-radius: 50%;
  font-size: 12px;
  font-weight: 600;
  margin-right: 8px;
  transform: translateY(-1px);
  box-shadow: 0 2px 6px rgba(0, 113, 227, 0.3);
  flex-shrink: 0;
}

[data-theme='dark'] .latex-render .sub-question-badge {
  background: #0a84ff;
  box-shadow: 0 2px 6px rgba(10, 132, 255, 0.3);
}

/* 小问徽章模式的段落间距 */
.latex-render p {
  margin: 0 0 16px;
  line-height: 1.8;
}

.latex-render p:last-child {
  margin-bottom: 0;
}
</style>
