<script setup lang="ts">
/**
 * V2.1.1 P1：集合详情（来源链路 + 题目列表 + 移除题目）
 *
 * 来源链路：Document（资料）→ Collection（集合）→ Questions（题目）。
 */
import { ref, computed, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { AppButton, AppConfirm, AppEmpty, AppIcon } from '@/components/ui'
import { useToast } from '@/composables/useToast'
import { collectionApi, type CollectionDetail } from '@/api/client'

const route = useRoute()
const router = useRouter()
const toast = useToast()

const detail = ref<CollectionDetail | null>(null)
const loading = ref(false)
const removeTarget = ref<string | null>(null)

const showRemoveConfirm = computed({
  get: () => removeTarget.value !== null,
  set: (v: boolean) => {
    if (!v) removeTarget.value = null
  },
})

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
const DOCUMENT_TYPE_LABELS: Record<string, string> = {
  exam: '正式试卷', mock_exam: '模拟试卷', class_exercise: '课堂练习', class_example: '课堂例题',
  homework: '课后作业', preview_exercise: '课前预习', textbook_example: '教材例题',
  teaching_material: '教学讲义/资料', exercise_book: '教辅练习', chapter_exercise: '章节练习',
  unit_exercise: '单元练习', special_training: '专题训练', wrong_question: '错题整理',
  mixed: '混合资料', other: '其他',
}

async function load() {
  loading.value = true
  try {
    const { data } = await collectionApi.get(route.params.id as string)
    detail.value = data
  } catch (e: any) {
    toast.error(e?.response?.data?.error ?? '加载集合失败')
  } finally {
    loading.value = false
  }
}

async function doRemove() {
  if (!detail.value || !removeTarget.value) return
  try {
    await collectionApi.removeQuestion(detail.value.id, removeTarget.value)
    toast.success('已从集合移除')
    removeTarget.value = null
    load()
  } catch (e: any) {
    toast.error(e?.response?.data?.error ?? '移除失败')
  }
}

onMounted(load)
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div>
        <AppButton variant="ghost" size="sm" @click="router.back()">
          <AppIcon name="chevron-left" :size="14" /> 返回
        </AppButton>
        <h1 class="page-title">{{ detail?.title ?? '集合详情' }}</h1>
        <p v-if="detail" class="page-sub">
          类型：{{ COLLECTION_TYPE_LABELS[detail.collection_type] ?? detail.collection_type }}
          <template v-if="detail.grade || detail.subject">｜{{ detail.grade }} {{ detail.subject }}</template>
        </p>
      </div>
    </div>

    <!-- 来源链路：Document → Collection -->
    <div v-if="detail" class="chain-card">
      <div class="chain-item">
        <span class="chain-role">资料（Document）</span>
        <span class="chain-value">📄 {{ detail.document_title ?? '未命名资料' }}</span>
        <span v-if="detail.document_type" class="chain-badge">
          {{ DOCUMENT_TYPE_LABELS[detail.document_type] ?? detail.document_type }}
        </span>
      </div>
      <div class="chain-arrow">↓</div>
      <div class="chain-item">
        <span class="chain-role">集合（Collection）</span>
        <span class="chain-value">{{ detail.title }}</span>
      </div>
      <div class="chain-arrow">↓</div>
      <div class="chain-item">
        <span class="chain-role">题目（Questions）</span>
        <span class="chain-value">{{ detail.questions.length }} 道</span>
      </div>
    </div>

    <div v-if="loading" class="loading-hint">加载中…</div>
    <AppEmpty v-else-if="detail && detail.questions.length === 0" title="集合暂无题目" description="混合资料可在解析完成后通过分组步骤把题目归入集合" />
    <div v-else-if="detail" class="question-list">
      <div v-for="q in detail.questions" :key="q.id" class="question-item">
        <span class="q-no">{{ q.question_no ?? '—' }}</span>
        <span class="q-stem">{{ q.stem }}</span>
        <span v-if="q.score != null" class="q-score">{{ q.score }} 分</span>
        <button class="q-remove" title="从集合移除" @click="removeTarget = q.question_id">
          <AppIcon name="trash" :size="14" />
        </button>
      </div>
    </div>

    <AppConfirm
      v-model="showRemoveConfirm"
      title="移除题目"
      message="将该题目从集合中移除（题目本身保留在题库中）。"
      confirm-text="移除"
      danger
      @confirm="doRemove"
      @cancel="removeTarget = null"
    />
  </div>
</template>

<style scoped>
.page { display: flex; flex-direction: column; gap: 14px; padding: 4px 0 40px; }
.page-title { font-size: 20px; font-weight: 700; margin: 8px 0 0; }
.page-sub { font-size: 13px; color: var(--text-secondary); margin: 4px 0 0; }
.loading-hint { font-size: 13px; color: var(--text-secondary); }

.chain-card {
  border: 1px solid var(--border); border-radius: 12px; padding: 14px;
  background: var(--bg-card, var(--bg-input));
  display: flex; flex-direction: column; gap: 6px;
}
.chain-item { display: flex; align-items: center; gap: 10px; font-size: 13px; }
.chain-role { width: 120px; flex-shrink: 0; font-size: 11px; font-weight: 700; color: var(--text-secondary); }
.chain-value { font-weight: 600; }
.chain-badge {
  font-size: 11px; padding: 1px 8px; border-radius: 10px;
  color: var(--purple); background: var(--purple-light);
}
.chain-arrow { font-size: 12px; color: var(--text-secondary); padding-left: 56px; }

.question-list { display: flex; flex-direction: column; gap: 8px; }
.question-item {
  display: flex; align-items: center; gap: 10px;
  border: 1px solid var(--border); border-radius: 10px; padding: 10px 12px;
  background: var(--bg-card, var(--bg-input));
}
.q-no { font-weight: 700; color: var(--purple); min-width: 30px; }
.q-stem { flex: 1; font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.q-score { font-size: 12px; color: var(--text-secondary); flex-shrink: 0; }
.q-remove {
  border: none; background: none; color: var(--danger); cursor: pointer;
  display: flex; align-items: center; padding: 4px;
}
.q-remove:hover { opacity: 0.7; }
</style>
