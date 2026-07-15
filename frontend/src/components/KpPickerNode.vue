<template>
  <div>
    <div
      class="kp-picker-node"
      :class="{ active: selectedKpId === node.id }"
      :style="{ paddingLeft: level * 18 + 8 + 'px' }"
      @click="emit('select', node)"
    >
      <span
        v-if="hasChildren"
        class="kp-picker-chevron"
        :class="{ expanded: isExpanded }"
        @click.stop="emit('toggle-expand', node)"
      >
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
          <path d="m9 18 6-6-6-6" />
        </svg>
      </span>
      <span v-else class="kp-picker-chevron-placeholder" />
      <span class="kp-picker-name">{{ node.name }}</span>
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
  gap: 4px;
  padding: 5px 8px;
  border-radius: 6px;
  cursor: pointer;
  font-size: 13px;
  color: var(--text-secondary);
  transition: background 0.12s, color 0.12s;
  user-select: none;
}

.kp-picker-node:hover {
  background: var(--accent-light);
  color: var(--accent);
}

.kp-picker-node.active {
  background: var(--accent);
  color: #fff;
  font-weight: 600;
}

.kp-picker-chevron {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  flex-shrink: 0;
  color: var(--text-muted);
  transition: transform 0.15s;
  cursor: pointer;
}

.kp-picker-chevron.expanded {
  transform: rotate(90deg);
}

.kp-picker-chevron-placeholder {
  width: 16px;
  flex-shrink: 0;
}

.kp-picker-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
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
  background: rgba(255, 255, 255, 0.25);
  color: rgba(255, 255, 255, 0.8);
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
