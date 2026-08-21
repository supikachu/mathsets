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
  fill: []
  viewQuestions: []
}>()

const sourceLabel = computed(() =>
  displayPaperSource(props.paper.source_type, props.paper.sub_source_type) || '试卷',
)

const tone = computed(() => {
  const raw = `${props.paper.source_type || ''} ${props.paper.sub_source_type || ''} ${sourceLabel.value}`
  if (/开学|entrance/.test(raw)) return 'green'
  if (/高考|gaokao|真题/.test(raw)) return 'dark'
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

const tags = computed(() =>
  [props.paper.grade, props.paper.year ? String(props.paper.year) : '', region.value, sourceLabel.value]
    .filter(Boolean),
)
</script>

<template>
  <article class="pr-card">
    <div class="pr-thumb" :class="`tone-${tone}`">
      <span class="pr-thumb-label">{{ sourceLabel }}</span>
      <span class="pr-thumb-dots" aria-hidden="true" />
    </div>

    <div class="pr-body">
      <button type="button" class="pr-title" @click="emit('open')">{{ paper.title }}</button>
      <div class="pr-meta">
        <span v-for="t in tags" :key="t" class="pr-chip">{{ t }}</span>
        <span class="pr-stat">题量 <em>{{ paper.question_count ?? 0 }}</em></span>
        <span v-if="dateText" class="pr-date">{{ dateText }}</span>
      </div>
    </div>

    <div class="pr-actions">
      <button type="button" class="pr-link" @click="emit('open')">
        <AppIcon name="file-text" :size="14" />
        打开
      </button>
      <button type="button" class="pr-link" @click="emit('fill')">
        <AppIcon name="plus" :size="14" />
        补录
      </button>
      <button type="button" class="pr-link" @click="emit('viewQuestions')">
        <AppIcon name="search" :size="14" />
        看题目
      </button>
    </div>
  </article>
</template>

<style scoped>
.pr-card {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 16px 4px 18px;
  border-bottom: 1px dashed var(--border-color, #e5e7eb);
}

.pr-thumb {
  flex-shrink: 0;
  width: 56px;
  height: 56px;
  border: 1px solid var(--border-color, #e5e7eb);
  border-radius: 8px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: flex-start;
  padding-top: 8px;
  position: relative;
  overflow: hidden;
  background: #fafafa;
}

.pr-thumb-label {
  font-size: 11px;
  font-weight: 600;
  line-height: 1.2;
  text-align: center;
  padding: 0 4px;
}

.tone-green .pr-thumb-label { color: #16a34a; }
.tone-blue .pr-thumb-label { color: #2563eb; }
.tone-dark .pr-thumb-label { color: #1e3a5f; }

.pr-thumb-dots {
  position: absolute;
  inset: 28px 8px 8px;
  background-image: radial-gradient(circle, #d1d5db 1px, transparent 1.2px);
  background-size: 6px 6px;
  opacity: 0.7;
}

.pr-body {
  flex: 1;
  min-width: 0;
}

.pr-title {
  display: block;
  width: 100%;
  text-align: left;
  border: none;
  background: none;
  padding: 0;
  font-size: 16px;
  font-weight: 650;
  color: var(--text-primary, #111827);
  cursor: pointer;
  line-height: 1.4;
}

.pr-title:hover {
  color: #2563eb;
}

.pr-meta {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  margin-top: 8px;
  font-size: 12px;
  color: var(--text-muted, #6b7280);
}

.pr-chip {
  padding: 2px 8px;
  border-radius: 4px;
  background: #f3f4f6;
  color: #6b7280;
}

.pr-stat em {
  font-style: normal;
  font-weight: 600;
  color: #dc2626;
  margin-left: 2px;
}

.pr-date {
  color: #9ca3af;
}

.pr-actions {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 12px;
}

.pr-link {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  border: none;
  background: none;
  padding: 4px 2px;
  font-size: 13px;
  color: #2563eb;
  cursor: pointer;
}

.pr-link:hover {
  text-decoration: underline;
}

@media (max-width: 720px) {
  .pr-card {
    flex-wrap: wrap;
  }
  .pr-actions {
    width: 100%;
    justify-content: flex-end;
    padding-left: 72px;
  }
}
</style>
