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
import { ref, computed, watch, onMounted } from 'vue'
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

// ─── 状态 ──────────────────────────────────────────────────────────────
// 知识树收起状态持久化到 localStorage，防止路由切换后状态丢失
const collapsed = ref(localStorage.getItem('knowledge-tree-collapsed') === 'true')
watch(collapsed, (val) => {
  localStorage.setItem('knowledge-tree-collapsed', String(val))
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
  collapsed.value = !collapsed.value
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
  await loadTrees()
  if (activeTreeId.value) await loadTreeData()
})
</script>

<template>
  <div class="kt-nav-wrapper" :class="{ 'is-collapsed': collapsed }">
    <!-- 实际侧栏：折叠时宽度为 0，内容隐藏 -->
    <aside class="kt-nav">
      <header class="kt-nav-header">
        <div class="kt-nav-title">
          <AppIcon name="list" :size="14" />
          <span>知识树导航</span>
        </div>
      </header>

      <!-- 全部 + 树切换 + 列表 共用滚动区 -->
      <div class="kt-nav-body">
        <!-- ===== 第 1 层：学段切换（极简下划线 Tab） ===== -->
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

        <!-- ===== 第 2 层：学科切换（轻量浅色小标签） ===== -->
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

        <!-- ===== 第 3 层：模式 Tabs（无缝分段控制器） ===== -->
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
              :style="{ paddingLeft: 6 + item.depth * 16 + 'px' }"
              @click="selectNode(item.node.id)"
            >
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

    <!-- 边缘悬浮 Toggle 按钮：长条胶囊状，具备 Icon 旋转与微交互 Prompt Tooltip -->
    <button
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
      <span class="toggle-tooltip">{{ collapsed ? '展开导航' : '收起导航' }}</span>
    </button>
  </div>
</template>

<style scoped>
/* ── 外层 wrapper：负责宽度过渡，承载悬浮按钮 ── */
.kt-nav-wrapper {
  position: relative;
  flex-shrink: 0;
  height: 100%;
  width: 260px;
  transition: width 0.3s cubic-bezier(0.4, 0, 0.2, 1);
}

.kt-nav-wrapper.is-collapsed {
  width: 0;
}

/* ── 实际侧栏 ── */
.kt-nav {
  width: 260px;
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--bg-card);
  border-right: 1px solid var(--border-color);
  overflow: hidden;
  transition: opacity 0.25s ease;
}

.kt-nav-wrapper.is-collapsed .kt-nav {
  opacity: 0;
  pointer-events: none;
}

/* ── 边缘悬浮 Toggle 按钮：精细化侧栏折叠把手 ── */
.kt-nav-edge-toggle {
  position: absolute;
  top: 50%;
  right: -11px;
  transform: translateY(-50%);
  width: 22px;
  height: 38px;
  border-radius: 19px;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  box-shadow: 0 2px 6px rgba(0, 0, 0, 0.06);
  color: var(--text-muted);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  z-index: 20;
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

/* Hover 浮跃与色值高亮（完全贴合系统主题变量） */
.kt-nav-edge-toggle:hover {
  background: var(--bg-hover);
  color: var(--accent);
  border-color: var(--accent-light);
  box-shadow: 0 3px 10px rgba(0, 0, 0, 0.08);
  transform: translateY(-50%) scale(1.05);
}

/* Active 弹簧按压反馈 */
.kt-nav-edge-toggle:active {
  transform: translateY(-50%) scale(0.95);
}

/* 侧栏折叠时 edge toggle 的定位适配 */
.kt-nav-wrapper.is-collapsed .kt-nav-edge-toggle {
  right: -24px;
  box-shadow: 0 3px 10px rgba(0, 0, 0, 0.12);
}

/* 悬浮微型 Prompt Tooltip */
.toggle-tooltip {
  position: absolute;
  left: calc(100% + 6px);
  top: 50%;
  transform: translateY(-50%) translateX(-4px);
  white-space: nowrap;
  background: var(--text-primary);
  color: var(--bg-card);
  font-size: 11px;
  font-weight: 500;
  padding: 3px 7px;
  border-radius: 4px;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
  pointer-events: none;
  opacity: 0;
  visibility: hidden;
  transition: all 0.18s cubic-bezier(0.4, 0, 0.2, 1);
}

.kt-nav-edge-toggle:hover .toggle-tooltip {
  opacity: 1;
  visibility: visible;
  transform: translateY(-50%) translateX(0);
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

/* ── 滚动主体 ── */
.kt-nav-body {
  flex: 1;
  min-height: 0;
  padding: 6px 8px 12px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  overflow-y: auto;
  overscroll-behavior: contain;
  scrollbar-width: thin;
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

/* ═══ 第 1 层：学段 — 极简下划线 Tab ═══ */
.kt-stage-row {
  display: flex;
  gap: 18px;
  padding: 0 4px;
  border-bottom: 1px solid var(--divider);
}

.kt-stage-tab {
  position: relative;
  padding: 3px 2px 6px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  font-size: 12.5px;
  font-weight: 500;
  cursor: pointer;
  transition: color 0.15s ease;
}

.kt-stage-tab:hover {
  color: var(--text-secondary);
}

.kt-stage-tab.active {
  color: var(--text-primary);
  font-weight: 600;
}

/* 品牌蓝下划线：伪元素实现，避免影响布局高度 */
.kt-stage-tab.active::after {
  content: '';
  position: absolute;
  left: 0;
  right: 0;
  bottom: -1px;
  height: 2px;
  background: var(--accent);
  border-radius: 1px;
}

/* ═══ 第 2 层：学科 — 轻量浅色小标签 ═══ */
.kt-subject-row {
  display: flex;
  gap: 6px;
  padding: 0 2px;
}

.kt-subject-tag {
  padding: 2px 9px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-full);
  background: transparent;
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.15s ease;
}

.kt-subject-tag:hover {
  border-color: var(--text-muted);
  color: var(--text-secondary);
}

/* 选中态：浅蓝底 + 深蓝字 + 极浅蓝边框（取消实心蓝背景） */
.kt-subject-tag.active {
  background: var(--accent-light);
  border-color: var(--accent-light);
  color: var(--accent);
  font-weight: 600;
}

/* ═══ 第 3 层：模式 — 无缝分段控制器 (Segmented Control) ═══ */
.kt-mode-segment {
  display: flex;
  gap: 2px;
  padding: 2px;
  background: var(--bg-active);
  border-radius: 6px;
}

.kt-mode-item {
  flex: 1;
  padding: 4px 6px;
  border: none;
  border-radius: 4px;
  background: transparent;
  color: var(--text-muted);
  font-size: 11.5px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.18s cubic-bezier(0.4, 0, 0.2, 1);
}

.kt-mode-item:hover:not(.active) {
  color: var(--text-secondary);
}

/* 选中态：纯白底色 + 细微阴影，模拟物理滑块 */
.kt-mode-item.active {
  background: var(--bg-card);
  color: var(--text-primary);
  font-weight: 600;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1);
}

[data-theme='dark'] .kt-mode-item.active {
  background: var(--bg-input);
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.35);
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

/* 树列表 */
.kt-nav-list {
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.kt-nav-loading {
  padding: 24px 12px;
  text-align: center;
  color: var(--text-muted);
  font-size: 12.5px;
}

.kt-nav-row {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 5px 8px;
  border-radius: 5px;
  cursor: pointer;
  transition: background 0.12s;
  font-size: 12.5px;
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

/* ── 树节点展开/折叠按钮 ── */
.row-expand {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border-radius: 4px;
  background: transparent;
  border: none;
  padding: 0;
  color: var(--text-muted);
  cursor: pointer;
  flex-shrink: 0;
  transition: background-color 0.15s ease, color 0.15s ease, transform 0.15s ease;
  margin-right: 1px;
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
  margin: 0 8px;
}

.kt-nav-row.selected .row-dot {
  background: var(--accent);
}

.row-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.row-count {
  flex-shrink: 0;
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
