/** 解析试卷题号："14" / "14." / "17(2)" / "17（1）" */
export function parsePaperQuestionNo(raw?: string | null): { major: number; minor: number } | null {
  const s = (raw || '').trim()
  if (!s) return null
  const m = s.match(/^(\d{1,3})\s*(?:[.．、]?\s*)(?:[（(]\s*(\d+)\s*[）)])?/)
  if (!m) return null
  return { major: Number(m[1]), minor: m[2] ? Number(m[2]) : 0 }
}

/** 题干首行「14.」「第 4 题」 */
export function inferQuestionNoFromStem(stem?: string | null): string {
  const line = (stem || '').trimStart().split('\n')[0] || ''
  const stripped = line.replace(/^[#>*\s_*]+/, '').replace(/^第\s*/, '')
  const m = stripped.match(/^(\d{1,3})(?:\s*(?:题|[.．、])|\s*[（(]|$)/)
  return m ? m[1] : ''
}

export function resolvedQuestionNo(q: {
  question_no?: string | number | null
  stem?: string
}): string {
  const explicit = q.question_no != null && q.question_no !== ''
    ? String(q.question_no).trim()
    : ''
  if (explicit) return explicit
  return inferQuestionNoFromStem(q.stem)
}

function paperOrderKey(q: {
  question_no?: string | number | null
  display_order?: number | null
  stem?: string
}): [number, number, number] {
  const parsed = parsePaperQuestionNo(resolvedQuestionNo(q))
  if (parsed) return [0, parsed.major, parsed.minor]
  if (typeof q.display_order === 'number' && Number.isFinite(q.display_order)) {
    return [1, q.display_order, 0]
  }
  return [2, Number.MAX_SAFE_INTEGER, 0]
}

export function comparePaperQuestionOrder(
  a: { question_no?: string | number | null; display_order?: number | null; stem?: string },
  b: { question_no?: string | number | null; display_order?: number | null; stem?: string },
): number {
  const ka = paperOrderKey(a)
  const kb = paperOrderKey(b)
  for (let i = 0; i < 3; i++) {
    if (ka[i] !== kb[i]) return ka[i] - kb[i]
  }
  return 0
}

export function sortByPaperQuestionNo<T>(
  items: T[],
  pick: (item: T) => { question_no?: string | number | null; display_order?: number | null; stem?: string },
): T[] {
  return items
    .map((item, index) => ({ item, index }))
    .sort((a, b) => comparePaperQuestionOrder(pick(a.item), pick(b.item)) || a.index - b.index)
    .map((x) => x.item)
}
