<script setup lang="ts">
/**
 * TypesetPreview — 逐页 SVG 预览 + 印前预检面板（T5.5，结构见实施计划 §7.3）
 *
 * 只认一个入参：`request`（与 `/export/pdf` 完全同一份请求体）。预览与导出用的是同一次编译
 * 的产物，所以这里看到的就是点「导出」会拿到的东西（R12）。
 *
 * 三处刻意的设计：
 * - **只挂载当前页**。一页 SVG ~200KB，百页卷 ~4MB（`handlers/typeset.rs` 尾部那笔账），
 *   整卷进 DOM 会把面板做成第二个标签页。
 * - **换参数不清屏**。新结果到达前沿用上一份（`result` 只在成功分支赋值），配合 300ms
 *   debounce：微调边距来回改七次也只编三次，而屏幕不会闪成白片。
 * - **请求有序号守卫**。debounce 只挡重复发，挡不住回来的顺序 —— 百页卷冷编 2.5s，期间
 *   改了参数的旧响应晚到会把手上的版面洗掉，所以 `seq` 不匹配的响应一律丢弃。
 */
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import {
  typesetApi,
  type ExamRequest,
  type Issue,
  type IssueSeverity,
  type PreviewResponse,
} from '@/api/client'
import { AppIcon, AppSelect } from '@/components/ui'
import { intrinsicWidthPx, sanitizeSvg } from '@/utils/svgSanitize'

const props = defineProps<{ request: ExamRequest | null }>()

/** 参数改完攒 300ms 再发（任务分解 T5.5） */
const DEBOUNCE_MS = 300

const ZOOM_OPTIONS = [
  { value: '75', label: '75%' },
  { value: '100', label: '100%' },
  { value: '125', label: '125%' },
  { value: '150', label: '150%' },
  { value: 'fit', label: '适应宽度' },
]

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

const result = ref<PreviewResponse | null>(null)
const loading = ref(false)
const errorText = ref('')
const page = ref(1)
const zoom = ref('fit')

let seq = 0
let timer: ReturnType<typeof setTimeout> | null = null

const pageCount = computed(() => result.value?.page_count ?? 0)
const issues = computed<Issue[]>(() => result.value?.issues ?? [])
const engineWarnings = computed(() => result.value?.warnings ?? [])

/// 分级展示：错误在前，同级按页码排（教师从上往下读就是从上卷往下卷）
const sortedIssues = computed(() =>
  [...issues.value].sort(
    (a, b) =>
      SEVERITY_RANK[a.severity] - SEVERITY_RANK[b.severity] || (a.page ?? 0) - (b.page ?? 0),
  ),
)
const severityCounts = computed(() =>
  (['error', 'warning', 'info'] as IssueSeverity[])
    .map((sev) => ({ sev, n: issues.value.filter((i) => i.severity === sev).length }))
    .filter((c) => c.n > 0),
)

/// 当前页只有一张：过完 sanitize 才进 DOM（B7），读不懂的整页不显示
const currentSvg = computed(() => {
  const src = result.value?.pages[page.value - 1]
  return src ? sanitizeSvg(src) : null
})
const rejected = computed(() => !!result.value && !currentSvg.value && pageCount.value >= page.value)

/// 缩放按「实际尺寸 = 100%」标：SVG 根元素自己声明的宽度乘倍率；适应宽度交给 CSS 铺满
const sheetStyle = computed(() => {
  if (zoom.value === 'fit') return { width: '100%' }
  const natural = currentSvg.value ? intrinsicWidthPx(currentSvg.value) : null
  if (!natural) return { width: '100%' }
  return { width: `${Math.round((natural * Number(zoom.value)) / 100)}px` }
})

const statusText = computed(() => {
  if (loading.value) return '正在排版…'
  if (errorText.value) return errorText.value
  if (!pageCount.value) return '没有可预览的内容'
  return `${pageCount.value} 页 · 预检 ${issues.value.length} 条`
})

function go(next: number) {
  page.value = Math.min(Math.max(1, next), Math.max(1, pageCount.value))
}

function jumpTo(issue: Issue) {
  if (issue.page) go(issue.page)
}

function onZoom(value?: string) {
  if (value) zoom.value = value
}

function onJump(event: Event) {
  const raw = (event.target as HTMLInputElement).value
  const next = Number.parseInt(raw, 10)
  go(Number.isFinite(next) ? next : 1)
}

function errorMessage(e: unknown): string {
  const err = e as {
    response?: { data?: { error?: string; message?: string } }
    message?: string
  }
  return (
    err?.response?.data?.error || err?.response?.data?.message || err?.message || '预览失败，请稍后重试'
  )
}

async function run() {
  const req = props.request
  if (!req) {
    result.value = null
    errorText.value = ''
    loading.value = false
    return
  }
  const mine = ++seq
  loading.value = true
  try {
    const { data } = await typesetApi.preview(req)
    if (mine !== seq) return
    result.value = data
    errorText.value = ''
    go(page.value)
  } catch (e) {
    if (mine !== seq) return
    errorText.value = errorMessage(e)
  } finally {
    if (mine === seq) loading.value = false
  }
}

function schedule() {
  if (timer) clearTimeout(timer)
  timer = setTimeout(run, DEBOUNCE_MS)
}

watch(() => props.request, schedule, { deep: true, immediate: true })

onBeforeUnmount(() => {
  if (timer) clearTimeout(timer)
  seq += 1 // 在途的响应回来也没人接了，让它自己作废
})
</script>

<template>
  <div class="tp">
    <div class="tp-bar">
      <div class="tp-nav">
        <button type="button" class="tp-btn" :disabled="page <= 1" @click="go(page - 1)">
          <AppIcon name="chevron-left" :size="15" />
        </button>
        <span class="tp-readout">
          第
          <input
            class="tp-jump"
            type="number"
            min="1"
            :max="pageCount || 1"
            :value="page"
            @change="onJump"
          />
          / <strong>{{ pageCount || '—' }}</strong> 页
        </span>
        <button
          type="button"
          class="tp-btn"
          :disabled="page >= pageCount"
          @click="go(page + 1)"
        >
          <AppIcon name="chevron-right" :size="15" />
        </button>
      </div>

      <div class="tp-zoom">
        <span class="tp-caption">缩放</span>
        <AppSelect :model-value="zoom" :options="ZOOM_OPTIONS" @update:model-value="onZoom" />
      </div>

      <div class="tp-status" :class="{ 'is-busy': loading, 'is-bad': !!errorText && !loading }">
        <AppIcon :name="errorText && !loading ? 'alert' : loading ? 'clock' : 'check-circle'" :size="13" />
        <span>{{ statusText }}</span>
        <button v-if="errorText && !loading" type="button" class="tp-retry" @click="run">重试</button>
      </div>
    </div>

    <div class="tp-stage">
      <div v-if="currentSvg" class="tp-sheet" :style="sheetStyle">
        <!-- 已过 sanitizeSvg：脚本载体、事件属性、javascript: 链接都剥掉了（B7） -->
        <div class="tp-svg" v-html="currentSvg" />
      </div>
      <p v-else-if="rejected" class="tp-empty tp-empty--bad">
        第 {{ page }} 页的 SVG 未通过安全检查，已不予显示
      </p>
      <p v-else-if="loading" class="tp-empty">正在排版…</p>
      <p v-else class="tp-empty">{{ errorText || '没有可预览的内容' }}</p>
    </div>

    <section class="tp-checks">
      <header class="tp-checks-head">
        <AppIcon name="shield-check" :size="14" />
        <span class="tp-checks-title">印前预检</span>
        <span v-for="c in severityCounts" :key="c.sev" class="tp-count" :data-sev="c.sev">
          {{ SEVERITY_LABEL[c.sev] }} {{ c.n }}
        </span>
        <span class="tp-hint">点一条就跳到它所在的那一页</span>
      </header>

      <ul v-if="sortedIssues.length" class="tp-list">
        <li v-for="(it, i) in sortedIssues" :key="i">
          <button
            type="button"
            class="tp-item"
            :class="{ 'is-active': it.page === page }"
            :disabled="!it.page"
            :title="it.page ? `跳到第 ${it.page} 页` : '这条发生在排版之前，没有页可跳'"
            @click="jumpTo(it)"
          >
            <span class="tp-dot" :data-sev="it.severity"></span>
            <span class="tp-tag">{{ FIELD_LABEL[it.field] }}</span>
            <span v-if="it.question_no" class="tp-tag">第 {{ it.question_no }} 题</span>
            <span v-if="it.page" class="tp-tag tp-tag--page">第 {{ it.page }} 页</span>
            <span v-else class="tp-tag tp-tag--muted">排版之前，无页可定位</span>
            <span class="tp-reason">{{ it.reason }}</span>
          </button>
        </li>
      </ul>
      <p v-else class="tp-clean">没报出问题：图够清、内容没画出纸外、中文由思源系字体绘制。</p>

      <details v-if="engineWarnings.length" class="tp-engine">
        <summary>排版引擎告警 {{ engineWarnings.length }} 条（原文，给人看）</summary>
        <ul>
          <li v-for="(w, i) in engineWarnings" :key="i">{{ w }}</li>
        </ul>
      </details>
    </section>
  </div>
</template>

<style scoped>
.tp {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.tp-bar {
  display: flex;
  align-items: center;
  gap: 16px;
  flex-wrap: wrap;
}

.tp-nav {
  display: flex;
  align-items: center;
  gap: 6px;
}

.tp-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  padding: 0;
  color: var(--text-primary);
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition: background 0.15s ease, opacity 0.15s ease;
}

.tp-btn:hover:not(:disabled) {
  background: var(--bg-hover);
}

.tp-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.tp-readout {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 13px;
  color: var(--text-secondary);
}

.tp-jump {
  width: 48px;
  padding: 3px 6px;
  font: inherit;
  font-variant-numeric: tabular-nums;
  text-align: center;
  color: var(--text-primary);
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  outline: none;
}

.tp-zoom {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 170px;
}

.tp-caption {
  font-size: 12px;
  color: var(--text-muted);
  white-space: nowrap;
}

.tp-status {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-left: auto;
  font-size: 12px;
  color: var(--text-secondary);
}

.tp-status.is-busy {
  color: var(--accent);
}

.tp-status.is-bad {
  color: var(--danger);
}

.tp-retry {
  padding: 2px 8px;
  font: inherit;
  font-size: 12px;
  color: var(--text-secondary);
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  cursor: pointer;
}

.tp-stage {
  max-height: 56vh;
  padding: 16px;
  overflow: auto;
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
}

.tp-sheet {
  margin: 0 auto;
  background: #fff;
  box-shadow: var(--shadow-md);
  border-radius: 2px;
}

.tp-svg :deep(svg) {
  display: block;
  width: 100%;
  height: auto;
}

.tp-empty {
  margin: 0;
  padding: 40px 0;
  font-size: 13px;
  text-align: center;
  color: var(--text-muted);
}

.tp-empty--bad {
  color: var(--danger);
}

.tp-checks {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px 12px;
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
}

.tp-checks-head {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--text-secondary);
}

.tp-checks-title {
  font-weight: 600;
  color: var(--text-primary);
}

.tp-count {
  padding: 1px 6px;
  font-variant-numeric: tabular-nums;
  border-radius: 999px;
  border: 1px solid var(--border-color);
}

.tp-count[data-sev='error'] {
  color: var(--danger);
}

.tp-count[data-sev='warning'] {
  color: var(--warning);
}

.tp-hint {
  margin-left: auto;
  font-size: 11px;
  color: var(--text-muted);
}

.tp-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  max-height: 190px;
  margin: 0;
  padding: 0;
  overflow-y: auto;
  list-style: none;
}

.tp-item {
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

.tp-item:not(:disabled):hover {
  background: var(--bg-hover);
}

.tp-item:disabled {
  cursor: default;
}

.tp-item.is-active {
  background: var(--accent-light);
}

.tp-dot {
  width: 7px;
  height: 7px;
  flex-shrink: 0;
  background: var(--text-muted);
  border-radius: 50%;
}

.tp-dot[data-sev='error'] {
  background: var(--danger);
}

.tp-dot[data-sev='warning'] {
  background: var(--warning);
}

.tp-tag {
  flex-shrink: 0;
  padding: 0 5px;
  font-size: 11px;
  color: var(--text-secondary);
  border: 1px solid var(--border-color);
  border-radius: 3px;
}

.tp-tag--page {
  color: var(--accent);
  border-color: var(--accent);
}

.tp-tag--muted {
  color: var(--text-muted);
}

.tp-reason {
  color: var(--text-secondary);
}

.tp-clean {
  margin: 0;
  font-size: 12px;
  color: var(--text-secondary);
}

.tp-engine {
  font-size: 11px;
  color: var(--text-muted);
}

.tp-engine ul {
  margin: 4px 0 0;
  padding-left: 16px;
}
</style>
