<script setup lang="ts">
/**
 * AttributeSidePanel — 录题右侧常驻属性面板（替代 AttributeModal）
 *
 * 设计要点：
 * - 320px 宽常驻右侧，Flex 纵向布局，与编辑区/预览区并排
 * - 顶部 "✨ AI 智能打标" 按钮，调用 aiTaggingApi.tag() 一键回填
 * - 知识点用 KnowledgeTreeCascader（自管数据加载）
 * - 标签分为核心素养 / 解题方法 / 学校来源 三组
 * - AI 回填字段加 --purple-light 边框高亮动画，用户手动修改后取消
 * - 严格使用 CSS 变量，复用 AppButton / AppIcon，无第三方 UI 库
 */
import { ref, reactive, computed, watch, nextTick } from 'vue'
import {
  aiTaggingApi,
  tagsApi,
  type Tag,
  type TagCategory,
  type QuestionType,
} from '@/api/client'
import { AppButton, AppIcon, AppSelect } from '@/components/ui'
import KnowledgeTreeCascader from '@/components/KnowledgeTreeCascader.vue'
import { useToast } from '@/composables/useToast'
import { useSpaceStore } from '@/stores/space'

// ─────────────────────────────────────────────────────────────────────
// v-model 绑定
// ─────────────────────────────────────────────────────────────────────
const tagIds = defineModel<string[]>('tagIds', { required: true })
const knowledgeNodeIds = defineModel<string[]>('knowledgeNodeIds', { required: true })
const aiGeneratedFields = defineModel<Set<string>>('aiGeneratedFields', { required: true })

// ─────────────────────────────────────────────────────────────────────
// Props
// ─────────────────────────────────────────────────────────────────────
const props = defineProps<{
  competenceTags: Tag[]
  methodTags: Tag[]
  schoolTags: Tag[]
  /** 父组件 form 的引用，用于 AI 打标时读取题干文本与回填字段 */
  form: {
    stem: string
    question_type: string
    sub_type: string
    difficulty: string
    difficulty_coefficient: number
    academic_year: string
    grade_semester: string
    exam_type: string
    exam_region: string
    options: { label: string; content: string }[]
    sub_answers: string[]
    solutions: string[]
  }
}>()

const toast = useToast()
const space = useSpaceStore()

// ─────────────────────────────────────────────────────────────────────
// 标签分类与限额
// ─────────────────────────────────────────────────────────────────────
const TAG_LIMITS: Record<TagCategory, number> = {
  core_competence: 3,
  method: 5,
  school: 1,
  scene: 3,
  error_prone: 2,
}

const allTagsMap = computed(() => {
  const m = new Map<string, Tag>()
  for (const t of props.methodTags) m.set(t.id, t)
  for (const t of props.competenceTags) m.set(t.id, t)
  for (const t of props.schoolTags) m.set(t.id, t)
  return m
})

const selectedTagsList = computed(() =>
  tagIds.value
    .map((id) => allTagsMap.value.get(id))
    .filter((t): t is Tag => !!t),
)

const selectedCompetenceTags = computed(() =>
  selectedTagsList.value.filter((t) => t.category === 'core_competence'),
)
const selectedMethodTags = computed(() =>
  selectedTagsList.value.filter((t) => t.category === 'method'),
)
const selectedSchoolTags = computed(() =>
  selectedTagsList.value.filter((t) => t.category === 'school'),
)

const topMethods = computed(() =>
  [...props.methodTags].sort((a, b) => b.use_count - a.use_count).slice(0, 8),
)
const topSchools = computed(() =>
  [...props.schoolTags].sort((a, b) => b.use_count - a.use_count).slice(0, 8),
)

function toggleTag(tag: Tag) {
  const idx = tagIds.value.indexOf(tag.id)
  if (idx >= 0) {
    tagIds.value.splice(idx, 1)
    return
  }
  const count = selectedTagsList.value.filter((t) => t.category === tag.category).length
  const limit = TAG_LIMITS[tag.category] ?? 99
  if (count >= limit) {
    toast.warning('已达到该类别最大可选择上限')
    return
  }
  tagIds.value.push(tag.id)
}

// ─────────────────────────────────────────────────────────────────────
// 标签搜索 / 创建
// ─────────────────────────────────────────────────────────────────────
interface SuggestState {
  query: string
  results: Tag[]
  loading: boolean
  timer: ReturnType<typeof setTimeout> | null
}

const suggestMethod = reactive<SuggestState>({ query: '', results: [], loading: false, timer: null })
const suggestSchool = reactive<SuggestState>({ query: '', results: [], loading: false, timer: null })

function onSuggestInput(state: SuggestState, category: TagCategory) {
  if (state.timer) clearTimeout(state.timer)
  const q = state.query.trim()
  if (!q) {
    state.results = []
    return
  }
  state.timer = setTimeout(async () => {
    state.loading = true
    try {
      const res = await tagsApi.suggest(q, category)
      state.results = res.data
    } catch {
      state.results = []
    } finally {
      state.loading = false
    }
  }, 200)
}

async function createNewTag(name: string, category: TagCategory, state: SuggestState) {
  try {
    const res = await tagsApi.create({ name, category })
    tagIds.value.push(res.data.id)
    toast.success(`已创建并选中标签「${name}」`)
    state.query = ''
    state.results = []
  } catch (e: any) {
    toast.error(e.response?.data?.error || '创建标签失败')
  }
}

// ─────────────────────────────────────────────────────────────────────
// 基础属性选项（题型 / 学年 / 年级学期 / 考试类型）
// ─────────────────────────────────────────────────────────────────────
const typeOptions = [
  { label: '选择题', value: 'choice' },
  { label: '填空题', value: 'fill' },
  { label: '解答题', value: 'solution' },
]

const currentYear = new Date().getFullYear()
const academicYearOptions = [
  { label: `${currentYear - 1}-${String(currentYear).slice(2)}`, value: `${currentYear - 1}-${String(currentYear).slice(2)}` },
  { label: `${currentYear}-${String(currentYear + 1).slice(2)}`, value: `${currentYear}-${String(currentYear + 1).slice(2)}` },
  { label: `${currentYear + 1}-${String(currentYear + 2).slice(2)}`, value: `${currentYear + 1}-${String(currentYear + 2).slice(2)}` },
]

const gradeSemesterOptions = [
  ...['初一', '初二', '初三'].flatMap(g => [
    { label: `${g}上`, value: `${g}上` },
    { label: `${g}下`, value: `${g}下` },
  ]),
  ...['高一', '高二', '高三'].flatMap(g => [
    { label: `${g}上`, value: `${g}上` },
    { label: `${g}下`, value: `${g}下` },
  ]),
]

const examTypeOptions = [
  { label: '期末', value: '期末' },
  { label: '期中', value: '期中' },
  { label: '月考', value: '月考' },
  { label: '周测', value: '周测' },
  { label: '模拟', value: '模拟' },
  { label: '高考', value: '高考' },
  { label: '中考', value: '中考' },
  { label: '竞赛', value: '竞赛' },
]

// 难度星级：1-5 星 ↔ easy/medium/hard + difficulty_coefficient
const difficultyStars = computed<number>({
  get: () => {
    if (props.form.difficulty === 'easy') return props.form.difficulty_coefficient > 0.8 ? 1 : 2
    if (props.form.difficulty === 'medium') return 3
    return props.form.difficulty_coefficient < 0.3 ? 5 : 4
  },
  set: (v: number) => {
    props.form.difficulty_coefficient = [0.9, 0.75, 0.55, 0.35, 0.2][v - 1] ?? 0.55
    props.form.difficulty = v <= 2 ? 'easy' : v === 3 ? 'medium' : 'hard'
    clearFieldHighlight('difficulty')
  },
})

// ─────────────────────────────────────────────────────────────────────
// AI 智能打标
// ─────────────────────────────────────────────────────────────────────
const aiTagging = ref(false)

/** 拼接题干 + 选项 + 答案 + 解析为完整题目文本 */
function buildTaggingContent(): string {
  const parts: string[] = [props.form.stem || '']
  if (props.form.options?.length) {
    parts.push(props.form.options.map((o) => `${o.label}. ${o.content}`).join('\n'))
  }
  if (props.form.sub_answers?.length) {
    const ans = props.form.sub_answers.filter((s) => s.trim())
    if (ans.length) parts.push('参考答案：' + ans.join('；'))
  }
  if (props.form.solutions?.length) {
    const sol = props.form.solutions.filter((s) => s.trim())
    if (sol.length) parts.push('解析：' + sol.join('\n'))
  }
  return parts.filter(Boolean).join('\n\n')
}

/** 难度数值 1-5 → form 内部使用的字符串枚举 */
function difficultyNumToString(n: number | null): string {
  if (n == null) return 'medium'
  if (n <= 2) return 'easy'
  if (n === 3) return 'medium'
  return 'hard'
}

async function runAiTagging() {
  const content = buildTaggingContent()
  if (!content.trim()) {
    toast.warning('请先输入题干内容')
    return
  }
  aiTagging.value = true
  aiTaggingInProgress = true
  try {
    const res = await aiTaggingApi.tag({
      content,
      space_id: space.currentSpaceId || undefined,
    })
    const data = res.data
    const newAiFields = new Set<string>()

    // 知识点回填
    if (data.knowledge_nodes?.length) {
      knowledgeNodeIds.value = data.knowledge_nodes.map((n) => n.node_id)
      newAiFields.add('knowledge_node')
    }

    // 难度回填
    if (data.difficulty != null) {
      props.form.difficulty = difficultyNumToString(data.difficulty)
      const diffStars = data.difficulty
      props.form.difficulty_coefficient =
        [0.9, 0.75, 0.55, 0.35, 0.2][diffStars - 1] ?? 0.55
      newAiFields.add('difficulty')
    }

    // 题型回填
    if (data.question_type) {
      props.form.question_type = data.question_type as QuestionType
      newAiFields.add('question_type')
    }

    // 年级 / 认知层次回填（暂存到 aiGeneratedFields 标记位，由父组件决定是否使用）
    if (data.grade_level) newAiFields.add(`grade_level:${data.grade_level}`)
    if (data.cognitive_level) newAiFields.add(`cognitive_level:${data.cognitive_level}`)

    if (data.unmatched_knowledge_points?.length) {
      toast.info(`AI 识别到 ${data.unmatched_knowledge_points.length} 个未匹配知识点，请手动确认`)
    }

    aiGeneratedFields.value = newAiFields
    toast.success(`AI 打标完成，已回填 ${newAiFields.size} 个字段`)
    // 等待 watch 触发完毕后再放开标记，防止清掉刚加的高亮
    await nextTick()
  } catch (e: any) {
    toast.error(e.response?.data?.error || 'AI 打标失败，请稍后重试')
  } finally {
    aiTagging.value = false
    aiTaggingInProgress = false
  }
}

// ─────────────────────────────────────────────────────────────────────
// 手动编辑 → 取消 AI 高亮
// ─────────────────────────────────────────────────────────────────────
// AI 回填进行中的标志：避免 watch 在 AI 设置字段时立刻清掉刚加的高亮
let aiTaggingInProgress = false

function clearFieldHighlight(field: string) {
  if (aiTaggingInProgress) return
  if (!aiGeneratedFields.value.has(field)) return
  const next = new Set(aiGeneratedFields.value)
  next.delete(field)
  aiGeneratedFields.value = next
}

// 知识点手动变更 → 取消高亮
watch(knowledgeNodeIds, () => {
  clearFieldHighlight('knowledge_node')
})

// 基础属性手动变更 → 取消对应高亮
watch(
  () => props.form.question_type,
  () => clearFieldHighlight('question_type'),
)
watch(
  () => props.form.academic_year,
  () => clearFieldHighlight('academic_year'),
)
watch(
  () => props.form.grade_semester,
  () => clearFieldHighlight('grade_semester'),
)
watch(
  () => props.form.exam_type,
  () => clearFieldHighlight('exam_type'),
)
watch(
  () => props.form.exam_region,
  () => clearFieldHighlight('exam_region'),
)

// 暴露给父组件：当 form 字段被用户手动修改时，可调用此方法清除对应高亮
defineExpose({ clearFieldHighlight })
</script>

<template>
  <aside class="attr-side-panel">
    <!-- ===== 顶部：标题 + AI 智能打标按钮 ===== -->
    <header class="asp-header">
      <div class="asp-title">
        <AppIcon name="sliders" :size="15" />
        <span>题目属性</span>
      </div>
      <AppButton
        variant="primary"
        size="sm"
        :loading="aiTagging"
        :disabled="aiTagging"
        @click="runAiTagging"
      >
        <AppIcon name="sparkles" :size="14" />
        <span>{{ aiTagging ? '打标中…' : 'AI 智能打标' }}</span>
      </AppButton>
    </header>

    <!-- ===== 滚动主体 ===== -->
    <div class="asp-body">
      <!-- 基础属性（题型 / 难度 / 学年 / 年级学期 / 考试类型 / 考试地区） -->
      <section class="asp-section asp-section-meta">
        <div class="asp-section-head">
          <label class="asp-label">基础属性</label>
        </div>
        <div class="asp-meta-grid">
          <!-- 题型 -->
          <div
            class="asp-meta-cell"
            :class="{ 'ai-highlight': aiGeneratedFields.has('question_type') }"
          >
            <label class="asp-meta-label">题型</label>
            <AppSelect
              :model-value="props.form.question_type"
              :options="typeOptions"
              placeholder="选择题型"
              class="asp-meta-select"
              @update:model-value="(v: string | undefined) => { props.form.question_type = v ?? ''; clearFieldHighlight('question_type') }"
            />
          </div>

          <!-- 学年 -->
          <div
            class="asp-meta-cell"
            :class="{ 'ai-highlight': aiGeneratedFields.has('academic_year') }"
          >
            <label class="asp-meta-label">学年</label>
            <AppSelect
              :model-value="props.form.academic_year || undefined"
              :options="academicYearOptions"
              placeholder="学年"
              clearable
              class="asp-meta-select"
              @update:model-value="(v: string | undefined) => { props.form.academic_year = v ?? ''; clearFieldHighlight('academic_year') }"
            />
          </div>

          <!-- 难度星级（整行） -->
          <div
            class="asp-meta-cell asp-meta-cell-full"
            :class="{ 'ai-highlight': aiGeneratedFields.has('difficulty') }"
          >
            <label class="asp-meta-label">难度</label>
            <div class="asp-diff-row">
              <button
                v-for="n in 5"
                :key="n"
                type="button"
                class="asp-star"
                :class="{ active: difficultyStars >= n }"
                @click="difficultyStars = n"
              >
                <AppIcon name="star" :size="14" />
              </button>
              <span class="asp-diff-hint">{{ difficultyStars }} 星</span>
            </div>
          </div>

          <!-- 年级学期 -->
          <div
            class="asp-meta-cell"
            :class="{ 'ai-highlight': aiGeneratedFields.has('grade_semester') }"
          >
            <label class="asp-meta-label">年级学期</label>
            <AppSelect
              :model-value="props.form.grade_semester || undefined"
              :options="gradeSemesterOptions"
              placeholder="年级学期"
              clearable
              class="asp-meta-select"
              @update:model-value="(v: string | undefined) => { props.form.grade_semester = v ?? ''; clearFieldHighlight('grade_semester') }"
            />
          </div>

          <!-- 考试类型 -->
          <div
            class="asp-meta-cell"
            :class="{ 'ai-highlight': aiGeneratedFields.has('exam_type') }"
          >
            <label class="asp-meta-label">考试类型</label>
            <AppSelect
              :model-value="props.form.exam_type || undefined"
              :options="examTypeOptions"
              placeholder="考试类型"
              clearable
              class="asp-meta-select"
              @update:model-value="(v: string | undefined) => { props.form.exam_type = v ?? ''; clearFieldHighlight('exam_type') }"
            />
          </div>

          <!-- 考试地区（整行） -->
          <div
            class="asp-meta-cell asp-meta-cell-full"
            :class="{ 'ai-highlight': aiGeneratedFields.has('exam_region') }"
          >
            <label class="asp-meta-label">考试地区</label>
            <input
              v-model="props.form.exam_region"
              placeholder="如：北京市"
              class="asp-input"
              @input="clearFieldHighlight('exam_region')"
            />
          </div>
        </div>
      </section>

      <!-- 知识点 -->
      <section
        class="asp-section"
        :class="{ 'ai-highlight': aiGeneratedFields.has('knowledge_node') }"
      >
        <div class="asp-section-head">
          <label class="asp-label">知识点</label>
          <span class="asp-counter">{{ knowledgeNodeIds.length }}/3</span>
        </div>
        <KnowledgeTreeCascader
          v-model="knowledgeNodeIds"
          :max="3"
          placeholder="选择知识点…"
        />
      </section>

      <!-- 核心素养 -->
      <section class="asp-section">
        <div class="asp-section-head">
          <label class="asp-label">核心素养</label>
          <span class="asp-counter">{{ selectedCompetenceTags.length }}/3</span>
        </div>
        <div v-if="competenceTags.length === 0" class="asp-empty">暂无可选素养</div>
        <div v-else class="asp-chip-grid">
          <button
            v-for="t in competenceTags"
            :key="t.id"
            type="button"
            class="asp-chip"
            :class="{ active: tagIds.includes(t.id) }"
            @click="toggleTag(t)"
          >
            <span v-if="tagIds.includes(t.id)" class="asp-chip-check">✓</span>
            <span>{{ t.name }}</span>
          </button>
        </div>
      </section>

      <!-- 解题方法 -->
      <section class="asp-section">
        <div class="asp-section-head">
          <label class="asp-label">解题方法</label>
          <span class="asp-counter">{{ selectedMethodTags.length }}/5</span>
        </div>
        <div class="asp-typeahead">
          <input
            v-model="suggestMethod.query"
            class="asp-input"
            placeholder="搜索或创建方法标签…"
            @input="onSuggestInput(suggestMethod, 'method')"
          />
          <div v-if="suggestMethod.results.length" class="asp-popover">
            <button
              v-for="t in suggestMethod.results"
              :key="t.id"
              type="button"
              class="asp-popover-item"
              @click="toggleTag(t); suggestMethod.query = ''; suggestMethod.results = []"
            >
              <span>{{ t.name }}</span>
              <span class="asp-popover-count">{{ t.use_count }} 次</span>
            </button>
          </div>
          <button
            v-if="suggestMethod.query.trim() && !suggestMethod.results.some(t => t.name === suggestMethod.query.trim())"
            type="button"
            class="asp-create-btn"
            @click="createNewTag(suggestMethod.query.trim(), 'method', suggestMethod)"
          >
            <AppIcon name="plus" :size="12" />
            <span>创建「{{ suggestMethod.query.trim() }}」</span>
          </button>
        </div>
        <div v-if="topMethods.length" class="asp-chip-grid">
          <button
            v-for="t in topMethods"
            :key="t.id"
            type="button"
            class="asp-chip"
            :class="{ active: tagIds.includes(t.id) }"
            @click="toggleTag(t)"
          >
            <span v-if="tagIds.includes(t.id)" class="asp-chip-check">✓</span>
            <span>{{ t.name }}</span>
          </button>
        </div>
      </section>

      <!-- 学校来源 -->
      <section class="asp-section">
        <div class="asp-section-head">
          <label class="asp-label">学校来源</label>
          <span class="asp-counter">{{ selectedSchoolTags.length }}/1</span>
        </div>
        <div class="asp-typeahead">
          <input
            v-model="suggestSchool.query"
            class="asp-input"
            placeholder="搜索或创建学校标签…"
            @input="onSuggestInput(suggestSchool, 'school')"
          />
          <div v-if="suggestSchool.results.length" class="asp-popover">
            <button
              v-for="t in suggestSchool.results"
              :key="t.id"
              type="button"
              class="asp-popover-item"
              @click="toggleTag(t); suggestSchool.query = ''; suggestSchool.results = []"
            >
              <span>{{ t.name }}</span>
              <span class="asp-popover-count">{{ t.use_count }} 次</span>
            </button>
          </div>
          <button
            v-if="suggestSchool.query.trim() && !suggestSchool.results.some(t => t.name === suggestSchool.query.trim())"
            type="button"
            class="asp-create-btn"
            @click="createNewTag(suggestSchool.query.trim(), 'school', suggestSchool)"
          >
            <AppIcon name="plus" :size="12" />
            <span>创建「{{ suggestSchool.query.trim() }}」</span>
          </button>
        </div>
        <div v-if="topSchools.length" class="asp-chip-grid">
          <button
            v-for="t in topSchools"
            :key="t.id"
            type="button"
            class="asp-chip"
            :class="{ active: tagIds.includes(t.id) }"
            @click="toggleTag(t)"
          >
            <span v-if="tagIds.includes(t.id)" class="asp-chip-check">✓</span>
            <span>{{ t.name }}</span>
          </button>
        </div>
      </section>
    </div>
  </aside>
</template>

<style scoped>
/* ===== 容器：320px 常驻右侧 ===== */
.attr-side-panel {
  width: 320px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-card);
  border-radius: var(--radius-lg);
  border: 1px solid var(--border-color);
  box-shadow: var(--shadow-sm);
  overflow: hidden;
  height: 100%;
}

[data-theme='dark'] .attr-side-panel {
  border-color: #3a3a3c;
  box-shadow: none;
}

/* ===== 顶部 ===== */
.asp-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 16px;
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
  gap: 8px;
}

.asp-title {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 13.5px;
  font-weight: 650;
  color: var(--text-primary);
  letter-spacing: -0.01em;
}

.asp-title :deep(.app-icon) {
  color: var(--text-secondary);
}

/* ===== 滚动主体 ===== */
.asp-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 20px;
}

/* ===== 区块 ===== */
.asp-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 8px;
  border-radius: var(--radius-md);
  transition: box-shadow 0.4s ease;
}

/* ===== 基础属性区块：两列紧凑栅格 ===== */
.asp-section-meta {
  padding: 4px 8px 4px;
  border-bottom: 1px solid var(--border-color);
  padding-bottom: 16px;
}

.asp-meta-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px 8px;
}

.asp-meta-cell {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
  padding: 6px 8px;
  border-radius: var(--radius-sm);
  border: 1px solid transparent;
  transition: box-shadow 0.4s ease, border-color 0.2s ease;
}

.asp-meta-cell-full {
  grid-column: 1 / -1;
}

.asp-meta-label {
  font-size: 11px;
  font-weight: 600;
  color: var(--text-muted);
  letter-spacing: 0.02em;
}

.asp-meta-select {
  width: 100%;
}

/* 让 AppSelect 在 cell 内宽度填满 */
.asp-meta-cell :deep(.app-select-wrapper) {
  width: 100%;
  min-width: 0;
}

.asp-meta-cell :deep(.app-select-trigger) {
  width: 100%;
  min-width: 0;
}

/* 难度星级 */
.asp-diff-row {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  height: 28px;
}

.asp-star {
  color: var(--border-strong, #d1d1d6);
  background: none;
  border: none;
  cursor: pointer;
  padding: 4px;
  display: inline-flex;
  transition: transform 0.15s ease, color 0.2s ease;
}

.asp-star :deep(svg),
.asp-star svg {
  pointer-events: none;
}

.asp-star:hover {
  transform: scale(1.15);
}

.asp-star.active {
  color: var(--star-color, #ff9500);
}

.asp-star.active :deep(svg),
.asp-star.active svg {
  color: var(--star-color, #ff9500) !important;
}

.asp-diff-hint {
  margin-left: 8px;
  font-size: 11px;
  color: var(--text-muted);
  font-weight: 500;
}

.asp-section-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.asp-label {
  font-size: 12.5px;
  font-weight: 650;
  color: var(--text-secondary);
  letter-spacing: 0.01em;
}

.asp-counter {
  font-size: 11px;
  color: var(--text-muted);
  font-weight: 500;
}

.asp-empty {
  font-size: 12px;
  color: var(--text-muted);
  padding: 6px 0;
}

/* ===== 输入框 ===== */
.asp-input {
  width: 100%;
  height: 32px;
  padding: 0 10px;
  border-radius: 8px;
  border: 1px solid var(--border-color);
  background: var(--bg-input);
  color: var(--text-primary);
  font-size: 12.5px;
  outline: none;
  box-sizing: border-box;
  transition: border-color 0.2s;
}

.asp-input:focus {
  border-color: var(--accent);
  background: var(--bg-card);
}

.asp-input::placeholder {
  color: var(--text-muted);
}

/* ===== Typeahead Popover ===== */
.asp-typeahead {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.asp-popover {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  right: 0;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: 8px;
  box-shadow: var(--shadow-md);
  z-index: 50;
  max-height: 200px;
  overflow-y: auto;
  padding: 4px;
}

.asp-popover-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
  padding: 7px 10px;
  border: none;
  background: transparent;
  border-radius: 6px;
  font-size: 12.5px;
  color: var(--text-primary);
  cursor: pointer;
  transition: background 0.15s;
  text-align: left;
}

.asp-popover-item:hover {
  background: var(--bg-hover);
}

.asp-popover-count {
  font-size: 11px;
  color: var(--text-muted);
}

.asp-create-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 5px 10px;
  border: 1px dashed var(--accent);
  border-radius: 6px;
  background: var(--accent-light);
  color: var(--accent);
  font-size: 11.5px;
  font-weight: 600;
  cursor: pointer;
  align-self: flex-start;
  transition: all 0.2s;
}

.asp-create-btn:hover {
  background: var(--accent);
  color: #fff;
}

/* ===== 标签 Chip 网格 ===== */
.asp-chip-grid {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.asp-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 4px 10px;
  border-radius: 9999px;
  border: 1px solid var(--border-color);
  background: var(--bg-input);
  color: var(--text-secondary);
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.18s ease;
  white-space: nowrap;
}

.asp-chip:hover {
  border-color: var(--accent);
  color: var(--accent);
  background: var(--accent-light);
}

.asp-chip.active {
  background: var(--accent);
  border-color: var(--accent);
  color: #fff;
  font-weight: 600;
}

.asp-chip-check {
  font-size: 10px;
  font-weight: 700;
  display: inline-flex;
  align-items: center;
}

/* ===== AI 高亮动画（与 QuestionEdit.vue 的 .ai-highlight 一致） ===== */
@keyframes asp-ai-breathe {
  0%, 100% {
    box-shadow: 0 0 0 2px var(--purple);
  }
  50% {
    box-shadow: 0 0 8px 2px var(--purple-light);
  }
}

.asp-section.ai-highlight,
.asp-meta-cell.ai-highlight {
  animation: asp-ai-breathe 2s ease-in-out infinite;
  border-radius: var(--radius-sm);
}

/* ===== 让 KnowledgeTreeCascader 宽度填满 ===== */
.asp-section :deep(.cascader-trigger) {
  width: 100%;
}

/* ===== 移动端：宽度自适应，但仍保持纵向栈 ===== */
@media (max-width: 1100px) {
  .attr-side-panel {
    width: 280px;
  }
}

@media (max-width: 900px) {
  .attr-side-panel {
    width: 100%;
    height: auto;
    max-height: 480px;
  }
}
</style>
