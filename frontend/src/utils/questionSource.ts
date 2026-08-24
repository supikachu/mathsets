/**
 * 题目来源级联字典（与后端 source_category / source_kind 对齐）
 */

export type SourceCategory = 'paper' | 'practice' | 'other'

export type PaperKind =
  | 'monthly_test'
  | 'unit_test'
  | 'stage_test'
  | 'midterm'
  | 'final'
  | 'gaokao'
  | 'mock'

export type PracticeKind =
  | 'preview'
  | 'class_example'
  | 'in_class'
  | 'homework'
  | 'unit_review'

export type OtherKind =
  | 'special'
  | 'workbook'
  | 'textbook_example'
  | 'lecture'
  | 'wrong_question'

export type SourceKind = PaperKind | PracticeKind | OtherKind

export const SOURCE_CATEGORY_LABELS: Record<SourceCategory, string> = {
  paper: '试卷',
  practice: '练习',
  other: '其他',
}

export const PAPER_KIND_OPTIONS: { value: PaperKind; label: string }[] = [
  { value: 'monthly_test', label: '月测' },
  { value: 'unit_test', label: '单元测' },
  { value: 'stage_test', label: '阶段测' },
  { value: 'midterm', label: '期中' },
  { value: 'final', label: '期末' },
  { value: 'gaokao', label: '高考真题' },
  { value: 'mock', label: '模拟题' },
]

export const PRACTICE_KIND_OPTIONS: { value: PracticeKind; label: string }[] = [
  { value: 'preview', label: '课前预习' },
  { value: 'class_example', label: '课堂例题' },
  { value: 'in_class', label: '随堂练习' },
  { value: 'homework', label: '课后作业' },
  { value: 'unit_review', label: '单元复习' },
]

export const OTHER_KIND_OPTIONS: { value: OtherKind; label: string }[] = [
  { value: 'special', label: '专题资料' },
  { value: 'workbook', label: '教辅练习' },
  { value: 'textbook_example', label: '教材例题' },
  { value: 'lecture', label: '讲义' },
  { value: 'wrong_question', label: '错题' },
]

export function kindsForCategory(category: SourceCategory) {
  if (category === 'paper') return PAPER_KIND_OPTIONS
  if (category === 'practice') return PRACTICE_KIND_OPTIONS
  return OTHER_KIND_OPTIONS
}

export function defaultKindForCategory(category: SourceCategory): SourceKind {
  if (category === 'paper') return 'monthly_test'
  if (category === 'practice') return 'in_class'
  return 'special'
}

/** 模拟题默认建议创建试卷 */
export function defaultCreatePaper(kind: SourceKind): boolean {
  return kind === 'midterm' || kind === 'final' || kind === 'gaokao' || kind === 'mock'
}

export function sourceKindLabel(kind: string): string {
  const all = [...PAPER_KIND_OPTIONS, ...PRACTICE_KIND_OPTIONS, ...OTHER_KIND_OPTIONS]
  return all.find((o) => o.value === kind)?.label ?? kind
}

export function sourceCategoryLabel(category: string): string {
  return SOURCE_CATEGORY_LABELS[category as SourceCategory] ?? category
}

/** 旧扁平 document_type → 级联 */
export function mapLegacyDocumentType(t: string): { category: SourceCategory; kind: SourceKind } {
  switch (t) {
    case 'exam':
      return { category: 'paper', kind: 'monthly_test' }
    case 'mock_exam':
      return { category: 'paper', kind: 'mock' }
    case 'preview_exercise':
      return { category: 'practice', kind: 'preview' }
    case 'class_example':
      return { category: 'practice', kind: 'class_example' }
    case 'class_exercise':
      return { category: 'practice', kind: 'in_class' }
    case 'homework':
      return { category: 'practice', kind: 'homework' }
    case 'unit_exercise':
      return { category: 'practice', kind: 'unit_review' }
    case 'chapter_exercise':
    case 'special_training':
      return { category: 'other', kind: 'special' }
    case 'exercise_book':
      return { category: 'other', kind: 'workbook' }
    case 'textbook_example':
      return { category: 'other', kind: 'textbook_example' }
    case 'teaching_material':
      return { category: 'other', kind: 'lecture' }
    case 'wrong_question':
      return { category: 'other', kind: 'wrong_question' }
    default:
      if (t.includes(':')) {
        const [c, k] = t.split(':')
        if ((c === 'paper' || c === 'practice' || c === 'other') && k) {
          return { category: c, kind: k as SourceKind }
        }
      }
      return { category: 'practice', kind: 'in_class' }
  }
}

/** 试卷卡片/筛选展示：code 与中文互认 */
export function displayPaperSource(sourceType?: string | null, sub?: string | null): string {
  const raw = (sourceType || '').trim()
  const subRaw = (sub || '').trim()
  const isMock = raw === 'mock' || raw === '高考模拟' || raw === '模拟题' || raw === 'mock_exam'
  if (isMock && subRaw) return subRaw
  if (raw) {
    const label = sourceKindLabel(raw)
    return label || subRaw
  }
  return subRaw
}

export function displaySourceLabel(category?: string | null, kind?: string | null, legacy?: string | null): string {
  if (category && kind) {
    return `${sourceCategoryLabel(category)} · ${sourceKindLabel(kind)}`
  }
  if (legacy) {
    const m = mapLegacyDocumentType(legacy)
    return `${sourceCategoryLabel(m.category)} · ${sourceKindLabel(m.kind)}`
  }
  return ''
}

export interface PaperSourceMeta {
  title: string
  year?: number
  stage?: string
  grade?: string
  subject?: string
  semester?: string
  region_province?: string
  region_city?: string
  school_name?: string
  source_type?: string
  sub_source_type?: string
  paper_id?: string
}

export interface QuestionSourceState {
  source_category: SourceCategory
  source_kind: SourceKind
  create_paper: boolean
  title?: string
  sub_source_type?: string
  paper_meta?: PaperSourceMeta
}

/** 中文科目名 → form.subject 代码 */
export function normalizeSubjectCode(raw?: string | null): 'math' | 'physics' | undefined {
  if (!raw) return undefined
  const s = raw.trim()
  if (s === 'math' || s === 'physics') return s
  if (s.includes('物理') || s.toLowerCase() === 'physics') return 'physics'
  if (s.includes('数学') || s.toLowerCase() === 'math') return 'math'
  return undefined
}

/** 来源级联 / 试卷信息 → 题目表单可 merge 字段 */
export interface QuestionFormSourceFields {
  stage?: 'junior' | 'senior'
  subject?: 'math' | 'physics'
  grade?: string
  grade_semester?: string
  year?: string
  region_province?: string
  region_city?: string
  source_type?: string
  sub_source_type?: string
  /** 仅进 metadata，侧栏无独立控件 */
  school_name?: string
  source_category?: SourceCategory
  source_kind?: SourceKind
}

/**
 * 把识别来源条状态映射为题目表单字段。
 * 空字符串表示「明确清空」；undefined 表示不改该字段。
 */
export function applySourceStateToQuestionFields(state: QuestionSourceState): QuestionFormSourceFields {
  const out: QuestionFormSourceFields = {
    source_category: state.source_category,
    source_kind: state.source_kind,
    source_type: sourceKindLabel(state.source_kind),
  }

  const sub = state.sub_source_type || state.paper_meta?.sub_source_type
  if (sub) out.sub_source_type = sub
  else if (state.source_kind !== 'mock') out.sub_source_type = ''

  const pm = state.paper_meta
  if (state.source_category === 'paper' && pm) {
    if (pm.stage === 'junior' || pm.stage === 'senior') out.stage = pm.stage
    const subj = normalizeSubjectCode(pm.subject)
    if (subj) out.subject = subj
    if (pm.grade != null && pm.grade !== '') out.grade = pm.grade
    if (pm.semester) out.grade_semester = pm.semester
    if (pm.year != null) out.year = String(pm.year)
    if (pm.region_province != null) out.region_province = pm.region_province
    if (pm.region_city != null) out.region_city = pm.region_city
    if (pm.school_name != null) out.school_name = pm.school_name
  }

  return out
}

/** 解析应挂到题目上的 paper_ids（不建卷 / 练习 / 其他 → 空） */
export function resolvePaperIdsFromSource(state: QuestionSourceState): string[] {
  if (state.source_category !== 'paper' || !state.create_paper) return []
  const id = state.paper_meta?.paper_id
  return id ? [id] : []
}
