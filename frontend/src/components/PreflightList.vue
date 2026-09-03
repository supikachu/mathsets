<script setup lang="ts">
/**
 * PreflightList — 预检 / 降级清单的唯一画法（T5.6）
 *
 * 两个数据源都归一到 `Issue` 再进来：`/typeset/preview` 的 `issues`（全量，带 severity
 * 与页码）与 `X-Export-Warnings` 回执（后端只给四字段，无级别无页码）。排序、点击定位、
 * 当前页高亮、无页可定位的坦白写法，只在这里有一份（R15）。
 */
import { computed } from 'vue'
import type { Issue, IssueSeverity } from '@/api/client'
import { AppIcon } from '@/components/ui'

const props = withDefaults(
  defineProps<{
    items: Issue[]
    /** 预览当前页：命中的行描色，让「跳过去了」这件事看得见 */
    activePage?: number | null
    title?: string
    /** 清单口径说明（预览全量 / 回执可能截断） */
    note?: string
  }>(),
  { activePage: null, title: '印前预检', note: '' },
)

const emit = defineEmits<{ locate: [page: number] }>()

const FIELD_LABEL: Record<Issue['field'], string> = {
  stem: '题干',
  analysis: '解析',
  choice: '选项',
  answer: '答案',
  structure: '结构',
  image: '图片',
  other: '版面',
}
const SEVERITY_LABEL: Record<IssueSeverity, string> = {
  error: '错误',
  warning: '警告',
  info: '提示',
}
const SEVERITY_RANK: Record<IssueSeverity, number> = { error: 0, warning: 1, info: 2 }

/// 分级展示：错误在前，同级按页码排（教师从上往下读就是从上卷往下卷）
const sorted = computed(() =>
  [...props.items].sort(
    (a, b) =>
      SEVERITY_RANK[a.severity] - SEVERITY_RANK[b.severity] || (a.page ?? 0) - (b.page ?? 0),
  ),
)
const counts = computed(() =>
  (['error', 'warning', 'info'] as IssueSeverity[])
    .map((sev) => ({ sev, n: props.items.filter((i) => i.severity === sev).length }))
    .filter((c) => c.n > 0),
)
</script>

<template>
  <section class="pl">
    <header class="pl-head">
      <AppIcon name="shield-check" :size="14" />
      <span class="pl-title">{{ title }}</span>
      <span v-for="c in counts" :key="c.sev" class="pl-count" :data-sev="c.sev">
        {{ SEVERITY_LABEL[c.sev] }} {{ c.n }}
      </span>
      <span class="pl-hint">
        <template v-if="note">{{ note }}</template>
        <template v-else>点一条就跳到它所在的那一页</template>
      </span>
    </header>

    <ul v-if="sorted.length" class="pl-list">
      <li v-for="(it, i) in sorted" :key="i">
        <button
          type="button"
          class="pl-item"
          :class="{ 'is-active': it.page && it.page === activePage }"
          :disabled="!it.page"
          :title="it.page ? `跳到第 ${it.page} 页` : '这条发生在排版之前，没有页可跳'"
          @click="it.page && emit('locate', it.page)"
        >
          <span class="pl-dot" :data-sev="it.severity"></span>
          <span class="pl-tag">{{ FIELD_LABEL[it.field] }}</span>
          <span v-if="it.question_no" class="pl-tag">第 {{ it.question_no }} 题</span>
          <span v-if="it.page" class="pl-tag pl-tag--page">第 {{ it.page }} 页</span>
          <span v-else class="pl-tag pl-tag--muted">无页可定位</span>
          <span class="pl-reason">{{ it.reason }}</span>
        </button>
      </li>
    </ul>
    <p v-else class="pl-clean">没报出问题：图够清、内容没画出纸外、中文由思源系字体绘制。</p>
  </section>
</template>

<style scoped>
.pl {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px 12px;
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
}

.pl-head {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--text-secondary);
}

.pl-title {
  font-weight: 600;
  color: var(--text-primary);
}

.pl-count {
  padding: 1px 6px;
  font-variant-numeric: tabular-nums;
  border-radius: 999px;
  border: 1px solid var(--border-color);
}

.pl-count[data-sev='error'] {
  color: var(--danger);
}

.pl-count[data-sev='warning'] {
  color: var(--warning);
}

.pl-hint {
  margin-left: auto;
  font-size: 11px;
  color: var(--text-muted);
}

.pl-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  max-height: 190px;
  margin: 0;
  padding: 0;
  overflow-y: auto;
  list-style: none;
}

.pl-item {
  display: flex;
  align-items: baseline;
  gap: 7px;
  width: 100%;
  padding: 5px 6px;
  font: inherit;
  font-size: 12px;
  line-height: 1.5;
  text-align: left;
  color: var(--text-primary);
  background: none;
  border: none;
  border-radius: var(--radius-sm);
  cursor: pointer;
}

.pl-item:not(:disabled):hover {
  background: var(--bg-hover);
}

.pl-item:disabled {
  cursor: default;
}

.pl-item.is-active {
  background: var(--accent-light);
}

.pl-dot {
  width: 7px;
  height: 7px;
  flex-shrink: 0;
  background: var(--text-muted);
  border-radius: 50%;
}

.pl-dot[data-sev='error'] {
  background: var(--danger);
}

.pl-dot[data-sev='warning'] {
  background: var(--warning);
}

.pl-tag {
  flex-shrink: 0;
  padding: 0 5px;
  font-size: 11px;
  color: var(--text-secondary);
  border: 1px solid var(--border-color);
  border-radius: 3px;
}

.pl-tag--page {
  color: var(--accent);
  border-color: var(--accent);
}

.pl-tag--muted {
  color: var(--text-muted);
}

.pl-reason {
  color: var(--text-secondary);
}

.pl-clean {
  margin: 0;
  font-size: 12px;
  color: var(--text-secondary);
}
</style>
