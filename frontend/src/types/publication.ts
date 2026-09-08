/**
 * PublicationDocument — Studio 编辑态真相源（结构化排版 IR）
 *
 * 设计见 docs/结构化排版_主线与冻结.md
 * 编译目标：装配为 ExamRequest / LayoutDoc → Typst
 */

import type { ExamSectionRequest, ExportMode, LayoutSpec } from '@/api/client'

/** 文档种类：有边界扩展，不做自由杂志 */
export type PublicationKind = 'exam' | 'handout' | 'topic'

export type PubBlockType =
  | 'section'
  | 'question'
  | 'rich_text'
  | 'multi_column_box'
  | 'callout'

/** 可复用样式集（先服务盒子皮肤与区域默认） */
export interface StyleSet {
  id: string
  name: string
  /** region = 题干/选项等；box = 多栏盒子皮肤；both = 两者 */
  scope: 'region' | 'box' | 'both'
  font_size_pt?: number
  weight?: 'regular' | 'bold'
  color?: string
  /** 盒子专用 */
  box?: BoxStyle
}

/** 多栏盒子外观（对齐视频产品配置面的子集） */
export interface BoxStyle {
  show_title?: boolean
  title?: string
  title_icon?: string
  icon_size_px?: number
  title_pos?: { x_pct: number; y_pct: number }
  colors?: {
    title?: string
    background?: string
    container?: string
  }
  gradient?: boolean
  radius_px?: number
  gap_px?: number
}

export interface PubBlockBase {
  id: string
  type: PubBlockType
}

export interface SectionBlock extends PubBlockBase {
  type: 'section'
  title: string
  instruction?: string
}

export interface QuestionBlock extends PubBlockBase {
  type: 'question'
  question_id: string
  /** 展示用题号（可与自动编号并存） */
  display_no?: number
  default_score?: number
}

export interface RichTextBlock extends PubBlockBase {
  type: 'rich_text'
  /** 纯文本或轻量 markdown；P3 再接富文本 marks */
  text: string
}

export interface MultiColumnBoxBlock extends PubBlockBase {
  type: 'multi_column_box'
  columns: 2 | 3
  children: PubBlock[]
  style_set_id?: string
  box_style?: BoxStyle
}

export interface CalloutBlock extends PubBlockBase {
  type: 'callout'
  kind: 'knowledge' | 'error_prone' | 'tip' | 'approach'
  text: string
}

export type PubBlock =
  | SectionBlock
  | QuestionBlock
  | RichTextBlock
  | MultiColumnBoxBlock
  | CalloutBlock

export interface PublicationExportOptions {
  mode: ExportMode
  include_answer: boolean
  include_analysis: boolean
  answer_at_end: boolean
  callouts?: {
    knowledge?: boolean
    error_prone?: boolean
    analysis?: boolean
  }
}

/**
 * 排版文档（可 sessionStorage / 日后入库）
 */
export interface PublicationDocument {
  version: 1
  id: string
  kind: PublicationKind
  title: string
  subtitle?: string
  /** 整卷版面；缺省时由 mode 取预设 */
  page?: LayoutSpec | null
  style_sets: StyleSet[]
  /** 区域默认样式集 id */
  region_styles?: {
    section_title?: string
    stem?: string
    option?: string
  }
  body: PubBlock[]
  export: PublicationExportOptions
  updated_at: string
}

export const STUDIO_DOCUMENT_KEY = 'studio_document'
export const STUDIO_LEGACY_SECTIONS_KEY = 'typeset_sections'
export const STUDIO_LEGACY_TITLE_KEY = 'typeset_title'

function newId(prefix: string): string {
  return `${prefix}_${crypto.randomUUID().slice(0, 8)}`
}

/** 从试题篮 ExamSectionRequest[] 装配初始文档 */
export function documentFromSections(
  sections: ExamSectionRequest[],
  opts?: { title?: string; kind?: PublicationKind },
): PublicationDocument {
  const body: PubBlock[] = []
  let displayNo = 1
  for (const sec of sections) {
    body.push({
      id: newId('sec'),
      type: 'section',
      title: sec.title,
      instruction: sec.instruction,
    })
    for (const q of sec.questions) {
      body.push({
        id: newId('q'),
        type: 'question',
        question_id: q.id,
        display_no: displayNo++,
        default_score: q.default_score,
      })
    }
  }
  return {
    version: 1,
    id: newId('doc'),
    kind: opts?.kind ?? 'exam',
    title: opts?.title?.trim() || '未命名试卷',
    style_sets: [],
    body,
    export: {
      mode: 'teacher',
      include_answer: true,
      include_analysis: true,
      answer_at_end: false,
      callouts: { knowledge: true, error_prone: true, analysis: false },
    },
    updated_at: new Date().toISOString(),
  }
}

/** 展平为导出用的 ExamSectionRequest[]（忽略未实现的盒子嵌套时仍按线性 question 归属到上一 section） */
export function sectionsFromDocument(doc: PublicationDocument): ExamSectionRequest[] {
  const result: ExamSectionRequest[] = []
  let current: ExamSectionRequest | null = null

  const visit = (blocks: PubBlock[]) => {
    for (const b of blocks) {
      if (b.type === 'section') {
        current = { title: b.title, instruction: b.instruction, questions: [] }
        result.push(current)
      } else if (b.type === 'question') {
        if (!current) {
          current = { title: '题目', questions: [] }
          result.push(current)
        }
        current.questions.push(
          b.default_score && b.default_score > 0
            ? { id: b.question_id, default_score: b.default_score }
            : { id: b.question_id },
        )
      } else if (b.type === 'multi_column_box') {
        visit(b.children)
      }
    }
  }

  visit(doc.body)
  return result
}

export function countQuestions(doc: PublicationDocument): number {
  let n = 0
  const walk = (blocks: PubBlock[]) => {
    for (const b of blocks) {
      if (b.type === 'question') n++
      else if (b.type === 'multi_column_box') walk(b.children)
    }
  }
  walk(doc.body)
  return n
}

export function saveDocumentToSession(doc: PublicationDocument): void {
  sessionStorage.setItem(STUDIO_DOCUMENT_KEY, JSON.stringify(doc))
}

export function loadDocumentFromSession(): PublicationDocument | null {
  const raw = sessionStorage.getItem(STUDIO_DOCUMENT_KEY)
  if (raw) {
    try {
      return JSON.parse(raw) as PublicationDocument
    } catch {
      /* fall through */
    }
  }
  // 兼容旧入口：typeset_sections
  const legacy = sessionStorage.getItem(STUDIO_LEGACY_SECTIONS_KEY)
  if (legacy) {
    try {
      const sections = JSON.parse(legacy) as ExamSectionRequest[]
      const title = sessionStorage.getItem(STUDIO_LEGACY_TITLE_KEY) || undefined
      return documentFromSections(sections, { title })
    } catch {
      return null
    }
  }
  return null
}

/** 对象树展示用扁平行 */
export interface TreeRow {
  id: string
  depth: number
  label: string
  blockType: PubBlockType
}

export function flattenTree(doc: PublicationDocument): TreeRow[] {
  const rows: TreeRow[] = []
  const walk = (blocks: PubBlock[], depth: number) => {
    for (const b of blocks) {
      if (b.type === 'section') {
        rows.push({ id: b.id, depth, label: b.title, blockType: b.type })
      } else if (b.type === 'question') {
        rows.push({
          id: b.id,
          depth,
          label: `第 ${b.display_no ?? '?'} 题`,
          blockType: b.type,
        })
      } else if (b.type === 'rich_text') {
        rows.push({
          id: b.id,
          depth,
          label: b.text.slice(0, 24) || '文本',
          blockType: b.type,
        })
      } else if (b.type === 'multi_column_box') {
        rows.push({
          id: b.id,
          depth,
          label: `多栏盒子（${b.columns} 栏 · ${b.children.length} 项）`,
          blockType: b.type,
        })
        walk(b.children, depth + 1)
      } else if (b.type === 'callout') {
        rows.push({ id: b.id, depth, label: `提示 · ${b.kind}`, blockType: b.type })
      }
    }
  }
  walk(doc.body, 0)
  return rows
}
