<template>
  <div class="plr-page">
    <!-- ===== 顶部标题栏 ===== -->
    <header class="plr-header">
      <div class="plr-header-left">
        <h1 class="page-title">
          <AppIcon name="upload" :size="24" /> 推库审批
        </h1>
        <span class="plr-subtitle">各空间已通过内部审核、申请推送到公共题库的题目</span>
      </div>
      <div class="plr-header-right">
        <AppButton variant="ghost" size="sm" @click="fetchList" :loading="loading">
          <AppIcon name="history" :size="14" /> 刷新
        </AppButton>
        <AppBadge v-if="!loading" :color="list.length > 0 ? 'yellow' : 'gray'">
          待审批: {{ list.length }} 条
        </AppBadge>
      </div>
    </header>

    <!-- ===== 加载中 ===== -->
    <div v-if="loading" class="loading-hint">加载中…</div>

    <!-- ===== 空状态 ===== -->
    <AppEmpty
      v-else-if="list.length === 0"
      icon="check-circle"
      description="暂无待审批的推库申请"
    />

    <!-- ===== 申请列表 ===== -->
    <div v-else class="plr-list">
      <div v-for="row in list" :key="row.id" class="plr-card">
        <!-- 卡片头部：来源信息 -->
        <div class="plr-card-header">
          <div class="plr-source-line">
            <AppBadge color="blue">
              <AppIcon name="folder" :size="12" /> 来源空间
              <span class="plr-source-name">{{ row.source_space_name }}</span>
            </AppBadge>
            <AppBadge color="purple">
              <AppIcon name="user" :size="12" /> 申请人
              <span class="plr-source-name">{{ row.submitter_name }}</span>
            </AppBadge>
            <span class="plr-submit-time">
              <AppIcon name="clock" :size="12" />
              {{ formatTime(row.created_at) }} 提交
            </span>
          </div>
        </div>

        <!-- 题目摘要区（可点击跳转详情） -->
        <div class="plr-card-body" @click="goQuestionDetail(row.question_id)">
          <div class="plr-q-meta">
            <AppBadge :color="typeBadgeColor(row.question_type)">
              {{ typeLabel(row.question_type) }}
            </AppBadge>
            <AppBadge :color="diffBadgeColor(row.difficulty)">
              {{ diffLabel(row.difficulty) }}
            </AppBadge>
            <AppIcon name="arrow-right" :size="12" class="plr-link-icon" />
          </div>
          <div class="plr-q-stem line-clamp-2">
            <LatexRender :text="row.stem" :inline="true" />
          </div>
        </div>

        <!-- 卡片底部：操作按钮 -->
        <div class="plr-card-footer">
          <AppButton
            variant="primary"
            size="sm"
            :loading="reviewing === row.id && lastAction === 'approved'"
            :disabled="reviewing === row.id"
            @click="handleApprove(row)"
          >
            <AppIcon name="check-circle" :size="15" /> 通过并推送
          </AppButton>
          <AppButton
            variant="danger"
            size="sm"
            :loading="reviewing === row.id && lastAction === 'rejected'"
            :disabled="reviewing === row.id"
            @click="openRejectDialog(row)"
          >
            <AppIcon name="x-circle" :size="15" /> 驳回
          </AppButton>
        </div>
      </div>
    </div>

    <!-- ===== 驳回弹窗 ===== -->
    <AppModal v-model="rejectDialog" title="驳回推库申请">
      <div class="reject-dialog-body">
        <p class="reject-hint">
          驳回后申请人可修改题目后重新申请推送，请填写驳回原因：
        </p>
        <textarea
          v-model="rejectComment"
          class="reject-textarea"
          rows="4"
          placeholder="请输入驳回原因（必填）..."
        />
      </div>
      <div class="reject-dialog-actions">
        <AppButton variant="ghost" @click="cancelReject">取消</AppButton>
        <AppButton
          variant="danger"
          :loading="rejecting"
          :disabled="!rejectComment.trim()"
          @click="confirmReject"
        >
          确认驳回
        </AppButton>
      </div>
    </AppModal>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import {
  publicLibraryApi,
  type PublicLibrarySubmission,
} from '@/api/client'
import { AppButton, AppBadge, AppEmpty, AppModal, AppIcon } from '@/components/ui'
import LatexRender from '@/components/LatexRender.vue'
import { useToast } from '@/composables/useToast'
import { useAuthStore } from '@/stores/auth'
import {
  typeLabel,
  typeBadgeColor,
  diffLabel,
  diffBadgeColor,
  formatTime,
} from '@/utils/questionDisplay'

const router = useRouter()
const toast = useToast()
const auth = useAuthStore()

const list = ref<PublicLibrarySubmission[]>([])
const loading = ref(true)
const reviewing = ref<string | null>(null)
// 修正 3：防重复提交 —— 记录最后一次动作，用于按钮 loading 显示
const lastAction = ref<'approved' | 'rejected' | null>(null)

// 驳回弹窗
const rejectDialog = ref(false)
const rejectComment = ref('')
const rejecting = ref(false)
const currentRow = ref<PublicLibrarySubmission | null>(null)

async function fetchList() {
  loading.value = true
  try {
    const res = await publicLibraryApi.listPending()
    list.value = res.data
  } catch (e: any) {
    toast.error(e.response?.data?.error || e.message || '加载失败')
  } finally {
    loading.value = false
  }
}

function goQuestionDetail(questionId: string) {
  router.push(`/questions/${questionId}`)
}

// ===== 通过：直接调用，立即生效 =====
async function handleApprove(row: PublicLibrarySubmission) {
  // 防连点：已处于 loading 则忽略
  if (reviewing.value === row.id) return
  reviewing.value = row.id
  lastAction.value = 'approved'
  try {
    await publicLibraryApi.review(row.id, 'approved')
    toast.success('已通过，题目已推送到公共题库')
    list.value = list.value.filter(item => item.id !== row.id)
  } catch (e: any) {
    toast.error(e.response?.data?.error || e.message || '操作失败')
  } finally {
    reviewing.value = null
    lastAction.value = null
  }
}

// ===== 驳回：弹窗收集原因 =====
function openRejectDialog(row: PublicLibrarySubmission) {
  if (reviewing.value === row.id) return
  currentRow.value = row
  rejectComment.value = ''
  rejectDialog.value = true
}

function cancelReject() {
  rejectDialog.value = false
  rejectComment.value = ''
  currentRow.value = null
}

async function confirmReject() {
  if (!currentRow.value || !rejectComment.value.trim()) return
  // 防重复提交：点击后立即禁用
  if (rejecting.value) return
  rejecting.value = true
  reviewing.value = currentRow.value.id
  lastAction.value = 'rejected'
  try {
    await publicLibraryApi.review(
      currentRow.value.id,
      'rejected',
      rejectComment.value.trim(),
    )
    toast.success('已驳回推库申请')
    list.value = list.value.filter(item => item.id !== currentRow.value!.id)
    rejectDialog.value = false
    rejectComment.value = ''
    currentRow.value = null
  } catch (e: any) {
    toast.error(e.response?.data?.error || e.message || '操作失败')
  } finally {
    rejecting.value = false
    reviewing.value = null
    lastAction.value = null
  }
}

onMounted(() => {
  // 兜底：路由已 meta 拦截，但避免任何情况下越权访问数据
  if (!auth.isAdminUnified) {
    toast.error('仅超级管理员可访问推库审批')
    router.replace('/dashboard')
    return
  }
  fetchList()
})
</script>

<style scoped>
.plr-page {
  padding: 20px 24px;
  height: 100%;
  overflow-y: auto;
}

/* ===== 顶部标题栏 ===== */
.plr-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 20px;
}

.plr-header-left {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.plr-header-left .page-title {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0;
}

.plr-subtitle {
  font-size: 12px;
  color: var(--text-muted);
}

.plr-header-right {
  display: flex;
  align-items: center;
  gap: 10px;
}

/* ===== 加载提示 ===== */
.loading-hint {
  text-align: center;
  padding: 48px 20px;
  color: var(--text-muted);
}

/* ===== 申请卡片列表 ===== */
.plr-list {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.plr-card {
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  padding: 16px 18px;
  transition: var(--transition-fast);
}

.plr-card:hover {
  border-color: var(--accent);
  box-shadow: var(--shadow-sm);
}

/* 卡片头部 */
.plr-card-header {
  margin-bottom: 12px;
  padding-bottom: 12px;
  border-bottom: 1px dashed var(--border-color);
}

.plr-source-line {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 10px;
}

.plr-source-name {
  font-weight: 600;
  margin-left: 4px;
}

.plr-submit-time {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  color: var(--text-muted);
  margin-left: auto;
}

/* 卡片主体 - 题目摘要 */
.plr-card-body {
  cursor: pointer;
  border-radius: var(--radius-sm);
  padding: 4px 6px;
  margin: 0 -6px;
  transition: var(--transition-fast);
}

.plr-card-body:hover {
  background: var(--bg-hover);
}

.plr-q-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
}

.plr-link-icon {
  margin-left: auto;
  color: var(--text-muted);
}

.plr-q-stem {
  font-size: 14px;
  line-height: 1.6;
  color: var(--text-primary);
}

.line-clamp-2 {
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

/* 卡片底部操作 */
.plr-card-footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  margin-top: 14px;
  padding-top: 12px;
  border-top: 1px solid var(--border-color);
}

/* ===== 驳回弹窗 ===== */
.reject-dialog-body {
  min-width: 360px;
}

.reject-hint {
  font-size: 13px;
  color: var(--text-secondary);
  margin-bottom: 12px;
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

.reject-dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 16px;
}

/* ===== 响应式 ===== */
@media (max-width: 768px) {
  .plr-page {
    padding: 16px;
  }

  .plr-header {
    flex-direction: column;
    align-items: stretch;
  }

  .plr-header-right {
    justify-content: space-between;
  }

  .plr-card {
    padding: 14px;
  }

  .plr-card-footer {
    flex-direction: column;
  }

  .plr-card-footer :deep(button) {
    width: 100%;
  }
}
</style>
