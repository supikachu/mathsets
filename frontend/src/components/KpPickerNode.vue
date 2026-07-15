<template>
  <div>
    <div
      class="kp-picker-node"
      :class="{ active: selectedKpId === node.id, 'has-children': hasChildren }"
      :style="{ paddingLeft: level * 16 + 8 + 'px' }"
      @click="hasChildren ? emit('toggle-expand', node) : emit('select', node)"
    >
      <!-- 树干引导虚线（子节点 level >= 1） -->
      <span v-if="level > 0" class="kp-picker-tree-line" :style="{ left: (level - 1) * 16 + 14 + 'px' }" />
      <!-- 展开折叠图标 -->
      <span
        v-if="hasChildren"
        class="kp-picker-chevron"
        :class="{ expanded: isExpanded }"
        @click.stop="emit('toggle-expand', node)"
      >
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
          <path d="m9 18 6-6-6-6" />
        </svg>
      </span>
      <span v-else class="kp-picker-leaf-dot" />
      <!-- 节点名称（点击选中） -->
      <span class="kp-picker-name" @click.stop="emit('select', node)">{{ node.name }}</span>
      <span v-if="hasChildren" class="kp-picker-count">{{ node.children.length }}</span>
    </div>
    <Transition name="kp-picker-expand">
      <div v-if="hasChildren && isExpanded" class="kp-picker-children">
        <KpPickerNode
          v-for="child in node.children"
          :key="child.id"
          :node="child"
          :level="level + 1"
          :selected-kp-id="selectedKpId"
          :expanded="expanded"
          @select="emit('select', $event)"
          @toggle-expand="emit('toggle-expand', $event)"
        />
      </div>
    </Transition>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import type { KnowledgePoint } from '@/api/client'

const props = defineProps<{
  node: KnowledgePoint
  level: number
  selectedKpId: string | null
  expanded: Record<string, boolean>
}>()

const emit = defineEmits<{
  select: [node: KnowledgePoint]
  'toggle-expand': [node: KnowledgePoint]
}>()

const hasChildren = computed(() => (props.node.children?.length ?? 0) > 0)
const isExpanded = computed(() => props.expanded[props.node.id] === true)
</script>

<style scoped>
.kp-picker-node {
  display: flex;
  align-items: center;
  gap: 5px;
  padding: 5px 10px;
  border-radius: 6px;
  cursor: pointer;
  font-size: 13px;
  color: var(--text-secondary);
  transition: background 0.12s, color 0.12s;
  user-select: none;
  position: relative;
}

.kp-picker-node:hover {
  background: rgba(0, 0, 0, 0.04);
  color: var(--text-primary);
}

[data-theme='dark'] .kp-picker-node:hover {
  background: rgba(255, 255, 255, 0.06);
}

.kp-picker-node.active {
  background: var(--purple-light);
  color: var(--purple);
  font-weight: 600;
}

/* 树干引导虚线 */
.kp-picker-tree-line {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 0;
  border-left: 1px dashed #e5e5ea;
  pointer-events: none;
}

[data-theme='dark'] .kp-picker-tree-line {
  border-left-color: rgba(255, 255, 255, 0.1);
}

/* Chevron 展开折叠图标 */
.kp-picker-chevron {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  flex-shrink: 0;
  color: var(--text-muted);
  transition: transform 0.2s cubic-bezier(0.4, 0, 0.2, 1);
  cursor: pointer;
}

.kp-picker-chevron.expanded {
  transform: rotate(90deg);
}

/* 叶子节点圆点 */
.kp-picker-leaf-dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--text-muted);
  opacity: 0.3;
  flex-shrink: 0;
  margin-left: 5px;
  margin-right: 1px;
}

.kp-picker-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  cursor: pointer;
}

.kp-picker-count {
  font-size: 10px;
  color: var(--text-muted);
  background: var(--bg-tertiary);
  border-radius: 8px;
  padding: 1px 6px;
  flex-shrink: 0;
}

.kp-picker-node.active .kp-picker-count {
  background: rgba(175, 82, 222, 0.15);
  color: var(--purple);
}

.kp-picker-children {
  overflow: hidden;
}

.kp-picker-expand-enter-active,
.kp-picker-expand-leave-active {
  transition: max-height 0.2s ease, opacity 0.2s ease;
  max-height: 500px;
}

.kp-picker-expand-enter-from,
.kp-picker-expand-leave-to {
  max-height: 0;
  opacity: 0;
}
</style>
