<script setup lang="ts">
/**
 * V2.1.1 P1：标签候选审核页（管理员）
 *
 * 列表（pending/approved/rejected/merged 过滤）→ 详情（来源题干）→
 * 四分支审核：接受为新标签（new_node）/ 加为别名（alias）/ 并入已有标签（merge）/ 拒绝。
 */
import { ref, reactive, onMounted } from 'vue'
import { AppButton, AppModal, AppConfirm, AppIcon, AppEmpty } from '@/components/ui'
import { useToast } from '@/composables/useToast'
import {
  tagCandidateApi,
  knowledgeTreeApi,
  type TagCandidate,
  type TagCandidateDetail,
} from '@/api/client'
import KnowledgeTreeCascader from '@/components/KnowledgeTreeCascader.vue'

const toast = useToast()

const items = ref<TagCandidate[]>([])
const total = ref(0)
const page = ref(1)
const pageSize = 20
const loading = ref(false)
const statusFilter = ref('pending')

const STATUS_LABELS: Record<string, string> = {
  pending: '待审核',
  approved: '已通过',
  rejected: '已拒绝',
  merged: '已并入',
}
const KIND_LABELS: Record<string, string> = {
  chapter: '章节',
  knowledge: '知识点',
  method: '方法',
}

// 详情
const detail = ref<TagCandidateDetail | null>(null)
const showDetail = ref(false)
const detailLoading = ref(false)

// 审核弹窗
const reviewTarget = ref<TagCandidate | null>(null)
const showReviewDialog = ref(false)
const reviewing = ref(false)
const reviewAction = ref<'new_node' | 'alias' | 'merge'>('new_node')
const reviewForm = reactive({
  treeId: '',
  name: '',
  targetNodeIds: [] as string[],
  reason: '',
})

// 拒绝确认
const rejectTarget = ref<TagCandidate | null>(null)
const showRejectConfirm = ref(false)

const trees = ref<{ id: string; name: string }[]>([])

async function loadTrees() {
  try {
    const res = await knowledgeTreeApi.list()
    trees.value = res.data.map((t: any) => ({ id: t.id, name: t.name }))
    if (trees.value.length && !reviewForm.treeId) {
      reviewForm.treeId = trees.value[0].id
    }
  } catch { /* 忽略 */ }
}

async function loadList() {
  loading.value = true
  try {
    const { data } = await tagCandidateApi.list({
      status: statusFilter.value === 'ALL' ? undefined : statusFilter.value,
      page: page.value,
      page_size: pageSize,
    })
    items.value = data.items
    total.value = data.total
  } catch (e: any) {
    toast.error(e?.response?.data?.error ?? '加载候选失败')
  } finally {
    loading.value = false
  }
}

function switchStatus(s: string) {
  statusFilter.value = s
  page.value = 1
  loadList()
}

async function openDetail(c: TagCandidate) {
  detailLoading.value = true
  detail.value = null
  try {
    const { data } = await tagCandidateApi.get(c.id)
    detail.value = data
    showDetail.value = true
  } catch (e: any) {
    toast.error(e?.response?.data?.error ?? '加载详情失败')
  } finally {
    detailLoading.value = false
  }
}

function openReview(c: TagCandidate, action: 'new_node' | 'alias' | 'merge') {
  reviewTarget.value = c
  reviewAction.value = action
  reviewForm.name = c.raw_name
  reviewForm.reason = ''
  reviewForm.targetNodeIds = []
  showReviewDialog.value = true
}

async function doReview() {
  const c = reviewTarget.value
  if (!c) return
  if (reviewAction.value === 'new_node' && !reviewForm.treeId) {
    toast.warning('请选择目标知识树')
    return
  }
  if (reviewAction.value !== 'new_node' && reviewForm.targetNodeIds.length === 0) {
    toast.warning('请选择目标标签')
    return
  }
  reviewing.value = true
  try {
    await tagCandidateApi.approve(c.id, {
      action: reviewAction.value,
      tree_id: reviewAction.value === 'new_node' ? reviewForm.treeId : undefined,
      name: reviewAction.value === 'new_node' ? reviewForm.name.trim() || c.raw_name : undefined,
      target_node_id: reviewAction.value === 'new_node' ? undefined : reviewForm.targetNodeIds[0],
      reason: reviewForm.reason.trim() || undefined,
    })
    toast.success('已审核通过')
    showReviewDialog.value = false
    loadList()
    if (detail.value?.candidate.id === c.id) openDetail(c)
  } catch (e: any) {
    toast.error(e?.response?.data?.error ?? '审核失败')
  } finally {
    reviewing.value = false
  }
}

async function doReject() {
  const c = rejectTarget.value
  if (!c) return
  try {
    await tagCandidateApi.reject(c.id)
    toast.success('已拒绝')
    showRejectConfirm.value = false
    loadList()
    if (detail.value?.candidate.id === c.id) openDetail(c)
  } catch (e: any) {
    toast.error(e?.response?.data?.error ?? '操作失败')
  }
}

onMounted(() => {
  loadList()
  loadTrees()
})
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div>
        <h1 class="page-title">标签候选审核</h1>
        <p class="page-sub">AI 未匹配的标签进入候选队列，审核后成为正式标签 / 别名 / 并入已有标签</p>
      </div>
      <span class="pending-badge">待审核 {{ statusFilter === 'pending' ? total : '—' }}</span>
    </div>

    <!-- 状态过滤 -->
    <div class="status-tabs">
      <button
        v-for="(label, key) in { ALL: '全部', pending: '待审核', approved: '已通过', rejected: '已拒绝', merged: '已并入' }"
        :key="key"
        :class="{ active: statusFilter === key }"
        @click="switchStatus(key)"
      >{{ label }}</button>
    </div>

    <!-- 列表 -->
    <div v-if="loading" class="loading-hint">加载中…</div>
    <AppEmpty v-else-if="items.length === 0" title="暂无候选" description="AI 解析时未匹配的标签会出现在这里" />
    <div v-else class="candidate-list">
      <div v-for="c in items" :key="c.id" class="candidate-card" @click="openDetail(c)">
        <div class="candidate-main">
          <span class="candidate-kind">{{ KIND_LABELS[c.kind] ?? c.kind }}</span>
          <span class="candidate-name">{{ c.raw_name }}</span>
          <span
            class="candidate-status"
            :class="c.status"
          >{{ STATUS_LABELS[c.status] ?? c.status }}</span>
          <span v-if="c.ai_confidence" class="candidate-conf">
            置信 {{ (Number(c.ai_confidence) * 100).toFixed(0) }}%
          </span>
        </div>
        <div class="candidate-meta">
          <span>{{ c.normalized_name }}</span>
          <span>{{ new Date(c.created_at).toLocaleString() }}</span>
        </div>
      </div>
    </div>

    <!-- 详情 -->
    <AppModal v-model="showDetail" title="候选详情" size="lg">
      <template v-if="detail">
        <div class="detail-block">
          <div class="detail-row"><span class="detail-label">原始标签</span><strong>{{ detail.candidate.raw_name }}</strong></div>
          <div class="detail-row"><span class="detail-label">规范化</span>{{ detail.candidate.normalized_name }}</div>
          <div class="detail-row"><span class="detail-label">维度</span>{{ KIND_LABELS[detail.candidate.kind] ?? detail.candidate.kind }}</div>
          <div class="detail-row"><span class="detail-label">置信度</span>{{ detail.candidate.ai_confidence ?? '—' }}</div>
        </div>
        <div v-if="detail.source_stem" class="detail-block">
          <div class="detail-label">来源题目</div>
          <div class="source-stem">{{ detail.source_stem }}</div>
        </div>
        <div class="detail-actions">
          <AppButton variant="primary" size="sm" @click="openReview(detail.candidate, 'new_node')">接受为新标签</AppButton>
          <AppButton variant="primary" size="sm" @click="openReview(detail.candidate, 'alias')">加为已有标签别名</AppButton>
          <AppButton variant="primary" size="sm" @click="openReview(detail.candidate, 'merge')">并入已有标签</AppButton>
          <AppButton variant="danger" size="sm" @click="rejectTarget = detail.candidate; showRejectConfirm = true">拒绝</AppButton>
        </div>
      </template>
    </AppModal>

    <!-- 审核弹窗 -->
    <AppModal v-model="showReviewDialog" :title="reviewAction === 'new_node' ? '接受为新标签' : reviewAction === 'alias' ? '加为已有标签别名' : '并入已有标签'">
      <div class="review-form">
        <template v-if="reviewAction === 'new_node'">
          <div class="form-group">
            <label class="form-label">目标知识树</label>
            <select v-model="reviewForm.treeId" class="form-input">
              <option v-for="t in trees" :key="t.id" :value="t.id">{{ t.name }}</option>
            </select>
          </div>
          <div class="form-group">
            <label class="form-label">标签名称</label>
            <input v-model="reviewForm.name" class="form-input" />
          </div>
        </template>
        <template v-else>
          <div class="form-group">
            <label class="form-label">目标标签（单选）</label>
            <KnowledgeTreeCascader v-model="reviewForm.targetNodeIds" :max="1" placeholder="选择目标标签…" />
          </div>
        </template>
        <div class="form-group">
          <label class="form-label">审核备注</label>
          <input v-model="reviewForm.reason" class="form-input" placeholder="选填" />
        </div>
        <div class="form-actions">
          <AppButton variant="ghost" @click="showReviewDialog = false">取消</AppButton>
          <AppButton variant="primary" :loading="reviewing" @click="doReview">确认</AppButton>
        </div>
      </div>
    </AppModal>

    <!-- 拒绝确认 -->
    <AppConfirm
      v-model="showRejectConfirm"
      title="拒绝该候选"
      :message="`确定拒绝候选「${rejectTarget?.raw_name}」吗？拒绝后该标签不会进入正式标签体系。`"
      confirm-text="拒绝"
      danger
      @confirm="doReject"
    />

    <!-- 分页 -->
    <div v-if="total > pageSize" class="pagination">
      <AppButton variant="ghost" size="sm" :disabled="page <= 1" @click="page--; loadList()">上一页</AppButton>
      <span>{{ page }} / {{ Math.ceil(total / pageSize) }}</span>
      <AppButton variant="ghost" size="sm" :disabled="page * pageSize >= total" @click="page++; loadList()">下一页</AppButton>
    </div>
  </div>
</template>

<style scoped>
.page { display: flex; flex-direction: column; gap: 14px; padding: 4px 0 40px; }
.page-head { display: flex; align-items: flex-start; justify-content: space-between; }
.page-title { font-size: 20px; font-weight: 700; margin: 0; }
.page-sub { font-size: 13px; color: var(--text-secondary); margin: 4px 0 0; }
.pending-badge {
  font-size: 12px; font-weight: 600; color: var(--warning);
  background: var(--warning-light); padding: 4px 12px; border-radius: 9999px;
}
.status-tabs { display: flex; gap: 6px; flex-wrap: wrap; }
.status-tabs button {
  padding: 6px 14px; font-size: 13px; border: 1px solid var(--border);
  border-radius: 9999px; background: var(--bg-input); color: var(--text-secondary); cursor: pointer;
}
.status-tabs button.active { border-color: var(--accent); color: var(--accent); font-weight: 600; }
.loading-hint { font-size: 13px; color: var(--text-secondary); }
.candidate-list { display: flex; flex-direction: column; gap: 8px; }
.candidate-card {
  border: 1px solid var(--border); border-radius: 10px; padding: 10px 12px;
  background: var(--bg-card, var(--bg-input)); cursor: pointer; transition: all 0.15s;
}
.candidate-card:hover { border-color: var(--accent); }
.candidate-main { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
.candidate-kind {
  font-size: 11px; padding: 1px 7px; border-radius: 6px;
  background: var(--purple-light); color: var(--purple); font-weight: 600;
}
.candidate-name { font-size: 15px; font-weight: 600; }
.candidate-status { font-size: 11px; padding: 1px 8px; border-radius: 10px; }
.candidate-status.pending { color: var(--warning); background: var(--warning-light); }
.candidate-status.approved { color: var(--success); background: var(--success-light, rgba(52,199,89,.12)); }
.candidate-status.rejected { color: var(--danger); background: var(--danger-light); }
.candidate-status.merged { color: var(--text-secondary); background: var(--bg-input); }
.candidate-conf { font-size: 12px; color: var(--text-secondary); }
.candidate-meta { display: flex; justify-content: space-between; font-size: 12px; color: var(--text-secondary); margin-top: 6px; }

.detail-block { border: 1px solid var(--border); border-radius: 8px; padding: 10px 12px; margin-bottom: 10px; }
.detail-row { display: flex; gap: 10px; font-size: 13px; padding: 3px 0; }
.detail-label { width: 76px; flex-shrink: 0; color: var(--text-secondary); font-size: 12px; }
.source-stem { font-size: 13px; color: var(--text-primary); margin-top: 6px; line-height: 1.6; }
.detail-actions { display: flex; gap: 8px; flex-wrap: wrap; }
.review-form { display: flex; flex-direction: column; gap: 10px; }
.form-group { display: flex; flex-direction: column; gap: 4px; }
.form-label { font-size: 12px; font-weight: 600; color: var(--text-secondary); }
.form-input {
  width: 100%; padding: 8px 10px; border: 1px solid var(--border); border-radius: 8px;
  font-size: 13px; background: var(--bg-input); color: var(--text-primary); outline: none;
}
.form-input:focus { border-color: var(--accent); }
.form-actions { display: flex; justify-content: flex-end; gap: 8px; }
.pagination { display: flex; align-items: center; justify-content: center; gap: 12px; font-size: 13px; color: var(--text-secondary); }
</style>
