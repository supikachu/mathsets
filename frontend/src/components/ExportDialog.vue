<script setup lang="ts">
/**
 * ExportDialog — 三格式 × 三模式的导出入口（T5.6 完整化，口径见实施计划 §7.2 与 R15）
 *
 * 三处刻意的设计：
 * - **一份请求体**。`buildRequest()` 同时喂导出与预览，所以预览看到的就是点「导出」会拿到的
 *   东西；Basket 里那份重复的预览序列化已删（R14 的收口）。
 * - **预览只对 PDF 开**。`/typeset/preview` 出的是 typst 排的那套页，Word 走 OMML + Word 自己
 *   的分页，Markdown 压根没有版面 —— 选着 Word 给教师看一页 PDF 的版面是撒谎。
 * - **参数只在客户端夹紧**。后端对 `LayoutSpec` 零校验，边距填 500mm 会算出负内容宽 ⇒ typst
 *   报错 ⇒ HTTP 500。教师唯一的输入路径就是这一格，所以在这里钳住而不是等他看懂报错。
 */
import { computed, ref } from 'vue'
import {
  exportApi,
  typesetApi,
  type Binding,
  type BlankStyle,
  type ColorMode,
  type ExamRequest,
  type ExamSectionRequest,
  type ExportMode,
  type ExportResult,
  type ExportWarning,
  type Issue,
  type LayoutSpec,
  type Margins,
  type Paper,
  type ProfilePreset,
} from '@/api/client'
import { AppButton, AppIcon, AppModal, AppSelect } from '@/components/ui'
import PreflightList from '@/components/PreflightList.vue'
import TypesetPreview from '@/components/TypesetPreview.vue'
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

const POSITIONS = [
  { value: 'left', label: '左侧装订带' },
  { value: 'center_fold', label: '中缝对折' },
]

const COLORS = [
  { value: 'rich', label: '彩色（屏幕 / 激光打印）' },
  { value: 'print_black_only', label: '纯黑 K100（付印）' },
]

const AREAS: { key: keyof Binding['areas']; label: string }[] = [
  { key: 'school', label: '学校' },
  { key: 'class', label: '班级' },
  { key: 'name', label: '姓名' },
  { key: 'exam_no', label: '考号' },
]

const MARGIN_FIELDS: { key: keyof Margins; label: string }[] = [
  { key: 'top_mm', label: '上' },
  { key: 'bottom_mm', label: '下' },
  { key: 'left_mm', label: '左' },
  { key: 'right_mm', label: '右' },
  { key: 'gutter_mm', label: '栏间距' },
]

/** 页边距与留白高度的可填区间（R15：越界的值排不进纸，代价是一次看不懂的 500） */
const MARGIN_MM = { min: 5, max: 40 }
const GUTTER_MM = { min: 0, max: 40 }
const BLANK_CM = { min: 2, max: 20, step: 0.5 }

const format = ref<ExportFormat>('pdf')
const mode = ref<ExportMode>('teacher')
const title = ref(props.defaultTitle || '未命名试卷')
const includeAnswer = ref(true)
const includeAnalysis = ref(true)
const answerAtEnd = ref(false)
const calloutKnowledge = ref(true)
const calloutErrorProne = ref(true)
const calloutAnalysis = ref(false)
const bundle = ref(false)

// ── 版面（T3.8；T4.12 起 PDF 与 Word 共用）：预设是整套 spec 的起点，微调只改本地这份副本
const presets = ref<ProfilePreset[]>([])
const presetId = ref('')
const layout = ref<LayoutSpec | null>(null)
/** 手动选过预设后，切模式不再回头覆盖用户调好的版面 */
const presetTouched = ref(false)
/** 关掉密封线再打开时用它兜底 —— 不该把装订位与填涂区洗回默认值 */
let bindingMemo: Binding | null = null

const showPreview = ref(false)

const busy = ref(false)
const warnings = ref<ExportWarning[]>([])
const truncated = ref(false)

const isTeacher = computed(() => mode.value === 'teacher')
const isPdf = computed(() => format.value === 'pdf')
const isDocx = computed(() => format.value === 'docx')
/** 版面区块与 `spec` 实参的作用域：PDF 与 Word 共用同一份纸张口径（T4.12） */
const hasLayout = computed(() => isPdf.value || isDocx.value)
/** 预览挂在 PDF 上；格式换走时 `pickFormat` 会顺手把开关关掉，不留一个「开着却无效」的态 */
const previewOpen = computed(() => showPreview.value && isPdf.value && !!props.questionCount)

/// 预设清单只在第一次选到 PDF / Word 时拉；拉不到就带着 `spec: undefined` 发出去，
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
  bindingMemo = layout.value.binding ? JSON.parse(JSON.stringify(layout.value.binding)) : null
}

function pickFormat(next: ExportFormat) {
  format.value = next
  if (next !== 'pdf') showPreview.value = false
  if (next === 'pdf' || next === 'docx') {
    // 已经有一份版面（可能刚在另一种格式上调过）就沿用，别用模式预设把微调洗掉
    if (!layout.value) applyPreset(MODE_PRESET[mode.value])
    void loadPresets()
  }
}

// 默认格式是 PDF，首次挂载即拉预设，不等教师手动切格式
void loadPresets()

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

function onColorChange(value?: string) {
  if (layout.value && value) layout.value.color = value as ColorMode
}

function onBlankChange(value?: string) {
  if (layout.value && value) {
    layout.value.answer_blank.style = value as BlankStyle
  }
}

function clampNum(raw: string, lo: number, hi: number, fallback: number): number {
  const n = Number.parseFloat(raw)
  if (!Number.isFinite(n)) return fallback
  return Math.min(hi, Math.max(lo, Math.round(n * 10) / 10))
}

function onMarginChange(key: keyof Margins, event: Event) {
  if (!layout.value) return
  const range = key === 'gutter_mm' ? GUTTER_MM : MARGIN_MM
  const current = layout.value.margins[key]
  layout.value.margins[key] = clampNum((event.target as HTMLInputElement).value, range.min, range.max, current)
}

function onBlankHeightChange(event: Event) {
  if (!layout.value) return
  const current = layout.value.answer_blank.height_cm
  layout.value.answer_blank.height_cm = clampNum(
    (event.target as HTMLInputElement).value,
    BLANK_CM.min,
    BLANK_CM.max,
    current,
  )
}

function onBindingToggle(event: Event) {
  if (!layout.value) return
  const on = (event.target as HTMLInputElement).checked
  if (!on) {
    if (layout.value.binding) bindingMemo = JSON.parse(JSON.stringify(layout.value.binding))
    layout.value.binding = null
    return
  }
  layout.value.binding = bindingMemo
    ? (JSON.parse(JSON.stringify(bindingMemo)) as Binding)
    : { position: 'center_fold', areas: { school: true, class: true, name: true, exam_no: true } }
}

function onBindingPosition(value?: string) {
  const b = layout.value?.binding
  if (b && value) b.position = value as Binding['position']
}

function onAreaChange(key: keyof Binding['areas'], event: Event) {
  const b = layout.value?.binding
  if (b) b.areas[key] = (event.target as HTMLInputElement).checked
}

function onHeaderFooter(key: keyof LayoutSpec['header_footer'], event: Event) {
  if (!layout.value) return
  layout.value.header_footer[key] = (event.target as HTMLInputElement).checked
}

const HEADER_FOOTER_FIELDS: { key: keyof LayoutSpec['header_footer']; label: string }[] = [
  { key: 'header_title', label: '页眉当前大题名' },
  { key: 'page_number', label: '页脚页码' },
  { key: 'odd_even_outer', label: '奇偶页码外侧对齐（双面印）' },
]

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
    // spec.profile 由后端按 mode 回填（pdf.rs:130），这里带过去只是整套参数的一个字段
    spec: hasLayout.value ? (layout.value ?? undefined) : undefined,
  }
}

/// 预览吃快照：这一路要过 300ms debounce 再加一次冷编，`spec` 若与 `layout` 共享引用，
/// 教师边等编译边改边距，在途那趟序列化就会把新旧值混成一份没人排过的版面
const previewRequest = computed<ExamRequest | null>(() =>
  previewOpen.value ? (JSON.parse(JSON.stringify(buildRequest())) as ExamRequest) : null,
)

/// 回执只有四字段（无级别无页码），归一成 `Issue` 才能共用 `PreflightList` 的行画法（R15）
const receiptRows = computed<Issue[]>(() =>
  warnings.value.map((w) => ({
    field: w.field,
    severity: 'warning' as const,
    question_no: w.question_no ?? undefined,
    latex: w.latex ?? undefined,
    reason: w.reason,
  })),
)

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
</script>

<template>
  <AppModal
    :model-value="modelValue"
    title="导出试卷"
    size="md"
    :width="previewOpen ? '1240px' : ''"
    @update:model-value="close"
  >
    <div class="ex-body" :class="{ 'ex-body--split': previewOpen }">
      <div class="ex-controls">
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

        <div v-if="hasLayout" class="ex-field">
          <span class="ex-label">
            版面
            <span class="ex-note">先选预设，再微调；微调只作用于本次导出</span>
          </span>

          <template v-if="layout">
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
                <span class="ex-select-caption">
                  色彩<span v-if="!isPdf" class="ex-pdf">仅 PDF</span>
                </span>
                <AppSelect
                  :model-value="layout.color"
                  :options="COLORS"
                  @update:model-value="onColorChange"
                />
              </div>
            </div>

            <div class="ex-sub">
              <span class="ex-sub-caption">页边距（mm）</span>
              <div class="ex-num-grid">
                <label v-for="m in MARGIN_FIELDS" :key="m.key" class="ex-num-row">
                  <span>{{ m.label }}</span>
                  <input
                    class="ex-num"
                    type="number"
                    :min="m.key === 'gutter_mm' ? GUTTER_MM.min : MARGIN_MM.min"
                    :max="m.key === 'gutter_mm' ? GUTTER_MM.max : MARGIN_MM.max"
                    :step="0.5"
                    :value="layout.margins[m.key]"
                    @change="onMarginChange(m.key, $event)"
                  />
                </label>
              </div>
            </div>

            <div class="ex-sub">
              <label class="ex-switch-row">
                <input
                  type="checkbox"
                  :checked="!!layout.binding"
                  @change="onBindingToggle"
                />
                <span>密封线 / 装订带</span>
              </label>
              <div v-if="layout.binding" class="ex-binding">
                <div class="ex-select-row ex-select-row--narrow">
                  <span class="ex-select-caption">装订位</span>
                  <AppSelect
                    :model-value="layout.binding.position"
                    :options="POSITIONS"
                    @update:model-value="onBindingPosition"
                  />
                </div>
                <div class="ex-switch-list">
                  <label v-for="a in AREAS" :key="a.key" class="ex-switch-row">
                    <input
                      type="checkbox"
                      :checked="layout.binding.areas[a.key]"
                      @change="onAreaChange(a.key, $event)"
                    />
                    <span>{{ a.label }}</span>
                  </label>
                </div>
              </div>
            </div>

            <div class="ex-sub">
              <span class="ex-sub-caption">
                页眉页脚<span v-if="!isPdf" class="ex-pdf">仅 PDF</span>
              </span>
              <div class="ex-switch-list">
                <label v-for="f in HEADER_FOOTER_FIELDS" :key="f.key" class="ex-switch-row">
                  <input
                    type="checkbox"
                    :checked="layout.header_footer[f.key]"
                    @change="onHeaderFooter(f.key, $event)"
                  />
                  <span>{{ f.label }}</span>
                </label>
              </div>
            </div>

            <div class="ex-sub">
              <span class="ex-sub-caption">
                答题留白<span v-if="!isPdf" class="ex-pdf">仅 PDF</span>
              </span>
              <div class="ex-blank">
                <div class="ex-select-row ex-select-row--narrow">
                  <span class="ex-select-caption">样式</span>
                  <AppSelect
                    :model-value="layout.answer_blank.style"
                    :options="BLANK_STYLES"
                    @update:model-value="onBlankChange"
                  />
                </div>
                <label class="ex-slider">
                  <span>高度</span>
                  <input
                    type="range"
                    :min="BLANK_CM.min"
                    :max="BLANK_CM.max"
                    :step="BLANK_CM.step"
                    :value="layout.answer_blank.height_cm"
                    @input="onBlankHeightChange"
                  />
                  <strong class="ex-slider-value">{{ layout.answer_blank.height_cm.toFixed(1) }}cm</strong>
                </label>
              </div>
            </div>
          </template>
          <p v-else class="ex-layout-note">
            版面预设清单没拉下来，这一格空着：导出会按输出模式取默认预设，本身不受影响。
          </p>

          <div class="ex-preview-row">
            <label class="ex-switch-row" :class="{ 'is-muted': !isPdf }">
              <input v-model="showPreview" type="checkbox" :disabled="!isPdf || !questionCount" />
              <span>预览版面</span>
            </label>
            <span class="ex-note">
              {{
                isPdf
                  ? '每改一次参数就重排一次（攒 300ms），开着才吃服务器'
                  : '预览走排版引擎，Word 的分页在 Word 自己手里、Markdown 没有版面'
              }}
            </span>
          </div>

          <p class="ex-layout-note">
            {{
              isPdf
                ? '字体族跟随预设（正文思源宋体 / 标题思源黑体）；逐字段改字体要先有字体清单接口，本版不开放。'
                : 'Word 同步纸张 / 边距 / 栏数 / 左装订带；中缝对折、留白、页眉页脚与色彩模式是 PDF 专属。'
            }}
          </p>
        </div>

        <div v-if="format === 'markdown'" class="ex-field">
          <label class="ex-switch-row">
            <input v-model="bundle" type="checkbox" />
            <span>图片打包为 ZIP（附 images/ 目录，离线可用）</span>
          </label>
        </div>

        <!-- 没开预览时，回执就是屏上唯一那份清单；开了预览则由面板底部显示全量预检（R15） -->
        <div v-if="!previewOpen && receiptRows.length" class="ex-warnings">
          <p class="ex-warnings-caption">
            本次导出回执 {{ warnings.length }} 条内容降级{{ truncated ? '（头上限截断，非全量）' : '' }}
          </p>
          <PreflightList :items="receiptRows" title="导出回执" note="回执口径不含级别与页码" />
          <p v-if="truncated" class="ex-warnings-more">
            看全量：格式切到 PDF，勾「预览版面」，面板底部的预检清单带页码。
          </p>
        </div>

        <p v-else-if="previewOpen && warnings.length" class="ex-warnings-caption">
          本次导出回执 {{ warnings.length }} 条 —— 全量清单在右侧面板底部（预览口径带页码）。
        </p>

        <div class="ex-actions">
          <button type="button" class="ex-print-link" @click="usePrintFallback">
            改用浏览器打印
          </button>
          <AppButton :loading="busy" :disabled="!questionCount" @click="runExport">
            {{ busy ? '生成中…' : '开始导出' }}
          </AppButton>
        </div>
      </div>

      <div v-if="previewOpen" class="ex-pane">
        <!-- v-if 才挂载：没勾预览就一次请求都不发；请求体与「开始导出」同源 -->
        <TypesetPreview :request="previewRequest" />
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

/* 预览打开时左右两栏：A3 对折一页 420mm，不给半屏宽度「适应宽度」就是白给 */
.ex-body--split {
  display: grid;
  grid-template-columns: minmax(0, 460px) minmax(0, 1fr);
  gap: 22px;
  align-items: start;
}

.ex-controls {
  display: flex;
  flex-direction: column;
  gap: 18px;
}

/* 窄窗口里 AppModal 的 max-width 会被视口夹住，两栏会把预览挤成一两百 px —— 改回单列 */
@media (max-width: 1080px) {
  .ex-body--split {
    grid-template-columns: minmax(0, 1fr);
  }
}

.ex-pane {
  position: sticky;
  top: 0;
  min-width: 0;
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

.ex-sub {
  display: flex;
  flex-direction: column;
  gap: 7px;
  padding: 9px 10px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
}

.ex-sub-caption {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--text-secondary);
}

.ex-pdf {
  padding: 0 4px;
  font-size: 10px;
  font-weight: 600;
  color: var(--text-muted);
  border: 1px solid var(--border-color);
  border-radius: 3px;
}

.ex-num-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(78px, 1fr));
  gap: 8px;
}

.ex-num-row {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--text-secondary);
}

.ex-num {
  width: 100%;
  min-width: 0;
  padding: 5px 7px;
  font: inherit;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  color: var(--text-primary);
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  outline: none;
}

.ex-num:focus {
  border-color: var(--accent);
}

.ex-binding {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.ex-select-row {
  display: flex;
  flex-direction: column;
  gap: 5px;
}

.ex-select-row--narrow {
  width: 190px;
  max-width: 100%;
}

.ex-select-caption {
  font-size: 12px;
  color: var(--text-secondary);
}

.ex-blank {
  display: flex;
  align-items: flex-end;
  gap: 14px;
  flex-wrap: wrap;
}

.ex-slider {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--text-secondary);
}

.ex-slider input {
  width: 150px;
  accent-color: var(--accent);
}

.ex-slider-value {
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  color: var(--text-primary);
}

.ex-preview-row {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  padding-top: 2px;
}

.ex-layout-note {
  margin: 0;
  font-size: 11px;
  line-height: 1.5;
  color: var(--text-muted);
}

.ex-warnings {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px 12px;
  font-size: 12px;
  color: var(--text-secondary);
  background: var(--warning-light);
  border: 1px solid var(--warning);
  border-radius: var(--radius-sm);
}

.ex-warnings-caption {
  margin: 0;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
}

.ex-warnings-more {
  margin: 0;
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
