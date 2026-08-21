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
  <div class="pf-bar">
    <div class="pf-row">
      <span class="pf-label">年级:</span>
      <div class="pf-tags">
        <button
          type="button"
          class="pf-tag"
          :class="{ active: !modelValue.grade }"
          @click="patch({ grade: '', semester: '' })"
        >全部</button>
        <button
          v-for="opt in gradeOpts"
          :key="`${opt.grade}-${opt.semester}`"
          type="button"
          class="pf-tag"
          :class="{ active: isGradeActive(opt.grade, opt.semester) }"
          @click="patch({ grade: opt.grade, semester: opt.semester })"
        >{{ opt.label }}</button>
      </div>
    </div>

    <div class="pf-row">
      <span class="pf-label">年份:</span>
      <div class="pf-tags">
        <button
          v-for="y in yearOpts"
          :key="y"
          type="button"
          class="pf-tag"
          :class="{ active: modelValue.year === y }"
          @click="patch({ year: y })"
        >{{ y }}</button>
      </div>
    </div>

    <div class="pf-row">
      <span class="pf-label">类型:</span>
      <div class="pf-tags">
        <button
          type="button"
          class="pf-tag"
          :class="{ active: !modelValue.sourceKind }"
          @click="patch({ sourceKind: '' })"
        >全部</button>
        <button
          v-for="opt in PAPER_KIND_OPTIONS"
          :key="opt.value"
          type="button"
          class="pf-tag"
          :class="{ active: modelValue.sourceKind === opt.value }"
          @click="patch({ sourceKind: opt.value })"
        >{{ opt.label }}</button>
      </div>
    </div>

    <div class="pf-row">
      <span class="pf-label">地区:</span>
      <div class="pf-tags">
        <button
          v-for="p in regionOpts"
          :key="p"
          type="button"
          class="pf-tag"
          :class="{ active: modelValue.region === p }"
          @click="patch({ region: p, city: '' })"
        >{{ p }}</button>
      </div>
    </div>

    <div v-if="modelValue.region && modelValue.region !== '全部' && modelValue.region !== '全国' && cityOpts.length" class="pf-row">
      <span class="pf-label">市/区:</span>
      <div class="pf-tags">
        <button
          type="button"
          class="pf-tag"
          :class="{ active: !modelValue.city }"
          @click="patch({ city: '' })"
        >全部</button>
        <button
          v-for="c in cityOpts"
          :key="c"
          type="button"
          class="pf-tag"
          :class="{ active: modelValue.city === c }"
          @click="patch({ city: c })"
        >{{ c }}</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.pf-bar {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 12px 20px 16px;
}

.pf-row {
  display: flex;
  align-items: flex-start;
  gap: 16px;
  padding: 2px 0;
}

.pf-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-muted);
  letter-spacing: 0.03em;
  flex-shrink: 0;
  min-width: 44px;
  height: 30px;
  line-height: 30px;
}

.pf-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 8px;
  flex: 1;
  min-width: 0;
}

.pf-tag {
  padding: 3px 8px;
  border: none;
  border-radius: 0;
  font-size: 13px;
  font-weight: 500;
  color: var(--text-secondary);
  background: transparent;
  cursor: pointer;
  white-space: nowrap;
}

.pf-tag:hover {
  color: var(--text-primary);
}

.pf-tag.active {
  color: #1890ff;
  font-weight: 600;
}
</style>
