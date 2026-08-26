/** 从 B / ['B'] / $\mathrm{B}$ / { options: ['B'] } 抽出 A–D，避免预览二次包裹或显示 — */

export function extractChoiceLetters(raw: unknown): string[] {
  if (raw == null || raw === '') return []
  if (Array.isArray(raw)) {
    const out: string[] = []
    for (const item of raw) {
      for (const letter of extractChoiceLetters(item)) {
        if (!out.includes(letter)) out.push(letter)
      }
    }
    return out
  }
  if (typeof raw === 'object') {
    const obj = raw as Record<string, unknown>
    if (Array.isArray(obj.options)) return extractChoiceLetters(obj.options)
    if (obj.value != null && typeof obj.value === 'object') {
      return extractChoiceLetters(obj.value)
    }
    return []
  }
  if (typeof raw !== 'string') return []

  const mathrm = raw.match(/\\mathrm\s*\{([A-Za-z]+)\}/)
  const source = mathrm ? mathrm[1] : raw
  const letters: string[] = []
  for (const ch of source.toUpperCase()) {
    if (ch >= 'A' && ch <= 'D' && !letters.includes(ch)) letters.push(ch)
  }
  if (mathrm) return letters
  const trimmed = source.trim()
  if (/^[A-Da-d]$/.test(trimmed)) return [trimmed.toUpperCase()]
  const compact = trimmed.replace(/[\s,，、$\\{}]/g, '')
  if (/^[A-D]+$/.test(compact)) return letters
  return trimmed.length <= 12 ? letters : []
}

export function choiceAnswerLatex(raw: unknown): string {
  const letters = extractChoiceLetters(raw)
  if (!letters.length) return ''
  return `$\\mathrm{${letters.join('')}}$`
}
