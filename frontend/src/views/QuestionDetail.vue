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
            <AppButton variant="primary" size="sm" @click="$router.push(`/questions/${q!.id}/edit`)">编辑</AppButton>
            <AppButton variant="success" size="sm" :loading="submitting" @click="submitReview">提交审核</AppButton>
            <button class="btn-soft-danger" @click="confirmDelete"><AppIcon name="trash" :size="16" /> 删除</button>
          </template>
          <template v-else-if="q?.status === 'rejected'">
            <AppButton variant="primary" size="sm" @click="$router.push(`/questions/${q!.id}/edit`)">重新编辑</AppButton>
          </template>
          <template v-else-if="q?.status === 'pending' && q?.can_review">
            <AppButton variant="success" size="sm" @click="handleReview('approved')"><AppIcon name="check-circle" :size="16" /> 通过</AppButton>
            <button class="btn-soft-danger" @click="handleReview('rejected')"><AppIcon name="x-circle" :size="16" /> 驳回</button>
          </template>
          <template v-else-if="q?.status === 'published' && auth.isAdmin">
            <AppButton variant="outline" size="sm" @click="toast.info('停用功能即将上线')"><AppIcon name="ban" :size="16" /> 停用</AppButton>
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
                <span v-if="q?.source" class="paper-source-tag">{{ q.source }}</span>
                <AppBadge :color="typeBadgeColor(q?.question_type || '')">{{ typeLabel(q?.question_type || '') }}</AppBadge>
                <span class="paper-difficulty">
                  <AppIcon v-for="n in 5" :key="n" name="star" :size="12" :class="{ active: diffStars >= n }" class="paper-star" />
                </span>
              </div>
              <div class="paper-header-right">
                <span class="pill-badge">
                  <AppIcon :name="statusIcon(q?.status || '')" :size="13" />
                  <span>{{ statusLabel(q?.status || '') }}</span>
                  <span class="pill-divider"></span>
                  <span>{{ q?.default_score }}分</span>
                  <template v-if="q?.grade">
                    <span class="pill-divider"></span>
                    <span>{{ q.grade }}</span>
                  </template>
                  <template v-if="q?.semester">
                    <span class="pill-divider"></span>
                    <span>{{ q.semester }}</span>
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
                <!-- 解答题答案 — 自动注入小问题号徽章 -->
                <div v-else-if="q?.question_type === 'solution'" class="card-answer-content">
                  <div v-for="(ans, i) in (q!.correct_answer as string[])" :key="i" class="sub-answer-item">
                    <span class="sub-question-badge">{{ i + 1 }}</span>
                    <LatexRender :text="ans" :sub-question-badge="true" />
                  </div>
                </div>
                <!-- 判断题答案 -->
                <div v-else-if="q?.question_type === 'judgment'" class="card-answer-content">
                  <span class="paper-judge-tag" :class="q?.correct_answer?.[0] === true ? 'judge-correct' : 'judge-wrong'">
                    {{ q?.correct_answer?.[0] === true ? '正确' : '错误' }}
                  </span>
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
            <div v-if="q?.knowledge_points?.length" class="kp-tags">
              <span v-for="kp in q!.knowledge_points" :key="kp.id" class="kp-tag">{{ kp.name }}</span>
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
import { ref, computed, onMounted, onBeforeUnmount, nextTick, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { questionApi, type QuestionDetail } from '@/api/client'
import client from '@/api/client'
import { useAuthStore } from '@/stores/auth'
import LatexRender from '@/components/LatexRender.vue'
import { AppButton, AppBadge, AppModal, AppConfirm, AppIcon } from '@/components/ui'
import { useToast } from '@/composables/useToast'
import { typeLabel, typeBadgeColor, statusLabel, statusIcon, formatTime } from '@/utils/questionDisplay'

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

// 难度星数
const diffStars = computed(() => {
  const map: Record<string, number> = { easy: 1, medium: 3, hard: 5 }
  return map[q.value?.difficulty || ''] || 0
})

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

// ---- 选择题选项自适应网格布局（与列表页逻辑一致）----
const OPTION_GAP = 16
const optionsContainer = ref<HTMLElement | null>(null)
const optionLayout = ref<'grid-4' | 'grid-2' | 'grid-1'>('grid-2')
let resizeObserver: ResizeObserver | null = null
let layoutTimer: ReturnType<typeof setTimeout> | null = null

const optionLayoutClass = computed(() => optionLayout.value)

function computeOptionLayout() {
  const container = optionsContainer.value
  if (!container) return
  const containerWidth = container.clientWidth
  if (containerWidth === 0) return

  const optionEls = container.querySelectorAll<HTMLElement>('.paper-opt')
  if (optionEls.length === 0) return

  // 临时切换为 block 布局测量真实宽度
  const prevDisplay = container.style.display
  const prevCols = container.style.gridTemplateColumns
  container.style.display = 'block'
  container.style.gridTemplateColumns = ''

  let maxWidth = 0
  const prevStyles: { el: HTMLElement; display: string; width: string }[] = []
  optionEls.forEach(el => {
    prevStyles.push({ el, display: el.style.display, width: el.style.width })
    el.style.display = 'inline-flex'
    el.style.width = 'auto'
    el.style.whiteSpace = 'nowrap'
    const w = el.scrollWidth
    if (w > maxWidth) maxWidth = w
    el.style.whiteSpace = ''
  })

  prevStyles.forEach(({ el, display, width }) => {
    el.style.display = display
    el.style.width = width
  })
  container.style.display = prevDisplay
  container.style.gridTemplateColumns = prevCols

  if (maxWidth === 0) return

  const slot = maxWidth + OPTION_GAP
  if (slot * 4 <= containerWidth) {
    optionLayout.value = 'grid-4'
  } else if (slot * 2 <= containerWidth) {
    optionLayout.value = 'grid-2'
  } else {
    optionLayout.value = 'grid-1'
  }
}

function scheduleCompute() {
  if (layoutTimer) clearTimeout(layoutTimer)
  layoutTimer = setTimeout(() => computeOptionLayout(), 50)
}

// 题目数据变化后计算布局
watch([q, optionList], () => {
  nextTick(() => setTimeout(() => computeOptionLayout(), 120))
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

const isMultiChoice = computed(() => {
  if (q.value?.question_type !== 'choice') return false
  if ((q.value as any)?.sub_type === 'multi') return true
  const ans = q.value?.correct_answer
  return Array.isArray(ans) && ans.length > 1
})

onMounted(async () => {
  await fetchDetail()
  nextTick(() => {
    setTimeout(() => computeOptionLayout(), 150)
    if (optionsContainer.value) {
      resizeObserver = new ResizeObserver(() => scheduleCompute())
      resizeObserver.observe(optionsContainer.value)
    }
  })
})

onBeforeUnmount(() => {
  if (resizeObserver) {
    resizeObserver.disconnect()
    resizeObserver = null
  }
  if (layoutTimer) clearTimeout(layoutTimer)
})
</script>

<style scoped>
/* ============ 页面根容器 ============ */
.detail-page {
  height: 100vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
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
.btn-soft-danger {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 7px 14px;
  font-size: 13px;
  font-weight: 500;
  border-radius: 999px;
  border: none;
  background: #fee2e2;
  color: #dc2626;
  cursor: pointer;
  transition: all 0.2s ease;
  white-space: nowrap;
  letter-spacing: -0.01em;
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

[data-theme='dark'] .btn-soft-danger {
  background: rgba(220, 38, 38, 0.15);
  color: #ff6961;
}

[data-theme='dark'] .btn-soft-danger:hover {
  background: rgba(220, 38, 38, 0.25);
}

/* 操作按钮圆润化 */
.header-actions :deep(.btn) {
  border-radius: 999px;
}

/* 降低成功按钮饱和度 */
.header-actions :deep(.btn-success) {
  background: #34c759;
}

.header-actions :deep(.btn-success:hover) {
  background: #2db84e;
}

/* ============ 内容区：Flex 双栏 ============ */
.detail-body {
  flex: 1;
  display: flex;
  gap: 16px;
  min-height: 0;
  padding: 0 24px 16px;
}

/* 中间滚动区 */
.paper-scroll {
  flex: 1;
  min-width: 0;
  overflow-y: auto;
  padding: 4px 0;
}

/* 右侧滚动区 */
.side-scroll {
  flex: 0 0 280px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 4px 0;
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

.paper-source-tag {
  font-size: 12px;
  font-weight: 600;
  padding: 3px 10px;
  border-radius: 6px;
  background: var(--accent-light);
  color: var(--accent);
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

/* 判断题 */
.paper-judgment {
  margin-bottom: 16px;
}

.paper-judge-tag {
  display: inline-block;
  padding: 4px 16px;
  border-radius: 6px;
  font-weight: 600;
  font-size: 14px;
}

.judge-correct {
  background: var(--success-light);
  color: var(--success);
}

.judge-wrong {
  background: var(--danger-light, rgba(239, 68, 68, 0.1));
  color: var(--danger, #ef4444);
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

/* 卡片标题 — 苹果风格小标题 */
.card-section-title {
  font-size: 13px;
  font-weight: 600;
  color: #86868b;
  margin-bottom: 12px;
  letter-spacing: 0.5px;
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

/* 解答题多小问答案项 */
.sub-answer-item {
  display: flex;
  align-items: flex-start;
  gap: 0;
  margin-bottom: 16px;
  line-height: 1.8;
}

.sub-answer-item:last-child {
  margin-bottom: 0;
}

.sub-answer-item > .sub-question-badge {
  margin-top: 2px;
}

.sub-answer-item :deep(.latex-render) {
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

/* 结论高亮区 */
.paper-conclusion {
  margin-top: 12px;
  padding: 10px 14px;
  border-radius: var(--radius-md);
  background: var(--accent-light);
  border-left: 3px solid var(--accent);
  font-size: 14px;
  color: var(--text-primary);
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
  color: var(--text-muted);
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
  color: var(--text-muted);
}

.meta-val {
  color: var(--text-primary);
  font-weight: 500;
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

/* ============ 响应式 ============ */
@media (max-width: 1024px) {
  .detail-body {
    flex-direction: column;
  }
  .side-scroll {
    flex: none;
    flex-direction: row;
    gap: 14px;
  }
  .side-card {
    flex: 1;
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
