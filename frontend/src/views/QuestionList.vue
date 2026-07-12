<template>
  <div class="ql-page">
    <!-- ===== 固定顶栏（不随列表滚动） ===== -->
    <div class="ql-fixed-header">
      <!-- 顶栏：题库空间 + 标题 + 新建按钮 -->
      <div class="ql-topbar">
        <h1 class="page-title ql-title"><AppIcon name="file-text" :size="20" /> 题目管理</h1>
        <div class="header-actions">
          <button
            v-if="basket.count.value > 0"
            class="basket-btn"
            @click="toast.info(`试题篮中有 ${basket.count.value} 道题目`)"
          >
            <AppIcon name="shopping-cart" :size="16" />
            <span>试题篮</span>
            <span class="basket-count">{{ basket.count.value }}</span>
          </button>
          <AppButton variant="primary" @click="$router.push('/questions/new')">
            <AppIcon name="plus" :size="17" /> 新建题目
          </AppButton>
        </div>
      </div>

      <!-- 搜索框 + 蓝色搜索按钮 -->
      <div class="ql-search-row">
        <div class="ql-search-box">
          <AppIcon name="search" :size="16" class="ql-search-icon" />
          <input
            v-model="query.keyword"
            class="ql-search-input"
            placeholder="请输入您的搜索词"
            @keydown.enter="onSearchSubmit"
          />
        </div>
        <button class="ql-search-btn" @click="onSearchSubmit">搜索</button>
      </div>

      <!-- 标签平铺筛选面板 -->
      <div class="ql-filter-panel">
        <!-- 年级 -->
        <div class="ql-filter-row">
          <span class="ql-filter-label">年级</span>
          <div class="ql-filter-tags">
            <button
              v-for="opt in gradeOptions"
              :key="opt.value"
              class="ql-tag"
              :class="{ active: !query.grade && opt.value === '__all' || query.grade === opt.value }"
              @click="selectTag('grade', opt.value)"
            >{{ opt.label }}</button>
          </div>
        </div>

        <!-- 题型 -->
        <div class="ql-filter-row">
          <span class="ql-filter-label">题型</span>
          <div class="ql-filter-tags">
            <button
              v-for="opt in typeOptions"
              :key="opt.value"
              class="ql-tag"
              :class="{ active: !query.question_type && opt.value === '__all' || query.question_type === opt.value }"
              @click="selectTag('question_type', opt.value)"
            >{{ opt.label }}</button>
          </div>
        </div>

        <!-- 难度 -->
        <div class="ql-filter-row">
          <span class="ql-filter-label">难度</span>
          <div class="ql-filter-tags">
            <button
              v-for="opt in difficultyOptions"
              :key="opt.value"
              class="ql-tag"
              :class="{ active: !query.difficulty && opt.value === '__all' || query.difficulty === opt.value }"
              @click="selectTag('difficulty', opt.value)"
            >{{ opt.label }}</button>
          </div>
        </div>

        <!-- 状态 -->
        <div class="ql-filter-row">
          <span class="ql-filter-label">状态</span>
          <div class="ql-filter-tags">
            <button
              v-for="opt in statusOptions"
              :key="opt.value"
              class="ql-tag"
              :class="{ active: !query.status && opt.value === '__all' || query.status === opt.value }"
              @click="selectTag('status', opt.value)"
            >{{ opt.label }}</button>
          </div>
        </div>
      </div>

      <!-- 知识点筛选 chip -->
      <div v-if="selectedKpId" class="kp-filter-chip">
        <AppIcon name="tag" :size="14" />
        <span>{{ selectedKpName }}</span>
        <button class="chip-clear" @click="clearKp"><AppIcon name="x" :size="13" /></button>
      </div>
    </div>

    <!-- ===== 可滚动列表区域 ===== -->
    <div class="ql-scroll-area">
      <div v-if="loading" class="loading-hint">加载中…</div>

      <template v-else>
        <AppEmpty v-if="cardList.length === 0" description="没有找到匹配的题目" />

        <!-- ===== 题目卡片列表 ===== -->
        <div class="q-card-list">
          <div
            v-for="card in cardList"
            :key="card.id"
            class="q-card"
            :class="{ 'is-expanded': expandedIds.has(card.id) }"
          >
            <!-- Row 1: Header — 来源 / 题型 / 难度 / 状态 -->
            <div class="q-card-header">
              <div class="q-card-tags">
                <span v-if="card.source" class="q-source">
                  <AppIcon name="bookmark" :size="12" :stroke="2" />
                  {{ card.source }}
                </span>
                <span class="q-tag" :class="`q-tag--${card.question_type}`">
                  {{ typeLabel(card.question_type) }}
                </span>
                <span class="q-tag" :class="`q-tag--${card.difficulty}`">
                  {{ diffLabel(card.difficulty) }}
                </span>
                <span class="q-tag q-tag--neutral">
                  <AppIcon :name="statusIcon(card.status)" :size="11" :stroke="2" />
                  {{ statusLabel(card.status) }}
                </span>
              </div>
              <span class="q-card-time">{{ formatTime(card.updated_at) }}</span>
            </div>

            <!-- Row 2: Body — 题干 + 选项 -->
            <div class="q-card-body" @click="goDetail(card)">
              <div class="q-stem">
                <LatexRender :text="card.stem" />
              </div>
              <!-- 选择题选项（列表页不标注正确答案） -->
              <div v-if="card.question_type === 'choice' && card.parsedOptions.length > 0" class="q-options">
                <div
                  v-for="opt in card.parsedOptions"
                  :key="opt.label"
                  class="q-option"
                >
                  <span class="q-option-label">{{ opt.label }}</span>
                  <LatexRender :text="opt.content" :inline="true" />
                </div>
              </div>
            </div>

            <!-- 展开解析区域 -->
            <Transition name="q-analysis">
              <div v-if="expandedIds.has(card.id)" class="q-analysis-section">
                <div class="q-analysis-title">
                  <AppIcon name="lightbulb" :size="14" :stroke="2" />
                  <span>答案解析</span>
                </div>

                <!-- 正确答案高亮卡片（选择题 / 填空题） -->
                <div
                  v-if="card.correctAnswer && (card.question_type === 'choice' || card.question_type === 'fill')"
                  class="q-answer-card"
                  :class="`q-answer-card--${card.question_type}`"
                >
                  <span class="q-answer-card-label">正确答案</span>
                  <span class="q-answer-card-value">{{ card.correctAnswer }}</span>
                  <AppIcon
                    name="check-circle"
                    :size="16"
                    :stroke="2.2"
                    class="q-answer-card-icon"
                  />
                </div>

                <!-- 解答题正确答案 -->
                <div
                  v-if="card.correctAnswer && card.question_type === 'solution'"
                  class="q-answer-inline"
                >
                  <span class="q-answer-inline-label">参考答案</span>
                  <span class="q-answer-inline-value">{{ card.correctAnswer }}</span>
                </div>

                <div v-if="card.analysis" class="q-analysis-body">
                  <LatexRender :text="card.analysis" />
                </div>
                <div v-else class="q-analysis-empty">暂无解析内容</div>
              </div>
            </Transition>

            <!-- Row 3: Footer — 知识点 + 操作按钮 -->
            <div class="q-card-footer">
              <div class="q-kps">
                <span class="q-kps-label">
                  <AppIcon name="tag" :size="12" :stroke="2" />
                  知识点
                </span>
                <span
                  v-for="kp in card.knowledgePoints"
                  :key="kp.id"
                  class="q-kp-chip"
                >
                  {{ kp.name }}
                </span>
                <span v-if="card.knowledgePoints.length === 0" class="q-kp-empty">未关联</span>
                <span v-if="card.grade" class="q-grade-chip">{{ card.grade }}</span>
              </div>
              <div class="q-actions">
                <button class="q-action-btn q-action--ghost" @click="toggleAnalysis(card.id)">
                  <AppIcon
                    :name="expandedIds.has(card.id) ? 'chevron-up' : 'lightbulb'"
                    :size="14"
                    :stroke="2"
                  />
                  {{ expandedIds.has(card.id) ? '收起解析' : '答案解析' }}
                </button>
                <button
                  class="q-action-btn"
                  :class="{ 'q-action--active': basket.isInBasket(card.id) }"
                  @click="toggleBasket(card.id)"
                >
                  <AppIcon
                    :name="basket.isInBasket(card.id) ? 'check' : 'plus'"
                    :size="14"
                    :stroke="2.5"
                  />
                  {{ basket.isInBasket(card.id) ? '已加入' : '加入试题篮' }}
                </button>
              </div>
            </div>
          </div>
        </div>

        <AppPagination
          v-if="cardList.length > 0"
          :page="page"
          :has-more="hasMore"
          @update:page="onPageChange"
        />
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, onMounted, onBeforeUnmount, watch } from 'vue'
import { useRouter } from 'vue-router'
import { questionApi, type QuestionSummary, type QuestionDetail, type QuestionQuery } from '@/api/client'
import LatexRender from '@/components/LatexRender.vue'
import { AppButton, AppSelect, AppEmpty, AppPagination, AppIcon } from '@/components/ui'
import { useSelectedKp } from '@/composables/useSelectedKp'
import { useQuestionBasket } from '@/composables/useQuestionBasket'
import { useToast } from '@/composables/useToast'
import { useSpaceStore } from '@/stores/space'
import {
  typeLabel,
  diffLabel,
  statusLabel,
  statusIcon,
  formatTime,
} from '@/utils/questionDisplay'

const router = useRouter()
const toast = useToast()
const space = useSpaceStore()
const { selectedKpId, selectedKpName, clear } = useSelectedKp()
const basket = useQuestionBasket()

// ---- 卡片数据类型 ----
interface QuestionCard {
  id: string
  stem: string
  question_type: string
  difficulty: string
  status: string
  grade: string | null
  source: string | null
  updated_at: string
  version: number
  parsedOptions: { label: string; content: string }[]
  correctAnswer: string
  analysis: string | null
  knowledgePoints: { id: string; name: string }[]
}

const cardList = ref<QuestionCard[]>([])
const loading = ref(false)
const page = ref(1)
const pageSize = 20
const hasMore = ref(false)
const expandedIds = ref<Set<string>>(new Set())

const query = reactive<QuestionQuery>({
  keyword: '',
  question_type: undefined,
  difficulty: undefined,
  status: undefined,
  grade: undefined,
  knowledge_point_id: selectedKpId.value ?? undefined,
  space_id: space.currentSpaceId || undefined,
  page: 1,
  page_size: pageSize,
})

const typeOptions = [
  { label: '不限', value: '__all' },
  { label: '选择题', value: 'choice' },
  { label: '填空题', value: 'fill' },
  { label: '解答题', value: 'solution' },
  { label: '判断题', value: 'judgment' },
]

const difficultyOptions = [
  { label: '不限', value: '__all' },
  { label: '简单', value: 'easy' },
  { label: '中等', value: 'medium' },
  { label: '困难', value: 'hard' },
]

const statusOptions = [
  { label: '不限', value: '__all' },
  { label: '草稿', value: 'draft' },
  { label: '待审核', value: 'pending' },
  { label: '驳回', value: 'rejected' },
  { label: '已发布', value: 'published' },
  { label: '已停用', value: 'disabled' },
]

const gradeOptions = [
  { label: '不限', value: '__all' },
  ...['初一', '初二', '初三', '高一', '高二', '高三'].map((g) => ({ label: g, value: g })),
]

let searchTimer: ReturnType<typeof setTimeout> | null = null

function onSearchInput() {
  if (searchTimer) clearTimeout(searchTimer)
  searchTimer = setTimeout(() => {
    page.value = 1
    fetchList()
  }, 300)
}

function onSearchSubmit() {
  page.value = 1
  fetchList()
}

function selectTag(field: 'grade' | 'question_type' | 'difficulty' | 'status', value: string) {
  if (value === '__all') {
    ;(query as any)[field] = undefined
  } else {
    ;(query as any)[field] = value
  }
  page.value = 1
  fetchList()
}

function onFilterChange() {
  page.value = 1
  fetchList()
}

function onPageChange(p: number) {
  page.value = p
  fetchList()
}

// ---- 工具函数：解析选项 ----
function parseOptions(raw: any): { label: string; content: string }[] {
  if (!raw) return []
  let opts = raw
  if (typeof opts === 'string') {
    try { opts = JSON.parse(opts) } catch { return [] }
  }
  if (!Array.isArray(opts)) return []
  return opts.map((opt: any) => {
    if (typeof opt === 'string') {
      const match = opt.match(/^([A-Z])[.、．]\s*(.*)$/)
      if (match) return { label: match[1], content: match[2] }
      return { label: '', content: opt }
    }
    if (opt && typeof opt === 'object' && opt.label) {
      return { label: opt.label, content: opt.content || '' }
    }
    return { label: '', content: String(opt) }
  })
}

// ---- 工具函数：解析正确答案 ----
function extractAnswerItem(item: any): string {
  if (typeof item === 'string') return item
  if (item && typeof item === 'object') {
    if (item.answer) return item.answer
    if (item.value) return item.value
    if (item.text) return item.text
  }
  return String(item)
}

function parseAnswer(raw: any): string {
  if (raw == null) return ''
  if (typeof raw === 'string') {
    try {
      const parsed = JSON.parse(raw)
      if (typeof parsed === 'string') return parsed
      if (Array.isArray(parsed)) return parsed.map(extractAnswerItem).join(', ')
      if (typeof parsed === 'object') return extractAnswerItem(parsed)
      return String(parsed)
    } catch {
      return raw
    }
  }
  if (Array.isArray(raw)) return raw.map(extractAnswerItem).join(', ')
  if (typeof raw === 'object') return extractAnswerItem(raw)
  return String(raw)
}

function isCorrectOption(card: QuestionCard, label: string): boolean {
  const ans = card.correctAnswer
  if (!ans) return false
  return ans.split(/[,，、\s]+/).includes(label)
}

// ---- 获取列表 + 批量获取详情 ----
async function fetchList() {
  loading.value = true
  try {
    query.page = page.value
    query.page_size = pageSize
    const res = await questionApi.list(query)
    const summaries: QuestionSummary[] = res.data
    hasMore.value = summaries.length >= pageSize

    // 并发获取每道题的详情
    const details = await Promise.all(
      summaries.map((s) => questionApi.get(s.id).catch(() => null))
    )

    cardList.value = summaries.map((s, i) => {
      const detail: QuestionDetail | null = details[i]?.data ?? null
      return {
        id: s.id,
        stem: s.stem,
        question_type: s.question_type,
        difficulty: s.difficulty,
        status: s.status,
        grade: s.grade,
        source: detail?.source ?? null,
        updated_at: s.updated_at,
        version: s.version,
        parsedOptions: parseOptions(detail?.options),
        correctAnswer: parseAnswer(detail?.correct_answer),
        analysis: detail?.analysis ?? null,
        knowledgePoints: detail?.knowledge_points ?? [],
      }
    })
  } catch {
    /* handled by interceptor */
  } finally {
    loading.value = false
  }
}

function goDetail(row: QuestionSummary) {
  router.push(`/questions/${row.id}`)
}

function clearKp() {
  clear()
}

function toggleAnalysis(id: string) {
  const next = new Set(expandedIds.value)
  if (next.has(id)) {
    next.delete(id)
  } else {
    next.add(id)
  }
  expandedIds.value = next
}

function toggleBasket(id: string) {
  if (basket.isInBasket(id)) {
    basket.remove(id)
    toast.info('已从试题篮中移除')
  } else {
    basket.add(id)
    toast.success('已加入试题篮')
  }
}

watch(selectedKpId, (id) => {
  query.knowledge_point_id = id ?? undefined
  page.value = 1
  fetchList()
})

watch(() => space.currentSpaceId, (newId) => {
  query.space_id = newId || undefined
  page.value = 1
  fetchList()
})

onMounted(fetchList)
onBeforeUnmount(() => {
  if (searchTimer) clearTimeout(searchTimer)
})
</script>

<style scoped>
/* ===== Page Layout: fixed header + scroll area ===== */
.ql-page {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.ql-fixed-header {
  flex-shrink: 0;
  padding-bottom: 12px;
  border-bottom: 1px solid var(--border-color);
  background: var(--bg-primary);
  z-index: 10;
}

.ql-topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 14px;
}

.ql-title {
  margin-bottom: 0;
}

/* ===== Search Row ===== */
.ql-search-row {
  display: flex;
  gap: 8px;
  margin-bottom: 12px;
}

.ql-search-box {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 14px;
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  transition: var(--transition-fast);
}

.ql-search-box:focus-within {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
}

.ql-search-icon {
  color: var(--text-muted);
  flex-shrink: 0;
}

.ql-search-input {
  flex: 1;
  border: none;
  outline: none;
  background: transparent;
  font-size: 14px;
  color: var(--text-primary);
  padding: 9px 0;
}

.ql-search-input::placeholder {
  color: var(--text-muted);
}

.ql-search-btn {
  padding: 9px 24px;
  border-radius: var(--radius-sm);
  background: var(--accent);
  color: #fff;
  font-size: 14px;
  font-weight: 600;
  transition: var(--transition-fast);
  white-space: nowrap;
}

.ql-search-btn:hover {
  background: var(--accent-hover);
}

.ql-search-btn:active {
  transform: scale(0.97);
}

/* ===== Filter Panel (tag style) ===== */
.ql-filter-panel {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px 14px;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
}

.ql-filter-row {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 3px 0;
}

.ql-filter-label {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary);
  flex-shrink: 0;
  min-width: 36px;
  padding-top: 4px;
}

.ql-filter-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.ql-tag {
  padding: 3px 12px;
  border-radius: var(--radius-full);
  font-size: 13px;
  font-weight: 500;
  color: var(--text-secondary);
  background: transparent;
  transition: var(--transition-fast);
  white-space: nowrap;
}

.ql-tag:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.ql-tag.active {
  background: var(--accent);
  color: #fff;
  font-weight: 600;
}

/* ===== Scroll Area ===== */
.ql-scroll-area {
  flex: 1;
  overflow-y: auto;
  padding-top: 14px;
}

/* ===== Header Actions ===== */
.header-actions {
  display: flex;
  align-items: center;
  gap: 10px;
}

.basket-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 7px 14px;
  border-radius: var(--radius-full);
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  box-shadow: var(--shadow-xs);
  color: var(--text-secondary);
  font-size: 13px;
  font-weight: 600;
  transition: var(--transition-fast);
}

.basket-btn:hover {
  background: var(--bg-hover);
  color: var(--accent);
}

.basket-count {
  display: flex;
  align-items: center;
  justify-content: center;
  min-width: 18px;
  height: 18px;
  padding: 0 5px;
  border-radius: var(--radius-full);
  background: var(--accent);
  color: #fff;
  font-size: 11px;
  font-weight: 700;
}

/* ===== Filter Chip ===== */
.kp-filter-chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 5px 8px 5px 12px;
  margin-bottom: 12px;
  border-radius: var(--radius-full);
  background: var(--accent-light);
  color: var(--accent);
  font-size: 13px;
  font-weight: 500;
}

.chip-clear {
  width: 20px;
  height: 20px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  color: var(--accent);
  transition: var(--transition-fast);
}

.chip-clear:hover {
  background: var(--accent);
  color: #fff;
}

/* ===== Loading ===== */
.loading-hint {
  text-align: center;
  padding: 48px 20px;
  color: var(--text-muted);
}

/* ===== Card List ===== */
.q-card-list {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

/* ===== Question Card ===== */
.q-card {
  background: var(--bg-card);
  border-radius: var(--radius-md);
  border: 1px solid var(--border-color);
  box-shadow: var(--shadow-sm);
  overflow: hidden;
  transition: box-shadow 0.3s cubic-bezier(0.4, 0, 0.2, 1),
              transform 0.3s cubic-bezier(0.4, 0, 0.2, 1);
}

.q-card:hover {
  box-shadow: var(--shadow-card-hover);
  transform: translateY(-1px);
}

.q-card.is-expanded {
  box-shadow: var(--shadow-md);
  border-color: var(--border-strong);
}

/* ---- Row 1: Header ---- */
.q-card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 20px;
  border-bottom: 1px solid var(--divider);
  gap: 12px;
}

.q-card-tags {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.q-source {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  font-weight: 500;
  color: var(--text-secondary);
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.q-tag {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 3px 10px;
  border-radius: var(--radius-full);
  font-size: 11.5px;
  font-weight: 600;
  letter-spacing: 0.01em;
  white-space: nowrap;
}

/* Type tags */
.q-tag--choice {
  background: rgba(0, 113, 227, 0.1);
  color: var(--accent);
}
.q-tag--fill {
  background: var(--warning-light);
  color: var(--warning);
}
.q-tag--solution {
  background: var(--success-light);
  color: var(--success);
}
.q-tag--judgment {
  background: var(--bg-active);
  color: var(--text-secondary);
}

/* Difficulty tags */
.q-tag--easy {
  background: var(--success-light);
  color: var(--success);
}
.q-tag--medium {
  background: var(--warning-light);
  color: var(--warning);
}
.q-tag--hard {
  background: var(--danger-light);
  color: var(--danger);
}

/* Neutral / status tag */
.q-tag--neutral {
  background: var(--bg-active);
  color: var(--text-muted);
}

.q-card-time {
  font-size: 11.5px;
  color: var(--text-muted);
  white-space: nowrap;
  flex-shrink: 0;
}

/* ---- Row 2: Body ---- */
.q-card-body {
  padding: 16px 20px;
  cursor: pointer;
  transition: background 0.2s ease;
}

.q-card-body:hover {
  background: var(--bg-hover);
}

.q-stem {
  font-size: 14.5px;
  line-height: 1.75;
  color: var(--text-primary);
}

.q-stem :deep(.katex) {
  font-size: 1.02em;
}

.q-stem :deep(.katex-display) {
  margin: 8px 0;
}

/* ---- Options (choice question — no correct marking in list view) ---- */
.q-options {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
  margin-top: 14px;
}

@media (max-width: 640px) {
  .q-options {
    grid-template-columns: 1fr;
  }
}

.q-option {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 8px 12px;
  border-radius: var(--radius-sm);
  background: var(--bg-input);
  border: 1px solid transparent;
  font-size: 13.5px;
  line-height: 1.6;
  transition: var(--transition-fast);
}

.q-option:hover {
  background: var(--bg-hover);
}

.q-option-label {
  flex-shrink: 0;
  width: 22px;
  height: 22px;
  border-radius: 50%;
  background: var(--bg-active);
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 11px;
  font-weight: 700;
  color: var(--text-secondary);
}

/* ---- Analysis expand section — Apple frosted style ---- */
.q-analysis-section {
  padding: 0 20px;
  border-top: 1px solid var(--divider);
  background: linear-gradient(
    180deg,
    rgba(0, 0, 0, 0.015) 0%,
    rgba(0, 0, 0, 0.025) 100%
  );
  overflow: hidden;
}

[data-theme='dark'] .q-analysis-section {
  background: linear-gradient(
    180deg,
    rgba(255, 255, 255, 0.02) 0%,
    rgba(255, 255, 255, 0.035) 100%
  );
}

.q-analysis-title {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 14px 0 10px;
  font-size: 11.5px;
  font-weight: 700;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.06em;
}

/* ---- Correct answer highlight card (choice & fill) ---- */
.q-answer-card {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 14px;
  border-radius: var(--radius-sm);
  margin-bottom: 12px;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-left: 2.5px solid var(--text-muted);
}

.q-answer-card--choice {
  border-left-color: var(--accent);
}

.q-answer-card--fill {
  border-left-color: var(--success);
}

.q-answer-card-label {
  font-size: 12px;
  font-weight: 500;
  color: var(--text-muted);
  letter-spacing: 0.02em;
  flex-shrink: 0;
}

.q-answer-card-value {
  font-size: 16px;
  font-weight: 600;
  letter-spacing: 0.01em;
  flex: 1;
  color: var(--text-primary);
}

.q-answer-card--choice .q-answer-card-value {
  color: var(--accent);
}

.q-answer-card--fill .q-answer-card-value {
  color: var(--success);
}

.q-answer-card-icon {
  flex-shrink: 0;
  opacity: 0.5;
}

.q-answer-card--choice .q-answer-card-icon {
  color: var(--accent);
}

.q-answer-card--fill .q-answer-card-icon {
  color: var(--success);
}

/* ---- Solution answer inline ---- */
.q-answer-inline {
  display: flex;
  align-items: baseline;
  gap: 8px;
  padding: 8px 0 10px;
}

.q-answer-inline-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-muted);
  flex-shrink: 0;
}

.q-answer-inline-value {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
}

/* ---- Analysis body ---- */
.q-analysis-body {
  padding: 4px 0 16px;
  font-size: 13.5px;
  line-height: 1.85;
  color: var(--text-secondary);
}

.q-analysis-body :deep(.katex) {
  font-size: 1em;
}

.q-analysis-body :deep(.katex-display) {
  margin: 6px 0;
}

.q-analysis-empty {
  padding: 4px 0 16px;
  font-size: 13px;
  color: var(--text-muted);
}

/* ---- Row 3: Footer ---- */
.q-card-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 20px;
  border-top: 1px solid var(--divider);
  gap: 12px;
  flex-wrap: wrap;
}

.q-kps {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.q-kps-label {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  font-size: 11.5px;
  font-weight: 600;
  color: var(--text-muted);
}

.q-kp-chip {
  padding: 3px 10px;
  border-radius: var(--radius-full);
  background: var(--purple-light);
  color: var(--purple);
  font-size: 11.5px;
  font-weight: 500;
  white-space: nowrap;
}

.q-kp-empty {
  font-size: 11.5px;
  color: var(--text-muted);
}

.q-grade-chip {
  padding: 3px 10px;
  border-radius: var(--radius-full);
  background: var(--teal-light);
  color: var(--teal);
  font-size: 11.5px;
  font-weight: 500;
}

.q-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.q-action-btn {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 7px 14px;
  border-radius: var(--radius-sm);
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  color: var(--text-secondary);
  font-size: 12.5px;
  font-weight: 600;
  white-space: nowrap;
  transition: var(--transition-fast);
}

.q-action-btn:hover {
  background: var(--bg-hover);
  border-color: var(--border-strong);
  color: var(--text-primary);
}

.q-action-btn:active {
  transform: scale(0.96);
}

/* Active state for basket button */
.q-action--active {
  background: var(--success-light) !important;
  border-color: var(--success) !important;
  color: var(--success) !important;
}

/* Ghost style for analysis button */
.q-action--ghost:hover {
  color: var(--accent);
  border-color: var(--accent);
  background: var(--accent-light);
}

/* ===== Transitions ===== */
.q-analysis-enter-active {
  transition: max-height 0.4s cubic-bezier(0.4, 0, 0.2, 1),
              opacity 0.3s ease,
              padding 0.4s cubic-bezier(0.4, 0, 0.2, 1);
}

.q-analysis-leave-active {
  transition: max-height 0.3s cubic-bezier(0.4, 0, 0.2, 1),
              opacity 0.2s ease,
              padding 0.3s cubic-bezier(0.4, 0, 0.2, 1);
}

.q-analysis-enter-from,
.q-analysis-leave-to {
  max-height: 0;
  opacity: 0;
  padding-top: 0;
  padding-bottom: 0;
}

.q-analysis-enter-to,
.q-analysis-leave-from {
  max-height: 600px;
  opacity: 1;
}

/* ===== Responsive ===== */
@media (max-width: 640px) {
  .q-card-header {
    padding: 10px 14px;
  }
  .q-card-body {
    padding: 12px 14px;
  }
  .q-card-footer {
    padding: 10px 14px;
  }
  .q-analysis-section {
    padding: 0 14px;
  }
  .q-source {
    max-width: 120px;
  }
  .q-card-time {
    display: none;
  }
  .q-actions {
    width: 100%;
    justify-content: flex-end;
  }
}
</style>
