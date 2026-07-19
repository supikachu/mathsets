<script setup lang="ts">
import { ref, reactive, computed, watch, nextTick } from 'vue'
import { aiApi, tagsApi, type ParsedQuestion, type Tag } from '@/api/client'
import { AppButton, AppBadge, AppModal, AppConfirm, AppIcon } from '@/components/ui'
import { useToast } from '@/composables/useToast'
import { useSelectedKp } from '@/composables/useSelectedKp'
import { parseMarkdownToQuestion, RECOMMENDED_PROMPT } from '@/utils/parseMarkdown'
import { compressImage, blobToFile } from '@/utils/imageCompressor'
import { runWithConcurrency, withBackoffRetry, type PoolResult } from '@/utils/concurrency'
import { pdfToImages, type PdfPageImage } from '@/utils/pdfToImages'
import { saveBatchSnapshot, clearBatchSnapshot, hasUnfinishedSnapshot, type BatchSnapshot } from '@/utils/batchSnapshot'

interface BatchItem {
  question: ParsedQuestion | null
  page: number
  status: 'success' | 'error'
  error?: string
}

const show = defineModel<boolean>({ required: true })
const applyingAiResult = defineModel<boolean>('applyingAiResult', { default: false })
const attrSelectedKps = defineModel<{ id: string; name: string }[]>('attrSelectedKps', { required: true })
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
    academic_year: string
    grade_semester: string
    exam_region: string
    exam_type: string
    options: { label: string; content: string }[]
    correctAnswer: any
    blanks: { position: number; answer: string }[]
    sub_answers: string[]
    solutions: string[]
    tagIds: string[]
    knowledgePointIds: string[]
    hasUnsaved: boolean
  }
}>()

const emit = defineEmits<{
  (e: 'applied'): void
}>()

const toast = useToast()
const { select: selectKp } = useSelectedKp()

// AI Mode tab: 'markdown' | 'image'
const aiMode = ref<'markdown' | 'image'>('markdown')
const aiText = ref('')
const aiError = ref('')
const aiParsing = ref(false)
const aiResult = ref<ParsedQuestion | null>(null)
const promptCopied = ref(false)

// Batch processing state
const aiBatchResults = ref<BatchItem[]>([])
const aiBatchIndex = ref(0)
const aiBatchProgress = ref({ current: 0, total: 0, text: '' })
const aiImageFile = ref<File | null>(null)
const aiUploadAreaHover = ref(false)
const fileInputRef = ref<HTMLInputElement | null>(null)
const batchProcessedSet = ref<Set<number>>(new Set())

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
      selectKp(highConfidenceMatch.matched_id!, highConfidenceMatch.matched_name!)
      attrSelectedKps.value = [{ id: highConfidenceMatch.matched_id!, name: highConfidenceMatch.matched_name! }]
      props.form.knowledgePointIds = [highConfidenceMatch.matched_id!]
      aiGeneratedFields.value.add('knowledge_point')
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

// File Drag/Drop & Select
function handleFileDrop(e: DragEvent) {
  aiUploadAreaHover.value = false
  const file = e.dataTransfer?.files?.[0]
  if (file) startImageParse(file)
}

function handleFileSelect(e: Event) {
  const file = (e.target as HTMLInputElement).files?.[0]
  if (file) startImageParse(file)
}

async function startImageParse(file: File) {
  const oldSnapshot = await hasUnfinishedSnapshot()
  if (oldSnapshot) {
    pendingUploadAction = () => doStartImageParse(file)
    snapshotOverwriteConfirm.value = true
    return
  }
  doStartImageParse(file)
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

async function doStartImageParse(file: File) {
  aiImageFile.value = file
  const isPdf = file.type === 'application/pdf'
  try {
    if (isPdf) {
      await doPdfParse(file)
    } else {
      await doImageParse(file)
    }
  } catch (e: any) {
    toast.error(e?.message || '文件解析失败')
  } finally {
    aiParsing.value = false
    aiBatchProgress.value = { current: 0, total: 0, text: '' }
  }
}

async function doImageParse(file: File) {
  aiParsing.value = true
  aiBatchProgress.value = { current: 0, total: 1, text: '正在压缩图片…' }
  try {
    const compressed = await compressImage(file)
    aiBatchProgress.value = { current: 0, total: 1, text: '正在上传并识别图片（约 10-30 秒）…' }
    const imageFile = blobToFile(compressed)
    const res = await withBackoffRetry(() => aiApi.parseImage(imageFile))
    const questions = res.data.data
    await handleBatchResults(questions.map((q) => ({
      question: q, page: 1, status: 'success' as const
    })), 'image')
  } catch (e: any) {
    console.error('[doImageParse] 失败:', e?.message || e)
    toast.error(e?.response?.data?.error || e?.message || '图片识别失败')
    await clearBatchSnapshot()
  }
}

async function doPdfParse(file: File) {
  aiParsing.value = true
  aiBatchProgress.value = { current: 0, total: 0, text: '正在渲染 PDF…' }

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

  aiBatchProgress.value = { current: 0, total: pages.length, text: `开始 OCR 识别（${pages.length} 页）…` }

  const results = await runWithConcurrency(
    pages,
    async (pageImg) => {
      const blob = await (await fetch(pageImg.dataUrl)).blob()
      const compressed = await compressImage(blob)
      const imageFile = blobToFile(compressed, `page-${pageImg.page}.webp`)
      const res = await withBackoffRetry(() => aiApi.parseImage(imageFile))
      return res.data.data
    },
    (cur, total) => {
      aiBatchProgress.value = { current: cur, total, text: `OCR 识别中… ${cur}/${total} 页完成` }
    },
  )

  const batchItems: BatchItem[] = []
  for (let i = 0; i < results.length; i++) {
    const r = results[i] as PoolResult<ParsedQuestion[]>
    if (r.status === 'success' && r.data) {
      for (const q of r.data) {
        batchItems.push({ question: q, page: pages[i].page, status: 'success' })
      }
    } else {
      batchItems.push({ question: null, page: pages[i].page, status: 'error', error: r.error })
    }
  }

  await handleBatchResults(batchItems, 'pdf', pages.length)
}

async function handleBatchResults(items: BatchItem[], source: 'image' | 'pdf', totalPages?: number) {
  aiBatchResults.value = items
  aiBatchIndex.value = 0
  aiParsing.value = false
  aiBatchProgress.value = { current: 0, total: 0, text: '' }
  aiResult.value = null

  const successQuestions = items.filter(i => i.status === 'success').map(i => i.question!)
  if (successQuestions.length > 0) {
    await saveBatchSnapshot({
      questions: successQuestions,
      currentIndex: 0,
      processedIds: [],
      createdAt: Date.now(),
      source,
      totalPages,
    })
  }
}

function isBatchProcessed(idx: number): boolean {
  return batchProcessedSet.value.has(idx)
}

function applyBatchQuestion() {
  const item = aiBatchResults.value[aiBatchIndex.value]
  if (!item || item.status !== 'success' || !item.question) return

  doApplyAiResult(item.question)

  show.value = true
  batchProcessedSet.value.add(aiBatchIndex.value)
  updateBatchSnapshot()
  moveToNextBatch()
}

function skipBatchQuestion() {
  batchProcessedSet.value.add(aiBatchIndex.value)
  updateBatchSnapshot()
  moveToNextBatch()
}

function moveToNextBatch() {
  for (let i = aiBatchIndex.value + 1; i < aiBatchResults.value.length; i++) {
    if (!batchProcessedSet.value.has(i)) {
      aiBatchIndex.value = i
      return
    }
  }
  const allProcessed = aiBatchResults.value.every((_, i) => batchProcessedSet.value.has(i))
  if (allProcessed) {
    toast.success('所有题目已处理完毕')
    closeBatchReview()
  } else {
    for (let i = 0; i < aiBatchResults.value.length; i++) {
      if (!batchProcessedSet.value.has(i)) {
        aiBatchIndex.value = i
        return
      }
    }
  }
}

async function mergeWithPrevious(idx: number) {
  if (idx <= 0) return
  const current = aiBatchResults.value[idx]
  const previous = aiBatchResults.value[idx - 1]
  if (current.status !== 'success' || previous.status !== 'success') {
    toast.warning('无法合并：前后题目必须都是成功解析状态')
    return
  }
  if (!current.question || !previous.question) return

  previous.question.stem += '\n' + current.question.stem
  if (current.question.options?.length) {
    previous.question.options = [
      ...(previous.question.options || []),
      ...current.question.options,
    ]
  }
  if (current.question.analysis?.length) {
    previous.question.analysis = [
      ...(previous.question.analysis || []),
      ...current.question.analysis,
    ]
  }
  if (current.question.correct_answer.kind === 'solution' && previous.question.correct_answer.kind === 'solution') {
    const prevSubs = previous.question.correct_answer.value.subs || []
    const curSubs = current.question.correct_answer.value.subs || []
    previous.question.correct_answer.value.subs = [...prevSubs, ...curSubs]
  }
  previous.question.warnings.push(`已合并第 ${idx + 1} 题（跨页拼接）`)
  previous.question.knowledge_points = [
    ...new Set([...previous.question.knowledge_points, ...current.question.knowledge_points]),
  ]

  aiBatchResults.value.splice(idx, 1)
  aiBatchIndex.value = idx - 1

  await updateBatchSnapshot()
  toast.success('已合并到上一题')
}

async function retryFailedPage(idx: number) {
  const item = aiBatchResults.value[idx]
  if (!item || item.status !== 'error') return

  aiParsing.value = true
  try {
    if (!aiImageFile.value) {
      toast.error('无法重新解析：原始文件已丢失')
      return
    }

    const isPdf = aiImageFile.value.type === 'application/pdf'
    let imageFile: File

    if (isPdf) {
      const pages: PdfPageImage[] = []
      for await (const p of pdfToImages(aiImageFile.value)) {
        pages.push(p)
      }
      const targetPage = pages.find(p => p.page === item.page)
      if (!targetPage) {
        toast.error('无法找到原始页面')
        return
      }
      const blob = await (await fetch(targetPage.dataUrl)).blob()
      const compressed = await compressImage(blob)
      imageFile = blobToFile(compressed, `page-${item.page}.webp`)
    } else {
      const compressed = await compressImage(aiImageFile.value)
      imageFile = blobToFile(compressed)
    }

    const res = await withBackoffRetry(() => aiApi.parseImage(imageFile))
    const questions = res.data.data

    const newItems: BatchItem[] = questions.map(q => ({
      question: q, page: item.page, status: 'success' as const
    }))
    aiBatchResults.value.splice(idx, 1, ...newItems)
    aiBatchIndex.value = idx

    await updateBatchSnapshot()
    toast.success(`第 ${item.page} 页重新解析成功，识别出 ${questions.length} 道题`)
  } catch (e: any) {
    toast.error(e?.response?.data?.error || e?.message || '重新解析失败')
  } finally {
    aiParsing.value = false
  }
}

async function updateBatchSnapshot() {
  const successQuestions = aiBatchResults.value
    .filter(i => i.status === 'success' && !batchProcessedSet.value.has(aiBatchResults.value.indexOf(i)))
    .map(i => i.question!)
  if (successQuestions.length > 0) {
    await saveBatchSnapshot({
      questions: successQuestions,
      currentIndex: aiBatchIndex.value,
      processedIds: [...batchProcessedSet.value],
      createdAt: Date.now(),
      source: aiImageFile.value?.type === 'application/pdf' ? 'pdf' : 'image',
    })
  } else {
    await clearBatchSnapshot()
  }
}

async function closeBatchReview() {
  const unprocessed = aiBatchResults.value.filter((_, i) => !batchProcessedSet.value.has(i)).length
  if (unprocessed > 0) {
    toast.info(`还有 ${unprocessed} 题未处理，下次可从快照恢复`)
  } else {
    await clearBatchSnapshot()
  }
  aiBatchResults.value = []
  aiBatchIndex.value = 0
  batchProcessedSet.value = new Set()
  show.value = false
}

async function restoreFromSnapshot() {
  snapshotRestoreConfirm.value = false
  if (!pendingSnapshotRestore) return

  aiBatchResults.value = pendingSnapshotRestore.questions.map((q, i) => ({
    question: q,
    page: i + 1,
    status: 'success' as const,
  }))
  aiBatchIndex.value = pendingSnapshotRestore.currentIndex
  batchProcessedSet.value = new Set(pendingSnapshotRestore.processedIds)
  show.value = true
  pendingSnapshotRestore = null
  toast.success(`已恢复 ${aiBatchResults.value.length} 道题的批量录入`)
}

defineExpose({
  triggerSnapshotRestore: (snapshot: BatchSnapshot) => {
    pendingSnapshotRestore = snapshot
    snapshotRestoreConfirm.value = true
  },
  triggerFileParse: (file: File) => {
    show.value = true
    startImageParse(file)
  }
})
</script>

<template>
  <div>
    <!-- AI 智能识别弹窗 -->
    <AppModal v-model="show" title="AI 智能识别" size="lg">
      <div class="ai-dialog-body">
        <!-- 输入区 -->
        <div v-if="!aiResult && aiBatchResults.length === 0" class="ai-input-section">
          <!-- 模式切换 Tab -->
          <div class="ai-mode-tabs">
            <button :class="{ active: aiMode === 'markdown' }" @click="aiMode = 'markdown'">Markdown 粘贴</button>
            <button :class="{ active: aiMode === 'image' }" @click="aiMode = 'image'">图片/PDF 识别</button>
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

          <!-- 图片/PDF 上传区 -->
          <div v-if="aiMode === 'image'" class="ai-upload-section">
            <div
              class="ai-upload-area"
              :class="{ dragover: aiUploadAreaHover }"
              @dragover.prevent="aiUploadAreaHover = true"
              @dragleave.prevent="aiUploadAreaHover = false"
              @drop.prevent="handleFileDrop"
              @click="fileInputRef?.click()"
            >
              <AppIcon name="upload" :size="48" />
              <p class="ai-upload-hint">点击或拖拽上传图片/PDF 文件</p>
              <p class="ai-upload-sub">支持 JPEG / PNG / WebP / PDF（最多 30 页）</p>
              <input
                ref="fileInputRef"
                type="file"
                accept="image/*,application/pdf"
                style="display:none"
                @change="handleFileSelect"
              />
            </div>
            <div v-if="aiBatchProgress.total > 0" class="ai-batch-progress">
              <div class="ai-progress-bar">
                <div class="ai-progress-fill" :style="{ width: (aiBatchProgress.current / aiBatchProgress.total * 100) + '%' }"></div>
              </div>
              <span>{{ aiBatchProgress.text }}</span>
            </div>
            <div class="ai-actions">
              <AppButton variant="ghost" @click="show = false">取消</AppButton>
            </div>
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
            <AppButton variant="success" @click="applyAiResult"><AppIcon name="check" :size="16" /> 应用到表单</AppButton>
          </div>
        </div>

        <!-- 批量审阅面板 -->
        <div v-else class="ai-batch-section">
          <div class="ai-batch-header">
            <span class="ai-batch-title">批量审阅（{{ aiBatchIndex + 1 }} / {{ aiBatchResults.length }}）</span>
            <div class="ai-batch-stats">
              <AppBadge color="green">{{ aiBatchResults.filter(r => r.status === 'success').length }} 题成功</AppBadge>
              <AppBadge v-if="aiBatchResults.filter(r => r.status === 'error').length" color="red">{{ aiBatchResults.filter(r => r.status === 'error').length }} 页失败</AppBadge>
            </div>
          </div>
          <div class="ai-batch-body">
            <!-- 左侧题目列表 -->
            <div class="ai-batch-list">
              <div
                v-for="(item, idx) in aiBatchResults"
                :key="idx"
                class="ai-batch-card"
                :class="{
                  active: idx === aiBatchIndex,
                  error: item.status === 'error',
                  processed: isBatchProcessed(idx)
                }"
                @click="aiBatchIndex = idx"
              >
                <template v-if="item.status === 'success'">
                  <span class="ai-batch-num">第{{ idx + 1 }}题</span>
                  <span class="ai-batch-type">{{ ({ choice: '选择', fill: '填空', solution: '解答' } as Record<string, string>)[item.question!.question_type] }}</span>
                  <span class="ai-batch-stem">{{ item.question!.stem.slice(0, 40) }}{{ item.question!.stem.length > 40 ? '…' : '' }}</span>
                </template>
                <template v-else>
                  <span class="ai-batch-num">第{{ item.page }}页</span>
                  <span class="ai-batch-error-icon">⚠ 解析失败</span>
                  <span class="ai-batch-error-msg">{{ item.error }}</span>
                </template>
                <span v-if="isBatchProcessed(idx)" class="ai-batch-check">✓</span>
              </div>
            </div>
            <!-- 右侧当前题预览 -->
            <div class="ai-batch-preview">
              <template v-if="aiBatchResults[aiBatchIndex]?.status === 'success'">
                <div class="ai-preview-block">
                  <div class="ai-preview-label">题干</div>
                  <div class="ai-preview-content">{{ aiBatchResults[aiBatchIndex].question!.stem }}</div>
                </div>
                <div v-if="aiBatchResults[aiBatchIndex].question!.options?.length" class="ai-preview-block">
                  <div class="ai-preview-label">选项</div>
                  <div v-for="opt in aiBatchResults[aiBatchIndex].question!.options" :key="opt.label" class="ai-preview-option">
                    <span class="ai-opt-label">{{ opt.label }}.</span> {{ opt.content }}
                  </div>
                </div>
                <div class="ai-preview-block">
                  <div class="ai-preview-label">答案</div>
                  <div class="ai-preview-content">
                    <span v-if="aiBatchResults[aiBatchIndex].question!.correct_answer.kind === 'choice'">{{ aiBatchResults[aiBatchIndex].question!.correct_answer.value.options?.join(', ') }}</span>
                    <span v-else-if="aiBatchResults[aiBatchIndex].question!.correct_answer.kind === 'fill'">{{ aiBatchResults[aiBatchIndex].question!.correct_answer.value.blanks?.map(b => b.answer).join('、') }}</span>
                    <span v-else>{{ aiBatchResults[aiBatchIndex].question!.correct_answer.value.subs?.map(s => s.content).join('；') }}</span>
                  </div>
                </div>
                <div v-if="aiBatchResults[aiBatchIndex].question!.warnings.length" class="ai-warnings">
                  <div v-for="(w, i) in aiBatchResults[aiBatchIndex].question!.warnings" :key="i" class="ai-warning-item">⚠ {{ w }}</div>
                </div>
                <div class="ai-batch-actions">
                  <AppButton variant="ghost" size="sm" @click="mergeWithPrevious(aiBatchIndex)" v-if="aiBatchIndex > 0">
                    <AppIcon name="arrow-up" :size="14" /> 向上合并
                  </AppButton>
                  <AppButton variant="ghost" size="sm" @click="skipBatchQuestion">跳过此题</AppButton>
                  <AppButton variant="success" size="sm" @click="applyBatchQuestion">
                    <AppIcon name="check" :size="14" /> 应用此题
                  </AppButton>
                </div>
              </template>
              <template v-else>
                <div class="ai-batch-error-detail">
                  <p class="ai-batch-error-title">⚠ 第{{ aiBatchResults[aiBatchIndex]?.page }}页解析失败</p>
                  <p class="ai-batch-error-desc">{{ aiBatchResults[aiBatchIndex]?.error }}</p>
                  <AppButton variant="primary" size="sm" :loading="aiParsing" @click="retryFailedPage(aiBatchIndex)">
                    <AppIcon name="refresh" :size="14" /> 重新解析此页
                  </AppButton>
                </div>
              </template>
            </div>
          </div>
          <div class="ai-actions">
            <AppButton variant="ghost" @click="closeBatchReview">完成/关闭</AppButton>
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
}

.ai-mode-tabs {
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
  min-height: 200px;
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
  display: flex;
  justify-content: flex-end;
  gap: 8px;
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
.ai-upload-section { display: flex; flex-direction: column; gap: 16px; }
.ai-upload-area {
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

/* 批量进度 */
.ai-batch-progress { display: flex; align-items: center; gap: 12px; }
.ai-progress-bar { flex: 1; height: 8px; background: var(--bg-input); border-radius: 4px; overflow: hidden; }
.ai-progress-fill { height: 100%; background: var(--accent); transition: width 0.3s; border-radius: 4px; }
.ai-batch-progress span { font-size: 13px; color: var(--text-secondary); white-space: nowrap; }

/* 批量审阅面板 */
.ai-batch-section { display: flex; flex-direction: column; gap: 16px; }
.ai-batch-header { display: flex; justify-content: space-between; align-items: center; }
.ai-batch-title { font-size: 16px; font-weight: 600; }
.ai-batch-stats { display: flex; gap: 8px; }
.ai-batch-body { display: flex; gap: 16px; min-height: 400px; }
.ai-batch-list { width: 280px; overflow-y: auto; display: flex; flex-direction: column; gap: 8px; flex-shrink: 0; }
.ai-batch-card {
  padding: 10px 12px;
  border: 1px solid var(--border-color, #eee);
  border-radius: 8px;
  cursor: pointer;
  transition: all 0.15s;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  font-size: 13px;
}
.ai-batch-card:hover { border-color: var(--accent); }
.ai-batch-card.active { border-color: var(--accent); background: var(--bg-hover, rgba(99, 102, 241, 0.05)); }
.ai-batch-card.error { border-color: var(--danger); background: var(--danger-light); }
.ai-batch-card.processed { opacity: 0.5; }
.ai-batch-num { font-weight: 600; color: var(--text-secondary); }
.ai-batch-type { font-size: 11px; padding: 2px 6px; border-radius: 4px; background: var(--bg-input); color: var(--text-secondary); }
.ai-batch-stem { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-primary); }
.ai-batch-error-icon { color: var(--danger); font-weight: 600; }
.ai-batch-error-msg { color: var(--danger); font-size: 12px; width: 100%; }
.ai-batch-check { color: var(--success); font-weight: bold; }
.ai-batch-preview { flex: 1; overflow-y: auto; padding: 4px; }
.ai-batch-actions { display: flex; gap: 8px; margin-top: 16px; flex-wrap: wrap; }
.ai-batch-error-detail { text-align: center; padding: 48px 24px; }
.ai-batch-error-title { font-size: 18px; font-weight: 600; color: #ef4444; margin-bottom: 8px; }
.ai-batch-error-desc { color: var(--text-secondary, #888); margin-bottom: 16px; }
</style>
