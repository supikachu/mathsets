<script setup lang="ts">
/**
 * KnowledgeTreeCheckbox — 手写递归多选树组件（极简纯粹版）
 *
 * 设计要点：
 * - 纯粹的多选树：无搜索框、无工具条、无过滤按钮，只呈现层级 + 勾选
 * - 扁平索引：根实例 DFS 一次构建 nodeIndex Map<nodeId, { parentId, childrenIds, name, namePath }>，
 *   经 provide/inject 共享给所有递归子实例；追溯复杂度 O(树深度)，杜绝逐节点递归
 * - 对称级联：勾选节点级联全选所有子孙；取消勾选级联清空所有子孙（显式 ID 存储，提交语义不变）
 * - 三态预计算：modelValue 变化时沿 parentId 链向上标记一次祖先集合（O(k·深度)），渲染期 O(1) 查表
 * - 层级感：每级 12px 缩进 + 浅灰虚线肘形引导线（#dcdfe6），组卷网标准文件树风格
 * - 文本策略：节点名称自然换行（white-space: normal + word-break: break-word，line-height: 20px），
 *   不做单行截断；行 Flex 顶部对齐，箭头/14px 复选框 margin-top 锚定首行文字中心
 * - 点击分离：点行（文字区）→ 级联勾选；点小箭头 → 折叠/展开；checkbox 与行同效
 * - AI 高亮：props.highlightIds 命中行渲染浅金色微光背景（AI 打标新增节点视觉反馈）
 */
import { ref, computed, watch, provide, inject, nextTick, onBeforeUnmount } from 'vue'
import type { ComputedRef, Ref } from 'vue'
import { AppIcon } from '@/components/ui'
import type { KnowledgeNodeTreeNode } from '@/api/client'

// 关键：允许组件在 template 内递归自引用
defineOptions({ name: 'KnowledgeTreeCheckbox' })

// ─────────────────────────────────────────────────────────────────────
// 扁平索引与共享上下文
// ─────────────────────────────────────────────────────────────────────
interface TreeNodeMeta {
  parentId: string | null
  childrenIds: string[]
  name: string
  /** 完整知识路径（如「集合与常用逻辑用语 / 集合 / 集合的概念」），用于 title 悬浮提示 */
  namePath: string
}

interface KtcbCtx {
  nodeIndex: Ref<Map<string, TreeNodeMeta>>
  selectedSet: ComputedRef<Set<string>>
  /** 已选节点的全部祖先集合（用于 indeterminate 三态，O(1) 查询） */
  indeterminateSet: ComputedRef<Set<string>>
  expandedIds: Ref<Set<string>>
  highlightSet: ComputedRef<Set<string>>
  /** 反向定位高亮集合（双击已选标签触发，短暂高亮定位节点） */
  locatingSet: ComputedRef<Set<string>>
  toggle: (id: string) => void
  toggleExpand: (id: string) => void
  isExpanded: (node: KnowledgeNodeTreeNode) => boolean
  metaOf: (id: string) => TreeNodeMeta | undefined
}

const TREE_CTX_KEY = Symbol('ktcb-ctx')

const props = withDefaults(defineProps<{
  /** 当前层级的子节点数组（递归时由父层传入 node.children） */
  nodes: KnowledgeNodeTreeNode[]
  /** 已选节点 ID 数组（顶层单一数据源，递归层透传） */
  modelValue: string[]
  /** 缩进层级（顶层 0，每深入一层 +1） */
  depth?: number
  /** AI 打标新增的节点 ID（仅根实例读取，浅金色高亮） */
  highlightIds?: string[]
}>(), { depth: 0, highlightIds: () => [] })

const emit = defineEmits<{
  'update:modelValue': [ids: string[]]
}>()

// 非空 = 当前实例是递归子层，直接复用根上下文；null = 当前实例是根，负责创建并 provide
const parentCtx = inject<KtcbCtx | null>(TREE_CTX_KEY, null)
const isRoot = !parentCtx

// ─────────────────────────────────────────────────────────────────────
// 组件级状态（所有实例均持有；根实例负责构建与 provide，子实例复用根 ctx）
// ─────────────────────────────────────────────────────────────────────
const nodeIndex = ref<Map<string, TreeNodeMeta>>(new Map())
const expandedIds = ref<Set<string>>(new Set())
const selectedSet = computed(() => new Set(props.modelValue))
const highlightSet = computed(() => new Set(props.highlightIds))

// 反向定位：locatingId 命中行短暂高亮（双击已选标签触发）
const locatingId = ref<string | null>(null)
const locatingSet = computed(() => {
  const s = new Set<string>()
  if (locatingId.value) s.add(locatingId.value)
  return s
})
/** 高亮移除定时器（重复定位时清理重置，确保动画重播） */
let locateTimer: number | null = null
/** 平滑滚动延迟定时器（等面板展开动画完成） */
let scrollTimer: number | null = null

/**
 * 反向定位：展开到指定节点并滚动至可视区居中。
 * 供父组件（编辑面板双击已选标签）通过 defineExpose 调用。
 * @returns 节点存在于当前树并已定位返回 true；未找到返回 false
 */
async function expandTo(nodeId: string): Promise<boolean> {
  if (!nodeIndex.value.has(nodeId)) return false
  // 1. 沿 parentId 链收集全部祖先（从上到下），展开到目标层级
  const chain: string[] = []
  let cur = nodeIndex.value.get(nodeId)?.parentId ?? null
  while (cur) {
    chain.push(cur)
    cur = nodeIndex.value.get(cur)?.parentId ?? null
  }
  const next = new Set(expandedIds.value)
  chain.forEach(id => next.add(id))
  expandedIds.value = next
  // 2. 高亮重播：同节点重复定位时先清除（nextTick 后 DOM 已刷新），
  //    再重设 locatingId 强制 CSS 动画重新播放；重置 2s 移除定时器
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
  // 3. 等待树节点 DOM 渲染完成 + 面板展开动画结束（400ms）后平滑滚动居中
  //    （CSS.escape 防御：nodeId 含选择器元字符时避免 SyntaxError）
  await nextTick()
  scrollTimer = window.setTimeout(() => {
    scrollTimer = null
    document
      .querySelector(`[data-node-id="${CSS.escape(nodeId)}"]`)
      ?.scrollIntoView({ behavior: 'smooth', block: 'center' })
  }, 400)
  return true
}

// 卸载时清理全部定时器，避免组件销毁后回调残留
onBeforeUnmount(() => {
  if (locateTimer !== null) window.clearTimeout(locateTimer)
  if (scrollTimer !== null) window.clearTimeout(scrollTimer)
  locateTimer = null
  scrollTimer = null
})

// 【关键】defineExpose 是编译宏，必须在 <script setup> 顶层作用域调用；
// 放在嵌套 if/else 块内会失效（ReferenceError: defineExpose is not defined），
// 并导致下方 ctx 赋值中断、模板绑定崩溃（Cannot read properties of undefined）
defineExpose({ expandTo })

// ─────────────────────────────────────────────────────────────────────
// 根实例：构建上下文（子实例跳过，直接复用 parentCtx）
// ─────────────────────────────────────────────────────────────────────
let ctx: KtcbCtx
if (parentCtx) {
  ctx = parentCtx
} else {

  // 三态预计算：对每个已选 ID 沿 parentId 链向上标记祖先 → O(k·深度)
  const indeterminateSet = computed(() => {
    const sel = selectedSet.value
    const set = new Set<string>()
    for (const id of props.modelValue) {
      let cur = nodeIndex.value.get(id)?.parentId ?? null
      while (cur) {
        if (!sel.has(cur)) set.add(cur)
        cur = nodeIndex.value.get(cur)?.parentId ?? null
      }
    }
    return set
  })

  // DFS 一次构建扁平索引；切换树数据时重置展开态（默认收起，逐级探索）
  watch(
    () => props.nodes,
    (newNodes) => {
      const map = new Map<string, TreeNodeMeta>()
      const walk = (list: KnowledgeNodeTreeNode[], parentId: string | null, parentPath: string) => {
        for (const n of list) {
          const namePath = parentPath ? `${parentPath} / ${n.name}` : n.name
          map.set(n.id, {
            parentId,
            childrenIds: n.children.map(c => c.id),
            name: n.name,
            namePath,
          })
          if (n.children.length > 0) walk(n.children, n.id, namePath)
        }
      }
      walk(newNodes, null, '')
      nodeIndex.value = map
      expandedIds.value = new Set()
    },
    { immediate: true },
  )

  // 收集整个子树的子孙 ID（迭代 + childrenIds，O(子树大小)）
  function collectDescendants(meta: TreeNodeMeta): string[] {
    const out: string[] = []
    const stack = [...meta.childrenIds]
    while (stack.length > 0) {
      const cur = stack.pop()!
      out.push(cur)
      const m = nodeIndex.value.get(cur)
      if (m) stack.push(...m.childrenIds)
    }
    return out
  }

  // 对称级联：勾选 → 自身 + 所有子孙；取消 → 自身 + 所有子孙移除
  function toggle(id: string) {
    const meta = nodeIndex.value.get(id)
    const next = new Set(props.modelValue)
    if (next.has(id)) {
      next.delete(id)
      if (meta) for (const d of collectDescendants(meta)) next.delete(d)
    } else {
      next.add(id)
      if (meta) for (const d of collectDescendants(meta)) next.add(d)
    }
    emit('update:modelValue', Array.from(next))
  }

  function toggleExpand(id: string) {
    const next = new Set(expandedIds.value)
    if (next.has(id)) next.delete(id)
    else next.add(id)
    expandedIds.value = next
  }

  function isExpanded(node: KnowledgeNodeTreeNode): boolean {
    return expandedIds.value.has(node.id)
  }

  ctx = {
    nodeIndex,
    selectedSet,
    indeterminateSet,
    expandedIds,
    highlightSet,
    locatingSet,
    toggle,
    toggleExpand,
    isExpanded,
    metaOf: (id) => nodeIndex.value.get(id),
  }
  provide(TREE_CTX_KEY, ctx)
}

// ─────────────────────────────────────────────────────────────────────
// 模板绑定（根与子实例统一走 ctx；顶层同名状态为本实例 props 计算，
// 子实例必须使用 ctx 的根集合，故此处以 ctx 前缀解构避免重名）
// ─────────────────────────────────────────────────────────────────────
const ctxSelectedSet = ctx.selectedSet
const ctxIndeterminateSet = ctx.indeterminateSet
const ctxHighlightSet = ctx.highlightSet
const ctxLocatingSet = ctx.locatingSet

function isIndeterminate(node: KnowledgeNodeTreeNode): boolean {
  return !ctxSelectedSet.value.has(node.id) && ctxIndeterminateSet.value.has(node.id)
}

function onRowClick(node: KnowledgeNodeTreeNode) {
  ctx.toggle(node.id)
}

function onChildUpdate(ids: string[]) {
  emit('update:modelValue', ids)
}

function titleOf(node: KnowledgeNodeTreeNode): string {
  return ctx.metaOf(node.id)?.namePath ?? node.name
}
</script>

<template>
  <ul class="ktcb-list" :class="{ 'ktcb-list--root': isRoot }">
    <li v-for="node in nodes" :key="node.id" class="ktcb-item">
      <div
        class="ktcb-row"
        :class="{
          'ktcb-row--selected': ctxSelectedSet.has(node.id),
          'ktcb-row--ai': ctxHighlightSet.has(node.id),
          'ktcb-row--locating': ctxLocatingSet.has(node.id),
        }"
        :data-node-id="node.id"
        :title="titleOf(node)"
        @click="onRowClick(node)"
      >
        <!-- 展开/折叠箭头 -->
        <button
          v-if="node.children.length > 0"
          type="button"
          class="ktcb-arrow"
          :class="{ 'ktcb-arrow--open': ctx.isExpanded(node) }"
          :title="ctx.isExpanded(node) ? '折叠' : '展开'"
          @click.stop="ctx.toggleExpand(node.id)"
        >
          <AppIcon name="chevron-right" :size="12" />
        </button>
        <span v-else class="ktcb-arrow-spacer" />

        <!-- 复选框：class="ktcb-checkbox" 明确绑定 -->
        <input
          type="checkbox"
          class="ktcb-checkbox"
          :checked="ctxSelectedSet.has(node.id)"
          :indeterminate.prop="isIndeterminate(node)"
          tabindex="-1"
          @click.stop="ctx.toggle(node.id)"
        />

        <!-- 节点名称 -->
        <span class="ktcb-name">{{ node.name }}</span>
      </div>

      <!-- 递归渲染子层（仅当展开时） -->
      <KnowledgeTreeCheckbox
        v-if="node.children.length > 0 && ctx.isExpanded(node)"
        :nodes="node.children"
        :model-value="modelValue"
        :depth="depth + 1"
        @update:model-value="onChildUpdate"
      />
    </li>
  </ul>
</template>

<style>
/*
 * ⚠️ 不使用 scoped —— 因为本组件会递归调用自身。
 * Vue scoped 的 data-v-xxxx 属性只会被注入到当前实例直接渲染的 DOM 上，
 * 递归子实例渲染的 DOM 没有父实例的 scoped 属性，导致深层节点的
 * .ktcb-checkbox / .ktcb-list 等选择器全部失效。
 * 改为全局样式，用 .ktcb- 前缀严格命名空间隔离，杜绝全局污染。
 */

/* ===== 列表容器 ===== */
.ktcb-list {
  list-style: none;
  margin: 0;
  padding: 0;
}

.ktcb-list--root {
  padding: 4px 2px;
}

/* ===== 层级虚线引导线（核心：子层左侧 dashed 竖线） ===== */
/* 通过 .ktcb-item 内嵌的递归 .ktcb-list 实现，排除根层 */
.ktcb-item > .ktcb-list {
  margin-left: 8px;
  padding-left: 6px;
  border-left: 1px dashed #dcdfe6;
}

[data-theme='dark'] .ktcb-item > .ktcb-list {
  border-left-color: #4c4c5e;
}

/* ===== 节点行：顶部对齐（多行文本首行锚定箭头/复选框） ===== */
.ktcb-row {
  display: flex;
  align-items: flex-start;  /* 顶部对齐，不因多行文本让图标居中漂移 */
  gap: 4px;
  padding: 3px 4px;
  border-radius: 5px;
  cursor: pointer;
  transition: background 0.18s ease, color 0.15s ease;
  font-size: 13px;
  color: var(--text-primary, #303133);
  user-select: none;
}

.ktcb-row:hover {
  background: #f0f4f8;
}

[data-theme='dark'] .ktcb-row:hover {
  background: var(--bg-hover, rgba(255, 255, 255, 0.06));
}

/* 选中态：浅蓝背景 + 主题色文字 */
.ktcb-row--selected {
  background: var(--accent-light, rgba(59, 130, 246, 0.08));
  color: var(--accent, #3b82f6);
  font-weight: 600;
}

[data-theme='dark'] .ktcb-row--selected {
  background: rgba(59, 130, 246, 0.12);
}

/* AI 高亮：浅金色微光 + 左侧金色描边 */
.ktcb-row--ai {
  background: rgba(250, 204, 21, 0.13);
  box-shadow: inset 2px 0 0 rgba(234, 179, 8, 0.7);
}

.ktcb-row--ai:hover {
  background: rgba(250, 204, 21, 0.2);
}

[data-theme='dark'] .ktcb-row--ai {
  background: rgba(250, 204, 21, 0.08);
}

/* ===== 反向定位高亮（双击已选标签触发，短暂金色脉冲） ===== */
.ktcb-row--locating {
  animation: ktcb-locate-pulse 2s ease-out;
  border-radius: 6px;
}
@keyframes ktcb-locate-pulse {
  0% { background: rgba(250, 204, 21, 0.45); box-shadow: 0 0 0 3px rgba(250, 204, 21, 0.35); }
  60% { background: rgba(250, 204, 21, 0.2); box-shadow: 0 0 0 2px rgba(250, 204, 21, 0.15); }
  100% { background: transparent; box-shadow: none; }
}

/* ===== 折叠/展开小箭头 ===== */
.ktcb-arrow {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  width: 14px;
  height: 14px;
  margin-top: 3px;
  border: none;
  background: transparent;
  color: var(--text-muted, #909399);
  cursor: pointer;
  padding: 0;
  border-radius: 3px;
  transition: color 0.15s, transform 0.2s cubic-bezier(0.4, 0, 0.2, 1);
}

.ktcb-arrow:hover {
  background: var(--bg-hover, rgba(0, 0, 0, 0.05));
  color: var(--text-primary, #303133);
}

.ktcb-arrow--open {
  transform: rotate(90deg);
}

/* 叶子节点占位（与箭头等宽保持文本列对齐） */
.ktcb-arrow-spacer {
  display: inline-block;
  flex-shrink: 0;
  width: 14px;
  height: 14px;
  margin-top: 3px;
}

/* ===== 复选框：14px 标准尺寸，完全清除浏览器默认样式 ===== */
.ktcb-checkbox {
  /* 清除浏览器默认的巨大复选框 */
  appearance: none !important;
  -webkit-appearance: none !important;

  /* 压制 base.css 全局 input { padding: 10px 14px; width: 100% } */
  padding: 0 !important;
  margin: 0;
  margin-top: 3px;

  /* 强制 13×13px，border-box 让尺寸包含 border */
  display: inline-block;
  box-sizing: border-box;
  width: 13px !important;
  height: 13px !important;
  min-width: 13px;
  min-height: 13px;
  max-width: 13px;
  max-height: 13px;
  flex-shrink: 0;

  /* 边框与圆角 */
  border: 1.5px solid #c0c4cc;
  border-radius: 3px;
  background: #fff;

  /* 用于 ::after 伪元素定位 */
  position: relative;

  cursor: pointer;
  transition: background 0.15s ease, border-color 0.15s ease;
}

[data-theme='dark'] .ktcb-checkbox {
  background: var(--bg-input, #2a2a3a);
  border-color: #4c4c5e;
}

.ktcb-checkbox:hover {
  border-color: var(--accent, #3b82f6);
}

/* ── checked：主题色背景 + 白色对勾 ── */
.ktcb-checkbox:checked {
  background: var(--accent, #3b82f6);
  border-color: var(--accent, #3b82f6);
}

.ktcb-checkbox:checked::after {
  content: '';
  position: absolute;
  /* 对勾适配 13px 容器 */
  left: 3px;
  top: 1px;
  width: 4px;
  height: 7px;
  border: 1.5px solid #fff;
  border-top: none;
  border-left: none;
  transform: rotate(45deg);
}

/* ── indeterminate（半选）：主题色背景 + 白色横杠 ── */
.ktcb-checkbox:indeterminate {
  background: var(--accent, #3b82f6);
  border-color: var(--accent, #3b82f6);
}

.ktcb-checkbox:indeterminate::after {
  content: '';
  position: absolute;
  /* 横杠适配 13px 容器：top = (13 - 2) / 2 = 5.5 → 取 5px */
  left: 2px;
  top: 5px;
  width: 7px;
  height: 1.5px;
  background: #fff;
  border-radius: 1px;
}

/* ===== 节点名称：允许自然换行，废弃单行截断 ===== */
.ktcb-name {
  flex: 1;
  min-width: 0;
  /* 允许换行，不截断 */
  white-space: normal;
  word-break: break-word;
  /* 行高 20px，与 margin-top:3px 共同决定首行对齐基准 */
  line-height: 20px;
}
</style>