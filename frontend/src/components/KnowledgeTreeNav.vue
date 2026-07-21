<script setup lang="ts">
/**
 * KnowledgeTreeNav — 题库列表左侧常驻知识树导航面板
 *
 * 视觉规范：260px 宽，右侧 1px 分割线，扁平化树渲染，全部 CSS 变量
 */
import { ref, computed, watch, onMounted } from 'vue'
import { AppIcon, AppEmpty } from '@/components/ui'
import {
  knowledgeTreeApi,
  knowledgeNodeApi,
  type KnowledgeTree,
  type KnowledgeNodeTreeNode,
} from '@/api/client'

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
}>()

// ─── 状态 ──────────────────────────────────────────────────────────────
const collapsed = ref(false)
const trees = ref<KnowledgeTree[]>([])
const activeTreeId = ref<string>('')
const treeData = ref<KnowledgeNodeTreeNode[]>([])
const loading = ref(false)

// 内部选中态（用于即时视觉反馈，无需等待父组件回传）
const internalSelected = ref('')

// 展开/折叠节点 ID 集合
const expandedIds = ref<Set<string>>(new Set())

// ─── 计算属性 ─────────────────────────────────────────────────────────
const showTreeSelector = computed(() => !props.treeId && trees.value.length > 1)

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

// ─── 数据加载 ─────────────────────────────────────────────────────────
async function loadTrees() {
  try {
    const res = await knowledgeTreeApi.list()
    trees.value = res.data
    if (props.treeId) {
      activeTreeId.value = props.treeId
    } else if (trees.value.length > 0 && !activeTreeId.value) {
      activeTreeId.value = trees.value[0].id
    }
  } catch (e) {
    console.error('[TreeNav] 加载知识树列表失败', e)
  }
}

async function loadTreeData() {
  if (!activeTreeId.value) return
  loading.value = true
  try {
    const res = await knowledgeNodeApi.getTree(activeTreeId.value)
    treeData.value = res.data
    // 默认展开所有根节点
    expandedIds.value = new Set(
      treeData.value.filter((n) => n.children.length > 0).map((n) => n.id),
    )
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

        <!-- 多树切换 -->
        <div v-if="showTreeSelector" class="kt-nav-tree-tabs">
          <button
            v-for="t in trees"
            :key="t.id"
            type="button"
            class="kt-nav-tree-tab"
            :class="{ active: activeTreeId === t.id }"
            @click="activeTreeId = t.id"
          >
            {{ t.name }}
          </button>
        </div>

        <!-- 树列表 -->
        <div class="kt-nav-list">
          <div v-if="loading" class="kt-nav-loading">加载中…</div>
          <AppEmpty v-else-if="flatList.length === 0" description="无知识点" />
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
  padding: 8px 8px 12px;
  display: flex;
  flex-direction: column;
  gap: 8px;
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

/* 多树切换 */
.kt-nav-tree-tabs {
  display: flex;
  gap: 4px;
  overflow-x: auto;
  scrollbar-width: thin;
  padding-bottom: 2px;
}

.kt-nav-tree-tab {
  padding: 4px 10px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-full);
  background: transparent;
  color: var(--text-secondary);
  font-size: 11.5px;
  cursor: pointer;
  white-space: nowrap;
  transition: all 0.15s;
  flex-shrink: 0;
}

.kt-nav-tree-tab:hover {
  border-color: var(--text-muted);
  color: var(--text-primary);
}

.kt-nav-tree-tab.active {
  background: var(--accent);
  border-color: var(--accent);
  color: var(--text-inverse);
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

/* 细滚动条（WebKit） */
.kt-nav-tree-tabs::-webkit-scrollbar {
  height: 4px;
}

.kt-nav-tree-tabs::-webkit-scrollbar-thumb {
  background: var(--border-strong);
  border-radius: 2px;
}
</style>
