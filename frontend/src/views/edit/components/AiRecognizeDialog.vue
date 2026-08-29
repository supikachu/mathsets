<script setup lang="ts">
import { ref, watch, nextTick, computed, onBeforeUnmount } from 'vue'
import {
  documentApi,
  collectionApi,
  questionApi,
  aiTaskApi,
  isTimeoutError,
  type ParsedQuestion,
  type DocumentMeta,
  type ConfirmDocumentRequest,
  type QuestionDetail,
  type QuestionCollectionSummary,
  type AiStagedQuestion,
  type AiParseTaskDetail,
  type TagMatch,
  type TaggingMatch,
  type TaggingUnmatched,
} from '@/api/client'
import { AppButton, AppModal, AppConfirm, AppIcon, AppBadge } from '@/components/ui'
import LatexRender from '@/components/LatexRender.vue'
import QuestionOptions from '@/components/QuestionOptions.vue'
import QuestionStructureView from '@/components/QuestionStructureView.vue'
import { typeLabel, typeBadgeColor, diffLabel, diffBadgeColor } from '@/utils/questionDisplay'
import { useToast } from '@/composables/useToast'
import { useAiParsePolling } from '@/composables/useAiParsePolling'
import { normalizeChoiceAnswerBlank } from '@/utils/parseMarkdown'
import { defaultStructure, partsFromParsed, type QuestionPart } from '@/utils/questionParts'
import {
  comparePaperQuestionOrder,
  resolvedQuestionNo,
  sortByPaperQuestionNo,
} from '@/utils/paperQuestionOrder'
import { STAGE2_EXTERNAL_PROMPT } from '@/prompts/stage2External'
import { compressImage, blobToFile } from '@/utils/imageCompressor'
import { withBackoffRetry, isAbortError } from '@/utils/concurrency'
import { pdfToImages, type PdfPageImage } from '@/utils/pdfToImages'
import { clearBatchSnapshot, hasUnfinishedSnapshot, type BatchSnapshot } from '@/utils/batchSnapshot'
import { loadAiSourceFile, saveAiSourceFile } from '@/utils/aiSourceFile'
import { displaySourceLabel, type QuestionSourceState } from '@/utils/questionSource'
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
    parts?: QuestionPart[]
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
  (e: 'tagging-ready', questions: ParsedQuestion[]): void
  (e: 'open-question', index: number): void
  (e: 'save-all'): void
  (e: 'source-updated', state: QuestionSourceState): void
  (e: 'document-updated', doc: DocumentMeta | null): void
}>()

const toast = useToast()
/** 当前识别会话的来源状态（保存题目时写入 metadata） */
const sourceState = ref<QuestionSourceState | null>(null)

// AI Mode tab: 'markdown' | 'image' | 'pdf'（图片与 PDF 各走独立通道）
const aiMode = ref<'markdown' | 'image' | 'pdf'>('markdown')
const aiText = ref('')
const aiError = ref('')
const aiParsing = ref(false)
const aiResult = ref<ParsedQuestion | null>(null)
const promptCopied = ref(false)
/** 录入模式：full=全自动 OCR+Stage2；ocr_export=仅 OCR，站外 JSON 导入 */
const ingestMode = ref<'full' | 'ocr_export'>('full')
const ocrMarkdown = ref('')
const jsonImportText = ref('')
const importingJson = ref(false)
const startingTagging = ref(false)
const stoppingTagging = ref(false)
const ocrCopied = ref(false)
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
/** 解析失败后停留在预览空态，避免只闪一下 toast 后看起来像「没结果」 */
const parseFailMessage = ref('')
const currentDoc = ref<DocumentMeta | null>(null)
/** 同一解析任务只首次灌入预览，后续轮询只合并标签 */
const presentedParseTaskId = ref('')
const docConfirming = ref(false)
const cancelling = ref(false)
let lastConfirmKey = ''
/** 取消解析时中止并行 classify / 上传重试，避免限速后后台继续打模型 */
let backgroundAiAbort: AbortController | null = null

function abortBackgroundAi() {
  backgroundAiAbort?.abort()
  backgroundAiAbort = null
}

function beginBackgroundAi(): AbortSignal {
  abortBackgroundAi()
  backgroundAiAbort = new AbortController()
  return backgroundAiAbort.signal
}

// 解析任务轮询
const {
  isPolling,
  statusText: taskStatusText,
  error: taskError,
  task: pollTask,
  taskId: pollTaskId,
  startPolling,
  cancel: cancelTask,
  reset: resetPolling,
  stopPolling,
  resumePolling,
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
let pendingOverwriteSnapshot: BatchSnapshot | null = null
let pendingSnapshotRestore: BatchSnapshot | null = null

// Copied functions
async function copyPrompt() {
  try {
    await navigator.clipboard.writeText(STAGE2_EXTERNAL_PROMPT)
    toast.success('已复制结构化 Prompt，请连同 OCR 文本发给外部模型')
    promptCopied.value = true
    setTimeout(() => { promptCopied.value = false }, 3000)
  } catch {
    toast.error('复制失败，请手动选择提示词文本复制')
  }
}

async function copyOcrMarkdown() {
  if (!ocrMarkdown.value) {
    toast.warning('暂无 OCR 文本')
    return
  }
  try {
    await navigator.clipboard.writeText(ocrMarkdown.value)
    ocrCopied.value = true
    toast.success('OCR 全文已复制')
    setTimeout(() => { ocrCopied.value = false }, 2000)
  } catch {
    toast.error('复制失败')
  }
}

function openLeftOcrTab() {
  if (!ocrMarkdown.value) {
    toast.warning('OCR 尚未完成')
    return
  }
  leftPaneTab.value = 'ocr'
}

async function importExternalJson() {
  const raw = jsonImportText.value.trim()
  if (!raw) {
    toast.warning('请粘贴外部模型输出的 JSON')
    return
  }
  const id = pollTaskId.value || pollTask.value?.id
  if (!id) {
    toast.error('没有可导入的识别任务，请先上传文件完成 OCR')
    return
  }
  importingJson.value = true
  try {
    // 等外部模型较久后再导入时，空闲 keep-alive 可能已失效；先探活再 POST。
    try {
      await aiTaskApi.getParseTask(id, { timeout: 4000 })
    } catch {
      /* 半开连接在此失败，后续导入会走新连接 */
    }
    let imported = 0
    let message = ''
    try {
      const { data } = await aiTaskApi.importParseQuestions(id, { raw, replace: true })
      imported = data.imported
      message = data.message
    } catch (e: unknown) {
      if (!isTimeoutError(e)) throw e
      const recovered = await recoverImportedStaged(id)
      if (!recovered) {
        const { data } = await aiTaskApi.importParseQuestions(id, { raw, replace: true })
        imported = data.imported
        message = data.message
      } else {
        imported = recovered.staged.length
        message = `已导入 ${imported} 道题，可点击「智能打标」开始打标`
      }
    }
    jsonImportText.value = ''
    const { data: task } = await aiTaskApi.getParseTask(id)
    applyImportedPreview(id, task, message || `已导入 ${imported} 道题`)
  } catch (e: any) {
    toast.error(
      e?.response?.data?.error
      || (isTimeoutError(e) ? '导入超时，请再试一次' : e?.message)
      || '导入失败',
    )
  } finally {
    importingJson.value = false
  }
}

function applyImportedPreview(id: string, task: AiParseTaskDetail, successMessage: string) {
  if (task.ocr_markdown) ocrMarkdown.value = task.ocr_markdown
  const staged = (task.staged_questions ?? []).filter(s => !s.saved && !s.merged_into)
  const questions = dedupePreviewQuestions(sortByPaperQuestionNo(
    staged.map(s => stagedToParsed(s, id)),
    q => q,
  ))
  previewQuestions.value = questions
  aiResult.value = questions[0] ?? null
  parseFailMessage.value = ''
  presentedParseTaskId.value = id
  emit('batch-parsed', questions)
  rightPaneTab.value = 'preview'
  taggingPanelOpen.value = false
  toast.success(successMessage || `已导入 ${questions.length} 道题`)
}

async function recoverImportedStaged(taskId: string): Promise<{ staged: AiStagedQuestion[] } | null> {
  for (let i = 0; i < 6; i++) {
    if (i > 0) await new Promise(r => setTimeout(r, 1500))
    try {
      const { data } = await aiTaskApi.getParseTask(taskId)
      const staged = (data.staged_questions ?? []).filter(s => !s.saved && !s.merged_into)
      if (staged.length > 0) return { staged }
    } catch {
      /* 轮询确认时忽略瞬时失败 */
    }
  }
  return null
}

type PreviewCard = ParsedQuestion & { origIndex: number }

function previewIdentityKey(q: ParsedQuestion, fallback: number): string {
  const no = resolvedQuestionNo(q)
  if (no) return `no:${no}`
  const stem = (q.stem || '').replace(/\s+/g, '').slice(0, 96)
  if (stem) return `stem:${stem}`
  const partStem = (q.parts || [])
    .map((p) => `${p.label || ''}${p.stem || ''}`)
    .join('')
    .replace(/\s+/g, '')
    .slice(0, 96)
  if (partStem) return `parts:${partStem}`
  return `idx:${q.ai_meta?.staged_index ?? fallback}`
}

function dedupePreviewQuestions(items: ParsedQuestion[]): ParsedQuestion[] {
  const seen = new Set<string>()
  const out: ParsedQuestion[] = []
  items.forEach((q, i) => {
    const key = previewIdentityKey(q, i)
    if (seen.has(key)) return
    seen.add(key)
    out.push(q)
  })
  return out
}

function findSnapshotForPreview(q: ParsedQuestion, snapshots: any[], fallbackIndex: number) {
  const staged = q.ai_meta?.staged_index
  if (staged) {
    const hit = snapshots.find((s: any) => s?.aiMeta?.staged_index === staged)
    if (hit) return hit
  }
  const no = resolvedQuestionNo(q)
  if (no) {
    const hit = snapshots.find((s: any) => String(s?.question_no ?? '').trim() === String(no))
    if (hit) return hit
  }
  return snapshots[fallbackIndex]
}

const previewCards = computed((): PreviewCard[] => {
  const snapshots = props.editedSnapshots ?? []
  let items: ParsedQuestion[]
  if (previewQuestions.value.length) {
    items = previewQuestions.value.map((q, i) => overlayParsedFromSnapshot(q, findSnapshotForPreview(q, snapshots, i)))
  } else if (snapshots.length) {
    items = snapshots.map((s) => overlayParsedFromSnapshot(parsedStubFromSnapshot(s), s))
  } else if (aiResult.value) {
    items = [aiResult.value]
  } else {
    items = []
  }
  return dedupePreviewQuestions(items)
    .map((question, origIndex) => ({ ...question, origIndex }))
    .sort((a, b) => comparePaperQuestionOrder(a, b) || a.origIndex - b.origIndex)
})

const previewCount = computed(() => previewCards.value.length)
const unsavedPreviewCount = computed(() =>
  (props.editedSnapshots ?? []).filter((s) => s && !s.saved).length,
)
const canSaveAll = computed(() => (props.editedSnapshots?.length ?? 0) > 0)

type RightPaneTab = 'source' | 'preview' | 'ocr' | 'tagging'
const rightPaneTab = ref<RightPaneTab>('source')
const leftPaneTab = ref<'source' | 'ocr'>('source')

const sourceTabHint = computed(() => {
  const s = sourceState.value
  if (!s) return ''
  return displaySourceLabel(s.source_category, s.source_kind)
})

watch(previewCount, (n, prev) => {
  if (n > 0 && !prev && rightPaneTab.value !== 'ocr') {
    rightPaneTab.value = 'preview'
  }
})

watch(ingestMode, (mode) => {
  if (mode !== 'ocr_export') {
    taggingPanelOpen.value = false
    if (rightPaneTab.value === 'ocr' || rightPaneTab.value === 'tagging') {
      rightPaneTab.value = previewCount.value > 0 ? 'preview' : 'source'
    }
  }
})

watch(currentDoc, (doc) => {
  emit('document-updated', doc)
  const hasPreview =
    previewCount.value > 0
    || previewQuestions.value.length > 0
    || (props.editedSnapshots?.length ?? 0) > 0
  if (doc && !hasPreview) rightPaneTab.value = 'source'
})
function cardSaved(idx: number) {
  return Boolean(props.editedSnapshots?.[idx]?.saved)
}

function previewQuestionLabel(card: PreviewCard, displayIdx: number): string {
  const no = resolvedQuestionNo(card)
  return no ? `第 ${no} 题` : `第 ${displayIdx + 1} 题`
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
const ingestLocked = computed(
  () => docFlowState.value === 'progress' || docFlowState.value === 'uploading',
)
const showJsonTab = computed(() => ingestMode.value === 'ocr_export')
const showTaggingAction = computed(() => ingestMode.value === 'ocr_export' && previewCount.value > 0)
const taggingPanelOpen = ref(false)
const taggingPanelVisible = computed(() =>
  showTaggingAction.value && (taggingPanelOpen.value || taggingStats.value.running),
)

function toggleTaggingPanel() {
  if (taggingStats.value.running) {
    taggingPanelOpen.value = true
    return
  }
  taggingPanelOpen.value = !taggingPanelOpen.value
}

const taggingStats = computed(() => {
  const items = previewQuestions.value
  const total = items.length
  let pending = 0
  let done = 0
  let failed = 0
  let idle = 0
  for (const q of items) {
    const s = q.tagging_status
    if (s === 'pending') pending++
    else if (s === 'done') done++
    else if (s === 'failed') failed++
    else idle++
  }
  return {
    total,
    pending,
    done,
    failed,
    idle,
    running: pending > 0,
    startable: idle + failed > 0,
  }
})

function currentParseTaskId(): string {
  return pollTaskId.value
    || pollTask.value?.id
    || previewQuestions.value[0]?.ai_meta?.task_id
    || ''
}

function parseTaskIdsFromQuestions(questions: Array<{ ai_meta?: { task_id?: string } }> | undefined | null): string[] {
  if (!Array.isArray(questions)) return []
  const ids = new Set<string>()
  for (const q of questions) {
    const id = q?.ai_meta?.task_id
    if (typeof id === 'string' && id.length > 0) ids.add(id)
  }
  return [...ids]
}

/** 丢弃未确认暂存：清本地预览，并请求后端删掉未保存的 staged_questions */
async function discardParseStaged(extraIds: string[] = []) {
  const ids = new Set(extraIds.filter(Boolean))
  const cur = currentParseTaskId()
  if (cur) ids.add(cur)
  previewQuestions.value = []
  aiResult.value = null
  taggingPanelOpen.value = false
  presentedParseTaskId.value = ''
  stopPolling()
  if (!ids.size) return
  await Promise.all([...ids].map((id) =>
    aiTaskApi.clearParseStaged(id).catch((e: any) => {
      console.warn('[AiRecognizeDialog] 清空暂存失败:', e?.message)
    }),
  ))
}

function markPreviewTaggingPending() {
  previewQuestions.value = previewQuestions.value.map((q) => {
    if (q.tagging_status === 'done' || q.tagging_status === 'pending') return q
    return {
      ...q,
      tagging_status: 'pending',
      warnings: [
        ...(q.warnings ?? []).filter(w => w !== '标签识别中，完成后自动回填'),
        '标签识别中，完成后自动回填',
      ],
    }
  })
  emit('tagging-ready', previewQuestions.value)
}

async function startTagging() {
  const id = currentParseTaskId()
  if (!id) {
    toast.error('没有可打标的识别任务')
    return
  }
  if (taggingStats.value.running) return
  startingTagging.value = true
  try {
    const { data } = await aiTaskApi.startParseTagging(id)
    if (!data.started) {
      toast.warning(data.message || '没有待打标的题目')
      return
    }
    presentedParseTaskId.value = id
    markPreviewTaggingPending()
    toast.success(data.message || `已开始打标 ${data.started} 道题`)
    await resumePolling(id)
  } catch (e: any) {
    toast.error(e?.response?.data?.error || e?.message || '开始打标失败')
  } finally {
    startingTagging.value = false
  }
}

async function stopTagging() {
  const id = currentParseTaskId()
  if (!id) return
  stoppingTagging.value = true
  try {
    await aiTaskApi.cancelParseTagging(id)
    stopPolling()
    const { data } = await aiTaskApi.getParseTask(id)
    const staged = (data.staged_questions ?? []).filter(s => !s.saved && !s.merged_into)
    if (staged.length) {
      applyStagedPreview(staged, id, true)
    } else {
      previewQuestions.value = previewQuestions.value.map((q) => (
        q.tagging_status === 'pending' ? { ...q, tagging_status: 'idle' } : q
      ))
      emit('tagging-ready', previewQuestions.value)
    }
  } catch (e: any) {
    toast.error(e?.response?.data?.error || e?.message || '停止打标失败')
  } finally {
    stoppingTagging.value = false
  }
}

function getTaggingProgress() {
  const s = taggingStats.value
  return {
    running: s.running,
    pending: s.pending,
    done: s.done,
    failed: s.failed,
    idle: s.idle,
    total: s.total,
  }
}

/** 打标尚未回写：此时卡片上只有 OCR 推断的名称，保存不会带上标签 */
function cardTaggingPending(q: ParsedQuestion): boolean {
  return q.tagging_status === 'pending'
}

/**
 * 卡片知识点标签。
 *
 * `confirmed` 表示该名称背后有知识树节点 UUID，能随保存落库；
 * OCR 推断的裸名称（`knowledge_points`）没有 UUID，属性面板读不到、保存也不会写入，
 * 必须区分展示，否则会误导用户以为打标已完成。
 */
function cardKnowledgePoints(q: ParsedQuestion): { name: string; confirmed: boolean }[] {
  const confirmed = new Set<string>()
  for (const m of q.tagging_matches ?? []) {
    if (m.target_type !== 'knowledge_node' || !m.target_id) continue
    const name = (m.target_name || m.ai_name || '').trim()
    if (name) confirmed.add(name)
  }
  if (confirmed.size === 0) {
    for (const m of q.kp_matches ?? []) {
      if (!m.matched_id) continue
      const name = (m.matched_name || m.ai_name || '').trim()
      if (name) confirmed.add(name)
    }
  }
  if (confirmed.size > 0) {
    return [...confirmed].map(name => ({ name, confirmed: true }))
  }

  const unconfirmed = new Set<string>()
  for (const name of q.knowledge_points ?? []) {
    const t = (name || '').trim()
    if (t) unconfirmed.add(t)
  }
  for (const m of q.kp_matches ?? []) {
    if (m.matched_id) continue
    const t = (m.ai_name || '').trim()
    if (t) unconfirmed.add(t)
  }
  return [...unconfirmed].map(name => ({ name, confirmed: false }))
}

function cardQuestionType(q: ParsedQuestion): string {
  if (q.question_type === 'choice' && (q.sub_type === 'multi' || q.sub_type === 'multiple')) {
    return 'multiple'
  }
  return q.question_type
}

function parsedStubFromSnapshot(s: any): ParsedQuestion {
  const qType = s?.question_type || 'choice'
  const names = s?.nodeNames && typeof s.nodeNames === 'object'
    ? Object.values(s.nodeNames as Record<string, string>).filter(Boolean)
    : []
  return {
    question_type: qType,
    sub_type: s?.sub_type || '',
    difficulty: s?.difficulty || 'medium',
    stem: s?.stem || '',
    options: Array.isArray(s?.options) ? s.options : [],
    correct_answer: { kind: 'choice', value: { options: [] } },
    analysis: [],
    knowledge_points: names,
    confidence: 0,
    warnings: [],
    image_placeholders: [],
    image_urls: [],
    parts: Array.isArray(s?.parts) ? s.parts : [],
    kp_matches: [],
    tagging_matches: Array.isArray(s?.taggingMatches) ? s.taggingMatches : [],
    tagging_suggestion_id: s?.taggingSuggestionId || null,
    ai_meta: s?.aiMeta,
    question_no: s?.question_no ?? null,
    display_order: typeof s?.display_order === 'number' ? s.display_order : null,
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
  } else if (qType === 'solution' && Array.isArray(s.parts) && s.parts.length) {
    correct_answer = { kind: 'solution', value: { subs: [] } }
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
    stem: normalizeChoiceAnswerBlank(s.stem ?? q.stem, qType, Boolean(options?.length)),
    question_type: qType,
    sub_type: s.sub_type ?? q.sub_type,
    difficulty: s.difficulty ?? q.difficulty,
    options,
    correct_answer,
    analysis,
    parts: qType === 'solution'
      ? (Array.isArray(s.parts) && s.parts.length ? s.parts : q.parts)
      : q.parts,
    tagging_matches: Array.isArray(s.taggingMatches) && s.taggingMatches.length
      ? s.taggingMatches
      : q.tagging_matches,
    knowledge_points: kpFromNodes.length ? kpFromNodes : q.knowledge_points,
    question_no: s.question_no ?? q.question_no,
    display_order: typeof s.display_order === 'number' ? s.display_order : q.display_order,
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
  leftPaneTab.value = 'source'
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
  ocrMarkdown.value = ''
  jsonImportText.value = ''
  ingestMode.value = 'full'
  leftPaneTab.value = 'source'
  if (rightPaneTab.value === 'ocr') rightPaneTab.value = 'source'
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

type AiPaperSessionRestore = {
  document?: DocumentMeta | null
  source?: QuestionSourceState | null
  taskId?: string | null
}

function overlaySourceOnDoc(doc: DocumentMeta, source: QuestionSourceState): DocumentMeta {
  const paperMeta = source.paper_meta
    || doc.metadata?.paper_meta
    || doc.ai_classification?.paper_meta
  const title = source.title || source.paper_meta?.title || doc.title
  return {
    ...doc,
    title,
    metadata: {
      ...(doc.metadata || {}),
      source_category: source.source_category,
      source_kind: source.source_kind,
      create_paper: source.create_paper,
      title: source.title,
      paper_meta: paperMeta,
    },
    ai_classification: {
      document_type: doc.ai_classification?.document_type || doc.document_type || 'exam',
      confidence: doc.ai_classification?.confidence ?? 1,
      level: doc.ai_classification?.level ?? 1,
      checked_pages: doc.ai_classification?.checked_pages ?? 0,
      ...doc.ai_classification,
      source_category: source.source_category,
      source_kind: source.source_kind,
      create_paper: source.create_paper,
      title: source.title || doc.ai_classification?.title,
      paper_meta: paperMeta || doc.ai_classification?.paper_meta,
    },
  }
}

function stubDocFromSource(source: QuestionSourceState, fileName?: string): DocumentMeta {
  const title = source.title || source.paper_meta?.title || null
  return overlaySourceOnDoc({
    id: 'restored-local',
    creator_id: '',
    file_name: fileName || `${title || '未命名资料'}.pdf`,
    file_size: null,
    mime: null,
    page_count: 0,
    document_type: null,
    type_label: null,
    title,
    source_type: source.source_kind,
    sub_source_type: source.sub_source_type || null,
    status: 'classified',
    ai_classification: null,
    metadata: {},
    conversion_engine: null,
    created_at: '',
    updated_at: '',
  }, source)
}

async function fetchDocById(id: string): Promise<DocumentMeta | null> {
  if (!id || id === 'restored-local') return null
  try {
    const res = await documentApi.get(id)
    return res.data.data
  } catch {
    return null
  }
}

async function fetchDocByTaskId(taskId: string): Promise<DocumentMeta | null> {
  try {
    const { data } = await aiTaskApi.getParseTask(taskId)
    if (!data.document_id) return null
    return await fetchDocById(data.document_id)
  } catch {
    return null
  }
}

async function fetchDocByFileName(fileName?: string): Promise<DocumentMeta | null> {
  const name = fileName?.trim()
  if (!name) return null
  try {
    const res = await documentApi.list()
    const docs = res.data.data || []
    const byTime = (a: DocumentMeta, b: DocumentMeta) =>
      String(b.updated_at || '').localeCompare(String(a.updated_at || ''))
    const exact = docs.filter(d => d.file_name === name)
    if (exact.length) return [...exact].sort(byTime)[0]
    const stem = name.replace(/\.[^.]+$/, '')
    const loose = docs.filter(d => (d.file_name || '').replace(/\.[^.]+$/, '') === stem)
    if (loose.length) return [...loose].sort(byTime)[0]
  } catch {
    /* ignore */
  }
  return null
}

function applyStagedPreview(staged: AiStagedQuestion[], taskId: string, emitReady: boolean) {
  const questions = dedupePreviewQuestions(staged.map(s => stagedToParsed(s, taskId)))
  previewQuestions.value = questions
  if (questions[0]) aiResult.value = questions[0]
  if (rightPaneTab.value !== 'ocr') {
    rightPaneTab.value = 'preview'
  }
  if (emitReady) emit('tagging-ready', questions)
}

/** 离开后再恢复：从解析任务重建题目预览，并继续轮询未完成的打标 */
async function restorePreviewFromParseTask(taskId: string) {
  if (!taskId) return
  try {
    const { data } = await aiTaskApi.getParseTask(taskId)
    if (data.ocr_markdown) ocrMarkdown.value = data.ocr_markdown
    if (data.pipeline === 'ocr_export') ingestMode.value = 'ocr_export'
    const staged = (data.staged_questions ?? []).filter(s => !s.saved && !s.merged_into)
    if (!staged.length) {
      if (data.pipeline === 'ocr_export' || data.phase === 'ocr_ready') {
        rightPaneTab.value = 'ocr'
        await resumePolling(taskId)
      }
      return
    }
    presentedParseTaskId.value = data.id
    applyStagedPreview(staged, taskId, true)
    const taggingPending = staged.some(s => s.tagging_status === 'pending')
    if (taggingPending) {
      taggingPanelOpen.value = true
      await resumePolling(taskId)
    } else if (data.pipeline === 'ocr_export') {
      rightPaneTab.value = 'preview'
    }
  } catch (e) {
    console.warn('[AiRecognizeDialog] 恢复识别预览失败', e)
  }
}

/** 恢复「试卷信息」：资料元数据 + 来源级联。有 taskId 时同时重建题目预览。 */
async function restorePaperSession(opts: AiPaperSessionRestore) {
  if (opts.source) sourceState.value = opts.source

  let doc: DocumentMeta | null = null
  const cachedId = opts.document?.id
  if (cachedId) doc = await fetchDocById(cachedId)
  if (!doc && opts.document?.id === 'restored-local') doc = opts.document
  if (!doc && opts.document) doc = opts.document
  if (!doc && opts.taskId) doc = await fetchDocByTaskId(opts.taskId)
  if (!doc) doc = await fetchDocByFileName(ocrFileName.value)

  if (opts.source) {
    doc = doc ? overlaySourceOnDoc(doc, opts.source) : stubDocFromSource(opts.source, ocrFileName.value)
  }
  if (doc) currentDoc.value = doc
  if (opts.taskId) await restorePreviewFromParseTask(opts.taskId)
}

function mergeStagedTagging(staged: AiStagedQuestion[], taskId: string) {
  const pendingCount = staged.filter(s => s.tagging_status === 'pending').length
  if (!previewQuestions.value.length || previewQuestions.value.length !== staged.length) {
    if (staged.length) applyStagedPreview(staged, taskId, true)
    if (pendingCount === 0) resetPolling()
    return
  }
  const byIndex = new Map(staged.map(s => [s.index, s]))
  let changed = false
  previewQuestions.value = previewQuestions.value.map((q) => {
    const idx = q.ai_meta?.staged_index
    if (idx == null || idx === '') return q
    const s = byIndex.get(idx)
    if (!s) return q
    if (s.tagging_status === 'pending') return q
    const sid = s.suggestion_id || s.suggestion?.suggestion_id || null
    if (q.tagging_suggestion_id && q.tagging_suggestion_id === sid && (q.tagging_matches?.length ?? 0) > 0) {
      return q
    }
    const next = stagedToParsed(s, taskId)
    changed = true
    return {
      ...q,
      kp_matches: next.kp_matches,
      tagging_suggestion_id: next.tagging_suggestion_id,
      tagging_unmatched: next.tagging_unmatched,
      tag_matches: next.tag_matches,
      tagging_matches: next.tagging_matches,
      grade_level: next.grade_level,
      cognitive_level: next.cognitive_level,
      tagging_difficulty: next.tagging_difficulty,
      tagging_question_type: next.tagging_question_type,
      tagging_stage: next.tagging_stage,
      // 必须一并更新，否则卡片会一直显示「标签识别中」、保存守卫也永远拦着
      tagging_status: next.tagging_status,
      warnings: (q.warnings ?? []).filter(w => w !== '标签识别中，完成后自动回填'),
    }
  })
  if (aiResult.value?.ai_meta?.staged_index) {
    const cur = previewQuestions.value.find(q => q.ai_meta?.staged_index === aiResult.value?.ai_meta?.staged_index)
    if (cur) aiResult.value = cur
  }
  if (changed) emit('tagging-ready', previewQuestions.value)
  if (pendingCount === 0) {
    resetPolling()
  }
}

function presentParsedQuestions(questions: ParsedQuestion[], message: string, keepPolling = false) {
  const ordered = dedupePreviewQuestions(sortByPaperQuestionNo(questions, q => q))
  previewQuestions.value = ordered
  aiResult.value = ordered[0] ?? null
  parseFailMessage.value = ''
  // 识别完成只结束进度，保留 currentDoc / 试卷信息，避免切回「试卷信息」时表单被卸载清空
  docFlowState.value = 'idle'
  if (!keepPolling) {
    resetPolling()
  }
  groupingQuestions.value = []
  groupingCollections.value = []
  pdfDirectActive.value = false
  pdfFallbackReason.value = ''
  pdfFallbackSubmitting.value = false
  toast.success(message)
  emit('batch-parsed', ordered)
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
  if ('parts' in props.form) props.form.parts = defaultStructure().parts
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

  props.form.stem = normalizeChoiceAnswerBlank(q.stem, q.question_type, Boolean(q.options?.length))
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
    props.form.parts = partsFromParsed(q)
    aiGeneratedFields.value.add('sub_answers')
  }

  if (q.question_type !== 'solution') {
    props.form.solutions = q.analysis.map(a => a.content)
    aiGeneratedFields.value.add('solutions')
  }

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
    pendingOverwriteSnapshot = oldSnapshot
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
  const extraIds = parseTaskIdsFromQuestions(pendingOverwriteSnapshot?.questions)
  pendingOverwriteSnapshot = null
  await discardParseStaged(extraIds)
  await clearBatchSnapshot()
  if (pendingUploadAction) {
    pendingUploadAction()
    pendingUploadAction = null
  }
}

function dismissSnapshotOverwrite() {
  pendingUploadAction = null
  pendingOverwriteSnapshot = null
}

/** 图片通道：压缩 → 上传（file_type=image，无原始 PDF）→ AI 分类 → 类型确认 */
async function doStartImageParse(file: File) {
  docFlowKind.value = 'image'
  aiImageFile.value = file
  try {
    const pages = await compressToPage(file)
    await uploadAndClassify(pages, false, undefined, file.name)
  } catch (e: any) {
    if (isAbortError(e)) return
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
    await uploadAndClassify(pages, true, file, file.name)
  } catch (e: any) {
    if (isAbortError(e)) return
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
async function uploadAndClassify(pages: File[], isPdf: boolean, originalPdf?: File, originalFileName?: string) {
  parseFailMessage.value = ''
  docFlowState.value = 'uploading'
  aiBatchProgress.value = { current: 0, total: pages.length, text: '正在上传资料（最多 30 页）…' }
  const signal = beginBackgroundAi()
  const fileName = originalFileName?.trim()
    || originalPdf?.name
    || aiImageFile.value?.name
    || undefined
  const res = await withBackoffRetry(
    () =>
      documentApi.upload(pages, {
        file_name: fileName,
        file_type: isPdf ? 'pdf' : 'image',
        pdf: originalPdf,
      }, signal),
    3,
    signal,
  )
  if (signal.aborted) return
  const doc = res.data.data
  currentDoc.value = doc

  // OCR 先行：立刻建解析任务
  const parseMode = isPdf || docFlowKind.value === 'pdf' ? 'pdf_direct' : undefined
  pdfDirectActive.value = parseMode === 'pdf_direct'
  await startTask(doc.id, parseMode)
  if (signal.aborted) return

  // 分类并行，仅预填来源条。不套前端 429 退避：后端 send_chat 已重试，
  // 再套一层会在取消解析后继续 POST /classify。
  void (async () => {
    try {
      const cls = await documentApi.classify(doc.id, signal)
      if (signal.aborted || currentDoc.value?.id !== doc.id) return
      currentDoc.value = cls.data.data
    } catch (e: any) {
      if (isAbortError(e) || signal.aborted) return
      console.warn('[AiRecognizeDialog] 分类建议失败（不影响识别）:', e?.message)
    }
  })()
}

function humanizeParseError(raw: string): string {
  const s = raw.toLowerCase()
  if (s.includes('insufficient balance') || s.includes('http 402') || raw.includes('余额不足')) {
    return 'AI 服务余额不足，请充值后再试'
  }
  if (s.includes('http 401') || s.includes('invalid api key') || s.includes('incorrect api key')) {
    return 'AI API Key 无效或已过期，请到设置页检查'
  }
  if (s.includes('http 403')) {
    return 'AI 服务拒绝访问，请检查密钥权限'
  }
  if (raw.includes('免费档不可用')) {
    return '该 Gemini 模型在免费档不可用，请改用 Flash 或开通付费'
  }
  if (raw.includes('RPD') || raw.includes('太平洋时间') || raw.includes('今日请求次数已用尽')) {
    return 'Gemini 免费额度今日请求次数已用尽，将于太平洋时间午夜重置'
  }
  if (s.includes('http 429') || s.includes('rate limit') || raw.includes('速率限制') || raw.includes('请求过于频繁')) {
    return 'AI 服务请求过于频繁（已达速率限制），请稍后再试'
  }
  if (s.includes('provider returned error') || s.includes('stealth') || raw.includes('Ox Alpha')) {
    return 'OpenRouter 上游（Ox Alpha / Stealth）暂时拒绝了请求，请确认模型 ID 为 stealth/ox-alpha 后重试'
  }
  const oneLine = raw.replace(/\s+/g, ' ').trim()
  return oneLine.length > 180 ? `${oneLine.slice(0, 180)}…` : oneLine
}

function resetDocFlow() {
  abortBackgroundAi()
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
  parseFailMessage.value = ''
  presentedParseTaskId.value = ''
  lastConfirmKey = ''
}

/** 后置确认来源（不启动解析；解析已在上传后开始） */
async function onConfirmDoc(body: ConfirmDocumentRequest) {
  const doc = currentDoc.value
  if (!doc) return
  if (docConfirming.value) return
  const key = JSON.stringify(body)
  if (key === lastConfirmKey) return
  lastConfirmKey = key
  docConfirming.value = true
  try {
    const res = await documentApi.confirm(doc.id, body)
    const next = res.data.data
    const paperId = (res.data as any).paper_id as string | undefined
      || next?.metadata?.linked_paper_id
    const prevLinked = currentDoc.value?.metadata?.linked_paper_id
    const prevConfirmed = currentDoc.value?.metadata?.user_confirmed
    // 避免整对象替换触发来源条回种，从而再次 confirm
    if (
      next
      && (paperId !== prevLinked || next.metadata?.user_confirmed !== prevConfirmed)
    ) {
      currentDoc.value = next
    }
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
    lastConfirmKey = ''
    toast.error(e?.response?.data?.error || e?.message || '保存来源失败')
  } finally {
    docConfirming.value = false
  }
}

function onSourceState(state: QuestionSourceState) {
  sourceState.value = state
  emit('source-updated', state)
}

/** 创建解析任务并进入进度态（左侧保留原文） */
async function startTask(documentId: string, parseMode?: 'pdf_direct' | 'page') {
  docFlowState.value = 'progress'
  await startPolling(
    documentId,
    parseMode,
    ingestMode.value === 'ocr_export' ? 'ocr_export' : undefined,
  )
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
  abortBackgroundAi()
  try {
    await cancelTask()
    pdfDirectActive.value = false
    stopPolling()
    if (docFlowState.value === 'progress') {
      toast.warning('解析已取消')
      docFlowState.value = 'confirm'
    }
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
    stem: normalizeChoiceAnswerBlank(p.stem ?? '', p.question_type ?? 'solution', Boolean(p.options?.length)),
    options: p.options,
    correct_answer: (p.correct_answer ?? { kind: 'solution', value: { subs: [] } }) as any,
    analysis: Array.isArray(p.analysis) ? p.analysis : [],
    parts: Array.isArray(p.parts) ? p.parts : [],
    knowledge_points: Array.isArray(p.knowledge_points) ? p.knowledge_points : [],
    confidence: typeof p.confidence === 'number' ? p.confidence : 0,
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
    tagging_status: s.tagging_status ?? null,
    warnings: s.tagging_status === 'pending'
      ? [...(Array.isArray(p.warnings) ? p.warnings : []), '标签识别中，完成后自动回填']
      : (Array.isArray(p.warnings) ? p.warnings : []),
    question_no: p.question_no != null && String(p.question_no).trim() !== ''
      ? String(p.question_no).trim()
      : null,
    display_order: typeof p.display_order === 'number' ? p.display_order : null,
  }
}

/// 监听任务终态：成功 → 工作台（非 Mixed）/ 分组（Mixed）
watch(pollTask, async (t) => {
  if (!t) return
  if (t.ocr_markdown) ocrMarkdown.value = t.ocr_markdown
  if (t.pipeline === 'ocr_export') ingestMode.value = 'ocr_export'
  if (t.status === 'success' || t.status === 'partial_success') {
    const staged = (t.staged_questions ?? []).filter(s => !s.saved && !s.merged_into)
    if (presentedParseTaskId.value === t.id) {
      mergeStagedTagging(staged, t.id)
      return
    }
    pdfDirectActive.value = false
    if (
      staged.length === 0
      && (t.pipeline === 'ocr_export' || t.phase === 'ocr_ready')
    ) {
      stopPolling()
      leftPaneTab.value = 'source'
      rightPaneTab.value = 'ocr'
      toast.success('OCR 已完成，可在左侧切换查看文本，将 JSON 粘贴到右侧导入')
      docFlowState.value = 'idle'
      return
    }
    presentedParseTaskId.value = t.id
    // 暂存链路：题目尚未落库，从 staged_questions 构建待确认列表（跳过已保存/跨页合并项）
    if (staged.length === 0) {
      stopPolling()
      parseFailMessage.value = '未识别到有效题目'
      rightPaneTab.value = 'preview'
      toast.warning(parseFailMessage.value)
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
    const taggingPending = staged.filter(s => s.tagging_status === 'pending').length
    const base =
      t.status === 'partial_success'
        ? `部分成功：识别到 ${t.success_count} 题（${t.failed_count} 题失败），请在右侧预览`
        : `成功识别 ${questions.length} 道题，点击右侧卡片进入编辑`
    const extra = [
      taggingPending > 0 ? `${taggingPending} 题标签识别中` : '',
      unmatchedCount > 0 ? `${unmatchedCount} 个未匹配项，确认保存后提交审核` : '',
      dupCount > 0 ? `${dupCount} 题与题库已有内容重复` : '',
    ].filter(Boolean).join('；')
    presentParsedQuestions(questions, extra ? `${base}；${extra}` : base, taggingPending > 0)
  } else if (t.status === 'failed') {
    const errMsg = t.error_message || taskError.value || '解析失败'
    stopPolling()
    // PDF 直连模式失败（PDF_DIRECT_FAILED 前缀）→ 不回到确认页，
    // 进入 pdf_fallback 让用户选择是否拆页 OCR 重试
    if (pdfDirectActive.value && errMsg.startsWith('PDF_DIRECT_FAILED')) {
      pdfDirectActive.value = false
      pdfFallbackReason.value = errMsg.replace(/^PDF_DIRECT_FAILED:?\s*/, '')
      docFlowState.value = 'pdf_fallback'
    } else {
      parseFailMessage.value = humanizeParseError(errMsg)
      rightPaneTab.value = 'preview'
      toast.error(parseFailMessage.value)
      docFlowState.value = 'confirm'
    }
  } else if (t.status === 'cancelled') {
    pdfDirectActive.value = false
    stopPolling()
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
  const signal = beginBackgroundAi()
  aiBatchProgress.value = { current: 1, total: 1, text: 'AI 正在重新识别资料类型…' }
  try {
    const cls = await documentApi.classify(doc.id, signal)
    if (signal.aborted) return
    currentDoc.value = cls.data.data
  } catch (e: any) {
    if (isAbortError(e) || signal.aborted) return
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

  const taskId = questions[0]?.ai_meta?.task_id
  if (taskId) {
    await restorePaperSession({ taskId })
  }

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
  restorePaperSession,
  restorePreviewFromParseTask,
  getSourceState: () => sourceState.value,
  getCurrentDoc: () => currentDoc.value,
  getPreviewQuestions: () => previewQuestions.value,
  getParseTaskId: () => currentParseTaskId(),
  isTaggingRunning: () => taggingStats.value.running,
  getTaggingProgress,
  startTagging,
  stopTagging,
  discardParseStaged,
})

onBeforeUnmount(() => {
  abortBackgroundAi()
  if (markdownParseTimer) clearTimeout(markdownParseTimer)
  // 离开页面前把内存里的原稿再写一遍 IndexedDB，覆盖「上传时尚未落盘」的旧会话
  const keepDraft = Boolean(
    sessionStorage.getItem('q-batch-draft-new-ai') || sessionStorage.getItem('q-batch-draft-new'),
  )
  if (keepDraft && aiImageFile.value) {
    void saveAiSourceFile(aiImageFile.value, sourceKind.value === 'pdf' ? 'pdf' : 'image')
  }
  if (currentDoc.value) emit('document-updated', currentDoc.value)
  if (sourceState.value) emit('source-updated', sourceState.value)
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
              <div class="ai-split-title-row">
                <div
                  v-if="showOriginalSource"
                  class="ai-pane-tabs ai-src-tabs"
                  role="tablist"
                  aria-label="左侧内容"
                >
                  <button
                    type="button"
                    role="tab"
                    class="ai-pane-tab"
                    :class="{ active: leftPaneTab === 'source' }"
                    :aria-selected="leftPaneTab === 'source'"
                    @click="leftPaneTab = 'source'"
                  >
                    原文
                  </button>
                  <button
                    type="button"
                    role="tab"
                    class="ai-pane-tab"
                    :class="{ active: leftPaneTab === 'ocr' }"
                    :aria-selected="leftPaneTab === 'ocr'"
                    :disabled="!ocrMarkdown"
                    @click="openLeftOcrTab"
                  >
                    OCR
                  </button>
                </div>
                <span v-else class="ai-split-title">智能录入</span>
                <div class="ai-mode-seg" role="radiogroup" aria-label="录入模式">
                  <label
                    class="ai-mode-seg-item"
                    :class="{ active: ingestMode === 'full', disabled: ingestLocked }"
                  >
                    <input type="radio" v-model="ingestMode" value="full" :disabled="ingestLocked">
                    全自动
                  </label>
                  <label
                    class="ai-mode-seg-item"
                    :class="{ active: ingestMode === 'ocr_export', disabled: ingestLocked }"
                  >
                    <input type="radio" v-model="ingestMode" value="ocr_export" :disabled="ingestLocked">
                    站外结构化
                  </label>
                </div>
              </div>
              <div class="ai-tool-row">
                <button type="button" class="ai-tool-btn is-accent" @click="copyPrompt">
                  <AppIcon name="copy" :size="13" />
                  {{ promptCopied ? '已复制' : '复制 Prompt' }}
                </button>
                <button type="button" class="ai-tool-btn is-muted" @click="clearEditor">
                  <AppIcon name="trash" :size="13" />
                  清空
                </button>
              </div>
            </header>
            <div
              class="ai-split-body"
              :class="{ dragover: aiUploadAreaHover }"
              @dragover.prevent="aiUploadAreaHover = true"
              @dragleave.prevent="onEditorDragLeave"
              @drop.prevent="handleFileDrop"
              @paste="onEditorPaste"
            >
              <div
                v-if="!showOriginalSource"
                class="ai-drop-empty"
                @click="fileInputRef?.click()"
              >
                <div class="ai-drop-glyph">
                  <AppIcon name="upload" :size="26" />
                </div>
                <p>将 PDF 或图片拖到此处</p>
                <span>全自动会继续切题；站外结构化只做 OCR。复制 Prompt 后与 OCR 一并发给外部模型，LaTeX 反斜杠须在 JSON 里双写</span>
              </div>
              <div
                v-else-if="leftPaneTab === 'ocr'"
                class="ai-ocr-view"
              >
                <header class="ai-ocr-view-head">
                  <span>OCR 文本</span>
                  <button type="button" class="ai-tool-btn" :disabled="!ocrMarkdown" @click="copyOcrMarkdown">
                    <AppIcon name="copy" :size="13" />
                    {{ ocrCopied ? '已复制' : '复制全文' }}
                  </button>
                </header>
                <pre class="ai-ocr-pre">{{ ocrMarkdown || '识别完成后将在此显示 OCR 文本。' }}</pre>
              </div>
              <div v-else class="ai-source-preview">
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
              <span v-else class="ai-split-hint">支持 PDF、图片（拖放或点击上传）</span>
              <div v-if="aiError" class="ai-error">{{ aiError }}</div>
            </footer>
          </section>

          <!-- 右侧：试卷信息 / 题目预览 可切换 -->
          <section class="ai-split-pane">
            <header class="ai-split-head">
              <div
                class="ai-pane-tabs"
                :class="{ 'is-three': showJsonTab }"
                role="tablist"
                aria-label="右侧内容"
              >
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
                <button
                  v-if="showJsonTab"
                  type="button"
                  role="tab"
                  class="ai-pane-tab"
                  :class="{ active: rightPaneTab === 'ocr' }"
                  :aria-selected="rightPaneTab === 'ocr'"
                  @click="rightPaneTab = 'ocr'"
                >
                  外部 JSON
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
                <div
                  v-if="(currentDoc && sourceTabHint) || showTaggingAction"
                  class="ai-preview-toolbar"
                >
                  <button
                    v-if="currentDoc && sourceTabHint"
                    type="button"
                    class="ai-source-summary"
                    @click="rightPaneTab = 'source'"
                  >
                    <span>{{ sourceTabHint }}</span>
                  </button>
                  <div class="ai-preview-toolbar-actions">
                    <button
                      v-if="showTaggingAction"
                      type="button"
                      class="ai-preview-action"
                      :class="{ 'is-active': taggingPanelVisible }"
                      @click="toggleTaggingPanel"
                    >
                      {{ taggingStats.running
                        ? `打标中 ${taggingStats.done + taggingStats.failed}/${taggingStats.total}`
                        : '智能打标' }}
                      <span v-if="taggingStats.pending > 0" class="ai-tab-count">{{ taggingStats.pending }}</span>
                    </button>
                    <button
                      v-if="currentDoc && sourceTabHint"
                      type="button"
                      class="ai-preview-action"
                      @click="rightPaneTab = 'source'"
                    >
                      编辑
                    </button>
                  </div>
                </div>
                <div v-if="taggingPanelVisible" class="ai-tagging-inline">
                  <section class="ai-inset-card">
                    <span class="ai-inset-label">智能打标</span>
                    <template v-if="taggingStats.running">
                      <p class="ai-inset-hint">
                        正在打标 {{ taggingStats.done + taggingStats.failed }} / {{ taggingStats.total }}
                        （进行中 {{ taggingStats.pending }} 题）
                      </p>
                      <div class="ai-tagging-bar">
                        <div
                          class="ai-tagging-bar-fill"
                          :style="{ width: Math.round(((taggingStats.done + taggingStats.failed) / Math.max(taggingStats.total, 1)) * 100) + '%' }"
                        />
                      </div>
                      <p class="ai-inset-hint">离开录入界面将终止未完成的打标。</p>
                    </template>
                    <template v-else-if="taggingStats.total > 0 && taggingStats.done === taggingStats.total">
                      <p class="ai-inset-hint">{{ taggingStats.done }} 道题已打标完成，可在下方卡片核对后全部保存。</p>
                    </template>
                    <template v-else>
                      <p class="ai-inset-hint">
                        已导入 {{ taggingStats.total }} 道题。打标会填写知识点、章节与通法，需消耗 AI 额度；只有点击开始后才会执行。
                      </p>
                      <p v-if="taggingStats.failed > 0" class="ai-inset-hint">
                        其中 {{ taggingStats.failed }} 题上次打标失败，可重新开始。
                      </p>
                      <p v-if="taggingStats.done > 0" class="ai-inset-hint">
                        已完成 {{ taggingStats.done }} 题，开始后只会处理尚未打标的题目。
                      </p>
                    </template>
                  </section>
                  <AppButton
                    v-if="taggingStats.running"
                    class="ai-import-cta"
                    variant="outline"
                    block
                    :loading="stoppingTagging"
                    :disabled="stoppingTagging"
                    @click="stopTagging"
                  >
                    停止打标
                  </AppButton>
                  <AppButton
                    v-else
                    class="ai-import-cta"
                    variant="primary"
                    block
                    :loading="startingTagging"
                    :disabled="!taggingStats.startable || startingTagging"
                    @click="startTagging"
                  >
                    {{ taggingStats.failed > 0 && taggingStats.idle === 0 ? '重新打标失败项' : '开始打标' }}
                  </AppButton>
                </div>
                <div
                  v-if="previewCount === 0"
                  class="ai-preview-empty"
                  :class="{ 'is-error': Boolean(parseFailMessage) }"
                >
                  <span class="ai-empty-kicker">{{ parseFailMessage ? '识别失败' : '题目预览' }}</span>
                  <p>{{
                    parseFailMessage
                      || (docFlowState === 'progress' ? '识别中，完成后将在此展示' : '识别完成后将在此展示题目卡片')
                  }}</p>
                </div>
                <div v-else class="ai-q-card-list">
                  <p class="ai-preview-hint">点卡片进入该题编辑；校对完成后可一次性全部保存</p>
                  <p
                    v-if="showTaggingAction && taggingStats.startable && !taggingStats.running"
                    class="ai-preview-hint"
                  >
                    题目已导入，尚未打标。请点击「智能打标」开始，系统不会自动打标。
                  </p>
                  <article
                    v-for="(card, idx) in previewCards"
                    :key="card.ai_meta?.staged_index || card.origIndex"
                    class="ai-q-card"
                    :class="{ 'is-saved': cardSaved(card.origIndex) }"
                    role="button"
                    tabindex="0"
                    @click="openPreviewCard(card.origIndex)"
                    @keydown.enter.prevent="openPreviewCard(card.origIndex)"
                  >
                    <span class="ai-q-index">{{ previewQuestionLabel(card, idx) }}</span>
                    <span v-if="cardSaved(card.origIndex)" class="ai-q-saved">已保存</span>
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
                      <div v-if="card.stem" class="ai-q-stem">
                        <LatexRender :text="card.stem" />
                      </div>
                      <QuestionOptions
                        v-if="(card.question_type === 'choice' || card.question_type === 'multiple') && card.options?.length"
                        :options="card.options"
                      />
                      <QuestionStructureView
                        v-if="card.question_type === 'solution' && card.parts?.length"
                        class="ai-q-parts"
                        section="stems"
                        :parts="partsFromParsed(card)"
                      />
                    </div>
                    <div
                      v-if="cardTaggingPending(card) || cardKnowledgePoints(card).length"
                      class="ai-q-kps"
                    >
                      <span v-if="cardTaggingPending(card)" class="ai-q-kp ai-q-kp--pending">
                        标签识别中…
                      </span>
                      <span
                        v-for="kp in cardKnowledgePoints(card).slice(0, 4)"
                        :key="kp.name"
                        class="ai-q-kp"
                        :class="{ 'ai-q-kp--unconfirmed': !kp.confirmed }"
                        :title="kp.confirmed
                          ? '已匹配知识树节点，保存时会一并写入'
                          : 'AI 初步识别，尚未匹配到知识树节点，保存不会带上此标签'"
                      >{{ kp.name }}</span>
                    </div>
                  </article>
                </div>
              </div>
              <div v-show="rightPaneTab === 'ocr'" class="ai-ocr-pane">
                <section class="ai-inset-card">
                  <label class="ai-inset-label" for="ai-json-import">粘贴外部模型 JSON</label>
                  <p class="ai-inset-hint">须为可 parse 的 JSON；公式只用 $...$，LaTeX 命令写成 \\frac、\\odot（导入时会再修一层非法转义）</p>
                  <textarea
                    id="ai-json-import"
                    v-model="jsonImportText"
                    class="ai-json-import ai-json-import--fill"
                    placeholder='{"questions":[...]} 或题目数组'
                  />
                </section>
                <AppButton
                  class="ai-import-cta"
                  variant="primary"
                  block
                  :loading="importingJson"
                  :disabled="!jsonImportText.trim() || !pollTaskId"
                  @click="importExternalJson"
                >
                  导入题目
                </AppButton>
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
  gap: 16px;
  font-family: var(--font-cn-isolated, -apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", sans-serif);
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
  border: 1px solid var(--divider, rgba(0, 0, 0, 0.06));
  border-radius: var(--radius-xl, 22px);
  overflow: hidden;
  box-shadow: var(--shadow-sm);
}

.ai-split-head {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 12px 14px;
  background: color-mix(in srgb, var(--bg-card, #fff) 78%, transparent);
  border-bottom: 1px solid var(--divider, rgba(0, 0, 0, 0.06));
  backdrop-filter: var(--blur-nav, saturate(180%) blur(20px));
  -webkit-backdrop-filter: var(--blur-nav, saturate(180%) blur(20px));
  flex-wrap: wrap;
}

.ai-split-title {
  font-size: 15px;
  font-weight: 650;
  letter-spacing: -0.022em;
  color: var(--text-primary);
}

.ai-split-title-row {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
  min-width: 0;
}

.ai-mode-seg {
  display: inline-grid;
  grid-template-columns: 1fr 1fr;
  padding: 2px;
  background: rgba(118, 118, 128, 0.12);
  border-radius: 9px;
}

.ai-mode-seg-item {
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: 26px;
  padding: 0 12px;
  border-radius: 7px;
  font-size: 12px;
  font-weight: 600;
  letter-spacing: -0.01em;
  color: var(--text-secondary, #6e6e73);
  cursor: pointer;
  user-select: none;
  white-space: nowrap;
}

.ai-mode-seg-item.active {
  color: var(--text-primary);
  background: var(--bg-card, #fff);
  box-shadow:
    0 1px 1px rgba(0, 0, 0, 0.04),
    0 1px 3px rgba(0, 0, 0, 0.12);
}

.ai-mode-seg-item.disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.ai-mode-seg-item input {
  position: absolute;
  inset: 0;
  margin: 0;
  opacity: 0;
  cursor: inherit;
}

.ai-tool-row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
  flex-shrink: 0;
}

.ai-tool-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 5px;
  min-height: 28px;
  padding: 0 10px;
  border: 0;
  border-radius: 8px;
  background: rgba(118, 118, 128, 0.12);
  color: var(--text-primary);
  font-size: 12px;
  font-weight: 600;
  letter-spacing: -0.01em;
  cursor: pointer;
}

.ai-tool-btn:hover:not(:disabled) {
  background: rgba(118, 118, 128, 0.18);
}

.ai-tool-btn:active:not(:disabled) {
  transform: scale(0.97);
}

.ai-tool-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.ai-tool-btn.is-accent {
  background: var(--accent-light, rgba(0, 113, 227, 0.1));
  color: var(--accent, #0071e3);
}

.ai-tool-btn.is-accent:hover:not(:disabled) {
  background: color-mix(in srgb, var(--accent, #0071e3) 16%, transparent);
}

.ai-tool-btn.is-muted {
  color: var(--text-secondary, #6e6e73);
}

.ai-drop-empty {
  flex: 1;
  min-height: 220px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 32px 24px;
  text-align: center;
  color: var(--text-secondary, #6e6e73);
  cursor: pointer;
  background:
    radial-gradient(ellipse at 50% 0%, color-mix(in srgb, var(--accent, #0071e3) 6%, transparent), transparent 62%);
}

.ai-drop-glyph {
  display: grid;
  place-items: center;
  width: 56px;
  height: 56px;
  border-radius: 16px;
  background: var(--accent-light, rgba(0, 113, 227, 0.1));
  color: var(--accent, #0071e3);
}

.ai-drop-empty p {
  margin: 0;
  font-size: 15px;
  font-weight: 650;
  letter-spacing: -0.02em;
  color: var(--text-primary);
}

.ai-drop-empty span {
  font-size: 13px;
  line-height: 1.5;
  max-width: 280px;
}

.ai-ocr-pane {
  display: flex;
  flex-direction: column;
  gap: 12px;
  height: 100%;
  min-height: 0;
  padding: 12px;
  background: var(--bg-primary, #f5f5f7);
  overflow: auto;
}

.ai-inset-card {
  display: flex;
  flex-direction: column;
  min-height: 0;
  background: var(--bg-card, #fff);
  border-radius: 12px;
  box-shadow: var(--shadow-xs);
  overflow: hidden;
}

.ai-ocr-pane .ai-inset-card:first-child {
  flex: 1;
}

.ai-inset-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 10px 12px 6px;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary, #6e6e73);
}

.ai-inset-label {
  display: block;
  padding: 12px 14px 8px;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary, #6e6e73);
}

.ai-inset-hint {
  margin: 0;
  padding: 0 14px 8px;
  font-size: 12px;
  line-height: 1.45;
  color: var(--text-muted, #8e8e93);
}

.ai-tagging-bar {
  margin: 4px 14px 10px;
  height: 6px;
  border-radius: 99px;
  background: rgba(118, 118, 128, 0.16);
  overflow: hidden;
}

.ai-tagging-bar-fill {
  height: 100%;
  border-radius: inherit;
  background: var(--accent, #0071e3);
  transition: width 0.25s ease;
}

pre.ai-ocr-pre,
.ai-json-import {
  font-family:
    var(--font-mono, "SF Mono", Menlo, Consolas),
    "PingFang SC",
    "Hiragino Sans GB",
    "Noto Sans SC",
    "Microsoft YaHei",
    monospace;
  font-size: 13px;
  font-variant-ligatures: none;
  line-height: 1.55;
  color: var(--text-primary);
}

.ai-ocr-pre {
  flex: 1;
  min-height: 140px;
  margin: 0;
  padding: 4px 14px 16px;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-word;
  border: 0;
  background: transparent;
}

.ai-json-import {
  min-height: 132px;
  resize: vertical;
  margin: 0;
  padding: 0 14px 14px;
  border: 0;
  background: transparent;
  color: var(--text-primary);
  outline: none;
}

.ai-json-import--fill {
  flex: 1;
  min-height: 220px;
  resize: none;
}

.ai-import-cta {
  flex-shrink: 0;
}

.ai-import-cta :deep(.btn) {
  min-height: 44px;
  border-radius: 12px;
  font-size: 15px;
  font-weight: 600;
  letter-spacing: -0.022em;
}

.ai-pane-tabs.is-three {
  grid-template-columns: 1fr 1fr 1fr;
  max-width: 400px;
}

.ai-pane-tabs.is-four {
  grid-template-columns: repeat(4, minmax(0, 1fr));
  max-width: 560px;
}

.ai-pane-tabs.is-four .ai-pane-tab {
  font-size: 12px;
  padding: 5px 4px;
}

.ai-pane-tabs {
  display: inline-grid;
  grid-template-columns: 1fr 1fr;
  min-width: min(240px, 100%);
  flex: 1 1 200px;
  max-width: 320px;
  padding: 2px;
  background: rgba(118, 118, 128, 0.12);
  border-radius: 9px;
}

.ai-src-tabs {
  flex: 0 0 auto;
  min-width: 148px;
  max-width: 168px;
}

.ai-pane-tab {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  min-height: 28px;
  padding: 0 12px;
  border: 0;
  border-radius: 7px;
  background: transparent;
  color: var(--text-secondary, #6e6e73);
  font-size: 12px;
  font-weight: 600;
  letter-spacing: -0.01em;
  cursor: pointer;
}

.ai-pane-tab.active {
  color: var(--text-primary, #1d1d1f);
  background: var(--bg-card, #fff);
  box-shadow:
    0 1px 1px rgba(0, 0, 0, 0.04),
    0 1px 3px rgba(0, 0, 0, 0.12);
}

.ai-pane-tab:disabled {
  opacity: 0.4;
  cursor: not-allowed;
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

[data-theme='dark'] .ai-mode-seg,
[data-theme='dark'] .ai-pane-tabs,
[data-theme='dark'] .ai-tool-btn {
  background: rgba(118, 118, 128, 0.24);
}

[data-theme='dark'] .ai-mode-seg-item.active,
[data-theme='dark'] .ai-pane-tab.active {
  background: #636366;
  color: #fff;
  box-shadow: none;
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
  background: var(--bg-primary, #f5f5f7);
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
  flex: 1 1 auto;
  min-width: 0;
  margin: 0;
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

.ai-preview-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin: 12px 14px 0;
}

.ai-preview-toolbar-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.ai-preview-action {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: 0;
  border-radius: 10px;
  padding: 8px 12px;
  background: var(--bg-card, #fff);
  color: var(--accent, #0071e3);
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.04);
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  white-space: nowrap;
}

.ai-preview-action.is-active {
  background: color-mix(in srgb, var(--accent, #0071e3) 12%, #fff);
}

.ai-tagging-inline {
  margin: 10px 14px 0;
}

.ai-tagging-inline .ai-import-cta {
  margin-top: 10px;
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
  overflow: hidden;
  background: #e8eaed;
}

.ai-ocr-view {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-primary, #f5f5f7);
}

.ai-ocr-view-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  flex-shrink: 0;
  padding: 10px 12px 4px;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary, #6e6e73);
}

.ai-ocr-view .ai-ocr-pre {
  min-height: 0;
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

.ai-preview-empty.is-error .ai-empty-kicker,
.ai-preview-empty.is-error p {
  color: var(--danger, #c0392b);
}

.ai-preview-empty.is-error p {
  max-width: 280px;
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
  border-radius: 18px;
  border: 0;
  box-shadow: var(--shadow-xs, 0 1px 2px rgba(0, 0, 0, 0.04));
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

.ai-q-parts {
  margin-top: 8px;
}

.ai-q-parts :deep(.part-node) {
  padding: 2px 0;
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

/* 未匹配到知识树节点：虚线描边 + 更淡的字色，与已确认标签区分 */
.ai-q-kp--unconfirmed {
  color: var(--text-tertiary, #86868b);
  background: transparent;
  border: 1px dashed var(--border-color);
  padding: 1px 7px;
}

.ai-q-kp--pending {
  color: var(--accent);
  background: var(--accent-light);
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
