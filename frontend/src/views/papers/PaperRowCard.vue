<script setup lang="ts">
import { computed } from 'vue'
import { AppIcon } from '@/components/ui'
import AppBadge from '@/components/ui/AppBadge.vue'
import type { PaperSummary } from '@/api/client'
import { displayPaperSource } from '@/utils/questionSource'

const props = defineProps<{
  paper: PaperSummary
}>()

const emit = defineEmits<{
  open: []
  fill: []
  viewQuestions: []
}>()

const sourceLabel = computed(() =>
  displayPaperSource(props.paper.source_type, props.paper.sub_source_type) || '试卷',
)

const typeBadgeColor = computed(() => {
  const t = `${props.paper.source_type || ''} ${props.paper.sub_source_type || ''}`
  if (/gaokao|高考|真题/.test(t)) return 'purple'
  if (/mock|模拟/.test(t)) return 'teal'
  if (/midterm|期中/.test(t)) return 'blue'
  if (/final|期末/.test(t)) return 'green'
  return 'blue'
})

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

const sourceMeta = computed(() => {
  const parts = [
    props.paper.year ? `${props.paper.year}年` : '',
    props.paper.grade || '',
    region.value,
    sourceLabel.value,
  ].filter(Boolean)
  return parts.join(' · ')
})
</script>

<template>
  <article class="q-card paper-card" @click="emit('open')">
    <!-- 来源角标：贴左上角边缘，绝对定位 -->
    <span v-if="sourceMeta" class="q-source-badge" :title="sourceMeta">
      {{ sourceMeta }}
    </span>

    <!-- 学校角标：贴右上角边缘 -->
    <span v-if="paper.school_name" class="q-school-tag" :title="paper.school_name">
      <AppIcon name="landmark" :size="11" :stroke="1.6" />
      <span class="q-school-name">{{ paper.school_name }}</span>
    </span>

    <!-- Row 1: Header 属性标签 -->
    <div class="q-card-header">
      <div class="q-card-tags">
        <AppBadge :color="typeBadgeColor" class="flex-shrink-0">
          {{ sourceLabel }}
        </AppBadge>
        <span v-if="paper.grade" class="q-ghost-tag flex-shrink-0">
          <span class="q-dot q-dot--blue"></span>
          {{ paper.grade }}
        </span>
        <span v-if="paper.year" class="q-ghost-tag flex-shrink-0">
          <span class="q-dot q-dot--emerald"></span>
          {{ paper.year }}年
        </span>
        <span v-if="region" class="q-ghost-tag flex-shrink-0">
          <span class="q-dot q-dot--purple"></span>
          {{ region }}
        </span>
      </div>
    </div>

    <!-- Row 2: Body — 试卷标题与信息 -->
    <div class="q-card-body paper-body">
      <h3 class="paper-title">{{ paper.title }}</h3>
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
      </div>
    </div>

    <!-- Row 3: Footer — 底部操作栏 -->
    <div class="q-card-footer flex items-center justify-between w-full gap-2">
      <div class="q-footer-info flex items-center gap-2">
        <span class="paper-tag-pill">含参考答案与解析</span>
      </div>

      <div class="q-actions flex items-center gap-2">
        <button
          type="button"
          class="q-action-btn q-action--ghost"
          @click.stop="emit('viewQuestions')"
        >
          <AppIcon name="search" :size="13" />
          <span>查看试题</span>
        </button>
        <button
          type="button"
          class="q-action-btn"
          @click.stop="emit('fill')"
        >
          <AppIcon name="plus" :size="13" />
          <span>补录</span>
        </button>
        <button
          type="button"
          class="q-action-btn q-action--primary"
          @click.stop="emit('open')"
        >
          <AppIcon name="external-link" :size="13" />
          <span>打开试卷</span>
        </button>
      </div>
    </div>
  </article>
</template>

<style scoped>
/* ===== 试卷卡片（完全对齐 q-card 规范） ===== */
.paper-card {
  position: relative;
  background: var(--bg-card);
  border-radius: 12px;
  border: 1px solid var(--border-color);
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.04);
  transition: transform 0.3s cubic-bezier(0.25, 0.8, 0.25, 1), box-shadow 0.3s ease, border-color 0.3s ease;
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

/* ---- 来源角标 ---- */
.q-source-badge {
  position: absolute;
  top: 0;
  left: 0;
  max-width: 70%;
  padding: 4px 16px 4px 12px;
  font-size: 11px;
  line-height: 1.4;
  color: var(--text-muted);
  background: rgba(100, 116, 139, 0.08);
  border-radius: 12px 0 6px 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  pointer-events: none;
  z-index: 2;
}

/* ---- 学校角标 ---- */
.q-school-tag {
  position: absolute;
  top: 0;
  right: 0;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  max-width: 38%;
  padding: 4px 12px 4px 14px;
  font-size: 11px;
  line-height: 1.4;
  color: var(--text-muted);
  background: rgba(100, 116, 139, 0.08);
  border-radius: 0 12px 0 6px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  pointer-events: none;
  z-index: 2;
}

[data-theme='dark'] .q-source-badge,
[data-theme='dark'] .q-school-tag {
  background: rgba(148, 163, 184, 0.12);
}

/* ---- Header Row 1 ---- */
.q-card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 26px 20px 10px;
  border-bottom: 1px solid var(--divider);
  gap: 12px;
}

.q-card-tags {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
  min-width: 0;
}

/* ---- Ghost Tag ---- */
.q-ghost-tag {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 2px 4px;
  font-size: 12px;
  font-weight: 500;
  line-height: 1.5;
  color: var(--text-secondary);
}

.q-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  flex-shrink: 0;
}

.q-dot--blue { background: #3b82f6; }
.q-dot--emerald { background: #10b981; }
.q-dot--purple { background: #8b5cf6; }

/* ---- Body Row 2 ---- */
.paper-body {
  padding: 16px 20px 18px;
}

.paper-title {
  margin: 0 0 10px 0;
  font-size: 15.5px;
  font-weight: 650;
  color: var(--text-primary);
  line-height: 1.5;
  letter-spacing: -0.01em;
  transition: color 0.18s ease;
}

.paper-card:hover .paper-title {
  color: var(--accent);
}

.paper-meta-row {
  display: flex;
  align-items: center;
  gap: 16px;
  font-size: 12.5px;
  color: var(--text-secondary);
}

.paper-stat-item {
  display: inline-flex;
  align-items: center;
  gap: 5px;
}

/* ---- Footer Row 3 ---- */
.q-card-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 20px;
  background-color: #fafbfc;
  border-top: 1px solid #f0f0f2;
  border-radius: 0 0 12px 12px;
  gap: 12px;
}

[data-theme='dark'] .q-card-footer {
  background-color: rgba(0, 0, 0, 0.15);
  border-top-color: var(--divider);
}

.paper-tag-pill {
  font-size: 11.5px;
  color: var(--text-muted);
  background: var(--bg-hover);
  padding: 2px 8px;
  border-radius: 4px;
}

.q-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.q-action-btn {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 5px 12px;
  border-radius: var(--radius-sm);
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  color: var(--text-secondary);
  font-size: 12px;
  font-weight: 600;
  white-space: nowrap;
  transition: var(--transition-fast);
  cursor: pointer;
}

.q-action-btn:hover {
  background: var(--bg-hover);
  border-color: var(--border-strong);
  color: var(--text-primary);
}

.q-action-btn:active {
  transform: scale(0.96);
}

.q-action--ghost:hover {
  color: var(--accent);
  border-color: var(--accent);
  background: var(--accent-light);
}

.q-action--primary {
  background: var(--accent) !important;
  color: #ffffff !important;
  border-color: var(--accent) !important;
}

.q-action--primary:hover {
  background: var(--accent-hover) !important;
  border-color: var(--accent-hover) !important;
}
</style>
