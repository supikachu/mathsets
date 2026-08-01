<template>
  <div style="padding: 20px 24px; height: 100%; overflow-y: auto;">
    <div class="flex items-center justify-between mb-3">
      <h1 class="page-title" style="margin-bottom: 0"><AppIcon name="shield-check" :size="24" /> 审核队列</h1>
      <AppBadge v-if="!loading" :color="list.length > 0 ? 'yellow' : 'gray'">
        待审核: {{ list.length }} 题
      </AppBadge>
    </div>

    <div v-if="loading" class="loading-hint">加载中…</div>

    <!--
      修正 1：个人空间也必须调用 reviewable_by_me，后端会根据 space kind 自动返回
      该用户作为 creator 且 status=pending 的待审题目（自审模式）。
      绝不能在此处把 list 置空，否则会让个人空间的题目永远卡在 pending 状态。
    -->
    <AppEmpty v-else-if="list.length === 0" icon="check-circle" :description="emptyHint" />

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
            <AppBadge :color="diffBadgeColor(q.difficulty)">
              {{ diffLabel(q.difficulty) }}
            </AppBadge>
            <span class="text-sm text-muted">创建者: {{ q.creator_name || q.creator_id?.substring(0, 8) || '—' }}</span>
          </div>
          <span class="text-sm text-muted">{{ formatTime(q.updated_at) }}</span>
        </div>
        <div class="q-item-content line-clamp-2"><LatexRender :text="q.stem" :inline="true" /></div>
        <div class="q-item-actions" style="margin-top: 8px">
          <AppButton variant="primary" size="sm" :loading="reviewing === q.id" @click.stop="handleReview(q, 'approved')">通过</AppButton>
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
        <AppButton variant="ghost" @click="cancelReject">取消</AppButton>
        <AppButton variant="primary" :loading="rejecting" @click="confirmReject">确认驳回</AppButton>
      </div>
    </AppModal>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { questionApi, type QuestionSummary } from '@/api/client'
import { AppBadge, AppButton, AppEmpty, AppModal, AppIcon } from '@/components/ui'
import LatexRender from '@/components/LatexRender.vue'
import { useToast } from '@/composables/useToast'
import { useSpaceStore } from '@/stores/space'
import { typeLabel, typeBadgeColor, diffLabel, diffBadgeColor, formatTime } from '@/utils/questionDisplay'

const toast = useToast()
const space = useSpaceStore()
const list = ref<QuestionSummary[]>([])
const loading = ref(true)
const rejectDialog = ref(false)
const rejectComment = ref('')
const currentQ = ref<QuestionSummary | null>(null)
const reviewing = ref<string | null>(null)
const rejecting = ref(false)

// 修正 1：所有空间 kind 都调用同一个接口；后端会按身份和空间 kind 自动过滤。
// - 个人空间：返回 status=pending 且 creator=当前用户 的题目（自审模式）
// - 团队空间：返回 reviewer_ids 包含当前用户 的 pending 题目（交叉审核）
// - 公共空间：返回空（公共题目已终审，无内部审核流程）
const currentSpaceKind = computed(() => space.currentSpace?.kind || 'personal')

const emptyHint = computed(() => {
  if (currentSpaceKind.value === 'personal') return '暂无待审核题目（个人空间自审：提交后在此审核）'
  if (currentSpaceKind.value === 'team') return '所有题目已审核完毕'
  return '公共空间无需审核'
})

async function fetchList() {
  loading.value = true
  try {
    const res = await questionApi.list({
      reviewable_by_me: true,
      space_id: space.currentSpaceId || undefined,
      page_size: 50,
    })
    list.value = res.data
  } catch { /* handled */ }
  finally { loading.value = false }
}

// 切换空间时自动刷新
watch(() => space.currentSpaceId, () => fetchList())

function handleReview(q: QuestionSummary, action: string) {
  currentQ.value = q
  if (action === 'rejected') {
    rejectDialog.value = true
  } else {
    confirmReview(q, action)
  }
}

function cancelReject() {
  rejectDialog.value = false
  rejectComment.value = ''
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
    if (action === 'approved') {
      await questionApi.approve(q.id)
    } else {
      await questionApi.reject(q.id, { reject_reason: comment })
    }
    toast.success(action === 'approved' ? '已通过' : '已驳回')
    list.value = list.value.filter(item => item.id !== q.id)
    return true
  } catch (e: any) {
    console.error('审核操作失败:', e)
    toast.error(e.response?.data?.error || e.response?.data?.message || e.message || '操作失败')
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

.no-review-hint {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 60px 20px;
  color: var(--text-muted);
  text-align: center;
}

.no-review-hint p {
  font-size: 15px;
  font-weight: 600;
  color: var(--text-secondary);
  margin: 4px 0 0;
}

.no-review-hint span {
  font-size: 13px;
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
