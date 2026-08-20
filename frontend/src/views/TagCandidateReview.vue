<script setup lang="ts">
/**
 * 标签候选审核页（管理员）
 *
 * knowledge_node（章节 / 知识点 / 题型专题）走知识树；tag（通用方法 / 核心素养）走扁平标签。
 */
import { ref, reactive, computed, onMounted } from 'vue'
import { AppButton, AppModal, AppEmpty } from '@/components/ui'
import { useToast } from '@/composables/useToast'
import {
  tagCandidateApi,
  knowledgeTreeApi,
  tagsApi,
  type TagCandidate,
  type TagCandidateDetail,
  type KnowledgeTree,
  type KnowledgeTreeKind,
  type Tag,
  type TagCategory,
} from '@/api/client'
import KnowledgeTreeCascader from '@/components/KnowledgeTreeCascader.vue'
import LatexRender from '@/components/LatexRender.vue'

const toast = useToast()

const items = ref<TagCandidate[]>([])
const total = ref(0)
const page = ref(1)
const pageSize = 20
const loading = ref(false)
const statusFilter = ref('pending')
const kindFilter = ref('ALL')

const STATUS_LABELS: Record<string, string> = {
  pending: '待审核',
  approved: '已通过',
  rejected: '已拒绝',
  merged: '已并入',
}
const KIND_LABELS: Record<string, string> = {
  chapter: '章节',
  knowledge: '知识点',
  method: '通用方法',
  pattern: '题型专题',
  core_competence: '核心素养',
}
const TARGET_TYPE_LABELS: Record<string, string> = {
  knowledge_node: '知识节点',
  tag: '扁平标签',
}

const KIND_TABS: { key: string; label: string }[] = [
  { key: 'ALL', label: '全部维度' },
  { key: 'chapter', label: '章节' },
  { key: 'knowledge', label: '知识点' },
  { key: 'pattern', label: '题型专题' },
  { key: 'method', label: '通用方法' },
  { key: 'core_competence', label: '核心素养' },
]

function isTagKind(kind: string) {
  return kind === 'method' || kind === 'core_competence'
}

function treeKindFor(kind: string): KnowledgeTreeKind {
  if (kind === 'chapter') return 'chapter'
  if (kind === 'pattern') return 'ability'
  return 'knowledge'
}

function tagCategoryFor(kind: string): TagCategory {
  return kind === 'core_competence' ? 'core_competence' : 'method'
}

const detail = ref<TagCandidateDetail | null>(null)
const showDetail = ref(false)
const detailLoading = ref(false)

/** 来源题目题干（兼容旧字段 source_stem） */
const detailStem = computed(() =>
  detail.value?.source_question?.stem ?? detail.value?.source_stem ?? '',
)

/** 安全提取选择题选项（兼容数组 / 对象 / JSON 字符串） */
const detailOptionList = computed(() => {
  const q = detail.value?.source_question
  if (!q || q.question_type !== 'choice') return []
  let parsed: unknown = q.options
  if (!parsed) return []
  if (typeof parsed === 'string') {
    try {
      parsed = JSON.parse(parsed)
    } catch {
      return []
    }
  }
  if (!Array.isArray(parsed)) return []
  return parsed.map((opt: unknown) => {
    if (typeof opt === 'string') {
      const match = opt.match(/^([A-Z])[.、．]\s*(.*)$/)
      if (match) return { label: match[1], content: match[2] }
      return { label: '', content: opt }
    }
    if (opt && typeof opt === 'object' && 'label' in opt) {
      const o = opt as { label: string; content?: string }
      return { label: o.label, content: o.content || '' }
    }
    return { label: '', content: String(opt) }
  })
})

const reviewTarget = ref<TagCandidate | null>(null)
const showReviewDialog = ref(false)
const reviewing = ref(false)
const reviewAction = ref<'new_node' | 'alias' | 'merge'>('new_node')
const reviewForm = reactive({
  treeId: '',
  parentNodeIds: [] as string[],
  name: '',
  targetNodeIds: [] as string[],
  targetTagId: '',
  reason: '',
})

const rejectTarget = ref<TagCandidate | null>(null)
const showRejectDialog = ref(false)
const rejectReason = ref('')
const rejecting = ref(false)

const trees = ref<KnowledgeTree[]>([])
const tagOptions = ref<Tag[]>([])

const reviewIsTag = computed(() => isTagKind(reviewTarget.value?.kind ?? ''))
const reviewTreeKind = computed(() => treeKindFor(reviewTarget.value?.kind ?? 'knowledge'))
const filteredTrees = computed(() => {
  const k = reviewTreeKind.value
  return trees.value.filter((t) => t.kind === k)
})

const reviewTitle = computed(() => {
  if (reviewAction.value === 'new_node') {
    return reviewIsTag.value ? '接受为新标签' : '接受为新节点'
  }
  if (reviewAction.value === 'alias') return '加为已有别名'
  return '并入已有（合并）'
})

async function loadTrees() {
  try {
    const res = await knowledgeTreeApi.list()
    trees.value = res.data
  } catch { /* 忽略 */ }
}

async function loadTags(kind: string) {
  try {
    const res = await tagsApi.list({ category: tagCategoryFor(kind) })
    tagOptions.value = res.data
  } catch {
    tagOptions.value = []
  }
}

async function loadList() {
  loading.value = true
  try {
    const { data } = await tagCandidateApi.list({
      status: statusFilter.value === 'ALL' ? undefined : statusFilter.value,
      kind: kindFilter.value === 'ALL' ? undefined : kindFilter.value,
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

function switchKind(k: string) {
  kindFilter.value = k
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

function pickDefaultTree(kind: string) {
  const k = treeKindFor(kind)
  const match = trees.value.find((t) => t.kind === k)
  return match?.id ?? ''
}

async function openReview(c: TagCandidate, action: 'new_node' | 'alias' | 'merge') {
  reviewTarget.value = c
  reviewAction.value = action
  reviewForm.name = c.raw_name
  reviewForm.reason = ''
  reviewForm.targetNodeIds = []
  reviewForm.parentNodeIds = []
  reviewForm.targetTagId = c.suggested_tag_id || ''
  reviewForm.targetNodeIds = c.suggested_node_id ? [c.suggested_node_id] : []
  reviewForm.treeId = pickDefaultTree(c.kind)
  if (isTagKind(c.kind) && action !== 'new_node') {
    await loadTags(c.kind)
  }
  showReviewDialog.value = true
}

function onTreeChange() {
  reviewForm.parentNodeIds = []
}

async function doReview() {
  const c = reviewTarget.value
  if (!c) return
  const tagMode = isTagKind(c.kind)
  if (!tagMode && reviewAction.value === 'new_node' && !reviewForm.treeId) {
    toast.warning('请选择目标知识树')
    return
  }
  if (reviewAction.value !== 'new_node') {
    if (tagMode && !reviewForm.targetTagId) {
      toast.warning('请选择目标标签')
      return
    }
    if (!tagMode && reviewForm.targetNodeIds.length === 0) {
      toast.warning('请选择目标节点')
      return
    }
  }
  reviewing.value = true
  try {
    await tagCandidateApi.approve(c.id, {
      action: reviewAction.value,
      tree_id: !tagMode && reviewAction.value === 'new_node' ? reviewForm.treeId : undefined,
      parent_id:
        !tagMode && reviewAction.value === 'new_node' && reviewForm.parentNodeIds[0]
          ? reviewForm.parentNodeIds[0]
          : undefined,
      name: reviewAction.value === 'new_node' ? reviewForm.name.trim() || c.raw_name : undefined,
      target_node_id: !tagMode && reviewAction.value !== 'new_node' ? reviewForm.targetNodeIds[0] : undefined,
      target_tag_id: tagMode && reviewAction.value !== 'new_node' ? reviewForm.targetTagId : undefined,
      reason: reviewForm.reason.trim() || undefined,
    })
    toast.success(reviewAction.value === 'merge' ? '已并入' : '已审核通过')
    showReviewDialog.value = false
    loadList()
    if (detail.value?.candidate.id === c.id) openDetail(c)
  } catch (e: any) {
    toast.error(e?.response?.data?.error ?? '审核失败')
  } finally {
    reviewing.value = false
  }
}

function openReject(c: TagCandidate) {
  rejectTarget.value = c
  rejectReason.value = ''
  showRejectDialog.value = true
}

async function doReject() {
  const c = rejectTarget.value
  if (!c) return
  rejecting.value = true
  try {
    await tagCandidateApi.reject(c.id, rejectReason.value.trim() || undefined)
    toast.success('已拒绝')
    showRejectDialog.value = false
    loadList()
    if (detail.value?.candidate.id === c.id) openDetail(c)
  } catch (e: any) {
    toast.error(e?.response?.data?.error ?? '操作失败')
  } finally {
    rejecting.value = false
  }
}

const newNodeActionLabel = computed(() =>
  isTagKind(detail.value?.candidate.kind ?? '') ? '接受为新标签' : '接受为新节点',
)

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
        <p class="page-sub">未匹配项确认保存后进入队列；节点进知识树，方法/素养进扁平标签</p>
      </div>
      <span class="pending-badge">待审核 {{ statusFilter === 'pending' ? total : '—' }}</span>
    </div>

    <div class="status-tabs">
      <button
        v-for="(label, key) in { ALL: '全部', pending: '待审核', approved: '已通过', rejected: '已拒绝', merged: '已并入' }"
        :key="key"
        :class="{ active: statusFilter === key }"
        @click="switchStatus(key)"
      >{{ label }}</button>
    </div>
    <div class="status-tabs">
      <button
        v-for="tab in KIND_TABS"
        :key="tab.key"
        :class="{ active: kindFilter === tab.key }"
        @click="switchKind(tab.key)"
      >{{ tab.label }}</button>
    </div>

    <div v-if="loading" class="loading-hint">加载中…</div>
    <AppEmpty v-else-if="items.length === 0" title="暂无候选" description="确认保存后未匹配的标签会出现在这里" />
    <div v-else class="candidate-list">
      <div v-for="c in items" :key="c.id" class="candidate-card" @click="openDetail(c)">
        <div class="candidate-main">
          <span class="candidate-kind">{{ KIND_LABELS[c.kind] ?? c.kind }}</span>
          <span class="candidate-type">{{ TARGET_TYPE_LABELS[c.target_type] ?? c.target_type }}</span>
          <span class="candidate-name">{{ c.raw_name }}</span>
          <span v-if="c.suggested_node_id || c.suggested_tag_id" class="candidate-alias-hint">建议别名</span>
          <span class="candidate-status" :class="c.status">{{ STATUS_LABELS[c.status] ?? c.status }}</span>
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

    <AppModal v-model="showDetail" title="候选详情" size="lg">
      <template v-if="detail">
        <div class="detail-block">
          <div class="detail-row"><span class="detail-label">原始标签</span><strong>{{ detail.candidate.raw_name }}</strong></div>
          <div class="detail-row"><span class="detail-label">规范化</span>{{ detail.candidate.normalized_name }}</div>
          <div class="detail-row"><span class="detail-label">维度</span>{{ KIND_LABELS[detail.candidate.kind] ?? detail.candidate.kind }}</div>
          <div class="detail-row"><span class="detail-label">落点</span>{{ TARGET_TYPE_LABELS[detail.candidate.target_type] ?? detail.candidate.target_type }}</div>
          <div class="detail-row"><span class="detail-label">置信度</span>{{ detail.candidate.ai_confidence ?? '—' }}</div>
          <div v-if="detail.suggested_node || detail.candidate.suggested_node_id" class="detail-row">
            <span class="detail-label">建议节点</span>
            <div v-if="detail.suggested_node" class="suggest-info">
              <strong class="suggest-name">{{ detail.suggested_node.name }}</strong>
              <span class="suggest-path">{{ detail.suggested_node.name_path }}</span>
              <span class="suggest-tree">{{ detail.suggested_node.tree_name }}</span>
            </div>
            <span v-else class="suggest-missing">节点已删除或不存在</span>
          </div>
          <div v-if="detail.suggested_tag || detail.candidate.suggested_tag_id" class="detail-row">
            <span class="detail-label">建议标签</span>
            <div v-if="detail.suggested_tag" class="suggest-info">
              <strong class="suggest-name">{{ detail.suggested_tag.name }}</strong>
              <span class="suggest-tree">{{ detail.suggested_tag.category }}</span>
            </div>
            <span v-else class="suggest-missing">标签已删除或不存在</span>
          </div>
          <div v-if="detail.candidate.review_note" class="detail-row">
            <span class="detail-label">审核备注</span>{{ detail.candidate.review_note }}
          </div>
        </div>
        <div v-if="detailStem" class="detail-block">
          <div class="detail-label">来源题目</div>
          <div class="source-stem">
            <LatexRender :text="detailStem" />
          </div>
          <div v-if="detailOptionList.length" class="source-options">
            <div
              v-for="opt in detailOptionList"
              :key="opt.label"
              class="source-opt"
            >
              <span class="source-opt-letter">
                <LatexRender :text="`$\\mathrm{${opt.label}.}$`" :inline="true" />
              </span>
              <div class="source-opt-content">
                <LatexRender :text="opt.content" :inline="true" />
              </div>
            </div>
          </div>
        </div>
        <div v-if="detail.candidate.status === 'pending'" class="detail-actions">
          <AppButton variant="primary" size="sm" @click="openReview(detail.candidate, 'new_node')">{{ newNodeActionLabel }}</AppButton>
          <AppButton variant="primary" size="sm" @click="openReview(detail.candidate, 'alias')">加为已有别名</AppButton>
          <AppButton variant="primary" size="sm" @click="openReview(detail.candidate, 'merge')">并入已有（合并）</AppButton>
          <AppButton variant="danger" size="sm" @click="openReject(detail.candidate)">拒绝</AppButton>
        </div>
      </template>
    </AppModal>

    <AppModal v-model="showReviewDialog" :title="reviewTitle">
      <div class="review-form">
        <template v-if="reviewAction === 'new_node' && !reviewIsTag">
          <div class="form-group">
            <label class="form-label">目标知识树</label>
            <select v-model="reviewForm.treeId" class="form-input" @change="onTreeChange">
              <option value="" disabled>请选择</option>
              <option v-for="t in filteredTrees" :key="t.id" :value="t.id">{{ t.name }}</option>
            </select>
          </div>
          <div v-if="reviewForm.treeId" class="form-group">
            <label class="form-label">父节点（可选）</label>
            <KnowledgeTreeCascader
              v-model="reviewForm.parentNodeIds"
              :tree-id="reviewForm.treeId"
              :kind="reviewTreeKind"
              :max="1"
              placeholder="不选则为树根…"
            />
          </div>
          <div class="form-group">
            <label class="form-label">节点名称</label>
            <input v-model="reviewForm.name" class="form-input" />
          </div>
        </template>
        <template v-else-if="reviewAction === 'new_node' && reviewIsTag">
          <div class="form-group">
            <label class="form-label">标签名称</label>
            <input v-model="reviewForm.name" class="form-input" />
          </div>
        </template>
        <template v-else-if="!reviewIsTag">
          <div class="form-group">
            <label class="form-label">{{ reviewAction === 'alias' ? '加为别名的目标节点' : '并入的目标节点' }}</label>
            <KnowledgeTreeCascader
              v-model="reviewForm.targetNodeIds"
              :kind="reviewTreeKind"
              :max="1"
              placeholder="选择目标节点…"
            />
          </div>
        </template>
        <template v-else>
          <div class="form-group">
            <label class="form-label">{{ reviewAction === 'alias' ? '加为别名的目标标签' : '并入的目标标签' }}</label>
            <select v-model="reviewForm.targetTagId" class="form-input">
              <option value="" disabled>请选择</option>
              <option v-for="t in tagOptions" :key="t.id" :value="t.id">{{ t.name }}</option>
            </select>
          </div>
        </template>
        <div class="form-group">
          <label class="form-label">审核备注</label>
          <input v-model="reviewForm.reason" class="form-input" placeholder="选填，将写入审核记录" />
        </div>
        <div class="form-actions">
          <AppButton variant="ghost" @click="showReviewDialog = false">取消</AppButton>
          <AppButton variant="primary" :loading="reviewing" @click="doReview">确认</AppButton>
        </div>
      </div>
    </AppModal>

    <AppModal v-model="showRejectDialog" title="拒绝该候选">
      <div class="review-form">
        <p class="page-sub">确定拒绝候选「{{ rejectTarget?.raw_name }}」吗？拒绝后该标签不会进入正式标签体系。</p>
        <div class="form-group">
          <label class="form-label">拒绝原因</label>
          <textarea v-model="rejectReason" class="form-input" rows="3" placeholder="选填" />
        </div>
        <div class="form-actions">
          <AppButton variant="ghost" @click="showRejectDialog = false">取消</AppButton>
          <AppButton variant="danger" :loading="rejecting" @click="doReject">拒绝</AppButton>
        </div>
      </div>
    </AppModal>

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
.candidate-kind, .candidate-type {
  font-size: 11px; padding: 1px 7px; border-radius: 6px;
  background: var(--purple-light); color: var(--purple); font-weight: 600;
}
.candidate-type { background: var(--bg-input); color: var(--text-secondary); }
.candidate-name { font-size: 15px; font-weight: 600; }
.candidate-alias-hint {
  font-size: 11px; padding: 1px 7px; border-radius: 6px;
  background: var(--accent-light, rgba(59,130,246,.12)); color: var(--accent); font-weight: 600;
}
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
.suggest-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  flex: 1;
}
.suggest-name { font-size: 13px; color: var(--text-primary); }
.suggest-path {
  font-size: 12px;
  color: var(--text-secondary);
  line-height: 1.4;
  word-break: break-word;
}
.suggest-tree {
  display: inline-flex;
  align-self: flex-start;
  margin-top: 2px;
  font-size: 11px;
  font-weight: 600;
  padding: 1px 7px;
  border-radius: 6px;
  background: var(--bg-input);
  color: var(--text-secondary);
}
.suggest-missing { font-size: 13px; color: var(--text-secondary); }
.source-stem {
  font-size: 13px;
  color: var(--text-primary);
  margin-top: 6px;
  line-height: 1.6;
  overflow-x: auto;
}
.source-stem :deep(.katex-display) {
  margin: 0.4em 0;
}
.source-options {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 6px 12px;
  margin-top: 10px;
}
.source-opt {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  font-size: 13px;
  line-height: 1.5;
  min-width: 0;
}
.source-opt-letter {
  flex-shrink: 0;
  color: var(--text-secondary);
}
.source-opt-content {
  min-width: 0;
  overflow-x: auto;
}
@media (max-width: 640px) {
  .source-options {
    grid-template-columns: 1fr;
  }
}
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
