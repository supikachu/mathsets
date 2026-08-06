<template>
  <div ref="container" class="latex-render" :class="{ 'latex-inline': inline }" />
</template>

<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount, watch } from 'vue'
import katex from 'katex'
import 'viewerjs/dist/viewer.css'
import Viewer from 'viewerjs'

const props = defineProps<{
  text: string
  inline?: boolean
  subQuestionBadge?: boolean
}>()

const container = ref<HTMLElement>()
// Viewer 实例 —— 单组件单实例，动态文本变更时调用 update() 重新绑定
let viewer: Viewer | null = null

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
 * 图片 src scheme 白名单：
 * 仅放行 http/https（含协议相对 //）、站内相对路径、安全的 base64 位图、blob: 本地预览。
 * 拒绝 javascript: / vbscript: / data:text/html / data:image/svg+xml 等可执行载荷。
 */
function isSafeImageSrc(src: string): boolean {
  if (/^(https?:)?\/\//i.test(src)) return true // https:// 或 //cdn.example.com
  if (/^\//.test(src)) return true              // /uploads/xxx 站内绝对路径
  if (/^\.{1,2}\//.test(src)) return true       // ./ ../ 相对路径
  if (/^data:image\/(png|jpe?g|gif|webp|bmp);base64,/i.test(src)) return true
  // blob: URL —— 浏览器本地预览（同源内存对象引用，不可执行 JS，安全）
  // 用于 QuestionEdit 上传本地图片后生成的 blob:http://localhost:5173/...
  if (/^blob:/i.test(src)) return true
  return false
}

/**
 * 将 /uploads/... 相对路径补全为可访问的完整 URL。
 *
 * 解决场景：
 *   后端 uploadImage 返回 `/uploads/questions/xxx.png` 相对路径，
 *   浏览器在 http://localhost:5173 (Vite dev) 上将其解析为
 *   http://localhost:5173/uploads/... → 404（Vite proxy 已配置可拦截，
 *   但生产环境若前后端不同源，仍会失败）。
 *
 * 策略：
 *   - 绝对 URL (http://, https://, //, blob:, data:) → 原样返回
 *   - /uploads/... → 若设置了 VITE_API_BASE_URL 则拼上后端域名，否则保持相对路径
 *     （dev 模式下由 vite.config.ts 中的 /uploads 代理处理；
 *      prod 模式下需在 .env 中设置 VITE_API_BASE_URL=http://backend-host）
 *
 * 注意：fallback 不使用硬编码 localhost:3000 —— 避免构建产物在
 *      不同环境（dev/test/prod）下绑死单一后端地址，遵循 12-factor 配置外置原则。
 */
function resolveImageUrl(url: string): string {
  // 绝对 URL 或协议相对 URL 或 blob:/data: → 不处理
  if (/^(https?:)?\/\//i.test(url)) return url
  if (/^(blob:|data:)/i.test(url)) return url
  // /uploads/ 相对路径 → 按 VITE_API_BASE_URL 拼接
  if (url.startsWith('/uploads/')) {
    const base = import.meta.env.VITE_API_BASE_URL || ''
    if (base) {
      // 去掉 base 末尾的斜杠，避免双斜杠
      return `${base.replace(/\/$/, '')}${url}`
    }
  }
  // 其他相对路径（./ ../）或未设置 base → 原样返回
  return url
}

/**
 * 智能豁免判定：是否需要禁止深色模式反色。
 *
 * 判定规则（满足任一即豁免）：
 *   1. URL 后缀为 .jpg / .jpeg（不区分大小写）—— JPEG 多为实拍图/扫描件，
 *      本身有完整白底与彩色信息，反色会破坏内容
 *   2. alt 文本或 URL 字段中出现 `=no-invert` 标记 —— 用户显式声明豁免
 *
 * 用法示例：
 *   ![配图 =no-invert](url)            —— alt 标记豁免
 *   ![配图](url =no-invert)            —— URL 字段标记豁免（兼容 Markdown title 位置）
 *   ![配图](https://example.com/a.jpg) —— 后缀自动豁免
 *   ![配图](https://example.com/a.png) —— 默认不豁免，深色模式反色
 */
function shouldDisableInvert(alt: string, url: string, urlField: string): boolean {
  // JPEG 后缀豁免（兼容 URL 后带 query string 的情况）
  if (/\.(jpe?g)(\?.*)?$/i.test(url)) return true
  // 显式标记豁免：alt / url / 完整 url 字段中任一含 =no-invert
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
      // 【安全】显式 trust:false —— 拒绝 \href / \htmlClass / \htmlId 等
      //   生成 HTML 的命令（默认即 false，此处显式声明防御未来改动）
      trust: false,
      strict: 'warn', // 非法命令仅警告不执行
      macros: katexMacros, // 固定宏映射（无用户输入参与宏定义）
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

  // 处理 Markdown 图片语法 ![alt](url) 或 ![alt](url =no-invert)
  // 【安全】阶段 2 的 escapeHtml 使 alt/url 处于实体态；此处解码后
  //   必须对 alt 二次转义，否则可构造 alt="x" onerror="..." 属性逃逸。
  //   src 解码后做 scheme 白名单校验，拒绝 javascript: 等危险协议。
  // 【智能豁免】检测 =no-invert 标记或 JPEG 后缀，输出 data-no-invert="true"
  //   供深色模式 CSS :not([data-no-invert="true"]) 排除反色。
  html = html.replace(/!\[([^\]]*)\]\(([^)]+)\)/g, (match, alt, urlField) => {
    const decode = (s: string) => s
      .replace(/&amp;/g, '&')
      .replace(/&lt;/g, '<')
      .replace(/&gt;/g, '>')
      .replace(/&quot;/g, '"')
      .replace(/&#39;/g, "'")
    const decodedAlt = decode(alt)
    const decodedUrlField = decode(urlField)

    // 拆分 URL 与可选的尾部标记（如 "url =no-invert" 或 "url 'title'"）
    // 第一个 token 是真正的 URL，其余视为标记，便于后续豁免判定
    const tokens = decodedUrlField.split(/\s+/)
    const decodedUrl = tokens[0]
    const trailingMarker = tokens.slice(1).join(' ')

    // 非白名单 URL：降级为转义文本，不渲染任何 img 标签
    if (!isSafeImageSrc(decodedUrl)) {
      return `<span class="latex-img-invalid">${escapeHtml(match)}</span>`
    }

    // 智能豁免判定：JPEG 后缀 OR 显式 =no-invert 标记
    // 注意传给 shouldDisableInvert 的是 decodedUrlField（完整字段）而非 trailingMarker，
    // 因为 =no-invert 也可能直接拼接在 URL query 中（如 url?flag=no-invert=true）
    const noInvert = shouldDisableInvert(decodedAlt, decodedUrl, decodedUrlField)
    const noInvertAttr = noInvert ? ' data-no-invert="true"' : ''

    // URL 智能补全：/uploads/... 在 dev 模式下保持相对路径（Vite proxy 处理），
    // 在 prod 模式下（设置 VITE_API_BASE_URL）拼接后端域名
    const finalUrl = resolveImageUrl(decodedUrl)

    // 属性值二次转义：阻断 " ' < > & 全部实体化，杜绝属性逃逸
    return `<img src="${escapeHtml(finalUrl)}" alt="${escapeHtml(decodedAlt)}" class="latex-img" loading="lazy" decoding="async" referrerpolicy="no-referrer"${noInvertAttr} />`
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

/**
 * 渲染 + Viewer 同步更新：
 * 由于 render() 通过 innerHTML 重置了容器内容，旧 img 的 click 监听
 * 随节点销毁自动释放，无内存泄漏；新 img 节点需要 viewer.update()
 * 重新扫描并绑定 click 监听，才能支持点击放大。
 */
function renderAndViewerUpdate() {
  render()
  viewer?.update()
}

/**
 * 图片点击事件拦截器：阻止 click 事件冒泡到外层容器。
 *
 * 解决场景：
 *   QuestionList 中题目卡片整体 @click 触发路由跳转到详情页，
 *   点击题干内的图片时，Viewer.js 灯箱已打开（容器层监听已先触发），
 *   但事件继续冒泡到外层卡片，导致同时跳转详情页。
 *
 * 注意：
 *   - 使用 stopPropagation() 而非 stopImmediatePropagation()，
 *     仅阻断冒泡，不影响同一 container 上 Viewer.js 的 click 监听器
 *   - 通过事件委托在 container 上单点监听，动态新增图片无需重新绑定
 */
const handleImageClick = (e: MouseEvent) => {
  const target = e.target as HTMLElement
  if (target && target.tagName === 'IMG' && target.classList.contains('latex-img')) {
    e.stopPropagation()  // 阻止冒泡到外层题卡的 @click 路由跳转
  }
}

onMounted(() => {
  render()
  // 初始化 Viewer：作用于当前组件 root container
  // filter 仅绑定 .latex-img，避免误捕获 KaTeX 输出中的 SVG 等
  if (container.value) {
    viewer = new Viewer(container.value, {
      // 显式标注 img: HTMLImageElement —— viewerjs 自带类型把 filter 声明为 Function，
      // 无法推断参数类型，需手动标注避免 noImplicitAny 报错
      filter: (img: HTMLImageElement) => img.classList.contains('latex-img'),
      toolbar: {
        zoomIn: true,
        zoomOut: true,
        oneToOne: true,
        reset: true,
        rotate: true,
      },
      title: false,    // 不显示 alt 文字浮层（数学题 alt 多含敏感标记如 =no-invert）
      tooltip: false,  // 不显示缩放百分比提示
    })
    // 拦截图片点击事件冒泡，防止触发外层题目卡片的路由跳转
    container.value.addEventListener('click', handleImageClick)
  }
})

// 文本/模式动态变化时，先重渲染再 update Viewer，确保新增图片可点击放大
watch(() => [props.text, props.inline, props.subQuestionBadge], renderAndViewerUpdate)

onBeforeUnmount(() => {
  // 销毁 Viewer 释放事件监听，防止组件卸载后残留引用
  viewer?.destroy()
  viewer = null
  // 移除点击事件监听器，防止内存泄漏
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
/* 行间公式自带上下 margin，隐藏公式前后的 <br> 避免额外空行。
   br:has(+ .katex-display) 隐藏公式前的 <br>，
   .katex-display + br 隐藏公式后的 <br> */
.latex-render .katex-display + br,
.latex-render br:has(+ .katex-display) {
  display: none;
}

/* ============ 图片样式（阶段二放宽尺寸 + Lightbox 鼠标提示） ============ */

/* 块级图片：放宽至 100% 宽 / 480px 高，确保复杂坐标系/连通器等宽幅图充分展示 */
.latex-render img.latex-img.img-block {
  max-width: 100%;
  max-height: 480px;
  display: block;
  margin: 12px auto;
  border-radius: 6px;
  border: 1px solid #f0f0f0;
  cursor: zoom-in;  /* 提示用户可点击放大查看几何细节 */
}

/* 行内图片 */
.latex-render img.latex-img.img-inline {
  display: inline-block;
  vertical-align: middle;
  margin: 0 4px;
  max-height: 1.5em;
  border-radius: 3px;
  cursor: zoom-in;
}

[data-theme='dark'] .latex-render img.latex-img {
  border-color: rgba(255, 255, 255, 0.08);
}

/* ============ 深色模式智能反色（阶段二核心） ============
 * 痛点：数学题配图多为「透明背景 + 黑色线条」的几何图/坐标系，
 *       深色模式下黑色线条与深色背景融合，完全不可见。
 * 策略：
 *   - 默认反色 (filter: invert(1) hue-rotate(180deg)) + 浅灰底兜底
 *   - 通过 :not([data-no-invert="true"]) 排除 JPEG 实拍图与显式标记的图
 *   - hue-rotate(180deg) 让彩色图反色后色相保持原貌（红→青等互补色）
 *   - 浅灰底 #f5f5f7 防止半透明 PNG 反色后发灰
 */
[data-theme='dark'] .latex-render img.latex-img:not([data-no-invert="true"]) {
  filter: invert(1) hue-rotate(180deg);
  background: #f5f5f7;
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
