<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  exportApi,
  type ExamRequest,
  type ExamSectionRequest,
  type ExportMode,
  type ExportWarning,
} from '@/api/client'
import { AppButton, AppIcon, AppModal } from '@/components/ui'
import { useToast } from '@/composables/useToast'

type ExportFormat = 'docx' | 'pdf' | 'markdown'

interface FormatOption {
  value: ExportFormat
  label: string
  hint: string
  icon: string
  enabled: boolean
  milestone?: string
}

interface ModeOption {
  value: ExportMode
  label: string
  hint: string
}

const props = withDefaults(
  defineProps<{
    modelValue: boolean
    /** 页面所见的大题分组（title 含中文序号前缀），由 Basket 序列化 */
    sections: ExamSectionRequest[]
    questionCount: number
    defaultTitle?: string
    /** 导出范围说明，如「一、单选题」 */
    scopeLabel?: string
  }>(),
  { defaultTitle: '', scopeLabel: '' },
)

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  /** print 兜底：由宿主视图执行 window.print() */
  print: []
}>()

const toast = useToast()

const FORMATS: FormatOption[] = [
  { value: 'docx', label: 'Word', hint: '公式可编辑', icon: 'file-text', enabled: false, milestone: 'M2 交付' },
  { value: 'pdf', label: 'PDF', hint: '标准排版', icon: 'document', enabled: false, milestone: 'M3 交付' },
  { value: 'markdown', label: 'Markdown', hint: '纯文本 · 可移植', icon: 'download', enabled: true },
]

const MODES: ModeOption[] = [
  { value: 'student', label: '学生练习', hint: '仅题目，无答案无解析' },
  { value: 'teacher', label: '教师讲义', hint: '内嵌答案解析与考点提示' },
  { value: 'exam', label: '标准考卷', hint: '题目成卷，卷末汇总答案' },
]

const format = ref<ExportFormat>('markdown')
const mode = ref<ExportMode>('teacher')
const title = ref(props.defaultTitle || '未命名试卷')
const includeAnswer = ref(true)
const includeAnalysis = ref(true)
const answerAtEnd = ref(false)
const calloutKnowledge = ref(true)
const calloutErrorProne = ref(true)
const calloutAnalysis = ref(false)
const bundle = ref(false)

const busy = ref(false)
const warnings = ref<ExportWarning[]>([])
const truncated = ref(false)
const detailsOpen = ref(false)

const isTeacher = computed(() => mode.value === 'teacher')

/** 模式决定答案/解析/卷末汇总默认值，用户仍可在此之后手动微调 */
function pickMode(next: ExportMode) {
  mode.value = next
  if (next === 'student') {
    includeAnswer.value = false
    includeAnalysis.value = false
    answerAtEnd.value = false
  } else if (next === 'teacher') {
    includeAnswer.value = true
    includeAnalysis.value = true
    answerAtEnd.value = false
  } else {
    includeAnswer.value = true
    includeAnalysis.value = true
    answerAtEnd.value = true
  }
}

function buildRequest(): ExamRequest {
  return {
    title: title.value.trim() || props.defaultTitle || '未命名试卷',
    exam_meta: { instructions: [] },
    mode: mode.value,
    sections: props.sections,
    options: {
      include_answer: includeAnswer.value,
      include_analysis: includeAnalysis.value,
      answer_at_end: answerAtEnd.value,
      callouts: {
        knowledge: isTeacher.value && calloutKnowledge.value,
        error_prone: isTeacher.value && calloutErrorProne.value,
        analysis: isTeacher.value && calloutAnalysis.value,
      },
    },
  }
}

function saveBlob(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  document.body.appendChild(a)
  a.click()
  a.remove()
  // 交给浏览器发起下载后再释放，立即 revoke 会中断部分实现
  setTimeout(() => URL.revokeObjectURL(url), 1000)
}

async function runExport() {
  if (!props.questionCount || busy.value) return
  busy.value = true
  warnings.value = []
  truncated.value = false
  detailsOpen.value = false
  try {
    const req = buildRequest()
    const res = await exportApi.markdown(req, { bundle: bundle.value })
    const fallback = `${req.title}.${res.blob.type.includes('zip') ? 'zip' : 'md'}`
    saveBlob(res.blob, res.filename || fallback)
    warnings.value = res.warnings
    truncated.value = res.truncated
    if (res.warnings.length) {
      toast.info(`已导出，但有 ${res.warnings.length} 条降级警告`)
    } else {
      toast.success('试卷已导出')
    }
  } catch (e) {
    toast.error((e as Error).message || '导出失败')
  } finally {
    busy.value = false
  }
}

function close() {
  emit('update:modelValue', false)
}

function usePrintFallback() {
  emit('print')
  close()
}

function warningText(w: ExportWarning): string {
  const where = w.question_no ? `第 ${w.question_no} 题` : '卷级'
  return `${where} · ${w.reason}`
}
</script>

<template>
  <AppModal :model-value="modelValue" title="导出试卷" size="md" @update:model-value="close">
    <div class="ex-body">
      <p v-if="scopeLabel" class="ex-scope">
        导出范围：<strong>{{ scopeLabel }}</strong>（{{ questionCount }} 题）
      </p>

      <div class="ex-field">
        <label class="ex-label" for="ex-title">试卷标题</label>
        <input id="ex-title" v-model="title" class="ex-input" type="text" maxlength="80" />
      </div>

      <div class="ex-field">
        <span class="ex-label">输出格式</span>
        <div class="ex-seg" role="radiogroup" aria-label="输出格式">
          <button
            v-for="opt in FORMATS"
            :key="opt.value"
            type="button"
            class="ex-seg-btn"
            :class="{ 'is-active': format === opt.value, 'is-disabled': !opt.enabled }"
            :disabled="!opt.enabled"
            :title="opt.enabled ? opt.hint : `${opt.label} ${opt.milestone}`"
            @click="opt.enabled && (format = opt.value)"
          >
            <AppIcon :name="opt.icon" :size="15" />
            <span class="ex-seg-label">{{ opt.label }}</span>
            <span class="ex-seg-hint">{{ opt.enabled ? opt.hint : opt.milestone }}</span>
          </button>
        </div>
      </div>

      <div class="ex-field">
        <span class="ex-label">输出模式</span>
        <div class="ex-mode-grid">
          <button
            v-for="opt in MODES"
            :key="opt.value"
            type="button"
            class="ex-mode-card"
            :class="{ 'is-active': mode === opt.value }"
            @click="pickMode(opt.value)"
          >
            <span class="ex-mode-label">{{ opt.label }}</span>
            <span class="ex-mode-hint">{{ opt.hint }}</span>
          </button>
        </div>
      </div>

      <div class="ex-field">
        <span class="ex-label">内容开关</span>
        <div class="ex-switch-list">
          <label class="ex-switch-row">
            <input v-model="includeAnswer" type="checkbox" />
            <span>包含答案</span>
          </label>
          <label class="ex-switch-row">
            <input v-model="includeAnalysis" type="checkbox" />
            <span>包含解析</span>
          </label>
          <label class="ex-switch-row" :class="{ 'is-muted': !includeAnswer }">
            <input v-model="answerAtEnd" type="checkbox" :disabled="!includeAnswer" />
            <span>答案汇总到卷末</span>
          </label>
        </div>
      </div>

      <div class="ex-field" :class="{ 'is-muted': !isTeacher }">
        <span class="ex-label">
          教师提示框
          <span v-if="!isTeacher" class="ex-note">仅「教师讲义」模式生效</span>
        </span>
        <div class="ex-switch-list">
          <label class="ex-switch-row">
            <input v-model="calloutKnowledge" type="checkbox" :disabled="!isTeacher" />
            <span>考点</span>
          </label>
          <label class="ex-switch-row">
            <input v-model="calloutErrorProne" type="checkbox" :disabled="!isTeacher" />
            <span>易错点</span>
          </label>
          <label class="ex-switch-row">
            <input v-model="calloutAnalysis" type="checkbox" :disabled="!isTeacher" />
            <span>思路点拨</span>
          </label>
        </div>
      </div>

      <div v-if="format === 'markdown'" class="ex-field">
        <label class="ex-switch-row">
          <input v-model="bundle" type="checkbox" />
          <span>图片打包为 ZIP（附 images/ 目录，离线可用）</span>
        </label>
      </div>

      <div v-if="warnings.length" class="ex-warnings">
        <button type="button" class="ex-warnings-head" @click="detailsOpen = !detailsOpen">
          <AppIcon name="alert" :size="14" />
          <span>{{ warnings.length }} 条内容降级警告</span>
          <AppIcon :name="detailsOpen ? 'chevron-up' : 'chevron-down'" :size="13" />
        </button>
        <ul v-if="detailsOpen" class="ex-warnings-list">
          <li v-for="(w, i) in warnings" :key="i">{{ warningText(w) }}</li>
        </ul>
        <p v-if="truncated" class="ex-warnings-more">
          警告过多已截断，完整清单请改用预览接口。
        </p>
      </div>

      <div class="ex-actions">
        <button type="button" class="ex-print-link" @click="usePrintFallback">
          改用浏览器打印
        </button>
        <AppButton
          :loading="busy"
          :disabled="!questionCount"
          @click="runExport"
        >
          {{ busy ? '生成中…' : '开始导出' }}
        </AppButton>
      </div>
    </div>
  </AppModal>
</template>

<style scoped>
.ex-body {
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.ex-scope {
  margin: 0;
  font-size: 13px;
  color: var(--text-secondary);
}

.ex-field {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.ex-field.is-muted {
  opacity: 0.62;
}

.ex-label {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0.02em;
  color: var(--text-secondary);
}

.ex-note {
  font-size: 11px;
  font-weight: 500;
  color: var(--text-muted);
}

.ex-input {
  width: 100%;
  padding: 9px 12px;
  font: inherit;
  font-size: 14px;
  color: var(--text-primary);
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  outline: none;
  transition: border-color 0.15s ease, box-shadow 0.15s ease;
}

.ex-input:focus {
  border-color: var(--accent);
  box-shadow: var(--shadow-focus);
}

.ex-seg {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 8px;
}

.ex-seg-btn {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 3px;
  padding: 10px 12px;
  font: inherit;
  text-align: left;
  color: var(--text-primary);
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: border-color 0.15s ease, background 0.15s ease, transform 0.15s ease;
}

.ex-seg-btn:hover:not(:disabled) {
  transform: translateY(-1px);
}

.ex-seg-btn.is-active {
  color: var(--accent);
  background: var(--accent-light);
  border-color: var(--accent);
}

.ex-seg-btn.is-disabled {
  cursor: not-allowed;
  opacity: 0.55;
}

.ex-seg-label {
  font-size: 13px;
  font-weight: 600;
}

.ex-seg-hint {
  font-size: 11px;
  color: var(--text-muted);
}

.ex-mode-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 8px;
}

.ex-mode-card {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 12px;
  font: inherit;
  text-align: left;
  color: var(--text-primary);
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: border-color 0.15s ease, box-shadow 0.15s ease, transform 0.15s ease;
}

.ex-mode-card:hover {
  transform: translateY(-1px);
}

.ex-mode-card.is-active {
  border-color: var(--accent);
  box-shadow: var(--shadow-focus);
}

.ex-mode-label {
  font-size: 13px;
  font-weight: 600;
}

.ex-mode-hint {
  font-size: 11px;
  line-height: 1.45;
  color: var(--text-muted);
}

.ex-switch-list {
  display: flex;
  flex-wrap: wrap;
  gap: 10px 20px;
}

.ex-switch-row {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 13px;
  cursor: pointer;
}

.ex-switch-row.is-muted {
  opacity: 0.6;
}

.ex-switch-row input {
  width: 15px;
  height: 15px;
  accent-color: var(--accent);
}

.ex-warnings {
  padding: 10px 12px;
  font-size: 12px;
  color: var(--text-secondary);
  background: var(--warning-light);
  border: 1px solid var(--warning);
  border-radius: var(--radius-sm);
}

.ex-warnings-head {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  padding: 0;
  font: inherit;
  font-weight: 600;
  color: inherit;
  background: none;
  border: none;
  cursor: pointer;
}

.ex-warnings-list {
  margin: 8px 0 0;
  padding-left: 18px;
}

.ex-warnings-list li {
  margin-top: 3px;
}

.ex-warnings-more {
  margin: 8px 0 0;
  color: var(--text-secondary);
}

.ex-actions {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 14px;
  margin-top: 2px;
}

.ex-print-link {
  padding: 0;
  font: inherit;
  font-size: 12px;
  color: var(--text-secondary);
  background: none;
  border: none;
  border-bottom: 1px dashed currentColor;
  cursor: pointer;
}
</style>
