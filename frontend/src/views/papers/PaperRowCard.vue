<script setup lang="ts">
import { computed } from 'vue'
import { AppIcon } from '@/components/ui'
import type { PaperSummary } from '@/api/client'
import { displayPaperSource } from '@/utils/questionSource'

const props = defineProps<{
  paper: PaperSummary
}>()

const emit = defineEmits<{
  open: []
  download: []
  fill: []
  analysis: []
}>()

const sourceLabel = computed(() =>
  displayPaperSource(props.paper.source_type, props.paper.sub_source_type) || '试卷',
)

const region = computed(() =>
  [props.paper.region_province, props.paper.region_city].filter(Boolean).join('') || '',
)

const dateText = computed(() => {
  const d = props.paper.updated_at
  if (!d) return ''
  const dt = new Date(d)
  if (Number.isNaN(dt.getTime())) return d.slice(0, 10).replace(/-/g, '/')
  const y = dt.getFullYear()
  const m = String(dt.getMonth() + 1).padStart(2, '0')
  const day = String(dt.getDate()).padStart(2, '0')
  return `${y}/${m}/${day}`
})
</script>

<template>
  <article class="q-card paper-card" @click="emit('open')">
    <div class="paper-card-inner">
      <!-- 左侧：试卷主体信息 -->
      <div class="paper-info-col">
        <h3 class="paper-title" :title="paper.title">
          {{ paper.title }}
        </h3>
        <div class="paper-meta-row">
          <span class="paper-stat-item">
            <AppIcon name="file-text" :size="13" class="text-blue-500" />
            <span>总题量：</span>
            <strong class="text-blue-600 dark:text-blue-400">{{ paper.question_count ?? 0 }} 题</strong>
          </span>
          <span v-if="dateText" class="paper-stat-item text-gray-400">
            <AppIcon name="calendar" :size="13" />
            <span>更新时间：{{ dateText }}</span>
          </span>
          <span v-if="paper.grade" class="paper-meta-pill">{{ paper.grade }}</span>
          <span v-if="paper.year" class="paper-meta-pill">{{ paper.year }}年</span>
          <span v-if="region" class="paper-meta-pill">{{ region }}</span>
          <span v-if="sourceLabel" class="paper-meta-pill">{{ sourceLabel }}</span>
          <span v-if="paper.school_name" class="paper-meta-pill">{{ paper.school_name }}</span>
        </div>
      </div>

      <!-- 右侧：功能按钮组（下载、补录、分析） -->
      <div class="paper-actions-col" @click.stop>
        <button
          type="button"
          class="paper-action-btn"
          title="下载试卷"
          @click="emit('download')"
        >
          <AppIcon name="download" :size="14" />
          <span>下载</span>
        </button>
        <button
          type="button"
          class="paper-action-btn"
          title="补录试题"
          @click="emit('fill')"
        >
          <AppIcon name="plus" :size="14" />
          <span>补录</span>
        </button>
        <button
          type="button"
          class="paper-action-btn"
          title="试卷分析"
          @click="emit('analysis')"
        >
          <AppIcon name="chart" :size="14" />
          <span>分析</span>
        </button>
      </div>
    </div>
  </article>
</template>

<style scoped>
/* ===== 试卷卡片（纯净单体卡片，只保留主体与右侧操作） ===== */
.paper-card {
  position: relative;
  background: var(--bg-card);
  border-radius: 12px;
  border: 1px solid var(--border-color);
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.04);
  transition: transform 0.26s cubic-bezier(0.25, 0.8, 0.25, 1), box-shadow 0.26s ease, border-color 0.26s ease;
  cursor: pointer;
  margin-bottom: 14px;
}

.paper-card:hover {
  transform: translateY(-2px);
  box-shadow: 0 8px 24px rgba(149, 157, 165, 0.15);
  border-color: rgba(0, 113, 227, 0.3);
}

[data-theme='dark'] .paper-card {
  border-color: rgba(255, 255, 255, 0.08);
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.3);
}

[data-theme='dark'] .paper-card:hover {
  border-color: rgba(10, 132, 255, 0.4);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45);
}

.paper-card-inner {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 18px 22px;
  gap: 20px;
}

/* ---- 左侧信息列 ---- */
.paper-info-col {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.paper-title {
  margin: 0;
  font-size: 15.5px;
  font-weight: 650;
  color: var(--text-primary);
  line-height: 1.45;
  letter-spacing: -0.01em;
  transition: color 0.18s ease;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.paper-card:hover .paper-title {
  color: var(--accent);
}

.paper-meta-row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 12px;
  font-size: 12.5px;
  color: var(--text-secondary);
}

.paper-stat-item {
  display: inline-flex;
  align-items: center;
  gap: 5px;
}

.paper-meta-pill {
  display: inline-flex;
  align-items: center;
  padding: 2px 8px;
  border-radius: 4px;
  font-size: 11.5px;
  color: var(--text-muted);
  background: var(--bg-hover);
}

/* ---- 右侧按钮组 ---- */
.paper-actions-col {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.paper-action-btn {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 6px 13px;
  border-radius: 8px;
  background: var(--bg-hover);
  border: 1px solid var(--border-color);
  color: var(--text-secondary);
  font-size: 13px;
  font-weight: 500;
  white-space: nowrap;
  transition: var(--transition-fast);
  cursor: pointer;
}

.paper-action-btn:hover {
  background: var(--accent-light);
  border-color: rgba(0, 113, 227, 0.3);
  color: var(--accent);
}

.paper-action-btn:active {
  transform: scale(0.96);
}

@media (max-width: 768px) {
  .paper-card-inner {
    flex-direction: column;
    align-items: flex-start;
    gap: 14px;
  }
  .paper-actions-col {
    width: 100%;
    justify-content: flex-end;
  }
  .paper-title {
    white-space: normal;
  }
}
</style>
