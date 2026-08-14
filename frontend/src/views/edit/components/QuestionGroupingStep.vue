<script setup lang="ts">
/**
 * V2.1.1 Mixed 资料题目分组步骤（F1 步骤 5，计划书 §6.2）
 *
 * 先整体解析 → 用户把题目分配到各集合：
 * - 单选/多选题目 → 目标集合 → 分配所选
 * - 一键"全部归入"某集合
 * - 已分配题目显示归属徽标；未分配保留"未分组"
 */
import { ref, computed } from 'vue'
import { AppButton, AppIcon } from '@/components/ui'
import { useToast } from '@/composables/useToast'
import { collectionApi, type QuestionCollectionSummary } from '@/api/client'

export interface GroupQuestion {
  question_id: string
  question_no: string | null
  stem: string
  /** 已归属集合 ID（null = 未分组） */
  collection_id: string | null
}

const props = defineProps<{
  questions: GroupQuestion[]
  collections: QuestionCollectionSummary[]
}>()

const emit = defineEmits<{ (e: 'complete'): void }>()

const toast = useToast()

const selectedIds = ref<Set<string>>(new Set())
const targetCollectionId = ref('')
const assigning = ref(false)

const COLLECTION_TYPE_LABELS: Record<string, string> = {
  class_exercise: '课堂练习',
  class_example: '课堂例题',
  homework: '课后作业',
  preview_exercise: '课前预习',
  textbook_example: '教材例题',
  teaching_material: '教学讲义/资料',
  exercise_book: '教辅练习',
  chapter_exercise: '章节练习',
  unit_exercise: '单元练习',
  special_training: '专题训练',
  wrong_question: '错题整理',
  other: '其他',
}

const unassignedCount = computed(() => props.questions.filter(q => !q.collection_id).length)

function toggleSelect(qid: string) {
  const s = new Set(selectedIds.value)
  if (s.has(qid)) {
    s.delete(qid)
  } else {
    s.add(qid)
  }
  selectedIds.value = s
}

function selectAll() {
  selectedIds.value = new Set(props.questions.map(q => q.question_id))
}

function clearSelection() {
  selectedIds.value = new Set()
}

function collectionName(cid: string | null): string {
  if (!cid) return '未分组'
  const c = props.collections.find(x => x.id === cid)
  if (!c) return '未知集合'
  return `${COLLECTION_TYPE_LABELS[c.collection_type] ?? c.collection_type} · ${c.title}`
}

async function assignSelected() {
  const ids = [...selectedIds.value]
  if (ids.length === 0) {
    toast.warning('请先选择要分配的题目')
    return
  }
  if (!targetCollectionId.value) {
    toast.warning('请选择目标集合')
    return
  }
  assigning.value = true
  try {
    const { data } = await collectionApi.batchAddQuestions(targetCollectionId.value, ids.map(id => ({ question_id: id })))
    // 回写本地归属状态
    for (const q of props.questions) {
      if (selectedIds.value.has(q.question_id)) {
        q.collection_id = targetCollectionId.value
      }
    }
    selectedIds.value = new Set()
    toast.success(`已分配 ${data.inserted} 题到集合`)
    if (data.skipped > 0) {
      toast.warning(`${data.skipped} 题已在集合中，跳过`)
    }
  } catch (e: any) {
    toast.error(e?.response?.data?.error ?? e?.message ?? '分配失败')
  } finally {
    assigning.value = false
  }
}

async function assignAllTo(targetId: string) {
  const ids = props.questions.filter(q => !q.collection_id).map(q => q.question_id)
  if (ids.length === 0) {
    toast.warning('没有未分组的题目')
    return
  }
  assigning.value = true
  try {
    const { data } = await collectionApi.batchAddQuestions(targetId, ids.map(id => ({ question_id: id })))
    for (const q of props.questions) {
      if (!q.collection_id) q.collection_id = targetId
    }
    toast.success(`已将 ${data.inserted} 题全部归入集合`)
  } catch (e: any) {
    toast.error(e?.response?.data?.error ?? e?.message ?? '分配失败')
  } finally {
    assigning.value = false
  }
}
</script>

<template>
  <div class="grouping-step">
    <div class="grouping-head">
      <div>
        <div class="grouping-title">题目分组</div>
        <div class="grouping-sub">
          共 {{ questions.length }} 题，未分组 {{ unassignedCount }} 题 — 解析完成，请把题目归入对应集合
        </div>
      </div>
      <AppButton variant="ghost" size="sm" @click="selectAll">全选</AppButton>
      <AppButton variant="ghost" size="sm" @click="clearSelection">清空</AppButton>
    </div>

    <!-- 分配操作条 -->
    <div class="grouping-toolbar">
      <select v-model="targetCollectionId" class="grouping-select">
        <option value="">选择目标集合…</option>
        <option v-for="c in collections" :key="c.id" :value="c.id">
          {{ COLLECTION_TYPE_LABELS[c.collection_type] ?? c.collection_type }} · {{ c.title }}
        </option>
      </select>
      <AppButton variant="primary" size="sm" :loading="assigning" :disabled="selectedIds.size === 0" @click="assignSelected">
        <AppIcon name="check" :size="14" /> 分配所选（{{ selectedIds.size }}）
      </AppButton>
    </div>

    <!-- 一键归入 -->
    <div class="grouping-quick">
      <span class="grouping-quick-label">全部归入：</span>
      <button
        v-for="c in collections"
        :key="c.id"
        type="button"
        class="grouping-quick-btn"
        :disabled="assigning || unassignedCount === 0"
        @click="assignAllTo(c.id)"
      >
        {{ COLLECTION_TYPE_LABELS[c.collection_type] ?? c.collection_type }} · {{ c.title }}
      </button>
    </div>

    <!-- 题目列表 -->
    <div class="grouping-list">
      <label
        v-for="(q, i) in questions"
        :key="q.question_id"
        class="grouping-item"
        :class="{ selected: selectedIds.has(q.question_id) }"
      >
        <input
          type="checkbox"
          :checked="selectedIds.has(q.question_id)"
          @change="toggleSelect(q.question_id)"
        />
        <span class="grouping-qno">{{ q.question_no ?? i + 1 }}</span>
        <span class="grouping-stem">{{ q.stem }}</span>
        <span class="grouping-badge" :class="{ unassigned: !q.collection_id }">
          {{ collectionName(q.collection_id) }}
        </span>
      </label>
    </div>

    <div class="grouping-actions">
      <AppButton variant="primary" @click="emit('complete')">
        <AppIcon name="check" :size="16" /> 完成分组，进入录入
      </AppButton>
    </div>
  </div>
</template>

<style scoped>
.grouping-step { display: flex; flex-direction: column; gap: 12px; }
.grouping-head { display: flex; align-items: center; gap: 8px; }
.grouping-head > div:first-child { flex: 1; }
.grouping-title { font-size: 15px; font-weight: 700; color: var(--text-primary); }
.grouping-sub { font-size: 12px; color: var(--text-secondary); margin-top: 2px; }
.grouping-toolbar { display: flex; gap: 8px; align-items: center; }
.grouping-select {
  flex: 1;
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: 8px;
  font-size: 13px;
  background: var(--bg-input);
  color: var(--text-primary);
  outline: none;
}
.grouping-quick { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
.grouping-quick-label { font-size: 12px; color: var(--text-secondary); }
.grouping-quick-btn {
  font-size: 12px;
  padding: 4px 10px;
  border: 1px solid var(--border);
  border-radius: 12px;
  background: var(--bg-input);
  color: var(--accent);
  cursor: pointer;
}
.grouping-quick-btn:disabled { opacity: 0.4; cursor: not-allowed; }
.grouping-list {
  max-height: 320px;
  overflow-y: auto;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  display: flex;
  flex-direction: column;
}
.grouping-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  border-bottom: 1px solid var(--border);
  cursor: pointer;
  font-size: 13px;
}
.grouping-item:last-child { border-bottom: none; }
.grouping-item.selected { background: var(--accent-light, rgba(88, 86, 214, 0.08)); }
.grouping-qno {
  font-weight: 700;
  color: var(--purple);
  min-width: 28px;
  flex-shrink: 0;
}
.grouping-stem {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-primary);
}
.grouping-badge {
  flex-shrink: 0;
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 10px;
  background: var(--success-light, rgba(52, 199, 89, 0.12));
  color: var(--success);
}
.grouping-badge.unassigned {
  background: var(--warning-light);
  color: var(--warning);
}
.grouping-actions { display: flex; justify-content: flex-end; }
</style>
