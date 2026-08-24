/** 解答题问树：类型与编号/遍历工具 */

export interface AnalysisBlock {
  id: string
  title: string
  content: string
}

export interface QuestionPart {
  id: string
  label: string
  stem: string
  children: QuestionPart[]
  answer?: string
  analyses: AnalysisBlock[]
  no_analysis_needed: boolean
  labelDirty: boolean
}

export interface QuestionStructure {
  version: 1
  parts: QuestionPart[]
}

export const MAX_PART_DEPTH = 2

const ARABIC = ['(1)', '(2)', '(3)', '(4)', '(5)', '(6)', '(7)', '(8)', '(9)', '(10)']
const ROMAN_LOWER = ['(i)', '(ii)', '(iii)', '(iv)', '(v)', '(vi)', '(vii)', '(viii)', '(ix)', '(x)']
const CN_NUMS = ['一', '二', '三', '四', '五', '六', '七', '八', '九', '十']

export function cnNum(n: number): string {
  return CN_NUMS[n - 1] || String(n)
}

export function newId(): string {
  return crypto.randomUUID()
}

export function newAnalysis(index = 1): AnalysisBlock {
  return { id: newId(), title: `解法${cnNum(index)}`, content: '' }
}

export function defaultLeaf(label = '(1)'): QuestionPart {
  return {
    id: newId(),
    label,
    stem: '',
    children: [],
    answer: '',
    analyses: [newAnalysis(1)],
    no_analysis_needed: false,
    labelDirty: false,
  }
}

export function defaultStructure(): QuestionStructure {
  return { version: 1, parts: [defaultLeaf()] }
}

export function isSimpleTree(parts: QuestionPart[]): boolean {
  return parts.length === 1 && !parts[0].stem.trim() && parts[0].children.length === 0
}

export function isLeaf(part: QuestionPart): boolean {
  return part.children.length === 0
}

export function walkLeaves(parts: QuestionPart[]): QuestionPart[] {
  const out: QuestionPart[] = []
  const rec = (nodes: QuestionPart[]) => {
    for (const n of nodes) {
      if (isLeaf(n)) out.push(n)
      else rec(n.children)
    }
  }
  rec(parts)
  return out
}

export function findPart(parts: QuestionPart[], id: string): QuestionPart | null {
  for (const p of parts) {
    if (p.id === id) return p
    const hit = findPart(p.children, id)
    if (hit) return hit
  }
  return null
}

export function findParent(parts: QuestionPart[], id: string): QuestionPart | null {
  for (const p of parts) {
    if (p.children.some((c) => c.id === id)) return p
    const hit = findParent(p.children, id)
    if (hit) return hit
  }
  return null
}

export function partPath(parts: QuestionPart[], id: string): QuestionPart[] {
  const path: QuestionPart[] = []
  const rec = (nodes: QuestionPart[]): boolean => {
    for (const n of nodes) {
      path.push(n)
      if (n.id === id) return true
      if (rec(n.children)) return true
      path.pop()
    }
    return false
  }
  rec(parts)
  return path
}

export function partDepth(parts: QuestionPart[], id: string): number {
  return partPath(parts, id).length
}

/** 叶子展示编号：简单树为空；否则拼祖先 label，如 (1)、(2)(i) */
export function leafPathLabel(parts: QuestionPart[], id: string): string {
  if (isSimpleTree(parts)) return ''
  return partPath(parts, id).map((p) => p.label).join('')
}

export function walkLeavesWithPath(parts: QuestionPart[]): { part: QuestionPart; pathLabel: string }[] {
  return walkLeaves(parts).map((part) => ({
    part,
    pathLabel: leafPathLabel(parts, part.id),
  }))
}

function labelAt(depth: number, index: number): string {
  const seq = depth <= 1 ? ARABIC : ROMAN_LOWER
  return seq[index] || `(${index + 1})`
}

export function relabelTree(parts: QuestionPart[], depth = 1): void {
  parts.forEach((p, i) => {
    if (!p.labelDirty) p.label = labelAt(depth, i)
    if (p.children.length) relabelTree(p.children, depth + 1)
  })
}

export function addSibling(parts: QuestionPart[], afterId: string | null): string {
  const leaf = defaultLeaf()
  if (!afterId) {
    parts.push(leaf)
    relabelTree(parts)
    return leaf.id
  }
  const idx = parts.findIndex((p) => p.id === afterId)
  if (idx >= 0) {
    parts.splice(idx + 1, 0, leaf)
    relabelTree(parts)
    return leaf.id
  }
  for (const p of parts) {
    if (p.children.some((c) => c.id === afterId) || findPart(p.children, afterId)) {
      const id = addSibling(p.children, afterId)
      relabelTree(parts)
      return id
    }
  }
  parts.push(leaf)
  relabelTree(parts)
  return leaf.id
}

/** 给当前叶子加子问：自身变为分支，答案迁到第一子问 */
export function addChild(parts: QuestionPart[], id: string): string | null {
  const node = findPart(parts, id)
  if (!node) return null
  if (partDepth(parts, id) >= MAX_PART_DEPTH) return null
  if (!isLeaf(node)) {
    const child = defaultLeaf()
    node.children.push(child)
    relabelTree(parts)
    return child.id
  }
  const first = defaultLeaf()
  first.stem = ''
  first.answer = node.answer
  first.analyses = node.analyses.length ? node.analyses : [newAnalysis(1)]
  first.no_analysis_needed = node.no_analysis_needed
  node.answer = undefined
  node.analyses = []
  node.no_analysis_needed = false
  node.children = [first]
  relabelTree(parts)
  return first.id
}

export function removePart(parts: QuestionPart[], id: string): boolean {
  const idx = parts.findIndex((p) => p.id === id)
  if (idx >= 0) {
    if (parts.length === 1 && !parts[0].children.length) return false
    parts.splice(idx, 1)
    if (parts.length === 0) parts.push(defaultLeaf())
    relabelTree(parts)
    return true
  }
  for (const p of parts) {
    if (removePart(p.children, id)) {
      if (p.children.length === 0) {
        p.analyses = [newAnalysis(1)]
        p.answer = p.answer || ''
      }
      relabelTree(parts)
      return true
    }
  }
  return false
}

export function collectPartTexts(parts: QuestionPart[]): string[] {
  const out: string[] = []
  const rec = (nodes: QuestionPart[]) => {
    for (const n of nodes) {
      out.push(n.stem, n.answer || '')
      for (const a of n.analyses) out.push(a.content)
      rec(n.children)
    }
  }
  rec(parts)
  return out
}

export function mapPartTexts(parts: QuestionPart[], fn: (s: string) => Promise<string>): Promise<void> {
  const rec = async (nodes: QuestionPart[]): Promise<void> => {
    for (const n of nodes) {
      n.stem = await fn(n.stem || '')
      if (n.answer != null) n.answer = await fn(n.answer)
      for (const a of n.analyses) a.content = await fn(a.content || '')
      await rec(n.children)
    }
  }
  return rec(parts)
}

export function cloneParts(parts: QuestionPart[]): QuestionPart[] {
  return JSON.parse(JSON.stringify(parts)) as QuestionPart[]
}

export function partsFromStructureJson(raw: unknown): QuestionPart[] {
  if (!raw || typeof raw !== 'object') return []
  const parts = (raw as { parts?: unknown }).parts
  return Array.isArray(parts) && parts.length ? normalizeIncomingParts(parts) : []
}

export function normalizeIncomingParts(raw: unknown): QuestionPart[] {
  if (!Array.isArray(raw) || raw.length === 0) return [defaultLeaf()]
  const mapOne = (p: any): QuestionPart => ({
    id: String(p?.id || newId()),
    label: String(p?.label || ''),
    stem: String(p?.stem || ''),
    children: Array.isArray(p?.children) ? p.children.map(mapOne) : [],
    answer: p?.answer != null ? String(p.answer) : '',
    analyses: Array.isArray(p?.analyses) && p.analyses.length
      ? p.analyses.map((a: any, i: number) => ({
          id: String(a?.id || newId()),
          title: String(a?.title || `解法${cnNum(i + 1)}`),
          content: String(a?.content || ''),
        }))
      : [newAnalysis(1)],
    no_analysis_needed: !!p?.no_analysis_needed,
    labelDirty: !!(p?.labelDirty ?? p?.label_dirty),
  })
  const parts = raw.map(mapOne)
  relabelTree(parts)
  return parts
}

export function flattenParts(parts: QuestionPart[], depth = 1): { part: QuestionPart; depth: number }[] {
  const out: { part: QuestionPart; depth: number }[] = []
  const rec = (nodes: QuestionPart[], d: number) => {
    for (const n of nodes) {
      out.push({ part: n, depth: d })
      if (n.children.length) rec(n.children, d + 1)
    }
  }
  rec(parts, depth)
  return out
}

export function leafCount(parts: QuestionPart[]): number {
  return walkLeaves(parts).length
}

export function allLeavesSkipAnalysis(parts: QuestionPart[]): boolean {
  const leaves = walkLeaves(parts)
  return leaves.length > 0 && leaves.every((l) => l.no_analysis_needed)
}

export function partsHaveContent(parts: QuestionPart[]): boolean {
  return flattenParts(parts).some(
    (n) =>
      !!(n.part.stem || '').trim()
      || !!(n.part.answer || '').trim()
      || n.part.analyses.some((a) => (a.content || '').trim()),
  )
}

export function taggingTextFromParts(parts: QuestionPart[]): string {
  const chunks: string[] = []
  const rec = (nodes: QuestionPart[]) => {
    for (const n of nodes) {
      if (n.stem.trim()) chunks.push(`${n.label} ${n.stem}`.trim())
      if ((n.answer || '').trim()) chunks.push(`${n.label} 答案：${n.answer}`)
      for (const a of n.analyses) {
        if (a.content.trim()) chunks.push(`${n.label} ${a.title || '解析'}：${a.content}`)
      }
      rec(n.children)
    }
  }
  rec(parts)
  return chunks.join('\n')
}

function toApiPart(p: QuestionPart): Record<string, unknown> {
  const branch = p.children.length > 0
  return {
    id: p.id,
    label: p.label,
    stem: p.stem,
    children: p.children.map(toApiPart),
    answer: branch ? null : (p.answer ?? ''),
    analyses: branch ? [] : p.analyses,
    no_analysis_needed: branch ? false : p.no_analysis_needed,
    label_dirty: p.labelDirty,
  }
}

export function toStructurePayload(parts: QuestionPart[]): QuestionStructure {
  return { version: 1, parts: cloneParts(parts) }
}

export function toStructureApiJson(parts: QuestionPart[]): { version: 1; parts: Record<string, unknown>[] } {
  return { version: 1, parts: parts.map(toApiPart) }
}

/** 旧扁平小题答案 + 整题解法 → 一层叶子树（AI 树结构落地前的过渡） */
export function partsFromFlatAnswers(subAnswers: string[], solutions: string[]): QuestionPart[] {
  const answers = subAnswers.length ? subAnswers : ['']
  const sols = solutions.filter((s) => s?.trim())
  const parts = answers.map((ans) => {
    const leaf = defaultLeaf()
    leaf.answer = ans
    leaf.analyses = (sols.length ? sols : ['']).map((content, j) => ({
      id: newId(),
      title: `解法${cnNum(j + 1)}`,
      content,
    }))
    return leaf
  })
  relabelTree(parts)
  return parts
}

export function partsFromParsed(q: {
  parts?: unknown
  correct_answer?: { kind?: string; value?: { subs?: { content?: string }[] } }
  analysis?: { content?: string }[]
}): QuestionPart[] {
  if (Array.isArray(q.parts) && q.parts.length) return normalizeIncomingParts(q.parts)
  const subs =
    q.correct_answer?.kind === 'solution'
      ? (q.correct_answer.value?.subs ?? []).map((s) => s.content || '')
      : ['']
  return partsFromFlatAnswers(
    subs.length ? subs : [''],
    (q.analysis || []).map((a) => a.content || ''),
  )
}
