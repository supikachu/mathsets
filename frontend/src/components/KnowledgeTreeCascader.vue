<script setup lang="ts">
/**
 * KnowledgeTreeCascader — 知识点树形多选器
 *
 * 用途：替代旧 KpPickerNode，作为录题面板和列表筛选的核心知识点选择器。
 *
 * 设计要点：
 * - 复用 AppIcon / AppEmpty，不引入任何第三方 UI 库
 * - 触发器风格对齐 AppSelect（Apple 风格圆角输入框）
 * - Popover 用 Teleport + fixed 定位，避免父容器裁剪
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
} from '@/api/client'

const props = withDefaults(
  defineProps<{
    /** 选中节点 ID 列表 */
    modelValue: string[]
    /** 锁定知识树 ID（不传则允许用户在多棵树之间切换） */
    treeId?: string
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
const triggerRef = ref<HTMLElement | null>(null)
const popoverRef = ref<HTMLElement | null>(null)
const popoverStyle = ref({ top: '0px', left: '0px', width: '320px' })

const trees = ref<KnowledgeTree[]>([])
const activeTreeId = ref<string>('')
const treeData = ref<KnowledgeNodeTreeNode[]>([])
const loading = ref(false)

const searchText = ref('')
const expandedIds = ref<Set<string>>(new Set())
const selectedIds = ref<Set<string>>(new Set(props.modelValue))

/** ID → 节点，用于回显已选名称（即便节点不在当前树里也能展示） */
const nodeMap = ref<Map<string, KnowledgeNodeTreeNode>>(new Map())

// ─── 计算属性 ─────────────────────────────────────────────────────────
const showTreeSelector = computed(() => !props.treeId && trees.value.length > 1)

const selectedNodes = computed(() =>
  Array.from(selectedIds.value)
    .map((id) => nodeMap.value.get(id))
    .filter((n): n is KnowledgeNodeTreeNode => !!n),
)

const activeTreeName = computed(() =>
  trees.value.find((t) => t.id === activeTreeId.value)?.name ?? '',
)

interface FlatItem {
  node: KnowledgeNodeTreeNode
  depth: number
}

/** 扁平化展示列表：搜索时强制展开所有匹配项及其祖先链 */
const filteredFlatList = computed<FlatItem[]>(() => {
  const result: FlatItem[] = []
  const q = searchText.value.trim().toLowerCase()

  function walk(nodes: KnowledgeNodeTreeNode[], depth: number, ancestorMatched: boolean) {
    for (const n of nodes) {
      const name = n.name.toLowerCase()
      const code = (n.code || '').toLowerCase()
      const selfMatched = !q || name.includes(q) || code.includes(q)
      const shouldShow = selfMatched || ancestorMatched
      if (shouldShow) {
        result.push({ node: n, depth })
      }
      // 搜索时：任一祖先或自身匹配，则展开子树；否则尊重 expandedIds
      const shouldRecurse =
        shouldShow && (q ? true : expandedIds.value.has(n.id))
      if (shouldRecurse && n.children.length > 0) {
        walk(n.children, depth + 1, selfMatched || ancestorMatched)
      }
    }
  }

  walk(treeData.value, 0, false)
  return result
})

// ─── 方法 ──────────────────────────────────────────────────────────────
function updatePopoverPosition() {
  if (!triggerRef.value) return
  const rect = triggerRef.value.getBoundingClientRect()
  const spaceBelow = window.innerHeight - rect.bottom
  const maxH = Math.min(420, Math.max(280, spaceBelow - 8))
  popoverStyle.value = {
    top: `${rect.bottom + 4}px`,
    left: `${rect.left}px`,
    width: `${Math.max(320, rect.width)}px`,
  }
  if (popoverRef.value) {
    popoverRef.value.style.maxHeight = `${maxH}px`
  }
}

function toggle() {
  if (props.disabled) return
  open.value = !open.value
  if (open.value) {
    nextTick(updatePopoverPosition)
  }
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

function onClickOutside(e: MouseEvent) {
  const target = e.target as Node
  if (triggerRef.value?.contains(target)) return
  if (popoverRef.value?.contains(target)) return
  open.value = false
}

function onEscape(e: KeyboardEvent) {
  if (e.key === 'Escape' && open.value) {
    open.value = false
    triggerRef.value?.focus()
  }
}

function onScrollOrResize() {
  if (open.value) updatePopoverPosition()
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
    const res = await knowledgeTreeApi.list()
    trees.value = res.data
    if (props.treeId) {
      activeTreeId.value = props.treeId
    } else if (trees.value.length > 0 && !activeTreeId.value) {
      activeTreeId.value = trees.value[0].id
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
    treeData.value = res.data
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
watch(
  () => props.treeId,
  (newId) => {
    if (newId) activeTreeId.value = newId
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

watch(open, (val) => {
  if (val) {
    window.addEventListener('scroll', onScrollOrResize, true)
    window.addEventListener('resize', onScrollOrResize)
  } else {
    window.removeEventListener('scroll', onScrollOrResize, true)
    window.removeEventListener('resize', onScrollOrResize)
  }
})

onMounted(async () => {
  document.addEventListener('click', onClickOutside)
  document.addEventListener('keydown', onEscape)
  await loadTrees()
  if (activeTreeId.value) await loadTreeData()
})

onBeforeUnmount(() => {
  document.removeEventListener('click', onClickOutside)
  document.removeEventListener('keydown', onEscape)
  window.removeEventListener('scroll', onScrollOrResize, true)
  window.removeEventListener('resize', onScrollOrResize)
})
</script>

<template>
  <div class="kt-cascader" :class="{ disabled }">
    <!-- 触发器 -->
    <button
      ref="triggerRef"
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

    <!-- 已选 chips -->
    <div v-if="selectedNodes.length > 0" class="cascader-chips">
      <span v-for="n in selectedNodes" :key="n.id" class="chip">
        <span class="chip-name">{{ n.name }}</span>
        <button class="chip-x" type="button" @click="toggleSelect(n.id)">
          <AppIcon name="x" :size="11" />
        </button>
      </span>
    </div>

    <!-- Popover -->
    <Teleport to="body">
      <Transition name="cascader-pop">
        <div
          v-if="open"
          ref="popoverRef"
          class="cascader-popover"
          :style="popoverStyle"
        >
          <!-- 搜索栏 -->
          <div class="pop-search">
            <AppIcon name="search" :size="13" class="pop-search-icon" />
            <input
              v-model="searchText"
              placeholder="搜索知识点…"
              class="pop-search-input"
            />
          </div>

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

          <!-- 树列表 -->
          <div class="pop-tree-list">
            <div v-if="loading" class="pop-loading">加载中…</div>
            <AppEmpty v-else-if="filteredFlatList.length === 0" description="无匹配知识点" />
            <template v-else>
              <div
                v-for="item in filteredFlatList"
                :key="item.node.id"
                class="pop-row"
                :class="{ selected: isSelected(item.node.id) }"
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
              <button type="button" class="footer-btn primary" @click="open = false">
                完成
              </button>
            </div>
          </div>
        </div>
      </Transition>
    </Teleport>
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

/* ── Popover ── */
.cascader-popover {
  position: fixed;
  z-index: 10000;
  display: flex;
  flex-direction: column;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  box-shadow: var(--shadow-lg);
  backdrop-filter: var(--blur-modal);
  -webkit-backdrop-filter: var(--blur-modal);
  overflow: hidden;
}

.pop-search {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 12px;
  border-bottom: 1px solid var(--divider);
}

.pop-search-icon {
  color: var(--text-muted);
  flex-shrink: 0;
}

.pop-search-input {
  flex: 1;
  border: none;
  outline: none;
  background: transparent;
  color: var(--text-primary);
  font-size: 13px;
  font-family: inherit;
}

.pop-search-input::placeholder {
  color: var(--text-muted);
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

.pop-tree-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
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

/* 滚动条 */
.pop-tree-list::-webkit-scrollbar {
  width: 5px;
}
.pop-tree-list::-webkit-scrollbar-track {
  background: transparent;
}
.pop-tree-list::-webkit-scrollbar-thumb {
  background: var(--border-color);
  border-radius: 3px;
}

/* ── 过渡动画 ── */
.cascader-pop-enter-active {
  transition: opacity 0.15s ease, transform 0.15s ease;
}
.cascader-pop-leave-active {
  transition: opacity 0.1s ease, transform 0.1s ease;
}
.cascader-pop-enter-from {
  opacity: 0;
  transform: translateY(-4px) scale(0.98);
}
.cascader-pop-leave-to {
  opacity: 0;
  transform: translateY(-2px) scale(0.98);
}

/* ── 响应式：小屏收紧宽度 ── */
@media (max-width: 480px) {
  .cascader-popover {
    width: calc(100vw - 16px) !important;
    left: 8px !important;
  }
}
</style>
