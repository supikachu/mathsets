<script setup lang="ts">
import { ref, watch, nextTick } from 'vue'
import { aiApi, type ParsedQuestion } from '@/api/client'
import { AppButton, AppModal, AppConfirm, AppIcon } from '@/components/ui'
import { useToast } from '@/composables/useToast'
import { parseMarkdownToQuestion, RECOMMENDED_PROMPT } from '@/utils/parseMarkdown'
import { compressImage, blobToFile } from '@/utils/imageCompressor'
import { runWithConcurrency, withBackoffRetry, type PoolResult } from '@/utils/concurrency'
import { pdfToImages, type PdfPageImage } from '@/utils/pdfToImages'
import { clearBatchSnapshot, hasUnfinishedSnapshot, type BatchSnapshot } from '@/utils/batchSnapshot'

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

// AI Mode tab: 'markdown' | 'image'
const aiMode = ref<'markdown' | 'image'>('markdown')

// OCR 引擎本次覆盖（'default' = 跟随系统设置）
const ocrEngineOverride = ref<'default' | 'doc2x' | 'mineru_local' | 'qwen_vl'>('default')

// 把本次覆盖转换为传给后端的 ocr_provider 参数（default 时返回 undefined 走用户偏好）
function ocrProviderParam(): string | undefined {
  return ocrEngineOverride.value === 'default' ? undefined : ocrEngineOverride.value
}
const aiText = ref('')
const aiError = ref('')
const aiParsing = ref(false)
const aiResult = ref<ParsedQuestion | null>(null)
const promptCopied = ref(false)

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
    // 强制规范化 MIME 类型 — blob.type 在某些降级路径下可能为空，
    // 直接传给 FormData 会导致 multipart part 缺少 Content-Type，
    // 后端解析失败。这里根据压缩结果推断 MIME 并构造一致的 File。
    const mimeType = compressed.type || 'image/webp'
    const ext = mimeType === 'image/png' ? 'png'
      : mimeType === 'image/jpeg' ? 'jpg'
      : 'webp'
    const imageFile = new File([compressed], `upload.${ext}`, { type: mimeType })

    aiBatchProgress.value = { current: 0, total: 1, text: '正在上传并识别图片（约 10-30 秒）…' }
    const res = await withBackoffRetry(() => aiApi.parseImage(imageFile, ocrProviderParam()))
    const questions = res.data.data
    await handleBatchResults(questions)
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
      const res = await withBackoffRetry(() => aiApi.parseImage(imageFile, ocrProviderParam()))
      return res.data.data
    },
    (cur, total) => {
      aiBatchProgress.value = { current: cur, total, text: `OCR 识别中… ${cur}/${total} 页完成` }
    },
  )

  // 收集所有成功识别的题目，失败的页面只记录日志
  const allQuestions: ParsedQuestion[] = []
  let failedPages = 0
  for (let i = 0; i < results.length; i++) {
    const r = results[i] as PoolResult<ParsedQuestion[]>
    if (r.status === 'success' && r.data) {
      allQuestions.push(...r.data)
    } else {
      failedPages++
      console.warn(`[doPdfParse] 第 ${pages[i].page} 页解析失败:`, r.error)
    }
  }

  if (failedPages > 0) {
    toast.warning(`${failedPages} 页解析失败已跳过，成功识别 ${allQuestions.length} 题`)
  }

  await handleBatchResults(allQuestions)
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
        <div v-if="!aiResult" class="ai-input-section">
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
            <!-- OCR 引擎本次覆盖（M3 新增） -->
            <div class="ocr-engine-selector">
              <label class="ocr-engine-label">
                <AppIcon name="sparkles" :size="14" />
                <span>识别引擎</span>
              </label>
              <select v-model="ocrEngineOverride" class="ocr-engine-select">
                <option value="default">默认（跟随系统设置）</option>
                <option value="doc2x">Doc2X（高精度公式）</option>
                <option value="mineru_local">MinerU（私有部署）</option>
                <option value="qwen_vl">Qwen-VL（通用兜底）</option>
              </select>
            </div>
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

/* ===== OCR 引擎本次覆盖选择器（M3 新增） ===== */
.ocr-engine-selector {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 12px;
  background: var(--bg-input, #f9fafb);
  border: 1px solid var(--border, #e5e7eb);
  border-radius: 8px;
}
.ocr-engine-label {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 13px;
  font-weight: 500;
  color: var(--text-secondary, #6b7280);
  white-space: nowrap;
}
.ocr-engine-select {
  flex: 1;
  border: none;
  background: transparent;
  font-size: 13px;
  color: var(--text-primary, #111827);
  cursor: pointer;
  outline: none;
}
.ocr-engine-select:focus {
  color: var(--purple, #7c3aed);
}
</style>
