<script setup lang="ts">
import { computed } from 'vue'
import { PAPER_KIND_OPTIONS } from '@/utils/questionSource'
import {
  YEAR_OPTIONS,
  PROVINCE_OPTIONS,
  citiesForProvince,
  gradeSemesterOptions,
} from '@/utils/paperFormOptions'

export interface PaperFilterState {
  year: string
  grade: string
  semester: string
  sourceKind: string
  region: string
  city: string
}

const props = defineProps<{
  stage?: string
  modelValue: PaperFilterState
}>()

const emit = defineEmits<{
  'update:modelValue': [value: PaperFilterState]
}>()

const yearOpts = ['全部', ...[...YEAR_OPTIONS].reverse(), '更早以前']
const gradeOpts = computed(() => gradeSemesterOptions(props.stage || 'senior'))
const cityOpts = computed(() => citiesForProvince(props.modelValue.region))
const regionOpts = ['全部', '全国', ...PROVINCE_OPTIONS]

function patch(partial: Partial<PaperFilterState>) {
  emit('update:modelValue', { ...props.modelValue, ...partial })
}

function isGradeActive(grade: string, semester: string) {
  return props.modelValue.grade === grade && props.modelValue.semester === semester
}
</script>

<template>
  <div class="ql-matrix-panel pf-bar">
    <div class="ql-matrix-row">
      <span class="ql-matrix-label">年级:</span>
      <div class="ql-matrix-tags">
        <button
          type="button"
          class="ql-mtag"
          :class="{ active: !modelValue.grade }"
          @click="patch({ grade: '', semester: '' })"
        >全部</button>
        <button
          v-for="opt in gradeOpts"
          :key="`${opt.grade}-${opt.semester}`"
          type="button"
          class="ql-mtag"
          :class="{ active: isGradeActive(opt.grade, opt.semester) }"
          @click="patch({ grade: opt.grade, semester: opt.semester })"
        >{{ opt.label }}</button>
      </div>
    </div>

    <div class="ql-matrix-row">
      <span class="ql-matrix-label">年份:</span>
      <div class="ql-matrix-tags">
        <button
          v-for="y in yearOpts"
          :key="y"
          type="button"
          class="ql-mtag"
          :class="{ active: modelValue.year === y }"
          @click="patch({ year: y })"
        >{{ y }}</button>
      </div>
    </div>

    <div class="ql-matrix-row">
      <span class="ql-matrix-label">类型:</span>
      <div class="ql-matrix-tags">
        <button
          type="button"
          class="ql-mtag"
          :class="{ active: !modelValue.sourceKind }"
          @click="patch({ sourceKind: '' })"
        >全部</button>
        <button
          v-for="opt in PAPER_KIND_OPTIONS"
          :key="opt.value"
          type="button"
          class="ql-mtag"
          :class="{ active: modelValue.sourceKind === opt.value }"
          @click="patch({ sourceKind: opt.value })"
        >{{ opt.label }}</button>
      </div>
    </div>

    <div class="ql-matrix-row">
      <span class="ql-matrix-label">地区:</span>
      <div class="ql-matrix-tags">
        <button
          v-for="p in regionOpts"
          :key="p"
          type="button"
          class="ql-mtag"
          :class="{ active: modelValue.region === p }"
          @click="patch({ region: p, city: '' })"
        >{{ p }}</button>
      </div>
    </div>

    <div v-if="modelValue.region && modelValue.region !== '全部' && modelValue.region !== '全国' && cityOpts.length" class="ql-matrix-row">
      <span class="ql-matrix-label">市/区:</span>
      <div class="ql-matrix-tags">
        <button
          type="button"
          class="ql-mtag"
          :class="{ active: !modelValue.city }"
          @click="patch({ city: '' })"
        >全部</button>
        <button
          v-for="c in cityOpts"
          :key="c"
          type="button"
          class="ql-mtag"
          :class="{ active: modelValue.city === c }"
          @click="patch({ city: c })"
        >{{ c }}</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* ===== 多维属性矩阵筛选面板（试卷筛选，完全对齐 QuestionList 规范） ===== */
.ql-matrix-panel {
  position: relative;
  z-index: 10;
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 16px 20px 16px;
  background: var(--bg-primary); /* 极淡灰背景，暗色模式下为 #1c1c1e */
  border-top: 1px solid var(--border-color);
  border-bottom: 1px solid var(--border-color); /* 底部清晰分割线 */
}

/* —— 平铺标签行 —— */
.ql-matrix-row {
  display: flex;
  align-items: flex-start;
  gap: 16px;
  padding: 2px 0;
}

.ql-matrix-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-muted);
  letter-spacing: 0.03em;
  flex-shrink: 0;
  min-width: 44px;
  height: 30px;
  line-height: 30px;
}

.ql-matrix-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 7px;
  flex: 1;
  min-width: 0;
}

/* 单个标签 — 扁平化：纯文本，无背景/边框/圆角 */
.ql-mtag {
  padding: 3px 10px;
  border-radius: 0;
  font-size: 13px;
  font-weight: 500;
  color: var(--text-secondary);
  background: transparent;
  border: none;
  transition: var(--transition-fast);
  white-space: nowrap;
  cursor: pointer;
}

.ql-mtag:hover {
  color: var(--text-primary);
}

/* 选中态：品牌蓝文字，无背景 */
.ql-mtag.active {
  color: var(--accent);
  font-weight: 600;
  background: transparent;
  border: none;
}

/* 暗色模式适配 */
[data-theme='dark'] .ql-matrix-panel {
  background: var(--bg-primary);
  border-color: var(--border-color);
}

[data-theme='dark'] .ql-matrix-label {
  color: var(--text-muted);
}

[data-theme='dark'] .ql-mtag {
  color: var(--text-secondary);
}

[data-theme='dark'] .ql-mtag:hover {
  color: var(--text-primary);
}

[data-theme='dark'] .ql-mtag.active {
  color: var(--accent);
}
</style>
