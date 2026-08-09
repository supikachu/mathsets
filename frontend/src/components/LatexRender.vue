<template>
  <div
    ref="container"
    class="latex-render"
    :class="{ 'latex-inline': inline, 'editable-mode': mode === 'editable' }"
  />
</template>

<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount, watch } from 'vue'
import katex from 'katex'

export interface ImageConfig {
  width?: number
  align?: 'left' | 'center' | 'right'
}

export interface ImageClickPayload {
  target: HTMLImageElement
  url: string
  mdId: string
  config: ImageConfig
}

const props = defineProps<{
  text: string
  inline?: boolean
  subQuestionBadge?: boolean
  mode?: 'readonly' | 'editable'
}>()

const emit = defineEmits<{
  (e: 'image-click', payload: ImageClickPayload): void
}>()

const container = ref<HTMLElement>()

// 全局 KaTeX 宏：将 \emptyset 映射为 \varnothing，符合国内教材椭圆空集符号
const katexMacros = {
  '\\emptyset': '\\varnothing',
}

// 图片 ID 生成计数器 —— 组件级，避免频繁 render 导致 ID 冲突
let imageCounter = 0

// 将公式中的 Unicode 空集符号 ∅ (U+2205) 替换为 \varnothing
// KaTeX macros 只对 LaTeX 命令生效，Unicode 字符需预处理
function normalizeEmptyset(s: string): string {
  return s.replace(/\u2205/g, '\\varnothing')
}

/**
 * 将国内教材中的圆弧变体命令统一替换为 \htmlClass{math-arc}{...}
 *
 * KaTeX 原生的 \overgroup 虽能拉伸但两端带向下的"倒钩"，
 * 不符合国内初高中教材中的平滑几何圆弧规范。
 * 业界最高级方案：注入 CSS 类，用 border-radius 绘制无倒钩圆弧。
 *
 * 覆盖的圆弧变体：
 *   \overset{\frown}{AB}  —— 通用圆弧标记
 *   \overparen{AB}        —— 数学圆弧（部分宏包）
 *   \wideparen{AB}        —— 宽圆弧（部分宏包）
 *   \overgroup{AB}        —— KaTeX 原生（带倒钩）
 */
function normalizeArcs(s: string): string {
  return s.replace(
    /\\(?:overset\s*\{?\\frown\}?|overparen|wideparen|overgroup)\s*\{([^}]+)\}/g,
    '\\htmlClass{math-arc}{$1}',
  )
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
 * 图片 src scheme 白名单：
 * 仅放行 http/https（含协议相对 //）、站内相对路径、安全的 base64 位图、blob: 本地预览。
 * 拒绝 javascript: / vbscript: / data:text/html / data:image/svg+xml 等可执行载荷。
 */
function isSafeImageSrc(src: string): boolean {
  if (/^(https?:)?\/\//i.test(src)) return true
  if (/^\//.test(src)) return true
  if (/^\.{1,2}\//.test(src)) return true
  if (/^images\//i.test(src)) return true // 兼容老数据（后端新数据已替换为 /uploads/...）
  if (/^data:image\/(png|jpe?g|gif|webp|bmp);base64,/i.test(src)) return true
  if (/^blob:/i.test(src)) return true
  return false
}

/**
 * 将 /uploads/... 相对路径补全为可访问的完整 URL。
 * 策略：
 *   - 绝对 URL 或 blob:/data: → 原样返回
 *   - /uploads/... → 若设置了 VITE_API_BASE_URL 则拼上后端域名，否则保持相对路径
 */
function resolveImageUrl(url: string): string {
  if (/^(https?:)?\/\//i.test(url)) return url
  if (/^(blob:|data:)/i.test(url)) return url
  if (url.startsWith('/uploads/')) {
    const base = import.meta.env.VITE_API_BASE_URL || ''
    if (base) {
      return `${base.replace(/\/$/, '')}${url}`
    }
    return url
  }
  // 兼容老数据：images/xxx.png 前缀补 /（变成 /images/...），由后端静态资源兜底
  // 注：新数据后端已替换为 /uploads/questions/{uuid}.ext，本分支仅用于回看历史遗留题目
  if (/^images\//i.test(url)) {
    const base = import.meta.env.VITE_API_BASE_URL || ''
    const full = base ? `${base.replace(/\/$/, '')}/${url}` : `/${url}`
    return full
  }
  return url
}

/**
 * 智能豁免判定：是否需要禁止深色模式反色。
 * 判定规则（满足任一即豁免）：
 *   1. URL 后缀为 .jpg / .jpeg（但 /uploads/questions/ 题目配图目录例外，因 MinerU 提取的 PDF 图多为 jpg 却是黑白几何图）
 *   2. alt 文本或 URL 字段中出现 `=no-invert` 标记
 */
function shouldDisableInvert(alt: string, url: string, urlField: string): boolean {
  if (/\.(jpe?g)(\?.*)?$/i.test(url)) {
    // 题目配图目录（手动上传 + AI 录入）下的 JPEG 不豁免：数学题配图多为黑白几何图/坐标系，应反色
    // 其他目录（如 /uploads/avatars/）的 JPEG 保持豁免：实拍图/照片反色会失真
    if (!url.includes('/uploads/questions/')) {
      return true
    }
  }
  if (
    alt.includes('=no-invert') ||
    url.includes('=no-invert') ||
    urlField.includes('=no-invert')
  ) {
    return true
  }
  return false
}

/**
 * 解析 Markdown 扩展配置字符串 {width:300, align:left}
 * 返回结构化的 ImageConfig 对象。
 */
function parseImageConfig(configStr: string | undefined): ImageConfig {
  const config: ImageConfig = {}
  if (!configStr) return config

  const widthMatch = configStr.match(/width:\s*(\d+)/i)
  if (widthMatch) config.width = parseInt(widthMatch[1], 10)

  const alignMatch = configStr.match(/align:\s*(left|center|right)/i)
  if (alignMatch) config.align = alignMatch[1].toLowerCase() as 'left' | 'center' | 'right'

  return config
}

/**
 * 根据 ImageConfig 生成内联 style 属性字符串。
 *
 * 核心业务约束：
 *   1. 排版极简：display: block + margin: auto 实现对齐，禁止 float
 *   2. 等比例缩放：只改 width，强制 height: auto
 *   3. 防溢出：max-width: 100%
 */
function buildImageStyle(config: ImageConfig): string {
  let style = 'display: block; height: auto; max-width: 100%;'

  if (config.width) {
    style += ` width: ${config.width}px;`
  }

  switch (config.align) {
    case 'left':
      style += ' margin: 12px auto 12px 0;'
      break
    case 'right':
      style += ' margin: 12px 0 12px auto;'
      break
    case 'center':
    default:
      style += ' margin: 12px auto;'
      break
  }

  return style
}

/**
 * 从 <img> 的 style 属性反向解析出 ImageConfig。
 * 用于 editable 模式下点击图片时，回传当前配置给调节面板。
 */
function parseStyleToConfig(styleStr: string): ImageConfig {
  const config: ImageConfig = {}

  const widthMatch = styleStr.match(/width:\s*(\d+)px/i)
  if (widthMatch) config.width = parseInt(widthMatch[1], 10)

  if (/margin:[^;]*auto 12px 0/i.test(styleStr)) {
    config.align = 'left'
  } else if (/margin:[^;]*0 12px auto/i.test(styleStr)) {
    config.align = 'right'
  } else {
    config.align = 'center'
  }

  return config
}

/**
 * 渲染单个公式为 KaTeX HTML。
 * 【关键】传入的 formula 必须是 raw string（未经 HTML 转义），
 * 这样 KaTeX 才能正确识别 \sqrt、\frac、\text 等 LaTeX 命令。
 */
function renderKatex(formula: string, displayMode: boolean): string {
  try {
    const raw = normalizeArcs(normalizeEmptyset(formula.trim()))
    return katex.renderToString(raw, {
      displayMode,
      throwOnError: false,
      trust: (context: { command: string }) => context.command === '\\htmlClass',
      strict: false,
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
  // ============================================================

  // ---- 阶段 1: 提取公式，留下纯字母数字占位符 ----
  const mathStore: { formula: string; displayMode: boolean }[] = []
  let html = text

  html = html.replace(/\$\$([\s\S]+?)\$\$/g, (_, formula) => {
    const i = mathStore.length
    mathStore.push({ formula, displayMode: true })
    return `__MATH_PLACEHOLDER_${i}__`
  })

  html = html.replace(/\$([\s\S]+?)\$/g, (_, formula) => {
    const i = mathStore.length
    mathStore.push({ formula, displayMode: false })
    return `__MATH_PLACEHOLDER_${i}__`
  })

  // ---- 阶段 2: 对纯文本做 escapeHtml + 换行格式化 ----
  html = escapeHtml(html)

  // 小问徽章处理
  if (props.subQuestionBadge) {
    html = html.replace(/\((\d+)\)|（(\d+)）/g, (_, half, full) => {
      return `<span class="sub-question-badge">${half || full}</span>`
    })
  }

  // 处理 Markdown 图片语法 ![alt](url){config} 或 ![alt](url)
  // 【新特性】支持可选的 {} 扩展属性：{width:300, align:left}
  // 【安全】阶段 2 的 escapeHtml 使 alt/url 处于实体态；此处解码后
  //   必须对 alt 二次转义，否则可构造 alt="x" onerror="..." 属性逃逸。
  //   src 解码后做 scheme 白名单校验，拒绝 javascript: 等危险协议。
  html = html.replace(
    /!\[([^\]]*)\]\(([^)]+)\)(?:\{([^}]*)\})?/g,
    (match, alt, urlField, configStr) => {
      const decode = (s: string) => s
        .replace(/&amp;/g, '&')
        .replace(/&lt;/g, '<')
        .replace(/&gt;/g, '>')
        .replace(/&quot;/g, '"')
        .replace(/&#39;/g, "'")
      const decodedAlt = decode(alt)
      const decodedUrlField = decode(urlField)

      // 拆分 URL 与可选的尾部标记（如 "url =no-invert"）
      const tokens = decodedUrlField.split(/\s+/)
      const decodedUrl = tokens[0]

      if (!isSafeImageSrc(decodedUrl)) {
        return `<span class="latex-img-invalid">${escapeHtml(match)}</span>`
      }

      const noInvert = shouldDisableInvert(decodedAlt, decodedUrl, decodedUrlField)
      const noInvertAttr = noInvert ? ' data-no-invert="true"' : ''

      const finalUrl = resolveImageUrl(decodedUrl)

      // 解析 {width:300, align:left} 配置
      const config = parseImageConfig(configStr)
      const hasConfig = configStr && configStr.trim().length > 0
      const style = buildImageStyle(config)

      // 生成唯一 mdId，用于 editable 模式下定位回写
      const mdId = `img_${Date.now().toString(36)}_${(imageCounter++).toString(36)}`
      // 带 {} 配置的强制为块级图片
      const blockClass = hasConfig ? 'img-block' : ''

      return `<img src="${escapeHtml(finalUrl)}" alt="${escapeHtml(decodedAlt)}" class="latex-img ${blockClass}" data-md-id="${mdId}" style="${style}" loading="lazy" decoding="async" referrerpolicy="no-referrer"${noInvertAttr} />`
    },
  )

  // 处理换行
  if (props.subQuestionBadge && !props.inline) {
    html = `<p>${html.replace(/\n/g, '</p><p>')}</p>`
    html = html.replace(/<p>\s*<\/p>/g, '')
  } else {
    html = html.replace(/\n/g, '<br>')
  }

  // ---- 阶段 3: 最后才渲染 KaTeX，直接替换占位符 ----
  for (let i = 0; i < mathStore.length; i++) {
    const { formula, displayMode } = mathStore[i]
    const katexHtml = renderKatex(formula, displayMode)
    html = html.split(`__MATH_PLACEHOLDER_${i}__`).join(katexHtml)
  }

  // 设置 innerHTML
  container.value.innerHTML = html

  // 后处理：为无 {} 配置的图片区分块级和行内（带配置的已强制块级）
  const imgs = container.value.querySelectorAll('img.latex-img:not(.img-block)')
  imgs.forEach((img) => {
    const prev = img.previousSibling
    const next = img.nextSibling
    const isBlock =
      (!prev || (prev.nodeName === 'BR')) &&
      (!next || (next.nodeName === 'BR'))
    if (isBlock) {
      img.classList.add('img-block')
      // 应用默认块级样式（无 width 配置）
      const style = buildImageStyle({})
      img.setAttribute('style', style)
      if (prev?.nodeName === 'BR') prev.remove()
      if (next?.nodeName === 'BR') next.remove()
    } else {
      img.classList.add('img-inline')
    }
  })
}

/**
 * 图片点击事件处理：
 * - readonly 模式：不拦截，让事件自然冒泡（列表页跳转详情/详情页无副作用）
 * - editable 模式：拦截冒泡，派发 image-click 事件给父组件
 */
const handleImageClick = (e: MouseEvent) => {
  const target = e.target as HTMLElement
  const isImg = target?.tagName === 'IMG' && target.classList.contains('latex-img')

  if (!isImg) return

  if (props.mode === 'editable') {
    // 编辑模式：拦截冒泡，派发事件
    e.preventDefault()
    e.stopPropagation()

    const imgEl = target as HTMLImageElement
    const mdId = imgEl.getAttribute('data-md-id') || ''
    const url = imgEl.getAttribute('src') || ''
    const styleStr = imgEl.getAttribute('style') || ''
    const config = parseStyleToConfig(styleStr)

    emit('image-click', { target: imgEl, url, mdId, config })
  }
  // readonly 模式：不拦截，让事件自然冒泡
}

onMounted(() => {
  render()
  if (container.value) {
    container.value.addEventListener('click', handleImageClick)
  }
})

// 文本/模式动态变化时重新渲染
watch(() => [props.text, props.inline, props.subQuestionBadge], render)

onBeforeUnmount(() => {
  if (container.value) {
    container.value.removeEventListener('click', handleImageClick)
  }
})
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
.latex-render .katex-display + br,
.latex-render br:has(+ .katex-display) {
  display: none;
}

/* ============ 图片样式 ============ */

/* 全局兜底：防止 inline style 未应用时图片溢出 */
.latex-render img.latex-img {
  max-width: 100%;
  height: auto;
}

/* 块级图片：由内联 style 控制 width/margin，CSS 仅提供边框和圆角 */
.latex-render img.latex-img.img-block {
  border-radius: 6px;
  border: 1px solid #f0f0f0;
}

/* 行内图片（无 {} 配置且非独立成行） */
.latex-render img.latex-img.img-inline {
  display: inline-block;
  vertical-align: middle;
  margin: 0 4px;
  max-height: 1.5em;
  max-width: 100%;
  border-radius: 3px;
}

[data-theme='dark'] .latex-render img.latex-img {
  border-color: rgba(255, 255, 255, 0.08);
}

/* ============ 深色模式智能反色 ============
 * 痛点：数学题配图多为「透明背景 + 黑色线条」的几何图/坐标系，
 *       深色模式下黑色线条与深色背景融合，完全不可见。
 * 策略：
 *   - 默认反色 (filter: invert(1) hue-rotate(180deg)) + 浅灰底兜底
 *   - 通过 :not([data-no-invert="true"]) 排除 JPEG 实拍图与显式标记的图
 */
[data-theme='dark'] .latex-render img.latex-img:not([data-no-invert="true"]) {
  filter: invert(1) hue-rotate(180deg);
  background: #f5f5f7;
}

/* ============ editable 模式交互提示 ============ */
.latex-render.editable-mode img.latex-img.img-block {
  cursor: pointer;
  transition: outline 0.2s;
}
.latex-render.editable-mode img.latex-img.img-block:hover {
  outline: 2px solid var(--primary-color, #0071e3);
  outline-offset: 2px;
}

/* ============ 教材级 CSS 圆弧（替代 KaTeX \overgroup 倒钩） ============ */
.latex-render .math-arc {
  position: relative;
  display: inline-block;
  padding-top: 0.35em;
}
.latex-render .math-arc::before {
  content: "";
  position: absolute;
  top: 0.08em;
  left: 0;
  right: 0;
  height: 0.25em;
  border-top: 0.07em solid currentColor;
  border-radius: 50% 50% 0 0 / 100% 100% 0 0;
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

.latex-render p {
  margin: 0 0 16px;
  line-height: 1.8;
}

.latex-render p:last-child {
  margin-bottom: 0;
}
</style>
