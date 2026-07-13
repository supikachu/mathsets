<template>
  <div class="detail-page">
    <div v-if="loading" class="loading-hint">加载中…</div>

    <template v-else>
      <!-- 头部导航 -->
      <div class="detail-header">
        <div class="header-left">
          <AppButton variant="ghost" size="sm" @click="$router.push('/questions')"><AppIcon name="chevron-left" :size="17" /> 返回列表</AppButton>
          <h1 class="page-title">题目详情</h1>
        </div>
        <div class="flex gap-2">
          <template v-if="q?.status === 'draft'">
            <AppButton variant="primary" size="sm" @click="$router.push(`/questions/${q!.id}/edit`)">编辑</AppButton>
            <AppButton variant="success" size="sm" :loading="submitting" @click="submitReview">提交审核</AppButton>
            <AppButton variant="danger" size="sm" @click="confirmDelete"><AppIcon name="trash" :size="17" /> 删除</AppButton>
          </template>
          <template v-else-if="q?.status === 'rejected'">
            <AppButton variant="primary" size="sm" @click="$router.push(`/questions/${q!.id}/edit`)">重新编辑</AppButton>
          </template>
          <template v-else-if="q?.status === 'pending' && q?.can_review">
            <AppButton variant="success" size="sm" @click="handleReview('approved')"><AppIcon name="check-circle" :size="17" /> 通过</AppButton>
            <AppButton variant="danger" size="sm" @click="handleReview('rejected')"><AppIcon name="x-circle" :size="17" /> 驳回</AppButton>
          </template>
          <template v-else-if="q?.status === 'published' && auth.isAdmin">
            <AppButton variant="outline" size="sm" @click="toast.info('停用功能即将上线')"><AppIcon name="ban" :size="17" /> 停用</AppButton>
          </template>
        </div>
      </div>

      <div class="detail-layout">
        <!-- 主内容区 -->
        <div class="detail-main">
          <!-- 题目卡片 -->
          <div class="question-card">
            <!-- 题目信息条 -->
            <div class="q-info-bar">
              <AppBadge :color="statusBadgeColor(q?.status || '')"><AppIcon :name="statusIcon(q?.status || '')" :size="13" /> {{ statusLabel(q?.status || '') }}</AppBadge>
              <AppBadge :color="typeBadgeColor(q?.question_type || '')">{{ typeLabel(q?.question_type || '') }}</AppBadge>
              <span class="q-info-item"><AppIcon name="star" :size="13" /> {{ diffLabel(q?.difficulty || '') }}</span>
              <span class="q-info-item">{{ q?.default_score }}分</span>
              <span v-if="q?.grade" class="q-info-item">{{ q.grade }}</span>
              <span v-if="q?.semester" class="q-info-item">{{ q.semester }}</span>
              <span v-if="q?.source" class="q-info-item">{{ q.source }}</span>
            </div>

            <!-- 题干 -->
            <div class="q-stem">
              <LatexRender :text="q?.stem || ''" />
            </div>

            <!-- 选项（选择题） -->
            <div v-if="q?.question_type === 'choice' && optionList.length" class="q-options">
              <div
                v-for="opt in optionList"
                :key="opt.label"
                class="q-opt"
                :class="{ correct: isCorrect(opt.label) }"
              >
                <span class="q-opt-letter">{{ opt.label }}</span>
                <LatexRender :text="opt.content" :inline="true" />
                <AppIcon v-if="isCorrect(opt.label)" name="check-circle" :size="16" class="q-opt-check" />
              </div>
            </div>

            <!-- 判断题 -->
            <div v-else-if="q?.question_type === 'judgment'" class="q-answer-inline">
              <span class="q-answer-tag" :class="q?.correct_answer?.[0] === true ? 'tag-correct' : 'tag-wrong'">
                {{ q?.correct_answer?.[0] === true ? '正确' : '错误' }}
              </span>
            </div>

            <!-- 填空题答案 -->
            <div v-else-if="q?.question_type === 'fill' && q?.correct_answer" class="q-blanks">
              <div v-for="(item, i) in (q!.correct_answer as any[])" :key="i" class="q-blank">
                <span class="q-blank-num">{{ i + 1 }}</span>
                <LatexRender :text="item.answer || String(item)" :inline="true" />
              </div>
            </div>

            <!-- 解答题答案 -->
            <div v-else-if="q?.question_type === 'solution' && q?.correct_answer" class="q-solution-answer">
              <LatexRender v-for="(ans, i) in (q!.correct_answer as string[])" :key="i" :text="ans" />
            </div>
          </div>

          <!-- 参考答案 -->
          <div v-if="q?.question_type === 'choice' && optionList.length" class="answer-block">
            <div class="block-title"><AppIcon name="check-circle" :size="18" /> 参考答案</div>
            <div class="answer-content">
              <span class="answer-letter" v-for="a in correctLabels" :key="a">{{ a }}</span>
            </div>
          </div>

          <!-- 解析 -->
          <div v-if="q?.analysis" class="analysis-block">
            <div class="block-title"><AppIcon name="lightbulb" :size="18" /> 解析</div>
            <div class="analysis-content">
              <LatexRender :text="q.analysis" />
            </div>
          </div>

          <!-- 评分标准（解答题） -->
          <div v-if="q?.grading_criteria && Array.isArray(q.grading_criteria) && q.grading_criteria.length" class="grading-block">
            <div class="block-title"><AppIcon name="list" :size="18" /> 评分标准</div>
            <div class="grading-list">
              <div v-for="(step, i) in (q.grading_criteria as any[])" :key="i" class="grading-step">
                <span class="grading-step-label">{{ step.label || `步骤${i + 1}` }}</span>
                <span class="grading-step-score">{{ step.score || 0 }}分</span>
                <span v-if="step.desc" class="grading-step-desc">{{ step.desc }}</span>
              </div>
            </div>
          </div>
        </div>

        <!-- 侧边栏 -->
        <div class="detail-side">
          <!-- 知识点 -->
          <div class="side-card">
            <div class="side-title"><AppIcon name="tag" :size="16" /> 知识点</div>
            <div v-if="q?.knowledge_points?.length" class="kp-tags">
              <span v-for="kp in q!.knowledge_points" :key="kp.id" class="kp-tag">{{ kp.name }}</span>
            </div>
            <div v-else class="side-empty">未关联知识点</div>
          </div>

          <!-- 元信息 -->
          <div class="side-card">
            <div class="side-title"><AppIcon name="info" :size="16" /> 元信息</div>
            <div class="meta-list">
              <div class="meta-row"><span class="meta-label">创建者</span><span class="meta-val">{{ q?.creator_name || q?.creator_id?.substring(0, 8) || '—' }}</span></div>
              <div class="meta-row"><span class="meta-label">版本</span><span class="meta-val">v{{ q?.version }}</span></div>
              <div class="meta-row"><span class="meta-label">创建</span><span class="meta-val">{{ formatTime(q?.created_at) }}</span></div>
              <div class="meta-row"><span class="meta-label">更新</span><span class="meta-val">{{ formatTime(q?.updated_at) }}</span></div>
            </div>
          </div>
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
import { ref, computed, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { questionApi, type QuestionDetail } from '@/api/client'
import client from '@/api/client'
import { useAuthStore } from '@/stores/auth'
import LatexRender from '@/components/LatexRender.vue'
import { AppButton, AppBadge, AppModal, AppConfirm, AppIcon } from '@/components/ui'
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

// 安全提取选项列表（兼容数组/对象/字符串）
const optionList = computed(() => {
  const opts = q.value?.options
  if (!opts) return []
  if (Array.isArray(opts)) return opts as { label: string; content: string }[]
  return []
})

// 正确答案标签列表
const correctLabels = computed(() => {
  const ans = q.value?.correct_answer
  if (!ans) return []
  if (Array.isArray(ans)) return ans.map(String)
  return [String(ans)]
})

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
  const ans = q.value?.correct_answer
  if (!ans) return false
  if (Array.isArray(ans)) return ans.map(String).includes(label)
  return String(ans) === label
}

onMounted(fetchDetail)
</script>

<style scoped>
.detail-page {
  padding: 20px 24px;
  height: 100%;
  overflow-y: auto;
  box-sizing: border-box;
}

.loading-hint {
  text-align: center;
  padding: 48px 20px;
  color: var(--text-muted);
}

/* 头部 */
.detail-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 20px;
  flex-wrap: wrap;
  gap: 12px;
}

.header-left {
  display: flex;
  align-items: center;
  gap: 12px;
}

.page-title {
  font-size: 20px;
  font-weight: 700;
  color: var(--text-primary);
  margin: 0;
}

/* 布局 */
.detail-layout {
  display: grid;
  grid-template-columns: 1fr 280px;
  gap: 20px;
  align-items: start;
}

@media (max-width: 1024px) {
  .detail-layout {
    grid-template-columns: 1fr;
  }
}

/* 题目卡片 */
.question-card {
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: 10px;
  padding: 24px 28px;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.04);
}

.q-info-bar {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  padding-bottom: 16px;
  margin-bottom: 20px;
  border-bottom: 1px solid var(--border-light, rgba(0, 0, 0, 0.06));
}

.q-info-item {
  font-size: 13px;
  color: var(--text-muted);
  display: inline-flex;
  align-items: center;
  gap: 3px;
}

.q-stem {
  font-size: 15px;
  line-height: 1.8;
  color: var(--text-primary);
  margin-bottom: 20px;
}

.q-stem :deep(p) {
  margin: 0 0 8px;
}

/* 选项 */
.q-options {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin-top: 8px;
}

.q-opt {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 12px 16px;
  border: 1px solid var(--border-color);
  border-radius: 8px;
  font-size: 14px;
  line-height: 1.6;
  color: var(--text-primary);
  transition: border-color 0.2s, background 0.2s;
}

.q-opt.correct {
  border-color: var(--success);
  background: var(--success-light);
}

.q-opt-letter {
  flex-shrink: 0;
  width: 24px;
  height: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  background: var(--bg-input);
  font-weight: 700;
  font-size: 13px;
  color: var(--text-secondary);
}

.q-opt.correct .q-opt-letter {
  background: var(--success);
  color: #fff;
}

.q-opt-check {
  margin-left: auto;
  color: var(--success);
  flex-shrink: 0;
}

/* 判断题 */
.q-answer-inline {
  margin-top: 12px;
}

.q-answer-tag {
  display: inline-block;
  padding: 4px 16px;
  border-radius: 6px;
  font-weight: 600;
  font-size: 14px;
}

.tag-correct {
  background: var(--success-light);
  color: var(--success);
}

.tag-wrong {
  background: var(--danger-light, rgba(239, 68, 68, 0.1));
  color: var(--danger, #ef4444);
}

/* 填空题 */
.q-blanks {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 12px;
}

.q-blank {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  background: var(--bg-input);
  border-radius: 6px;
  font-size: 14px;
}

.q-blank-num {
  flex-shrink: 0;
  width: 20px;
  height: 20px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  background: var(--accent);
  color: #fff;
  font-size: 11px;
  font-weight: 700;
}

/* 解答题答案 */
.q-solution-answer {
  margin-top: 12px;
  padding: 12px 16px;
  background: var(--bg-input);
  border-radius: 8px;
  font-size: 14px;
  line-height: 1.7;
}

/* 参考答案块 */
.answer-block {
  margin-top: 16px;
  padding: 16px 20px;
  background: var(--success-light);
  border: 1px solid var(--success);
  border-radius: 8px;
}

.block-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 14px;
  font-weight: 700;
  color: var(--text-primary);
  margin-bottom: 10px;
}

.answer-content {
  display: flex;
  gap: 8px;
}

.answer-letter {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  border-radius: 50%;
  background: var(--success);
  color: #fff;
  font-weight: 700;
  font-size: 16px;
}

/* 解析块 */
.analysis-block {
  margin-top: 16px;
  padding: 20px 24px;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: 10px;
}

.analysis-content {
  font-size: 14px;
  line-height: 1.8;
  color: var(--text-primary);
}

.analysis-content :deep(p) {
  margin: 0 0 8px;
}

/* 评分标准 */
.grading-block {
  margin-top: 16px;
  padding: 20px 24px;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: 10px;
}

.grading-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.grading-step {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 12px;
  background: var(--bg-input);
  border-radius: 6px;
  font-size: 13px;
}

.grading-step-label {
  font-weight: 600;
  color: var(--text-primary);
}

.grading-step-score {
  color: var(--accent);
  font-weight: 600;
}

.grading-step-desc {
  color: var(--text-muted);
}

/* 侧边栏 */
.detail-side {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.side-card {
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: 10px;
  padding: 16px 18px;
}

.side-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 14px;
  font-weight: 700;
  color: var(--text-primary);
  margin-bottom: 12px;
  padding-bottom: 10px;
  border-bottom: 1px solid var(--border-color);
}

.kp-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.kp-tag {
  display: inline-block;
  padding: 3px 10px;
  background: var(--accent-light);
  color: var(--accent);
  border-radius: 12px;
  font-size: 12px;
  font-weight: 500;
}

.side-empty {
  font-size: 13px;
  color: var(--text-muted);
}

.meta-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.meta-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: 13px;
}

.meta-label {
  color: var(--text-muted);
}

.meta-val {
  color: var(--text-primary);
  font-weight: 500;
}

/* 弹窗 */
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
  box-sizing: border-box;
}

.reject-textarea:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
}
</style>
