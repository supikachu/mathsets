<template>
  <div class="dashboard">
    <!-- ===== 欢迎语 ===== -->
    <div class="dash-welcome">
      <h1 class="dash-hello">Hello! {{ auth.displayName || '用户' }}</h1>
      <p class="dash-subtitle">这是你使用协同题库的第 <span class="dash-days">{{ daysSince }}</span> 天</p>
    </div>

    <!-- ===== Bento Grid ===== -->
    <div class="bento-grid">
      <!-- 题目总数 + 趋势折线图 -->
      <div class="bento-card bento-trend">
        <div class="bento-head">
          <span class="bento-title">题目总数</span>
          <span class="bento-number">{{ stats.total }}</span>
        </div>
        <div class="trend-chart">
          <svg
            v-if="trendData.length > 1"
            :viewBox="`0 0 ${chartW} ${chartH}`"
            preserveAspectRatio="none"
            class="trend-svg"
          >
            <defs>
              <linearGradient :id="`grad-${uid}`" x1="0" y1="0" x2="0" y2="1">
                <stop offset="0%" stop-color="#ff9f0a" stop-opacity="0.28" />
                <stop offset="100%" stop-color="#ff9f0a" stop-opacity="0" />
              </linearGradient>
            </defs>
            <path :d="areaPath" :fill="`url(#grad-${uid})`" class="trend-area" />
            <path :d="linePath" fill="none" stroke="#ff9f0a" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" class="trend-line" />
            <circle
              v-for="(pt, i) in chartPoints"
              :key="i"
              :cx="pt.x"
              :cy="pt.y"
              r="3"
              fill="#ff9f0a"
              class="trend-dot"
            />
          </svg>
          <div class="trend-empty" v-else>暂无趋势数据</div>
        </div>
      </div>

      <!-- 活跃天数热力图 -->
      <div class="bento-card bento-heatmap">
        <div class="bento-head">
          <span class="bento-title">活跃天数</span>
          <span class="bento-badge">{{ heatmapStats.activeDays }} 天</span>
        </div>
        <div class="heatmap-wrap">
          <div class="heatmap-months">
            <span
              v-for="m in heatmapMonths"
              :key="m.label"
              class="heatmap-month-label"
              :style="{ marginLeft: m.offset > 0 ? m.offset * 14 + 'px' : '0' }"
            >{{ m.label }}</span>
          </div>
          <div class="heatmap-grid">
            <div class="heatmap-days-col" v-for="(col, ci) in heatmapGrid" :key="ci">
              <div
                v-for="(cell, ri) in col"
                :key="ri"
                class="heatmap-cell"
                :class="`heat-${cell.level}`"
                :title="cell.tooltip"
              />
            </div>
          </div>
          <div class="heatmap-legend">
            <span class="heatmap-legend-text">Less</span>
            <span class="heatmap-cell heat-0" />
            <span class="heatmap-cell heat-1" />
            <span class="heatmap-cell heat-2" />
            <span class="heatmap-cell heat-3" />
            <span class="heatmap-cell heat-4" />
            <span class="heatmap-legend-text">More</span>
          </div>
        </div>
      </div>

      <!-- 知识点分类 -->
      <div class="bento-card bento-kp">
        <div class="bento-head">
          <span class="bento-title">知识点分类</span>
        </div>
        <div class="kp-grid">
          <div v-for="cat in kpCategories" :key="cat.name" class="kp-cell">
            <span class="kp-icon" :style="{ '--icon-color': cat.color }">
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" v-html="cat.iconPath" />
            </span>
            <div class="kp-info">
              <span class="kp-name">{{ cat.name }}</span>
              <span class="kp-count">{{ cat.count }}</span>
            </div>
          </div>
        </div>
      </div>

      <!-- 题目标签分布 -->
      <div class="bento-card bento-tags">
        <div class="bento-head">
          <span class="bento-title">标签分布</span>
        </div>
        <div class="tag-bars">
          <div v-for="tag in tagDistribution" :key="tag.name" class="tag-bar-row">
            <span class="tag-bar-label">{{ tag.name }}</span>
            <div class="tag-bar-track">
              <div class="tag-bar-fill" :style="{ width: tag.percent + '%', background: `linear-gradient(90deg, ${tag.color}, ${tag.color}dd)` }" />
            </div>
            <span class="tag-bar-count">{{ tag.count }}</span>
          </div>
        </div>
      </div>

      <!-- 题型分布 -->
      <div class="bento-card bento-type">
        <div class="bento-head">
          <span class="bento-title">题型分布</span>
        </div>
        <div class="type-donut">
          <svg viewBox="0 0 120 120" class="donut-svg">
            <circle cx="60" cy="60" r="52" fill="none" stroke="var(--bg-input)" stroke-width="8" />
            <circle
              v-for="(seg, i) in donutSegments"
              :key="i"
              cx="60" cy="60" r="52"
              fill="none"
              :stroke="seg.color"
              stroke-width="8"
              :stroke-dasharray="`${seg.arc} ${seg.rest}`"
              :stroke-dashoffset="seg.offset"
              stroke-linecap="round"
              class="donut-seg"
              transform="rotate(-90 60 60)"
            />
          </svg>
          <div class="donut-center">
            <span class="donut-total">{{ stats.total }}</span>
            <span class="donut-label">题目</span>
          </div>
        </div>
        <div class="donut-legend">
          <div v-for="(seg, i) in donutSegments" :key="i" class="donut-legend-item">
            <span class="donut-legend-dot" :style="{ background: seg.color }" />
            <span class="donut-legend-text">{{ seg.label }}</span>
            <span class="donut-legend-count">{{ seg.count }}</span>
          </div>
        </div>
      </div>

      <!-- 难度分布 -->
      <div class="bento-card bento-diff">
        <div class="bento-head">
          <span class="bento-title">难度分布</span>
        </div>
        <div class="diff-list">
          <div v-for="diff in difficultyDist" :key="diff.level" class="diff-row">
            <div class="diff-stars">
              <span
                v-for="s in 5"
                :key="s"
                class="diff-star"
                :class="{ filled: s <= diff.level }"
              />
            </div>
            <div class="diff-track">
              <div class="diff-track-fill" :style="{ width: diff.percent + '%', background: diff.trackColor }" />
            </div>
            <span class="diff-badge" :style="{ background: diff.color, color: diff.textColor }">
              {{ diff.count }}
            </span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useSpaceStore } from '@/stores/space'
import { questionApi, type QuestionSummary, type QuestionQuery } from '@/api/client'
import client from '@/api/client'

const auth = useAuthStore()
const space = useSpaceStore()

const uid = Math.random().toString(36).slice(2, 8)

const stats = ref({
  total: 0,
  draft: 0,
  pending: 0,
  rejected: 0,
  published: 0,
  disabled: 0,
})

const daysSince = computed(() => 1)

// ===== 趋势折线图（平滑曲线） =====
const trendData = ref<{ label: string; value: number }[]>([])

const chartW = 300
const chartH = 80
const chartPadX = 12
const chartPadY = 10

const chartPoints = computed(() => {
  if (trendData.value.length === 0) return []
  const max = Math.max(...trendData.value.map((d) => d.value), 1)
  const stepX = (chartW - chartPadX * 2) / Math.max(trendData.value.length - 1, 1)
  return trendData.value.map((d, i) => ({
    x: chartPadX + i * stepX,
    y: chartPadY + (1 - d.value / max) * (chartH - chartPadY * 2),
  }))
})

// Catmull-Rom 平滑曲线
function smoothPath(pts: { x: number; y: number }[]): string {
  if (pts.length < 2) return ''
  if (pts.length === 2) return `M ${pts[0].x} ${pts[0].y} L ${pts[1].x} ${pts[1].y}`
  let path = `M ${pts[0].x} ${pts[0].y}`
  for (let i = 0; i < pts.length - 1; i++) {
    const p0 = pts[i - 1] || pts[i]
    const p1 = pts[i]
    const p2 = pts[i + 1]
    const p3 = pts[i + 2] || p2
    const cp1x = p1.x + (p2.x - p0.x) / 6
    const cp1y = p1.y + (p2.y - p0.y) / 6
    const cp2x = p2.x - (p3.x - p1.x) / 6
    const cp2y = p2.y - (p3.y - p1.y) / 6
    path += ` C ${cp1x} ${cp1y}, ${cp2x} ${cp2y}, ${p2.x} ${p2.y}`
  }
  return path
}

const linePath = computed(() => smoothPath(chartPoints.value))

const areaPath = computed(() => {
  const pts = chartPoints.value
  if (pts.length < 2) return ''
  const line = smoothPath(pts)
  return `${line} L ${pts[pts.length - 1].x} ${chartH} L ${pts[0].x} ${chartH} Z`
})

// ===== 热力图 =====
interface HeatCell { level: number; tooltip: string }

const heatmapGrid = ref<HeatCell[][]>([])
const heatmapMonths = ref<{ label: string; offset: number }[]>([])
const heatmapStats = ref({ activeDays: 0 })

function generateHeatmap() {
  const cols: HeatCell[][] = []
  let activeCount = 0
  const today = new Date()
  const startDate = new Date(today)
  startDate.setDate(startDate.getDate() - 139)
  const startDay = startDate.getDay()
  startDate.setDate(startDate.getDate() - startDay)

  const monthLabels: { label: string; offset: number }[] = []
  let lastMonth = -1
  let colIndex = 0

  for (let c = 0; c < 21; c++) {
    const col: HeatCell[] = []
    for (let r = 0; r < 7; r++) {
      const date = new Date(startDate)
      date.setDate(date.getDate() + c * 7 + r)
      if (date > today) { col.push({ level: 0, tooltip: '' }); continue }
      const rand = Math.random()
      let level = 0
      if (rand > 0.7) level = 1
      if (rand > 0.85) level = 2
      if (rand > 0.93) level = 3
      if (rand > 0.97) level = 4
      if (level > 0) activeCount++
      col.push({ level, tooltip: `${date.getMonth() + 1}月${date.getDate()}日: ${level === 0 ? '无活动' : level + ' 次活动'}` })
      const month = date.getMonth()
      if (month !== lastMonth && r === 0) { monthLabels.push({ label: `${month + 1}月`, offset: colIndex }); lastMonth = month }
    }
    cols.push(col)
    colIndex++
  }
  heatmapGrid.value = cols
  heatmapMonths.value = monthLabels
  heatmapStats.value = { activeDays: activeCount }
}

// ===== 知识点分类 =====
const kpCategories = ref([
  { name: '集合', count: 0, color: '#0071e3', iconPath: '<circle cx="8" cy="8" r="5" /><circle cx="16" cy="16" r="5" />' },
  { name: '函数', count: 0, color: '#af52de', iconPath: '<path d="M4 20 C 4 16, 8 12, 12 12 S 20 8, 20 4" /><circle cx="4" cy="20" r="1.5" fill="currentColor" stroke="none" /><circle cx="20" cy="4" r="1.5" fill="currentColor" stroke="none" />' },
  { name: '几何', count: 0, color: '#ff9f0a', iconPath: '<path d="M12 3 L21 19 L3 19 Z" />' },
  { name: '数列', count: 0, color: '#34c759', iconPath: '<rect x="3" y="14" width="4" height="7" rx="0.5" /><rect x="10" y="10" width="4" height="11" rx="0.5" /><rect x="17" y="6" width="4" height="15" rx="0.5" />' },
  { name: '概率', count: 0, color: '#ff3b30', iconPath: '<circle cx="12" cy="12" r="8" /><path d="M12 4 L12 12 L18 12" />' },
  { name: '向量', count: 0, color: '#5ac8fa', iconPath: '<path d="M5 19 L19 5" /><path d="M14 5 L19 5 L19 10" />' },
])

// ===== 题目标签分布 =====
const tagDistribution = ref([
  { name: '真题', count: 0, percent: 0, color: '#0071e3' },
  { name: '创新题', count: 0, percent: 0, color: '#af52de' },
  { name: '易错题', count: 0, percent: 0, color: '#ff9f0a' },
])

// ===== 题型分布 =====
const typeDistribution = ref([
  { label: '选择题', count: 0, color: '#0071e3' },
  { label: '填空题', count: 0, color: '#34c759' },
  { label: '解答题', count: 0, color: '#ff9f0a' },
])

const donutSegments = computed(() => {
  const total = typeDistribution.value.reduce((s, t) => s + t.count, 0)
  if (total === 0) return []
  const circumference = 2 * Math.PI * 52
  let offset = 0
  return typeDistribution.value.map((t) => {
    const ratio = t.count / total
    const arc = ratio * circumference
    const seg = { label: t.label, count: t.count, color: t.color, arc, rest: circumference - arc, offset: -offset }
    offset += arc
    return seg
  })
})

// ===== 难度分布 =====
const difficultyDist = ref([
  { level: 1, count: 0, percent: 0, color: 'rgba(52, 199, 89, 0.12)', textColor: '#34c759', trackColor: '#34c759' },
  { level: 2, count: 0, percent: 0, color: 'rgba(90, 200, 250, 0.12)', textColor: '#5ac8fa', trackColor: '#5ac8fa' },
  { level: 3, count: 0, percent: 0, color: 'rgba(255, 159, 10, 0.12)', textColor: '#ff9f0a', trackColor: '#ff9f0a' },
  { level: 4, count: 0, percent: 0, color: 'rgba(255, 159, 10, 0.18)', textColor: '#ff9f0a', trackColor: '#ff7800' },
  { level: 5, count: 0, percent: 0, color: 'rgba(255, 59, 48, 0.12)', textColor: '#ff3b30', trackColor: '#ff3b30' },
])

onMounted(() => {
  generateHeatmap()
  fetchDashboardData()
})

watch(() => space.currentSpaceId, () => {
  fetchDashboardData()
})

async function fetchDashboardData() {
  const spaceId = space.currentSpaceId || undefined

  try {
    const res = await questionApi.stats({ space_id: spaceId })
    stats.value = {
      total: res.data.total,
      draft: res.data.draft,
      pending: res.data.pending,
      rejected: res.data.rejected || 0,
      published: res.data.published,
      disabled: res.data.disabled || 0,
    }
  } catch { /* silent */ }

  try {
    const query: QuestionQuery = { space_id: spaceId, page: 1, page_size: 200 }
    const res = await questionApi.list(query)
    const questions = res.data || []

    // 题型
    typeDistribution.value[0].count = questions.filter(q => q.question_type === 'choice').length
    typeDistribution.value[1].count = questions.filter(q => q.question_type === 'fill').length
    typeDistribution.value[2].count = questions.filter(q => q.question_type === 'solution').length

    // 难度
    const diffMap: Record<string, number> = { easy: 1, medium: 3, hard: 5 }
    for (const diff of difficultyDist.value) { diff.count = 0; diff.percent = 0 }
    for (const q of questions) {
      const level = diffMap[q.difficulty] || 3
      const idx = difficultyDist.value.findIndex(d => d.level === level)
      if (idx >= 0) difficultyDist.value[idx].count++
    }
    const maxDiff = Math.max(...difficultyDist.value.map(d => d.count), 1)
    for (const diff of difficultyDist.value) {
      diff.percent = Math.round(diff.count / maxDiff * 100)
    }

    // 标签
    const tagCounts = { 真题: 0, 创新题: 0, 易错题: 0 }
    for (const q of questions) {
      const tags = (q as any).tags as string[] | undefined
      if (tags) { for (const t of tags) { if (t in tagCounts) tagCounts[t as keyof typeof tagCounts]++ } }
    }
    const maxTag = Math.max(...Object.values(tagCounts), 1)
    tagDistribution.value[0].count = tagCounts.真题
    tagDistribution.value[0].percent = Math.round(tagCounts.真题 / maxTag * 100)
    tagDistribution.value[1].count = tagCounts.创新题
    tagDistribution.value[1].percent = Math.round(tagCounts.创新题 / maxTag * 100)
    tagDistribution.value[2].count = tagCounts.易错题
    tagDistribution.value[2].percent = Math.round(tagCounts.易错题 / maxTag * 100)

    // 知识点
    for (const cat of kpCategories.value) { cat.count = 0 }
    for (const q of questions) {
      const kpNames = (q as any).knowledge_point_names as string[] | undefined
      if (kpNames) { for (const cat of kpCategories.value) { if (kpNames.some(n => n.includes(cat.name))) cat.count++ } }
    }

    // 趋势
    const today = new Date()
    const dayLabels: { label: string; value: number }[] = []
    for (let i = 6; i >= 0; i--) {
      const d = new Date(today)
      d.setDate(d.getDate() - i)
      const count = questions.filter(q => {
        if (!q.created_at) return false
        const qd = new Date(q.created_at)
        return qd.getFullYear() === d.getFullYear() && qd.getMonth() === d.getMonth() && qd.getDate() === d.getDate()
      }).length
      dayLabels.push({ label: `${d.getMonth() + 1}/${d.getDate()}`, value: count })
    }
    trendData.value = dayLabels
  } catch { /* silent */ }
}
</script>

<style scoped>
.dashboard {
  display: flex;
  flex-direction: column;
  gap: 20px;
  padding: 24px 28px;
  height: 100%;
  overflow-y: auto;
  background: #f4f7fc;
}

[data-theme='dark'] .dashboard {
  background: #000000;
}

/* ===== Welcome ===== */
.dash-welcome {
  padding: 4px 0 4px;
}

.dash-hello {
  font-size: 32px;
  font-weight: 800;
  letter-spacing: -0.03em;
  color: var(--text-primary);
  margin-bottom: 2px;
}

.dash-subtitle {
  font-size: 13px;
  color: var(--text-muted);
  font-weight: 400;
}

.dash-days {
  font-weight: 600;
  color: var(--accent);
}

/* ===== Bento Grid ===== */
.bento-grid {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  grid-auto-rows: minmax(min-content, auto);
  gap: 20px;
}

.bento-trend { grid-column: span 1; }
.bento-heatmap { grid-column: span 3; }
.bento-kp { grid-column: span 2; }
.bento-tags { grid-column: span 1; }
.bento-type { grid-column: span 1; }
.bento-diff { grid-column: span 2; }

@media (max-width: 1200px) {
  .bento-grid { grid-template-columns: repeat(2, 1fr); }
  .bento-trend { grid-column: span 1; }
  .bento-heatmap { grid-column: span 1; }
  .bento-kp { grid-column: span 2; }
  .bento-tags { grid-column: span 1; }
  .bento-type { grid-column: span 1; }
  .bento-diff { grid-column: span 2; }
}

@media (max-width: 768px) {
  .bento-grid { grid-template-columns: 1fr; }
  .bento-trend, .bento-heatmap, .bento-kp, .bento-tags, .bento-type, .bento-diff {
    grid-column: span 1;
  }
}

/* ===== Card base ===== */
.bento-card {
  background: var(--bg-card);
  border-radius: 12px;
  border: 1px solid var(--border-color);
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.02), 0 2px 8px rgba(0, 0, 0, 0.02);
  padding: 18px 20px;
  transition: transform 0.3s cubic-bezier(0.4, 0, 0.2, 1), box-shadow 0.3s cubic-bezier(0.4, 0, 0.2, 1);
}

.bento-card:hover {
  transform: translateY(-2px);
  box-shadow: 0 8px 30px rgba(0, 0, 0, 0.05), 0 4px 12px rgba(0, 0, 0, 0.04);
}

[data-theme='dark'] .bento-card {
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.3), 0 2px 8px rgba(0, 0, 0, 0.2);
}

[data-theme='dark'] .bento-card:hover {
  box-shadow: 0 8px 30px rgba(0, 0, 0, 0.4), 0 4px 12px rgba(0, 0, 0, 0.3);
}

.bento-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 14px;
}

.bento-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary);
  letter-spacing: 0.01em;
}

.bento-number {
  font-size: 32px;
  font-weight: 800;
  color: var(--text-primary);
  letter-spacing: -0.03em;
  font-family: -apple-system, 'SF Pro Display', system-ui, sans-serif;
}

.bento-badge {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
  padding: 4px 10px;
  border-radius: var(--radius-full);
  background: var(--bg-input);
}

/* ===== Trend chart ===== */
.trend-chart {
  height: 80px;
  margin-top: 4px;
}

.trend-svg {
  width: 100%;
  height: 100%;
  overflow: visible;
}

.trend-line {
  filter: drop-shadow(0 1px 3px rgba(255, 159, 10, 0.35));
}

.trend-dot {
  opacity: 0;
  transition: opacity 0.2s ease;
}

.trend-svg:hover .trend-dot {
  opacity: 1;
}

.trend-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  font-size: 13px;
  color: var(--text-muted);
}

/* ===== Heatmap ===== */
.heatmap-wrap {
  display: flex;
  flex-direction: column;
  gap: 8px;
  overflow-x: auto;
}

.heatmap-months {
  display: flex;
  gap: 0;
  height: 16px;
  margin-bottom: 2px;
  position: relative;
  padding-left: 2px;
}

.heatmap-month-label {
  font-size: 10px;
  color: var(--text-muted);
  position: absolute;
  white-space: nowrap;
}

.heatmap-grid {
  display: flex;
  gap: 3px;
  overflow-x: auto;
}

.heatmap-days-col {
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.heatmap-cell {
  width: 11px;
  height: 11px;
  border-radius: 2px;
  flex-shrink: 0;
}

.heat-0 { background: var(--bg-active); }
.heat-1 { background: rgba(52, 199, 89, 0.3); }
.heat-2 { background: rgba(52, 199, 89, 0.5); }
.heat-3 { background: rgba(52, 199, 89, 0.75); }
.heat-4 { background: #34c759; }

[data-theme='dark'] .heat-0 { background: rgba(255, 255, 255, 0.08); }
[data-theme='dark'] .heat-1 { background: rgba(48, 209, 88, 0.25); }
[data-theme='dark'] .heat-2 { background: rgba(48, 209, 88, 0.45); }
[data-theme='dark'] .heat-3 { background: rgba(48, 209, 88, 0.7); }
[data-theme='dark'] .heat-4 { background: #30d158; }

.heatmap-legend {
  display: flex;
  align-items: center;
  gap: 4px;
  justify-content: flex-end;
}

.heatmap-legend-text {
  font-size: 10px;
  color: var(--text-muted);
}

.heatmap-legend .heatmap-cell {
  width: 10px;
  height: 10px;
}

/* ===== Knowledge point grid ===== */
.kp-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 10px;
}

@media (max-width: 1100px) {
  .kp-grid { grid-template-columns: repeat(2, 1fr); }
}

.kp-cell {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 12px 14px;
  border-radius: 10px;
  background: transparent;
  transition: background 0.2s ease;
}

.kp-cell:hover {
  background: var(--bg-hover);
}

.kp-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 36px;
  height: 36px;
  border-radius: 50%;
  background: radial-gradient(circle at 30% 30%, color-mix(in srgb, var(--icon-color) 15%, transparent), color-mix(in srgb, var(--icon-color) 5%, transparent));
  color: var(--icon-color);
  flex-shrink: 0;
}

.kp-info {
  display: flex;
  flex-direction: column;
  gap: 0;
  min-width: 0;
}

.kp-name {
  font-size: 12px;
  font-weight: 500;
  color: var(--text-muted);
}

.kp-count {
  font-size: 24px;
  font-weight: 800;
  color: var(--text-primary);
  letter-spacing: -0.03em;
  line-height: 1.1;
}

/* ===== Tag bars ===== */
.tag-bars {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.tag-bar-row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.tag-bar-label {
  font-size: 12px;
  font-weight: 500;
  color: var(--text-secondary);
  width: 42px;
  flex-shrink: 0;
}

.tag-bar-track {
  flex: 1;
  height: 8px;
  border-radius: var(--radius-full);
  background: var(--bg-input);
  overflow: hidden;
}

.tag-bar-fill {
  height: 100%;
  border-radius: var(--radius-full);
  transition: width 0.6s cubic-bezier(0.4, 0, 0.2, 1);
}

.tag-bar-count {
  font-size: 13px;
  font-weight: 700;
  color: var(--text-primary);
  width: 20px;
  text-align: right;
  flex-shrink: 0;
}

/* ===== Donut chart ===== */
.type-donut {
  position: relative;
  width: 110px;
  height: 110px;
  margin: 0 auto 12px;
}

.donut-svg {
  width: 100%;
  height: 100%;
}

.donut-seg {
  transition: stroke-dasharray 0.6s cubic-bezier(0.4, 0, 0.2, 1);
}

.donut-center {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  display: flex;
  flex-direction: column;
  align-items: center;
  line-height: 1.1;
}

.donut-total {
  font-size: 22px;
  font-weight: 800;
  color: var(--text-primary);
  letter-spacing: -0.03em;
}

.donut-label {
  font-size: 10px;
  color: var(--text-muted);
}

.donut-legend {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.donut-legend-item {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
}

.donut-legend-dot {
  width: 8px;
  height: 8px;
  border-radius: 2px;
  flex-shrink: 0;
}

.donut-legend-text {
  flex: 1;
  color: var(--text-secondary);
}

.donut-legend-count {
  font-weight: 600;
  color: var(--text-primary);
}

/* ===== Difficulty distribution ===== */
.diff-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.diff-row {
  display: flex;
  align-items: center;
  gap: 12px;
}

.diff-stars {
  display: flex;
  gap: 2px;
  flex-shrink: 0;
}

.diff-star {
  width: 13px;
  height: 13px;
  background: var(--bg-active);
  clip-path: polygon(50% 0%, 61% 35%, 98% 35%, 68% 57%, 79% 91%, 50% 70%, 21% 91%, 32% 57%, 2% 35%, 39% 35%);
}

.diff-star.filled {
  background: var(--star-color);
}

[data-theme='dark'] .diff-star.filled {
  background: #ffd60a;
}

.diff-track {
  flex: 1;
  height: 8px;
  border-radius: var(--radius-full);
  background: var(--bg-input);
  overflow: hidden;
}

.diff-track-fill {
  height: 100%;
  border-radius: var(--radius-full);
  transition: width 0.6s cubic-bezier(0.4, 0, 0.2, 1);
}

.diff-badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 28px;
  padding: 3px 8px;
  border-radius: var(--radius-full);
  font-size: 12px;
  font-weight: 700;
  flex-shrink: 0;
}

/* ===== Responsive ===== */
@media (max-width: 640px) {
  .dash-hello { font-size: 24px; }
  .bento-card { padding: 14px 16px; }
  .kp-grid { grid-template-columns: 1fr 1fr; gap: 8px; }
  .bento-number { font-size: 26px; }
}
</style>
