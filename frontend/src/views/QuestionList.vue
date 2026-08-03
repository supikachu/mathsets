<template>
  <div class="ql-page">
    <!-- ===== 主体：左侧知识树导航 + 右侧列表区 ===== -->
    <div class="ql-body">
      <!-- 左侧常驻知识树导航（替代旧的 KpTreePanel） -->
      <KnowledgeTreeNav :selected-id="navNodeId" @select="handleKnowledgeNodeSelect" @context-change="handleContextChange" />

      <!-- 右侧：工具栏 + 列表区 -->
      <div class="ql-main">
    <!-- ===== Apple风格吸顶工具栏 ===== -->
    <div class="ql-sticky-bar">
      <!-- ===== 单行一体化响应式 Header 工具栏 ===== -->
      <div class="ql-header-bar">
        <!-- 1. 左侧：状态切换 Segmented Tab -->
        <div class="ql-seg-ctrl">
          <button
            v-for="tab in statusTabs"
            :key="tab.value"
            class="ql-seg-item"
            :class="{ active: currentStatus === tab.value }"
            @click="switchStatus(tab.value)"
          >
            <AppIcon :name="tab.icon" :size="14" class="ql-seg-icon" />
            <span class="ql-seg-label">{{ tab.label }}</span>
            <span
              v-if="tab.value === 'pending' && pendingReviewCount > 0"
              class="ql-seg-badge"
            >{{ pendingReviewCount > 99 ? '99+' : pendingReviewCount }}</span>
          </button>
        </div>

        <!-- 2. 中间：弹性伸缩搜索框 -->
        <div class="ql-search-wrap">
          <AppIcon name="search" :size="14" class="ql-search-icon" />
          <input
            v-model="query.keyword"
            class="ql-search-input"
            placeholder="搜索题目（输入即搜）"
            @input="onSearchInput"
            @keydown.enter="onSearchSubmit"
          />
        </div>

        <!-- 3. 右侧：操作区（筛选 + 试题篮 + 新建题目 + 统计） -->
        <div class="ql-header-actions">
          <button class="ql-filter-btn" :class="{ active: showFilter || hasAnyFilter }" @click="toggleFilter">
            <AppIcon name="filter" :size="14" />
            <span>筛选</span>
            <span v-if="hasAnyFilter" class="ql-filter-dot"></span>
          </button>

          <button
            v-if="basket.count.value > 0"
            class="ql-basket-btn"
            @click="toast.info(`试题篮中有 ${basket.count.value} 道题目`)"
          >
            <AppIcon name="shopping-cart" :size="15" />
            <span class="ql-basket-count">{{ basket.count.value }}</span>
          </button>

          <button class="ql-new-btn" @click="$router.push('/questions/new')">
            <AppIcon name="plus" :size="15" />
            <span>新建题目</span>
          </button>

          <span class="ql-status-text">共 <strong>{{ totalCount }}</strong> 道</span>
        </div>
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
            <!-- 来源角标：绝对定位，贴左上角边缘 -->
            <span v-if="sourceMeta(card)" class="q-source-badge" :title="sourceMeta(card)">
              {{ sourceMeta(card) }}
            </span>

            <!-- 学校角标：绝对定位，贴右上角边缘（与来源角标镜像） -->
            <span v-if="schoolName(card)" class="q-school-tag" :title="schoolName(card)">
              <AppIcon name="bookmark" :size="11" :stroke="2" />
              <span class="q-school-name">{{ schoolName(card) }}</span>
            </span>

            <!-- Row 1: 属性标签 -->
            <div class="q-card-header">
              <div class="q-card-tags">
                <AppBadge :color="typeBadgeColor(card.question_type)" class="flex-shrink-0">
                  {{ typeLabel(card.question_type) }}
                </AppBadge>
                <AppBadge :color="diffBadgeColor(card.difficulty)" class="flex-shrink-0">
                  {{ diffLabel(card.difficulty) }}
                </AppBadge>
                <AppBadge :color="statusBadgeColor(card.status)" class="flex items-center gap-1 flex-shrink-0">
                  <AppIcon :name="statusIcon(card.status)" :size="11" :stroke="2" />
                  {{ statusLabel(card.status) }}
                </AppBadge>
              </div>
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

            <!-- Row 3: Footer — 知识点（流式自适应 + hover-expand 多行展开） + 操作按钮 -->
            <div class="q-card-footer">
              <div
                class="q-footer-kp"
                :class="{ 'has-more': kpExpandIds.has(card.id) }"
                @mouseenter="updateKpExpandState(card.id, $event)"
              >
                <!-- 流式标签行：全部知识点横向排列，溢出折行后被单行高度隐藏 -->
                <span class="q-kp-flow">
                  <span
                    v-for="kn in card.knowledgeNodes"
                    :key="kn.id"
                    class="q-kp-tag"
                    :class="kpTagClass(kn.kind)"
                    :title="kn.name"
                  >{{ kn.name }}</span>
                  <span v-if="card.knowledgeNodes.length === 0" class="q-kp-text-empty">未关联知识点</span>
                </span>

                <!-- 悬停展开面板：绝对定位，向上弹出，多行完整渲染所有知识点 -->
                <!-- 显示由 .has-more:hover CSS 控制（真实溢出检测驱动 has-more） -->
                <div class="q-kp-expand-panel">
                  <span
                    v-for="kn in card.knowledgeNodes"
                    :key="kn.id"
                    class="q-kp-tag"
                    :class="kpTagClass(kn.kind)"
                    :title="kn.name"
                  >{{ kn.name }}</span>
                </div>
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
    </div>
  </div>


</template>

<script setup lang="ts">
import { ref, reactive, computed, onMounted, onBeforeUnmount, watch, nextTick, type ComponentPublicInstance } from 'vue'
import { useRouter } from 'vue-router'
import { questionApi, type QuestionSummary, type QuestionDetail, type QuestionQuery, type GradeLevel, type SemesterType, type ExamType, type KnowledgeNodeSummary } from '@/api/client'
import LatexRender from '@/components/LatexRender.vue'
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
  statusIcon,
  statusBadgeColor,
} from '@/utils/questionDisplay'

const router = useRouter()
const toast = useToast()
const space = useSpaceStore()
const basket = useQuestionBasket()

// 左侧知识树导航选中的节点 ID（空字符串 = 全部题目）
const navNodeId = ref('')

// ===== 状态切换 Segmented Tab =====
const statusTabs = [
  { label: '全部', value: 'ALL', icon: 'list' },
  { label: '已发布', value: 'published', icon: 'check' },
  { label: '草稿', value: 'draft', icon: 'pencil' },
  { label: '待审核', value: 'pending', icon: 'clock' },
] as const

const currentStatus = ref<string>('ALL')
const pendingReviewCount = ref(0)
const totalCount = ref(0)

function switchStatus(value: string) {
  currentStatus.value = value
  // 同步到 query.status（ALL → undefined 表示不过滤）
  query.status = value === 'ALL' ? undefined : (value as any)
  // 同步到矩阵筛选面板的 UI 状态
  filters.status = value === 'ALL' ? '__all' : value
  page.value = 1
  fetchList()
}

// 获取待审核数量（独立轻量请求，不干扰列表加载）
async function fetchPendingCount() {
  try {
    const res = await questionApi.list({
      ...query,
      status: 'pending' as any,
      page: 1,
      page_size: 1,
    })
    // 后端如返回 total 字段直接取，否则用 length >= 1 标识有待审核
    const total = (res as any).total ?? (res as any).pagination?.total
    if (typeof total === 'number') {
      pendingReviewCount.value = total
    } else {
      // 回退策略：有数据就标1，无数据标0
      pendingReviewCount.value = res.data?.length > 0 ? res.data.length : 0
    }
  } catch {
    pendingReviewCount.value = 0
  }
}

// 左侧树节点点击 → 同步到 query 并触发表格刷新（默认包含子孙节点）
function handleKnowledgeNodeSelect(nodeId: string) {
  navNodeId.value = nodeId
  query.knowledge_node_ids = nodeId ? [nodeId] : undefined
  query.include_descendants = nodeId ? true : undefined
  page.value = 1
  fetchList()
}

// 左侧学段/学科切换 → 同步到 query 并重置页码刷新列表
function handleContextChange(payload: { stage: string; subject: string }) {
  query.stage = payload.stage
  query.subject = payload.subject
  navNodeId.value = '' // 切换学段/学科后清空节点选中
  query.knowledge_node_ids = undefined
  query.include_descendants = undefined
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
  currentStatus.value = 'ALL'
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
  tags: { id: string; name: string; category: string }[]
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

// —— 知识点 Hover 面板：真实溢出检测（替代 length>2 启发式）——
// mouseenter 时用 scrollHeight > clientHeight 判断标签是否真正被隐藏，
// 未溢出则静默无视（不弹面板），溢出才标记 has-more 允许面板显示。
const kpExpandIds = ref<Set<string>>(new Set())
function updateKpExpandState(cardId: string, e: MouseEvent) {
  const flow = (e.currentTarget as HTMLElement).querySelector('.q-kp-flow') as HTMLElement | null
  const overflowed = !!flow && flow.scrollHeight > flow.clientHeight
  const next = new Set(kpExpandIds.value)
  if (overflowed) next.add(cardId)
  else next.delete(cardId)
  kpExpandIds.value = next
}

/** 知识点标签按维度着色：chapter→灰、knowledge→蓝、ability→紫 */
function kpTagClass(kind: string): string {
  if (kind === 'chapter') return 'kp-kind-chapter'
  if (kind === 'ability') return 'kp-kind-method'
  return 'kp-kind-knowledge'
}

const query = reactive<QuestionQuery>({
  keyword: '',
  question_type: undefined,
  difficulty: undefined,
  status: undefined,
  knowledge_node_ids: [],
  include_descendants: true,
  space_id: space.currentSpaceId || undefined,
  // 从 localStorage 恢复学段/学科（与 KnowledgeTreeNav 初始值保持同步）
  stage: (localStorage.getItem('nav_selected_stage') as string) || 'junior',
  subject: (localStorage.getItem('nav_selected_subject') as string) || 'math',
  page: 1,
  page_size: pageSize,
})

// 难度数值 1-5 → 字符串（兼容 diffLabel/diffBadgeColor 显示函数）
function difficultyNumToString(n: number): string {
  if (n <= 2) return 'easy'
  if (n === 3) return 'medium'
  return 'hard'
}

/// 元数据行左侧：年份 · 年级 · 省份市区 · 考试类型（剔除学校和学段，· 分隔，过滤空值）
function sourceMeta(card: QuestionCard): string {
  const m = card.metadata ?? {}
  const str = (v: unknown) => String(v ?? '').trim()

  // 1. 年份
  const year = str(m.year)

  // 2. 年级（移除学段，仅保留具体年级如"高一"）
  const grade = str(m.grade)

  // 3. 省份市区
  const province = str(m.region_province)
  const city = str(m.region_city)
  const region = [province, city].filter(Boolean).join('')

  // 4. 考试类型（高考模拟时优先显示细分模考类型：一模/二模/三模）
  const sourceType = str(m.source_type) || str(m.exam_type)
  const subSourceType = str(m.sub_source_type)
  const exam = (sourceType === '高考模拟' && subSourceType) ? subSourceType : sourceType

  return [year, grade, region, exam].filter(Boolean).join(' · ')
}

/// 学校标签：从 school 类别标签提取（多个用 / 连接，无则空串）
function schoolName(card: QuestionCard): string {
  const schools = (card.tags ?? [])
    .filter(t => t.category === 'school')
    .map(t => String(t.name ?? '').trim())
    .filter(Boolean)
  return schools.join(' / ')
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
  // 状态：同步到 segmented tab
  query.status = filters.status === '__all' ? undefined : (filters.status as any)
  currentStatus.value = filters.status === '__all' ? 'ALL' : filters.status
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
    // 捕获总数：优先取后端 PageResult.total，否则回退到当前已加载条数
    totalCount.value = (res as any).total ?? summaries.length
    hasMore.value = summaries.length >= pageSize

    // 并发获取每道题的详情
    const details = await Promise.all(
      summaries.map((s) => questionApi.get(s.id).catch(() => null))
    )

    cardList.value = summaries.map((s, i) => {
      const detail: QuestionDetail | null = details[i]?.data ?? null
      const meta = (detail?.metadata ?? {}) as Record<string, unknown>
      const province = String(meta.region_province ?? '').trim()
      const city = String(meta.region_city ?? '').trim()
      return {
        id: s.id,
        stem: s.stem,
        question_type: s.question_type,
        difficulty: difficultyNumToString(s.difficulty),
        status: s.status,
        grade_level: s.grade_level,
        semester: null,
        source: detail?.source ?? null,
        school_source: detail?.source ?? null,
        exam_type: null,
        // B2 后 metadata 长尾字段：year / region_province+region_city（旧 academic_year/exam_region 已废弃）
        region: [province, city].filter(Boolean).join('') || null,
        year: meta.year ? String(meta.year) : null,
        metadata: meta,
        tags: detail?.tags ?? [],
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

onMounted(() => {
  fetchList()
  fetchPendingCount()
})
onBeforeUnmount(() => {
  if (searchTimer) clearTimeout(searchTimer)
  if (layoutDebounce) clearTimeout(layoutDebounce)
  resizeObservers.forEach(ro => ro.disconnect())
})
</script>

<style scoped>
/* ===== 融合单行工具栏 (Header Bar) ===== */
.ql-header-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 8px 16px;
  border-bottom: 1px solid var(--divider);
  background: var(--bg-primary);
}

/* 右侧操作聚合区 */
.ql-header-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

/* 筛选激活小原点 */
.ql-filter-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--accent);
}

/* 统计文本（与右侧操作并排） */
.ql-status-text {
  font-size: 12.5px;
  color: var(--text-muted);
  white-space: nowrap;
  letter-spacing: -0.01em;
  line-height: 1;
  margin-left: 4px;
}

.ql-status-text strong {
  color: var(--text-secondary);
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  margin: 0 2px;
}

/* Segmented Control — Apple 风格胶囊分段控制器 */
.ql-seg-ctrl {
  display: inline-flex;
  align-items: center;
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  border-radius: 10px;
  padding: 3px;
  gap: 2px;
}

.ql-seg-item {
  position: relative;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 14px;
  border: none;
  background: transparent;
  border-radius: 7px;
  font-size: 13px;
  font-weight: 500;
  color: var(--text-secondary);
  cursor: pointer;
  transition: all 0.28s cubic-bezier(0.4, 0, 0.2, 1);
  white-space: nowrap;
  user-select: none;
}

.ql-seg-icon {
  flex-shrink: 0;
  opacity: 0.7;
  transition: opacity 0.28s ease;
}

.ql-seg-item:hover:not(.active) {
  color: var(--text-primary);
  background: var(--bg-hover);
}

.ql-seg-item:hover:not(.active) .ql-seg-icon {
  opacity: 0.9;
}

.ql-seg-item.active {
  background: var(--bg-canvas);
  color: var(--text-primary);
  font-weight: 600;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.08), 0 1px 2px rgba(0, 0, 0, 0.06);
}

.ql-seg-item.active .ql-seg-icon {
  opacity: 1;
  color: var(--accent);
}

[data-theme='dark'] .ql-seg-item.active {
  background: var(--bg-active);
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.3);
}

.ql-seg-label {
  line-height: 1;
}

/* 待审核数字徽标 — 红色小圆角，与文字保持间距 */
.ql-seg-badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 18px;
  height: 17px;
  margin-left: 4px;
  padding: 0 5px;
  border-radius: 9999px;
  background: var(--danger);
  color: #fff;
  font-size: 10.5px;
  font-weight: 700;
  line-height: 1;
  letter-spacing: 0.02em;
  box-shadow: 0 0 0 2px var(--bg-input);
  animation: ql-badge-pop 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
}

.ql-seg-item.active .ql-seg-badge {
  box-shadow: 0 0 0 2px var(--bg-canvas);
}

[data-theme='dark'] .ql-seg-item.active .ql-seg-badge {
  box-shadow: 0 0 0 2px var(--bg-active);
}

@keyframes ql-badge-pop {
  0% { transform: scale(0); opacity: 0; }
  100% { transform: scale(1); opacity: 1; }
}

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
  min-width: 600px; /* 防止 LaTeX 公式与题目选项被挤压变形 */
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

/* 搜索框 — 弹性自适应宽度，在中间自然拉伸 */
.ql-search-wrap {
  flex: 1;
  max-width: 320px;
  min-width: 150px;
  display: flex;
  align-items: center;
  gap: 0;
  height: 34px;
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  border-radius: 9px;
  transition: var(--transition-fast);
  overflow: hidden;
  flex-shrink: 1;
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
  padding: 0 12px;
  height: 100%;
}

.ql-search-input::placeholder {
  color: var(--text-muted);
}

/* 独立筛选按钮（从搜索框分离） */
.ql-filter-btn {
  display: flex;
  align-items: center;
  gap: 5px;
  height: 36px;
  padding: 0 14px;
  border-radius: 10px;
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  color: var(--text-secondary);
  font-size: 13px;
  font-weight: 500;
  transition: var(--transition-fast);
  white-space: nowrap;
  flex-shrink: 0;
  cursor: pointer;
}

.ql-filter-btn:hover {
  border-color: var(--accent);
  color: var(--accent);
  background: var(--accent-light);
}

.ql-filter-btn:active {
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

/* ===== 筛选面板展开/折叠动画 (max-height 方案, 确保折叠时高度归零) ===== */
.ql-filter-collapse {
  max-height: 0;
  overflow: hidden;
  opacity: 0;
  transition: max-height 0.35s cubic-bezier(0.32, 0.72, 0, 1),
    opacity 0.25s ease;
}

.ql-filter-collapse.is-open {
  max-height: 600px;
  opacity: 1;
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
  position: relative; /* 来源角标 & hover-expand 面板绝对定位基础 */
  background: var(--bg-card);
  border-radius: 16px; /* rounded-2xl */
  border: 1px solid transparent;
  box-shadow: var(--shadow-sm);
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

/* ---- 来源角标：贴左上角边缘，绝对定位（与学校角标镜像统一） ---- */
.q-source-badge {
  position: absolute;
  top: 0;
  left: 0;
  max-width: 70%;
  padding: 4px 16px 4px 12px;
  font-size: 11px;
  line-height: 1.4;
  color: var(--text-muted);
  background: rgba(100, 116, 139, 0.08); /* 极浅灰蓝底，融入卡片 */
  border-radius: 16px 0 6px 0; /* 左上外角贴合卡片 16px 圆角，右下小圆角 */
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  pointer-events: none; /* 不阻挡卡片点击 */
  z-index: 2;
}

[data-theme='dark'] .q-source-badge,
[data-theme='dark'] .q-school-tag {
  background: rgba(148, 163, 184, 0.12); /* dark 下浅灰蓝微亮化，保持无边框 */
}

/* ---- Header Row 1: 属性标签 ---- */
.q-card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 26px 20px 10px; /* 顶部加大：避开左上角来源角标 */
  border-bottom: 1px solid var(--divider);
  gap: 12px;
}

.q-card-tags {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap; /* 窄屏允许徽标换行，不硬裁（知识点已移出，最多两行） */
  min-width: 0;
  overflow: hidden;
}

/* ---- 学校角标：贴右上角边缘，绝对定位（与来源角标镜像统一） ---- */
.q-school-tag {
  position: absolute;
  top: 0;
  right: 0;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  max-width: 60%;
  padding: 4px 12px 4px 16px;
  font-size: 11px;
  line-height: 1.4;
  color: var(--text-muted);
  background: rgba(100, 116, 139, 0.08); /* 极浅灰蓝底，与来源角标一致 */
  border-radius: 0 16px 0 6px; /* 右上外角贴合卡片 16px 圆角，左下小圆角（镜像） */
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  pointer-events: none;
  z-index: 2;
}

.q-school-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 知识点标签（底部瘦身版）：极浅底色 + 无边框 + 小字号（内外一致） */
.q-kp-tag {
  display: inline-flex;
  align-items: center;
  flex-shrink: 1; /* 允许收缩，避免长标签把 +N 挤出容器 */
  min-width: 0;
  max-width: 160px;
  padding: 2px 6px;
  border-radius: 6px;
  font-size: 12px;
  line-height: 16px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  background: rgba(0, 113, 227, 0.06); /* 极浅蓝底，无边框 */
  color: var(--accent);
}

/* ── 按维度着色（与编辑页 AttributeSidePanel .is-chapter/.is-knowledge/.is-method 统一） ── */

/* 章节 (chapter)：灰色调 */
.q-kp-tag.kp-kind-chapter {
  background: var(--bg-active, #f0f0f5);
  color: var(--text-secondary, #6b7280);
}

/* 知识点 (knowledge)：亮蓝色调 */
.q-kp-tag.kp-kind-knowledge {
  background: var(--accent-light, rgba(37, 99, 235, 0.08));
  color: var(--accent, #2563eb);
}

/* 解题方法 (ability)：紫色调 */
.q-kp-tag.kp-kind-method {
  background: var(--purple-light, #f3e8ff);
  color: var(--purple, #8b5cf6);
}

/* +N 折叠徽标：极浅灰蓝底变体，hover 时提示可展开 */
.q-kp-text-empty {
  font-size: 12px;
  color: var(--text-muted);
  opacity: 0.6;
  white-space: nowrap;
}

/* ---- 知识点 Hover-Expand 面板 ---- */
/* 废弃旧 kp-tooltip Teleport 气泡，改用卡片内绝对定位的多行展开面板 */
/* 流式标签行：固定单行高度 + 允许折行 + 溢出隐藏 —— 屏幕越宽展示越多 */
/* 注意：height 24px = 标签 20px（16 行高 + 2px×2 padding）+ 行间 gap 4px，改动需同步 */
.q-kp-flow {
  display: flex;
  flex-wrap: wrap;
  align-content: flex-start;
  gap: 4px;
  height: 24px; /* 单行高度：装不下的标签折到第二行后被隐藏 */
  overflow: hidden;
  flex: 1;
  min-width: 0;
  /* 右侧渐隐遮罩：标签顶到右缘被截断时产生"逐渐消失"的褪色暗示 */
  -webkit-mask-image: linear-gradient(to right, black 85%, transparent 100%);
  mask-image: linear-gradient(to right, black 85%, transparent 100%);
}

.q-footer-kp {
  position: relative; /* 展开面板定位基础 */
  display: inline-flex;
  align-items: center;
  gap: 4px;
  flex: 1;
  min-width: 0;
  overflow: visible; /* 允许展开面板溢出 */
}

/* 展开面板：绝对定位，向上弹出（避免被下一张卡片遮挡），完整多行渲染 */
.q-kp-expand-panel {
  position: absolute;
  bottom: calc(100% + 6px); /* 向上生长，与知识点行保持 6px 视觉间距 */
  left: 0;
  right: 0;
  z-index: 100; /* 浮于当前卡片所有内容之上 */
  display: none;
  flex-wrap: wrap;
  gap: 4px;
  padding: 12px 16px;
  background: var(--bg-card, #ffffff); /* 卡片底色（亮色为纯白），完美遮挡下方内容 */
  border: 1px solid #f1f5f9; /* 极浅细边框，弱化轮廓 */
  border-radius: 8px;
  box-shadow: 0 10px 25px -5px rgba(0, 0, 0, 0.1), 0 8px 10px -6px rgba(0, 0, 0, 0.1); /* 弥散悬浮阴影 */
  max-height: 280px;
  overflow-y: auto;
  pointer-events: auto;
}

/* 悬停桥接：6px 透明区域属于触发盒，鼠标平滑移入面板不丢 hover */
.q-footer-kp.has-more::before {
  content: '';
  position: absolute;
  bottom: 100%;
  left: 0;
  right: 0;
  height: 6px;
}

/* 悬停触发展开：hover 整个 .q-footer-kp（需有 has-more 标记） */
.q-footer-kp.has-more:hover .q-kp-expand-panel,
.q-footer-kp.has-more:focus-within .q-kp-expand-panel {
  display: flex;
}

/* 展开面板内的标签样式：复用 .q-kp-tag，但取消 max-width 限制 */
.q-kp-expand-panel .q-kp-tag {
  max-width: 100%;
}

[data-theme='dark'] .q-kp-expand-panel {
  background: var(--bg-elevated, #1e1e20);
  border-color: rgba(255, 255, 255, 0.08);
  box-shadow: 0 10px 25px -5px rgba(0, 0, 0, 0.4), 0 8px 10px -6px rgba(0, 0, 0, 0.3);
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
  justify-content: space-between;
  padding: 10px 20px;
  border-top: 1px solid var(--divider);
  gap: 12px;
}

/* 左侧知识点区：样式已移至 .q-kp-expand-panel 附近（hover-expand 重构） */

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
  padding: 5px 10px;
  border-radius: var(--radius-sm);
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  color: var(--text-secondary);
  font-size: 12px;
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
@media (max-width: 960px) {
  .ql-header-bar {
    flex-wrap: wrap;
    gap: 8px 12px;
    padding: 8px 12px;
  }
  .ql-search-wrap {
    order: 3;
    flex: 1 0 100%;
    max-width: 100%;
  }
  .ql-status-text {
    display: none;
  }
}

@media (max-width: 640px) {
  .q-card-header {
    padding: 26px 14px 10px; /* 移动端同步顶部避让来源角标 */
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
