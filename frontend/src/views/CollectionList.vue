<script setup lang="ts">
/**
 * V2.1.1 P1：资料集合列表
 * 展示当前用户的 QuestionCollection（非试卷资料的题目容器）。
 */
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { AppEmpty } from '@/components/ui'
import { collectionApi, type QuestionCollectionSummary } from '@/api/client'
import { useToast } from '@/composables/useToast'

const router = useRouter()
const toast = useToast()

const items = ref<QuestionCollectionSummary[]>([])
const total = ref(0)
const loading = ref(false)

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

async function load() {
  loading.value = true
  try {
    const { data } = await collectionApi.list({ page_size: 100 })
    items.value = data.items
    total.value = data.total
  } catch (e: any) {
    toast.error(e?.response?.data?.error ?? '加载集合失败')
  } finally {
    loading.value = false
  }
}

function openDetail(id: string) {
  router.push(`/collections/${id}`)
}

onMounted(load)
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div>
        <h1 class="page-title">资料集合</h1>
        <p class="page-sub">课堂练习 / 课后作业 / 章节练习等非试卷资料的题目容器（共 {{ total }} 个）</p>
      </div>
    </div>

    <div v-if="loading" class="loading-hint">加载中…</div>
    <AppEmpty v-else-if="items.length === 0" title="暂无集合" description="通过 AI 录题上传非试卷资料后，会自动创建集合" />
    <div v-else class="collection-grid">
      <div
        v-for="c in items"
        :key="c.id"
        class="collection-card"
        @click="openDetail(c.id)"
      >
        <div class="collection-type">{{ COLLECTION_TYPE_LABELS[c.collection_type] ?? c.collection_type }}</div>
        <div class="collection-title">{{ c.title }}</div>
        <div class="collection-meta">
          <span v-if="c.grade">{{ c.grade }}</span>
          <span v-if="c.subject">{{ c.subject }}</span>
          <span v-if="c.semester">{{ c.semester }}</span>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.page { display: flex; flex-direction: column; gap: 14px; padding: 4px 0 40px; }
.page-title { font-size: 20px; font-weight: 700; margin: 0; }
.page-sub { font-size: 13px; color: var(--text-secondary); margin: 4px 0 0; }
.loading-hint { font-size: 13px; color: var(--text-secondary); }
.collection-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 12px; }
.collection-card {
  border: 1px solid var(--border); border-radius: 12px; padding: 14px;
  background: var(--bg-card, var(--bg-input)); cursor: pointer; transition: all 0.15s;
  display: flex; flex-direction: column; gap: 6px;
}
.collection-card:hover { border-color: var(--accent); transform: translateY(-1px); }
.collection-type {
  align-self: flex-start; font-size: 11px; font-weight: 600; padding: 2px 10px;
  border-radius: 9999px; color: var(--purple); background: var(--purple-light);
}
.collection-title { font-size: 15px; font-weight: 600; }
.collection-meta { display: flex; gap: 8px; font-size: 12px; color: var(--text-secondary); }
</style>
