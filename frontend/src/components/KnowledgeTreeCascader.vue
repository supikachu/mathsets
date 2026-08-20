<script setup lang="ts">
/**
 * KnowledgeTreeCascader — 知识点树形多选器
 *
 * 用途：替代旧 KpPickerNode，作为录题面板和列表筛选的核心知识点选择器。
 *
 * 设计要点：
 * - 复用 AppIcon / AppEmpty，不引入任何第三方 UI 库
 * - 触发器风格对齐 AppSelect（Apple 风格圆角输入框）
 * - 交互模型：平铺折叠（Push-down Accordion），取消悬浮层
 *   · 点击触发器 → 面板在 DOM 流中向下展开，推开后续表单字段
 *   · 树列表不设 max-height / overflow-y，依赖父级面板流式滚动（拒绝嵌套滚动条）
 *   · Esc 或"收起"按钮折叠面板
 * - 树采用"扁平化 + 深度缩进"渲染，规避递归组件的复杂度
 * - 选中状态用 Set 存储，O(1) 查询；不联动父子（保持最小可用语义）
 */
import { ref, computed, watch, onMounted, onBeforeUnmount, nextTick } from 'vue'
import { AppIcon, AppEmpty } from '@/components/ui'
import {
  knowledgeTreeApi,
  knowledgeNodeApi,
  type KnowledgeTree,
  type KnowledgeNodeTreeNode,
  type KnowledgeTreeKind,
} from '@/api/client'
import { unwrapTreeResponse } from '@/composables/useKnowledgeTreeCache'

const props = withDefaults(
  defineProps<{
    /** 选中节点 ID 列表 */
    modelValue: string[]
    /** 锁定知识树 ID（不传则允许用户在多棵树之间切换） */
    treeId?: string
    /** 按知识树 kind 过滤（chapter / knowledge / ability） */
    kind?: KnowledgeTreeKind
    placeholder?: string
    disabled?: boolean
    /** 最大可选数量，0 = 不限 */
    max?: number
  }>(),
  {
    placeholder: '选择知识点…',
    disabled: false,
    max: 0,
  },
)

const emit = defineEmits<{
  'update:modelValue': [value: string[]]
}>()

// ─── 状态 ──────────────────────────────────────────────────────────────
const open = ref(false)

const trees = ref<KnowledgeTree[]>([])
const activeTreeId = ref<string>('')
const treeData = ref<KnowledgeNodeTreeNode[]>([])
const loading = ref(false)

const expandedIds = ref<Set<string>>(new Set())
const selectedIds = ref<Set<string>>(new Set(props.modelValue))

/** ID → 节点，用于回显已选名称（即便节点不在当前树里也能展示） */
const nodeMap = ref<Map<string, KnowledgeNodeTreeNode>>(new Map())

/** 反向定位：点击已选 chip 时短暂高亮目标行 */
const locatingId = ref<string | null>(null)
let locateTimer: number | null = null
let scrollTimer: number | null = null
/** 等待切树 / 加载完成后再定位 */
let pendingLocateId: string | null = null
let pendingLocateStop: (() => void) | null = null
let pendingLocateTimeout: number | null = null

onBeforeUnmount(() => {
  if (locateTimer !== null) window.clearTimeout(locateTimer)
  if (scrollTimer !== null) window.clearTimeout(scrollTimer)
  if (pendingLocateTimeout !== null) window.clearTimeout(pendingLocateTimeout)
  pendingLocateStop?.()
  locateTimer = null
  scrollTimer = null
  pendingLocateTimeout = null
  pendingLocateStop = null
})

// ─── 计算属性 ─────────────────────────────────────────────────────────
const showTreeSelector = computed(() => !props.treeId && trees.value.length > 1)

const selectedNodes = computed(() =>
  Array.from(selectedIds.value)
    .map((id) => nodeMap.value.get(id))
    .filter((n): n is KnowledgeNodeTreeNode => !!n),
)

// ─── Chips 折叠：默认最多显示 2 个，剩余合并为 +N 徽标 ─────
const CHIP_LIMIT = 2
const showAllChips = ref(false)

const visibleChips = computed(() =>
  showAllChips.value ? selectedNodes.value : selectedNodes.value.slice(0, CHIP_LIMIT),
)

const hiddenChipsCount = computed(() =>
  Math.max(0, selectedNodes.value.length - CHIP_LIMIT),
)

const activeTreeName = computed(() =>
  trees.value.find((t) => t.id === activeTreeId.value)?.name ?? '',
)

interface FlatItem {
  node: KnowledgeNodeTreeNode
  depth: number
}

/** 扁平化展示列表：仅依据 expandedIds 决定是否递归子节点（不再做搜索过滤） */
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
function toggle() {
  if (props.disabled) return
  open.value = !open.value
}

function collapse() {
  open.value = false
}

function toggleExpand(id: string) {
  if (expandedIds.value.has(id)) expandedIds.value.delete(id)
  else expandedIds.value.add(id)
}

function hasChildren(n: KnowledgeNodeTreeNode): boolean {
  return n.children.length > 0
}

function isSelected(id: string): boolean {
  return selectedIds.value.has(id)
}

function toggleSelect(id: string) {
  if (selectedIds.value.has(id)) {
    selectedIds.value.delete(id)
  } else {
    if (props.max > 0 && selectedIds.value.size >= props.max) return
    selectedIds.value.add(id)
  }
  emit('update:modelValue', Array.from(selectedIds.value))
}

function clearAll() {
  selectedIds.value.clear()
  emit('update:modelValue', [])
}

/** Esc 折叠面板（仅当焦点在级联器内部时触发，避免干扰其他输入区） */
function onEscape() {
  if (open.value) open.value = false
}

/**
 * 在当前已加载树中展开祖先、高亮并滚动到节点。
 * @returns 节点存在于当前树则 true
 */
async function expandToInCurrentTree(nodeId: string): Promise<boolean> {
  const node = nodeMap.value.get(nodeId)
  if (!node) return false

  open.value = true

  const chain: string[] = []
  let cur = node.parent_id
  while (cur) {
    chain.push(cur)
    cur = nodeMap.value.get(cur)?.parent_id ?? null
  }
  const next = new Set(expandedIds.value)
  chain.forEach((id) => next.add(id))
  expandedIds.value = next

  if (locatingId.value === nodeId) {
    locatingId.value = null
    await nextTick()
  }
  locatingId.value = nodeId
  if (locateTimer !== null) window.clearTimeout(locateTimer)
  locateTimer = window.setTimeout(() => {
    if (locatingId.value === nodeId) locatingId.value = null
    locateTimer = null
  }, 2000)

  await nextTick()
  if (scrollTimer !== null) window.clearTimeout(scrollTimer)
  scrollTimer = window.setTimeout(() => {
    scrollTimer = null
    document
      .querySelector(`[data-cascader-node-id="${CSS.escape(nodeId)}"]`)
      ?.scrollIntoView({ behavior: 'smooth', block: 'center' })
  }, 400)
  return true
}

function cancelPendingLocate() {
  pendingLocateStop?.()
  pendingLocateStop = null
  pendingLocateId = null
  if (pendingLocateTimeout !== null) {
    window.clearTimeout(pendingLocateTimeout)
    pendingLocateTimeout = null
  }
}

/**
 * 点击已选 chip → 展开面板并定位到该节点。
 * chip 来自当前树的 nodeMap；若树仍在加载则等待后再定位。
 */
async function locateChip(nodeId: string) {
  if (props.disabled || !nodeId) return

  if (nodeMap.value.has(nodeId)) {
    await expandToInCurrentTree(nodeId)
    return
  }

  // 节点尚未进入 nodeMap（树仍在加载）：展开面板并等待
  open.value = true
  cancelPendingLocate()
  pendingLocateId = nodeId
  pendingLocateTimeout = window.setTimeout(() => {
    cancelPendingLocate()
  }, 15000)
  pendingLocateStop = watch(
    [loading, () => nodeMap.value.size],
    () => {
      if (loading.value || !pendingLocateId) return
      void expandToInCurrentTree(pendingLocateId).then((ok) => {
        if (ok) cancelPendingLocate()
      })
    },
    { immediate: true },
  )
}

function rebuildNodeMap() {
  const map = new Map<string, KnowledgeNodeTreeNode>()
  function walk(nodes: KnowledgeNodeTreeNode[]) {
    for (const n of nodes) {
      map.set(n.id, n)
      if (n.children.length > 0) walk(n.children)
    }
  }
  walk(treeData.value)
  nodeMap.value = map
}

// ─── 数据加载 ─────────────────────────────────────────────────────────
async function loadTrees() {
  try {
    const res = await knowledgeTreeApi.list(props.kind ? { kind: props.kind } : undefined)
    trees.value = res.data
    if (props.treeId) {
      activeTreeId.value = props.treeId
    } else if (trees.value.length > 0) {
      const stillValid = trees.value.some((t) => t.id === activeTreeId.value)
      if (!stillValid) {
        activeTreeId.value = trees.value[0].id
      }
    } else {
      activeTreeId.value = ''
      treeData.value = []
    }
  } catch (e) {
    console.error('[Cascader] 加载知识树列表失败', e)
  }
}

async function loadTreeData() {
  if (!activeTreeId.value) return
  loading.value = true
  try {
    const res = await knowledgeNodeApi.getTree(activeTreeId.value)
    treeData.value = unwrapTreeResponse(res.data)
    rebuildNodeMap()
    // 默认展开所有根节点
    expandedIds.value = new Set(
      treeData.value.filter((n) => n.children.length > 0).map((n) => n.id),
    )
  } catch (e) {
    console.error('[Cascader] 加载知识点树失败', e)
  } finally {
    loading.value = false
  }
}

// ─── 侦听 ──────────────────────────────────────────────────────────────
// 选中数下降到 ≤2 时自动收起"展开全部"状态，避免下次添加新 chip 仍展开
watch(
  () => selectedNodes.value.length,
  (n) => {
    if (n <= CHIP_LIMIT) showAllChips.value = false
  },
)

watch(
  () => props.treeId,
  (newId) => {
    if (newId) activeTreeId.value = newId
  },
)

watch(
  () => props.kind,
  async () => {
    await loadTrees()
    if (activeTreeId.value) await loadTreeData()
  },
)

watch(activeTreeId, () => {
  loadTreeData()
})

watch(
  () => props.modelValue,
  (newVal) => {
    selectedIds.value = new Set(newVal)
  },
  { deep: true },
)

onMounted(async () => {
  await loadTrees()
  if (activeTreeId.value) await loadTreeData()
})
</script>

<template>
  <div class="kt-cascader" :class="{ disabled }" @keydown.escape="onEscape">
    <!-- 触发器 -->
    <button
      type="button"
      class="cascader-trigger"
      :class="{ open, 'has-value': selectedIds.size > 0 }"
      :disabled="disabled"
      @click="toggle"
    >
      <span class="cascader-text" :class="{ placeholder: selectedIds.size === 0 }">
        <template v-if="selectedIds.size === 0">{{ placeholder }}</template>
        <template v-else>
          已选 {{ selectedIds.size }} 项
          <span v-if="activeTreeName" class="text-tree">· {{ activeTreeName }}</span>
        </template>
      </span>
      <span class="cascader-icons">
        <button
          v-if="selectedIds.size > 0 && !disabled"
          type="button"
          class="cascader-clear"
          @click.stop="clearAll"
        >
          <AppIcon name="x" :size="13" />
        </button>
        <AppIcon
          name="chevron-down"
          :size="13"
          class="cascader-chevron"
          :class="{ rotated: open }"
        />
      </span>
    </button>

    <!-- 已选 chips（默认最多 2 个，超出折叠为 +N 徽标；点击名称展开并定位到树节点） -->
    <div v-if="selectedNodes.length > 0" class="cascader-chips">
      <span
        v-for="n in visibleChips"
        :key="n.id"
        class="chip chip-clickable"
        :title="`点击定位到「${n.name}」`"
        @click="locateChip(n.id)"
      >
        <span class="chip-name">{{ n.name }}</span>
        <button class="chip-x" type="button" @click.stop="toggleSelect(n.id)">
          <AppIcon name="x" :size="11" />
        </button>
      </span>
      <!-- 折叠徽标：点击展开全部 chips -->
      <button
        v-if="hiddenChipsCount > 0 && !showAllChips"
        type="button"
        class="chip chip-more"
        :title="`点击展开剩余 ${hiddenChipsCount} 个`"
        @click="showAllChips = true"
      >
        +{{ hiddenChipsCount }}
      </button>
      <!-- 展开后提供收起按钮 -->
      <button
        v-if="showAllChips && hiddenChipsCount > 0"
        type="button"
        class="chip chip-collapse"
        @click="showAllChips = false"
      >
        收起
      </button>
    </div>

    <!-- 平铺折叠面板（Push-down Accordion） -->
    <!-- 利用 grid-template-rows 0fr→1fr 实现高度自适应的平滑过渡；
         内层 overflow:hidden 在折叠动画期间裁剪内容；
         树列表不设 max-height / overflow-y，依赖父级面板流式滚动 -->
    <Transition name="cascader-accordion">
      <div v-if="open" class="cascader-panel">
        <div class="cascader-panel-inner">
          <!-- 多树切换 -->
          <div v-if="showTreeSelector" class="pop-tree-tabs">
            <button
              v-for="t in trees"
              :key="t.id"
              type="button"
              class="pop-tree-tab"
              :class="{ active: activeTreeId === t.id }"
              @click="activeTreeId = t.id"
            >
              {{ t.name }}
            </button>
          </div>

          <!-- 树列表：自然撑开高度，不设 max-height / overflow-y -->
          <div class="pop-tree-list">
            <div v-if="loading" class="pop-loading">加载中…</div>
            <AppEmpty v-else-if="flatList.length === 0" description="无知识点" />
            <template v-else>
              <div
                v-for="item in flatList"
                :key="item.node.id"
                class="pop-row"
                :class="{
                  selected: isSelected(item.node.id),
                  locating: locatingId === item.node.id,
                }"
                :data-cascader-node-id="item.node.id"
                :style="{ paddingLeft: 8 + item.depth * 20 + 'px' }"
                @click="toggleSelect(item.node.id)"
              >
                <button
                  v-if="hasChildren(item.node)"
                  type="button"
                  class="row-expand"
                  @click.stop="toggleExpand(item.node.id)"
                >
                  <AppIcon
                    :name="expandedIds.has(item.node.id) ? 'chevron-down' : 'chevron-right'"
                    :size="11"
                  />
                </button>
                <span v-else class="row-expand-spacer" />

                <span class="row-check" :class="{ checked: isSelected(item.node.id) }">
                  <AppIcon v-if="isSelected(item.node.id)" name="check" :size="12" />
                </span>

                <span class="row-name">{{ item.node.name }}</span>
                <span v-if="item.node.code" class="row-code">{{ item.node.code }}</span>
                <span
                  v-if="item.node.question_count > 0"
                  class="row-count"
                >{{ item.node.question_count }}</span>
              </div>
            </template>
          </div>

          <!-- Footer -->
          <div class="pop-footer">
            <span class="footer-info">{{ selectedIds.size }} 项已选</span>
            <div class="footer-actions">
              <button type="button" class="footer-btn" @click="clearAll">清空</button>
              <button type="button" class="footer-btn primary" @click="collapse">
                <AppIcon name="chevron-up" :size="12" />
                <span>收起</span>
              </button>
            </div>
          </div>
        </div>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.kt-cascader {
  position: relative;
  width: 100%;
}

.kt-cascader.disabled {
  opacity: 0.5;
  pointer-events: none;
}

/* ── 触发器：复刻 AppSelect 的 Apple 风格 ── */
.cascader-trigger {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
  width: 100%;
  padding: 7px 12px;
  border: 1px solid var(--border-color);
  border-radius: 8px;
  background: var(--bg-input);
  color: var(--text-primary);
  font-size: 13px;
  font-family: inherit;
  line-height: 1.4;
  cursor: pointer;
  transition: border-color 0.2s, box-shadow 0.2s, background 0.2s;
  text-align: left;
  box-sizing: border-box;
  min-height: 36px;
}

.cascader-trigger:hover:not(:disabled) {
  border-color: var(--text-muted);
}

.cascader-trigger.open {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
  background: var(--bg-card);
}

.cascader-trigger:disabled {
  cursor: not-allowed;
}

.cascader-text {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.cascader-text.placeholder {
  color: var(--text-muted);
}

.text-tree {
  color: var(--text-muted);
  margin-left: 4px;
}

.cascader-icons {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
}

.cascader-clear {
  display: flex;
  align-items: center;
  justify-content: center;
  background: none;
  border: none;
  padding: 0;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  color: var(--text-muted);
  cursor: pointer;
  transition: color 0.15s, background 0.15s;
}

.cascader-clear:hover {
  color: var(--text-primary);
  background: var(--bg-hover);
}

.cascader-chevron {
  color: var(--text-muted);
  transition: transform 0.2s ease;
  flex-shrink: 0;
}

.cascader-chevron.rotated {
  transform: rotate(180deg);
}

/* ── Chips ── */
.cascader-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 8px;
}

.chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 3px 8px 3px 10px;
  border-radius: var(--radius-full);
  background: var(--accent-light);
  color: var(--accent);
  font-size: 12px;
  line-height: 1.4;
  max-width: 200px;
}

.chip-clickable {
  cursor: pointer;
  transition: filter 0.15s, box-shadow 0.15s;
}

.chip-clickable:hover {
  filter: brightness(0.97);
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 25%, transparent);
}

.chip-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.chip-x {
  display: flex;
  align-items: center;
  justify-content: center;
  background: none;
  border: none;
  padding: 0;
  color: inherit;
  cursor: pointer;
  opacity: 0.7;
  transition: opacity 0.15s;
}

.chip-x:hover {
  opacity: 1;
}

/* +N 折叠徽标 */
.chip-more {
  background: var(--bg-active);
  color: var(--text-secondary);
  font-weight: 600;
  border: 1px dashed var(--border-strong);
  cursor: pointer;
  transition: var(--transition-fast);
}

.chip-more:hover {
  background: var(--accent-light);
  color: var(--accent);
  border-color: var(--accent);
  border-style: solid;
}

/* "收起" 按钮 */
.chip-collapse {
  background: transparent;
  color: var(--text-muted);
  font-size: 11px;
  border: 1px solid transparent;
  cursor: pointer;
  transition: var(--transition-fast);
}

.chip-collapse:hover {
  color: var(--text-primary);
  background: var(--bg-hover);
}

/* ── 平铺折叠面板（Push-down Accordion） — 下凹 (Well) 视觉 ── */
/* 外层 grid 容器：grid-template-rows 1fr ↔ 0fr 实现高度自适应平滑过渡 */
.cascader-panel {
  display: grid;
  grid-template-rows: 1fr;
  margin-top: 8px;
}

/* 内层 overflow:hidden 在 0fr 折叠态裁剪内容；展开态自然撑开不裁剪
   下凹视觉：使用 --bg-input（比卡片底色略深）+ inset 阴影模拟抽屉凹陷感；
   顶部边框与触发器 input 边框自然衔接，无外侧悬浮阴影 */
.cascader-panel-inner {
  overflow: hidden;
  min-height: 0;
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  box-shadow: inset 0 2px 6px rgba(0, 0, 0, 0.05);
}

[data-theme='dark'] .cascader-panel-inner {
  box-shadow: inset 0 2px 6px rgba(0, 0, 0, 0.3);
}

.pop-tree-tabs {
  display: flex;
  gap: 4px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--divider);
  overflow-x: auto;
}

.pop-tree-tab {
  padding: 4px 10px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-full);
  background: transparent;
  color: var(--text-secondary);
  font-size: 12px;
  cursor: pointer;
  white-space: nowrap;
  transition: all 0.15s;
}

.pop-tree-tab:hover {
  border-color: var(--text-muted);
  color: var(--text-primary);
}

.pop-tree-tab.active {
  background: var(--accent);
  border-color: var(--accent);
  color: var(--text-inverse);
}

/* 树列表：拒绝嵌套滚动 —— 不设 max-height / overflow-y，自然撑开高度，
   依赖外层 AttributeSidePanel 的 overflow-y:auto 实现全局流式滚动 */
.pop-tree-list {
  padding: 4px;
}

.pop-loading {
  padding: 32px 12px;
  text-align: center;
  color: var(--text-muted);
  font-size: 13px;
}

.pop-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 8px;
  border-radius: 6px;
  cursor: pointer;
  transition: background 0.12s;
  font-size: 13px;
  color: var(--text-primary);
}

.pop-row:hover {
  background: var(--bg-hover);
}

.pop-row.selected {
  background: var(--accent-light);
  color: var(--accent);
}

.pop-row.locating {
  animation: cascader-locate-pulse 2s ease-out;
}

@keyframes cascader-locate-pulse {
  0% {
    background: rgba(250, 204, 21, 0.45);
    box-shadow: 0 0 0 3px rgba(250, 204, 21, 0.35);
  }
  60% {
    background: rgba(250, 204, 21, 0.2);
    box-shadow: 0 0 0 2px rgba(250, 204, 21, 0.15);
  }
  100% {
    background: transparent;
    box-shadow: none;
  }
}

.row-expand {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  background: none;
  border: none;
  padding: 0;
  color: var(--text-muted);
  cursor: pointer;
  flex-shrink: 0;
}

.row-expand-spacer {
  width: 16px;
  height: 16px;
  flex-shrink: 0;
}

.row-check {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border: 1.5px solid var(--border-strong);
  border-radius: 4px;
  background: transparent;
  color: var(--text-inverse);
  flex-shrink: 0;
  transition: all 0.15s;
}

.row-check.checked {
  background: var(--accent);
  border-color: var(--accent);
}

.row-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.row-code {
  font-size: 11px;
  color: var(--text-muted);
  font-family: var(--font-mono);
}

.row-count {
  flex-shrink: 0;
  padding: 0 6px;
  border-radius: var(--radius-full);
  background: var(--bg-active);
  color: var(--text-secondary);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
}

.pop-row.selected .row-count {
  background: var(--accent-light);
  color: var(--accent);
}

.pop-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
  border-top: 1px solid var(--divider);
  background: var(--bg-primary);
}

.footer-info {
  font-size: 12px;
  color: var(--text-muted);
}

.footer-actions {
  display: flex;
  gap: 8px;
}

.footer-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 4px 10px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-card);
  color: var(--text-secondary);
  font-size: 12px;
  cursor: pointer;
  transition: all 0.15s;
}

.footer-btn:hover {
  border-color: var(--text-muted);
  color: var(--text-primary);
}

.footer-btn.primary {
  background: var(--accent);
  border-color: var(--accent);
  color: var(--text-inverse);
}

.footer-btn.primary:hover {
  background: var(--accent-hover);
  border-color: var(--accent-hover);
}

/* ── 过渡动画：grid-template-rows 0fr↔1fr + opacity 淡入淡出 ── */
.cascader-accordion-enter-active,
.cascader-accordion-leave-active {
  transition: grid-template-rows 0.3s cubic-bezier(0.4, 0, 0.2, 1),
              opacity 0.25s ease;
}

.cascader-accordion-enter-from,
.cascader-accordion-leave-to {
  grid-template-rows: 0fr;
  opacity: 0;
}
</style>
