<template>
  <div class="ql-page">
    <!-- ===== 主体：左侧知识树导航 + 右侧列表区 ===== -->
    <div class="ql-body">
      <!-- 左侧常驻知识树导航（替代旧的 KpTreePanel） -->
      <KnowledgeTreeNav :selected-id="navNodeId" @select="handleKnowledgeNodeSelect" />

      <!-- 右侧：工具栏 + 列表区 -->
      <div class="ql-main">
    <!-- ===== Apple风格吸顶工具栏 ===== -->
    <div class="ql-sticky-bar">
      <div class="ql-toolbar">
        <!-- 中间：搜索框 -->
        <div class="ql-search-wrap">
          <AppIcon name="search" :size="15" class="ql-search-icon" />
          <input
            v-model="query.keyword"
            class="ql-search-input"
            placeholder="搜索题目（输入即搜）"
            @input="onSearchInput"
            @keydown.enter="onSearchSubmit"
          />
          <button class="ql-search-go" @click="toggleFilter">
            <AppIcon name="filter" :size="14" />
            筛选
          </button>
        </div>

        <!-- 右侧：新建题目 + 主题切换 -->
        <button
          v-if="basket.count.value > 0"
          class="ql-basket-btn"
          @click="toast.info(`试题篮中有 ${basket.count.value} 道题目`)"
        >
          <AppIcon name="shopping-cart" :size="16" />
          <span class="ql-basket-count">{{ basket.count.value }}</span>
        </button>
        <button class="ql-new-btn" @click="$router.push('/questions/new')">
          <AppIcon name="plus" :size="16" />
          新建题目
        </button>
        <ThemeToggle />
      </div>

      <!-- ===== 多维属性矩阵筛选面板 ===== -->
      <div class="ql-filter-collapse" :class="{ 'is-open': showFilter }">
        <div class="ql-matrix-panel">

          <!-- ── 顶层平铺标签组 ── -->
          <!-- 来源 -->
          <div class="ql-matrix-row">
            <span class="ql-matrix-label">来源:</span>
            <div class="ql-matrix-tags">
              <button
                v-for="opt in sourceOptions"
                :key="opt"
                class="ql-mtag"
                :class="{ active: filters.source === opt }"
                @click="selectFilter('source', opt)"
              >{{ opt }}</button>
            </div>
          </div>

          <!-- 来源子项（级联：仅"高考模拟"显示） -->
          <div class="ql-matrix-row ql-matrix-sub" :class="{ 'is-on': showSubSource }">
            <span class="ql-matrix-label">模拟类型:</span>
            <div class="ql-matrix-tags">
              <button
                v-for="opt in subSourceOptions"
                :key="opt"
                class="ql-mtag"
                :class="{ active: filters.subSource === opt }"
                @click="selectFilter('subSource', opt)"
              >{{ opt }}</button>
            </div>
          </div>

          <!-- 题型 -->
          <div class="ql-matrix-row">
            <span class="ql-matrix-label">题型:</span>
            <div class="ql-matrix-tags">
              <button
                v-for="opt in questionTypeOptions"
                :key="opt.label"
                class="ql-mtag"
                :class="{ active: filters.type === opt.value }"
                @click="selectFilter('type', opt.value)"
              >{{ opt.label }}</button>
            </div>
          </div>

          <!-- 难度 -->
          <div class="ql-matrix-row">
            <span class="ql-matrix-label">难度:</span>
            <div class="ql-matrix-tags">
              <button
                v-for="opt in difficultyOptions"
                :key="opt.value"
                class="ql-mtag"
                :class="{ active: filters.difficulty === opt.value }"
                @click="selectFilter('difficulty', opt.value)"
              >{{ opt.label }}</button>
            </div>
          </div>

          <!-- ── 底部折叠下拉组：态转换（未选=纯文字 / 已选=蓝标签×）── -->
          <div class="ql-matrix-row">
            <span class="ql-matrix-label">更多:</span>
            <div class="ql-matrix-dropdown-bar">
              <!-- 年份 -->
              <div class="ql-matrix-dropdown" :class="{ 'is-open': openDropdown === 'year' }">
                <!-- 未选中：纯文字触发器 -->
                <button v-if="!isFilterActive('year')" class="ql-dd-plain" @click.stop="toggleDropdown('year')">
                  <span>年份</span>
                  <AppIcon name="chevron-down" :size="11" class="ql-dd-caret" />
                </button>
                <!-- 已选中：蓝标签 + × 关闭 -->
                <span v-else class="ql-dd-chip">
                  {{ filters.year }}
                  <button class="ql-dd-chip-x" @click.stop="clearFilter('year')">
                    <AppIcon name="x" :size="10" />
                  </button>
                </span>
                <div v-if="openDropdown === 'year'" class="ql-dd-panel">
                  <button
                    v-for="opt in yearOptions"
                    :key="opt"
                    class="ql-dd-opt"
                    :class="{ active: filters.year === opt }"
                    @click.stop="selectFilter('year', opt); openDropdown = null"
                  >{{ opt }}</button>
                </div>
              </div>

              <!-- 地区（级联：选中后动态渲染市级下拉） -->
              <div class="ql-matrix-dropdown" :class="{ 'is-open': openDropdown === 'region' }">
                <button v-if="!isFilterActive('region')" class="ql-dd-plain" @click.stop="toggleDropdown('region')">
                  <span>地区</span>
                  <AppIcon name="chevron-down" :size="11" class="ql-dd-caret" />
                </button>
                <span v-else class="ql-dd-chip">
                  {{ filters.region }}
                  <button class="ql-dd-chip-x" @click.stop="clearFilter('region')">
                    <AppIcon name="x" :size="10" />
                  </button>
                </span>
                <div v-if="openDropdown === 'region'" class="ql-dd-panel">
                  <button
                    v-for="opt in regionOptions"
                    :key="opt"
                    class="ql-dd-opt"
                    :class="{ active: filters.region === opt }"
                    @click.stop="selectFilter('region', opt); openDropdown = null"
                  >{{ opt }}</button>
                </div>
              </div>

              <!-- 市/区（动态级联：仅当地区已选时渲染） -->
              <div v-if="showCityDropdown" class="ql-matrix-dropdown" :class="{ 'is-open': openDropdown === 'city' }">
                <button v-if="!isFilterActive('city')" class="ql-dd-plain" @click.stop="toggleDropdown('city')">
                  <span>市/区</span>
                  <AppIcon name="chevron-down" :size="11" class="ql-dd-caret" />
                </button>
                <span v-else class="ql-dd-chip">
                  {{ filters.city }}
                  <button class="ql-dd-chip-x" @click.stop="clearFilter('city')">
                    <AppIcon name="x" :size="10" />
                  </button>
                </span>
                <div v-if="openDropdown === 'city'" class="ql-dd-panel">
                  <button
                    v-for="opt in currentCityOptions"
                    :key="opt"
                    class="ql-dd-opt"
                    :class="{ active: filters.city === opt }"
                    @click.stop="selectFilter('city', opt); openDropdown = null"
                  >{{ opt }}</button>
                </div>
              </div>

              <!-- 年级 -->
              <div class="ql-matrix-dropdown" :class="{ 'is-open': openDropdown === 'grade' }">
                <button v-if="!isFilterActive('grade')" class="ql-dd-plain" @click.stop="toggleDropdown('grade')">
                  <span>年级</span>
                  <AppIcon name="chevron-down" :size="11" class="ql-dd-caret" />
                </button>
                <span v-else class="ql-dd-chip">
                  {{ filters.grade }}
                  <button class="ql-dd-chip-x" @click.stop="clearFilter('grade')">
                    <AppIcon name="x" :size="10" />
                  </button>
                </span>
                <div v-if="openDropdown === 'grade'" class="ql-dd-panel">
                  <button
                    v-for="opt in gradeOptions"
                    :key="opt"
                    class="ql-dd-opt"
                    :class="{ active: filters.grade === opt }"
                    @click.stop="selectFilter('grade', opt); openDropdown = null"
                  >{{ opt }}</button>
                </div>
              </div>

              <!-- 学期 -->
              <div class="ql-matrix-dropdown" :class="{ 'is-open': openDropdown === 'semester' }">
                <button v-if="!isFilterActive('semester')" class="ql-dd-plain" @click.stop="toggleDropdown('semester')">
                  <span>学期</span>
                  <AppIcon name="chevron-down" :size="11" class="ql-dd-caret" />
                </button>
                <span v-else class="ql-dd-chip">
                  {{ filters.semester }}
                  <button class="ql-dd-chip-x" @click.stop="clearFilter('semester')">
                    <AppIcon name="x" :size="10" />
                  </button>
                </span>
                <div v-if="openDropdown === 'semester'" class="ql-dd-panel">
                  <button
                    v-for="opt in semesterOptions"
                    :key="opt"
                    class="ql-dd-opt"
                    :class="{ active: filters.semester === opt }"
                    @click.stop="selectFilter('semester', opt); openDropdown = null"
                  >{{ opt }}</button>
                </div>
              </div>

              <!-- 状态 -->
              <div class="ql-matrix-dropdown" :class="{ 'is-open': openDropdown === 'status' }">
                <button v-if="!isFilterActive('status')" class="ql-dd-plain" @click.stop="toggleDropdown('status')">
                  <span>状态</span>
                  <AppIcon name="chevron-down" :size="11" class="ql-dd-caret" />
                </button>
                <span v-else class="ql-dd-chip">
                  {{ statusLabel(filters.status) }}
                  <button class="ql-dd-chip-x" @click.stop="clearFilter('status')">
                    <AppIcon name="x" :size="10" />
                  </button>
                </span>
                <div v-if="openDropdown === 'status'" class="ql-dd-panel">
                  <button
                    v-for="opt in statusOptions"
                    :key="opt.value"
                    class="ql-dd-opt"
                    :class="{ active: filters.status === opt.value }"
                    @click.stop="selectFilter('status', opt.value); openDropdown = null"
                  >{{ opt.label }}</button>
                </div>
              </div>

              <!-- 清空筛选（右侧） -->
              <button
                v-if="hasAnyFilter"
                type="button"
                class="ql-matrix-clear"
                @click="clearAllFilters"
              >
                <AppIcon name="x" :size="12" />
                清空筛选
              </button>
            </div>
          </div>

        </div>
      </div>

    </div>

    <!-- ===== 可滚动列表区域 ===== -->
    <div class="ql-scroll-area">
      <div v-if="loading" class="loading-hint">加载中…</div>

      <template v-else>
        <!-- 空状态：居中缺省页 + 清空筛选快捷按钮 -->
        <div v-if="cardList.length === 0" class="ql-empty-state">
          <div class="ql-empty-icon">
            <AppIcon name="search" :size="36" :stroke="1.5" />
          </div>
          <div class="ql-empty-title">没有找到匹配的题目</div>
          <div class="ql-empty-desc">
            尝试调整搜索关键词或筛选条件
          </div>
          <button
            v-if="hasAnyFilter"
            type="button"
            class="ql-empty-action"
            @click="clearAllFilters"
          >
            <AppIcon name="x" :size="14" />
            清空筛选条件
          </button>
        </div>

        <!-- ===== 题目卡片列表 ===== -->
        <div v-else class="q-card-list">
          <div
            v-for="card in cardList"
            :key="card.id"
            class="q-card"
            :class="{ 'is-expanded': expandedIds.has(card.id) }"
          >
            <!-- Header Row 1: 题目来源（年级·学期·考试类型·地区·年份） -->
            <div v-if="formatQuestionSource(card)" class="q-card-source-row">
              <AppIcon name="bookmark" :size="12" :stroke="2" />
              <span>{{ formatQuestionSource(card) }}</span>
            </div>

            <!-- Header Row 2: 属性标签 + 知识点 + 时间 -->
            <div class="q-card-header">
              <div class="q-card-tags">
                <AppBadge :color="typeBadgeColor(card.question_type)">
                  {{ typeLabel(card.question_type) }}
                </AppBadge>
                <AppBadge :color="diffBadgeColor(card.difficulty)">
                  {{ diffLabel(card.difficulty) }}
                </AppBadge>
                <AppBadge :color="statusBadgeColor(card.status)" class="flex items-center gap-1">
                  <AppIcon :name="statusIcon(card.status)" :size="11" :stroke="2" />
                  {{ statusLabel(card.status) }}
                </AppBadge>
                <span class="q-tags-divider">|</span>
                <span class="q-kp-inline">
                  <span
                    v-for="kn in card.knowledgeNodes"
                    :key="kn.id"
                    class="q-kp-text"
                  >{{ kn.name }}</span>
                  <span v-if="card.knowledgeNodes.length === 0" class="q-kp-text-empty">未关联知识点</span>
                </span>
              </div>
              <span class="q-card-time">{{ formatTime(card.updated_at) }}</span>
            </div>

            <!-- Row 2: Body — 题干 + 选项 -->
            <div class="q-card-body" :ref="setCardBodyRef(card.id)" @click="goDetail(card)">
              <div class="q-stem">
                <LatexRender :text="card.stem" />
              </div>
              <!-- 选择题选项（列表页不标注正确答案） -->
              <div v-if="card.question_type === 'choice' && card.parsedOptions.length > 0" class="q-options" :class="optionLayoutClass(card.id)">
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
                  <span class="q-answer-card-value"><LatexRender :text="card.correctAnswer" :inline="true" /></span>
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
                  <span class="q-answer-inline-value"><LatexRender :text="card.correctAnswer" :inline="true" /></span>
                </div>

                <div v-if="card.analysis" class="q-analysis-body">
                  <LatexRender :text="card.analysis" />
                </div>
                <div v-else class="q-analysis-empty">暂无解析内容</div>
              </div>
            </Transition>

            <!-- Row 3: Footer — 操作按钮 -->
            <div class="q-card-footer">
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
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, computed, onMounted, onBeforeUnmount, watch, nextTick, type ComponentPublicInstance } from 'vue'
import { useRouter } from 'vue-router'
import { questionApi, type QuestionSummary, type QuestionDetail, type QuestionQuery, type GradeLevel, type SemesterType, type ExamType, type KnowledgeNodeSummary } from '@/api/client'
import LatexRender from '@/components/LatexRender.vue'
import ThemeToggle from '@/components/ThemeToggle.vue'
import KnowledgeTreeNav from '@/components/KnowledgeTreeNav.vue'
import { AppButton, AppSelect, AppPagination, AppIcon, AppBadge } from '@/components/ui'
import { useQuestionBasket } from '@/composables/useQuestionBasket'
import { useToast } from '@/composables/useToast'
import { useSpaceStore } from '@/stores/space'
import {
  typeLabel,
  typeBadgeColor,
  diffLabel,
  diffBadgeColor,
  statusLabel,
  statusIcon,
  statusBadgeColor,
  formatTime,
} from '@/utils/questionDisplay'

const router = useRouter()
const toast = useToast()
const space = useSpaceStore()
const basket = useQuestionBasket()

// 左侧知识树导航选中的节点 ID（空字符串 = 全部题目）
const navNodeId = ref('')

// 左侧树节点点击 → 同步到 query 并触发表格刷新（默认包含子孙节点）
function handleKnowledgeNodeSelect(nodeId: string) {
  navNodeId.value = nodeId
  query.knowledge_node_ids = nodeId ? [nodeId] : undefined
  query.include_descendants = nodeId ? true : undefined
  page.value = 1
  fetchList()
}

// ---- 筛选面板展开状态 ----
const showFilter = ref(false)

function toggleFilter() {
  showFilter.value = !showFilter.value
  if (!showFilter.value) {
    // 收起时执行搜索
    page.value = 1
    fetchList()
  }
}

// 是否有任何筛选条件被激活（用于显示"清空筛选"按钮）
const hasAnyFilter = computed(() => {
  return !!(
    query.keyword ||
    query.question_type ||
    query.difficulty ||
    query.status ||
    (query.knowledge_node_ids && query.knowledge_node_ids.length > 0)
  )
})

function clearAllFilters() {
  query.keyword = ''
  query.question_type = undefined
  query.difficulty = undefined
  query.status = undefined
  query.knowledge_node_ids = undefined
  query.include_descendants = undefined
  navNodeId.value = ''
  // 重置多维筛选 UI 状态
  filters.source = '全部'
  filters.subSource = '全部高考模拟'
  filters.type = '__all'
  filters.difficulty = '__all'
  filters.status = '__all'
  filters.year = '全部'
  filters.grade = '全部'
  filters.semester = '全部'
  filters.region = '全部'
  filters.city = '全部'
  openDropdown.value = null
  page.value = 1
  fetchList()
}

// 下拉面板点击外部关闭
function onDropdownClickOutside(e: MouseEvent) {
  const target = e.target as Node
  if (!(e.target as HTMLElement).closest('.ql-matrix-dropdown')) {
    openDropdown.value = null
  }
}
onMounted(() => {
  document.addEventListener('click', onDropdownClickOutside)
})
onBeforeUnmount(() => {
  document.removeEventListener('click', onDropdownClickOutside)
})

function spaceKindLabel(kind: string) {
  if (kind === 'personal') return '个人'
  if (kind === 'public') return '公共'
  if (kind === 'team') return '团队'
  return kind
}

// 全局 SpaceSwitcher 切换空间时自动刷新列表
watch(
  () => space.currentSpaceId,
  (newId) => {
    query.space_id = newId || undefined
    page.value = 1
    fetchList()
  },
)

// ---- 卡片数据类型 ----
interface QuestionCard {
  id: string
  stem: string
  question_type: string
  difficulty: string
  status: string
  grade_level: GradeLevel | null
  semester: SemesterType | null
  source: string | null
  school_source: string | null
  exam_type: ExamType | null
  region: string | null
  year: string | null
  metadata: Record<string, unknown>
  updated_at: string
  version: number
  parsedOptions: { label: string; content: string }[]
  correctAnswer: string
  analysis: string | null
  knowledgeNodes: KnowledgeNodeSummary[]
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
  knowledge_node_ids: [],
  include_descendants: true,
  space_id: space.currentSpaceId || undefined,
  page: 1,
  page_size: pageSize,
})

// 难度数值 1-5 → 字符串（兼容 diffLabel/diffBadgeColor 显示函数）
function difficultyNumToString(n: number): string {
  if (n <= 2) return 'easy'
  if (n === 3) return 'medium'
  return 'hard'
}

// GradeLevel 枚举 → 中文标签
function gradeLevelLabel(g: GradeLevel | null | undefined): string {
  if (!g) return ''
  const map: Record<GradeLevel, string> = {
    grade_7: '初一',
    grade_8: '初二',
    grade_9: '初三',
    grade_10: '高一',
    grade_11: '高二',
    grade_12: '高三',
    other: '其他',
  }
  return map[g] || g
}

function semesterLabel(s: SemesterType | null | undefined): string {
  if (!s) return ''
  const map: Record<SemesterType, string> = {
    first: '上学期',
    second: '下学期',
    full_year: '全年',
  }
  return map[s] || s
}

function examTypeLabel(t: ExamType | null | undefined): string {
  if (!t) return ''
  const map: Record<ExamType, string> = {
    midterm: '期中',
    final: '期末',
    gaokao: '高考',
    mock: '模拟',
    entrance: '中考',
    daily: '日常',
    other: '其他',
  }
  return map[t] || t
}

/// 格式化题目来源：将年级、学期、考试类型、来源、地区等非空字段用 · 拼接
function formatQuestionSource(q: any): string {
  const parts: string[] = []

  // 1. 年级
  if (q.grade_level) {
    parts.push(gradeLevelLabel(q.grade_level) || q.grade_level)
  }
  // 2. 学期
  if (q.semester) {
    parts.push(semesterLabel(q.semester) || q.semester)
  }
  // 3. 考试类型
  if (q.exam_type) {
    parts.push(examTypeLabel(q.exam_type) || q.exam_type)
  }
  // 4. 来源学校
  if (q.school_source && q.school_source.trim() !== '') {
    parts.push(q.school_source.trim())
  }
  // 5. 地区
  if (q.region && q.region.trim() !== '') {
    parts.push(q.region.trim())
  }
  // 6. 年份
  if (q.year && q.year.trim() !== '') {
    parts.push(q.year.trim())
  }

  return parts.filter(Boolean).join(' · ')
}

// ============================================================================
// 多维属性矩阵筛选 — 数据字典
// ============================================================================
// 来源（顶层平铺标签）
const sourceOptions = [
  '全部', '课前预习', '课堂例题', '随堂练习', '课后作业',
  '单元复习', '单元测试', '阶段检测', '期中', '期末',
  '高考真题', '高考模拟',
]

// 高考模拟子类型（仅当 source === '高考模拟' 时级联显示）
const subSourceOptions = [
  '全部高考模拟', '一模', '二模', '三模', '模拟预测',
]

// 题型（label 为业务展示，value 对齐后端 QuestionType 枚举）
const questionTypeOptions = [
  { label: '全部', value: '__all' },
  { label: '单选题', value: 'choice' },
  { label: '多选题', value: 'choice' },
  { label: '填空题', value: 'fill' },
  { label: '解答题', value: 'solution' },
  { label: '判断题', value: 'judgment' },
]

// 难度
const difficultyOptions = [
  { label: '全部', value: '__all' },
  { label: '容易', value: 'easy' },
  { label: '适中', value: 'medium' },
  { label: '困难', value: 'hard' },
]

// 底部下拉组
const yearOptions = ['全部', '2020', '2021', '2022', '2023', '2024', '2025', '2026']
const gradeOptions = ['全部', '高一', '高二', '高三']
const semesterOptions = ['全部', '上学期', '下学期']
const regionOptions = ['全部', '北京', '上海', '浙江', '江苏', '广东', '湖北', '湖南', '四川', '山东']

// 地区 → 市区 级联字典（其他省份用空数组兜底）
const cityOptions: Record<string, string[]> = {
  '浙江': ['杭州市', '宁波市', '温州市', '绍兴市', '嘉兴市'],
  '江苏': ['南京市', '苏州市', '无锡市', '常州市', '南通市'],
  '广东': ['广州市', '深圳市', '珠海市', '佛山市', '东莞市'],
  '北京': ['东城区', '西城区', '海淀区', '朝阳区', '丰台区'],
  '上海': ['黄浦区', '徐汇区', '浦东新区', '静安区', '杨浦区'],
}

const statusOptions = [
  { label: '全部', value: '__all' },
  { label: '草稿', value: 'draft' },
  { label: '待审核', value: 'pending' },
  { label: '驳回', value: 'rejected' },
  { label: '已发布', value: 'published' },
  { label: '已停用', value: 'disabled' },
]

// ============================================================================
// 筛选响应式状态（UI 层）— 与后端 query 解耦，applyFilters 时映射
// ============================================================================
const filters = reactive({
  source: '全部',
  subSource: '全部高考模拟',
  type: '__all',
  difficulty: '__all',
  status: '__all',
  year: '全部',
  grade: '全部',
  semester: '全部',
  region: '全部',
  city: '全部',
})

// 级联逻辑：source 切换非"高考模拟"时，重置 subSource 并隐藏子行
const showSubSource = computed(() => filters.source === '高考模拟')
watch(() => filters.source, (v) => {
  if (v !== '高考模拟') filters.subSource = '全部高考模拟'
})

// 地区 → 市区 级联：切换地区时清空市级，并控制市级下拉是否渲染
const showCityDropdown = computed(() => filters.region !== '全部')
const currentCityOptions = computed(() => {
  const cities = cityOptions[filters.region] || []
  return ['全部', ...cities]
})
watch(() => filters.region, () => {
  filters.city = '全部' // 清空市级选中
})

// 底部下拉面板展开状态（同时只展开一个）
const openDropdown = ref<null | 'year' | 'grade' | 'semester' | 'region' | 'city' | 'status'>(null)
function toggleDropdown(key: typeof openDropdown.value) {
  openDropdown.value = openDropdown.value === key ? null : key
}

// 通用筛选点击：更新 filters → 映射到 query → 触发搜索
function selectFilter(field: keyof typeof filters, value: string) {
  ;(filters as any)[field] = value
  applyFilters()
}

// 判断下拉项是否已激活（值非"全部"/"__all"）
function isFilterActive(field: keyof typeof filters): boolean {
  const v = (filters as any)[field]
  return v !== '全部' && v !== '__all'
}

// 清除单个筛选项（× 图标点击），恢复为默认"全部"值
function clearFilter(field: keyof typeof filters) {
  const defaults: Record<string, string> = {
    source: '全部', subSource: '全部高考模拟', type: '__all', difficulty: '__all',
    status: '__all', year: '全部', grade: '全部', semester: '全部', region: '全部', city: '全部',
  }
  ;(filters as any)[field] = defaults[field] ?? '全部'
  // 地区清除时同步清空市级
  if (field === 'region') filters.city = '全部'
  applyFilters()
}

// 状态值 → 中文 label
function statusLabel(value: string): string {
  return statusOptions.find(o => o.value === value)?.label ?? ''
}

// 将 UI filters 映射到后端 query 并搜索
function applyFilters() {
  // 题型
  query.question_type = filters.type === '__all' ? undefined : (filters.type as any)
  // 难度
  query.difficulty = filters.difficulty === '__all' ? undefined : (filters.difficulty as any)
  // 状态
  query.status = filters.status === '__all' ? undefined : (filters.status as any)
  // TODO: source / subSource / year / grade / semester / region 待后端支持后映射
  page.value = 1
  fetchList()
}

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

// ---- 选项自适应布局：基于 KaTeX 渲染后真实宽度测量 ----
const OPTION_GAP = 16 // 选项间距 (px)
const OPTION_PADDING = 44 // 选项内 label 圆 + padding 估算 (px)
const optionLayoutMap = reactive<Record<string, 'grid-4' | 'grid-2' | 'grid-1'>>({})
const cardBodyRefs = reactive<Record<string, HTMLElement | null>>({})
const resizeObservers: ResizeObserver[] = []
let layoutDebounce: ReturnType<typeof setTimeout> | null = null

/** 为某张卡片计算选项布局 */
function computeOptionLayout(cardId: string) {
  const container = cardBodyRefs[cardId]
  if (!container) return
  const containerWidth = container.clientWidth
  if (containerWidth === 0) return

  // 获取该卡片内选项容器
  const optionsEl = container.querySelector<HTMLElement>('.q-options')
  if (!optionsEl) return

  const optionEls = optionsEl.querySelectorAll<HTMLElement>('.q-option')
  if (optionEls.length === 0) return

  // 临时切换为非Grid布局以测量选项内容真实宽度
  const prevDisplay = optionsEl.style.display
  const prevCols = optionsEl.style.gridTemplateColumns
  optionsEl.style.display = 'block'
  optionsEl.style.gridTemplateColumns = ''

  let maxWidth = 0
  const prevStyles: { el: HTMLElement; display: string; width: string }[] = []
  optionEls.forEach(el => {
    prevStyles.push({ el, display: el.style.display, width: el.style.width })
    el.style.display = 'inline-flex'
    el.style.width = 'auto'
    el.style.whiteSpace = 'nowrap'
    const w = el.scrollWidth
    if (w > maxWidth) maxWidth = w
    el.style.whiteSpace = ''
  })

  // 恢复选项元素样式
  prevStyles.forEach(({ el, display, width }) => {
    el.style.display = display
    el.style.width = width
  })

  // 恢复选项容器布局
  optionsEl.style.display = prevDisplay
  optionsEl.style.gridTemplateColumns = prevCols

  if (maxWidth === 0) return

  // 布局判定
  const slot = maxWidth + OPTION_GAP
  let layout: 'grid-4' | 'grid-2' | 'grid-1'
  if (slot * 4 <= containerWidth) {
    layout = 'grid-4'
  } else if (slot * 2 <= containerWidth) {
    layout = 'grid-2'
  } else {
    layout = 'grid-1'
  }
  optionLayoutMap[cardId] = layout
}

/** 对所有卡片重新计算布局（防抖） */
function recomputeAllLayouts() {
  if (layoutDebounce) clearTimeout(layoutDebounce)
  layoutDebounce = setTimeout(() => {
    Object.keys(cardBodyRefs).forEach(id => computeOptionLayout(id))
  }, 150)
}

/** 设置卡片 body 的 ref，并注册 ResizeObserver */
function setCardBodyRef(cardId: string) {
  return (el: Element | ComponentPublicInstance | null) => {
    if (el instanceof HTMLElement) {
      cardBodyRefs[cardId] = el
      // 注册 ResizeObserver 监听容器宽度变化
      const ro = new ResizeObserver(() => recomputeAllLayouts())
      ro.observe(el)
      resizeObservers.push(ro)
    } else {
      delete cardBodyRefs[cardId]
    }
  }
}

/** 获取某张卡片的选项布局类名 */
function optionLayoutClass(cardId: string): string {
  return optionLayoutMap[cardId] || 'grid-2'
}

// 监听 cardList 变化，在 DOM 更新后触发首次布局计算
watch(cardList, () => {
  nextTick(() => {
    setTimeout(() => {
      Object.keys(cardBodyRefs).forEach(id => computeOptionLayout(id))
    }, 100)
  })
})

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
        difficulty: difficultyNumToString(s.difficulty),
        status: s.status,
        grade_level: s.grade_level,
        semester: detail?.semester ?? null,
        source: detail?.source ?? null,
        school_source: detail?.source ?? null,
        exam_type: detail?.exam_type ?? null,
        region: (detail?.metadata?.exam_region as string) ?? null,
        year: detail?.metadata?.academic_year ? String(detail.metadata.academic_year) : null,
        metadata: detail?.metadata ?? {},
        updated_at: s.updated_at,
        version: s.version,
        parsedOptions: parseOptions(detail?.options),
        correctAnswer: parseAnswer(detail?.correct_answer),
        analysis: detail?.analysis ?? null,
        knowledgeNodes: detail?.knowledge_nodes ?? [],
      }
    })
  } catch (e: any) {
    console.error('列表加载失败:', e)
    toast.error(e.response?.data?.error || e.response?.data?.message || e.message || '列表加载失败')
  } finally {
    loading.value = false
  }
}

function goDetail(row: { id: string }) {
  router.push(`/questions/${row.id}`)
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

// 左侧导航节点变化已由 handleKnowledgeNodeSelect 处理，无需 watch

watch(() => space.currentSpaceId, (newId) => {
  query.space_id = newId || undefined
  page.value = 1
  fetchList()
})

onMounted(fetchList)
onBeforeUnmount(() => {
  if (searchTimer) clearTimeout(searchTimer)
  if (layoutDebounce) clearTimeout(layoutDebounce)
  resizeObservers.forEach(ro => ro.disconnect())
})
</script>

<style scoped>
/* ===== Apple风格吸顶工具栏 ===== */
.ql-page {
  position: absolute; /* 绝对定位撑满父级 .view.active，避免 100vh 与上方导航栏叠加溢出 */
  inset: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden; /* 锁定外层高度，彻底掐断全局滚动条 */
}

/* ===== 主体：左侧知识树 + 右侧列表区 ===== */
.ql-body {
  flex: 1;
  min-height: 0;
  display: flex;
  overflow: hidden;
}

/* 右侧主区：工具栏 + 滚动列表 */
.ql-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.ql-sticky-bar {
  position: sticky;
  top: 0;
  z-index: 100;
  flex-shrink: 0;
  background: var(--bg-primary);
  backdrop-filter: saturate(180%) blur(20px);
  -webkit-backdrop-filter: saturate(180%) blur(20px);
  border-bottom: 1px solid var(--border-color);
}

/* ===== 工具栏单行布局 ===== */
.ql-toolbar {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 12px 20px;
}

/* 题库空间切换 */
/* 空间切换 — Apple分段控件 */
.ql-space-segmented {
  display: inline-flex;
  align-items: center;
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  border-radius: 10px;
  padding: 3px;
  gap: 2px;
  flex-shrink: 0;
}

.ql-space-seg {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 5px 14px;
  border: none;
  background: transparent;
  border-radius: 7px;
  font-size: 13px;
  font-weight: 500;
  color: var(--text-secondary);
  cursor: pointer;
  transition: var(--transition-fast);
  white-space: nowrap;
}

.ql-space-seg:hover:not(.active) {
  color: var(--text-primary);
  background: var(--bg-hover);
}

.ql-space-seg.active {
  background: var(--bg-elevated, var(--bg-card));
  color: var(--text-primary);
  font-weight: 600;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.08);
}

[data-theme='dark'] .ql-space-seg.active {
  background: #3a3a3c;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.3);
}

/* 搜索框 */
.ql-search-wrap {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 0;
  height: 36px;
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  border-radius: 10px;
  transition: var(--transition-fast);
  overflow: hidden;
}

.ql-search-wrap:focus-within {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
}

.ql-search-icon {
  color: var(--text-muted);
  flex-shrink: 0;
  margin-left: 12px;
}

.ql-search-input {
  flex: 1;
  border: none;
  outline: none;
  background: transparent;
  font-size: 14px;
  color: var(--text-primary);
  padding: 0 10px;
  height: 100%;
}

.ql-search-input::placeholder {
  color: var(--text-muted);
}

.ql-search-go {
  display: flex;
  align-items: center;
  gap: 5px;
  height: 100%;
  padding: 0 16px;
  background: var(--accent);
  color: #fff;
  font-size: 13px;
  font-weight: 600;
  transition: var(--transition-fast);
  white-space: nowrap;
  flex-shrink: 0;
}

.ql-search-go:hover {
  background: var(--accent-hover);
}

.ql-search-go:active {
  transform: scale(0.96);
}

/* 新建题目按钮 */
.ql-new-btn {
  display: flex;
  align-items: center;
  gap: 5px;
  height: 36px;
  padding: 0 16px;
  border-radius: 10px;
  background: var(--text-primary);
  color: var(--bg-primary);
  font-size: 13px;
  font-weight: 600;
  transition: var(--transition-fast);
  white-space: nowrap;
  flex-shrink: 0;
}

.ql-new-btn:hover {
  opacity: 0.85;
}

.ql-new-btn:active {
  transform: scale(0.96);
}

/* 试题篮按钮 */
.ql-basket-btn {
  position: relative;
  display: flex;
  align-items: center;
  height: 36px;
  padding: 0 12px;
  border-radius: 10px;
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  color: var(--text-secondary);
  transition: var(--transition-fast);
  flex-shrink: 0;
}

.ql-basket-btn:hover {
  border-color: var(--accent);
  color: var(--accent);
}

.ql-basket-count {
  position: absolute;
  top: -4px;
  right: -4px;
  min-width: 18px;
  height: 18px;
  border-radius: 9px;
  background: var(--accent);
  color: #fff;
  font-size: 11px;
  font-weight: 700;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0 4px;
}

/* ===== 筛选面板展开/折叠动画 (grid 0fr→1fr 技术, 最平滑) ===== */
.ql-filter-collapse {
  display: grid;
  grid-template-rows: 0fr;
  opacity: 0;
  transition:
    grid-template-rows 0.4s cubic-bezier(0.32, 0.72, 0, 1),
    opacity 0.3s cubic-bezier(0.32, 0.72, 0, 1);
}

.ql-filter-collapse.is-open {
  grid-template-rows: 1fr;
  opacity: 1;
}

.ql-filter-collapse > .ql-matrix-panel {
  overflow: hidden; /* 折叠动画期间裁剪内容（grid 0fr→1fr 技术必需） */
  min-height: 0;
}

/* 展开态：解除裁剪，让底部下拉面板 .ql-dd-panel 能自由溢出父容器 */
.ql-filter-collapse.is-open > .ql-matrix-panel {
  overflow: visible;
}

/* ===== 多维属性矩阵筛选面板 ===== */
.ql-matrix-panel {
  position: relative; /* 建立堆叠上下文，凌驾于下方题目列表之上 */
  z-index: 100;
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 16px 20px 14px;
  background: var(--bg-card);
  border-top: 1px solid var(--border-color);
}

/* —— 平铺标签行 —— */
.ql-matrix-row {
  display: flex;
  align-items: flex-start;
  gap: 16px;
  padding: 2px 0;
}

.ql-matrix-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-muted);
  letter-spacing: 0.03em;
  flex-shrink: 0;
  min-width: 44px;
  height: 30px;
  line-height: 30px;
}

.ql-matrix-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 7px;
  flex: 1;
  min-width: 0;
}

/* 单个标签 — 扁平化：纯文本，无背景/边框/圆角 */
.ql-mtag {
  padding: 3px 10px;
  border-radius: 0;
  font-size: 13px;
  font-weight: 500;
  color: var(--text-secondary);
  background: transparent;
  border: none;
  transition: var(--transition-fast);
  white-space: nowrap;
  cursor: pointer;
}

.ql-mtag:hover {
  color: var(--text-primary);
}

/* 选中态：仅品牌蓝文字，无背景 */
.ql-mtag.active {
  color: #1890ff;
  font-weight: 600;
  background: transparent;
  border: none;
}

/* —— 级联子行（高考模拟子类型）—— */
.ql-matrix-sub {
  max-height: 0;
  opacity: 0;
  overflow: hidden;
  margin: 0;
  padding: 0;
  transition:
    max-height 0.3s cubic-bezier(0.32, 0.72, 0, 1),
    opacity 0.2s cubic-bezier(0.32, 0.72, 0, 1),
    margin 0.3s ease;
}

.ql-matrix-sub.is-on {
  max-height: 60px;
  opacity: 1;
  margin-top: -2px;
  padding: 2px 0;
}

.ql-matrix-sub .ql-matrix-label {
  color: var(--accent);
  opacity: 0.7;
}

/* —— 底部下拉组 —— */
.ql-matrix-dropdown-bar {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 12px;
  flex: 1;
  min-width: 0;
}

.ql-matrix-dropdown {
  position: relative; /* 作为 .ql-dd-panel 的定位上下文 */
  display: inline-flex;
  z-index: 101; /* 略高于 .ql-matrix-panel，确保展开的下拉浮层在同级之上 */
}

/* 未选中态：纯文字触发器（轻量，无边框） */
.ql-dd-plain {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 3px 0;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: var(--transition-fast);
}

.ql-dd-plain:hover {
  color: var(--text-primary);
}

.ql-matrix-dropdown.is-open .ql-dd-plain {
  color: #1890ff;
}

/* 选中态：浅蓝背景 + 蓝字 + × 关闭的 chip 标签 */
.ql-dd-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 3px 4px 3px 10px;
  border-radius: 4px;
  background: #e6f7ff;
  color: #1890ff;
  font-size: 12.5px;
  font-weight: 600;
  border: 1px solid #91d5ff;
  line-height: 1.4;
}

.ql-dd-chip-x {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 14px;
  height: 14px;
  border: none;
  background: transparent;
  color: #1890ff;
  cursor: pointer;
  border-radius: 50%;
  padding: 0;
  flex-shrink: 0;
  transition: var(--transition-fast);
}

.ql-dd-chip-x:hover {
  background: #1890ff;
  color: #fff;
}

.ql-dd-caret {
  color: var(--text-muted);
  transition: transform 0.2s;
}

.ql-matrix-dropdown.is-open .ql-dd-caret {
  transform: rotate(180deg);
  color: #1890ff;
}

/* 下拉面板 */
.ql-dd-panel {
  position: absolute;
  top: calc(100% + 6px);
  left: 0;
  z-index: 200; /* 高于矩阵面板(100)与下拉组(101)，绝对自由浮于题目列表之上 */
  min-width: 120px;
  max-height: 280px;
  overflow-y: auto;
  padding: 5px;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: 10px;
  box-shadow: var(--shadow-md, 0 4px 16px rgba(0, 0, 0, 0.12));
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.ql-dd-opt {
  display: block;
  width: 100%;
  text-align: left;
  padding: 7px 12px;
  border: none;
  background: transparent;
  border-radius: 6px;
  font-size: 12.5px;
  color: var(--text-primary);
  cursor: pointer;
  transition: var(--transition-fast);
  white-space: nowrap;
}

.ql-dd-opt:hover {
  background: var(--bg-hover);
}

.ql-dd-opt.active {
  background: #e6f7ff;
  color: #1890ff;
  font-weight: 600;
}

/* —— 清空筛选按钮 —— */
.ql-matrix-clear {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  margin-left: auto;
  padding: 5px 12px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  border-radius: 9999px;
  transition: var(--transition-fast);
}

.ql-matrix-clear:hover {
  color: var(--danger);
  background: var(--danger-light);
}

/* ===== 可滚动列表区域（独立滚动域） ===== */
.ql-scroll-area {
  flex: 1;
  min-height: 0; /* Flex 子项允许收缩，使 flex:1 + overflow-y:auto 生效 */
  overflow-y: auto;
  overscroll-behavior: contain; /* 切断滚动链：防止列表触底触发外层滚动/橡皮筋 */
  padding: 16px 20px;
  background: var(--bg-primary);
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

/* ===== Knowledge node filter row ===== */
.ql-filter-kp {
  display: flex;
  align-items: center;
  gap: 12px;
  flex: 1;
  min-width: 0;
}

.ql-filter-descendant {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--text-secondary);
  white-space: nowrap;
  cursor: pointer;
  user-select: none;
}

.ql-filter-descendant input {
  margin: 0;
  cursor: pointer;
}

/* ===== Loading ===== */
.loading-hint {
  text-align: center;
  padding: 48px 20px;
  color: var(--text-muted);
}

/* ===== 空状态：居中缺省页 ===== */
.ql-empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 80px 20px 60px;
  text-align: center;
  animation: ql-empty-fade 0.4s ease;
}

@keyframes ql-empty-fade {
  from { opacity: 0; transform: translateY(8px); }
  to { opacity: 1; transform: translateY(0); }
}

.ql-empty-icon {
  width: 72px;
  height: 72px;
  border-radius: 50%;
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-muted);
  margin-bottom: 18px;
}

.ql-empty-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: 6px;
  letter-spacing: -0.01em;
}

.ql-empty-desc {
  font-size: 13px;
  color: var(--text-muted);
  margin-bottom: 18px;
}

.ql-empty-action {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 7px 16px;
  border-radius: 9999px;
  border: 1px solid var(--border-color);
  background: var(--bg-card);
  color: var(--text-secondary);
  font-size: 12.5px;
  font-weight: 600;
  cursor: pointer;
  transition: var(--transition-fast);
}

.ql-empty-action:hover {
  border-color: var(--accent);
  color: var(--accent);
  background: var(--accent-light);
}

/* ===== Card List ===== */
.q-card-list {
  display: flex;
  flex-direction: column;
  gap: 16px; /* gap-4 */
}

/* ===== Question Card ===== */
.q-card {
  background: var(--bg-card);
  border-radius: 16px; /* rounded-2xl */
  border: 1px solid transparent;
  box-shadow: var(--shadow-sm);
  overflow: hidden;
  transition: all 0.3s cubic-bezier(0.25, 0.8, 0.25, 1);
}

.q-card:hover {
  transform: translateY(-4px); /* hover:-translate-y-1 */
  box-shadow: var(--shadow-md);
}

.q-card.is-expanded {
  box-shadow: var(--shadow-md);
}

[data-theme='dark'] .q-card {
  border-color: #3a3a3c;
  box-shadow: none;
}

[data-theme='dark'] .q-card:hover {
  border-color: #3a3a3c;
  box-shadow: none;
}

[data-theme='dark'] .q-card.is-expanded {
  box-shadow: none;
}

/* ---- Header Row 1: 来源 ---- */
.q-card-source-row {
  display: flex;
  align-items: center;
  gap: 5px;
  padding: 10px 20px 0;
  font-size: 13px;
  font-weight: 600;
  color: var(--accent);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* ---- Header Row 2: 标签 + 知识点 ---- */
.q-card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 20px;
  border-bottom: 1px solid var(--divider);
  gap: 12px;
}

.q-card-tags {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.q-tags-divider {
  color: var(--text-muted);
  font-size: 12px;
  margin: 0 2px;
}

.q-kp-inline {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.q-kp-text {
  font-size: 12px;
  color: var(--text-muted);
  white-space: nowrap;
}

.q-kp-text-empty {
  font-size: 12px;
  color: var(--text-muted);
  opacity: 0.6;
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
/* 选择题选项布局: 基于KaTeX真实宽度测量的自适应Grid */
.q-options {
  display: grid;
  gap: 16px;
  margin-top: 14px;
}

/* 一行四列 */
.q-options.grid-4 {
  grid-template-columns: repeat(4, 1fr);
}

/* 两行两列 */
.q-options.grid-2 {
  grid-template-columns: repeat(2, 1fr);
}

/* 四行一列 */
.q-options.grid-1 {
  grid-template-columns: 1fr;
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
  justify-content: flex-end;
  padding: 10px 20px;
  border-top: 1px solid var(--divider);
  gap: 12px;
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

<!-- 非 scoped 样式：打通父级高度链，让 .ql-page 的 absolute/inset:0 能撑满 .view.active -->
<style>
.view.active {
  height: 100%;
  position: relative; /* 配合子元素 .ql-page 的 absolute inset:0 撑满 */
}
</style>
