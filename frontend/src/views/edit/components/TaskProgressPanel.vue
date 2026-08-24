<script setup lang="ts">
/**
 * V2.1.1 任务进度面板（F1 步骤 4）
 *
 * 展示：状态、处理计数（已处理/总数、成功/失败）、当前页/总页、当前题号；
 * 非终态时可取消。
 */
import { computed } from 'vue'
import { AppButton, AppIcon } from '@/components/ui'
import type { AiParseTaskDetail } from '@/api/client'

const props = defineProps<{
  task: AiParseTaskDetail | null
  statusText: string
  cancelling?: boolean
}>()

const emit = defineEmits<{ (e: 'cancel'): void }>()

const STATUS_LABELS: Record<string, { label: string; color: string }> = {
  pending: { label: '排队中', color: 'var(--text-secondary)' },
  processing: { label: '解析中', color: 'var(--accent)' },
  retrying: { label: '重试中', color: 'var(--warning)' },
  success: { label: '成功', color: 'var(--success)' },
  partial_success: { label: '部分成功', color: 'var(--warning)' },
  failed: { label: '失败', color: 'var(--danger)' },
  cancelled: { label: '已取消', color: 'var(--text-secondary)' },
  completed: { label: '完成', color: 'var(--success)' },
}

const statusMeta = computed(() => {
  const s = props.task?.status
  if (!s) return { label: '提交中', color: 'var(--text-secondary)' }
  return STATUS_LABELS[s] ?? { label: s, color: 'var(--text-secondary)' }
})

const isTerminal = computed(() =>
  ['success', 'partial_success', 'failed', 'cancelled'].includes(props.task?.status ?? ''),
)

const percent = computed(() => {
  const t = props.task
  if (!t || t.total_count <= 0) return 0
  return Math.min(100, Math.round((t.processed_count / t.total_count) * 100))
})

const progressText = computed(() => {
  const t = props.task
  if (!t) return ''
  if (t.total_count > 0) return `已处理 ${t.processed_count} / ${t.total_count} 题`
  if (t.current_page && t.total_pages) return `第 ${t.current_page} / ${t.total_pages} 页`
  return ''
})
</script>

<template>
  <div class="task-progress-panel">
    <div class="task-status-row">
      <span class="task-status" :style="{ color: statusMeta.color }">{{ statusMeta.label }}</span>
      <span class="task-status-text">{{ statusText }}</span>
    </div>

    <div v-if="task" class="task-stats">
      <div class="task-progress-bar">
        <div class="task-progress-fill" :style="{ width: percent + '%' }"></div>
      </div>
      <div class="task-stats-grid">
        <div class="task-stat">
          <span class="task-stat-value">{{ progressText || '—' }}</span>
        </div>
        <div class="task-stat">
          <span class="task-stat-label">成功</span>
          <span class="task-stat-value success">{{ task.success_count }}</span>
        </div>
        <div class="task-stat">
          <span class="task-stat-label">失败</span>
          <span class="task-stat-value danger">{{ task.failed_count }}</span>
        </div>
        <div v-if="task.current_page && task.total_pages" class="task-stat">
          <span class="task-stat-label">页面</span>
          <span class="task-stat-value">{{ task.current_page }}/{{ task.total_pages }}</span>
        </div>
        <div v-if="task.current_question_no" class="task-stat">
          <span class="task-stat-label">当前题号</span>
          <span class="task-stat-value">{{ task.current_question_no }}</span>
        </div>
      </div>
    </div>

    <div class="task-actions">
      <AppButton
        v-if="!isTerminal && task"
        variant="danger"
        size="sm"
        :loading="props.cancelling"
        @click="emit('cancel')"
      >
        <AppIcon name="x" :size="14" /> 取消解析
      </AppButton>
    </div>
  </div>
</template>

<style scoped>
.task-progress-panel {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.task-status-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.task-status {
  font-size: 14px;
  font-weight: 700;
}
.task-status-text {
  font-size: 13px;
  color: var(--text-secondary);
}
.task-progress-bar {
  height: 8px;
  background: var(--bg-input);
  border-radius: 4px;
  overflow: hidden;
}
.task-progress-fill {
  height: 100%;
  background: var(--accent);
  transition: width 0.4s;
  border-radius: 4px;
}
.task-stats {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.task-stats-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(80px, 1fr));
  gap: 8px;
}
.task-stat {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  background: var(--bg-secondary, var(--bg-input));
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 8px 6px;
}
.task-stat-label {
  font-size: 11px;
  color: var(--text-secondary);
}
.task-stat-value {
  font-size: 14px;
  font-weight: 700;
  color: var(--text-primary);
}
.task-stat-value.success { color: var(--success); }
.task-stat-value.danger { color: var(--danger); }
.task-actions {
  display: flex;
  justify-content: flex-end;
}
</style>
