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

/**
 * 渲染单个公式为 KaTeX HTML。
 * 【关键】传入的 formula 必须是 raw string（未经 HTML 转义），
 * 这样 KaTeX 才能正确识别 \sqrt、\frac、\text 等 LaTeX 命令。
 * 任何反斜杠都不应在传给 KaTeX 之前被处理。
 */
function renderKatex(formula: string, displayMode: boolean): string {
  try {
    const raw = normalizeEmptyset(formula.trim())
    return katex.renderToString(raw, {
      displayMode,
      throwOnError: false,
      macros: katexMacros,
    })
  } catch {
    return `<span class="katex-error">${escapeHtml(formula)}</span>`
  }
}

function render() {
  if (!container.value) return
  const text = props.text || ''

  // ============================================================
  // 安全渲染生命周期（严格三阶段）：
  //   阶段 1: 提取公式 → 文本中只剩纯字母数字占位符
  //   阶段 2: 对纯文本做 escapeHtml + 换行格式化（此时无 KaTeX HTML）
  //   阶段 3: 最后才调用 katex.renderToString，输出绝不参与任何 .replace
  //
  // 【为什么不能用正则保护 KaTeX HTML】
  //   旧实现先渲染 KaTeX 再用 /<span class="katex[^"]*">[\s\S]*?<\/span>/g
  //   保护其输出。但 KaTeX 生成的是嵌套 <span> 结构，非贪婪匹配在遇到第一个
  //   </span> 时就会停止，导致内部的 <svg>/<path> 完全暴露，依然被注入 <br>。
  //   彻底解决：调换执行顺序，让 katex.renderToString 成为最后一步。
  // ============================================================

  // ---- 阶段 1: 提取公式，留下纯字母数字占位符 ----
  // 使用 __MATH_PLACEHOLDER_N__ 作为占位符（仅字母+下划线+数字），
  // 不会被 escapeHtml 改写，也不会被小问徽章正则误匹配。
  const mathStore: { formula: string; displayMode: boolean }[] = []
  let html = text

  // 先提取块级公式 $$...$$ （使用 [\s\S] 支持跨行公式）
  html = html.replace(/\$\$([\s\S]+?)\$\$/g, (_, formula) => {
    const i = mathStore.length
    mathStore.push({ formula, displayMode: true })
    return `__MATH_PLACEHOLDER_${i}__`
  })

  // 再提取行内公式 $...$ （使用 [\s\S] 支持跨行公式）
  html = html.replace(/\$([\s\S]+?)\$/g, (_, formula) => {
    const i = mathStore.length
    mathStore.push({ formula, displayMode: false })
    return `__MATH_PLACEHOLDER_${i}__`
  })

  // ---- 阶段 2: 对纯文本做 escapeHtml + 换行格式化 ----
  //    此时文本中只有普通内容 + 占位符，无任何 KaTeX HTML，
  //    可以安全地执行任何字符串替换。
  html = escapeHtml(html)

  // 小问徽章处理（占位符是 __MATH_PLACEHOLDER_N__，不含括号数字，不会被误匹配）
  if (props.subQuestionBadge) {
    html = html.replace(/\((\d+)\)|（(\d+)）/g, (_, half, full) => {
      return `<span class="sub-question-badge">${half || full}</span>`
    })
  }

  // 处理 Markdown 图片语法 ![alt](url)
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

  // 处理换行 — 此时 KaTeX 尚未渲染，img 标签也不会被影响
  if (props.subQuestionBadge && !props.inline) {
    // 小问徽章模式：用 <p> 段落包裹以拉开段落间距
    html = `<p>${html.replace(/\n/g, '</p><p>')}</p>`
    html = html.replace(/<p>\s*<\/p>/g, '')
  } else {
    // 普通模式：\n → <br>
    html = html.replace(/\n/g, '<br>')
  }

  // ---- 阶段 3: 最后才渲染 KaTeX，直接替换占位符 ----
  //    katex.renderToString 的输出直接拼入 html，绝不参与任何 .replace。
  //    这样 SVG <path> 的 d 属性中的 \n 不会被替换为 <br>，根号得以保留。
  for (let i = 0; i < mathStore.length; i++) {
    const { formula, displayMode } = mathStore[i]
    const katexHtml = renderKatex(formula, displayMode)
    // 使用 split + join 避免正则特殊字符问题（占位符是纯字母数字，正则也安全，
    // 但 split/join 更直观且绝不触发任何意外匹配）
    html = html.split(`__MATH_PLACEHOLDER_${i}__`).join(katexHtml)
  }

  // 设置 innerHTML
  container.value.innerHTML = html

  // 后处理：区分块级和行内图片
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
