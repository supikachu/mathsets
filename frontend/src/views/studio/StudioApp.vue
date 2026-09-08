<script setup lang="ts">
/**
 * Typeset Studio — 结构化排版工作台（/studio）
 *
 * P0：三栏壳 + PublicationDocument 对象树 + 版面参数 + Typst 校对预览/导出
 * 主线见 docs/结构化排版_主线与冻结.md
 */
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import {
  exportApi,
  type ExamRequest,
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
import {
  type PublicationDocument,
  type PublicationKind,
  countQuestions,
  flattenTree,
  loadDocumentFromSession,
  saveDocumentToSession,
  sectionsFromDocument,
} from '@/types/publication'

const router = useRouter()
const toast = useToast()

const doc = ref<PublicationDocument | null>(null)
const selectedId = ref<string | null>(null)
const centerTab = ref<'preview' | 'canvas'>('preview')

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

const KIND_OPTIONS = [
  { value: 'exam', label: '试卷' },
  { value: 'handout', label: '讲义' },
  { value: 'topic', label: '专题' },
]

const MODES = [
  { value: 'student', label: '学生练习', hint: '仅题目' },
  { value: 'teacher', label: '教师讲义', hint: '含解析提示' },
  { value: 'exam', label: '标准考卷', hint: '卷末答案' },
]

const treeRows = computed(() => (doc.value ? flattenTree(doc.value) : []))
const questionCount = computed(() => (doc.value ? countQuestions(doc.value) : 0))

const exportSections = computed(() =>
  doc.value ? sectionsFromDocument(doc.value) : [],
)

const previewRequest = computed<ExamRequest | null>(() => {
  if (!doc.value || !questionCount.value) return null
  return snapshotRequest(exportSections.value)
})

onMounted(() => {
  const loaded = loadDocumentFromSession()
  if (!loaded || countQuestions(loaded) === 0) {
    toast.info('请先从试题篮选题再进入排版工作台')
    router.replace('/basket')
    return
  }
  doc.value = loaded
  title.value = loaded.title
  mode.value = loaded.export.mode
  includeAnswer.value = loaded.export.include_answer
  includeAnalysis.value = loaded.export.include_analysis
  answerAtEnd.value = loaded.export.answer_at_end
  if (loaded.export.callouts) {
    calloutKnowledge.value = !!loaded.export.callouts.knowledge
    calloutErrorProne.value = !!loaded.export.callouts.error_prone
    calloutAnalysis.value = !!loaded.export.callouts.analysis
  }
  void loadPresets()
})

watch(
  [doc, title, mode, includeAnswer, includeAnalysis, answerAtEnd, calloutKnowledge, calloutErrorProne, calloutAnalysis, layout],
  () => {
    if (!doc.value) return
    doc.value.title = title.value
    doc.value.export = {
      mode: mode.value,
      include_answer: includeAnswer.value,
      include_analysis: includeAnalysis.value,
      answer_at_end: answerAtEnd.value,
      callouts: {
        knowledge: calloutKnowledge.value,
        error_prone: calloutErrorProne.value,
        analysis: calloutAnalysis.value,
      },
    }
    doc.value.page = layout.value
    doc.value.updated_at = new Date().toISOString()
    saveDocumentToSession(doc.value)
  },
  { deep: true },
)

function onKindChange(value?: string) {
  if (doc.value && value) doc.value.kind = value as PublicationKind
}

function goBack() {
  router.push('/basket')
}

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
    const req = buildRequest(exportSections.value)
    const res: ExportResult = await exportApi.pdf(req)
    saveBlob(res.blob, res.filename || `${req.title}.pdf`)
    warnings.value = res.warnings
    toast.success(res.warnings.length ? `已导出（${res.warnings.length} 条警告）` : 'PDF 已导出')
  } catch (e) {
    toast.error((e as Error).message || '导出失败')
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div v-if="doc" class="studio">
    <header class="studio-header">
      <button type="button" class="studio-back" @click="goBack">
        <AppIcon name="arrow-left" :size="16" />
        <span>试题篮</span>
      </button>

      <div class="studio-header-center">
        <input v-model="title" class="studio-title" type="text" maxlength="80" placeholder="文档标题" />
        <AppSelect
          class="studio-kind"
          :model-value="doc.kind"
          :options="KIND_OPTIONS"
          @update:model-value="onKindChange"
        />
        <span class="studio-meta">{{ questionCount }} 题</span>
      </div>

      <div class="studio-header-actions">
        <div class="studio-tabs" role="tablist">
          <button
            type="button"
            class="studio-tab"
            :class="{ 'is-active': centerTab === 'preview' }"
            @click="centerTab = 'preview'"
          >
            Typst 预览
          </button>
          <button
            type="button"
            class="studio-tab"
            :class="{ 'is-active': centerTab === 'canvas' }"
            disabled
            title="P1：HTML 工作画布"
          >
            工作画布
          </button>
        </div>
        <AppButton :loading="busy" :disabled="!questionCount" @click="runExport">
          <AppIcon name="download" :size="14" />
          <span>{{ busy ? '导出中…' : '导出 PDF' }}</span>
        </AppButton>
      </div>
    </header>

    <div class="studio-main">
      <!-- 对象树 -->
      <aside class="studio-pane studio-tree">
        <h2 class="studio-pane-title">对象树</h2>
        <p class="studio-hint">P0 只读 · P2 支持多选装入多栏盒子</p>
        <ul class="studio-tree-list">
          <li
            v-for="row in treeRows"
            :key="row.id"
            class="studio-tree-item"
            :class="{ 'is-selected': selectedId === row.id }"
            :style="{ paddingLeft: `${12 + row.depth * 14}px` }"
            @click="selectedId = row.id"
          >
            <span class="studio-tree-type">{{ row.blockType }}</span>
            <span class="studio-tree-label">{{ row.label }}</span>
          </li>
        </ul>
      </aside>

      <!-- 中栏 -->
      <main class="studio-pane studio-center">
        <TypesetPreview v-if="centerTab === 'preview'" :request="previewRequest" />
        <div v-else class="studio-canvas-placeholder">
          HTML 工作画布（P1）— 可点选对象、装盒子
        </div>
      </main>

      <!-- 属性 / 版面 -->
      <aside class="studio-pane studio-props">
        <h2 class="studio-pane-title">版面与导出</h2>

        <section class="studio-section">
          <h3 class="studio-section-title">输出模式</h3>
          <div class="studio-mode-grid">
            <button
              v-for="opt in MODES"
              :key="opt.value"
              type="button"
              class="studio-mode-card"
              :class="{ 'is-active': mode === opt.value }"
              @click="pickMode(opt.value as any)"
            >
              <span class="studio-mode-label">{{ opt.label }}</span>
              <span class="studio-mode-hint">{{ opt.hint }}</span>
            </button>
          </div>
        </section>

        <section class="studio-section">
          <h3 class="studio-section-title">内容开关</h3>
          <label class="studio-switch"><input v-model="includeAnswer" type="checkbox" /><span>包含答案</span></label>
          <label class="studio-switch"><input v-model="includeAnalysis" type="checkbox" /><span>包含解析</span></label>
          <label class="studio-switch" :class="{ 'is-muted': !includeAnswer }">
            <input v-model="answerAtEnd" type="checkbox" :disabled="!includeAnswer" />
            <span>答案到卷末</span>
          </label>
        </section>

        <section class="studio-section" :class="{ 'is-muted': !isTeacher }">
          <h3 class="studio-section-title">教师提示</h3>
          <label class="studio-switch">
            <input v-model="calloutKnowledge" type="checkbox" :disabled="!isTeacher" /><span>考点</span>
          </label>
          <label class="studio-switch">
            <input v-model="calloutErrorProne" type="checkbox" :disabled="!isTeacher" /><span>易错</span>
          </label>
          <label class="studio-switch">
            <input v-model="calloutAnalysis" type="checkbox" :disabled="!isTeacher" /><span>思路</span>
          </label>
        </section>

        <section v-if="layout" class="studio-section">
          <h3 class="studio-section-title">整卷版面</h3>
          <div class="studio-select-row">
            <span>预设</span>
            <AppSelect :model-value="presetId" :options="presetOptions" @update:model-value="onPresetChange" />
          </div>
          <div class="studio-select-row">
            <span>纸张</span>
            <AppSelect :model-value="layout.paper" :options="PAPERS" @update:model-value="onPaperChange" />
          </div>
          <div class="studio-select-row">
            <span>栏数</span>
            <AppSelect
              :model-value="String(layout.columns)"
              :options="COLUMNS"
              @update:model-value="onColumnsChange"
            />
          </div>
          <div class="studio-select-row">
            <span>色彩</span>
            <AppSelect :model-value="layout.color" :options="COLORS" @update:model-value="onColorChange" />
          </div>

          <p class="studio-sub-caption">页边距（mm）</p>
          <div class="studio-num-grid">
            <label v-for="m in MARGIN_FIELDS" :key="m.key" class="studio-num-row">
              <span>{{ m.label }}</span>
              <input
                class="studio-num"
                type="number"
                :min="m.key === 'gutter_mm' ? GUTTER_MM.min : MARGIN_MM.min"
                :max="m.key === 'gutter_mm' ? GUTTER_MM.max : MARGIN_MM.max"
                :step="0.5"
                :value="layout.margins[m.key]"
                @change="onMarginChange(m.key, $event)"
              />
            </label>
          </div>

          <label class="studio-switch">
            <input type="checkbox" :checked="!!layout.binding" @change="onBindingToggle" />
            <span>密封线 / 装订带</span>
          </label>
          <div v-if="layout.binding" class="studio-binding">
            <div class="studio-select-row">
              <span>装订位</span>
              <AppSelect
                :model-value="layout.binding.position"
                :options="POSITIONS"
                @update:model-value="onBindingPosition"
              />
            </div>
            <label v-for="a in AREAS" :key="a.key" class="studio-switch">
              <input
                type="checkbox"
                :checked="layout.binding.areas[a.key]"
                @change="onAreaChange(a.key, $event)"
              />
              <span>{{ a.label }}</span>
            </label>
          </div>

          <p class="studio-sub-caption">页眉页脚</p>
          <label v-for="f in HEADER_FOOTER_FIELDS" :key="f.key" class="studio-switch">
            <input
              type="checkbox"
              :checked="layout.header_footer[f.key]"
              @change="onHeaderFooter(f.key, $event)"
            />
            <span>{{ f.label }}</span>
          </label>

          <p class="studio-sub-caption">答题留白</p>
          <div class="studio-select-row">
            <span>样式</span>
            <AppSelect
              :model-value="layout.answer_blank.style"
              :options="BLANK_STYLES"
              @update:model-value="onBlankChange"
            />
          </div>
          <label class="studio-slider">
            <span>高度</span>
            <input
              type="range"
              :min="BLANK_CM.min"
              :max="BLANK_CM.max"
              :step="BLANK_CM.step"
              :value="layout.answer_blank.height_cm"
              @input="onBlankHeightChange"
            />
            <strong>{{ layout.answer_blank.height_cm.toFixed(1) }}cm</strong>
          </label>
        </section>

        <section v-if="receiptRows.length" class="studio-section">
          <PreflightList :items="receiptRows" title="导出回执" />
        </section>
      </aside>
    </div>
  </div>
</template>

<style scoped>
.studio {
  display: flex;
  flex-direction: column;
  height: 100vh;
  background: var(--bg-primary, #f5f5f7);
}

.studio-header {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 16px;
  background: var(--bg-surface, #fff);
  border-bottom: 1px solid var(--border-secondary, #e5e5e5);
  flex-shrink: 0;
}

.studio-back {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: none;
  background: none;
  color: var(--text-link, #0071e3);
  font-size: 13px;
  cursor: pointer;
  padding: 6px 10px;
  border-radius: 6px;
}
.studio-back:hover {
  background: rgba(0, 0, 0, 0.04);
}

.studio-header-center {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 10px;
  min-width: 0;
}

.studio-title {
  border: none;
  background: transparent;
  font-size: 15px;
  font-weight: 600;
  text-align: center;
  max-width: 280px;
  padding: 4px 8px;
  border-radius: 6px;
}
.studio-title:hover,
.studio-title:focus {
  background: rgba(0, 0, 0, 0.04);
  outline: none;
}

.studio-kind {
  width: 100px;
  flex-shrink: 0;
}

.studio-meta {
  font-size: 12px;
  color: var(--text-secondary);
  white-space: nowrap;
}

.studio-header-actions {
  display: flex;
  align-items: center;
  gap: 10px;
}

.studio-tabs {
  display: flex;
  border: 1px solid var(--border-secondary, #e5e5e5);
  border-radius: 8px;
  overflow: hidden;
}

.studio-tab {
  border: none;
  background: transparent;
  padding: 6px 10px;
  font-size: 12px;
  cursor: pointer;
  color: var(--text-secondary);
}
.studio-tab.is-active {
  background: rgba(0, 113, 227, 0.08);
  color: var(--color-primary, #0071e3);
  font-weight: 600;
}
.studio-tab:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.studio-main {
  flex: 1;
  display: grid;
  grid-template-columns: 220px minmax(0, 1fr) 300px;
  overflow: hidden;
}

.studio-pane {
  overflow: auto;
  background: var(--bg-surface, #fff);
}

.studio-tree {
  border-right: 1px solid var(--border-secondary, #e5e5e5);
  padding: 12px;
}

.studio-center {
  background: var(--bg-secondary, #f0f0f2);
  padding: 12px;
}

.studio-props {
  border-left: 1px solid var(--border-secondary, #e5e5e5);
  padding: 12px;
}

.studio-pane-title {
  margin: 0 0 4px;
  font-size: 13px;
  font-weight: 600;
}

.studio-hint {
  margin: 0 0 10px;
  font-size: 11px;
  color: var(--text-tertiary);
}

.studio-tree-list {
  list-style: none;
  margin: 0;
  padding: 0;
}

.studio-tree-item {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 6px 8px;
  border-radius: 6px;
  cursor: pointer;
  font-size: 12px;
}
.studio-tree-item:hover {
  background: rgba(0, 0, 0, 0.04);
}
.studio-tree-item.is-selected {
  background: rgba(0, 113, 227, 0.1);
}

.studio-tree-type {
  font-size: 10px;
  color: var(--text-tertiary);
  text-transform: uppercase;
}

.studio-tree-label {
  color: var(--text-primary);
  word-break: break-all;
}

.studio-canvas-placeholder {
  display: grid;
  place-items: center;
  min-height: 60vh;
  color: var(--text-secondary);
  font-size: 14px;
  border: 1px dashed var(--border-secondary, #ccc);
  border-radius: 12px;
  background: #fff;
}

.studio-section {
  padding: 10px 0;
  border-bottom: 1px solid var(--border-tertiary, #f0f0f0);
}
.studio-section-title {
  margin: 0 0 8px;
  font-size: 12px;
  font-weight: 600;
}

.studio-mode-grid {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.studio-mode-card {
  text-align: left;
  border: 1px solid var(--border-secondary, #e5e5e5);
  border-radius: 8px;
  padding: 8px 10px;
  background: var(--bg-primary, #f5f5f7);
  cursor: pointer;
}
.studio-mode-card.is-active {
  border-color: var(--color-primary, #0071e3);
  background: rgba(0, 113, 227, 0.06);
}
.studio-mode-label {
  display: block;
  font-size: 12px;
  font-weight: 600;
}
.studio-mode-hint {
  font-size: 11px;
  color: var(--text-secondary);
}

.studio-switch {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  margin-bottom: 6px;
  cursor: pointer;
}
.studio-switch input {
  accent-color: var(--color-primary, #0071e3);
}

.is-muted {
  opacity: 0.45;
  pointer-events: none;
}

.studio-select-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
  font-size: 12px;
  color: var(--text-secondary);
}
.studio-select-row > span {
  min-width: 40px;
}

.studio-sub-caption {
  margin: 10px 0 6px;
  font-size: 11px;
  font-weight: 600;
  color: var(--text-secondary);
}

.studio-num-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 6px;
  margin-bottom: 8px;
}

.studio-num-row {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  color: var(--text-secondary);
}

.studio-num {
  width: 56px;
  padding: 3px 4px;
  border: 1px solid var(--border-secondary, #e5e5e5);
  border-radius: 6px;
  font-size: 12px;
  text-align: center;
}

.studio-binding {
  padding-left: 6px;
  margin-bottom: 8px;
}

.studio-slider {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--text-secondary);
}
.studio-slider input[type='range'] {
  flex: 1;
  accent-color: var(--color-primary, #0071e3);
}

@media (max-width: 1100px) {
  .studio-main {
    grid-template-columns: 180px minmax(0, 1fr);
  }
  .studio-props {
    display: none;
  }
}
</style>
