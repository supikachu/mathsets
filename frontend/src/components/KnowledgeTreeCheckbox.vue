<script setup lang="ts">
/**
 * KnowledgeTreeCheckbox — 手写递归多选树组件（组卷网风格 · 带连接线）
 *
 * 设计要点：
 * - 递归组件：通过 defineOptions({ name: 'KnowledgeTreeCheckbox' }) 允许组件自引用渲染无极嵌套 children
 * - 极简 API：props.nodes（当前层子节点）+ props.modelValue（已选 ID 数组，顶层单一数据源）
 * - 不做父子联动：知识点/章节/方法场景下用户需精确选择任意层级节点，联动会造成歧义
 * - 三态视觉：checked（自身已选）/ indeterminate（自身未选但子孙有选）/ unchecked
 * - 连接引导线：非根层 ul 加 padding-left，li::before 竖线 + li::after 横线，last-child 竖线截断
 * - 点击分离：点箭头→折叠/展开；点复选框→切换勾选；点文字→父节点折叠/叶节点勾选
 * - 零第三方 UI 库依赖：原生 <input type="checkbox"> + CSS 自定义样式，配合主题变量支持暗色模式
 */
import { ref, computed, watch } from 'vue'
import { AppIcon } from '@/components/ui'
import type { KnowledgeNodeTreeNode } from '@/api/client'

// 关键：允许组件在 template 内递归自引用
defineOptions({ name: 'KnowledgeTreeCheckbox' })

const props = withDefaults(defineProps<{
  /** 当前层级的子节点数组（递归时由父层传入 node.children） */
  nodes: KnowledgeNodeTreeNode[]
  /** 已选节点 ID 数组（顶层透传，递归层透传，单一数据源） */
  modelValue: string[]
  /** 缩进层级（顶层 0，每深入一层 +1） */
  depth?: number
}>(), { depth: 0 })

const emit = defineEmits<{
  'update:modelValue': [ids: string[]]
}>()

// 已选集合：O(1) 查找
const selectedSet = computed(() => new Set(props.modelValue))

// 本实例直接子节点的展开态（默认全展开，切换树数据时重置）
const expandedIds = ref<Set<string>>(new Set())

watch(
  () => props.nodes,
  (newNodes) => {
    expandedIds.value = new Set(newNodes.map(n => n.id))
  },
  { immediate: true },
)

function isExpanded(node: KnowledgeNodeTreeNode): boolean {
  return expandedIds.value.has(node.id)
}

function toggleExpand(node: KnowledgeNodeTreeNode) {
  const next = new Set(expandedIds.value)
  if (next.has(node.id)) next.delete(node.id)
  else next.add(node.id)
  expandedIds.value = next
}

// 判断子孙节点中是否有任意已选（用于 indeterminate 三态）
function hasDescendantSelected(node: KnowledgeNodeTreeNode): boolean {
  for (const child of node.children) {
    if (selectedSet.value.has(child.id) || hasDescendantSelected(child)) return true
  }
  return false
}

// 三态：自身未选但子孙有选 → indeterminate
function isIndeterminate(node: KnowledgeNodeTreeNode): boolean {
  if (selectedSet.value.has(node.id)) return false
  return hasDescendantSelected(node)
}

// 切换当前节点选中态：只动自身，不联动父子
function toggle(node: KnowledgeNodeTreeNode) {
  const next = new Set(props.modelValue)
  if (next.has(node.id)) next.delete(node.id)
  else next.add(node.id)
  emit('update:modelValue', Array.from(next))
}

// 行点击：父节点→折叠/展开；叶节点→切换勾选
function onRowClick(node: KnowledgeNodeTreeNode) {
  if (node.children.length > 0) {
    toggleExpand(node)
  } else {
    toggle(node)
  }
}

// 递归子层 emit 时直接透传给顶层（modelValue 是单一数据源，无需合并）
function onChildUpdate(ids: string[]) {
  emit('update:modelValue', ids)
}
</script>

<template>
  <ul class="ktcb-list" :class="{ 'is-root': depth === 0 }">
    <li v-for="node in nodes" :key="node.id" class="ktcb-item">
      <div
        class="ktcb-row"
        :class="{ 'is-selected': selectedSet.has(node.id) }"
        @click="onRowClick(node)"
      >
        <button
          v-if="node.children.length > 0"
          type="button"
          class="ktcb-arrow"
          :class="{ 'is-expanded': isExpanded(node) }"
          :title="isExpanded(node) ? '折叠' : '展开'"
          @click.stop="toggleExpand(node)"
        >
          <AppIcon name="chevron-right" :size="12" />
        </button>
        <span v-else class="ktcb-arrow-spacer" />
        <input
          type="checkbox"
          class="ktcb-checkbox"
          :checked="selectedSet.has(node.id)"
          :indeterminate.prop="isIndeterminate(node)"
          @click.stop="toggle(node)"
        />
        <span class="ktcb-name">{{ node.name }}</span>
        <span v-if="node.question_count > 0" class="ktcb-count">{{ node.question_count }}</span>
      </div>
      <!-- 递归渲染子层（仅当展开时） -->
      <KnowledgeTreeCheckbox
        v-if="node.children.length > 0 && isExpanded(node)"
        :nodes="node.children"
        :model-value="modelValue"
        :depth="depth + 1"
        @update:model-value="onChildUpdate"
      />
    </li>
  </ul>
</template>

<style scoped>
/* ===== 列表容器 ===== */
.ktcb-list {
  list-style: none;
  margin: 0;
  padding: 0;
}

.ktcb-list.is-root {
  padding: 4px 2px;
}

/* 非根层：缩进 + 为连接线腾出空间 */
.ktcb-list:not(.is-root) {
  padding-left: 18px;
}

/* ===== 列表项：连接引导线（组卷网风格） ===== */
.ktcb-list:not(.is-root) > .ktcb-item {
  position: relative;
}

/* 竖直引导线：从项顶到底（last-child 截断到横线位置） */
.ktcb-list:not(.is-root) > .ktcb-item::before {
  content: '';
  position: absolute;
  left: -9px;
  top: 0;
  bottom: 0;
  border-left: 1px dashed #c5cdd9;
}

/* 水平连接线：从竖线到节点 */
.ktcb-list:not(.is-root) > .ktcb-item::after {
  content: '';
  position: absolute;
  left: -9px;
  top: 15px;
  width: 9px;
  border-top: 1px dashed #c5cdd9;
}

/* last-child：竖线仅延伸到横线处 */
.ktcb-list:not(.is-root) > .ktcb-item:last-child::before {
  height: 16px;
  bottom: auto;
}

[data-theme='dark'] .ktcb-list:not(.is-root) > .ktcb-item::before,
[data-theme='dark'] .ktcb-list:not(.is-root) > .ktcb-item::after {
  border-color: var(--border-color);
}

/* ===== 节点行 ===== */
.ktcb-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 10px;
  border-radius: 8px;
  cursor: pointer;
  transition: background 0.2s ease, box-shadow 0.2s ease, color 0.18s ease;
  font-size: 13px;
  color: var(--text-primary);
  user-select: none;
  min-height: 30px;
}

.ktcb-row:hover {
  background: #f0f4f8;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.04);
}

[data-theme='dark'] .ktcb-row:hover {
  background: var(--bg-hover);
}

/* 选中态：浅蓝渐变卡片 + 内阴影浮起感 */
.ktcb-row.is-selected {
  background: linear-gradient(135deg, var(--accent-light) 0%, rgba(255, 255, 255, 0.35) 100%);
  box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.35), 0 1px 2px rgba(0, 0, 0, 0.04);
  color: var(--accent);
  font-weight: 600;
}

[data-theme='dark'] .ktcb-row.is-selected {
  background: linear-gradient(135deg, var(--accent-light) 0%, rgba(0, 0, 0, 0.1) 100%);
  box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.08);
}

/* ===== 折叠/展开小箭头 ===== */
.ktcb-arrow {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  padding: 0;
  flex-shrink: 0;
  border-radius: 3px;
  transition: background 0.15s ease, color 0.15s ease, transform 0.2s cubic-bezier(0.4, 0, 0.2, 1);
}

.ktcb-arrow:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.ktcb-arrow.is-expanded {
  transform: rotate(90deg);
}

.ktcb-arrow.is-expanded:hover {
  transform: rotate(90deg);
  background: var(--bg-hover);
}

/* 叶子节点的占位（与箭头等宽，保持对齐） */
.ktcb-arrow-spacer {
  display: inline-block;
  width: 16px;
  height: 16px;
  flex-shrink: 0;
}

/* ===== 原生 checkbox 美化 ===== */
.ktcb-checkbox {
  appearance: none;
  -webkit-appearance: none;
  width: 15px;
  height: 15px;
  border-radius: 4px;
  border: 1.5px solid var(--border-strong, #d1d1d6);
  background: var(--bg-card);
  cursor: pointer;
  flex-shrink: 0;
  position: relative;
  transition: background 0.15s ease, border-color 0.15s ease;
  margin: 0;
}

.ktcb-checkbox:hover {
  border-color: var(--accent);
}

.ktcb-checkbox:checked {
  background: var(--accent);
  border-color: var(--accent);
}

.ktcb-checkbox:checked::after {
  content: '';
  position: absolute;
  left: 4px;
  top: 1px;
  width: 4px;
  height: 8px;
  border: solid #fff;
  border-width: 0 2px 2px 0;
  transform: rotate(45deg);
}

.ktcb-checkbox:indeterminate {
  background: var(--accent);
  border-color: var(--accent);
}

.ktcb-checkbox:indeterminate::after {
  content: '';
  position: absolute;
  left: 3px;
  top: 6px;
  width: 7px;
  height: 2px;
  background: #fff;
  border-radius: 1px;
}

/* ===== 节点名称 & 计数 ===== */
.ktcb-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ktcb-count {
  flex-shrink: 0;
  padding: 0 6px;
  border-radius: 9999px;
  background: var(--bg-active);
  color: var(--text-secondary);
  font-size: 10.5px;
  font-variant-numeric: tabular-nums;
  line-height: 1.6;
}

.ktcb-row.is-selected .ktcb-count {
  background: rgba(255, 255, 255, 0.35);
  color: var(--accent);
}

[data-theme='dark'] .ktcb-row.is-selected .ktcb-count {
  background: rgba(0, 0, 0, 0.2);
}
</style>
