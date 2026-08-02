// LatexRender.vue XSS 修复验证：模拟阶段 2 转义 → 图片正则处理管线
// 与组件内 escapeHtml / isSafeImageSrc / 图片替换逻辑一致

function escapeHtml(s) {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

function isSafeImageSrc(src) {
  if (/^(https?:)?\/\//i.test(src)) return true
  if (/^\//.test(src)) return true
  if (/^\.{1,2}\//.test(src)) return true
  if (/^data:image\/(png|jpe?g|gif|webp|bmp);base64,/i.test(src)) return true
  return false
}

// 阶段 2 转义（整个文本）
function phase2Escape(text) {
  return escapeHtml(text)
}

// 图片正则处理（修复后逻辑）
function processImages(html) {
  const decode = (s) => s
    .replace(/&amp;/g, '&')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
  return html.replace(/!\[([^\]]*)\]\(([^)]+)\)/g, (match, alt, url) => {
    const decodedUrl = decode(url)
    const decodedAlt = decode(alt)
    if (!isSafeImageSrc(decodedUrl)) {
      return `<span class="latex-img-invalid">${escapeHtml(match)}</span>`
    }
    return `<img src="${escapeHtml(decodedUrl)}" alt="${escapeHtml(decodedAlt)}" class="latex-img" loading="lazy" />`
  })
}

let pass = 0, fail = 0
function check(name, cond) {
  if (cond) { pass++; console.log(`  PASS ${name}`) }
  else { fail++; console.log(`  FAIL ${name}`) }
}

// ── 用例 1：alt 属性逃逸（原漏洞）![x" onerror="alert(1)](url)
{
  const out = processImages(phase2Escape('![x" onerror="alert(1)](https://e.com/a.png)'))
  console.log('[1] alt 逃逸:', out)
  check('alt 中引号被实体化（无裸 " 逃逸）', out.includes('alt="x&quot; onerror=&quot;alert(1)"'))
  check('未出现裸 onerror 属性', !/onerror\s*=\s*"/.test(out))
  check('alt 属性值内无裸双引号（属性边界完整）', (out.match(/alt="[^"]*"/) || [''])[0] === 'alt="x&quot; onerror=&quot;alert(1)"')
}

// ── 用例 2：src javascript: 协议
{
  const out = processImages(phase2Escape('![x](javascript:alert(1))'))
  console.log('[2] javascript: src:', out)
  check('javascript: 被降级为文本（无 img 标签）', !out.includes('<img'))
  check('降级内容被转义', out.includes('latex-img-invalid'))
}

// ── 用例 3：data:image/svg+xml（可内嵌脚本）
{
  const out = processImages(phase2Escape('![x](data:image/svg+xml;base64,PHN2Zz48c2NyaXB0PmFsZXJ0KDEpPC9zY3JpcHQ+PC9zdmc+)'))
  console.log('[3] svg data URI:', out)
  check('svg data URI 被拒绝', !out.includes('<img'))
}

// ── 用例 4：合法 URL 正常渲染
{
  const out = processImages(phase2Escape('![图](https://cdn.example.com/a.png?x=1&y=2)'))
  console.log('[4] 合法 URL:', out)
  check('合法 https 图片正常渲染', out.includes('<img src="https://cdn.example.com/a.png?x=1&amp;y=2"'))
}

// ── 用例 5：站内相对路径
{
  const out = processImages(phase2Escape('![图](/uploads/avatar.png)'))
  console.log('[5] 相对路径:', out)
  check('站内路径正常渲染', out.includes('<img src="/uploads/avatar.png"'))
}

// ── 用例 6：alt 含 <script> 文本
{
  const out = processImages(phase2Escape('![<script>alert(1)</script>](https://e.com/a.png)'))
  console.log('[6] alt 含标签:', out)
  check('alt 中 < > 被实体化', out.includes('alt="&lt;script&gt;alert(1)&lt;/script&gt;"'))
}

console.log(`\n结果: ${pass} 通过 / ${fail} 失败`)
process.exit(fail === 0 ? 0 : 1)
