<script setup lang="ts">
/**
 * TypesetEditor — 独立排版编辑器（全屏沉浸式）
 *
 * 从试题篮进入，左侧参数面板 + 右侧 Typst SVG 实时预览。
 * 版面参数逻辑复用 useTypesetEditor composable（与 ExportDialog 同源）。
 */
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import {
  exportApi,
  type ExamRequest,
  type ExamSectionRequest,
  type ExportResult,
  type ExportWarning,
  type Issue,
} from '@/api/client'
import { AppButton, AppIcon, AppSelect } from '@/components/ui'
import TypesetPreview from '@/components/TypesetPreview.vue'
import PreflightList from '@/components/PreflightList.vue'
import { useToast } from '@/composables/useToast'
import {
  useTypesetEditor,
  PAPERS,
  COLUMNS,
  BLANK_STYLES,
  POSITIONS,
  COLORS,
  AREAS,
  MARGIN_FIELDS,
  HEADER_FOOTER_FIELDS,
  MARGIN_MM,
  GUTTER_MM,
  BLANK_CM,
} from '@/composables/useTypesetEditor'

const router = useRouter()
const toast = useToast()

const {
  mode,
  title,
  includeAnswer,
  includeAnalysis,
  answerAtEnd,
  calloutKnowledge,
  calloutErrorProne,
  calloutAnalysis,
  presetId,
  layout,
  isTeacher,
  presetOptions,
  loadPresets,
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
  snapshotRequest,
  buildRequest,
} = useTypesetEditor()

// ── 从 sessionStorage 读取试题篮传过来的数据 ──
const sections = ref<ExamSectionRequest[]>([])
const questionCount = computed(() =>
  sections.value.reduce((n, s) => n + s.questions.length, 0),
)

onMounted(() => {
  const raw = sessionStorage.getItem('typeset_sections')
  if (raw) {
    try {
      sections.value = JSON.parse(raw)
    } catch { /* ignore */ }
  }
  const savedTitle = sessionStorage.getItem('typeset_title')
  if (savedTitle) title.value = savedTitle

  if (!sections.value.length) {
    toast.info('没有找到试题数据，请从试题篮进入')
    router.replace('/basket')
    return
  }

  void loadPresets()
})

// ── 预览请求 ──
const previewRequest = computed<ExamRequest | null>(() =>
  questionCount.value > 0 ? snapshotRequest(sections.value) : null,
)

// ── 导出 ──
const MODES = [
  { value: 'student', label: '学生练习', hint: '仅题目，无答案无解析' },
  { value: 'teacher', label: '教师讲义', hint: '内嵌答案解析与考点提示' },
  { value: 'exam', label: '标准考卷', hint: '题目成卷，卷末汇总答案' },
]

const busy = ref(false)
const warnings = ref<ExportWarning[]>([])

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
  setTimeout(() => URL.revokeObjectURL(url), 1000)
}

async function runExport() {
  if (!questionCount.value || busy.value) return
  busy.value = true
  warnings.value = []
  try {
    const req = buildRequest(sections.value)
    const res: ExportResult = await exportApi.pdf(req)
    const fallback = `${req.title}.pdf`
    saveBlob(res.blob, res.filename || fallback)
    warnings.value = res.warnings
    if (res.warnings.length) {
      toast.info(`已导出，但有 ${res.warnings.length} 条降级警告`)
    } else {
      toast.success('PDF 已导出')
    }
  } catch (e) {
    toast.error((e as Error).message || '导出失败')
  } finally {
    busy.value = false
  }
}

function goBack() {
  router.push('/basket')
}
</script>

<template>
  <div class="te-page">
    <!-- 顶栏 -->
    <header class="te-header">
      <button type="button" class="te-back" @click="goBack">
        <AppIcon name="arrow-left" :size="16" />
        <span>返回试题篮</span>
      </button>
      <div class="te-header-center">
        <input
          v-model="title"
          class="te-title-input"
          type="text"
          maxlength="80"
          placeholder="试卷标题"
        />
        <span class="te-question-count">{{ questionCount }} 题</span>
      </div>
      <div class="te-header-actions">
        <AppButton :loading="busy" :disabled="!questionCount" @click="runExport">
          <AppIcon name="download" :size="14" />
          <span>{{ busy ? '导出中…' : '导出 PDF' }}</span>
        </AppButton>
      </div>
    </header>

    <!-- 主体：左参数 + 右预览 -->
    <div class="te-main">
      <!-- 左侧参数面板 -->
      <aside class="te-sidebar">
        <div class="te-section">
          <h3 class="te-section-title">输出模式</h3>
          <div class="te-mode-grid">
            <button
              v-for="opt in MODES"
              :key="opt.value"
              type="button"
              class="te-mode-card"
              :class="{ 'is-active': mode === opt.value }"
              @click="pickMode(opt.value as any)"
            >
              <span class="te-mode-label">{{ opt.label }}</span>
              <span class="te-mode-hint">{{ opt.hint }}</span>
            </button>
          </div>
        </div>

        <div class="te-section">
          <h3 class="te-section-title">内容开关</h3>
          <div class="te-switch-list">
            <label class="te-switch-row">
              <input v-model="includeAnswer" type="checkbox" />
              <span>包含答案</span>
            </label>
            <label class="te-switch-row">
              <input v-model="includeAnalysis" type="checkbox" />
              <span>包含解析</span>
            </label>
            <label class="te-switch-row" :class="{ 'is-muted': !includeAnswer }">
              <input v-model="answerAtEnd" type="checkbox" :disabled="!includeAnswer" />
              <span>答案汇总到卷末</span>
            </label>
          </div>
        </div>

        <div class="te-section" :class="{ 'is-muted': !isTeacher }">
          <h3 class="te-section-title">
            教师提示框
            <span v-if="!isTeacher" class="te-note">仅讲义模式</span>
          </h3>
          <div class="te-switch-list">
            <label class="te-switch-row">
              <input v-model="calloutKnowledge" type="checkbox" :disabled="!isTeacher" />
              <span>考点</span>
            </label>
            <label class="te-switch-row">
              <input v-model="calloutErrorProne" type="checkbox" :disabled="!isTeacher" />
              <span>易错点</span>
            </label>
            <label class="te-switch-row">
              <input v-model="calloutAnalysis" type="checkbox" :disabled="!isTeacher" />
              <span>思路点拨</span>
            </label>
          </div>
        </div>

        <div v-if="layout" class="te-section">
          <h3 class="te-section-title">版面参数</h3>

          <div class="te-layout-grid">
            <div class="te-select-row">
              <span class="te-caption">预设</span>
              <AppSelect
                :model-value="presetId"
                :options="presetOptions"
                @update:model-value="onPresetChange"
              />
            </div>
            <div class="te-select-row">
              <span class="te-caption">纸张</span>
              <AppSelect
                :model-value="layout.paper"
                :options="PAPERS"
                @update:model-value="onPaperChange"
              />
            </div>
            <div class="te-select-row">
              <span class="te-caption">栏数</span>
              <AppSelect
                :model-value="String(layout.columns)"
                :options="COLUMNS"
                @update:model-value="onColumnsChange"
              />
            </div>
            <div class="te-select-row">
              <span class="te-caption">色彩</span>
              <AppSelect
                :model-value="layout.color"
                :options="COLORS"
                @update:model-value="onColorChange"
              />
            </div>
          </div>

          <div class="te-sub">
            <span class="te-sub-caption">页边距（mm）</span>
            <div class="te-num-grid">
              <label v-for="m in MARGIN_FIELDS" :key="m.key" class="te-num-row">
                <span>{{ m.label }}</span>
                <input
                  class="te-num"
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

          <div class="te-sub">
            <label class="te-switch-row">
              <input
                type="checkbox"
                :checked="!!layout.binding"
                @change="onBindingToggle"
              />
              <span>密封线 / 装订带</span>
            </label>
            <div v-if="layout.binding" class="te-binding">
              <div class="te-select-row te-select-row--narrow">
                <span class="te-caption">装订位</span>
                <AppSelect
                  :model-value="layout.binding.position"
                  :options="POSITIONS"
                  @update:model-value="onBindingPosition"
                />
              </div>
              <div class="te-switch-list">
                <label v-for="a in AREAS" :key="a.key" class="te-switch-row">
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

          <div class="te-sub">
            <span class="te-sub-caption">页眉页脚</span>
            <div class="te-switch-list">
              <label v-for="f in HEADER_FOOTER_FIELDS" :key="f.key" class="te-switch-row">
                <input
                  type="checkbox"
                  :checked="layout.header_footer[f.key]"
                  @change="onHeaderFooter(f.key, $event)"
                />
                <span>{{ f.label }}</span>
              </label>
            </div>
          </div>

          <div class="te-sub">
            <span class="te-sub-caption">答题留白</span>
            <div class="te-blank">
              <div class="te-select-row te-select-row--narrow">
                <span class="te-caption">样式</span>
                <AppSelect
                  :model-value="layout.answer_blank.style"
                  :options="BLANK_STYLES"
                  @update:model-value="onBlankChange"
                />
              </div>
              <label class="te-slider">
                <span>高度</span>
                <input
                  type="range"
                  :min="BLANK_CM.min"
                  :max="BLANK_CM.max"
                  :step="BLANK_CM.step"
                  :value="layout.answer_blank.height_cm"
                  @input="onBlankHeightChange"
                />
                <strong class="te-slider-value">{{ layout.answer_blank.height_cm.toFixed(1) }}cm</strong>
              </label>
            </div>
          </div>
        </div>

        <!-- 导出回执 -->
        <div v-if="receiptRows.length" class="te-section">
          <PreflightList :items="receiptRows" title="导出回执" />
        </div>
      </aside>

      <!-- 右侧预览 -->
      <main class="te-preview">
        <TypesetPreview :request="previewRequest" />
      </main>
    </div>
  </div>
</template>

<style scoped>
.te-page {
  display: flex;
  flex-direction: column;
  height: 100vh;
  background: var(--bg-primary, #f5f5f7);
}

/* ── 顶栏 ── */
.te-header {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 10px 20px;
  background: var(--bg-surface, #fff);
  border-bottom: 1px solid var(--border-secondary, #e5e5e5);
  flex-shrink: 0;
}

.te-back {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  border: none;
  background: none;
  color: var(--text-link, #0071e3);
  font-size: 14px;
  cursor: pointer;
  border-radius: 6px;
  transition: background 0.15s;
}
.te-back:hover {
  background: var(--bg-hover, rgba(0, 0, 0, 0.04));
}

.te-header-center {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 12px;
  justify-content: center;
}

.te-title-input {
  border: none;
  background: none;
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary);
  text-align: center;
  width: 300px;
  padding: 4px 8px;
  border-radius: 6px;
  transition: background 0.15s;
}
.te-title-input:hover,
.te-title-input:focus {
  background: var(--bg-hover, rgba(0, 0, 0, 0.04));
  outline: none;
}

.te-question-count {
  font-size: 13px;
  color: var(--text-secondary);
  white-space: nowrap;
}

.te-header-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

/* ── 主体 ── */
.te-main {
  display: grid;
  grid-template-columns: 340px minmax(0, 1fr);
  flex: 1;
  overflow: hidden;
}

/* ── 左侧参数面板 ── */
.te-sidebar {
  overflow-y: auto;
  padding: 16px;
  border-right: 1px solid var(--border-secondary, #e5e5e5);
  background: var(--bg-surface, #fff);
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.te-section {
  padding: 12px 0;
  border-bottom: 1px solid var(--border-tertiary, #f0f0f0);
}
.te-section:last-child {
  border-bottom: none;
}

.te-section-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
  margin: 0 0 10px;
  display: flex;
  align-items: center;
  gap: 6px;
}

.te-note {
  font-size: 11px;
  font-weight: 400;
  color: var(--text-tertiary);
}

.te-mode-grid {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.te-mode-card {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 8px 12px;
  border: 1px solid var(--border-secondary, #e5e5e5);
  border-radius: 8px;
  background: var(--bg-primary, #f5f5f7);
  cursor: pointer;
  text-align: left;
  transition: all 0.15s;
}
.te-mode-card:hover {
  border-color: var(--color-primary, #0071e3);
}
.te-mode-card.is-active {
  border-color: var(--color-primary, #0071e3);
  background: rgba(0, 113, 227, 0.06);
}

.te-mode-label {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
}

.te-mode-hint {
  font-size: 11px;
  color: var(--text-secondary);
}

.te-switch-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.te-switch-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  color: var(--text-primary);
  cursor: pointer;
}
.te-switch-row input[type='checkbox'] {
  accent-color: var(--color-primary, #0071e3);
}

.is-muted {
  opacity: 0.45;
  pointer-events: none;
}

.te-layout-grid {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.te-select-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.te-caption {
  font-size: 12px;
  color: var(--text-secondary);
  min-width: 48px;
  flex-shrink: 0;
}

.te-sub {
  margin-top: 12px;
  padding-top: 10px;
  border-top: 1px solid var(--border-tertiary, #f0f0f0);
}

.te-sub-caption {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
  display: block;
  margin-bottom: 8px;
}

.te-num-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 6px;
}

.te-num-row {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--text-secondary);
}

.te-num {
  width: 64px;
  padding: 4px 6px;
  border: 1px solid var(--border-secondary, #e5e5e5);
  border-radius: 6px;
  font-size: 13px;
  text-align: center;
}

.te-binding {
  margin-top: 8px;
  padding-left: 8px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.te-select-row--narrow {
  max-width: 220px;
}

.te-blank {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.te-slider {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--text-secondary);
}
.te-slider input[type='range'] {
  flex: 1;
  accent-color: var(--color-primary, #0071e3);
}
.te-slider-value {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
  min-width: 48px;
  text-align: right;
}

/* ── 右侧预览 ── */
.te-preview {
  overflow: auto;
  padding: 16px;
  background: var(--bg-secondary, #f0f0f2);
}

/* ── 响应式 ── */
@media (max-width: 768px) {
  .te-main {
    grid-template-columns: 1fr;
  }
  .te-sidebar {
    border-right: none;
    border-bottom: 1px solid var(--border-secondary, #e5e5e5);
    max-height: 50vh;
  }
}
</style>
