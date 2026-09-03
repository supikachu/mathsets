/**
 * 预览 SVG 的基础净化（B7，口径见实施计划 R14）
 *
 * typst 自己不发脚本，这一层防的是「将来某条链路把外部 SVG 塞进 `PreviewResponse.pages`」。
 * 三件事：解析失败就整页不注入（fail-closed —— 宁可白屏也不渲染来历不明的图）；删掉可执行
 * 载体（`<script>` 与 SMIL 那一族，`<set attributeName="onload">` 是现成的脚本入口）；删掉
 * 事件属性与 `javascript:` 链接。
 *
 * 不引 DOMPurify：为这一个函数多一整包依赖不值当。被删的形态都在 SVG 规范的脚本入口清单上，
 * 不依赖「哪些元素算安全」这种白名单判断，所以这份短名单是可辩护的。
 */

/** 可执行载体：脚本、外部 HTML、SMIL 动画（能改任意属性）、SVG 1.1 的脚本钩子 */
const FORBIDDEN = new Set([
  'script',
  'foreignobject',
  'iframe',
  'embed',
  'object',
  'animate',
  'animatetransform',
  'animatecolor',
  'animatemotion',
  'set',
  'handler',
  'listener',
])

/// 可执行的伪协议。`data:image/*` 是合法图片引用（typst 就往里嵌位图），不在其列。
function isExecutableUrl(value: string): boolean {
  const squashed = value.replace(/[\u0000-\u0020]/g, '')
  return /^(?:javascript|vbscript|livescript|data:text\/html):/i.test(squashed)
}

/**
 * 干净则返回可直接 `v-html` 注入的 SVG 源码，不安全 / 读不懂则返回 `null`（调用方须按
 * 「不显示」处理，不许退回注入原文）。
 */
export function sanitizeSvg(source: string): string | null {
  if (!source.trim()) return null
  let doc: Document
  try {
    doc = new DOMParser().parseFromString(source, 'image/svg+xml')
  } catch {
    return null
  }
  // 根元素不是 svg：要么畸形（Chrome 把 parsererror 当根）、要么压根不是 SVG 文档
  if (doc.querySelector('parsererror') || doc.documentElement.localName.toLowerCase() !== 'svg') {
    return null
  }

  for (const el of Array.from(doc.querySelectorAll('*'))) {
    if (FORBIDDEN.has(el.localName.toLowerCase())) {
      el.remove()
      continue
    }
    for (const attr of Array.from(el.attributes)) {
      const name = attr.name.toLowerCase()
      if (name.startsWith('on') || ((name === 'href' || name === 'xlink:href') && isExecutableUrl(attr.value))) {
        el.removeAttribute(attr.name)
      }
    }
  }
  return new XMLSerializer().serializeToString(doc.documentElement)
}

/**
 * SVG 根元素声明的自然宽度换算成 CSS 像素（缩放控件要按「实际尺寸 = 100%」标）。
 *
 * 只认根标签上的 `width="…"`，单位缺省按 px。读不出来就返回 `null`，调用方回退到「适应宽度」。
 */
export function intrinsicWidthPx(svg: string): number | null {
  const root = /^\s*<svg[^>]*>/i.exec(svg)
  if (!root) return null
  const raw = /\swidth\s*=\s*"([^"]+)"/i.exec(root[0])?.[1]
  if (!raw) return null
  const num = parseFloat(raw)
  if (!Number.isFinite(num) || num <= 0) return null
  const unit = (/(pt|mm|cm|in|px)$/i.exec(raw)?.[1] ?? 'px').toLowerCase()
  const perPx = { px: 1, pt: 96 / 72, in: 96, cm: 96 / 2.54, mm: 96 / 25.4 }[unit]
  return num * (perPx ?? 1)
}
