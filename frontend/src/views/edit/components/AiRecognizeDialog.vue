<script setup lang="ts">
import { ref, watch, nextTick } from 'vue'
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
} from '@/api/client'
import { AppButton, AppModal, AppConfirm, AppIcon } from '@/components/ui'
import { useToast } from '@/composables/useToast'
import { useAiParsePolling } from '@/composables/useAiParsePolling'
import { parseMarkdownToQuestion, RECOMMENDED_PROMPT } from '@/utils/parseMarkdown'
import { compressImage, blobToFile } from '@/utils/imageCompressor'
import { withBackoffRetry } from '@/utils/concurrency'
import { pdfToImages, type PdfPageImage } from '@/utils/pdfToImages'
import { clearBatchSnapshot, hasUnfinishedSnapshot, type BatchSnapshot } from '@/utils/batchSnapshot'
import DocumentTypeConfirmStep from './DocumentTypeConfirmStep.vue'
import TaskProgressPanel from './TaskProgressPanel.vue'
import QuestionGroupingStep, { type GroupQuestion } from './QuestionGroupingStep.vue'

const show = defineModel<boolean>({ required: true })
const applyingAiResult = defineModel<boolean>('applyingAiResult', { default: false })
const knowledgeNodeIds = defineModel<string[]>('knowledgeNodeIds', { required: true })
const aiGeneratedFields = defineModel<Set<string>>('aiGeneratedFields', { required: true })

const props = defineProps<{
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
}>()

const emit = defineEmits<{
  (e: 'applied'): void
  // 批量识别成功后，把所有题目一次性抛给父组件进入多题工作台
  (e: 'batch-parsed', questions: ParsedQuestion[]): void
}>()

const toast = useToast()

// AI Mode tab: 'markdown' | 'image' | 'pdf'（图片与 PDF 各走独立通道）
const aiMode = ref<'markdown' | 'image' | 'pdf'>('markdown')
const aiText = ref('')
const aiError = ref('')
const aiParsing = ref(false)
const aiResult = ref<ParsedQuestion | null>(null)
const promptCopied = ref(false)

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

function doAiParse() {
  if (!aiText.value.trim()) {
    toast.warning('请输入题目文本')
    return
  }
  aiError.value = ''
  aiResult.value = null
  aiParsing.value = true
  try {
    // Markdown 模式：纯前端解析（不调用后端）
    aiResult.value = parseMarkdownToQuestion(aiText.value)
  } catch (e: any) {
    aiError.value = e.message || 'Markdown 解析失败'
  } finally {
    aiParsing.value = false
  }
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

  // Set fields
  props.form.question_type = q.question_type
  props.form.sub_type = q.sub_type || ''
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

  if (q.question_type === 'choice' && q.options) {
    props.form.options = q.options.map(o => ({ label: o.label, content: o.content }))
    if (q.correct_answer.kind === 'choice' && q.correct_answer.value.options) {
      const opts = q.correct_answer.value.options
      if (q.sub_type === 'multi' || opts.length > 1) {
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
    const highConfidenceMatch = q.kp_matches.find(m => m.score >= 0.95 && m.matched_id)
    if (highConfidenceMatch) {
      knowledgeNodeIds.value = [highConfidenceMatch.matched_id!]
      props.form.knowledgeNodeIds = [highConfidenceMatch.matched_id!]
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

/** 上传页图集 → 触发 AI 分类 → 进入类型确认步骤 */
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

  aiBatchProgress.value = { current: 1, total: 1, text: 'AI 正在识别资料类型…' }
  const cls = await withBackoffRetry(() => documentApi.classify(doc.id))
  currentDoc.value = cls.data.data
  docFlowState.value = 'confirm'
  aiBatchProgress.value = { current: 0, total: 0, text: '' }
}

function resetDocFlow() {
  docFlowState.value = 'idle'
  currentDoc.value = null
  aiBatchProgress.value = { current: 0, total: 0, text: '' }
  resetPolling()
  groupingQuestions.value = []
  groupingCollections.value = []
  pdfDirectActive.value = false
  pdfFallbackReason.value = ''
  pdfFallbackSubmitting.value = false
}

/** 用户确认资料类型 → 创建解析任务（P0-C）
 *  PDF 通道以 pdf_direct 模式建任务：仅直连解析，失败回 pdf_fallback 让用户选择回退；
 *  图片通道缺省模式（自动降级，与既有行为一致） */
async function onConfirmDoc(body: ConfirmDocumentRequest) {
  const doc = currentDoc.value
  if (!doc) return
  docConfirming.value = true
  try {
    const res = await documentApi.confirm(doc.id, body)
    currentDoc.value = res.data.data
    toast.success('资料类型已确认，开始解析')
    const parseMode = docFlowKind.value === 'pdf' ? 'pdf_direct' : undefined
    pdfDirectActive.value = parseMode === 'pdf_direct'
    await startTask(doc.id, parseMode)
  } catch (e: any) {
    toast.error(e?.response?.data?.error || e?.message || '确认失败')
  } finally {
    docConfirming.value = false
  }
}

/** 创建解析任务并进入进度页 */
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
/// 三维标签匹配结果（kind 携带 chapter/knowledge/ability），前端据此回填知识树。
/// 携带 `ai_meta`（task_id + staged_index），保存时后端据此完成容器关联/候选/标记。
function stagedToParsed(s: AiStagedQuestion, taskId: string): ParsedQuestion {
  const p = s.parsed as any
  return {
    question_type: p.question_type ?? 'solution',
    sub_type: p.sub_type,
    difficulty: p.difficulty,
    stem: p.stem ?? '',
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
  }
}

/// 监听任务终态：成功 → 工作台（非 Mixed）/ 分组（Mixed）
watch(pollTask, async (t) => {
  if (!t) return
  if (t.status === 'success' || t.status === 'partial_success') {
    pdfDirectActive.value = false
    // 暂存链路：题目尚未落库，从 staged_questions 构建待确认列表（跳过已保存/跨页合并项）
    const staged = (t.staged_questions ?? []).filter(s => !s.saved && !s.merged_into)
    if (staged.length === 0) {
      toast.warning('未识别到有效题目')
      docFlowState.value = 'confirm'
      return
    }
    const questions = staged.map(s => stagedToParsed(s, t.id))
    // 暂存链路下题目未落库，Mixed 分组改为在工作台保存时按暂存项 collection_id 归组；
    // 分组交互（batchAddQuestions 依赖已落库题目）在确认入库前不再适用。
    emit('batch-parsed', questions)
    resetDocFlow()
    show.value = false
    const base =
      t.status === 'partial_success'
        ? `部分成功：${t.success_count} 题待确认（${t.failed_count} 题失败）`
        : `成功识别 ${questions.length} 道题，已进入批量录入工作台（确认保存后入库）`
    // 未匹配标签将在确认保存时进入候选审核队列
    toast.success(
      t.pending_candidate_count > 0
        ? `${base}；${t.pending_candidate_count} 个未匹配标签待候选审核`
        : base,
    )
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
    emit('batch-parsed', parsed)
    resetDocFlow()
    show.value = false
    toast.success(`已完成分组，${parsed.length} 道题进入批量录入工作台`)
  })
}

/** 重新触发 AI 分类（用户对推荐结果不满意时） */
async function reclassifyDoc() {
  const doc = currentDoc.value
  if (!doc) return
  docFlowState.value = 'uploading'
  aiBatchProgress.value = { current: 1, total: 1, text: 'AI 正在重新识别资料类型…' }
  try {
    const cls = await withBackoffRetry(() => documentApi.classify(doc.id))
    currentDoc.value = cls.data.data
    docFlowState.value = 'confirm'
  } catch (e: any) {
    toast.error(e?.response?.data?.error || e?.message || '重新识别失败')
    docFlowState.value = 'confirm'
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

  // 直接 emit，进入多题工作台
  show.value = false
  emit('batch-parsed', questions)
  toast.success(`已恢复 ${questions.length} 道题，进入批量录入工作台`)
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
  }
})
</script>

<template>
  <div>
    <!-- AI 智能识别弹窗 -->
    <!-- width 固定 680px：三个 Tab（Markdown/图片/PDF）切换时外框尺寸完全一致 -->
    <AppModal v-model="show" title="AI 智能识别" width="680px">
      <div class="ai-dialog-body">
        <!-- 输入区 -->
        <div v-if="!aiResult" class="ai-input-section">
          <!-- 模式切换 Tab（图片 / PDF 各走独立解析通道） -->
          <div class="ai-mode-tabs">
            <button :class="{ active: aiMode === 'markdown' }" @click="aiMode = 'markdown'">Markdown 粘贴</button>
            <button :class="{ active: aiMode === 'image' }" @click="aiMode = 'image'">图片识别</button>
            <button :class="{ active: aiMode === 'pdf' }" @click="aiMode = 'pdf'">PDF 识别</button>
          </div>

          <!-- Markdown 模式：引导卡片（折叠式提示词） -->
          <div v-if="aiMode === 'markdown'" class="ai-guide-card">
            <!-- 步骤指示 -->
            <div class="ai-guide-steps">
              <div class="ai-guide-step">
                <span class="ai-step-num">1</span>
                <span class="ai-step-text">复制下方标准提示词并发送给 AI</span>
              </div>
              <div class="ai-guide-step">
                <span class="ai-step-num">2</span>
                <span class="ai-step-text">将 AI 生成的 Markdown 粘贴到下方文本框</span>
              </div>
            </div>

            <!-- 折叠的提示词详情 -->
            <details class="ai-prompt-details">
              <summary class="ai-prompt-summary">
                <span class="ai-prompt-summary-text">
                  <AppIcon name="chevron-down" :size="14" />
                  <span>查看完整提示词</span>
                </span>
              </summary>
              <div class="ai-prompt-preview">{{ RECOMMENDED_PROMPT }}</div>
            </details>

            <!-- 一键复制按钮（含成功态视觉反馈） -->
            <AppButton
              class="ai-copy-btn"
              :variant="promptCopied ? 'success' : 'primary'"
              size="md"
              @click="copyPrompt"
            >
              <AppIcon :name="promptCopied ? 'check-circle' : 'copy'" :size="16" />
              {{ promptCopied ? '已复制，去粘贴到 AI' : '一键复制标准 Prompt' }}
            </AppButton>
          </div>

          <!-- 图片/PDF 上传区（V2.1.1：上传 → 分类 → 确认类型 → 解析）；
               idle 态按 tab 区分通道，非 idle 态两个通道共用流程状态机 -->
          <div v-if="aiMode === 'image' || aiMode === 'pdf'" class="ai-upload-section">
            <!-- ① 选文件（图片：图片 OCR 通道；PDF：优先直连解析） -->
            <template v-if="docFlowState === 'idle'">
              <div
                class="ai-upload-area"
                :class="{ dragover: aiUploadAreaHover }"
                @dragover.prevent="aiUploadAreaHover = true"
                @dragleave.prevent="aiUploadAreaHover = false"
                @drop.prevent="handleFileDrop"
                @click="fileInputRef?.click()"
              >
                <AppIcon name="upload" :size="48" />
                <template v-if="aiMode === 'pdf'">
                  <p class="ai-upload-hint">点击或拖拽上传 PDF 文件</p>
                  <p class="ai-upload-sub">将优先尝试 PDF 直连解析，失败后可选择拆分图片逐页识别</p>
                </template>
                <template v-else>
                  <p class="ai-upload-hint">点击或拖拽上传图片文件</p>
                  <p class="ai-upload-sub">支持 JPEG / PNG / WebP</p>
                </template>
                <input
                  ref="fileInputRef"
                  type="file"
                  :accept="aiMode === 'pdf' ? 'application/pdf' : 'image/*'"
                  style="display:none"
                  @change="handleFileSelect"
                />
              </div>
              <div class="ai-actions">
                <AppButton variant="ghost" @click="show = false">取消</AppButton>
              </div>
            </template>

            <!-- ② 上传 + 分类中 -->
            <template v-else-if="docFlowState === 'uploading'">
              <div v-if="aiBatchProgress.total > 0" class="ai-batch-progress">
                <div class="ai-progress-bar">
                  <div class="ai-progress-fill" :style="{ width: (aiBatchProgress.current / aiBatchProgress.total * 100) + '%' }"></div>
                </div>
                <span>{{ aiBatchProgress.text }}</span>
              </div>
              <div class="ai-actions">
                <AppButton variant="ghost" @click="resetDocFlow">取消</AppButton>
              </div>
            </template>

            <!-- ③ PDF 直连失败 → 用户选择是否拆页 OCR 回退 -->
            <template v-else-if="docFlowState === 'pdf_fallback'">
              <div class="pdf-fallback-card">
                <div class="pdf-fallback-icon"><AppIcon name="alert" :size="28" /></div>
                <h4 class="pdf-fallback-title">PDF 直连解析失败</h4>
                <p v-if="pdfFallbackReason" class="pdf-fallback-reason">{{ pdfFallbackReason }}</p>
                <p class="pdf-fallback-hint">是否继续将 PDF 拆分为图片，逐页 OCR 识别？</p>
                <div class="ai-actions">
                  <AppButton variant="ghost" @click="resetDocFlow">取消</AppButton>
                  <AppButton variant="primary" :loading="pdfFallbackSubmitting" @click="fallbackToPageOcr">
                    <AppIcon name="sparkles" :size="16" /> 继续拆分图片识别
                  </AppButton>
                </div>
              </div>
            </template>

            <!-- ④ 资料类型确认 -->
            <template v-else-if="docFlowState === 'confirm' && currentDoc">
              <DocumentTypeConfirmStep
                :doc="currentDoc"
                :loading="docConfirming"
                @confirm="onConfirmDoc"
                @back="resetDocFlow"
                @reclassify="reclassifyDoc"
              />
            </template>

            <!-- ⑤ 解析进度 -->
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

            <!-- ⑥ Mixed 题目分组 -->
            <template v-else-if="docFlowState === 'grouping'">
              <QuestionGroupingStep
                :questions="groupingQuestions"
                :collections="groupingCollections"
                @complete="onGroupingComplete"
              />
            </template>
          </div>

          <!-- Markdown 模式：文本输入区 -->
          <template v-if="aiMode === 'markdown'">
            <p class="ai-hint">粘贴 AI 按推荐格式输出的 Markdown，系统将自动解析并填入表单。</p>
            <textarea
              v-model="aiText"
              class="ai-textarea"
              rows="10"
              placeholder="在此粘贴 AI 输出的 Markdown..."
            ></textarea>
            <div v-if="aiError" class="ai-error">{{ aiError }}</div>
            <div class="ai-actions">
              <AppButton variant="ghost" @click="show = false">取消</AppButton>
              <AppButton variant="primary" :loading="aiParsing" @click="doAiParse">
                <AppIcon name="sparkles" :size="16" /> {{ aiParsing ? '解析中…' : '开始识别' }}
              </AppButton>
            </div>
          </template>
        </div>

        <!-- 结果预览 -->
        <div v-else-if="aiResult" class="ai-result-section">
          <div class="ai-result-meta">
            <span class="ai-result-type">{{ ({ choice: '选择题', fill: '填空题', solution: '解答题' } as Record<string, string>)[aiResult.question_type] }}</span>
          </div>
          <div v-if="aiResult.warnings.length" class="ai-warnings">
            <div v-for="(w, i) in aiResult.warnings" :key="i" class="ai-warning-item">⚠ {{ w }}</div>
          </div>
          <div class="ai-result-preview">
            <div class="ai-preview-block">
              <div class="ai-preview-label">题干</div>
              <div class="ai-preview-content">{{ aiResult.stem }}</div>
            </div>
            <div v-if="aiResult.options?.length" class="ai-preview-block">
              <div class="ai-preview-label">选项</div>
              <div v-for="opt in aiResult.options" :key="opt.label" class="ai-preview-option">
                <span class="ai-opt-label">{{ opt.label }}.</span> {{ opt.content }}
              </div>
            </div>
            <div class="ai-preview-block">
              <div class="ai-preview-label">答案</div>
              <div class="ai-preview-content">
                <span v-if="aiResult.correct_answer.kind === 'choice'">{{ aiResult.correct_answer.value.options?.join(', ') }}</span>
                <span v-else-if="aiResult.correct_answer.kind === 'fill'">{{ aiResult.correct_answer.value.blanks?.map(b => b.answer).join('、') }}</span>
                <span v-else>{{ aiResult.correct_answer.value.subs?.map(s => s.content).join('；') }}</span>
              </div>
            </div>
            <div class="ai-preview-block">
              <div class="ai-preview-label">解析（{{ aiResult.analysis.length }} 种解法）</div>
              <div v-for="(a, i) in aiResult.analysis" :key="i" class="ai-preview-analysis">
                <strong>{{ a.title }}</strong>
                <div>{{ a.content }}</div>
              </div>
            </div>
          </div>
          <div class="ai-actions">
            <AppButton variant="ghost" @click="aiResult = null">返回修改</AppButton>
            <AppButton variant="primary" @click="applyAiResult"><AppIcon name="check" :size="16" /> 应用到表单</AppButton>
          </div>
        </div>
      </div>
    </AppModal>

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
.ai-dialog-body {
  display: flex;
  flex-direction: column;
  gap: 16px;
  /* 统一固定高度（≥380px 保底）：Tab 切换时外框不随内容抽搐；
     宽度由 AppModal width="680px" 统一控制，高度取三 Tab 最高的
     Markdown 模式，图片/PDF 模式由 Dropzone flex:1 填满剩余空间 */
  height: 520px;
  min-height: 380px;
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
.ai-progress-bar { flex: 1; height: 8px; background: var(--bg-input); border-radius: 4px; overflow: hidden; }
.ai-progress-fill { height: 100%; background: var(--accent); transition: width 0.3s; border-radius: 4px; }
.ai-batch-progress span { font-size: 13px; color: var(--text-secondary); white-space: nowrap; }
</style>
