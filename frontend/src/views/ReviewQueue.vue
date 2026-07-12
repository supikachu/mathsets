<template>
  <div>
    <div class="flex items-center justify-between mb-12">
      <h1 class="page-title" style="margin-bottom: 0"><AppIcon name="shield-check" :size="24" /> 审核队列</h1>
      <AppBadge v-if="!loading" :color="list.length > 0 ? 'yellow' : 'gray'">
        待审核: {{ list.length }} 题
      </AppBadge>
    </div>

    <div v-if="loading" class="loading-hint">加载中…</div>

    <AppEmpty v-else-if="list.length === 0" icon="check-circle" description="所有题目已审核完毕" />

    <template v-else>
      <div
        v-for="q in list"
        :key="q.id"
        class="q-item"
        @click="$router.push(`/questions/${q.id}`)"
      >
        <div class="q-item-header">
          <div class="q-item-meta">
            <AppBadge :color="typeBadgeColor(q.question_type)">
              {{ typeLabel(q.question_type) }}
            </AppBadge>
            <AppBadge color="gray">{{ diffLabel(q.difficulty) }}</AppBadge>
            <span class="text-sm text-muted">创建者: {{ q.creator_id?.substring(0, 8) || '—' }}</span>
          </div>
          <span class="text-sm text-muted">{{ formatTime(q.updated_at) }}</span>
        </div>
        <div class="q-item-content line-clamp-2"><LatexRender :text="q.stem" :inline="true" /></div>
        <div class="q-item-actions" style="margin-top: 8px">
          <AppButton variant="success" size="sm" :loading="reviewing === q.id" @click.stop="handleReview(q, 'approved')">通过</AppButton>
          <AppButton variant="danger" size="sm" @click.stop="handleReview(q, 'rejected')">驳回</AppButton>
        </div>
      </div>
    </template>

    <!-- 驳回弹窗 -->
    <AppModal v-model="rejectDialog" title="驳回原因">
      <div class="form-group">
        <textarea
          v-model="rejectComment"
          class="reject-textarea"
          rows="4"
          placeholder="请输入驳回原因..."
        />
      </div>
      <div class="form-actions">
        <AppButton variant="ghost" @click="rejectDialog = false">取消</AppButton>
        <AppButton variant="primary" :loading="rejecting" @click="confirmReject">确认驳回</AppButton>
      </div>
    </AppModal>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { questionApi, type QuestionSummary } from '@/api/client'
import client from '@/api/client'
import { AppBadge, AppButton, AppEmpty, AppModal, AppIcon } from '@/components/ui'
import LatexRender from '@/components/LatexRender.vue'
import { useToast } from '@/composables/useToast'
import { typeLabel, typeBadgeColor, diffLabel, formatTime } from '@/utils/questionDisplay'

const toast = useToast()
const list = ref<QuestionSummary[]>([])
const loading = ref(true)
const rejectDialog = ref(false)
const rejectComment = ref('')
const currentQ = ref<QuestionSummary | null>(null)
const reviewing = ref<string | null>(null)
const rejecting = ref(false)

async function fetchList() {
  loading.value = true
  try {
    const res = await questionApi.list({ reviewable_by_me: true, page_size: 50 })
    list.value = res.data
  } catch { /* handled */ }
  finally { loading.value = false }
}

function handleReview(q: QuestionSummary, action: string) {
  currentQ.value = q
  if (action === 'rejected') {
    rejectDialog.value = true
  } else {
    confirmReview(q, action)
  }
}

async function confirmReject() {
  if (!currentQ.value) return
  rejecting.value = true
  const ok = await confirmReview(currentQ.value, 'rejected', rejectComment.value)
  rejecting.value = false
  if (ok) {
    rejectComment.value = ''
    rejectDialog.value = false
  }
}

async function confirmReview(q: QuestionSummary, action: string, comment?: string): Promise<boolean> {
  reviewing.value = q.id
  try {
    await client.post(`/questions/${q.id}/review`, { action, comment })
    toast.success(action === 'approved' ? '已通过' : '已驳回')
    list.value = list.value.filter(item => item.id !== q.id)
    return true
  } catch (e: any) {
    toast.error(e.response?.data?.error || '操作失败')
    return false
  } finally {
    reviewing.value = null
  }
}

onMounted(fetchList)
</script>

<style scoped>
.loading-hint {
  text-align: center;
  padding: 48px 20px;
  color: var(--text-muted);
}

.line-clamp-2 {
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.reject-textarea {
  width: 100%;
  padding: 10px 12px;
  border-radius: var(--radius-sm);
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  color: var(--text-primary);
  font-size: 14px;
  font-family: inherit;
  resize: vertical;
}

.reject-textarea:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
}
</style>
