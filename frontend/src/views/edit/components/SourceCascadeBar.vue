<script setup lang="ts">
/**
 * 来源级联条：大类 → 子类；试卷可填卷信息 +「同时创建试卷」
 * 变更 debounce 后 emit confirm，不阻塞 OCR。
 */
import { ref, watch, computed, onMounted, nextTick } from 'vue'
import { AppIcon, AppSelect } from '@/components/ui'
import {
  paperApi,
  type DocumentMeta,
  type ConfirmDocumentRequest,
  type PaperBrief,
  type AiClassification,
  type AiPaperMetaSuggestion,
} from '@/api/client'
import {
  SOURCE_CATEGORY_LABELS,
  kindsForCategory,
  defaultKindForCategory,
  defaultCreatePaper,
  mapLegacyDocumentType,
  type SourceCategory,
  type SourceKind,
  type QuestionSourceState,
} from '@/utils/questionSource'
import {
  YEAR_OPTIONS,
  STAGE_OPTIONS,
  SUBJECT_OPTIONS,
  SEMESTER_OPTIONS,
  PROVINCE_OPTIONS,
  gradesForStage,
  citiesForProvince,
  canonicalProvince,
} from '@/utils/paperFormOptions'

const props = withDefaults(defineProps<{
  doc: DocumentMeta | null
  /** 保存中 */
  saving?: boolean
  /** panel：独占右侧整页，始终展开试卷详情 */
  variant?: 'bar' | 'panel'
}>(), {
  variant: 'bar',
})

const emit = defineEmits<{
  (e: 'confirm', body: ConfirmDocumentRequest): void
  (e: 'update:state', state: QuestionSourceState): void
}>()

const category = ref<SourceCategory>('practice')
const kind = ref<SourceKind>('in_class')
const createPaper = ref(false)
const title = ref('')
const subSource = ref('')

const paperForm = ref({
  title: '',
  year: '',
  stage: '',
  grade: '',
  subject: '数学',
  semester: '',
  regionProvince: '',
  regionCity: '',
  schoolName: '',
  paperId: '',
})

const paperBriefs = ref<PaperBrief[]>([])
const expanded = ref(props.variant === 'panel')
/** 本资料已创建/关联的试卷（与「关联已有试卷」下拉分开，避免 emit 冲掉 paper_id） */
const linkedPaperId = ref('')
let debounceTimer: ReturnType<typeof setTimeout> | null = null
let seeding = false
let seededDocId: string | null = null
let userTouched = false
let lastSentKey = ''

const isPanel = computed(() => props.variant === 'panel')
const kindOptions = computed(() => kindsForCategory(category.value))
const showPaperFields = computed(() => category.value === 'paper')
const showSubSource = computed(() => kind.value === 'mock')
const showPaperDetail = computed(() =>
  category.value === 'paper' && (isPanel.value || expanded.value),
)
const gradeOptions = computed(() => gradesForStage(paperForm.value.stage))
const yearChoices = computed(() => {
  const y = String(new Date().getFullYear())
  return YEAR_OPTIONS.includes(y) ? YEAR_OPTIONS : [...YEAR_OPTIONS, y]
})
const yearSelectOptions = computed(() => yearChoices.value.map((y) => ({ label: y, value: y })))
const citySelectOptions = computed(() => citiesForProvince(paperForm.value.regionProvince).map((c) => ({ label: c, value: c })))
const provinceSelectOptions = computed(() => PROVINCE_OPTIONS.map((p) => ({ label: p, value: p })))
const paperSelectOptions = computed(() => paperBriefs.value.map((p) => ({ label: p.title, value: p.id })))

function pickNonEmpty(current: string, incoming?: string | null) {
  if (current.trim()) return current
  return incoming ?? ''
}

function pickSuggest(current: string, incoming?: string | null) {
  return pickNonEmpty(current, incoming)
}

function applyAiPaperFields(
  ai: AiClassification | null | undefined,
  incomingTitle: string,
  mode: 'replace' | 'suggest',
) {
  if (!ai) return
  const suggest = mode === 'suggest'
    if (ai.create_paper != null && (!suggest || !userTouched)) {
    createPaper.value = ai.create_paper
  }
  const pm: AiPaperMetaSuggestion | undefined = ai.paper_meta
  if (!pm && !ai.title) return

  const nextTitle = suggest
    ? pickSuggest(title.value, pm?.title || ai.title || incomingTitle)
    : (pm?.title || ai.title || incomingTitle)
  title.value = nextTitle

  if (category.value !== 'paper') return

  paperForm.value = {
    title: suggest
      ? pickSuggest(paperForm.value.title, pm?.title || ai.title || incomingTitle)
      : (pm?.title || ai.title || incomingTitle),
    year: suggest
      ? pickSuggest(paperForm.value.year, pm?.year != null ? String(pm.year) : '')
      : (pm?.year != null ? String(pm.year) : ''),
    stage: suggest ? pickSuggest(paperForm.value.stage, pm?.stage) : (pm?.stage || ''),
    grade: suggest ? pickSuggest(paperForm.value.grade, pm?.grade) : (pm?.grade || ''),
    subject: suggest
      ? pickSuggest(paperForm.value.subject, pm?.subject) || '数学'
      : (pm?.subject || '数学'),
    semester: suggest ? pickSuggest(paperForm.value.semester, pm?.semester) : (pm?.semester || ''),
    regionProvince: suggest
      ? pickSuggest(paperForm.value.regionProvince, canonicalProvince(pm?.region_province))
      : canonicalProvince(pm?.region_province),
    regionCity: suggest ? pickSuggest(paperForm.value.regionCity, pm?.region_city) : (pm?.region_city || ''),
    schoolName: suggest ? pickSuggest(paperForm.value.schoolName, pm?.school_name) : (pm?.school_name || ''),
    paperId: paperForm.value.paperId,
  }
  if (pm?.sub_source_type) {
    subSource.value = suggest ? pickSuggest(subSource.value, pm.sub_source_type) : pm.sub_source_type
  }
  if (createPaper.value || isPanel.value) expanded.value = true
}

function seedFromDoc(doc: DocumentMeta | null, mode: 'replace' | 'suggest' = 'replace') {
  if (!doc) return
  const sameDoc = seededDocId === doc.id
  if (mode === 'replace' && sameDoc && userTouched) return

  seeding = true
  try {
    const meta = doc.metadata || {}
    const ai = doc.ai_classification
    let cat = (meta.source_category || ai?.source_category) as SourceCategory | undefined
    let k = (meta.source_kind || ai?.source_kind) as SourceKind | undefined
    if (!cat || !k) {
      const legacy = doc.document_type || ai?.document_type
      if (legacy) {
        const m = mapLegacyDocumentType(legacy)
        cat = m.category
        k = m.kind
      }
    }
    const pm = meta.paper_meta || {}
    const incomingTitle = (meta.title as string)
      || doc.title
      || ai?.title
      || doc.file_name?.replace(/\.[^.]+$/, '')
      || ''

    if (mode === 'suggest' && sameDoc && userTouched) {
      createPaper.value = createPaper.value || Boolean(meta.create_paper) || Boolean(ai?.create_paper)
      applyAiPaperFields(ai, incomingTitle, 'suggest')
    } else {
      category.value = cat || 'practice'
      kind.value = k || defaultKindForCategory(category.value)
      createPaper.value = ai?.create_paper ?? (Boolean(meta.create_paper) || defaultCreatePaper(kind.value))
      title.value = incomingTitle
      paperForm.value = {
        title: pm.title || incomingTitle,
        year: pm.year != null ? String(pm.year) : '',
        stage: pm.stage || '',
        grade: pm.grade || '',
        subject: pm.subject || '数学',
        semester: pm.semester || '',
        regionProvince: canonicalProvince(pm.region_province),
        regionCity: pm.region_city || '',
        schoolName: pm.school_name || '',
        paperId: pm.paper_id || '',
      }
      subSource.value = pm.sub_source_type || doc.sub_source_type || ''
      userTouched = false
      applyAiPaperFields(ai, incomingTitle, 'replace')
    }
    const linked = (meta.linked_paper_id as string) || pm.paper_id || ''
    if (linked) linkedPaperId.value = linked
    seededDocId = doc.id
  } finally {
    void nextTick().then(() => {
      seeding = false
    })
  }
  emitState()
}

onMounted(async () => {
  seedFromDoc(props.doc, 'replace')
  try {
    const { data } = await paperApi.listBrief()
    paperBriefs.value = data
  } catch { /* ignore */ }
})

watch(
  () => props.doc?.id,
  (id, prev) => {
    if (id && id !== prev) seedFromDoc(props.doc, 'replace')
  },
)
watch(
  () => JSON.stringify(props.doc?.ai_classification ?? null),
  (next, prev) => {
    if (!props.doc || next === prev) return
    seedFromDoc(props.doc, 'suggest')
  },
)
watch(
  () => props.doc?.metadata?.linked_paper_id,
  (id) => {
    if (typeof id === 'string' && id) {
      linkedPaperId.value = id
      emitState()
    }
  },
)

watch(
  () => paperForm.value.stage,
  (stage, prev) => {
    if (seeding || !prev || stage === prev) return
    if (!gradesForStage(stage).some((g) => g.value === paperForm.value.grade)) {
      paperForm.value.grade = ''
    }
  },
)

watch(
  () => paperForm.value.regionProvince,
  (province, prev) => {
    if (seeding || !prev || province === prev) return
    const cities = citiesForProvince(province)
    if (paperForm.value.regionCity && !cities.includes(paperForm.value.regionCity)) {
      paperForm.value.regionCity = ''
    }
  },
)

function selectCategory(c: SourceCategory) {
  category.value = c
  kind.value = defaultKindForCategory(c)
  if (c === 'paper') {
    createPaper.value = defaultCreatePaper(kind.value)
  } else {
    createPaper.value = false
  }
  scheduleConfirm()
}

function selectKind(k: SourceKind) {
  kind.value = k
  if (category.value === 'paper' && !props.doc?.metadata?.user_confirmed) {
    createPaper.value = defaultCreatePaper(k)
  }
  scheduleConfirm()
}

function buildBody(): ConfirmDocumentRequest {
  const body: ConfirmDocumentRequest = {
    source_category: category.value,
    source_kind: kind.value,
    create_paper: category.value === 'paper' && createPaper.value,
    title: title.value.trim() || undefined,
    source_type: kind.value,
    sub_source_type: showSubSource.value ? (subSource.value.trim() || undefined) : undefined,
  }
  if (category.value === 'paper') {
    body.paper_meta = {
      title: (paperForm.value.title || title.value).trim(),
      year: paperForm.value.year ? Number(paperForm.value.year) : undefined,
      stage: paperForm.value.stage || undefined,
      grade: paperForm.value.grade || undefined,
      subject: paperForm.value.subject || undefined,
      semester: paperForm.value.semester || undefined,
      region_province: canonicalProvince(paperForm.value.regionProvince) || undefined,
      region_city: paperForm.value.regionCity.trim() || undefined,
      school_name: paperForm.value.schoolName.trim() || undefined,
      source_type: kind.value,
      sub_source_type: showSubSource.value ? (subSource.value.trim() || undefined) : undefined,
      paper_id: paperForm.value.paperId || undefined,
    }
  }
  return body
}

function emitState() {
  const body = buildBody()
  const paperId = body.paper_meta?.paper_id || linkedPaperId.value || undefined
  emit('update:state', {
    source_category: category.value,
    source_kind: kind.value,
    create_paper: Boolean(body.create_paper),
    title: body.title,
    sub_source_type: body.sub_source_type,
    paper_meta: body.paper_meta
      ? { ...body.paper_meta, paper_id: paperId } as any
      : (paperId ? { title: title.value, paper_id: paperId } as any : undefined),
  })
}

function confirmKey(body: ConfirmDocumentRequest): string {
  return JSON.stringify(body)
}

function scheduleConfirm() {
  if (seeding) return
  userTouched = true
  emitState()
  if (!props.doc) return
  if (debounceTimer) clearTimeout(debounceTimer)
  debounceTimer = setTimeout(() => {
    if (seeding) return
    const body = buildBody()
    if (body.create_paper && !body.paper_meta?.paper_id && !body.paper_meta?.title?.trim()) {
      return
    }
    const key = confirmKey(body)
    if (key === lastSentKey) return
    lastSentKey = key
    emit('confirm', body)
  }, 600)
}

watch(
  [title, subSource, createPaper, paperForm],
  () => scheduleConfirm(),
  { deep: true },
)

watch(createPaper, (enabled) => {
  if (enabled) expanded.value = true
})
</script>

<template>
  <div class="source-cascade" :class="{ 'is-panel': isPanel }">
    <header v-if="!isPanel" class="sc-header">
      <div>
        <h3>来源信息</h3>
        <p>用于统一本批题目的属性与试卷归属</p>
      </div>
      <span v-if="saving" class="sc-saving">
        <AppIcon name="loader" :size="12" />
        同步中
      </span>
    </header>

    <section class="sc-section">
      <div class="sc-section-head">
        <span class="sc-section-label">资料来源</span>
        <span class="sc-section-hint">选择最符合原文的分类</span>
      </div>
      <div class="sc-cats sc-segmented">
        <button
          v-for="(label, key) in SOURCE_CATEGORY_LABELS"
          :key="key"
          type="button"
          class="sc-chip"
          :class="{ active: category === key }"
          @click="selectCategory(key as SourceCategory)"
        >{{ label }}</button>
      </div>
    </section>

    <section class="sc-section sc-section-plain">
      <div class="sc-section-head">
        <span class="sc-section-label">具体类型</span>
      </div>
      <div class="sc-kinds">
        <button
          v-for="opt in kindOptions"
          :key="opt.value"
          type="button"
          class="sc-chip sc-chip-sm"
          :class="{ active: kind === opt.value }"
          @click="selectKind(opt.value)"
        >{{ opt.label }}</button>
      </div>
    </section>

    <div v-if="showPaperFields" class="sc-paper-card">
      <div class="sc-paper-main">
        <div class="sc-paper-copy">
          <span class="sc-paper-title">创建试卷</span>
          <span class="sc-paper-desc">保存题目时，同时建立并关联一份试卷</span>
        </div>
        <label class="sc-switch" aria-label="同时创建试卷">
          <input v-model="createPaper" type="checkbox" @change="scheduleConfirm" />
          <span class="sc-switch-track" aria-hidden="true">
            <span class="sc-switch-thumb" />
          </span>
        </label>
      </div>
      <button
        v-if="!isPanel"
        type="button"
        class="sc-detail-toggle"
        @click="expanded = !expanded"
      >
        <span>{{ expanded ? '收起试卷信息' : (createPaper ? '编辑试卷信息' : '添加试卷信息') }}</span>
        <span v-if="!createPaper" class="sc-optional">选填</span>
        <AppIcon
          name="chevron-down"
          :size="14"
          :class="{ 'is-open': expanded }"
        />
      </button>
    </div>

    <div v-if="showPaperDetail" class="sc-form">
      <div class="sc-form-heading">
        <div>
          <h4>试卷详情</h4>
          <p>这些信息将同步到本批全部题目</p>
        </div>
      </div>
      <div class="sc-grid">
        <div class="sc-field sc-field-wide">
          <label for="sc-paper-title">试卷名称 <span v-if="createPaper" class="req">*</span></label>
          <input
            id="sc-paper-title"
            v-model="paperForm.title"
            type="text"
            placeholder="请输入试卷名称"
            @input="title = paperForm.title"
          />
        </div>
        <div class="sc-field">
          <label>年份</label>
          <AppSelect
            :model-value="paperForm.year || undefined"
            :options="yearSelectOptions"
            placeholder="未选"
            clearable
            @update:model-value="(v) => { paperForm.year = v ?? '' }"
          />
        </div>
        <div class="sc-field">
          <label>学段</label>
          <AppSelect
            :model-value="paperForm.stage || undefined"
            :options="[...STAGE_OPTIONS]"
            placeholder="未选"
            clearable
            @update:model-value="(v) => { paperForm.stage = v ?? '' }"
          />
        </div>
        <div class="sc-field">
          <label>年级</label>
          <AppSelect
            :model-value="paperForm.grade || undefined"
            :options="gradeOptions"
            :placeholder="paperForm.stage ? '未选' : '请先选学段'"
            :disabled="!paperForm.stage"
            clearable
            @update:model-value="(v) => { paperForm.grade = v ?? '' }"
          />
        </div>
        <div class="sc-field">
          <label>学科</label>
          <AppSelect
            :model-value="paperForm.subject || undefined"
            :options="[...SUBJECT_OPTIONS]"
            placeholder="未选"
            @update:model-value="(v) => { paperForm.subject = v || '数学' }"
          />
        </div>
        <div class="sc-field">
          <label>学期</label>
          <AppSelect
            :model-value="paperForm.semester || undefined"
            :options="[...SEMESTER_OPTIONS]"
            placeholder="未选"
            clearable
            @update:model-value="(v) => { paperForm.semester = v ?? '' }"
          />
        </div>
        <div class="sc-field">
          <label>省份</label>
          <AppSelect
            :model-value="paperForm.regionProvince || undefined"
            :options="provinceSelectOptions"
            placeholder="未选"
            clearable
            @update:model-value="(v) => { paperForm.regionProvince = v ?? '' }"
          />
        </div>
        <div class="sc-field">
          <label>城市</label>
          <AppSelect
            :model-value="paperForm.regionCity || undefined"
            :options="citySelectOptions"
            :placeholder="paperForm.regionProvince ? '未选' : '请先选省份'"
            :disabled="!paperForm.regionProvince"
            clearable
            @update:model-value="(v) => { paperForm.regionCity = v ?? '' }"
          />
        </div>
        <div class="sc-field">
          <label>学校</label>
          <input v-model="paperForm.schoolName" type="text" placeholder="学校名称（选填）" />
        </div>
      </div>
      <div v-if="showSubSource" class="sc-field">
        <label>模拟考试批次</label>
        <div class="sc-kinds">
          <button
            type="button"
            class="sc-chip sc-chip-sm"
            :class="{ active: subSource === '一模' }"
            @click="subSource = '一模'; scheduleConfirm()"
          >一模</button>
          <button
            type="button"
            class="sc-chip sc-chip-sm"
            :class="{ active: subSource === '二模' }"
            @click="subSource = '二模'; scheduleConfirm()"
          >二模</button>
        </div>
      </div>
      <div v-if="createPaper" class="sc-field sc-field-wide">
        <label>关联已有试卷</label>
        <AppSelect
          :model-value="paperForm.paperId || undefined"
          :options="paperSelectOptions"
          placeholder="不关联，新建"
          clearable
          @update:model-value="(v) => { paperForm.paperId = v ?? '' }"
        />
        <span class="sc-field-help">选择已有试卷后，不会重复创建</span>
      </div>
    </div>

    <div v-else class="sc-material-card">
      <label for="sc-material-title">资料名称 <span>选填</span></label>
      <input
        id="sc-material-title"
        v-model="title"
        class="sc-title-input"
        type="text"
        placeholder="为这批题目添加一个名称"
      />
    </div>
  </div>
</template>

<style scoped>
.source-cascade {
  container-type: inline-size;
  display: flex;
  flex-direction: column;
  gap: 16px;
  padding: 18px;
  color: var(--text-primary, #1d1d1f);
  background: color-mix(in srgb, var(--bg-card, #fff) 96%, transparent);
  border: 1px solid color-mix(in srgb, var(--text-primary, #1d1d1f) 8%, transparent);
  border-radius: 18px;
  box-shadow:
    0 1px 2px rgba(0, 0, 0, 0.03),
    0 10px 30px rgba(0, 0, 0, 0.045);
  font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif;
}

.source-cascade.is-panel {
  flex: 1;
  min-height: 0;
  padding: 4px 2px 12px;
  overflow: auto;
  background: transparent;
  border: 0;
  border-radius: 0;
  box-shadow: none;
}

.source-cascade.is-panel .sc-paper-card {
  border-bottom-left-radius: 0;
  border-bottom-right-radius: 0;
}

.source-cascade.is-panel .sc-form {
  margin-top: 0;
  border-top: 1px solid color-mix(in srgb, var(--text-primary, #1d1d1f) 7%, transparent);
  border-top-left-radius: 0;
  border-top-right-radius: 0;
}

.sc-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}

.sc-header h3,
.sc-form-heading h4 {
  margin: 0;
  color: var(--text-primary, #1d1d1f);
  font-size: 15px;
  font-weight: 650;
  letter-spacing: -0.01em;
}

.sc-header p,
.sc-form-heading p {
  margin: 3px 0 0;
  font-size: 12px;
  line-height: 1.4;
  color: var(--text-secondary, #6e6e73);
}

.sc-section {
  padding: 12px;
  background: var(--bg-muted, #f5f5f7);
  border-radius: 13px;
}

.sc-section-plain {
  padding: 0;
  background: transparent;
}

.sc-section-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 10px;
  margin-bottom: 9px;
}

.sc-section-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-primary, #1d1d1f);
}

.sc-section-hint {
  font-size: 11px;
  color: var(--text-tertiary, #86868b);
}

.sc-cats, .sc-kinds {
  display: flex;
  flex-wrap: wrap;
  gap: 7px;
}

.sc-segmented {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 3px;
  padding: 3px;
  background: color-mix(in srgb, var(--text-primary, #1d1d1f) 7%, transparent);
  border-radius: 10px;
}

.sc-chip {
  min-height: 32px;
  border: 1px solid color-mix(in srgb, var(--text-primary, #1d1d1f) 8%, transparent);
  background: var(--bg-card, #fff);
  border-radius: 999px;
  padding: 6px 13px;
  font-size: 13px;
  font-weight: 500;
  line-height: 1;
  cursor: pointer;
  color: var(--text-primary, #1d1d1f);
  transition: color 0.18s ease, background 0.18s ease, border-color 0.18s ease, transform 0.12s ease;
  -webkit-tap-highlight-color: transparent;
}

.sc-chip:hover:not(.active) {
  background: color-mix(in srgb, var(--bg-card, #fff) 75%, var(--text-primary, #1d1d1f) 4%);
}

.sc-chip:active {
  transform: scale(0.97);
}

.sc-segmented .sc-chip {
  width: 100%;
  border: 0;
  border-radius: 8px;
  background: transparent;
  box-shadow: none;
}

.sc-chip-sm {
  min-height: 30px;
  padding: 5px 11px;
  font-size: 12px;
}

.sc-chip.active {
  background: var(--accent, #0071e3);
  border-color: var(--accent, #0071e3);
  color: #fff;
  box-shadow: 0 2px 7px color-mix(in srgb, var(--accent, #0071e3) 24%, transparent);
}

.sc-segmented .sc-chip.active {
  color: var(--text-primary, #1d1d1f);
  background: var(--bg-card, #fff);
  box-shadow:
    0 1px 2px rgba(0, 0, 0, 0.08),
    0 2px 8px rgba(0, 0, 0, 0.06);
}

.sc-paper-card,
.sc-material-card {
  overflow: visible;
  background: var(--bg-muted, #f5f5f7);
  border-radius: 13px;
}

.sc-paper-main {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px 16px;
  padding: 13px 14px;
}

.sc-paper-copy {
  display: flex;
  min-width: 0;
  flex: 1;
  flex-direction: column;
  gap: 4px;
}

.sc-paper-title {
  font-size: 13px;
  font-weight: 600;
  line-height: 1.35;
  color: var(--text-primary, #1d1d1f);
}

.sc-paper-desc {
  font-size: 11px;
  line-height: 1.45;
  color: var(--text-secondary, #6e6e73);
  white-space: normal;
}

.sc-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(100%, 188px), 1fr));
  gap: 12px 14px;
}

.sc-field-wide {
  grid-column: 1 / -1;
}

.sc-switch {
  position: relative;
  display: inline-flex;
  align-items: center;
  cursor: pointer;
  flex-shrink: 0;
}

.sc-switch input {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  opacity: 0;
}

.sc-switch-track {
  position: relative;
  display: block;
  width: 42px;
  height: 25px;
  padding: 2px;
  background: #d1d1d6;
  border-radius: 999px;
  transition: background 0.2s ease;
}

.sc-switch-thumb {
  display: block;
  width: 21px;
  height: 21px;
  background: #fff;
  border-radius: 50%;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.28);
  transition: transform 0.2s cubic-bezier(0.22, 1, 0.36, 1);
}

.sc-switch input:checked + .sc-switch-track {
  background: #34c759;
}

.sc-switch input:checked + .sc-switch-track .sc-switch-thumb {
  transform: translateX(17px);
}

.sc-switch input:focus-visible + .sc-switch-track {
  outline: 3px solid color-mix(in srgb, var(--accent, #0071e3) 24%, transparent);
  outline-offset: 2px;
}

.sc-detail-toggle {
  display: flex;
  width: 100%;
  min-height: 40px;
  align-items: center;
  gap: 7px;
  padding: 0 14px;
  border: 0;
  border-top: 1px solid color-mix(in srgb, var(--text-primary, #1d1d1f) 7%, transparent);
  color: var(--accent, #0071e3);
  background: transparent;
  font-size: 12px;
  text-align: left;
  cursor: pointer;
}

.sc-detail-toggle > span:first-child {
  flex: 1;
}

.sc-detail-toggle .app-icon {
  transition: transform 0.2s ease;
}

.sc-detail-toggle .app-icon.is-open {
  transform: rotate(180deg);
}

.sc-optional {
  padding: 2px 6px;
  color: var(--text-tertiary, #86868b);
  background: color-mix(in srgb, var(--text-primary, #1d1d1f) 6%, transparent);
  border-radius: 5px;
  font-size: 10px;
}

.sc-form {
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 15px;
  background: var(--bg-muted, #f5f5f7);
  border-radius: 13px;
}

.sc-form-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.sc-field {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 6px;
  font-size: 12px;
}

.sc-field label,
.sc-material-card label {
  color: var(--text-secondary, #6e6e73);
  font-size: 11px;
  font-weight: 500;
}

.sc-field .req {
  color: #ff3b30;
}

.sc-field input,
.sc-field select,
.sc-title-input {
  width: 100%;
  min-width: 0;
  height: 38px;
  box-sizing: border-box;
  border: 1px solid color-mix(in srgb, var(--text-primary, #1d1d1f) 9%, transparent);
  border-radius: 9px;
  padding: 0 10px;
  color: var(--text-primary, #1d1d1f);
  font-size: 13px;
  line-height: 38px;
  background: var(--bg-card, #fff);
  outline: none;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.025);
  transition: border-color 0.18s ease, box-shadow 0.18s ease, background 0.18s ease;
}

.sc-field select:disabled {
  color: var(--text-tertiary, #86868b);
  background: color-mix(in srgb, var(--bg-muted, #f5f5f7) 80%, var(--bg-card, #fff));
  cursor: not-allowed;
  opacity: 0.85;
}

.sc-field input:focus,
.sc-field select:focus,
.sc-title-input:focus {
  border-color: var(--accent, #0071e3);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent, #0071e3) 16%, transparent);
}

.sc-field input::placeholder,
.sc-title-input::placeholder {
  color: var(--text-tertiary, #86868b);
  opacity: 0.72;
}

.sc-field-help {
  color: var(--text-tertiary, #86868b);
  font-size: 10px;
  line-height: 1.4;
}

.sc-material-card {
  display: flex;
  flex-direction: column;
  gap: 7px;
  padding: 13px 14px 14px;
}

.sc-material-card label {
  display: flex;
  justify-content: space-between;
}

.sc-material-card label span {
  color: var(--text-tertiary, #86868b);
  font-weight: 400;
}

.sc-title-input {
  display: block;
}

.sc-saving {
  margin: 0;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  flex-shrink: 0;
  padding: 5px 8px;
  color: var(--text-secondary, #6e6e73);
  background: var(--bg-muted, #f5f5f7);
  border-radius: 999px;
  font-size: 10px;
  font-weight: 500;
}

@container (max-width: 420px) {
  .source-cascade {
    padding: 12px;
    gap: 12px;
  }

  .source-cascade.is-panel {
    padding: 0 0 8px;
  }

  .sc-grid {
    grid-template-columns: 1fr;
    gap: 10px;
  }

  .sc-field-wide {
    grid-column: auto;
  }

  .sc-section {
    padding: 10px;
  }

  .sc-form {
    padding: 12px;
    gap: 12px;
  }

  .sc-section-hint {
    display: none;
  }

  .source-cascade:not(.is-panel) .sc-paper-desc {
    display: none;
  }

  .source-cascade.is-panel .sc-paper-desc {
    font-size: 10.5px;
    line-height: 1.45;
  }

  .sc-segmented {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }

  .sc-chip {
    min-height: 30px;
    padding: 5px 8px;
    font-size: 12px;
  }
}

@media (max-width: 720px) {
  .source-cascade.is-panel .sc-form,
  .source-cascade.is-panel .sc-paper-card,
  .source-cascade.is-panel .sc-material-card {
    border-radius: 12px;
  }
}

@media (prefers-reduced-motion: reduce) {
  .sc-chip,
  .sc-switch-track,
  .sc-switch-thumb,
  .sc-detail-toggle .app-icon {
    transition: none;
  }
}
</style>
