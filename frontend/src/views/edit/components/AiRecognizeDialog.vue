<script setup lang="ts">
import { ref, watch, nextTick, computed, onBeforeUnmount } from 'vue'
import {
  documentApi,
  collectionApi,
  questionApi,
  type ParsedQuestion,
  type DocumentMeta,
  type ConfirmDocumentRequest,
  type QuestionDetail,
  type QuestionCollectionSummary,
  type AiStagedQuestion,
  type TagMatch,
  type TaggingMatch,
  type TaggingUnmatched,
} from '@/api/client'
import { AppButton, AppModal, AppConfirm, AppIcon, AppBadge } from '@/components/ui'
import LatexRender from '@/components/LatexRender.vue'
import QuestionOptions from '@/components/QuestionOptions.vue'
import { typeLabel, typeBadgeColor, diffLabel, diffBadgeColor } from '@/utils/questionDisplay'
import { useToast } from '@/composables/useToast'
import { useAiParsePolling } from '@/composables/useAiParsePolling'
import { parseMarkdownToQuestion, RECOMMENDED_PROMPT, normalizeChoiceAnswerBlank } from '@/utils/parseMarkdown'
import { compressImage, blobToFile } from '@/utils/imageCompressor'
import { withBackoffRetry } from '@/utils/concurrency'
import { pdfToImages, type PdfPageImage } from '@/utils/pdfToImages'
import { clearBatchSnapshot, hasUnfinishedSnapshot, type BatchSnapshot } from '@/utils/batchSnapshot'
import { loadAiSourceFile, saveAiSourceFile } from '@/utils/aiSourceFile'
import { displaySourceLabel } from '@/utils/questionSource'
import SourceCascadeBar from './SourceCascadeBar.vue'
import TaskProgressPanel from './TaskProgressPanel.vue'
import QuestionGroupingStep, { type GroupQuestion } from './QuestionGroupingStep.vue'

const show = defineModel<boolean>({ required: true })
const applyingAiResult = defineModel<boolean>('applyingAiResult', { default: false })
const knowledgeNodeIds = defineModel<string[]>('knowledgeNodeIds', { required: true })
const chapterNodeIds = defineModel<string[]>('chapterNodeIds', { default: () => [] })
const methodNodeIds = defineModel<string[]>('methodNodeIds', { default: () => [] })
const aiGeneratedFields = defineModel<Set<string>>('aiGeneratedFields', { required: true })
const aiSessionActive = defineModel<boolean>('aiSessionActive', { default: false })

const props = defineProps<{
  /** 新建页内嵌为与「手动录题」并列的模块，不再套弹窗 */
  embedded?: boolean
  form: {
    stem: string
    question_type: string
    sub_type: string
    difficulty: string
    difficulty_coefficient: number
    default_score: number
    grade: string | undefined
    semester: string | undefined
    grade_semester: string
    // ── 长尾维度：与 QuestionList 数据字典对齐，统一存入 metadata(JSONB) ──
    year: string
    region_province: string
    region_city: string
    source_type: string
    sub_source_type: string
    options: { label: string; content: string }[]
    correctAnswer: any
    blanks: { position: number; answer: string }[]
    sub_answers: string[]
    solutions: string[]
    tagIds: string[]
    knowledgeNodeIds: string[]
    hasUnsaved: boolean
  }
  /** 编辑页回写的题目快照：返回识别后卡片展示最新内容 */
  editedSnapshots?: any[]
  /** 识别预览「全部保存」进行中 */
  savingAll?: boolean
}>()

const emit = defineEmits<{
  (e: 'applied'): void
  // 识别完成即把题目装进父组件工作台（不切页）；点卡片再进入对应题编辑
  (e: 'batch-parsed', questions: ParsedQuestion[]): void
  (e: 'open-question', index: number): void
  (e: 'save-all'): void
  (e: 'source-updated', state: import('@/utils/questionSource').QuestionSourceState): void
}>()

const toast = useToast()
/** 当前识别会话的来源状态（保存题目时写入 metadata） */
const sourceState = ref<import('@/utils/questionSource').QuestionSourceState | null>(null)

// AI Mode tab: 'markdown' | 'image' | 'pdf'（图片与 PDF 各走独立通道）
const aiMode = ref<'markdown' | 'image' | 'pdf'>('markdown')
const aiText = ref('')
const aiError = ref('')
const aiParsing = ref(false)
const aiResult = ref<ParsedQuestion | null>(null)
const promptCopied = ref(false)
const previewQuestions = ref<ParsedQuestion[]>([])
const ocrFileName = ref('')
const sourcePreviewUrl = ref('')
const sourceKind = ref<'markdown' | 'pdf' | 'image'>('markdown')
let markdownParseTimer: ReturnType<typeof setTimeout> | undefined

watch(
  [sourcePreviewUrl, previewQuestions, aiResult],
  () => {
    aiSessionActive.value = Boolean(
      sourcePreviewUrl.value || previewQuestions.value.length || aiResult.value,
    )
  },
  { immediate: true },
)

// V2.1.1 资料流程状态：idle（选文件）→ uploading（上传+分类中）→ confirm（确认类型）
// → progress（解析中）→ grouping（Mixed 分组）
// PDF 通道新增 pdf_fallback：直连解析失败 → 用户选择是否拆页 OCR 回退
const docFlowState = ref<'idle' | 'uploading' | 'confirm' | 'progress' | 'grouping' | 'pdf_fallback'>('idle')
// 当前文件流程归属通道（pdf 通道确认类型后以 pdf_direct 模式建任务）
const docFlowKind = ref<'image' | 'pdf'>('image')
// 当前轮询任务是否为 pdf_direct 模式（用于识别 PDF_DIRECT_FAILED 失败）
const pdfDirectActive = ref(false)
// PDF 直连失败原因（pdf_fallback 态展示）
const pdfFallbackReason = ref('')
// 回退任务提交中
const pdfFallbackSubmitting = ref(false)
const currentDoc = ref<DocumentMeta | null>(null)
const docConfirming = ref(false)
const cancelling = ref(false)

// 解析任务轮询
const {
  isPolling,
  statusText: taskStatusText,
  error: taskError,
  task: pollTask,
  startPolling,
  cancel: cancelTask,
  reset: resetPolling,
} = useAiParsePolling()

// Mixed 分组状态
const groupingQuestions = ref<GroupQuestion[]>([])
const groupingCollections = ref<QuestionCollectionSummary[]>([])

// Batch processing state（仅用于进度展示，不再有"批量审阅面板"）
const aiBatchProgress = ref({ current: 0, total: 0, text: '' })
const aiImageFile = ref<File | null>(null)
const aiUploadAreaHover = ref(false)
const fileInputRef = ref<HTMLInputElement | null>(null)

// Confirmations
const snapshotOverwriteConfirm = ref(false)
const snapshotRestoreConfirm = ref(false)
let pendingUploadAction: (() => void) | null = null
let pendingSnapshotRestore: BatchSnapshot | null = null

// Copied functions
async function copyPrompt() {
  try {
    await navigator.clipboard.writeText(RECOMMENDED_PROMPT)
    toast.success('提示词已复制，请粘贴到 AI 对话框使用')
    promptCopied.value = true
    setTimeout(() => { promptCopied.value = false }, 3000)
  } catch {
    toast.error('复制失败，请手动选择提示词文本复制')
  }
}

function parseMarkdownNow(text: string) {
  if (!text.trim()) {
    if (previewQuestions.value.length === 0) {
      aiResult.value = null
      aiError.value = ''
    }
    return
  }
  try {
    aiResult.value = parseMarkdownToQuestion(text)
    aiError.value = ''
    previewQuestions.value = []
  } catch (e: any) {
    aiResult.value = null
    aiError.value = e.message || 'Markdown 解析失败'
  }
}

function doAiParse() {
  if (!aiText.value.trim()) {
    toast.warning('请粘贴 Markdown，或将图片 / PDF 拖到左侧识别')
    return
  }
  aiParsing.value = true
  try {
    parseMarkdownNow(aiText.value)
    if (!aiResult.value && aiError.value) toast.warning(aiError.value)
  } finally {
    aiParsing.value = false
  }
}

watch(aiText, (text) => {
  if (docFlowState.value !== 'idle') return
  if (markdownParseTimer) clearTimeout(markdownParseTimer)
  markdownParseTimer = setTimeout(() => parseMarkdownNow(text), 280)
})

const previewCards = computed(() => {
  const snapshots = props.editedSnapshots ?? []
  if (previewQuestions.value.length) {
    return previewQuestions.value.map((q, i) => overlayParsedFromSnapshot(q, snapshots[i]))
  }
  if (snapshots.length) {
    return snapshots.map((s) => overlayParsedFromSnapshot(parsedStubFromSnapshot(s), s))
  }
  return aiResult.value ? [aiResult.value] : []
})

const previewCount = computed(() => previewCards.value.length)
const unsavedPreviewCount = computed(() =>
  (props.editedSnapshots ?? []).filter((s) => s && !s.saved).length,
)
const canSaveAll = computed(() => (props.editedSnapshots?.length ?? 0) > 0)

type RightPaneTab = 'source' | 'preview'
const rightPaneTab = ref<RightPaneTab>('source')

const sourceTabHint = computed(() => {
  const s = sourceState.value
  if (!s) return ''
  return displaySourceLabel(s.source_category, s.source_kind)
})

watch(previewCount, (n, prev) => {
  if (n > 0 && !prev) rightPaneTab.value = 'preview'
})

watch(currentDoc, (doc) => {
  if (doc && previewCount.value === 0) rightPaneTab.value = 'source'
})
function cardSaved(idx: number) {
  return Boolean(props.editedSnapshots?.[idx]?.saved)
}

const progressStripPct = computed(() => {
  const t = pollTask.value
  if (!t) return 35
  if (t.total_pages && t.current_page != null) {
    return Math.min(95, Math.round((t.current_page / Math.max(t.total_pages, 1)) * 100))
  }
  if (t.total_count > 0) {
    return Math.min(95, Math.round((t.processed_count / t.total_count) * 100))
  }
  return 40
})

const showOriginalSource = computed(() => Boolean(sourcePreviewUrl.value))

function cardKnowledgePoints(q: ParsedQuestion): string[] {
  if (q.knowledge_points?.length) return q.knowledge_points.filter(Boolean)
  return (q.kp_matches || [])
    .map(m => m.matched_name || m.ai_name)
    .filter((name): name is string => Boolean(name))
}

function cardQuestionType(q: ParsedQuestion): string {
  if (q.question_type === 'choice' && (q.sub_type === 'multi' || q.sub_type === 'multiple')) {
    return 'multiple'
  }
  return q.question_type
}

function parsedStubFromSnapshot(s: any): ParsedQuestion {
  const qType = s?.question_type || 'choice'
  return {
    question_type: qType,
    sub_type: s?.sub_type || '',
    difficulty: s?.difficulty || 'medium',
    stem: s?.stem || '',
    options: Array.isArray(s?.options) ? s.options : [],
    correct_answer: { kind: 'choice', value: { options: [] } },
    analysis: [],
    knowledge_points: [],
    confidence: 0,
    warnings: [],
    image_placeholders: [],
    image_urls: [],
    kp_matches: [],
  }
}

/** 用编辑页快照覆盖识别卡片上的题干/选项等，返回识别后能看到刚改过的内容 */
function overlayParsedFromSnapshot(q: ParsedQuestion, s: any): ParsedQuestion {
  if (!s) return q
  const options = Array.isArray(s.options)
    ? s.options.map((o: any) => ({ label: o.label, content: o.content || '' }))
    : q.options
  let correct_answer = q.correct_answer
  const qType = s.question_type || q.question_type
  if (qType === 'choice' || qType === 'multiple') {
    const opts = Array.isArray(s.correctAnswer)
      ? s.correctAnswer
      : (s.correctAnswer ? [s.correctAnswer] : [])
    correct_answer = { kind: 'choice', value: { options: opts } }
  } else if (qType === 'fill' && Array.isArray(s.blanks)) {
    correct_answer = { kind: 'fill', value: { blanks: s.blanks } }
  } else if (qType === 'solution' && Array.isArray(s.sub_answers)) {
    correct_answer = {
      kind: 'solution',
      value: { subs: s.sub_answers.map((content: string, i: number) => ({ sub_id: i + 1, content })) },
    }
  }
  const analysis = Array.isArray(s.solutions)
    ? s.solutions.filter((x: string) => x?.trim()).map((content: string) => ({ title: '', content }))
    : q.analysis
  const kpFromNodes = s.nodeNames && typeof s.nodeNames === 'object'
    ? Object.values(s.nodeNames as Record<string, string>).filter(Boolean)
    : []
  return {
    ...q,
    stem: normalizeChoiceAnswerBlank(s.stem ?? q.stem, qType),
    question_type: qType,
    sub_type: s.sub_type ?? q.sub_type,
    difficulty: s.difficulty ?? q.difficulty,
    options,
    correct_answer,
    analysis,
    knowledge_points: kpFromNodes.length ? kpFromNodes : q.knowledge_points,
  }
}

function revokeSourcePreview() {
  if (sourcePreviewUrl.value) {
    URL.revokeObjectURL(sourcePreviewUrl.value)
    sourcePreviewUrl.value = ''
  }
}

function setSourceFile(file: File) {
  revokeSourcePreview()
  aiImageFile.value = file
  ocrFileName.value = file.name
  sourceKind.value = isPdfFile(file) ? 'pdf' : 'image'
  sourcePreviewUrl.value = URL.createObjectURL(file)
  void saveAiSourceFile(file, sourceKind.value)
}

function fileFromClipboardEvent(e: ClipboardEvent): File | null {
  const items = e.clipboardData?.items
  if (!items) return null
  for (const item of items) {
    if (item.kind !== 'file') continue
    const file = item.getAsFile()
    if (file && (isPdfFile(file) || file.type.startsWith('image/'))) return file
  }
  return null
}

function onEditorPaste(e: ClipboardEvent) {
  const file = fileFromClipboardEvent(e)
  if (!file) return
  e.preventDefault()
  startFileParse(file)
}

function clearEditor() {
  if (docFlowState.value !== 'idle') {
    toast.warning('正在识别中，请先取消任务')
    return
  }
  aiText.value = ''
  aiResult.value = null
  aiError.value = ''
  previewQuestions.value = []
  ocrFileName.value = ''
  aiImageFile.value = null
  sourceKind.value = 'markdown'
  revokeSourcePreview()
}

function openPreviewCard(index: number) {
  if (props.savingAll) return
  // 刚识别完走 previewQuestions；返回后再恢复草稿时只剩 editedSnapshots
  const hasBatch = previewQuestions.value.length > 0 || (props.editedSnapshots?.length ?? 0) > 0
  if (hasBatch) {
    emit('open-question', index)
    if (!props.embedded) show.value = false
    return
  }
  if (aiResult.value) applyAiResult()
}

/** 从 IndexedDB 回填离开页面前缓存的 PDF/图片原稿（不重新识别） */
async function restoreOriginalSource() {
  if (sourcePreviewUrl.value) return true
  const cached = await loadAiSourceFile()
  if (!cached) return false
  setSourceFile(cached.file)
  return true
}

function presentParsedQuestions(questions: ParsedQuestion[], message: string) {
  previewQuestions.value = questions
  aiResult.value = questions[0] ?? null
  // 识别完成只结束进度，保留 currentDoc / 试卷信息，避免切回「试卷信息」时表单被卸载清空
  docFlowState.value = 'idle'
  resetPolling()
  groupingQuestions.value = []
  groupingCollections.value = []
  pdfDirectActive.value = false
  pdfFallbackReason.value = ''
  pdfFallbackSubmitting.value = false
  toast.success(message)
  emit('batch-parsed', questions)
}

function applyAiResult() {
  const q = aiResult.value
  if (!q) return
  // 直接覆盖反填 — 用户点击「应用到表单」即视为明确覆盖意图
  doApplyAiResult(q)
}

function doApplyAiResult(q: ParsedQuestion) {
  applyingAiResult.value = true

  // Reset fields
  props.form.options = []
  props.form.blanks = []
  props.form.sub_answers = ['']
  props.form.correctAnswer = ''
  props.form.solutions = ['']
  aiGeneratedFields.value = new Set()

  // Set fields — multiple 在编辑态映射为 choice + sub_type=multi
  if (q.question_type === 'multiple') {
    props.form.question_type = 'choice'
    props.form.sub_type = 'multi'
  } else {
    props.form.question_type = q.question_type
    props.form.sub_type = q.sub_type || ''
  }
  aiGeneratedFields.value.add('question_type')

  props.form.stem = q.stem
  aiGeneratedFields.value.add('stem')

  if (q.difficulty) {
    props.form.difficulty = q.difficulty
    const diffMap: Record<string, number> = { easy: 2, medium: 3, hard: 4 }
    const diffStars = diffMap[q.difficulty] || 3
    props.form.difficulty_coefficient = [0.9, 0.75, 0.55, 0.35, 0.2][diffStars - 1] ?? 0.55
    aiGeneratedFields.value.add('difficulty')
  }

  if ((q.question_type === 'choice' || q.question_type === 'multiple') && q.options) {
    props.form.options = q.options.map(o => ({ label: o.label, content: o.content }))
    if (q.correct_answer.kind === 'choice' && q.correct_answer.value.options) {
      const opts = q.correct_answer.value.options
      if (q.question_type === 'multiple' || q.sub_type === 'multi' || opts.length > 1) {
        props.form.sub_type = 'multi'
        props.form.correctAnswer = opts
      } else {
        props.form.correctAnswer = opts[0] || ''
      }
    }
    aiGeneratedFields.value.add('options')
    aiGeneratedFields.value.add('correctAnswer')
  } else if (q.question_type === 'fill') {
    if (q.correct_answer.kind === 'fill' && q.correct_answer.value.blanks) {
      props.form.blanks = q.correct_answer.value.blanks.map(b => ({ position: b.position, answer: b.answer }))
    }
    aiGeneratedFields.value.add('blanks')
  } else if (q.question_type === 'solution') {
    if (q.correct_answer.kind === 'solution' && q.correct_answer.value.subs) {
      props.form.sub_answers = q.correct_answer.value.subs.map(s => s.content)
    }
    aiGeneratedFields.value.add('sub_answers')
  }

  props.form.solutions = q.analysis.map(a => a.content)
  aiGeneratedFields.value.add('solutions')

  if (q.kp_matches?.length) {
    const kIds: string[] = []
    const cIds: string[] = []
    const mIds: string[] = []
    for (const m of q.kp_matches) {
      if (!m.matched_id) continue
      if (m.kind === 'chapter') cIds.push(m.matched_id)
      else if (m.kind === 'ability' || m.kind === 'pattern') mIds.push(m.matched_id)
      else kIds.push(m.matched_id)
    }
    knowledgeNodeIds.value = kIds
    props.form.knowledgeNodeIds = kIds
    chapterNodeIds.value = cIds
    methodNodeIds.value = mIds
    if (kIds.length + cIds.length + mIds.length > 0) {
      aiGeneratedFields.value.add('knowledge_node')
    }
  }

  props.form.hasUnsaved = true
  show.value = false
  toast.success('AI 识别结果已应用')

  emit('applied')

  nextTick(() => {
    applyingAiResult.value = false
  })
}

// File Drag/Drop & Select — 按文件实际类型路由通道：
// 图片 → 图片 OCR 通道；PDF → PDF 直连通道（自动切换到对应 tab）
function isPdfFile(file: File): boolean {
  return file.type === 'application/pdf' || /\.pdf$/i.test(file.name)
}

function handleFileDrop(e: DragEvent) {
  aiUploadAreaHover.value = false
  const file = e.dataTransfer?.files?.[0]
  if (file) startFileParse(file)
}

function onEditorDragLeave(e: DragEvent) {
  const current = e.currentTarget as HTMLElement
  const related = e.relatedTarget as Node | null
  if (related && current.contains(related)) return
  aiUploadAreaHover.value = false
}

function handleFileSelect(e: Event) {
  const file = (e.target as HTMLInputElement).files?.[0]
  if (file) startFileParse(file)
  // 允许重复选择同一文件重新触发 change
  ;(e.target as HTMLInputElement).value = ''
}

async function startFileParse(file: File) {
  if (!isPdfFile(file) && !file.type.startsWith('image/')) {
    toast.error('仅支持图片或 PDF 文件')
    return
  }
  const oldSnapshot = await hasUnfinishedSnapshot()
  if (oldSnapshot) {
    pendingUploadAction = () => doStartFileParse(file)
    snapshotOverwriteConfirm.value = true
    return
  }
  doStartFileParse(file)
}

function doStartFileParse(file: File) {
  setSourceFile(file)
  previewQuestions.value = []
  aiResult.value = null
  if (isPdfFile(file)) {
    aiMode.value = 'pdf'
    doStartPdfParse(file)
  } else {
    aiMode.value = 'image'
    doStartImageParse(file)
  }
}

async function executePendingUpload() {
  snapshotOverwriteConfirm.value = false
  await clearBatchSnapshot()
  if (pendingUploadAction) {
    pendingUploadAction()
    pendingUploadAction = null
  }
}

function dismissSnapshotOverwrite() {
  pendingUploadAction = null
}

/** 图片通道：压缩 → 上传（file_type=image，无原始 PDF）→ AI 分类 → 类型确认 */
async function doStartImageParse(file: File) {
  docFlowKind.value = 'image'
  aiImageFile.value = file
  try {
    const pages = await compressToPage(file)
    await uploadAndClassify(pages, false, undefined)
  } catch (e: any) {
    toast.error(e?.message || '图片解析失败')
    resetDocFlow()
  }
}

/** PDF 通道：前端拆页渲染（后端 pages 必填 + 回退模式复用）→ 上传（附原始 PDF）
 *  → AI 分类 → 类型确认 → pdf_direct 模式建任务（失败回 pdf_fallback 让用户选择） */
async function doStartPdfParse(file: File) {
  docFlowKind.value = 'pdf'
  aiImageFile.value = file
  try {
    const pages = await renderPdfPages(file)
    if (pages.length === 0) {
      toast.error('未能生成页面图片')
      return
    }
    await uploadAndClassify(pages, true, file)
  } catch (e: any) {
    toast.error(e?.message || 'PDF 解析失败')
    resetDocFlow()
  }
}

/** PDF → 页面图片（TD-1 默认方案 A：前端 pdfjs 渲染），上限 30 页 */
async function renderPdfPages(file: File): Promise<File[]> {
  const MAX_DOC_PAGES = 30
  const pages: PdfPageImage[] = []
  for await (const pageImg of pdfToImages(file, {
    onProgress: (cur, total) => {
      aiBatchProgress.value = { current: cur, total, text: `正在渲染 PDF 第 ${cur}/${total} 页…` }
    },
    onTruncated: (orig, actual) => {
      toast.warning(`PDF 文件过大（${orig} 页），为保证性能仅处理前 ${actual} 页`)
    },
  })) {
    pages.push(pageImg)
  }

  if (pages.length > MAX_DOC_PAGES) {
    toast.warning(`文档页数超过 ${MAX_DOC_PAGES} 页，仅处理前 ${MAX_DOC_PAGES} 页`)
    pages.length = MAX_DOC_PAGES
  }

  const out: File[] = []
  for (let i = 0; i < pages.length; i++) {
    aiBatchProgress.value = { current: i + 1, total: pages.length, text: `正在处理第 ${i + 1}/${pages.length} 页…` }
    const blob = await (await fetch(pages[i].dataUrl)).blob()
    const compressed = await compressImage(blob)
    out.push(blobToFile(compressed, `page-${pages[i].page}.webp`))
  }
  return out
}

/** 单图 → 压缩页图 */
async function compressToPage(file: File): Promise<File[]> {
  aiBatchProgress.value = { current: 0, total: 1, text: '正在压缩图片…' }
  const compressed = await compressImage(file)
  // 强制规范化 MIME 类型 — blob.type 在某些降级路径下可能为空
  const mimeType = compressed.type || 'image/webp'
  const ext = mimeType === 'image/png' ? 'png'
    : mimeType === 'image/jpeg' ? 'jpg'
    : 'webp'
  return [new File([compressed], `upload.${ext}`, { type: mimeType })]
}

/** 上传后立刻解析；分类并行预填来源条（不阻塞 OCR） */
async function uploadAndClassify(pages: File[], isPdf: boolean, originalPdf?: File) {
  docFlowState.value = 'uploading'
  aiBatchProgress.value = { current: 0, total: pages.length, text: '正在上传资料（最多 30 页）…' }
  const res = await withBackoffRetry(() =>
    documentApi.upload(pages, {
      file_type: isPdf ? 'pdf' : 'image',
      pdf: originalPdf,
    }),
  )
  const doc = res.data.data
  currentDoc.value = doc

  // OCR 先行：立刻建解析任务
  const parseMode = isPdf || docFlowKind.value === 'pdf' ? 'pdf_direct' : undefined
  pdfDirectActive.value = parseMode === 'pdf_direct'
  await startTask(doc.id, parseMode)

  // 分类并行，仅预填来源条
  void (async () => {
    try {
      const cls = await withBackoffRetry(() => documentApi.classify(doc.id))
      if (currentDoc.value?.id === doc.id) {
        currentDoc.value = cls.data.data
      }
    } catch (e: any) {
      console.warn('[AiRecognizeDialog] 分类建议失败（不影响识别）:', e?.message)
    }
  })()
}

function resetDocFlow() {
  docFlowState.value = 'idle'
  currentDoc.value = null
  sourceState.value = null
  aiBatchProgress.value = { current: 0, total: 0, text: '' }
  resetPolling()
  groupingQuestions.value = []
  groupingCollections.value = []
  pdfDirectActive.value = false
  pdfFallbackReason.value = ''
  pdfFallbackSubmitting.value = false
}

/** 后置确认来源（不启动解析；解析已在上传后开始） */
async function onConfirmDoc(body: ConfirmDocumentRequest) {
  const doc = currentDoc.value
  if (!doc) return
  docConfirming.value = true
  try {
    const res = await documentApi.confirm(doc.id, body)
    currentDoc.value = res.data.data
    const paperId = (res.data as any).paper_id as string | undefined
      || res.data.data?.metadata?.linked_paper_id
    if (paperId && sourceState.value) {
      sourceState.value = {
        ...sourceState.value,
        paper_meta: {
          ...(sourceState.value.paper_meta || { title: '' }),
          paper_id: paperId,
        },
      }
      emit('source-updated', sourceState.value)
    }
  } catch (e: any) {
    toast.error(e?.response?.data?.error || e?.message || '保存来源失败')
  } finally {
    docConfirming.value = false
  }
}

function onSourceState(state: import('@/utils/questionSource').QuestionSourceState) {
  sourceState.value = state
  emit('source-updated', state)
}

/** 创建解析任务并进入进度态（左侧保留原文） */
async function startTask(documentId: string, parseMode?: 'pdf_direct' | 'page') {
  docFlowState.value = 'progress'
  await startPolling(documentId, parseMode)
}

/** PDF 直连失败 → 用户确认回退：同 Document 重建 page 模式任务（页面图已上传，无需重传） */
async function fallbackToPageOcr() {
  const doc = currentDoc.value
  if (!doc) return
  pdfFallbackSubmitting.value = true
  try {
    pdfDirectActive.value = false
    await startTask(doc.id, 'page')
  } finally {
    pdfFallbackSubmitting.value = false
  }
}

/** 取消解析（已识别的题目保留在库中） */
async function onCancelTask() {
  cancelling.value = true
  try {
    await cancelTask()
  } finally {
    cancelling.value = false
  }
}

/// QuestionDetail → ParsedQuestion（供工作台 snapshot 转换）
function detailToParsed(d: QuestionDetail): ParsedQuestion {
  const analysis = d.analysis
    ? d.analysis.split('\n\n---\n\n').map((c, i) => ({ title: `解法${i + 1}`, content: c }))
    : []
  const diffMap: Record<number, string> = { 1: 'easy', 2: 'easy', 3: 'medium', 4: 'hard', 5: 'hard' }
  return {
    question_type: d.question_type,
    sub_type: undefined,
    difficulty: diffMap[d.difficulty] ?? 'medium',
    stem: d.stem,
    options: d.options ?? undefined,
    correct_answer: (d.correct_answer ?? { kind: 'solution', value: { subs: [] } }) as any,
    analysis,
    knowledge_points: [],
    confidence: 0.9,
    warnings: [],
    image_placeholders: [],
    // DB images 列（Worker 已把占位符替换为页面图/真实 URL，stem 内联可直接渲染）
    image_urls: Array.isArray(d.images) ? (d.images as string[]) : [],
    kp_matches: (d.knowledge_nodes ?? []).map(kn => ({
      ai_name: kn.name,
      matched_id: kn.id,
      matched_name: kn.name,
      score: kn.ai_confidence ?? 0,
      // 携带树类型，工作台按 kind 分发到 章节/知识点/方法（缺失兜底知识点）
      kind: kn.kind,
    })),
  }
}

/// 按任务产出的题目 ID 列表加载题目详情
async function loadParsedQuestions(ids: string[]): Promise<ParsedQuestion[]> {
  const out: ParsedQuestion[] = []
  for (const id of ids) {
    try {
      const { data } = await questionApi.get(id)
      out.push(detailToParsed(data))
    } catch (e: any) {
      console.warn('[AiRecognizeDialog] 加载题目详情失败:', id, e?.message)
    }
  }
  return out
}

/// 暂存项 → ParsedQuestion（解析结果暂存、确认后入库链路）
///
/// 后端不再自动落库：`parsed` 为后端 ParsedQuestion 序列化，`matched` 为
/// 三维标签匹配结果（kind 携带 chapter/knowledge/ability=题型专题），前端据此回填知识树。
/// 携带 `ai_meta`（task_id + staged_index），保存时后端据此完成容器关联/候选/标记。
function stagedToParsed(s: AiStagedQuestion, taskId: string): ParsedQuestion {
  const p = s.parsed as any
  const suggestion = s.suggestion
  const matches = suggestion?.matches ?? []
  const tagMatches: TagMatch[] = matches
    .filter((m) => m.target_type === 'tag')
    .map((m) => ({
      ai_name: m.ai_name,
      tag_id: m.target_id,
      tag_name: m.target_name,
      category: m.category || (m.dimension === 'method' ? 'method' : 'core_competence'),
      score: m.score,
      match_type: m.match_type,
    }))
  const unmatched: TaggingUnmatched[] = suggestion?.unmatched ?? []
  return {
    question_type: p.question_type ?? 'solution',
    sub_type: p.sub_type,
    difficulty: p.difficulty,
    stem: normalizeChoiceAnswerBlank(p.stem ?? '', p.question_type ?? 'solution'),
    options: p.options,
    correct_answer: (p.correct_answer ?? { kind: 'solution', value: { subs: [] } }) as any,
    analysis: Array.isArray(p.analysis) ? p.analysis : [],
    knowledge_points: Array.isArray(p.knowledge_points) ? p.knowledge_points : [],
    confidence: typeof p.confidence === 'number' ? p.confidence : 0,
    warnings: Array.isArray(p.warnings) ? p.warnings : [],
    image_placeholders: Array.isArray(p.image_placeholders) ? p.image_placeholders : [],
    image_urls: Array.isArray(s.images) ? s.images : [],
    kp_matches: (s.matched ?? []).map(m => ({
      ai_name: m.ai_name,
      matched_id: m.node_id,
      matched_name: m.node_name,
      score: m.score,
      kind: m.kind,
    })),
    ai_meta: { task_id: taskId, staged_index: s.index },
    tagging_suggestion_id: s.suggestion_id || suggestion?.suggestion_id || null,
    tagging_unmatched: unmatched,
    tag_matches: tagMatches,
    tagging_matches: matches as TaggingMatch[],
    grade_level: suggestion?.grade_level ?? null,
    cognitive_level: suggestion?.cognitive_level ?? null,
    tagging_difficulty: suggestion?.difficulty ?? null,
    tagging_question_type: suggestion?.question_type ?? null,
    tagging_stage: s.tagging_stage === 'junior' || s.tagging_stage === 'senior'
      ? s.tagging_stage
      : null,
    existing_question_id: s.existing_question_id ?? null,
  }
}

/// 监听任务终态：成功 → 工作台（非 Mixed）/ 分组（Mixed）
watch(pollTask, async (t) => {
  if (!t) return
  if (t.status === 'success' || t.status === 'partial_success') {
    const src = sourceState.value
    if (src?.create_paper && currentDoc.value) {
      void onConfirmDoc({
        source_category: src.source_category,
        source_kind: src.source_kind,
        create_paper: true,
        title: src.title,
        source_type: src.source_kind,
        sub_source_type: src.sub_source_type,
        paper_meta: src.paper_meta,
      })
    }
    pdfDirectActive.value = false
    // 暂存链路：题目尚未落库，从 staged_questions 构建待确认列表（跳过已保存/跨页合并项）
    const staged = (t.staged_questions ?? []).filter(s => !s.saved && !s.merged_into)
    if (staged.length === 0) {
      toast.warning('未识别到有效题目')
      docFlowState.value = 'confirm'
      return
    }
    const questions = staged.map(s => stagedToParsed(s, t.id))
    const unmatchedCount = staged.reduce((n, s) => {
      if (Array.isArray(s.suggestion?.unmatched)) return n + s.suggestion!.unmatched!.length
      const u = s.unmatched || {}
      return n + Object.values(u).reduce((a, arr) => a + (arr?.length ?? 0), 0)
    }, 0)
    const dupCount = staged.filter(s => s.existing_question_id).length
    const base =
      t.status === 'partial_success'
        ? `部分成功：识别到 ${t.success_count} 题（${t.failed_count} 题失败），请在右侧预览`
        : `成功识别 ${questions.length} 道题，点击右侧卡片进入编辑`
    const extra = [
      unmatchedCount > 0 ? `${unmatchedCount} 个未匹配项，确认保存后提交审核` : '',
      dupCount > 0 ? `${dupCount} 题与题库已有内容重复` : '',
    ].filter(Boolean).join('；')
    presentParsedQuestions(questions, extra ? `${base}；${extra}` : base)
  } else if (t.status === 'failed') {
    const errMsg = t.error_message || taskError.value || '解析失败'
    // PDF 直连模式失败（PDF_DIRECT_FAILED 前缀）→ 不回到确认页，
    // 进入 pdf_fallback 让用户选择是否拆页 OCR 重试
    if (pdfDirectActive.value && errMsg.startsWith('PDF_DIRECT_FAILED')) {
      pdfDirectActive.value = false
      pdfFallbackReason.value = errMsg.replace(/^PDF_DIRECT_FAILED:?\s*/, '')
      docFlowState.value = 'pdf_fallback'
    } else {
      toast.error(errMsg)
      docFlowState.value = 'confirm'
    }
  } else if (t.status === 'cancelled') {
    pdfDirectActive.value = false
    toast.warning('解析已取消，未保存任何题目')
    docFlowState.value = 'confirm'
  }
})

/** Mixed 分组完成 → 进入批量录入工作台 */
function onGroupingComplete() {
  const questions = groupingQuestions.value.map(q => q.stem).filter(Boolean)
  if (questions.length === 0) {
    toast.warning('没有可录入的题目')
    return
  }
  // 重新加载已分组题目的完整详情
  const ids = groupingQuestions.value.map(q => q.question_id)
  loadParsedQuestions(ids).then((parsed) => {
    if (parsed.length === 0) {
      toast.error('题目加载失败')
      return
    }
    presentParsedQuestions(parsed, `已完成分组，${parsed.length} 道题；点击右侧卡片进入编辑`)
  })
}

/** 重新触发 AI 分类（用户对推荐结果不满意时） */
async function reclassifyDoc() {
  const doc = currentDoc.value
  if (!doc) return
  aiBatchProgress.value = { current: 1, total: 1, text: 'AI 正在重新识别资料类型…' }
  try {
    const cls = await withBackoffRetry(() => documentApi.classify(doc.id))
    currentDoc.value = cls.data.data
  } catch (e: any) {
    toast.error(e?.response?.data?.error || e?.message || '重新识别失败')
  } finally {
    aiBatchProgress.value = { current: 0, total: 0, text: '' }
  }
}

// ============================================================
// 识别成功后：直接 emit 给父组件，进入多题工作台
// （不再弹出"批量审阅面板"，逐题应用由父组件 Tab 切换完成）
// ============================================================
async function handleBatchResults(questions: ParsedQuestion[]) {
  aiParsing.value = false
  aiBatchProgress.value = { current: 0, total: 0, text: '' }
  aiResult.value = null

  if (questions.length === 0) {
    toast.warning('未识别到任何题目')
    return
  }

  // 关闭弹窗，把数据交给父组件
  show.value = false
  emit('batch-parsed', questions)
  toast.success(`成功识别 ${questions.length} 道题，已进入批量录入工作台`)

  // 清理 snapshot（数据已交给父组件，不再需要断点续传）
  await clearBatchSnapshot()
}

// 从 snapshot 恢复（用户刷新页面后再次进入时触发）
async function restoreFromSnapshot() {
  snapshotRestoreConfirm.value = false
  if (!pendingSnapshotRestore) return

  const questions = pendingSnapshotRestore.questions
  pendingSnapshotRestore = null

  // 恢复后直接进入对应题的编辑页
  show.value = false
  emit('batch-parsed', questions)
  emit('open-question', 0)
  toast.success(`已恢复 ${questions.length} 道题`)
  await clearBatchSnapshot()
}

defineExpose({
  triggerSnapshotRestore: (snapshot: BatchSnapshot) => {
    pendingSnapshotRestore = snapshot
    snapshotRestoreConfirm.value = true
  },
  triggerFileParse: (file: File) => {
    show.value = true
    startFileParse(file)
  },
  restoreOriginalSource,
  getSourceState: () => sourceState.value,
})

onBeforeUnmount(() => {
  if (markdownParseTimer) clearTimeout(markdownParseTimer)
  // 离开页面前把内存里的原稿再写一遍 IndexedDB，覆盖「上传时尚未落盘」的旧会话
  const keepDraft = Boolean(
    sessionStorage.getItem('q-batch-draft-new-ai') || sessionStorage.getItem('q-batch-draft-new'),
  )
  if (keepDraft && aiImageFile.value) {
    void saveAiSourceFile(aiImageFile.value, sourceKind.value === 'pdf' ? 'pdf' : 'image')
  }
  revokeSourcePreview()
})
</script>

<template>
  <div :class="{ 'ai-embed-root': embedded }">
    <!-- 新建页 embedded：铺满模块区；编辑已有题仍走弹窗 -->
    <component
      :is="embedded ? 'div' : AppModal"
      :class="embedded ? 'ai-embed-shell' : undefined"
      v-bind="embedded ? {} : { modelValue: show, title: 'AI 智能识别', width: '960px' }"
      @update:model-value="(v: boolean) => { show = v }"
    >
      <div class="ai-dialog-body is-split" :class="{ 'is-embed': embedded }">
        <div class="ai-split">
          <!-- 左侧：Markdown 粘贴 + 图片/PDF 拖放 OCR -->
          <section class="ai-split-pane">
            <header class="ai-split-head">
              <span class="ai-split-title">{{ showOriginalSource ? '原文' : '标记好的内容' }}</span>
              <div class="ai-split-head-actions">
                <button type="button" class="ai-icon-btn" @click="copyPrompt">{{ promptCopied ? '已复制' : '复制 Prompt' }}</button>
                <button type="button" class="ai-icon-btn" @click="fileInputRef?.click()">上传文件</button>
                <button type="button" class="ai-icon-btn" @click="clearEditor">清空</button>
              </div>
            </header>
            <div
              class="ai-split-body"
              :class="{ dragover: aiUploadAreaHover }"
              @dragover.prevent="aiUploadAreaHover = true"
              @dragleave.prevent="onEditorDragLeave"
              @drop.prevent="handleFileDrop"
            >
              <textarea
                v-show="!showOriginalSource"
                v-model="aiText"
                class="ai-split-editor"
                placeholder="粘贴已标记的 Markdown，或将图片 / PDF 拖到此处识别…"
                @paste="onEditorPaste"
              />
              <div v-if="showOriginalSource" class="ai-source-preview">
                <iframe
                  v-if="sourceKind === 'pdf'"
                  class="ai-source-frame"
                  :src="sourcePreviewUrl"
                  title="PDF 原稿"
                />
                <img
                  v-else
                  class="ai-source-image"
                  :src="sourcePreviewUrl"
                  :alt="ocrFileName || '上传的图片原稿'"
                />
              </div>
              <input
                ref="fileInputRef"
                type="file"
                accept="image/*,application/pdf,.pdf"
                style="display:none"
                @change="handleFileSelect"
              />
              <div v-if="aiUploadAreaHover" class="ai-drop-mask">
                <AppIcon name="upload" :size="28" />
                <p>松开即可 OCR 识别</p>
              </div>
              <!-- 有原文时：仅上传中/失败回退用遮罩；解析进度用细条，不挡 PDF -->
              <div
                v-if="docFlowState === 'uploading' || docFlowState === 'pdf_fallback' || docFlowState === 'grouping' || (docFlowState === 'progress' && !showOriginalSource)"
                class="ai-flow-mask"
              >
                <template v-if="docFlowState === 'uploading'">
                  <div v-if="aiBatchProgress.total > 0" class="ai-batch-progress">
                    <div class="ai-progress-bar">
                      <div class="ai-progress-fill" :style="{ width: (aiBatchProgress.current / aiBatchProgress.total * 100) + '%' }"></div>
                    </div>
                    <span>{{ aiBatchProgress.text }}</span>
                  </div>
                  <AppButton variant="ghost" @click="resetDocFlow">取消</AppButton>
                </template>
                <template v-else-if="docFlowState === 'pdf_fallback'">
                  <div class="pdf-fallback-card">
                    <h4 class="pdf-fallback-title">PDF 直连解析失败</h4>
                    <p v-if="pdfFallbackReason" class="pdf-fallback-reason">{{ pdfFallbackReason }}</p>
                    <p class="pdf-fallback-hint">是否继续将 PDF 拆分为图片，逐页 OCR 识别？</p>
                    <div class="ai-actions">
                      <AppButton variant="ghost" @click="resetDocFlow">取消</AppButton>
                      <AppButton variant="primary" :loading="pdfFallbackSubmitting" @click="fallbackToPageOcr">
                        继续拆分图片识别
                      </AppButton>
                    </div>
                  </div>
                </template>
                <template v-else-if="docFlowState === 'progress'">
                  <TaskProgressPanel
                    :task="pollTask"
                    :status-text="taskStatusText"
                    :cancelling="cancelling"
                    @cancel="onCancelTask"
                  />
                  <div v-if="!isPolling && !pollTask" class="ai-batch-progress">
                    <div class="ai-progress-bar">
                      <div class="ai-progress-fill" :style="{ width: '100%' }"></div>
                    </div>
                    <span>{{ taskStatusText || '提交中…' }}</span>
                  </div>
                </template>
                <template v-else-if="docFlowState === 'grouping'">
                  <QuestionGroupingStep
                    :questions="groupingQuestions"
                    :collections="groupingCollections"
                    @complete="onGroupingComplete"
                  />
                </template>
              </div>
              <div
                v-else-if="docFlowState === 'progress' && showOriginalSource"
                class="ai-progress-strip"
              >
                <div class="ai-progress-bar">
                  <div
                    class="ai-progress-fill"
                    :style="{ width: progressStripPct + '%' }"
                  />
                </div>
                <span class="ai-progress-strip-text">{{ taskStatusText || '识别中…' }}</span>
                <AppButton variant="ghost" size="sm" :loading="cancelling" @click="onCancelTask">取消</AppButton>
              </div>
            </div>
            <footer class="ai-split-foot">
              <span v-if="ocrFileName" class="ai-file-chip">{{ ocrFileName }}</span>
              <span v-else class="ai-split-hint">支持 Markdown、截图、图片、PDF</span>
              <div v-if="aiError" class="ai-error">{{ aiError }}</div>
              <AppButton
                v-if="!showOriginalSource"
                variant="primary"
                size="sm"
                :loading="aiParsing"
                :disabled="!aiText.trim() || docFlowState !== 'idle'"
                @click="doAiParse"
              >
                开始识别
              </AppButton>
            </footer>
          </section>

          <!-- 右侧：试卷信息 / 题目预览 可切换 -->
          <section class="ai-split-pane">
            <header class="ai-split-head">
              <div class="ai-pane-tabs" role="tablist" aria-label="右侧内容">
                <button
                  type="button"
                  role="tab"
                  class="ai-pane-tab"
                  :class="{ active: rightPaneTab === 'source' }"
                  :aria-selected="rightPaneTab === 'source'"
                  @click="rightPaneTab = 'source'"
                >
                  试卷信息
                </button>
                <button
                  type="button"
                  role="tab"
                  class="ai-pane-tab"
                  :class="{ active: rightPaneTab === 'preview' }"
                  :aria-selected="rightPaneTab === 'preview'"
                  @click="rightPaneTab = 'preview'"
                >
                  题目预览
                  <span v-if="previewCount > 0" class="ai-tab-count">{{ previewCount }}</span>
                </button>
              </div>
              <div class="ai-split-head-actions">
                <AppButton
                  v-if="canSaveAll && rightPaneTab === 'preview'"
                  variant="primary"
                  size="sm"
                  :loading="savingAll"
                  :disabled="savingAll || unsavedPreviewCount === 0"
                  @click="emit('save-all')"
                >
                  <AppIcon name="save" :size="14" />
                  {{ unsavedPreviewCount === 0 ? '已全部保存' : (unsavedPreviewCount === previewCount ? '全部保存' : `保存剩余 ${unsavedPreviewCount} 题`) }}
                </AppButton>
              </div>
            </header>
            <div class="ai-split-body ai-preview-body">
              <div v-show="rightPaneTab === 'source'" class="ai-source-pane">
                <SourceCascadeBar
                  v-if="currentDoc"
                  variant="panel"
                  :doc="currentDoc"
                  :saving="docConfirming"
                  @confirm="onConfirmDoc"
                  @update:state="onSourceState"
                />
                <div v-else class="ai-preview-empty">
                  <span class="ai-empty-kicker">试卷信息</span>
                  <p>上传文件后即可在此填写来源与试卷属性</p>
                </div>
                <button
                  v-if="currentDoc?.ai_classification"
                  type="button"
                  class="ai-reclassify-link"
                  @click="reclassifyDoc"
                >重新识别来源</button>
              </div>
              <div v-show="rightPaneTab === 'preview'" class="ai-preview-pane">
                <button
                  v-if="currentDoc && sourceTabHint"
                  type="button"
                  class="ai-source-summary"
                  @click="rightPaneTab = 'source'"
                >
                  <span>{{ sourceTabHint }}</span>
                  <span>编辑</span>
                </button>
                <div
                  v-if="previewCount === 0"
                  class="ai-preview-empty"
                >
                  <span class="ai-empty-kicker">题目预览</span>
                  <p>{{ docFlowState === 'idle' && !currentDoc ? '识别完成后将在此展示题目卡片' : '识别中，完成后将在此展示' }}</p>
                </div>
                <div v-else class="ai-q-card-list">
                  <p class="ai-preview-hint">点卡片进入该题编辑；校对完成后可一次性全部保存</p>
                  <article
                    v-for="(card, idx) in previewCards"
                    :key="idx"
                    class="ai-q-card"
                    :class="{ 'is-saved': cardSaved(idx) }"
                    role="button"
                    tabindex="0"
                    @click="openPreviewCard(idx)"
                    @keydown.enter.prevent="openPreviewCard(idx)"
                  >
                    <span class="ai-q-index">第 {{ idx + 1 }} 题</span>
                    <span v-if="cardSaved(idx)" class="ai-q-saved">已保存</span>
                    <div class="ai-q-card-header">
                      <div class="ai-q-card-tags">
                        <AppBadge :color="typeBadgeColor(cardQuestionType(card))" class="flex-shrink-0">
                          {{ typeLabel(cardQuestionType(card)) }}
                        </AppBadge>
                        <span v-if="card.difficulty" class="ai-q-ghost-tag flex-shrink-0">
                          <span class="ai-q-dot" :class="`ai-q-dot--${diffBadgeColor(card.difficulty)}`"></span>
                          {{ diffLabel(card.difficulty) }}
                        </span>
                      </div>
                    </div>
                    <div class="ai-q-card-body">
                      <div class="ai-q-stem">
                        <LatexRender :text="card.stem" />
                      </div>
                      <QuestionOptions
                        v-if="(card.question_type === 'choice' || card.question_type === 'multiple') && card.options?.length"
                        :options="card.options"
                      />
                    </div>
                    <div v-if="cardKnowledgePoints(card).length" class="ai-q-kps">
                      <span
                        v-for="kp in cardKnowledgePoints(card).slice(0, 4)"
                        :key="kp"
                        class="ai-q-kp"
                      >{{ kp }}</span>
                    </div>
                  </article>
                </div>
              </div>
            </div>
          </section>
        </div>
      </div>
    </component>

    <!-- 快照覆盖警告 -->
    <AppConfirm
      v-model="snapshotOverwriteConfirm"
      title="发现未完成的批量录入"
      message="检测到您还有未处理完的批量题目。继续上传新题目将清空之前的记录，是否继续？"
      confirm-text="丢弃旧数据"
      danger
      @confirm="executePendingUpload"
      @cancel="dismissSnapshotOverwrite"
    />
    <!-- 快照恢复弹窗 -->
    <AppConfirm
      v-model="snapshotRestoreConfirm"
      title="恢复未完成的批量录入"
      message="检测到上次有未完成的批量录入，是否继续？"
      confirm-text="继续录入"
      @confirm="restoreFromSnapshot"
    />
  </div>
</template>

<style scoped>
/* ===== AI 智能识别弹窗 ===== */
.ai-embed-root {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.ai-embed-shell {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  background: transparent;
  border: none;
  overflow: hidden;
  padding: 0;
}

.ai-dialog-body {
  display: flex;
  flex-direction: column;
  gap: 16px;
  height: 520px;
  min-height: 380px;
}

.ai-dialog-body.is-embed,
.ai-dialog-body.is-split {
  height: auto;
  min-height: 0;
  flex: 1;
  gap: 0;
  container-type: inline-size;
  container-name: ai-workspace;
}

.ai-dialog-body.is-split:not(.is-embed) {
  height: 560px;
  min-height: 560px;
}

.ai-split {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: minmax(0, 1.08fr) minmax(280px, 0.92fr);
  gap: 14px;
  font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif;
}

@container ai-workspace (max-width: 920px) {
  .ai-split {
    grid-template-columns: minmax(0, 0.92fr) minmax(300px, 1.08fr);
    gap: 10px;
  }
}

@container ai-workspace (max-width: 760px) {
  .ai-split {
    grid-template-columns: 1fr;
    grid-template-rows: minmax(180px, 34%) minmax(0, 1fr);
    gap: 10px;
  }

  .ai-split-pane {
    border-radius: 14px;
  }
}

@container ai-workspace (max-width: 520px) {
  .ai-split {
    grid-template-rows: minmax(160px, 28%) minmax(0, 1fr);
    gap: 8px;
  }

  .ai-split-head {
    padding: 8px 10px;
  }

  .ai-pane-tabs {
    min-width: 0;
    width: 100%;
  }

  .ai-source-pane {
    padding: 10px 10px 14px;
  }
}

.ai-split-pane {
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-card, #fff);
  border: 1px solid color-mix(in srgb, var(--text-primary, #1d1d1f) 8%, transparent);
  border-radius: 18px;
  overflow: hidden;
  box-shadow:
    0 1px 2px rgba(0, 0, 0, 0.03),
    0 10px 28px rgba(0, 0, 0, 0.04);
}

.ai-split-head {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 10px 12px;
  background: color-mix(in srgb, var(--bg-card, #fff) 72%, transparent);
  border-bottom: 1px solid color-mix(in srgb, var(--text-primary, #1d1d1f) 6%, transparent);
  backdrop-filter: saturate(180%) blur(16px);
  flex-wrap: wrap;
}

.ai-split-title {
  font-size: 13px;
  font-weight: 650;
  letter-spacing: -0.01em;
  color: var(--text-primary);
}

.ai-pane-tabs {
  display: inline-grid;
  grid-template-columns: 1fr 1fr;
  min-width: min(210px, 100%);
  flex: 1 1 180px;
  max-width: 280px;
  padding: 3px;
  background: color-mix(in srgb, var(--text-primary, #1d1d1f) 7%, transparent);
  border-radius: 10px;
}

.ai-pane-tab {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  min-height: 28px;
  padding: 0 12px;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--text-secondary, #6e6e73);
  font-size: 12px;
  font-weight: 600;
  letter-spacing: -0.01em;
  cursor: pointer;
  transition: background 0.18s ease, color 0.18s ease, box-shadow 0.18s ease;
}

.ai-pane-tab.active {
  color: var(--text-primary, #1d1d1f);
  background: var(--bg-card, #fff);
  box-shadow:
    0 1px 2px rgba(0, 0, 0, 0.08),
    0 2px 8px rgba(0, 0, 0, 0.05);
}

.ai-tab-count {
  min-width: 16px;
  height: 16px;
  padding: 0 5px;
  border-radius: 999px;
  background: var(--accent, #0071e3);
  color: #fff;
  font-size: 10px;
  font-weight: 700;
  line-height: 16px;
}

[data-theme='dark'] .ai-pane-tab.active {
  background: var(--bg-elevated, #3a3a3c);
}

[data-theme='dark'] .ai-progress-strip {
  background: rgba(28, 28, 30, 0.78);
}

.ai-split-head-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}

.ai-icon-btn {
  border: none;
  background: transparent;
  color: var(--accent, #0071e3);
  font-size: 12px;
  font-weight: 550;
  padding: 5px 9px;
  border-radius: 8px;
  cursor: pointer;
}

.ai-icon-btn:hover {
  background: color-mix(in srgb, var(--accent, #0071e3) 10%, transparent);
}

.ai-qcount {
  font-size: 12px;
  font-weight: 650;
  color: var(--success, #16a34a);
  background: var(--success-light, rgba(22, 163, 74, 0.12));
  padding: 2px 8px;
  border-radius: 999px;
}

.ai-split-body {
  position: relative;
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.ai-split-body.dragover {
  outline: 2px dashed var(--accent, #3b82f6);
  outline-offset: -8px;
}

.ai-split-editor {
  flex: 1;
  min-height: 0;
  width: 100%;
  border: none;
  resize: none;
  padding: 14px 16px;
  font-size: 14px;
  line-height: 1.65;
  font-family: ui-monospace, 'SF Mono', Consolas, monospace;
  background: transparent;
  color: var(--text-primary);
}

.ai-split-editor:focus {
  outline: none;
}

.ai-drop-mask,
.ai-flow-mask {
  position: absolute;
  inset: 0;
  z-index: 2;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 16px;
  overflow: auto;
}

.ai-drop-mask {
  background: color-mix(in srgb, var(--bg-primary) 82%, transparent);
  color: var(--text-primary);
  font-size: 14px;
  font-weight: 600;
  pointer-events: none;
}

.ai-flow-mask {
  background: var(--bg-card, var(--bg-primary));
  align-items: stretch;
}

.ai-split-foot {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px 10px;
  border-top: 1px solid color-mix(in srgb, var(--text-primary, #1d1d1f) 6%, transparent);
  background: color-mix(in srgb, var(--bg-card, #fff) 78%, transparent);
}

.ai-split-foot-end {
  justify-content: flex-end;
}

.ai-split-hint {
  flex: 1;
  font-size: 12px;
  color: var(--text-secondary);
}

.ai-file-chip {
  flex: 1;
  min-width: 0;
  font-size: 12px;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ai-preview-body {
  overflow: auto;
  background: var(--bg-muted, #f5f5f7);
}

.ai-source-pane,
.ai-preview-pane {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.ai-source-pane {
  padding: 12px 14px 16px;
}

.ai-source-summary {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  margin: 12px 14px 0;
  padding: 10px 12px;
  border: 0;
  border-radius: 12px;
  background: var(--bg-card, #fff);
  color: var(--text-primary, #1d1d1f);
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.04);
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  text-align: left;
}

.ai-source-summary span:last-child {
  color: var(--accent, #0071e3);
  font-weight: 550;
}

.ai-reclassify-link {
  align-self: flex-end;
  margin-top: 8px;
  border: none;
  background: none;
  font-size: 12px;
  color: var(--accent, #0071e3);
  cursor: pointer;
  padding: 0 2px;
}

.ai-progress-strip {
  position: absolute;
  left: 10px;
  right: 10px;
  bottom: 10px;
  z-index: 5;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  background: rgba(255, 255, 255, 0.78);
  border: 1px solid color-mix(in srgb, var(--text-primary, #1d1d1f) 8%, transparent);
  border-radius: 14px;
  backdrop-filter: saturate(180%) blur(18px);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.08);
}

.ai-progress-strip .ai-progress-bar {
  flex: 1;
  height: 6px;
}

.ai-progress-strip-text {
  font-size: 12px;
  color: var(--text-secondary);
  white-space: nowrap;
}

.ai-source-preview {
  flex: 1;
  min-height: 0;
  background: var(--bg-muted, #e8eaed);
}

.ai-source-frame {
  width: 100%;
  height: 100%;
  border: none;
  background: #525659;
}

.ai-source-image {
  display: block;
  max-width: 100%;
  height: auto;
  margin: 0 auto;
}

.ai-preview-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 24px;
  color: var(--text-secondary, #6e6e73);
  font-size: 14px;
  text-align: center;
}

.ai-empty-kicker {
  font-size: 11px;
  font-weight: 650;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--text-tertiary, #86868b);
}

.ai-preview-empty p {
  margin: 0;
  max-width: 220px;
  line-height: 1.5;
}

.ai-q-card-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 12px 14px 16px;
}

.ai-preview-hint {
  margin: 0;
  font-size: 12px;
  color: var(--text-secondary, #6e6e73);
}

.ai-q-card {
  position: relative;
  background: var(--bg-card, #fff);
  border-radius: 16px;
  border: 1px solid color-mix(in srgb, var(--text-primary, #1d1d1f) 6%, transparent);
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.04);
  cursor: pointer;
  transition: transform 0.18s ease, box-shadow 0.18s ease;
}

.ai-q-card:hover,
.ai-q-card:focus-visible {
  transform: translateY(-1px);
  box-shadow: 0 10px 28px rgba(0, 0, 0, 0.08);
  outline: none;
}

.ai-q-index {
  position: absolute;
  top: 0;
  left: 0;
  padding: 5px 12px 5px 12px;
  font-size: 11px;
  font-weight: 600;
  line-height: 1.4;
  color: var(--text-tertiary, #86868b);
  background: color-mix(in srgb, var(--text-primary, #1d1d1f) 5%, transparent);
  border-radius: 16px 0 10px 0;
  pointer-events: none;
}

.ai-q-saved {
  position: absolute;
  top: 8px;
  right: 12px;
  font-size: 11px;
  font-weight: 650;
  color: var(--success, #16a34a);
  background: var(--success-light, rgba(22, 163, 74, 0.12));
  padding: 2px 8px;
  border-radius: 999px;
  pointer-events: none;
}

.ai-q-card.is-saved {
  border-color: rgba(22, 163, 74, 0.35);
}

.ai-q-card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 26px 16px 10px;
  border-bottom: 1px solid var(--divider, var(--border));
}

.ai-q-card-tags {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
  min-width: 0;
}

.ai-q-ghost-tag {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 12px;
  color: var(--text-secondary);
}

.ai-q-dot {
  display: inline-block;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  flex-shrink: 0;
}

.ai-q-dot--green { background: var(--success); }
.ai-q-dot--yellow { background: var(--warning); }
.ai-q-dot--red { background: var(--danger); }

.ai-q-card-body {
  padding: 14px 16px 12px;
}

.ai-q-stem {
  font-size: 14.5px;
  line-height: 1.75;
  color: var(--text-primary);
}

.ai-q-stem :deep(.katex) {
  font-size: 1.02em;
}

.ai-q-card :deep(.q-option-content) {
  overflow-x: auto;
  white-space: nowrap;
  scrollbar-width: none;
  -ms-overflow-style: none;
}

.ai-q-card :deep(.q-option-content::-webkit-scrollbar) {
  display: none;
}

.ai-q-kps {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  padding: 0 16px 14px;
}

.ai-q-kp {
  font-size: 11px;
  color: var(--text-secondary);
  background: var(--bg-muted, var(--bg-input));
  border-radius: 999px;
  padding: 2px 8px;
}

[data-theme='dark'] .ai-q-card {
  border-color: rgba(255, 255, 255, 0.08);
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.3);
}

[data-theme='dark'] .ai-q-card:hover,
[data-theme='dark'] .ai-q-card:focus-visible {
  border-color: rgba(10, 132, 255, 0.4);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45);
}

[data-theme='dark'] .ai-q-index {
  background: rgba(148, 163, 184, 0.12);
}

/* 输入区（Tab + 内容）：撑满主体，供内部 flex:1 元素分配剩余空间 */
.ai-input-section {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.ai-mode-tabs {
  flex-shrink: 0;
  display: flex;
  gap: 4px;
  border-bottom: 2px solid var(--border);
  padding-bottom: 0;
}

.ai-mode-tabs button {
  padding: 8px 16px;
  background: none;
  border: none;
  border-bottom: 2px solid transparent;
  margin-bottom: -2px;
  font-size: 14px;
  color: var(--text-secondary);
  cursor: pointer;
  transition: all 0.2s;
}

.ai-mode-tabs button:hover {
  color: var(--text-primary);
}

.ai-mode-tabs button.active {
  color: var(--purple);
  border-bottom-color: var(--purple);
  font-weight: 600;
}

/* ===== Markdown 模式：引导卡片 ===== */
.ai-guide-card {
  flex-shrink: 0;
  background: var(--bg-secondary, var(--bg-input));
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.ai-guide-steps {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.ai-guide-step {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 13px;
  color: var(--text-primary);
  line-height: 1.4;
}

.ai-step-num {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border-radius: 50%;
  background: var(--purple);
  color: white;
  font-size: 12px;
  font-weight: 700;
  flex-shrink: 0;
}

.ai-prompt-details {
  background: var(--bg-input);
  border-radius: 6px;
  border: 1px solid var(--border);
  overflow: hidden;
}

.ai-prompt-summary {
  display: flex;
  align-items: center;
  padding: 10px 12px;
  cursor: pointer;
  font-size: 12px;
  color: var(--text-secondary);
  font-weight: 500;
  list-style: none;
  user-select: none;
}

.ai-prompt-summary::-webkit-details-marker {
  display: none;
}

.ai-prompt-summary:hover {
  color: var(--text-primary);
}

.ai-prompt-summary-text {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.ai-prompt-details[open] .ai-prompt-summary .app-icon {
  transform: rotate(180deg);
  transition: transform 0.2s ease;
}

.ai-prompt-summary .app-icon {
  transition: transform 0.2s ease;
}

.ai-prompt-preview {
  font-family: 'SF Mono', 'Fira Code', 'Consolas', monospace;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-secondary);
  padding: 10px 12px;
  border-top: 1px solid var(--border);
  max-height: 220px;
  overflow-y: auto;
  white-space: pre-wrap;
  word-break: break-word;
}

/* 一键复制按钮 — 宽度自适应，文字居中 */
.ai-copy-btn {
  align-self: stretch;
  justify-content: center;
}

.ai-hint {
  font-size: 13px;
  color: var(--text-secondary);
  line-height: 1.5;
}

.ai-textarea {
  width: 100%;
  flex: 1;
  min-height: 180px;
  padding: 12px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  font-size: 14px;
  font-family: inherit;
  resize: vertical;
  background: var(--bg-input);
  color: var(--text-primary);
  line-height: 1.6;
}

.ai-textarea:focus {
  outline: none;
  border-color: var(--purple);
}

.ai-error {
  padding: 10px 12px;
  background: var(--danger-light);
  color: var(--danger);
  border-radius: var(--radius);
  font-size: 13px;
}

.ai-actions {
  flex-shrink: 0;
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}

/* 结果预览：撑满主体并内部滚动（固定高度下长题干不撑破弹窗） */
.ai-result-section {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.ai-result-meta {
  display: flex;
  align-items: center;
  gap: 10px;
}

.ai-result-type {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
}

.ai-warnings {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.ai-warning-item {
  font-size: 12px;
  color: var(--warning);
  background: var(--warning-light);
  padding: 6px 10px;
  border-radius: var(--radius);
}

.ai-result-preview {
  max-height: 400px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.ai-preview-block {
  border-left: 3px solid var(--purple-light);
  padding-left: 12px;
}

.ai-preview-label {
  font-size: 12px;
  font-weight: 700;
  color: var(--purple);
  margin-bottom: 4px;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

.ai-preview-content {
  font-size: 14px;
  color: var(--text-primary);
  line-height: 1.6;
}

.ai-preview-option {
  font-size: 14px;
  color: var(--text-primary);
  padding: 2px 0;
}

.ai-opt-label {
  font-weight: 700;
  margin-right: 4px;
}

.ai-preview-analysis {
  font-size: 13px;
  color: var(--text-secondary);
  padding: 6px 0;
}

/* 图片/PDF 上传区 */
/* 上传区容器：撑满主体剩余空间，Dropzone 随之伸缩 */
.ai-upload-section { flex: 1; min-height: 0; display: flex; flex-direction: column; gap: 16px; }
.ai-upload-area {
  flex: 1;
  justify-content: center;
  border: 2px dashed var(--border-color);
  border-radius: 12px;
  padding: 48px 24px;
  text-align: center;
  cursor: pointer;
  transition: all 0.2s;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  color: var(--text-secondary);
}
.ai-upload-area:hover, .ai-upload-area.dragover {
  border-color: var(--accent);
  background: var(--bg-hover);
}
.ai-upload-hint { font-size: 15px; font-weight: 500; color: var(--text-primary); margin: 0; }
.ai-upload-sub { font-size: 13px; margin: 0; }

/* PDF 直连失败回退卡片（margin auto 垂直居中于撑满的上传区内） */
.pdf-fallback-card {
  margin: auto 0;
  border: 1px solid var(--border-color);
  border-radius: 12px;
  padding: 28px 24px;
  text-align: center;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  background: var(--bg-secondary, var(--bg-input));
}
.pdf-fallback-icon {
  width: 52px;
  height: 52px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(240, 160, 30, 0.12);
  color: var(--warning, #e8a030);
}
.pdf-fallback-title { margin: 0; font-size: 16px; font-weight: 600; color: var(--text-primary); }
.pdf-fallback-reason {
  margin: 0;
  font-size: 13px;
  color: var(--text-secondary);
  max-width: 460px;
  word-break: break-all;
  max-height: 90px;
  overflow-y: auto;
}
.pdf-fallback-hint { margin: 0; font-size: 14px; color: var(--text-primary); }

/* 批量进度 */
.ai-batch-progress { display: flex; align-items: center; gap: 12px; }
.ai-progress-bar { flex: 1; height: 6px; background: color-mix(in srgb, var(--text-primary, #1d1d1f) 8%, transparent); border-radius: 999px; overflow: hidden; }
.ai-progress-fill { height: 100%; background: var(--accent, #0071e3); transition: width 0.3s; border-radius: 999px; }
.ai-batch-progress span { font-size: 13px; color: var(--text-secondary); white-space: nowrap; }
</style>
