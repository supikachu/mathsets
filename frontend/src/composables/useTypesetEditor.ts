/**
 * useTypesetEditor — 排版参数状态管理 composable
 *
 * 从 ExportDialog 中提取的版面参数编辑逻辑，供独立排版页面和导出弹窗共用。
 * 管理 LayoutSpec 的完整生命周期：预设加载 → 选择 → 微调 → 构建 ExamRequest。
 */
import { computed, ref } from 'vue'
import {
  typesetApi,
  type Binding,
  type BlankStyle,
  type ColorMode,
  type ExamRequest,
  type ExamSectionRequest,
  type ExportMode,
  type LayoutSpec,
  type Margins,
  type Paper,
  type ProfilePreset,
} from '@/api/client'
import { useToast } from '@/composables/useToast'

/** 模式 → 默认预设映射 */
const MODE_PRESET: Record<ExportMode, string> = {
  student: 'a4_practice',
  teacher: 'a4_lecture',
  exam: 'a3_fold_exam',
}

export const PAPERS = [
  { value: 'a4', label: 'A4（210×297）' },
  { value: 'a3_fold', label: 'A3 对折（420×297）' },
  { value: 'a3_tri', label: 'A3 三栏（420×297）' },
]

export const COLUMNS = [
  { value: '1', label: '单栏' },
  { value: '2', label: '双栏' },
  { value: '3', label: '三栏' },
]

export const BLANK_STYLES = [
  { value: 'lines', label: '横线' },
  { value: 'dots', label: '点阵' },
  { value: 'blank', label: '纯空白' },
]

export const POSITIONS = [
  { value: 'left', label: '左侧装订带' },
  { value: 'center_fold', label: '中缝对折' },
]

export const COLORS = [
  { value: 'rich', label: '彩色（屏幕 / 激光打印）' },
  { value: 'print_black_only', label: '纯黑 K100（付印）' },
]

export const AREAS: { key: keyof Binding['areas']; label: string }[] = [
  { key: 'school', label: '学校' },
  { key: 'class', label: '班级' },
  { key: 'name', label: '姓名' },
  { key: 'exam_no', label: '考号' },
]

export const MARGIN_FIELDS: { key: keyof Margins; label: string }[] = [
  { key: 'top_mm', label: '上' },
  { key: 'bottom_mm', label: '下' },
  { key: 'left_mm', label: '左' },
  { key: 'right_mm', label: '右' },
  { key: 'gutter_mm', label: '栏间距' },
]

export const HEADER_FOOTER_FIELDS: { key: keyof LayoutSpec['header_footer']; label: string }[] = [
  { key: 'header_title', label: '页眉当前大题名' },
  { key: 'page_number', label: '页脚页码' },
  { key: 'odd_even_outer', label: '奇偶页码外侧对齐（双面印）' },
]

export const MARGIN_MM = { min: 5, max: 40 }
export const GUTTER_MM = { min: 0, max: 40 }
export const BLANK_CM = { min: 2, max: 20, step: 0.5 }

export function useTypesetEditor() {
  const toast = useToast()

  const mode = ref<ExportMode>('teacher')
  const title = ref('未命名试卷')
  const includeAnswer = ref(true)
  const includeAnalysis = ref(true)
  const answerAtEnd = ref(false)
  const calloutKnowledge = ref(true)
  const calloutErrorProne = ref(true)
  const calloutAnalysis = ref(false)

  const presets = ref<ProfilePreset[]>([])
  const presetId = ref('')
  const layout = ref<LayoutSpec | null>(null)
  const presetTouched = ref(false)
  let bindingMemo: Binding | null = null

  const isTeacher = computed(() => mode.value === 'teacher')

  const presetOptions = computed(() =>
    presets.value.map((p) => ({ value: p.id, label: p.label })),
  )

  function applyPreset(id: string) {
    const hit = presets.value.find((p) => p.id === id)
    if (!hit) return
    presetId.value = hit.id
    layout.value = JSON.parse(JSON.stringify(hit.spec)) as LayoutSpec
    bindingMemo = layout.value.binding ? JSON.parse(JSON.stringify(layout.value.binding)) : null
  }

  async function loadPresets() {
    if (presets.value.length) return
    try {
      const { data } = await typesetApi.profiles()
      presets.value = data
      applyPreset(presetId.value || MODE_PRESET[mode.value])
    } catch (e) {
      toast.error((e as Error).message || '版面预设加载失败')
    }
  }

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
    if (layout.value && value) layout.value.answer_blank.style = value as BlankStyle
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

  function buildRequest(sections: ExamSectionRequest[]): ExamRequest {
    return {
      title: title.value.trim() || '未命名试卷',
      exam_meta: { instructions: [] },
      mode: mode.value,
      sections,
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
      spec: layout.value ?? undefined,
    }
  }

  /** 返回深拷贝的预览请求（防止 debounce 期间引用被修改） */
  function snapshotRequest(sections: ExamSectionRequest[]): ExamRequest {
    return JSON.parse(JSON.stringify(buildRequest(sections))) as ExamRequest
  }

  return {
    // state
    mode,
    title,
    includeAnswer,
    includeAnalysis,
    answerAtEnd,
    calloutKnowledge,
    calloutErrorProne,
    calloutAnalysis,
    presets,
    presetId,
    layout,
    presetTouched,
    isTeacher,
    presetOptions,

    // actions
    loadPresets,
    applyPreset,
    pickMode,
    onPresetChange,
    onPaperChange,
    onColumnsChange,
    onColorChange,
    onBlankChange,
    onMarginChange,
    onBlankHeightChange,
    onBindingToggle,
    onBindingPosition,
    onAreaChange,
    onHeaderFooter,
    buildRequest,
    snapshotRequest,
  }
}
