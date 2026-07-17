<script setup lang="ts">
import { computed, ref } from 'vue'
import { AppSelect, AppIcon } from '@/components/ui'

const questionType = defineModel<string>('questionType', { required: true })
const difficulty = defineModel<string>('difficulty', { required: true })
const difficultyCoefficient = defineModel<number>('difficultyCoefficient', { required: true })
const academicYear = defineModel<string>('academicYear', { required: true })
const gradeSemester = defineModel<string>('gradeSemester', { required: true })
const examType = defineModel<string>('examType', { required: true })
const examRegion = defineModel<string>('examRegion', { required: true })

const typeOptions = [
  { label: '选择题', value: 'choice' },
  { label: '填空题', value: 'fill' },
  { label: '解答题', value: 'solution' },
]

const currentYear = new Date().getFullYear()
const academicYearOptions = [
  { label: `${currentYear - 1}-${String(currentYear).slice(2)}`, value: `${currentYear - 1}-${String(currentYear).slice(2)}` },
  { label: `${currentYear}-${String(currentYear + 1).slice(2)}`, value: `${currentYear}-${String(currentYear + 1).slice(2)}` },
  { label: `${currentYear + 1}-${String(currentYear + 2).slice(2)}`, value: `${currentYear + 1}-${String(currentYear + 2).slice(2)}` },
]

const gradeSemesterOptions = [
  ...['初一', '初二', '初三'].flatMap(g => [
    { label: `${g}上`, value: `${g}上` },
    { label: `${g}下`, value: `${g}下` },
  ]),
  ...['高一', '高二', '高三'].flatMap(g => [
    { label: `${g}上`, value: `${g}上` },
    { label: `${g}下`, value: `${g}下` },
  ]),
]

const examTypeOptions = [
  { label: '期末', value: '期末' },
  { label: '期中', value: '期中' },
  { label: '月考', value: '月考' },
  { label: '周测', value: '周测' },
  { label: '模拟', value: '模拟' },
  { label: '高考', value: '高考' },
  { label: '中考', value: '中考' },
  { label: '竞赛', value: '竞赛' },
]

// Sync internal rating stars with incoming difficulty properties
const difficultyStars = computed<number>({
  get: () => {
    if (difficulty.value === 'easy') return difficultyCoefficient.value > 0.8 ? 1 : 2
    if (difficulty.value === 'medium') return 3
    return difficultyCoefficient.value < 0.3 ? 5 : 4
  },
  set: (v: number) => {
    difficultyCoefficient.value = [0.9, 0.75, 0.55, 0.35, 0.2][v - 1] ?? 0.55
    difficulty.value = v <= 2 ? 'easy' : v === 3 ? 'medium' : 'hard'
  },
})
</script>

<template>
  <div class="meta-bar bg-white rounded-2xl shadow-sm border-none p-4 mb-4 flex flex-wrap gap-4 items-center">
    <AppSelect v-model="questionType" :options="typeOptions" placeholder="题型" class="meta-field" />
    <div class="meta-field meta-field-diff">
      <div class="diff-row">
        <button
          v-for="n in 5"
          :key="n"
          type="button"
          class="star"
          :class="{ active: difficultyStars >= n }"
          @click="difficultyStars = n"
        >
          <AppIcon name="star" :size="15" />
        </button>
      </div>
    </div>
    <AppSelect v-model="academicYear" :options="academicYearOptions" placeholder="学年" clearable class="meta-field" />
    <AppSelect v-model="gradeSemester" :options="gradeSemesterOptions" placeholder="年级学期" clearable class="meta-field" />
    <AppSelect v-model="examType" :options="examTypeOptions" placeholder="考试类型" clearable class="meta-field" />
    <input
      v-model="examRegion"
      placeholder="考试地区"
      class="meta-field text-input"
    />
  </div>
</template>

<style scoped>
/* ============ 元数据工具栏 — 第一层：核心控制元数据栏（单行不换行） ============ */
.meta-bar {
  display: flex;
  align-items: center;
  white-space: nowrap;
  overflow-x: auto;
  gap: 8px;
  flex-shrink: 0;
  padding: 10px 14px;
  background: var(--bg-card);
  border: 1px solid transparent !important;
  border-radius: 16px;
  box-shadow: var(--shadow-sm);
  scrollbar-width: thin;
}

[data-theme='dark'] .meta-bar {
  border: 1px solid #3a3a3c !important;
  box-shadow: none !important;
}

.meta-bar::-webkit-scrollbar {
  height: 4px;
}

.meta-bar::-webkit-scrollbar-thumb {
  background: var(--border-color);
  border-radius: 2px;
}

/* 通用胶囊样式 — 适用于 AppSelect、input、button、div */
.meta-field {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex: initial;
  min-width: 0;
  padding: 4px 16px;
  height: 32px;
  border-radius: 9999px;
  background: #fff;
  border: 1px solid #e0e0e0;
  color: #8c8c8c;
  font-size: 13px;
  cursor: pointer;
  transition: all 0.2s ease;
  position: relative;
  box-sizing: border-box;
  white-space: nowrap;
}

/* 已选择/激活状态：文字加深、边框加深、淡雅背景 */
.meta-field :deep(.app-select-trigger.has-value),
.meta-field.text-input:not(:placeholder-shown) {
  color: #262626;
}

.meta-field.app-select-wrapper:has(.app-select-trigger.has-value) {
  color: #262626;
  border-color: #b7b7b7;
  background: rgba(0, 122, 255, 0.03);
}

[data-theme='dark'] .meta-field.app-select-wrapper:has(.app-select-trigger.has-value) {
  color: rgba(255, 255, 255, 0.95);
  border-color: rgba(255, 255, 255, 0.25);
  background: rgba(0, 122, 255, 0.08);
}

.meta-field.text-input:not(:placeholder-shown) {
  color: #262626;
  border-color: #b7b7b7;
  background: rgba(0, 122, 255, 0.03);
}

[data-theme='dark'] .meta-field.text-input:not(:placeholder-shown) {
  color: rgba(255, 255, 255, 0.95);
  border-color: rgba(255, 255, 255, 0.25);
  background: rgba(0, 122, 255, 0.08);
}

/* 暗色模式 */
[data-theme='dark'] .meta-field {
  background: rgba(255, 255, 255, 0.06);
  border-color: rgba(255, 255, 255, 0.12);
  color: rgba(255, 255, 255, 0.5);
}

.meta-field:hover {
  border-color: var(--accent);
}

.meta-field:focus-within {
  border-color: #b7b7b7;
  box-shadow: 0 0 0 3px rgba(0, 122, 255, 0.06);
}

/* AppSelect 直接作为 meta-field 时的胶囊样式 */
.meta-field.app-select-wrapper {
  display: inline-flex;
  width: auto;
  padding: 4px 16px;
  height: 32px;
  border-radius: 9999px;
  background: #fff;
  border: 1px solid #e0e0e0;
  color: #8c8c8c;
  box-sizing: border-box;
}

/* 内层 trigger 彻底隐形化 — 由外层胶囊接管全部视觉 */
.meta-field :deep(.app-select-trigger) {
  border: none !important;
  background: transparent !important;
  outline: none !important;
  box-shadow: none !important;
  appearance: none;
  -webkit-appearance: none;
  padding: 0 !important;
  min-height: auto !important;
  height: 100%;
  width: 100%;
  font-size: 13px;
  color: inherit;
  border-radius: 0;
}

.meta-field :deep(.app-select-trigger:hover) {
  border: none !important;
  background: transparent !important;
  box-shadow: none !important;
}

.meta-field :deep(.app-select-trigger.open) {
  border: none !important;
  box-shadow: none !important;
  background: transparent !important;
}

.meta-field :deep(.app-select-text) {
  white-space: nowrap;
  color: inherit;
}

.meta-field :deep(.app-select-text.placeholder) {
  color: var(--text-muted);
}

/* text-input 作为胶囊 */
.meta-field.text-input {
  border: 1px solid #d9d9d9;
  background: #fff;
  border-radius: 9999px;
  padding: 4px 16px;
  height: 32px;
  font-size: 13px;
  color: var(--text-primary);
  outline: none;
  width: auto;
  max-width: 140px;
}

[data-theme='dark'] .meta-field.text-input {
  background: rgba(255, 255, 255, 0.06);
  border-color: rgba(255, 255, 255, 0.15);
  color: rgba(255, 255, 255, 0.9);
}

.meta-field.text-input::placeholder {
  color: var(--text-muted);
}

.meta-field.text-input:focus {
  border-color: #b7b7b7;
  box-shadow: 0 0 0 3px rgba(0, 122, 255, 0.06);
}

/* 难度星级胶囊 — 对称内边距呼吸感 */
.meta-field-diff {
  gap: 2px;
  padding: 0 16px;
}

.diff-row {
  display: flex;
  align-items: center;
  gap: 2px;
  min-height: auto;
}

.star {
  color: var(--border-strong);
  background: none;
  border: none;
  cursor: pointer;
  padding: 4px;
  display: inline-flex;
  transition: var(--transition-fast);
}

/* SVG 图标不拦截点击，确保点击事件落在 button 上 */
.star :deep(svg),
.star svg {
  pointer-events: none;
}

.star:hover {
  transform: scale(1.12);
}

.star.active {
  color: var(--star-color);
}

/* 激活态星星图标覆盖降噪色 — 确保 SVG currentColor 生效 */
.star.active :deep(svg),
.star.active svg {
  color: var(--star-color) !important;
}
</style>
