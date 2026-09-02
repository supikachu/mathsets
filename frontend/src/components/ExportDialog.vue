<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  exportApi,
  typesetApi,
  type BlankStyle,
  type ExamRequest,
  type ExamSectionRequest,
  type ExportMode,
  type ExportResult,
  type ExportWarning,
  type LayoutSpec,
  type Paper,
  type ProfilePreset,
} from '@/api/client'
import { AppButton, AppIcon, AppModal, AppSelect } from '@/components/ui'
import { useToast } from '@/composables/useToast'

type ExportFormat = 'docx' | 'pdf' | 'markdown'

interface FormatOption {
  value: ExportFormat
  label: string
  hint: string
  icon: string
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
  { value: 'docx', label: 'Word', hint: '公式可编辑', icon: 'file-text' },
  { value: 'pdf', label: 'PDF', hint: '版面可控', icon: 'document' },
  { value: 'markdown', label: 'Markdown', hint: '纯文本 · 可移植', icon: 'download' },
]

const MODES: ModeOption[] = [
  { value: 'student', label: '学生练习', hint: '仅题目，无答案无解析' },
  { value: 'teacher', label: '教师讲义', hint: '内嵌答案解析与考点提示' },
  { value: 'exam', label: '标准考卷', hint: '题目成卷，卷末汇总答案' },
]

/** 与后端 `LayoutSpec::for_profile` 同一口径：模式决定默认预设 */
const MODE_PRESET: Record<ExportMode, string> = {
  student: 'a4_practice',
  teacher: 'a4_lecture',
  exam: 'a3_fold_exam',
}

const PAPERS = [
  { value: 'a4', label: 'A4（210×297）' },
  { value: 'a3_fold', label: 'A3 对折（420×297）' },
  { value: 'a3_tri', label: 'A3 三栏（420×297）' },
]

const COLUMNS = [
  { value: '1', label: '单栏' },
  { value: '2', label: '双栏' },
  { value: '3', label: '三栏' },
]

const BLANK_STYLES = [
  { value: 'lines', label: '横线' },
  { value: 'dots', label: '点阵' },
  { value: 'blank', label: '纯空白' },
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

// ── PDF 版面（T3.8）：预设是整套 spec 的起点，微调只改本地这份副本
const presets = ref<ProfilePreset[]>([])
const presetId = ref('')
const layout = ref<LayoutSpec | null>(null)
/** 手动选过预设后，切模式不再回头覆盖用户调好的版面 */
const presetTouched = ref(false)

const busy = ref(false)
const warnings = ref<ExportWarning[]>([])
const truncated = ref(false)
const detailsOpen = ref(false)

const isTeacher = computed(() => mode.value === 'teacher')
const isPdf = computed(() => format.value === 'pdf')

/// 预设清单只在第一次选到 PDF 时拉；拉不到就带着 `spec: undefined` 发出去，
/// 由后端按 mode 取默认预设 —— 版面下拉空着，导出本身不该被打断。
async function loadPresets() {
  if (presets.value.length) return
  try {
    const { data } = await typesetApi.profiles()
    presets.value = data
    applyPreset(presetId.value || MODE_PRESET[mode.value])
  } catch (e) {
    toast.error((e as Error).message || '版面预设加载失败，将按输出模式默认排版')
  }
}

/// 整体替换（T3.3 口径）：改过的字段整套回传，所以副本要断开与预设的引用
function applyPreset(id: string) {
  const hit = presets.value.find((p) => p.id === id)
  if (!hit) return
  presetId.value = hit.id
  layout.value = JSON.parse(JSON.stringify(hit.spec)) as LayoutSpec
}

function pickFormat(next: ExportFormat) {
  format.value = next
  if (next === 'pdf') {
    applyPreset(MODE_PRESET[mode.value])
    void loadPresets()
  }
}

const presetOptions = computed(() =>
  presets.value.map((p) => ({ value: p.id, label: p.label })),
)

function onPresetChange(value?: string) {
  if (!value) return
  presetTouched.value = true
  applyPreset(value)
}

function onPaperChange(value?: string) {
  if (layout.value && value) layout.value.paper = value as Paper
}

function onColumnsChange(value?: string) {
  if (layout.value && value) layout.value.columns = Number(value)
}

function onBlankChange(value?: string) {
  if (layout.value && value) {
    layout.value.answer_blank.style = value as BlankStyle
  }
}

/** 模式决定答案/解析/卷末汇总默认值，用户仍可在此之后手动微调 */
function pickMode(next: ExportMode) {
  mode.value = next
  if (!presetTouched.value) applyPreset(MODE_PRESET[next])
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
    spec: isPdf.value ? layout.value ?? undefined : undefined,
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
    let res: ExportResult
    let ext: string
    if (format.value === 'docx') {
      res = await exportApi.docx(req)
      ext = 'docx'
    } else if (format.value === 'pdf') {
      res = await exportApi.pdf(req)
      ext = 'pdf'
    } else {
      res = await exportApi.markdown(req, { bundle: bundle.value })
      ext = res.blob.type.includes('zip') ? 'zip' : 'md'
    }
    const fallback = `${req.title}.${ext}`
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
            :class="{ 'is-active': format === opt.value }"
            :title="opt.hint"
            @click="pickFormat(opt.value)"
          >
            <AppIcon :name="opt.icon" :size="15" />
            <span class="ex-seg-label">{{ opt.label }}</span>
            <span class="ex-seg-hint">{{ opt.hint }}</span>
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

      <div v-if="isPdf && layout" class="ex-field">
        <span class="ex-label">
          版面
          <span class="ex-note">先选预设，再微调；微调只作用于本次导出</span>
        </span>
        <div class="ex-layout-grid">
          <div class="ex-select-row">
            <span class="ex-select-caption">预设</span>
            <AppSelect
              :model-value="presetId"
              :options="presetOptions"
              @update:model-value="onPresetChange"
            />
          </div>
          <div class="ex-select-row">
            <span class="ex-select-caption">纸张</span>
            <AppSelect
              :model-value="layout.paper"
              :options="PAPERS"
              @update:model-value="onPaperChange"
            />
          </div>
          <div class="ex-select-row">
            <span class="ex-select-caption">栏数</span>
            <AppSelect
              :model-value="String(layout.columns)"
              :options="COLUMNS"
              @update:model-value="onColumnsChange"
            />
          </div>
          <div class="ex-select-row">
            <span class="ex-select-caption">留白样式</span>
            <AppSelect
              :model-value="layout.answer_blank.style"
              :options="BLANK_STYLES"
              @update:model-value="onBlankChange"
            />
          </div>
        </div>
        <p class="ex-layout-note">
          密封线：{{ layout.binding ? '居中折叠（M4 起排版）' : '不装订' }}
        </p>
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

.ex-layout-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px 14px;
}

.ex-select-row {
  display: flex;
  flex-direction: column;
  gap: 5px;
}

.ex-select-caption {
  font-size: 12px;
  color: var(--text-secondary);
}

.ex-layout-note {
  margin: 0;
  font-size: 11px;
  color: var(--text-muted);
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
