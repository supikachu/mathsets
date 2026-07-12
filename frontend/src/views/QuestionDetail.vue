<template>
  <div style="padding: 20px 24px; height: 100%; overflow-y: auto;">
    <div v-if="loading" class="loading-hint">加载中…</div>

    <template v-else>
      <!-- 头部导航 -->
      <div class="flex items-center justify-between mb-12">
        <div class="flex items-center gap-3">
          <AppButton variant="ghost" size="sm" @click="$router.push('/questions')"><AppIcon name="chevron-left" :size="17" /> 返回列表</AppButton>
          <h1 class="page-title" style="margin-bottom: 0">题目详情</h1>
        </div>
        <div class="flex gap-2">
          <!-- 草稿 → 编辑 / 提交审核 / 删除 -->
          <template v-if="q?.status === 'draft'">
            <AppButton variant="primary" size="sm" @click="$router.push(`/questions/${q!.id}/edit`)">编辑</AppButton>
            <AppButton variant="success" size="sm" :loading="submitting" @click="submitReview">提交审核</AppButton>
            <AppButton variant="danger" size="sm" @click="confirmDelete"><AppIcon name="trash" :size="17" /> 删除</AppButton>
          </template>
          <!-- 驳回 → 编辑 -->
          <template v-else-if="q?.status === 'rejected'">
            <AppButton variant="primary" size="sm" @click="$router.push(`/questions/${q!.id}/edit`)">重新编辑</AppButton>
          </template>
          <!-- 待审核 → 审核（仅组长） -->
          <template v-else-if="q?.status === 'pending' && q?.can_review">
            <AppButton variant="success" size="sm" @click="handleReview('approved')"><AppIcon name="check-circle" :size="17" /> 通过</AppButton>
            <AppButton variant="danger" size="sm" @click="handleReview('rejected')"><AppIcon name="x-circle" :size="17" /> 驳回</AppButton>
          </template>
          <!-- 已发布 → 停用（仅组长） -->
          <template v-else-if="q?.status === 'published' && auth.isAdmin">
            <AppButton variant="outline" size="sm" @click="toast.info('停用功能即将上线')"><AppIcon name="ban" :size="17" /> 停用</AppButton>
          </template>
        </div>
      </div>

      <div class="detail-layout">
        <!-- 主内容 -->
        <div class="detail-main">
          <!-- 状态标签 -->
          <div class="mb-12 flex items-center gap-2 flex-wrap">
            <AppBadge :color="statusBadgeColor(q?.status || '')"><AppIcon :name="statusIcon(q?.status || '')" :size="13" /> {{ statusLabel(q?.status || '') }}</AppBadge>
            <AppBadge :color="typeBadgeColor(q?.question_type || '')">{{ typeLabel(q?.question_type || '') }}</AppBadge>
            <span class="text-sm text-secondary">{{ diffLabel(q?.difficulty || '') }}</span>
            <span class="text-sm text-muted">{{ q?.default_score }}分</span>
            <span v-if="q?.grade" class="text-sm text-muted">· {{ q.grade }}</span>
            <span v-if="q?.semester" class="text-sm text-muted">{{ q.semester }}</span>
          </div>

          <!-- 题干 -->
          <AppCard :no-hover="true" class="mb-12">
            <template #header><span class="card-title"><AppIcon name="book-open" :size="20" /> 题干</span></template>
            <LatexRender :text="q?.stem || ''" />
          </AppCard>

          <!-- 选项（选择题） -->
          <AppCard v-if="q?.question_type === 'choice' && q?.options" :no-hover="true" class="mb-12">
            <template #header><span class="card-title"><AppIcon name="list" :size="20" /> 选项</span></template>
            <div
              v-for="opt in q!.options"
              :key="opt.label"
              class="option-row"
              :class="{ correct: isCorrect(opt.label) }"
            >
              <span class="option-label">{{ opt.label }}.</span>
              <LatexRender :text="opt.content" :inline="true" />
              <AppBadge v-if="isCorrect(opt.label)" color="green">正确答案</AppBadge>
            </div>
          </AppCard>

          <!-- 判断题 -->
          <AppCard v-else-if="q?.question_type === 'judgment'" :no-hover="true" class="mb-12">
            <template #header><span class="card-title"><AppIcon name="check-circle" :size="20" /> 答案</span></template>
            <AppBadge :color="q?.correct_answer?.[0] === true ? 'green' : 'red'">
              {{ q?.correct_answer?.[0] === true ? '正确' : '错误' }}
            </AppBadge>
          </AppCard>

          <!-- 填空题答案 -->
          <AppCard v-else-if="q?.question_type === 'fill' && q?.correct_answer" :no-hover="true" class="mb-12">
            <template #header><span class="card-title"><AppIcon name="file-text" :size="20" /> 参考答案</span></template>
            <div v-for="(item, i) in q!.correct_answer as any[]" :key="i" class="mb-8">
              <span class="text-sm text-secondary">第{{ i+1 }}空：</span>
              <LatexRender :text="item.answer || item" :inline="true" />
            </div>
          </AppCard>

          <!-- 解答题答案 -->
          <AppCard v-else-if="q?.question_type === 'solution' && q?.correct_answer" :no-hover="true" class="mb-12">
            <template #header><span class="card-title"><AppIcon name="file-text" :size="20" /> 参考答案</span></template>
            <LatexRender v-for="(ans, i) in q!.correct_answer as string[]" :key="i" :text="ans" />
          </AppCard>

          <!-- 解析 -->
          <AppCard v-if="q?.analysis" :no-hover="true" class="mb-12">
            <template #header><span class="card-title"><AppIcon name="lightbulb" :size="20" /> 解析</span></template>
            <LatexRender :text="q.analysis" />
          </AppCard>
        </div>

        <!-- 侧边栏 -->
        <div class="detail-side">
          <!-- 知识点 -->
          <AppCard :no-hover="true" class="mb-12">
            <template #header><span class="card-title"><AppIcon name="tag" :size="20" /> 知识点</span></template>
            <div v-if="q?.knowledge_points?.length" class="flex flex-wrap gap-4">
              <AppBadge v-for="kp in q!.knowledge_points" :key="kp.id" color="blue">{{ kp.name }}</AppBadge>
            </div>
            <div v-else class="text-sm text-muted">未关联知识点</div>
          </AppCard>

          <!-- 元信息 -->
          <AppCard :no-hover="true">
            <template #header><span class="card-title"><AppIcon name="info" :size="20" /> 元信息</span></template>
            <div class="meta-list">
              <div><span class="meta-label">创建者</span>{{ q?.creator_name || q?.creator_id?.substring(0, 8) || '—' }}</div>
              <div><span class="meta-label">版本</span>v{{ q?.version }}</div>
              <div><span class="meta-label">创建时间</span>{{ formatTime(q?.created_at) }}</div>
              <div><span class="meta-label">更新时间</span>{{ formatTime(q?.updated_at) }}</div>
              <div v-if="q?.source"><span class="meta-label">来源</span>{{ q.source }}</div>
            </div>
          </AppCard>
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
        <AppButton variant="primary" @click="confirmReject">确认驳回</AppButton>
      </div>
    </AppModal>

    <!-- 删除确认 -->
    <AppConfirm
      v-model="deleteDialog"
      title="确认删除"
      message="删除后不可恢复，确定要删除这道题吗？"
      confirm-text="删除"
      danger
      @confirm="doDelete"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { questionApi, type QuestionDetail } from '@/api/client'
import client from '@/api/client'
import { useAuthStore } from '@/stores/auth'
import LatexRender from '@/components/LatexRender.vue'
import { AppButton, AppCard, AppBadge, AppModal, AppConfirm, AppIcon } from '@/components/ui'
import { useToast } from '@/composables/useToast'
import { typeLabel, typeBadgeColor, diffLabel, statusLabel, statusBadgeColor, statusIcon, formatTime } from '@/utils/questionDisplay'

const route = useRoute()
const router = useRouter()
const auth = useAuthStore()
const toast = useToast()
const q = ref<QuestionDetail | null>(null)
const loading = ref(false)
const submitting = ref(false)
const rejectDialog = ref(false)
const rejectComment = ref('')
const deleteDialog = ref(false)

async function fetchDetail() {
  loading.value = true
  try {
    const res = await questionApi.get(route.params.id as string)
    q.value = res.data
  } catch { /* handled */ }
  finally { loading.value = false }
}

async function submitReview() {
  submitting.value = true
  try {
    await client.post(`/questions/${route.params.id}/submit`, {})
    toast.success('已提交审核')
    fetchDetail()
  } catch { /* handled */ }
  finally { submitting.value = false }
}

function confirmDelete() {
  deleteDialog.value = true
}

async function doDelete() {
  try {
    await client.delete(`/questions/${route.params.id}`)
    toast.success('已删除')
    router.push('/questions')
  } catch { /* handled */ }
}

function handleReview(action: string) {
  if (action === 'rejected') {
    rejectDialog.value = true
  } else {
    confirmReview(action)
  }
}

async function confirmReject() {
  const ok = await confirmReview('rejected', rejectComment.value)
  if (ok) {
    rejectComment.value = ''
    rejectDialog.value = false
  }
}

async function confirmReview(action: string, comment?: string): Promise<boolean> {
  try {
    await client.post(`/questions/${route.params.id}/review`, { action, comment })
    toast.success(action === 'approved' ? '已通过' : '已驳回')
    await fetchDetail()
    return true
  } catch (e: any) {
    toast.error(e.response?.data?.error || '操作失败')
    return false
  }
}

function isCorrect(label: string): boolean {
  if (!q.value?.correct_answer) return false
  const ans = q.value.correct_answer
  if (Array.isArray(ans)) return ans.includes(label)
  return ans === label
}

onMounted(fetchDetail)
</script>

<style scoped>
.loading-hint {
  text-align: center;
  padding: 48px 20px;
  color: var(--text-muted);
}

.detail-layout {
  display: grid;
  grid-template-columns: 3fr 1fr;
  gap: 20px;
}

@media (max-width: 899px) {
  .detail-layout {
    grid-template-columns: 1fr;
  }
}

.option-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 12px;
  margin-bottom: 8px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border-color);
}

.option-row.correct {
  border-color: var(--success);
  background: var(--success-light);
}

.option-label {
  font-family: monospace;
  font-weight: 600;
}

.meta-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
  font-size: 13px;
}

.meta-label {
  color: var(--text-muted);
  margin-right: 8px;
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
