<template>
  <div>
    <div
      class="kp-node"
      :class="{ active: selectedKpId === node.id }"
      :style="{ paddingLeft: level * 18 + 6 + 'px' }"
      @click="emit('select', node)"
    >
      <span
        v-if="hasChildren"
        class="kp-chevron"
        :class="{ expanded: isExpanded }"
        @click.stop="emit('toggle-expand', node)"
      >
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
          <path d="m9 18 6-6-6-6" />
        </svg>
      </span>
      <span v-else class="kp-chevron-placeholder" />
      <span class="kp-name">{{ node.name }}</span>
      <span v-if="hasChildren" class="kp-child-count">{{ node.children.length }}</span>
      <span class="kp-actions" @click.stop>
        <button class="kp-action-btn" @click="emit('edit', node)"><AppIcon name="pencil" :size="13" /></button>
        <button class="kp-action-btn" @click="emit('add-child', node)"><AppIcon name="plus" :size="13" /></button>
        <button class="kp-action-btn kp-action-danger" @click="emit('delete', node)"><AppIcon name="trash" :size="13" /></button>
      </span>
    </div>
    <Transition name="kp-expand">
      <div v-if="hasChildren && isExpanded" class="kp-children">
        <KpTreeNode
          v-for="child in node.children"
          :key="child.id"
          :node="child"
          :level="level + 1"
          :selected-kp-id="selectedKpId"
          :expanded="expanded"
          @select="emit('select', $event)"
          @toggle-expand="emit('toggle-expand', $event)"
          @edit="emit('edit', $event)"
          @add-child="emit('add-child', $event)"
          @delete="emit('delete', $event)"
        />
      </div>
    </Transition>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { AppIcon } from '@/components/ui'
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
  edit: [node: KnowledgePoint]
  'add-child': [node: KnowledgePoint]
  delete: [node: KnowledgePoint]
}>()

const hasChildren = computed(() => (props.node.children?.length ?? 0) > 0)
const isExpanded = computed(() => props.expanded[props.node.id] === true)
</script>

<style scoped>
.kp-node {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 6px 8px;
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition: background 0.15s ease, color 0.15s ease;
  font-size: 13.5px;
  position: relative;
}

.kp-node:hover {
  background: var(--bg-hover);
}

.kp-node.active {
  background: var(--accent-light);
  color: var(--accent);
}

.kp-chevron {
  flex-shrink: 0;
  width: 16px;
  height: 16px;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  user-select: none;
  color: var(--text-muted);
  border-radius: 4px;
  transition: transform 0.28s cubic-bezier(0.4, 0, 0.2, 1), color 0.15s ease, background 0.15s ease;
  transform: rotate(0deg);
}

.kp-chevron:hover {
  color: var(--text-primary);
  background: var(--bg-active);
}

.kp-chevron.expanded {
  transform: rotate(90deg);
}

.kp-chevron-placeholder {
  flex-shrink: 0;
  width: 16px;
  height: 16px;
}

.kp-name {
  flex: 1;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  letter-spacing: -0.01em;
}

.kp-node.active .kp-name {
  font-weight: 600;
}

.kp-child-count {
  font-size: 10.5px;
  font-weight: 600;
  color: var(--text-muted);
  background: var(--bg-active);
  border-radius: var(--radius-full);
  padding: 1px 7px;
  flex-shrink: 0;
  line-height: 1.5;
  font-feature-settings: 'tnum';
}

.kp-node.active .kp-child-count {
  background: rgba(0, 113, 227, 0.15);
  color: var(--accent);
}

.kp-actions {
  display: none;
  align-items: center;
  gap: 2px;
  flex-shrink: 0;
  margin-left: 2px;
}

.kp-node:hover .kp-actions {
  display: flex;
}

.kp-action-btn {
  width: 22px;
  height: 22px;
  border-radius: var(--radius-xs);
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  color: var(--text-muted);
  transition: var(--transition-fast);
}

.kp-action-btn:hover {
  background: var(--bg-active);
  color: var(--text-primary);
}

.kp-action-danger:hover {
  background: var(--danger-light);
  color: var(--danger);
}

/* Expand/collapse transition */
.kp-expand-enter-active,
.kp-expand-leave-active {
  overflow: hidden;
  transition: max-height 0.3s cubic-bezier(0.4, 0, 0.2, 1), opacity 0.2s ease;
}

.kp-expand-enter-from,
.kp-expand-leave-to {
  max-height: 0;
  opacity: 0;
}

.kp-expand-enter-to,
.kp-expand-leave-from {
  max-height: 2000px;
  opacity: 1;
}
</style>
