<template>
  <div class="detail-page">
    <div v-if="loading" class="loading-hint">加载中…</div>

    <template v-else>
      <!-- ============ 顶部吸顶导航 ============ -->
      <header class="detail-header">
        <div class="header-left">
          <button class="back-link" @click="backToList">
            <AppIcon name="chevron-left" :size="18" :stroke="2" />
            <span>返回列表</span>
          </button>
          <h1 class="page-title">题目详情</h1>
        </div>
        <div class="header-actions">
          <template v-if="q?.status === 'draft'">
            <AppButton variant="outline" size="sm" @click="$router.push(`/questions/${q!.id}/edit`)"><AppIcon name="pencil" :size="15" /> 编辑</AppButton>
            <AppButton variant="primary" size="sm" :loading="submitting" :disabled="submitting" @click="submitReview">提交审核</AppButton>
            <AppButton v-if="canDelete" variant="danger" size="sm" @click="confirmDelete"><AppIcon name="trash" :size="15" /> 删除</AppButton>
          </template>
          <template v-else-if="q?.status === 'rejected'">
            <AppButton variant="outline" size="sm" @click="$router.push(`/questions/${q!.id}/edit`)"><AppIcon name="pencil" :size="15" /> 重新编辑</AppButton>
          </template>
          <template v-else-if="q?.status === 'pending' && q?.can_review">
            <AppButton variant="primary" size="sm" @click="handleReview('approved')"><AppIcon name="check-circle" :size="15" /> 通过</AppButton>
            <AppButton variant="danger" size="sm" @click="handleReview('rejected')"><AppIcon name="x-circle" :size="15" /> 驳回</AppButton>
          </template>
          <template v-else-if="q?.status === 'published'">
            <AppButton variant="outline" size="sm" @click="$router.push(`/questions/${q!.id}/edit`)"><AppIcon name="pencil" :size="15" /> 编辑</AppButton>
            <!-- 推送到公共题库 / 撤回推库申请 -->
            <AppButton
              v-if="spaceStore.currentSpace?.kind !== 'public' && !hasPendingSubmission"
              variant="primary"
              size="sm"
              :loading="submittingPublic"
              :disabled="submittingPublic"
              @click="handleSubmitToPublic"
            ><AppIcon name="upload" :size="15" /> 推送到公共题库</AppButton>
            <AppButton
              v-if="hasPendingSubmission"
              variant="outline"
              size="sm"
              :loading="withdrawing"
              :disabled="withdrawing"
              @click="handleWithdrawSubmission"
            ><AppIcon name="x-circle" :size="15" /> 撤回推库申请</AppButton>
            <AppButton v-if="canDelete" variant="danger" size="sm" @click="confirmDelete"><AppIcon name="trash" :size="15" /> 删除</AppButton>
            <AppButton v-if="auth.isAdmin" variant="ghost" size="sm" @click="toast.info('停用功能即将上线')"><AppIcon name="ban" :size="15" /> 停用</AppButton>
          </template>
        </div>
      </header>

      <!-- ============ 内容区：Flex 双栏，各自独立滚动 ============ -->
      <div class="detail-body">
        <!-- ===== 中间：沉浸式试卷卡片 ===== -->
        <div class="paper-scroll">
          <div class="paper-card">
            <!-- 卡片头部属性栏 -->
            <div class="paper-header">
              <div class="paper-header-left">
                <AppBadge :color="typeBadgeColor(q?.question_type || '')">{{ typeLabel(q?.question_type || '') }}</AppBadge>
                <span class="paper-difficulty">
                  <AppIcon v-for="n in 5" :key="n" name="star" :size="12" :class="{ active: diffStars >= n }" class="paper-star" />
                </span>
              </div>
              <div class="paper-header-right">
                <span class="pill-badge">
                  <AppIcon :name="statusIcon(q?.status || '')" :size="13" />
                  <span>{{ statusLabel(q?.status || '') }}</span>
                  <template v-if="q?.grade_level">
                    <span class="pill-divider"></span>
                    <span>{{ gradeLevelLabel(q.grade_level) }}</span>
                  </template>
                  <template v-if="q?.semester">
                    <span class="pill-divider"></span>
                    <span>{{ semesterLabel(q.semester) }}</span>
                  </template>
                </span>
              </div>
            </div>

            <!-- 题干 -->
            <div class="paper-stem">
              <LatexRender :text="q?.stem || ''" />
            </div>

            <!-- 选择题选项 -->
            <div
              v-if="q?.question_type === 'choice' && optionList.length"
              ref="optionsContainer"
              class="paper-options"
              :class="optionLayoutClass"
            >
              <div
                v-for="opt in optionList"
                :key="opt.label"
                class="paper-opt"
                :class="{ correct: isCorrect(opt.label) }"
              >
                <span class="paper-opt-letter">{{ opt.label }}.</span>
                <span class="paper-opt-content"><LatexRender :text="opt.content" :inline="true" /></span>
              </div>
            </div>

            <!-- 答案与解析区 — 材质化卡片 -->
            <div v-if="hasAnswer || q?.analysis || hasGrading" class="answer-solution-block">
              <!-- 参考答案卡片（莫兰迪极淡蓝底） -->
              <div v-if="hasAnswer" class="answer-card">
                <div class="card-section-title">参考答案</div>
                <!-- 选择题答案 -->
                <div v-if="q?.question_type === 'choice' && correctLabels.length" class="card-answer-content">
                  <span class="paper-correct-answer" v-for="a in correctLabels" :key="a">{{ a }}</span>
                </div>
                <!-- 填空题答案 -->
                <div v-else-if="q?.question_type === 'fill'" class="card-answer-content as-fill-list">
                  <span v-for="(item, i) in (q!.correct_answer as any[])" :key="i" class="as-fill-item">
                    {{ i + 1 }}. <LatexRender :text="item.answer || String(item)" :inline="true" :sub-question-badge="true" />
                  </span>
                </div>
                <!-- 解答题答案 — 逐小问 Flex 隔离布局 -->
                <div v-else-if="q?.question_type === 'solution'" class="card-answer-content">
                  <div v-for="(ans, i) in (q!.correct_answer as string[])" :key="i" class="answer-item-row">
                    <span class="sub-question-badge">{{ i + 1 }}</span>
                    <div class="answer-item-body"><LatexRender :text="ans" /></div>
                  </div>
                </div>
              </div>

              <!-- 解析卡片（苹果系统柔和灰底） -->
              <div v-if="q?.analysis" class="analysis-card">
                <div class="card-section-title-row">
                  <span class="card-section-title">解析</span>
                  <div v-if="detailSolutions.length > 1" class="sol-seg">
                    <button
                      v-for="(s, i) in detailSolutions"
                      :key="i"
                      class="sol-seg-btn"
                      :class="{ active: activeSolution === i }"
                      @click="activeSolution = i"
                    >解法{{ cnNum(i + 1) }}</button>
                  </div>
                </div>
                <div class="paper-analysis-content">
                  <Transition name="sol-fade" mode="out-in">
                    <LatexRender :key="activeSolution" :text="splitSolution(detailSolutions[activeSolution]).body" :sub-question-badge="true" />
                  </Transition>
                </div>
                <div v-if="splitSolution(detailSolutions[activeSolution]).conclusion" class="paper-conclusion">
                  <LatexRender :text="splitSolution(detailSolutions[activeSolution]).conclusion" />
                </div>
              </div>

              <!-- 评分标准卡片 -->
              <div v-if="hasGrading" class="analysis-card">
                <div class="card-section-title">评分标准</div>
                <div class="as-grading-list">
                  <div v-for="(step, i) in (q!.grading_criteria as any[])" :key="i" class="paper-grading-step">
                    <span class="paper-grading-label">{{ step.label || `步骤${i + 1}` }}</span>
                    <span class="paper-grading-score">{{ step.score || 0 }}分</span>
                    <span v-if="step.desc" class="paper-grading-desc">{{ step.desc }}</span>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- ===== 右侧：知识点 + 元信息 ===== -->
        <div class="side-scroll">
          <!-- 知识点卡片 -->
          <div class="side-card">
            <div class="side-card-title"><AppIcon name="tag" :size="15" /> 知识点</div>
            <div v-if="q?.knowledge_nodes?.length" class="kp-tags">
              <span v-for="kn in q!.knowledge_nodes" :key="kn.id" class="kp-tag">{{ kn.name }}</span>
            </div>
            <div v-else class="side-empty">未关联知识点</div>
          </div>

          <!-- 元信息卡片 -->
          <div class="side-card">
            <div class="side-card-title"><AppIcon name="info" :size="15" /> 元信息</div>
            <div class="meta-list">
              <div class="meta-row"><span class="meta-label">创建者</span><span class="meta-val">{{ q?.creator_name || q?.creator_id?.substring(0, 8) || '—' }}</span></div>
              <div class="meta-row"><span class="meta-label">版本</span><span class="meta-val">v{{ q?.version }}</span></div>
              <div class="meta-row"><span class="meta-label">创建</span><span class="meta-val">{{ formatTime(q?.created_at) }}</span></div>
              <div class="meta-row"><span class="meta-label">更新</span><span class="meta-val">{{ formatTime(q?.updated_at) }}</span></div>
            </div>
          </div>

          <!-- 被引用的试卷（溯源卡片） -->
          <div class="side-card">
            <div class="side-card-title">
              <AppIcon name="files" :size="15" />
              被引用的试卷
              <span v-if="questionPapers.length" class="side-card-count">{{ questionPapers.length }}</span>
            </div>
            <div v-if="questionPapers.length" class="qp-list">
              <router-link
                v-for="p in questionPapers"
                :key="p.paper_id"
                :to="`/papers/${p.paper_id}`"
                class="qp-item"
              >
                <div class="qp-item-title">{{ p.title }}</div>
                <div class="qp-item-meta">
                  <span v-if="p.section">{{ p.section }}</span>
                  <span>分值 {{ p.score }}</span>
                  <span>序号 #{{ p.sort_order }}</span>
                </div>
              </router-link>
            </div>
            <div v-else class="side-empty">该题目暂未被试卷引用</div>
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

    <!-- 团队空间：审题人选择对话框（GAP-3 修复） -->
    <AppModal v-model="reviewerDialog" title="选择审题人">
      <div class="reviewer-dialog-body">
        <p class="reviewer-dialog-hint">
          团队空间需要交叉审核，请选择空间内的其他成员作为审题人
        </p>
        <select v-model="selectedReviewerId" class="reviewer-select">
          <option value="">请选择审题人…</option>
          <option v-for="m in reviewableMembers" :key="m.user_id" :value="m.user_id">
            {{ m.display_name || m.username }}（{{ m.role === 'owner' ? '拥有者' : '成员' }}）
          </option>
        </select>
      </div>
      <div class="form-actions">
        <AppButton variant="ghost" @click="reviewerDialog = false">取消</AppButton>
        <AppButton
          variant="primary"
          :disabled="!selectedReviewerId"
          :loading="submitting"
          @click="confirmSubmitWithReviewer"
        >确认提交</AppButton>
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
import { ref, computed, onMounted, onBeforeUnmount, nextTick, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { questionApi, spaceApi, paperApi, type QuestionDetail, type GradeLevel, type SemesterType, type SpaceMemberInfo, type QuestionPaperItem, publicLibraryApi } from '@/api/client'
import client from '@/api/client'
import { useAuthStore } from '@/stores/auth'
import { useSpaceStore } from '@/stores/space'
import LatexRender from '@/components/LatexRender.vue'
import { AppButton, AppBadge, AppModal, AppConfirm, AppIcon } from '@/components/ui'
import { useToast } from '@/composables/useToast'
import { typeLabel, typeBadgeColor, statusLabel, statusIcon, formatTime } from '@/utils/questionDisplay'
import { useOptionsLayout } from '@/composables/useOptionsLayout'

const route = useRoute()
const router = useRouter()
const auth = useAuthStore()
const spaceStore = useSpaceStore()
const toast = useToast()
const q = ref<QuestionDetail | null>(null)
const loading = ref(false)
const submitting = ref(false)
const rejectDialog = ref(false)
const rejectComment = ref('')
const deleteDialog = ref(false)

// ── 删除权限：按空间类型分流 ──
// 个人空间：始终允许；团队/公共空间：仅超级管理员或空间 Owner
const canDelete = computed(() => {
  if (!q.value) return false
  const status = q.value.status
  if (status !== 'draft' && status !== 'published') return false
  const space = spaceStore.currentSpace
  if (!space) return false
  if (space.kind === 'personal') return true
  return auth.isSuperAdmin || space.owner_user_id === auth.userId
})

// 难度星数：后端返回 1-5 数值，直接用作星数
const diffStars = computed(() => {
  const d = q.value?.difficulty
  if (typeof d === 'number') return d
  return 0
})

// GradeLevel 枚举 → 中文标签
function gradeLevelLabel(g: GradeLevel | null | undefined): string {
  if (!g) return ''
  const map: Record<GradeLevel, string> = {
    grade_7: '初一',
    grade_8: '初二',
    grade_9: '初三',
    grade_10: '高一',
    grade_11: '高二',
    grade_12: '高三',
    other: '其他',
  }
  return map[g] || g
}

// SemesterType 枚举 → 中文标签
function semesterLabel(s: SemesterType | null | undefined): string {
  if (!s) return ''
  const map: Record<SemesterType, string> = {
    first: '上学期',
    second: '下学期',
    full_year: '全年',
  }
  return map[s] || s
}

// 安全提取选项列表（兼容数组/对象/JSON字符串）
const optionList = computed(() => {
  const opts = q.value?.options
  if (!opts) return []
  let parsed = opts
  if (typeof parsed === 'string') {
    try { parsed = JSON.parse(parsed) } catch { return [] }
  }
  if (!Array.isArray(parsed)) return []
  return parsed.map((opt: any) => {
    if (typeof opt === 'string') {
      const match = opt.match(/^([A-Z])[.、．]\s*(.*)$/)
      if (match) return { label: match[1], content: match[2] }
      return { label: '', content: opt }
    }
    if (opt && typeof opt === 'object' && opt.label) {
      return { label: opt.label, content: opt.content || '' }
    }
    return { label: '', content: String(opt) }
  })
})

// 正确答案标签列表
const correctLabels = computed(() => {
  const ans = q.value?.correct_answer
  if (!ans) return []
  if (Array.isArray(ans)) return ans.map(String).sort()
  return [String(ans)]
})

// ===== 多解法 =====
const cnNums = ['一', '二', '三', '四', '五', '六', '七', '八', '九', '十']
function cnNum(n: number): string {
  return cnNums[n - 1] || String(n)
}

const activeSolution = ref(0)

const detailSolutions = computed(() => {
  const analysis = q.value?.analysis
  if (!analysis) return []
  if (analysis.includes('\n\n---\n\n')) return analysis.split(/\n\n---\n\n/)
  if (/\n解法[二三四五六七八九十]/.test(analysis)) return analysis.split(/\n(?=解法[二三四五六七八九十])/).map(s => s.trim())
  return [analysis]
})

function splitSolution(text: string): { body: string; conclusion: string } {
  if (!text) return { body: '', conclusion: '' }
  const patterns = [
    /(?:故|因此|所以|综上)[选答]\s*[A-Z](?:[、,，]\s*[A-Z])*\s*。?\s*$/,
    /(?:故|因此|所以|综上)[^。\n]*答案[^。\n]*[。]?\s*$/,
    /(?:故|因此|所以|综上)[^。\n]*[。]?\s*$/,
    /故选\s*[A-Z](?:[、,，]\s*[A-Z])*\s*。?\s*$/,
  ]
  for (const p of patterns) {
    const m = text.match(p)
    if (m) {
      const idx = text.lastIndexOf(m[0])
      return { body: text.substring(0, idx).trim(), conclusion: m[0].trim() }
    }
  }
  return { body: text.trim(), conclusion: '' }
}

// 是否有参考答案
const hasAnswer = computed(() => {
  const ans = q.value?.correct_answer
  if (!ans) return false
  if (Array.isArray(ans)) return ans.length > 0
  return !!ans
})

// 是否有评分标准
const hasGrading = computed(() => {
  const g = q.value?.grading_criteria
  return !!(g && Array.isArray(g) && g.length)
})

// ---- 选择题选项自适应网格布局（使用全局 Composable 提炼）----
const optionsContainer = ref<HTMLElement | null>(null)
const { layout: optionLayout } = useOptionsLayout(optionsContainer, optionList, '.paper-opt')
const optionLayoutClass = computed(() => optionLayout.value)

async function fetchDetail() {
  loading.value = true
  try {
    const res = await questionApi.get(route.params.id as string)
    q.value = res.data
    // 已发布题目：查询推库申请状态
    if (q.value?.status === 'published') {
      await checkSubmissionStatus()
    }
    // 加载被引用的试卷列表（溯源）
    loadQuestionPapers()
  } catch { /* handled */ }
  finally { loading.value = false }
}

// ── 题目被引用的试卷列表（溯源卡片）──
const questionPapers = ref<QuestionPaperItem[]>([])

async function loadQuestionPapers() {
  try {
    const res = await paperApi.getQuestionPapers(route.params.id as string)
    questionPapers.value = res.data
  } catch {
    questionPapers.value = []
  }
}

// ── 团队空间审题人选择（GAP-3 修复）──
const reviewerDialog = ref(false)
const selectedReviewerId = ref('')
const spaceMembers = ref<SpaceMemberInfo[]>([])

// 可选审题人：团队空间中排除自己和 viewer
const reviewableMembers = computed(() =>
  spaceMembers.value.filter(m => m.user_id !== auth.userId && m.role !== 'viewer'),
)

async function loadSpaceMembers() {
  if (spaceStore.currentSpace?.kind !== 'team' || !spaceStore.currentSpaceId) return
  try {
    const res = await spaceApi.get(spaceStore.currentSpaceId)
    spaceMembers.value = res.data.members || []
  } catch { /* handled */ }
}

async function submitReview() {
  // 团队空间：需要先选择审题人
  if (spaceStore.currentSpace?.kind === 'team') {
    await loadSpaceMembers()
    if (reviewableMembers.value.length === 0) {
      toast.error('团队空间内无可选审题人，请先邀请其他成员加入空间')
      return
    }
    selectedReviewerId.value = ''
    reviewerDialog.value = true
    return
  }

  // 个人空间：自审自发，直接提交
  submitting.value = true
  try {
    await questionApi.submit(route.params.id as string)
    toast.success('已提交审核')
    fetchDetail()
  } catch (e: any) {
    toast.error(e.response?.data?.error || e.response?.data?.message || e.message || '提交审核失败')
  } finally { submitting.value = false }
}

// 团队空间：确认选择审题人后提交
async function confirmSubmitWithReviewer() {
  if (!selectedReviewerId.value) return
  submitting.value = true
  try {
    await questionApi.submit(route.params.id as string, { reviewer_id: selectedReviewerId.value })
    toast.success('已提交审核')
    reviewerDialog.value = false
    selectedReviewerId.value = ''
    fetchDetail()
  } catch (e: any) {
    toast.error(e.response?.data?.error || e.response?.data?.message || e.message || '提交审核失败')
  } finally { submitting.value = false }
}

// ── 推送到公共题库 ──
const hasPendingSubmission = ref(false)
const pendingSubmissionId = ref<string | null>(null)
const submittingPublic = ref(false)
const withdrawing = ref(false)

async function checkSubmissionStatus() {
  if (!q.value) return
  try {
    const res = await publicLibraryApi.getSubmissionStatus(q.value.id)
    hasPendingSubmission.value = res.data.has_pending_submission
    pendingSubmissionId.value = res.data.submission_id
  } catch { /* ignore */ }
}

async function handleSubmitToPublic() {
  if (!q.value) return
  submittingPublic.value = true
  try {
    await publicLibraryApi.submitToPublic(q.value.id)
    toast.success('已提交推库申请，等待管理员审核')
    await checkSubmissionStatus()
  } catch (e: any) {
    toast.error(e.response?.data?.error || e.response?.data?.message || e.message || '推送失败')
  } finally { submittingPublic.value = false }
}

async function handleWithdrawSubmission() {
  if (!pendingSubmissionId.value) return
  withdrawing.value = true
  try {
    await publicLibraryApi.withdraw(pendingSubmissionId.value)
    toast.success('已撤回推库申请')
    hasPendingSubmission.value = false
    pendingSubmissionId.value = null
  } catch (e: any) {
    toast.error(e.response?.data?.error || e.response?.data?.message || e.message || '撤回失败')
  } finally { withdrawing.value = false }
}

// 返回列表：优先用 router.back() 回退，不产生重复历史条目
function backToList() {
  if (window.history.state?.back) {
    router.back()
  } else {
    router.replace('/questions')
  }
}

function confirmDelete() {
  deleteDialog.value = true
}

async function doDelete() {
  try {
    await client.delete(`/questions/${route.params.id}`)
    toast.success('已删除')
    backToList()
  } catch (e: any) {
    console.error('删除题目失败:', e)
    const errData = e.response?.data
    const errMsg = typeof errData === 'string' ? errData : (errData?.error || errData?.message)
    toast.error(errMsg || e.message || '删除失败')
  }
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
    if (action === 'approved') {
      await questionApi.approve(route.params.id as string)
    } else {
      await questionApi.reject(route.params.id as string, { reject_reason: comment })
    }
    toast.success(action === 'approved' ? '已通过' : '已驳回')
    await fetchDetail()
    return true
  } catch (e: any) {
    console.error('审核操作失败:', e)
    toast.error(e.response?.data?.error || e.response?.data?.message || e.message || '操作失败')
    return false
  }
}

function isCorrect(label: string): boolean {
  const ans = q.value?.correct_answer
  if (!ans) return false
  if (Array.isArray(ans)) return ans.map(String).includes(label)
  return String(ans) === label
}

const isMultiChoice = computed(() => {
  if (q.value?.question_type !== 'choice') return false
  if ((q.value as any)?.sub_type === 'multi') return true
  const ans = q.value?.correct_answer
  return Array.isArray(ans) && ans.length > 1
})

// ── 空间切换监听：防止幽灵页面 ──
// 详情页切换空间后，原题目可能不属于新空间，立即重定向回列表页
watch(() => spaceStore.currentSpaceId, () => {
  router.replace('/questions')
})

onMounted(async () => {
  await fetchDetail()
})
</script>

<style scoped>
/* ============ 页面根容器 ============ */
.detail-page {
  display: block;
  min-height: 100%;
  background: #f5f5f7;
}

[data-theme='dark'] .detail-page {
  background: var(--bg-primary);
}

.loading-hint {
  text-align: center;
  padding: 48px 20px;
  color: var(--text-muted);
  font-size: 14px;
}

/* ============ 顶部吸顶导航 ============ */
.detail-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-shrink: 0;
  padding: 12px 24px;
  background: rgba(255, 255, 255, 0.72);
  backdrop-filter: saturate(180%) blur(20px);
  -webkit-backdrop-filter: saturate(180%) blur(20px);
  border-bottom: 1px solid rgba(0, 0, 0, 0.06);
  gap: 12px;
}

[data-theme='dark'] .detail-header {
  background: rgba(30, 30, 30, 0.72);
  border-bottom-color: rgba(255, 255, 255, 0.08);
}

.header-left {
  display: flex;
  align-items: center;
  gap: 12px;
}

/* 返回链接 — 苹果蓝文字 */
.back-link {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  background: none;
  border: none;
  cursor: pointer;
  color: #0071e3;
  font-weight: 500;
  font-size: 15px;
  padding: 4px 4px 4px 0;
  transition: opacity 0.2s;
}

.back-link:hover {
  opacity: 0.7;
}

[data-theme='dark'] .back-link {
  color: #0a84ff;
}

.page-title {
  font-size: 17px;
  font-weight: 650;
  color: var(--text-primary);
  margin: 0;
  letter-spacing: -0.01em;
}

.header-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

/* 柔和危险按钮 — 浅红底+红文字 */
.btn-soft-danger,
.btn-soft-primary,
.btn-soft-secondary,
.btn-soft-success {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 6px 16px;
  font-size: 13px;
  font-weight: 500;
  border-radius: 999px;
  border: none;
  cursor: pointer;
  transition: all 0.2s ease;
  white-space: nowrap;
  letter-spacing: -0.01em;
}

/* 主操作 — 苹果蓝 */
.btn-soft-primary {
  background: #0071e3;
  color: #ffffff;
}

.btn-soft-primary:hover {
  background: #0077ed;
  transform: translateY(-1px);
  box-shadow: 0 2px 8px rgba(0, 113, 227, 0.25);
}

.btn-soft-primary:active {
  transform: scale(0.97);
}

.btn-soft-primary:disabled {
  opacity: 0.5;
  cursor: not-allowed;
  transform: none;
}

/* 次要操作 — 莫兰迪灰底 */
.btn-soft-secondary {
  background: #f5f5f7;
  color: #1d1d1f;
}

.btn-soft-secondary:hover {
  background: #e8e8ed;
  transform: translateY(-1px);
}

.btn-soft-secondary:active {
  transform: scale(0.97);
}

/* 成功操作 — 莫兰迪浅绿 */
.btn-soft-success {
  background: #e8f8ee;
  color: #248a3d;
}

.btn-soft-success:hover {
  background: #d4f0dd;
  color: #1a7a2e;
  transform: translateY(-1px);
  box-shadow: 0 2px 8px rgba(36, 138, 61, 0.15);
}

.btn-soft-success:active {
  transform: scale(0.97);
}

/* 危险操作 — 浅红底 */
.btn-soft-danger {
  background: #fee2e2;
  color: #dc2626;
}

.btn-soft-danger:hover {
  background: #fecaca;
  color: #b91c1c;
  transform: translateY(-1px);
  box-shadow: 0 2px 8px rgba(220, 38, 38, 0.15);
}

.btn-soft-danger:active {
  transform: scale(0.97);
}

[data-theme='dark'] .btn-soft-primary {
  background: #0a84ff;
}

[data-theme='dark'] .btn-soft-secondary {
  background: rgba(255, 255, 255, 0.08);
  color: #f5f5f7;
}

[data-theme='dark'] .btn-soft-secondary:hover {
  background: rgba(255, 255, 255, 0.12);
}

[data-theme='dark'] .btn-soft-success {
  background: rgba(48, 209, 88, 0.15);
  color: #30d158;
}

[data-theme='dark'] .btn-soft-danger {
  background: rgba(220, 38, 38, 0.15);
  color: #ff6961;
}

[data-theme='dark'] .btn-soft-danger:hover {
  background: rgba(220, 38, 38, 0.25);
}

/* 移除旧的 AppButton 覆盖 */
.header-actions :deep(.btn) {
  border-radius: 999px;
}

/* ============ 内容区：居中双栏 ============ */
.detail-body {
  flex: 1;
  display: flex;
  align-items: flex-start;
  justify-content: center;
  gap: 24px;
  max-width: 1200px;
  width: 100%;
  margin: 0 auto;
  padding: 20px;
}

/* 左侧内容区 — 自然高度，由全局滚动接管 */
.paper-scroll {
  flex: 1;
  min-width: 0;
}

/* 右侧粘性侧边栏 — 悬浮固定在可视区域内 */
.side-scroll {
  flex: 0 0 300px;
  position: sticky;
  top: 24px;
}

/* ============ 中间：沉浸式试卷卡片 ============ */
.paper-card {
  background: #ffffff;
  border-radius: 16px;
  padding: 28px 36px;
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.03), 0 1px 3px rgba(0, 0, 0, 0.02);
  border: none;
  margin: 16px 0;
  transition: box-shadow 0.3s cubic-bezier(0.25, 0.8, 0.25, 1);
}

[data-theme='dark'] .paper-card {
  background: #1c1c1e;
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.3), 0 1px 3px rgba(0, 0, 0, 0.15);
}

/* 卡片头部属性栏 */
.paper-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 8px;
  margin-bottom: 18px;
  padding-bottom: 14px;
  border-bottom: 1px solid rgba(0, 0, 0, 0.05);
}

[data-theme='dark'] .paper-header {
  border-bottom-color: rgba(255, 255, 255, 0.06);
}

.paper-header-left,
.paper-header-right {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.paper-difficulty {
  display: flex;
  gap: 1px;
}

.paper-star {
  color: #d1d1d6;
  transition: color 0.2s;
}

.paper-star.active {
  color: #ff9500;
}

.paper-meta-tag {
  font-size: 12px;
  color: var(--text-muted);
  display: inline-flex;
  align-items: center;
}

/* 题干 */
.paper-stem {
  font-size: 15px;
  line-height: 1.8;
  color: #1d1d1f;
  margin-bottom: 16px;
  word-break: break-word;
}

[data-theme='dark'] .paper-stem {
  color: #f5f5f7;
}

.paper-stem :deep(p) {
  margin: 0 0 8px;
}

/* 选项 — 自适应网格 */
.paper-options {
  display: grid;
  gap: 10px;
  margin-bottom: 16px;
}

.paper-options.grid-4 {
  grid-template-columns: repeat(4, 1fr);
}

.paper-options.grid-2 {
  grid-template-columns: repeat(2, 1fr);
}

.paper-options.grid-1 {
  grid-template-columns: 1fr;
}

.paper-opt {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 14px;
  line-height: 1.7;
  color: #3a3a3c;
  padding: 4px 0;
}

[data-theme='dark'] .paper-opt {
  color: #d1d1d6;
}

.paper-opt.correct {
  color: var(--success);
}

.paper-opt-letter {
  flex-shrink: 0;
  font-size: 14px;
  font-weight: 600;
  color: inherit;
}

.paper-opt-content {
  min-width: 0;
}

/* 填空题答案 */
.paper-blanks {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-bottom: 16px;
}

.paper-blank {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 14px;
  padding: 6px 0;
}

.paper-blank-num {
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
.paper-solution-answer {
  font-size: 14px;
  line-height: 1.8;
  color: #1d1d1f;
  margin-bottom: 16px;
}

[data-theme='dark'] .paper-solution-answer {
  color: #f5f5f7;
}

/* ============ 悬浮胶囊（Pill Badge） ============ */
.pill-badge {
  background: #ffffff;
  border-radius: 999px;
  padding: 6px 14px;
  font-size: 14px;
  font-weight: 500;
  color: #1d1d1f;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.06), 0 1px 2px rgba(0, 0, 0, 0.04);
  display: inline-flex;
  align-items: center;
  gap: 8px;
  transition: transform 0.3s cubic-bezier(0.25, 1, 0.5, 1), box-shadow 0.3s ease;
  cursor: default;
}

.pill-badge:hover {
  transform: translateY(-1px) scale(1.02);
  box-shadow: 0 4px 14px rgba(0, 0, 0, 0.1), 0 2px 4px rgba(0, 0, 0, 0.06);
}

.pill-badge svg {
  color: #86868b;
  flex-shrink: 0;
}

.pill-divider {
  width: 1px;
  height: 14px;
  background: rgba(0, 0, 0, 0.1);
  flex-shrink: 0;
}

[data-theme='dark'] .pill-badge {
  background: #2c2c2e;
  color: #f5f5f7;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.3), 0 1px 2px rgba(0, 0, 0, 0.2);
}

[data-theme='dark'] .pill-divider {
  background: rgba(255, 255, 255, 0.15);
}

/* ============ 答案与解析材质化卡片 ============ */
.answer-solution-block {
  margin-top: 4px;
}

/* 参考答案卡片 — 莫兰迪极淡蓝底 */
.answer-card {
  background: #f4f8fc;
  border-radius: 16px;
  padding: 20px 24px;
  margin-bottom: 24px;
  border: none;
  transition: all 0.3s ease;
}

.answer-card:hover {
  background: #edf3f9;
}

[data-theme='dark'] .answer-card {
  background: rgba(100, 160, 220, 0.08);
}

[data-theme='dark'] .answer-card:hover {
  background: rgba(100, 160, 220, 0.12);
}

/* 解析卡片 — 苹果系统柔和灰底 */
.analysis-card {
  background: #f5f5f7;
  border-radius: 16px;
  padding: 20px 24px;
  margin-bottom: 24px;
  border: none;
  transition: all 0.3s ease;
}

.analysis-card:last-child {
  margin-bottom: 0;
}

.analysis-card:hover {
  background: #ebebef;
}

[data-theme='dark'] .analysis-card {
  background: rgba(255, 255, 255, 0.05);
}

[data-theme='dark'] .analysis-card:hover {
  background: rgba(255, 255, 255, 0.08);
}

/* 卡片标题 — 精致排版层级 */
.card-section-title {
  font-size: 14px;
  font-weight: 600;
  color: #1d1d1f;
  margin-bottom: 16px;
  letter-spacing: -0.01em;
  display: block;
}

[data-theme='dark'] .card-section-title {
  color: #f5f5f7;
}

.card-section-title-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
}

.card-section-title-row .card-section-title {
  margin-bottom: 0;
}

.card-answer-content {
  font-size: 14px;
  line-height: 1.8;
  color: var(--text-primary);
}

.card-answer-content.as-fill-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

/* 解答题多小问答案行 — Flex 隔离防挤压 */
.answer-item-row {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  margin-bottom: 16px;
  line-height: 1.6;
}

.answer-item-row:last-child {
  margin-bottom: 0;
}

/* 模板层级徽章（在 scoped 范围内，直接生效） */
.answer-item-row > .sub-question-badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  background: #0071e3;
  color: #ffffff;
  border-radius: 50%;
  font-size: 12px;
  font-weight: 600;
  flex-shrink: 0;
  margin-top: 2px;
  box-shadow: 0 2px 6px rgba(0, 113, 227, 0.3);
}

[data-theme='dark'] .answer-item-row > .sub-question-badge {
  background: #0a84ff;
}

.answer-item-body {
  flex: 1;
  min-width: 0;
}

.as-grading-list {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.paper-correct-answer {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 28px;
  height: 28px;
  padding: 0 10px;
  border-radius: var(--radius-full);
  background: rgba(52, 199, 89, 0.12);
  color: var(--success);
  font-weight: 700;
  font-size: 15px;
  margin-right: 6px;
}

[data-theme='dark'] .paper-correct-answer {
  background: rgba(48, 209, 88, 0.15);
}

.paper-analysis-content {
  width: 100%;
  font-size: 14px;
  line-height: 1.8;
  color: var(--text-primary);
}

.paper-analysis-content :deep(p) {
  margin: 0 0 8px;
}

/* 多解法分段切换 */
.sol-seg {
  display: inline-flex;
  gap: 2px;
  padding: 2px;
  border-radius: var(--radius-full);
  background: var(--bg-input);
}

.sol-seg-btn {
  padding: 3px 10px;
  border: none;
  border-radius: var(--radius-full);
  background: transparent;
  font-size: 11px;
  font-weight: 500;
  color: var(--text-muted);
  cursor: pointer;
  transition: all 0.2s ease;
}

.sol-seg-btn.active {
  background: var(--bg-card);
  color: var(--accent);
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.08);
}

[data-theme='dark'] .sol-seg-btn.active {
  background: rgba(255, 255, 255, 0.12);
}

/* 结论收尾段 — 融入卡片底色，纯文本强化 */
.paper-conclusion {
  margin-top: 24px;
  padding: 0;
  border-radius: 0;
  background: transparent;
  border: none;
  box-shadow: none;
  font-size: 14px;
  font-weight: 600;
  color: #1d1d1f;
  line-height: 1.8;
}

[data-theme='dark'] .paper-conclusion {
  color: #f5f5f7;
}

/* 淡入淡出过渡 */
.sol-fade-enter-active,
.sol-fade-leave-active {
  transition: opacity 0.2s ease;
}

.sol-fade-enter-from,
.sol-fade-leave-to {
  opacity: 0;
}

/* 评分标准 */
.paper-grading {
  margin-bottom: 16px;
}

.paper-grading-step {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 12px;
  background: #ffffff;
  border: none;
  border-radius: 10px;
  font-size: 13px;
  margin-top: 0;
  transition: all 0.2s ease;
}

.paper-grading-step:hover {
  background: rgba(0, 113, 227, 0.04);
}

[data-theme='dark'] .paper-grading-step {
  background: rgba(255, 255, 255, 0.04);
}

.paper-grading-label {
  font-weight: 600;
  color: var(--text-primary);
}

.paper-grading-score {
  color: var(--accent);
  font-weight: 600;
}

.paper-grading-desc {
  color: var(--text-muted);
}

/* ============ 右侧卡片 ============ */
.side-card {
  background: #ffffff;
  border: none;
  border-radius: 16px;
  padding: 16px 18px;
  margin-bottom: 14px;
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.03), 0 1px 3px rgba(0, 0, 0, 0.02);
  transition: all 0.3s cubic-bezier(0.25, 0.8, 0.25, 1);
}

.side-card:hover {
  transform: translateY(-2px);
  box-shadow: 0 8px 28px rgba(0, 0, 0, 0.06), 0 2px 8px rgba(0, 0, 0, 0.03);
}

[data-theme='dark'] .side-card {
  background: #1c1c1e;
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.3), 0 1px 3px rgba(0, 0, 0, 0.15);
}

[data-theme='dark'] .side-card:hover {
  box-shadow: 0 8px 28px rgba(0, 0, 0, 0.4), 0 2px 8px rgba(0, 0, 0, 0.2);
}

.side-card-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  font-weight: 700;
  color: var(--text-primary);
  margin-bottom: 12px;
  padding-bottom: 10px;
  border-bottom: 1px solid rgba(0, 0, 0, 0.05);
}

[data-theme='dark'] .side-card-title {
  border-bottom-color: rgba(255, 255, 255, 0.08);
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
  transition: all 0.2s ease;
}

.kp-tag:hover {
  background: var(--accent);
  color: #fff;
}

.side-empty {
  font-size: 13px;
  color: #86868b;
}

/* ===== 被引用的试卷溯源卡片 ===== */
.side-card-count {
  margin-left: auto;
  font-size: 11px;
  font-weight: 600;
  color: var(--accent);
  background: var(--accent-light);
  padding: 1px 7px;
  border-radius: 9999px;
}

.qp-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.qp-item {
  display: block;
  padding: 8px 10px;
  border-radius: 8px;
  border: 1px solid var(--border-color);
  background: var(--bg-input, #f5f5f7);
  text-decoration: none;
  color: var(--text-primary);
  transition: all 0.15s ease;
}

.qp-item:hover {
  border-color: var(--accent);
  background: var(--accent-light);
}

.qp-item-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--accent);
  margin-bottom: 4px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.qp-item-meta {
  display: flex;
  gap: 10px;
  font-size: 11px;
  color: var(--text-muted);
  flex-wrap: wrap;
}

.meta-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.meta-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: 13px;
}

.meta-label {
  color: #86868b;
  font-size: 13px;
}

.meta-val {
  color: #1d1d1f;
  font-weight: 500;
  font-size: 13px;
}

[data-theme='dark'] .meta-label {
  color: #86868b;
}

[data-theme='dark'] .meta-val {
  color: #f5f5f7;
}

/* ============ 弹窗 ============ */
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

.reviewer-dialog-body {
  padding: 4px 0;
  min-width: 340px;
}

.reviewer-dialog-hint {
  font-size: 13px;
  color: var(--text-muted);
  margin: 0 0 12px 0;
  line-height: 1.4;
}

.reviewer-select {
  width: 100%;
  padding: 8px 12px;
  border-radius: var(--radius-sm, 8px);
  background: var(--bg-input, #fff);
  border: 1px solid var(--border-color, #d1d1d6);
  color: var(--text-primary, #1d1d1f);
  font-size: 14px;
  box-sizing: border-box;
}

.reviewer-select:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
}

/* ============ 响应式 ============ */
@media (max-width: 1024px) {
  .detail-body {
    flex-direction: column;
  }
  .side-scroll {
    flex: none;
    display: flex;
    flex-direction: row;
    gap: 14px;
    position: static;
  }
  .side-card {
    flex: 1;
    margin-bottom: 0;
  }
}

@media (max-width: 768px) {
  .detail-header {
    padding: 10px 16px;
  }
  .detail-body {
    padding: 0 16px 12px;
  }
  .paper-card {
    padding: 18px 20px;
    margin: 10px 0;
  }
  .paper-options.grid-4,
  .paper-options.grid-2 {
    grid-template-columns: 1fr;
  }
  .side-scroll {
    flex-direction: column;
  }
  .header-actions {
    flex-wrap: wrap;
  }
}
</style>
