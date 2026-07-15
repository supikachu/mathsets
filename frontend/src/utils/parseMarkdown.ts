import type { ParsedQuestion, ParsedOption, BlankAnswer } from '@/api/client'

/** 系统推荐提示词（用户复制后发给 AI） */
export const RECOMMENDED_PROMPT = `请将图片中的题目转换为 Markdown 格式，严格遵守以下格式规则：

## 题目
（题干内容。行内公式用 $...$ 包裹，如 $x^2+1$；块级公式用 $$...$$ 包裹，独占一行，前后不要空行）
（如果是选择题，选项按以下格式列出，每行一个：）
A. 选项A内容
B. 选项B内容
C. 选项C内容
D. 选项D内容

## 答案
（只写最终结果，禁止写推导过程）
（选择题：直接写字母，单选如 B，多选如 ACD）
（填空题：按空顺序写出答案，用分号分隔，如 答案一；答案二）
（解答题：按小题分行写出最终结果，每行一个，格式如下）
(1) 第一小题的最终结果
(2) 第二小题的最终结果

## 解析
（完整解答过程和思路分析。若有多小题，必须用 ### (1)、### (2) 等三级标题独占一行分隔每个小题的解答，每小题的推导步骤完整写出）

## 属性
知识点: 知识点1, 知识点2（多个用英文逗号分隔）
难度: 简单 或 中等 或 困难（三选一）

格式要求：
1. 必须包含 ## 题目、## 答案、## 解析、## 属性 四个部分，缺一不可
2. 所有数学公式必须用 $...$ 或 $$...$$ 包裹，不要用 Unicode 符号（如 x² 应写 $x^2$）
3. 不要输出任何其他内容（如"好的""以下是转换结果"等）
4. 不要添加代码块标记
5. ## 答案 只写最终结果，所有推导过程一律放到 ## 解析 中，不要混到答案里
6. ## 解析 中多小题必须用 ### (1)、### (2) 等三级标题独占一行分隔，标题前后不要空行
7. 块级公式 $$...$$ 必须独占一行，且公式行前后不要留空行（即公式与上下文紧邻）
8. 段落之间用单个空行分隔即可，不要连续多个空行
9. ## 属性 部分的知识点和难度各占一行，格式为"字段名: 值"
10. 难度只能填"简单""中等""困难"三个词之一`

/** 预处理：去除代码块包裹、统一换行 */
function preprocess(md: string): string {
  let text = md.replace(/\r\n/g, '\n').trim()
  // 去除 ```markdown ... ``` 或 ``` ... ``` 包裹
  text = text.replace(/^```(?:markdown)?\s*\n([\s\S]*?)\n```\s*$/, '$1')
  return text.trim()
}

/** 按 ## 二级标题切分，返回 {标题: 内容} 映射 */
function splitByHeading(md: string): Map<string, string> {
  const sections = new Map<string, string>()
  const lines = md.split('\n')
  let currentTitle = ''
  let currentContent: string[] = []

  for (const line of lines) {
    const match = line.match(/^##\s+(.+?)\s*$/)
    if (match) {
      if (currentTitle) {
        sections.set(currentTitle, currentContent.join('\n').trim())
      }
      currentTitle = match[1].trim()
      currentContent = []
    } else if (currentTitle) {
      currentContent.push(line)
    }
  }
  if (currentTitle) {
    sections.set(currentTitle, currentContent.join('\n').trim())
  }
  return sections
}

/** 从题干段分离题干和选项 */
function extractOptions(stemSection: string): { stem: string; options: ParsedOption[] | null } {
  const lines = stemSection.split('\n')
  const optionLines: { idx: number; label: string; content: string }[] = []

  lines.forEach((line, idx) => {
    const m = line.match(/^([A-D])[.、)]\s*(.+)/)
    if (m) {
      optionLines.push({ idx, label: m[1], content: m[2].trim() })
    }
  })

  if (optionLines.length < 2) {
    return { stem: stemSection.trim(), options: null }
  }

  // 选项行之前的内容为题干
  const firstOptionIdx = optionLines[0].idx
  const stem = lines.slice(0, firstOptionIdx).join('\n').trim()
  const options: ParsedOption[] = optionLines.map((o) => ({ label: o.label, content: o.content }))
  return { stem, options }
}

/** 题型判定 */
function detectType(stem: string, options: ParsedOption[] | null): string {
  if (options && options.length >= 2) return 'choice'
  if (/_{3,}/.test(stem)) return 'fill'
  return 'solution'
}

/** 选择题答案提取 */
function extractChoiceAnswer(answerSection: string): { options: string[]; isMulti: boolean } {
  const matches = answerSection.match(/[A-D]/g)
  if (!matches) return { options: [], isMulti: false }
  // 去重
  const unique = [...new Set(matches)]
  return { options: unique, isMulti: unique.length > 1 }
}

/** 填空题答案提取 */
function extractFillAnswer(answerSection: string): BlankAnswer[] {
  const parts = answerSection.split(/[；;]/).map((s) => s.trim()).filter(Boolean)
  return parts.map((answer, i) => ({ position: i + 1, answer }))
}

/** 解答题按小题拆分：支持 ### (N) 标题独占一行，或行首 (N) 内容 两种格式 */
function extractSolutionAnswer(answerSection: string): string[] {
  // 格式1：### (1) 标题独占一行
  const headingRegex = /^###\s*[（(]\s*(\d+)\s*[）)]\s*$/
  // 格式2：行首 (1) 答案内容
  const inlineRegex = /^[（(]\s*(\d+)\s*[）)]\s*(.*)$/

  const lines = answerSection.split('\n')
  const subs: { id: number; content: string[] }[] = []
  let current: { id: number; content: string[] } | null = null

  for (const line of lines) {
    let m = line.match(headingRegex)
    if (m) {
      if (current) subs.push(current)
      current = { id: parseInt(m[1]), content: [] }
      continue
    }
    m = line.match(inlineRegex)
    if (m) {
      if (current) subs.push(current)
      current = { id: parseInt(m[1]), content: [] }
      if (m[2].trim()) current.content.push(m[2].trim())
      continue
    }
    if (current) {
      current.content.push(line)
    } else {
      // 无小题标题的内容，整体作为 subs[0]
      if (!current) current = { id: 1, content: [] }
      current.content.push(line)
    }
  }
  if (current) subs.push(current)

  if (subs.length === 0) {
    return [answerSection.trim()]
  }

  // 按 id 排序，过滤空内容
  subs.sort((a, b) => a.id - b.id)
  const result = subs.map((s) => s.content.join('\n').trim()).filter(Boolean)
  return result.length > 0 ? result : [answerSection.trim()]
}

/**
 * 解析段处理：把 ### (N) 小问标题转为行首 (N) 标记，合并为单个 analysis 元素。
 * form.solutions 的数据模型是"多解法"而非"分小问"，同一题目的多个小问
 * 应合并为"解法一"，内部用 (1) (2) 行首标记分隔。
 */
function splitAnalysisBySub(analysisSection: string): { title: string; content: string }[] {
  const headingRegex = /^###\s*[（(]\s*(\d+)\s*[）)]\s*$/
  const lines = analysisSection.split('\n')
  const outLines: string[] = []

  for (const line of lines) {
    const m = line.match(headingRegex)
    if (m) {
      // ### (1) → 行首 (1)，作为小问分隔标记
      outLines.push(`(${m[1]})`)
    } else {
      outLines.push(line)
    }
  }

  const content = outLines.join('\n').replace(/\n{3,}/g, '\n\n').trim()
  return content ? [{ title: '解法一', content }] : []
}

/** 中文难度映射到英文枚举 */
function mapDifficulty(cn: string): string | undefined {
  const t = cn.trim()
  if (/简单|容易/.test(t)) return 'easy'
  if (/中等/.test(t)) return 'medium'
  if (/困难|难/.test(t)) return 'hard'
  return undefined
}

/** 从属性段提取知识点和难度 */
function extractAttributes(attrSection: string): {
  knowledgePoints: string[]
  difficulty: string | undefined
} {
  let knowledgePoints: string[] = []
  let difficulty: string | undefined

  const kpMatch = attrSection.match(/知识点[:：]\s*(.+)/)
  if (kpMatch) {
    knowledgePoints = kpMatch[1]
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean)
  }

  const diffMatch = attrSection.match(/难度[:：]\s*(.+)/)
  if (diffMatch) {
    difficulty = mapDifficulty(diffMatch[1])
  }

  return { knowledgePoints, difficulty }
}

/**
 * 将按推荐提示词格式输出的 markdown 解析为 ParsedQuestion。
 * 格式固定：## 题目 / ## 答案 / ## 解析 / ## 属性 四段式。
 */
export function parseMarkdownToQuestion(md: string): ParsedQuestion {
  const text = preprocess(md)
  const sections = splitByHeading(text)

  const stemSection = sections.get('题目') ?? ''
  const answerSection = sections.get('答案') ?? ''
  const analysisSection = sections.get('解析') ?? ''
  const attrSection = sections.get('属性') ?? ''

  // 提取题干 + 选项
  const { stem, options } = extractOptions(stemSection)

  // 题型判定
  const questionType = detectType(stem, options)

  // 答案提取（按题型）
  let correctAnswer: ParsedQuestion['correct_answer']
  let subType: string | undefined

  if (questionType === 'choice') {
    const { options: ansOpts, isMulti } = extractChoiceAnswer(answerSection)
    if (isMulti) subType = 'multi'
    correctAnswer = { kind: 'choice', value: { options: ansOpts } }
  } else if (questionType === 'fill') {
    const blanks = extractFillAnswer(answerSection)
    correctAnswer = { kind: 'fill', value: { blanks } }
  } else {
    const subs = extractSolutionAnswer(answerSection)
    correctAnswer = {
      kind: 'solution',
      value: { subs: subs.map((content, i) => ({ sub_id: i + 1, content })) },
    }
  }

  // 解析提取：按 ### (N) 标题拆分为多个小问，去掉标题字面文本
  let analysis: { title: string; content: string }[]
  const warnings: string[] = []
  if (analysisSection) {
    analysis = splitAnalysisBySub(analysisSection)
    if (analysis.length === 0) {
      analysis = [{ title: '解法一', content: '' }]
      warnings.push('## 解析 段为空，请手动补充解题过程')
    }
  } else {
    // 无解析段：不再用答案段兜底（会导致答案与解析混淆），给空占位 + 警告
    analysis = [{ title: '解法一', content: '' }]
    warnings.push('AI 未输出 ## 解析 段，请手动补充解题过程')
  }

  // 属性提取
  const { knowledgePoints, difficulty } = extractAttributes(attrSection)

  if (knowledgePoints.length > 0) {
    warnings.push('Markdown 模式解析，知识点为 AI 建议，请确认')
  } else {
    warnings.push('Markdown 模式解析，知识点未识别，请手动选择')
  }

  return {
    question_type: questionType as 'choice' | 'fill' | 'solution',
    sub_type: subType,
    difficulty,
    stem,
    options: options ?? undefined,
    correct_answer: correctAnswer,
    analysis,
    knowledge_points: knowledgePoints,
    confidence: 0.85,
    warnings,
    image_placeholders: [],
  }
}
