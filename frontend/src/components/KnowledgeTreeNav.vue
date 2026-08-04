<script setup lang="ts">
/**
 * KnowledgeTreeNav — 题库列表左侧常驻知识树导航面板
 *
 * 三层专业导航架构：
 *   1. 顶层联动：学段（初中 / 高中） + 学科（数学 / 物理）
 *   2. 中部 Tabs：章节选题 / 知识点选题 / 解题方法（Segmented Control 风格）
 *   3. 底部动态树：依据 stage + subject + treeMode 组合渲染
 *
 * 视觉规范：260px 宽，右侧 1px 分割线，扁平化树渲染，全部 CSS 变量
 */
import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import { AppIcon, AppEmpty } from '@/components/ui'
import {
  knowledgeTreeApi,
  knowledgeNodeApi,
  type KnowledgeTree,
  type KnowledgeTreeKind,
  type KnowledgeNodeTreeNode,
} from '@/api/client'
import { unwrapTreeResponse } from '@/composables/useKnowledgeTreeCache'

const props = withDefaults(
  defineProps<{
    /** 当前选中的节点 ID（空字符串表示未选/全部）— 仅用于视觉反馈 */
    selectedId?: string
    /** 锁定知识树 ID（不传则允许在多棵树之间切换） */
    treeId?: string
  }>(),
  { selectedId: '', treeId: '' },
)

const emit = defineEmits<{
  select: [nodeId: string]
  /** 学段或学科切换时通知父组件（父组件需据此刷新题目列表） */
  contextChange: [payload: { stage: string; subject: string }]
}>()

// ─── 学段 / 学科 / 模式 联动状态 ───────────────────────────────────────
type Stage = 'junior' | 'senior'
type Subject = 'math' | 'physics'
type TreeMode = 'chapter' | 'knowledge' | 'method'

const STAGES: { key: Stage; label: string }[] = [
  { key: 'junior', label: '初中' },
  { key: 'senior', label: '高中' },
]
const SUBJECTS: { key: Subject; label: string }[] = [
  { key: 'math', label: '数学' },
  { key: 'physics', label: '物理' },
]
/** 模式 → 后端 KnowledgeTreeKind 映射；method 暂无原生 kind，复用 ability 语义兜底 */
const MODES: { key: TreeMode; label: string; kind: KnowledgeTreeKind | null }[] = [
  { key: 'chapter', label: '章节', kind: 'chapter' },
  { key: 'knowledge', label: '知识点', kind: 'knowledge' },
  { key: 'method', label: '解题方法', kind: 'ability' },
]

const currentStage = ref<Stage>(
  (localStorage.getItem('nav_selected_stage') as Stage) || 'junior',
)
const currentSubject = ref<Subject>(
  (localStorage.getItem('nav_selected_subject') as Subject) || 'math',
)
const treeMode = ref<TreeMode>('chapter')

// 学段 / 科目持久化：切换时即时写入 localStorage，刷新或重新进入时恢复
watch(currentStage, (val) => {
  localStorage.setItem('nav_selected_stage', val)
})
watch(currentSubject, (val) => {
  localStorage.setItem('nav_selected_subject', val)
})

// ─── 响应式：窗口宽度监测 ──────────────────────────────────────────────
const winWidth = ref(window.innerWidth)
const isDrawerMode = computed(() => winWidth.value < 1280)
/** 抽屉模式下，树面板是否展开（默认收起） */
const drawerOpen = ref(false)

function onResize() {
  winWidth.value = window.innerWidth
  if (isDrawerMode.value) {
    drawerOpen.value = false
  } else {
    // 恢复全尺寸大屏 (>= 1280px) 时，知识树导航默认展开且可见
    collapsed.value = false
  }
}

onUnmounted(() => window.removeEventListener('resize', onResize))

// ─── 状态 ──────────────────────────────────────────────────────────────
// 知识树收起状态持久化：仅在全尺寸模式且用户显式折叠时持久化
const collapsed = ref(
  window.innerWidth >= 1280
    ? localStorage.getItem('knowledge-tree-collapsed') === 'true'
    : false,
)
watch(collapsed, (val) => {
  if (!isDrawerMode.value) {
    localStorage.setItem('knowledge-tree-collapsed', String(val))
  }
})

/** 统一判断当前知识树是否处于折叠/关闭状态 */
const isCollapsed = computed(() => {
  return isDrawerMode.value ? !drawerOpen.value : collapsed.value
})
const trees = ref<KnowledgeTree[]>([])
const activeTreeId = ref<string>('')
const treeData = ref<KnowledgeNodeTreeNode[]>([])
const loading = ref(false)

// 内部选中态（用于即时视觉反馈，无需等待父组件回传）
const internalSelected = ref('')

// 展开/折叠节点 ID 集合
const expandedIds = ref<Set<string>>(new Set())

// ─── 计算属性 ─────────────────────────────────────────────────────────
/** 当前模式期望的 tree.kind（null 表示不按 kind 过滤） */
const expectedKind = computed<KnowledgeTreeKind | null>(
  () => MODES.find((m) => m.key === treeMode.value)?.kind ?? null,
)

// ─── tree.code 命名规则：{subject}_{mode}_{stage} ─────────────────────
// 后端实际 code：math_knowledge_high / math_method_high / math_chapter_high
// 学段映射：junior→'junior'，senior→'high'（后端用 high 表示高中）
const STAGE_CODE: Record<Stage, string> = {
  junior: 'junior',
  senior: 'high',
}
const SUBJECT_CODE: Record<Subject, string> = {
  math: 'math',
  physics: 'physics',
}
const MODE_CODE: Record<TreeMode, string> = {
  chapter: 'chapter',
  knowledge: 'knowledge',
  method: 'method',
}

/** 期望的 tree code，如 'math_chapter_high'（高中数学章节树） */
const expectedCode = computed(() => {
  const subj = SUBJECT_CODE[currentSubject.value]
  const mode = MODE_CODE[treeMode.value]
  const stage = STAGE_CODE[currentStage.value]
  return `${subj}_${mode}_${stage}`
})

/** 当前模式下可用的树（按 kind 过滤） */
const availableTrees = computed<KnowledgeTree[]>(() =>
  expectedKind.value === null
    ? trees.value
    : trees.value.filter((t) => t.kind === expectedKind.value),
)

/** 物理学科 / 解题方法 等后端尚未覆盖时的兜底提示 */
const emptyHint = computed(() => {
  if (currentSubject.value === 'physics') return '物理学科资源敬请期待'
  if (treeMode.value === 'method') return '暂无解题方法树'
  if (availableTrees.value.length === 0) return '当前模式暂无知识树'
  return '无知识点'
})

interface FlatItem {
  node: KnowledgeNodeTreeNode
  depth: number
}

/** 扁平化展示列表：依据 expandedIds 决定是否递归子节点 */
const flatList = computed<FlatItem[]>(() => {
  const result: FlatItem[] = []

  function walk(nodes: KnowledgeNodeTreeNode[], depth: number) {
    for (const n of nodes) {
      result.push({ node: n, depth })
      if (expandedIds.value.has(n.id) && n.children.length > 0) {
        walk(n.children, depth + 1)
      }
    }
  }

  walk(treeData.value, 0)
  return result
})

// ─── 方法 ──────────────────────────────────────────────────────────────
function toggleCollapse() {
  if (isDrawerMode.value) {
    // 抽屉模式：切换抽屉开关
    drawerOpen.value = !drawerOpen.value
  } else {
    // 正常模式：收起 / 展开面板宽度
    collapsed.value = !collapsed.value
  }
}

function closeDrawer() {
  drawerOpen.value = false
}

function toggleExpand(id: string) {
  const next = new Set(expandedIds.value)
  if (next.has(id)) {
    next.delete(id)
  } else {
    next.add(id)
  }
  expandedIds.value = next
}

function hasChildren(n: KnowledgeNodeTreeNode): boolean {
  return n.children.length > 0
}

function selectNode(id: string) {
  // 点击已选节点 → 取消选择；否则切换到新节点
  const next = internalSelected.value === id ? '' : id
  internalSelected.value = next
  emit('select', next)
}

function selectAll() {
  internalSelected.value = ''
  emit('select', '')
}

function isSelected(id: string): boolean {
  return internalSelected.value === id
}

/** 切换学段 / 学科 / 模式：清空选中 + 重新加载 */
function setStage(s: Stage) {
  if (currentStage.value === s) return
  currentStage.value = s
  emit('contextChange', { stage: s, subject: currentSubject.value })
}
function setSubject(s: Subject) {
  if (currentSubject.value === s) return
  currentSubject.value = s
  emit('contextChange', { stage: currentStage.value, subject: s })
}
function setMode(m: TreeMode) {
  if (treeMode.value === m) return
  treeMode.value = m
}

// ─── 数据加载 ─────────────────────────────────────────────────────────
/**
 * 加载知识树列表 + 自动选中当前 stage/subject/mode 对应的树
 *
 * 匹配优先级：
 *   1. props.treeId（父组件锁定）
 *   2. 按 expectedCode 精确匹配（如 'math_chapter_high'）
 *   3. 按 expectedKind 兜底取第一棵
 */
async function loadTrees() {
  try {
    const res = await knowledgeTreeApi.list()
    trees.value = res.data

    let matched: KnowledgeTree | undefined
    if (props.treeId) {
      // 父组件锁定 treeId
      matched = trees.value.find((t) => t.id === props.treeId)
    } else {
      // 严格按 expectedCode 精确匹配（如 'math_chapter_high'）
      // 找不到则清空，由 emptyHint 显示无数据提示
      matched = trees.value.find((t) => t.code === expectedCode.value)
    }

    if (matched) {
      activeTreeId.value = matched.id
    } else {
      // 当前模式无匹配树 → 清空，由 emptyHint 兜底
      activeTreeId.value = ''
      treeData.value = []
    }
  } catch (e) {
    console.error('[TreeNav] 加载知识树列表失败', e)
  }
}

async function loadTreeData() {
  if (!activeTreeId.value) {
    treeData.value = []
    return
  }
  loading.value = true
  try {
    const res = await knowledgeNodeApi.getTree(activeTreeId.value)
    treeData.value = unwrapTreeResponse(res.data)
    // 默认折叠：初始不展开任何节点（削顶后顶层即真实内容根，保持清爽视图）
    expandedIds.value = new Set()
  } catch (e) {
    console.error('[TreeNav] 加载知识点树失败', e)
  } finally {
    loading.value = false
  }
}

// ─── 侦听 ──────────────────────────────────────────────────────────────
watch(
  () => props.selectedId,
  (id) => {
    internalSelected.value = id || ''
  },
  { immediate: true },
)

watch(
  () => props.treeId,
  (newId) => {
    if (newId) activeTreeId.value = newId
  },
)

watch(activeTreeId, () => {
  loadTreeData()
})

// 学段 / 学科 / 模式 联动：清空选中 + 重新加载树
watch([currentStage, currentSubject, treeMode], () => {
  internalSelected.value = ''
  expandedIds.value = new Set()
  activeTreeId.value = ''
  treeData.value = []
  emit('select', '')
  loadTrees()
})

onMounted(async () => {
  window.addEventListener('resize', onResize, { passive: true })
  await loadTrees()
  if (activeTreeId.value) await loadTreeData()
})
</script>

<template>
  <div
    class="kt-nav-wrapper"
    :class="{
      'is-collapsed': isCollapsed,
      'is-drawer-mode': isDrawerMode,
      'drawer-open': isDrawerMode && drawerOpen,
    }"
  >
    <!-- 抽屉模式背景遮罩 -->
    <Transition name="kt-backdrop">
      <div
        v-if="isDrawerMode && drawerOpen"
        class="kt-drawer-backdrop"
        @click="closeDrawer"
      />
    </Transition>

    <!-- 实际侧栏：折叠时宽度为 0，内容隐藏 -->
    <aside class="kt-nav" :class="{ 'is-collapsed': isCollapsed }">
      <header class="kt-nav-header">
        <div class="kt-nav-title">
          <AppIcon name="list" :size="14" />
          <span>知识树导航</span>
        </div>
        <!-- Header 内嵌折叠按鈕：所有模式均可用 -->
        <button
          type="button"
          class="kt-header-toggle"
          :title="isDrawerMode ? '关闭导航' : (collapsed ? '展开导航' : '收起导航')"
          :aria-label="isDrawerMode ? '关闭导航' : (collapsed ? '展开导航' : '收起导航')"
          @click="toggleCollapse"
        >
          <AppIcon
            :name="isDrawerMode ? 'x' : 'chevron-left'"
            class="header-toggle-icon"
            :class="{ rotated: !isDrawerMode && collapsed }"
            :size="15"
          />
        </button>
      </header>

      <!-- 全部 + 树切换 + 列表 共用滚动区：使用 v-show="!isCollapsed" 彻底防止 Ghost DOM 挤压与溢出 -->
      <div v-show="!isCollapsed" class="kt-nav-body">
        <!-- ===== 顶部筛选区组：三行等宽无界 Tab + 底部分割线 ===== -->
        <div class="kt-filter-group">
          <!-- ===== 第 1 层：学段切换（等宽占满无界 Tab） ===== -->
          <div class="kt-stage-row" role="tablist" aria-label="学段切换">
            <button
              v-for="s in STAGES"
              :key="s.key"
              type="button"
              class="kt-stage-tab"
              :class="{ active: currentStage === s.key }"
              role="tab"
              :aria-selected="currentStage === s.key"
              @click="setStage(s.key)"
            >
              {{ s.label }}
            </button>
          </div>

          <!-- ===== 第 2 层：学科切换（等宽占满无界 Tab） ===== -->
          <div class="kt-subject-row" role="tablist" aria-label="学科切换">
            <button
              v-for="s in SUBJECTS"
              :key="s.key"
              type="button"
              class="kt-subject-tag"
              :class="{ active: currentSubject === s.key }"
              role="tab"
              :aria-selected="currentSubject === s.key"
              @click="setSubject(s.key)"
            >
              {{ s.label }}
            </button>
          </div>

          <!-- ===== 第 3 层：模式切换（等宽占满无界 Tab） ===== -->
          <div class="kt-mode-segment" role="tablist" aria-label="选题模式">
            <button
              v-for="m in MODES"
              :key="m.key"
              type="button"
              class="kt-mode-item"
              :class="{ active: treeMode === m.key }"
              role="tab"
              :aria-selected="treeMode === m.key"
              @click="setMode(m.key)"
            >
              {{ m.label }}
            </button>
          </div>
        </div>

        <!-- "全部"快捷项 -->
        <button
          type="button"
          class="kt-nav-all"
          :class="{ active: !internalSelected }"
          @click="selectAll"
        >
          <AppIcon name="grid" :size="13" />
          <span>全部题目</span>
        </button>

        <!-- 树列表 -->
        <div class="kt-nav-list">
          <div v-if="loading" class="kt-nav-loading">加载中…</div>
          <AppEmpty v-else-if="flatList.length === 0" :description="emptyHint" />
          <template v-else>
            <div
              v-for="item in flatList"
              :key="item.node.id"
              class="kt-nav-row"
              :class="{ selected: isSelected(item.node.id) }"
              :style="{ paddingLeft: 8 + item.depth * 16 + 'px' }"
              @click="selectNode(item.node.id)"
            >
              <!-- 层级虚线引导线：根据 depth 渲染 depth 条垂直虚线 -->
              <span
                v-for="d in item.depth"
                :key="'guide-' + d"
                class="indent-guide"
                :style="{ left: 8 + (d - 1) * 16 + 'px' }"
              />
              <!-- 节点展开/折叠旋转按钮 -->
              <button
                v-if="hasChildren(item.node)"
                type="button"
                class="row-expand"
                :class="{ 'is-expanded': expandedIds.has(item.node.id) }"
                :title="expandedIds.has(item.node.id) ? '折叠' : '展开'"
                :aria-label="expandedIds.has(item.node.id) ? '折叠子知识点' : '展开子知识点'"
                @click.stop="toggleExpand(item.node.id)"
              >
                <AppIcon
                  name="chevron-right"
                  class="row-expand-icon"
                  :size="12"
                />
              </button>
              <span v-else class="row-dot" />

              <span class="row-name">{{ item.node.name }}</span>
              <span
                v-if="item.node.question_count > 0"
                class="row-count"
              >{{ item.node.question_count }}</span>
            </div>
          </template>
        </div>
      </div>
    </aside>

    <!-- 边缘悬浮 Toggle 把手：仅在正常模式（非抽屉）下显示，独立剥离在外层容器上 -->
    <button
      v-if="!isDrawerMode"
      type="button"
      class="kt-nav-edge-toggle"
      :class="{ 'is-collapsed': collapsed }"
      :title="collapsed ? '展开知识树导航' : '收起知识树导航'"
      :aria-label="collapsed ? '展开知识树导航' : '收起知识树导航'"
      @click="toggleCollapse"
    >
      <AppIcon
        name="chevron-left"
        class="toggle-chevron"
        :size="14"
      />
    </button>

    <!-- 抽屉模式：展开触发器（仅在 769px~1279px Mini 侧边栏模式且面板关闭时，紧贴侧边栏 64px 右边缘） -->
    <button
      v-if="winWidth >= 769 && isDrawerMode && !drawerOpen"
      type="button"
      class="kt-drawer-trigger"
      title="展开知识树导航"
      aria-label="展开知识树导航"
      @click="toggleCollapse"
    >
      <AppIcon name="chevron-right" :size="15" />
    </button>
  </div>
</template>

<style scoped>
/* ── 外层 wrapper：负责宽度平滑过渡 (260px -> 0px)，独立承载悬浮把手 ── */
.kt-nav-wrapper {
  position: relative;
  flex-shrink: 0;
  height: 100%;
  width: 260px;
  overflow: visible; /* 必须保持 visible，确保外部挂载的拉环按钮永远不被裁切 */
  transition: width 0.3s cubic-bezier(0.4, 0, 0.2, 1);
}

.kt-nav-wrapper.is-collapsed:not(.is-drawer-mode) {
  width: 0 !important;
  overflow: visible !important; /* 关键：wrapper 宽归零，但悬浮拉环按钮依然保留悬挂于视口分割线处 */
}

/* ── 实际侧栏：纯白浮动卡片（Apple 风格） ── */
.kt-nav {
  width: 260px;
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-sm);
  overflow: hidden;
  transition: width 0.3s cubic-bezier(0.4, 0, 0.2, 1), opacity 0.25s ease,
    box-shadow 0.28s ease;
}

/* 悬停时阴影加深，增强卡片浮动感 */
.kt-nav-wrapper:hover .kt-nav:not(.is-collapsed) {
  box-shadow: var(--shadow-md);
}

.kt-nav.is-collapsed:not(.is-drawer-mode) {
  width: 0 !important;
  opacity: 0;
  pointer-events: none;
  overflow: hidden !important;
}

/* ── 抽屉模式（width < 1280px）── */
/* wrapper 占位宽度归零，让右侧内容区独占全部空间 */
.kt-nav-wrapper.is-drawer-mode {
  width: 0 !important;
  overflow: visible; /* 让绝对/固定定位子元素溢出 */
}

/* 抽屉面板：固定浮层，从 mini 侧边栏右边缘滑入，赋予强烈右侧深层阴影 (shadow-2xl) */
.kt-nav-wrapper.is-drawer-mode .kt-nav {
  position: fixed;
  top: 0;
  left: var(--sidebar-mini-width, 64px); /* 与 mini 侧边栏右边缘精准对齐 */
  height: 100vh;
  z-index: 200;
  box-shadow: 10px 0 32px rgba(0, 0, 0, 0.22), 2px 0 8px rgba(0, 0, 0, 0.08); /* 强烈右侧立体阴影 */
  transform: translateX(-100%);
  transition:
    transform 0.32s cubic-bezier(0.4, 0, 0.2, 1),
    opacity 0.25s ease;
  opacity: 0;
  pointer-events: none;
}

/* 抽屉打开时滑入 */
.kt-nav-wrapper.is-drawer-mode.drawer-open .kt-nav {
  transform: translateX(0);
  opacity: 1;
  pointer-events: auto;
}

/* 抽屉模式下不需要普通 is-collapsed 的 0 宽度 */
.kt-nav-wrapper.is-drawer-mode.is-collapsed .kt-nav {
  width: 260px !important;
}

/* 背景遮罩 */
.kt-drawer-backdrop {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.35);
  backdrop-filter: blur(2px);
  -webkit-backdrop-filter: blur(2px);
  z-index: 199;
  cursor: pointer;
}

/* 遮罩动画 */
.kt-backdrop-enter-active,
.kt-backdrop-leave-active {
  transition: opacity 0.28s ease;
}
.kt-backdrop-enter-from,
.kt-backdrop-leave-to {
  opacity: 0;
}

/* ── Header 内嵌折叠/关闭按钮 ── */
.kt-header-toggle {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border-radius: 9999px; /* rounded-full 圆形按钮 */
  background: transparent;
  border: none;
  color: var(--text-muted);
  cursor: pointer;
  flex-shrink: 0;
  transition: background-color 0.18s ease, color 0.18s ease, transform 0.15s ease;
  outline: none;
}

.kt-header-toggle:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.kt-header-toggle:active {
  transform: scale(0.92);
}

.header-toggle-icon {
  transition: transform 0.25s cubic-bezier(0.4, 0, 0.2, 1);
}

.header-toggle-icon.rotated {
  transform: rotate(180deg);
}

/* ── 抽屉触发器：贴附在 Mini 侧边栏 (64px / w-16) 右边缘 (fixed left-[64px] top-1/2 -translate-y-1/2 z-50) ── */
.kt-drawer-trigger {
  position: fixed;
  top: 50%;
  left: 64px; /* 紧贴 Mini 侧边栏 64px (w-16) 的右边缘 */
  transform: translateY(-50%);
  width: 22px;
  height: 44px;
  border-radius: 0 12px 12px 0;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-left: none;
  color: var(--text-muted);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  z-index: 50;
  box-shadow: 3px 0 10px rgba(0, 0, 0, 0.12); /* shadow-md 吸附拉环立体阴影 */
  outline: none;
  transition: all 0.18s cubic-bezier(0.4, 0, 0.2, 1);
}

.kt-drawer-trigger:hover {
  background: var(--bg-hover);
  color: var(--accent);
  border-color: var(--accent-light);
  box-shadow: 4px 0 14px rgba(0, 0, 0, 0.16);
  transform: translateY(-50%) scale(1.05);
}

.kt-drawer-trigger:active {
  transform: translateY(-50%) scale(0.95);
}

/* ── 边缘悬浮 Toggle 按钮：精细化侧栏折叠拉环 ── */
.kt-nav-edge-toggle {
  position: absolute;
  top: 50%;
  right: -12px; /* 精准半靠在分割线上 */
  transform: translateY(-50%);
  width: 22px;
  height: 40px;
  border-radius: 0 12px 12px 0; /* 贴边半圆拉环 */
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-left: none;
  box-shadow: 2px 0 8px rgba(0, 0, 0, 0.08);
  color: var(--text-muted);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  z-index: 30;
  transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
  outline: none;
}

/* Icon 旋转效果 */
.toggle-chevron {
  transition: transform 0.25s cubic-bezier(0.4, 0, 0.2, 1), color 0.15s ease;
}

.kt-nav-edge-toggle.is-collapsed .toggle-chevron {
  transform: rotate(180deg);
}

/* Hover 浮跃与色值高亮 */
.kt-nav-edge-toggle:hover {
  background: var(--bg-hover);
  color: var(--accent);
  border-color: var(--accent-light);
  box-shadow: 3px 0 12px rgba(0, 0, 0, 0.14);
  transform: translateY(-50%) scale(1.06);
}

/* Active 弹簧按压反馈 */
.kt-nav-edge-toggle:active {
  transform: translateY(-50%) scale(0.95);
}

/* ── 顶部标题栏 ── */
.kt-nav-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 14px 10px;
  border-bottom: 1px solid var(--divider);
  flex-shrink: 0;
}

.kt-nav-title {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12.5px;
  font-weight: 650;
  color: var(--text-secondary);
  letter-spacing: 0.01em;
}

.kt-nav-title :deep(.app-icon) {
  color: var(--text-muted);
}

/* ── 滚动主体（右侧留出 14px 内边距，解耦滚动条与右侧浮动折叠按键） ── */
.kt-nav-body {
  flex: 1;
  min-height: 0;
  padding: 8px 14px 12px 8px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  overflow-y: auto;
  overscroll-behavior: contain;
  scrollbar-width: thin;
}

/* ── 顶部筛选区组：胶囊间距 + 极浅灰色分割线 ── */
.kt-filter-group {
  display: flex;
  flex-direction: column;
  gap: 12px; /* space-y-3 等效，胶囊间充足间距 */
  padding-bottom: 16px;
  margin-bottom: 16px;
  border-bottom: 1px solid var(--divider);
}

.kt-nav-body::-webkit-scrollbar {
  width: 6px;
}

.kt-nav-body::-webkit-scrollbar-thumb {
  background: var(--border-strong);
  border-radius: 3px;
}

.kt-nav-body::-webkit-scrollbar-track {
  background: transparent;
}

/* ═══ 第 1 层：学段 — 全圆角浮岛胶囊 (Pill-in-Pill) ═══ */
.kt-stage-row {
  display: flex;
  width: 100%;
  padding: 4px;
  gap: 4px;
  /* 白色胶囊底座 + 柔和外阴影 + 极浅边框 */
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: 9999px; /* rounded-full 全圆角 */
  box-shadow: 0 2px 12px rgba(0, 0, 0, 0.06);
}

.kt-stage-tab {
  flex: 1;
  padding: 6px 12px;
  border: none;
  border-radius: 9999px; /* 强制全圆角 */
  background: transparent;
  color: var(--text-muted);
  font-size: 12.5px;
  font-weight: 500;
  cursor: pointer;
  transition: background-color 0.22s ease, color 0.22s ease;
  text-align: center;
}

.kt-stage-tab:hover:not(.active) {
  background: var(--bg-hover);
  color: var(--text-secondary);
}

/* 选中态：无阴影浅灰实体填充 + 主题蓝文字 */
.kt-stage-tab.active {
  background: var(--bg-active);
  color: var(--accent);
  font-weight: 600;
  cursor: default;
}

/* ═══ 第 2 层：学科 — 全圆角浮岛胶囊 ═══ */
.kt-subject-row {
  display: flex;
  width: 100%;
  padding: 4px;
  gap: 4px;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: 9999px;
  box-shadow: 0 2px 12px rgba(0, 0, 0, 0.06);
}

.kt-subject-tag {
  flex: 1;
  padding: 6px 12px;
  border: none;
  border-radius: 9999px;
  background: transparent;
  color: var(--text-muted);
  font-size: 11.5px;
  font-weight: 500;
  cursor: pointer;
  transition: background-color 0.22s ease, color 0.22s ease;
  text-align: center;
}

.kt-subject-tag:hover:not(.active) {
  background: var(--bg-hover);
  color: var(--text-secondary);
}

.kt-subject-tag.active {
  background: var(--bg-active);
  color: var(--accent);
  font-weight: 600;
  cursor: default;
}

/* ═══ 第 3 层：模式 — 全圆角浮岛胶囊 ═══ */
.kt-mode-segment {
  display: flex;
  width: 100%;
  padding: 4px;
  gap: 4px;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: 9999px;
  box-shadow: 0 2px 12px rgba(0, 0, 0, 0.06);
}

.kt-mode-item {
  flex: 1;
  padding: 6px 12px;
  border: none;
  border-radius: 9999px;
  background: transparent;
  color: var(--text-muted);
  font-size: 11.5px;
  font-weight: 500;
  cursor: pointer;
  transition: background-color 0.22s ease, color 0.22s ease;
  text-align: center;
}

.kt-mode-item:hover:not(.active) {
  background: var(--bg-hover);
  color: var(--text-secondary);
}

.kt-mode-item.active {
  background: var(--bg-active);
  color: var(--accent);
  font-weight: 600;
  cursor: default;
}

/* 暗色模式：胶囊底座使用 card 背景，选中项使用 active 背景 */
[data-theme='dark'] .kt-stage-row,
[data-theme='dark'] .kt-subject-row,
[data-theme='dark'] .kt-mode-segment {
  background: var(--bg-card);
  border-color: var(--border-color);
  box-shadow: 0 2px 12px rgba(0, 0, 0, 0.3);
}

[data-theme='dark'] .kt-stage-tab.active,
[data-theme='dark'] .kt-subject-tag.active,
[data-theme='dark'] .kt-mode-item.active {
  background: var(--bg-active);
  color: var(--accent);
}

/* "全部题目"快捷项 */
.kt-nav-all {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 7px 10px;
  border: 1px solid transparent;
  border-radius: 6px;
  background: transparent;
  color: var(--text-secondary);
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: var(--transition-fast);
  text-align: left;
  width: 100%;
}

.kt-nav-all:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.kt-nav-all.active {
  background: var(--accent-light);
  color: var(--accent);
  font-weight: 600;
}

/* 树列表：左右内边距，避免高亮色块顶满边缘 */
.kt-nav-list {
  display: flex;
  flex-direction: column;
  gap: 1px;
  padding: 4px 8px 8px;
}

.kt-nav-loading {
  padding: 24px 12px;
  text-align: center;
  color: var(--text-muted);
  font-size: 12.5px;
}

/* 节点行：顶部对齐（支持多行文本），相对定位承载虚线 */
.kt-nav-row {
  position: relative;
  display: flex;
  align-items: flex-start;
  gap: 4px;
  padding: 6px 8px;
  border-radius: 8px;
  cursor: pointer;
  transition: background 0.15s ease, color 0.15s ease;
  font-size: 12.5px;
  line-height: 1.5;
  color: var(--text-primary);
  user-select: none;
}

.kt-nav-row:hover {
  background: var(--bg-hover);
}

.kt-nav-row.selected {
  background: var(--accent-light);
  color: var(--accent);
  font-weight: 600;
}

/* ── 层级虚线引导线：根据 depth 绝对定位垂直虚线 ── */
.indent-guide {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 0;
  border-left: 1px dashed var(--border-strong);
  opacity: 0.55;
  pointer-events: none;
}

/* 选中态时虚线变浅，避免与背景色冲突 */
.kt-nav-row.selected .indent-guide {
  opacity: 0.3;
}

/* ── 树节点展开/折叠按钮：顶部对齐多行文本首行 ── */
.row-expand {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  margin-top: 2px; /* 与首行文字视觉对齐（line-height 1.5 × 12.5px ≈ 18.75px） */
  border-radius: 4px;
  background: transparent;
  border: none;
  padding: 0;
  color: var(--text-muted);
  cursor: pointer;
  flex-shrink: 0;
  transition: background-color 0.15s ease, color 0.15s ease, transform 0.15s ease;
  margin-right: 2px;
}

.row-expand:hover {
  background-color: var(--accent-light);
  color: var(--accent);
}

.row-expand:active {
  transform: scale(0.88);
}

.row-expand.is-expanded {
  color: var(--text-secondary);
}

.row-expand:hover.is-expanded {
  color: var(--accent);
}

.row-expand-icon {
  transition: transform 0.22s cubic-bezier(0.4, 0, 0.2, 1);
  transform-origin: center;
}

.row-expand.is-expanded .row-expand-icon {
  transform: rotate(90deg);
}

.row-dot {
  width: 4px;
  height: 4px;
  border-radius: 50%;
  background: var(--border-strong);
  flex-shrink: 0;
  margin-top: 8px; /* 与首行文字视觉对齐 */
  margin-right: 8px;
}

.kt-nav-row.selected .row-dot {
  background: var(--accent);
}

/* 节点文本：允许自然换行，不截断 */
.row-name {
  flex: 1;
  min-width: 0;
  white-space: normal;
  word-break: break-word;
  overflow-wrap: anywhere;
  padding-top: 1px;
}

.row-count {
  flex-shrink: 0;
  margin-top: 2px; /* 与首行文字视觉对齐 */
  padding: 0 6px;
  border-radius: var(--radius-full);
  background: var(--bg-active);
  color: var(--text-secondary);
  font-size: 10.5px;
  font-variant-numeric: tabular-nums;
  line-height: 1.6;
}

.kt-nav-row.selected .row-count {
  background: var(--accent-light);
  color: var(--accent);
}

</style>
