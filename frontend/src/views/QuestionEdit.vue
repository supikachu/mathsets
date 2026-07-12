<template>
  <div class="edit-page">
    <!-- 加载提示 -->
    <template v-if="!isNew && loading">
      <div class="loading-hint">加载中…</div>
    </template>

    <template v-else>
      <!-- ==================== 顶部操作栏 ==================== -->
      <div class="flex items-center justify-between mb-3 flex-shrink-0">
        <div class="flex items-center gap-2">
          <AppButton variant="ghost" size="sm" @click="handleBack"><AppIcon name="chevron-left" :size="17" /> 返回</AppButton>
          <AppButton variant="ghost" size="sm" @click="handleAi"><AppIcon name="sparkles" :size="17" /> AI 智能识别</AppButton>
          <h1 class="edit-title">{{ isNew ? '录入新题' : '编辑题目' }}</h1>
          <AppBadge v-if="!isNew" color="gray">v{{ form.version }}</AppBadge>
        </div>
        <div class="flex items-center gap-2">
          <AppButton v-if="!isNew" variant="ghost" size="sm" @click="showHistory = true"><AppIcon name="history" :size="17" /> 历史版本</AppButton>
          <AppButton variant="outline" size="sm" :loading="saving" :disabled="saving || submitting" @click="handleSave(false)"><AppIcon name="save" :size="17" /> 保存</AppButton>
          <AppButton variant="success" size="sm" :loading="submitting" :disabled="saving || submitting" @click="handleSave(true)"><AppIcon name="send" :size="17" /> 提交审核</AppButton>
        </div>
      </div>

      <!-- ==================== 可折叠属性面板 ==================== -->
      <div class="mb-3 flex-shrink-0">
        <!-- 题目来源 & 知识点 -->
        <div class="collapse-section">
          <button class="collapse-header" @click="toggleCollapse('source')">
            <span><AppIcon name="pin" :size="20" /> 题目来源 & 知识点标签</span>
            <span class="collapse-arrow" :class="{ open: collapse.source }"><AppIcon name="chevron-down" :size="16" /></span>
          </button>
          <div v-show="collapse.source" class="collapse-body">
            <div class="form-grid-2">
              <div>
                <label class="field-label">来源类型</label>
                <AppSelect v-model="form.source" :options="sourceOptions" />
              </div>
              <div>
                <label class="field-label">知识点</label>
                <div class="kp-tags">
                  <AppBadge v-for="kp in selectedKps" :key="kp.id" color="blue">
                    {{ kp.name }}
                    <span class="kp-remove" @click="removeKp(kp.id)"><AppIcon name="x" :size="13" /></span>
                  </AppBadge>
                  <button class="kp-add-btn" @click="showKpDialog = true"><AppIcon name="plus" :size="17" /> 添加标签</button>
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- 基础属性 -->
        <div class="collapse-section">
          <button class="collapse-header" @click="toggleCollapse('basic')">
            <span><AppIcon name="settings" :size="20" /> 基础属性</span>
            <span class="collapse-arrow" :class="{ open: collapse.basic }"><AppIcon name="chevron-down" :size="16" /></span>
          </button>
          <div v-show="collapse.basic" class="collapse-body">
            <div class="form-grid-multi">
              <div>
                <label class="field-label">题型</label>
                <AppSelect v-model="form.question_type" :options="typeOptions" :disabled="!isNew" />
              </div>
              <div>
                <label class="field-label">难度</label>
                <div class="star-rating">
                  <button
                    v-for="n in 3"
                    :key="n"
                    type="button"
                    class="star"
                    :class="{ active: difficultyStars >= n }"
                    @click="difficultyStars = n"
                  ><AppIcon name="star" :size="20" /></button>
                  <span class="star-text">{{ ['简单', '中等', '困难'][difficultyStars - 1] || '' }}</span>
                </div>
              </div>
              <div>
                <label class="field-label">年级</label>
                <AppSelect v-model="form.grade" :options="gradeOptions" clearable />
              </div>
              <div>
                <label class="field-label">学期</label>
                <AppSelect v-model="form.semester" :options="semesterOptions" clearable />
              </div>
              <div>
                <label class="field-label">分值</label>
                <input type="number" v-model.number="form.default_score" min="1" max="100" class="num-input" />
              </div>
              <div>
                <label class="field-label">耗时(分)</label>
                <input type="number" v-model.number="form.estimated_time" min="1" max="60" class="num-input" />
              </div>
            </div>
          </div>
        </div>

        <!-- 协作设置 -->
        <div class="collapse-section">
          <button class="collapse-header" @click="toggleCollapse('collab')">
            <span><AppIcon name="users" :size="20" /> 协作设置</span>
            <span class="collapse-arrow" :class="{ open: collapse.collab }"><AppIcon name="chevron-down" :size="16" /></span>
          </button>
          <div v-show="collapse.collab" class="collapse-body">
            <div class="form-grid-2">
              <div>
                <label class="field-label">指定审题人</label>
                <template v-if="isTeamSpace">
                  <div v-if="spaceMembers.length === 0" class="text-sm text-muted">暂无其他团队成员</div>
                  <div v-else class="reviewer-checkboxes">
                    <label v-for="m in spaceMembers.filter(m => m.user_id !== auth.userId)" :key="m.user_id" class="reviewer-item">
                      <input type="checkbox" :value="m.user_id" v-model="form.reviewer_ids" />
                      <span>{{ m.display_name }} ({{ m.username }})</span>
                    </label>
                  </div>
                  <div class="text-sm text-muted" style="margin-top: 4px">不选则由团队其他成员审题</div>
                </template>
                <div v-else class="text-sm text-muted">个人空间默认自审，无需指定</div>
              </div>
              <div>
                <label class="field-label">内部备注（仅审核员可见）</label>
                <input v-model="form.internal_note" placeholder="记录命题意图或讨论要点…" />
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- ==================== 三组编辑/预览双栏 ==================== -->
      <div class="dual-sections">
        <!-- 题干 -->
        <div class="dual-row" ref="rowRefs[0]">
          <div class="dual-edit" :style="{ flex: `0 0 ${splitRatio * 100}%` }">
            <div class="dual-label"><AppIcon name="book-open" :size="20" /> 题干 *</div>
            <textarea v-model="form.stem" rows="5" class="edit-textarea" placeholder="输入题目内容，LaTeX 公式用 $...$ 包裹"></textarea>
          </div>
          <div class="dual-divider" @mousedown.prevent="startResize(0, $event)" />
          <div class="dual-preview" :style="{ flex: `0 0 ${(1 - splitRatio) * 100}%` }">
            <div class="dual-label">题干预览</div>
            <div class="preview-box"><LatexRender :text="form.stem || '（等待输入…）'" /></div>
          </div>
        </div>

        <!-- 答案 -->
        <div class="dual-row" ref="rowRefs[1]">
          <div class="dual-edit" :style="{ flex: `0 0 ${splitRatio * 100}%` }">
            <div class="dual-label"><AppIcon name="file-text" :size="20" /> 答案</div>
            <!-- 选择题选项 -->
            <div v-if="form.question_type === 'choice'">
              <div v-for="(opt, i) in form.options" :key="i" class="opt-row">
                <label class="radio-label" :class="{ checked: form.correctAnswer === opt.label }">
                  <input type="radio" :value="opt.label" v-model="form.correctAnswer" />
                  {{ opt.label }}
                </label>
                <input v-model="opt.content" :placeholder="`选项 ${opt.label}`" class="opt-input" />
                <AppButton v-if="form.options.length > 2" variant="ghost" size="sm" @click="form.options.splice(i, 1)"><AppIcon name="x" :size="17" /></AppButton>
              </div>
              <AppButton variant="outline" size="sm" @click="addOption"><AppIcon name="plus" :size="17" /> 添加选项</AppButton>
            </div>
            <div v-else-if="form.question_type === 'fill'">
              <div v-for="(blank, i) in form.blanks" :key="i" class="opt-row">
                <span class="blank-label">第{{ i+1 }}空</span>
                <input v-model="blank.answer" placeholder="填入答案" class="opt-input" />
                <AppButton v-if="form.blanks.length > 1" variant="ghost" size="sm" @click="form.blanks.splice(i, 1)"><AppIcon name="x" :size="17" /></AppButton>
              </div>
              <AppButton variant="outline" size="sm" @click="form.blanks.push({ position: Math.max(...form.blanks.map(b => b.position), 0) + 1, answer: '' })"><AppIcon name="plus" :size="17" /> 添加填空位</AppButton>
            </div>
            <div v-else-if="form.question_type === 'solution'">
              <textarea v-model="form.solutionAnswer" rows="3" class="edit-textarea" placeholder="完整解答过程，支持 $...$ LaTeX"></textarea>
              <div class="grading-label">分步评分</div>
              <div v-for="(step, i) in form.gradingSteps" :key="i" class="opt-row">
                <input v-model="step.label" placeholder="步骤名" class="step-input" />
                <input type="number" v-model.number="step.points" min="0" max="20" class="num-input" />
                <span class="text-muted text-sm">分</span>
                <AppButton v-if="form.gradingSteps.length > 1" variant="ghost" size="sm" @click="form.gradingSteps.splice(i, 1)"><AppIcon name="x" :size="17" /></AppButton>
              </div>
              <AppButton variant="outline" size="sm" @click="form.gradingSteps.push({ label: '', points: 1, description: '' })"><AppIcon name="plus" :size="17" /> 添加评分步骤</AppButton>
            </div>
            <div v-else-if="form.question_type === 'judgment'">
              <div class="radio-group">
                <label class="radio-label" :class="{ checked: form.judgmentCorrect === true }">
                  <input type="radio" :value="true" v-model="form.judgmentCorrect" />
                  正确
                </label>
                <label class="radio-label" :class="{ checked: form.judgmentCorrect === false }">
                  <input type="radio" :value="false" v-model="form.judgmentCorrect" />
                  错误
                </label>
              </div>
            </div>
          </div>
          <div class="dual-divider" @mousedown.prevent="startResize(1, $event)" />
          <div class="dual-preview" :style="{ flex: `0 0 ${(1 - splitRatio) * 100}%` }">
            <div class="dual-label">答案预览</div>
            <div class="preview-box">
              <div v-if="form.question_type === 'choice'">
                <div
                  v-for="opt in form.options.filter(o => o.content)"
                  :key="opt.label"
                  class="preview-opt"
                  :class="{ correct: form.correctAnswer === opt.label }"
                >
                  <span class="opt-letter">{{ opt.label }}.</span>
                  <LatexRender :text="opt.content" :inline="true" />
                  <AppBadge v-if="form.correctAnswer === opt.label" color="green"><AppIcon name="check" :size="13" /></AppBadge>
                </div>
              </div>
              <div v-else-if="form.question_type === 'judgment'">
                <AppBadge :color="form.judgmentCorrect ? 'green' : 'red'">
                  {{ form.judgmentCorrect ? '正确' : '错误' }}
                </AppBadge>
              </div>
              <LatexRender v-else-if="form.solutionAnswer" :text="form.solutionAnswer" />
              <span v-else class="text-muted">（等待输入…）</span>
            </div>
          </div>
        </div>

        <!-- 解析 -->
        <div class="dual-row" ref="rowRefs[2]">
          <div class="dual-edit" :style="{ flex: `0 0 ${splitRatio * 100}%` }">
            <div class="dual-label"><AppIcon name="lightbulb" :size="20" /> 解析</div>
            <textarea v-model="form.analysis" rows="4" class="edit-textarea" placeholder="解题思路与易错点，支持 $...$ LaTeX"></textarea>
          </div>
          <div class="dual-divider" @mousedown.prevent="startResize(2, $event)" />
          <div class="dual-preview" :style="{ flex: `0 0 ${(1 - splitRatio) * 100}%` }">
            <div class="dual-label">解析预览</div>
            <div class="preview-box"><LatexRender :text="form.analysis || '（等待输入…）'" /></div>
          </div>
        </div>
      </div>
    </template>

    <!-- 版本历史弹窗 -->
    <AppModal v-model="showHistory" title="历史版本">
      <div class="loading-hint">版本历史功能即将上线</div>
    </AppModal>

    <!-- 知识点选择弹窗 -->
    <AppModal v-model="showKpDialog" title="选择知识点">
      <div class="kp-dialog-tree">
        <div v-for="node in kpTree" :key="node.id" class="mb-8">
          <label class="checkbox-label">
            <input
              type="checkbox"
              :value="node.id"
              :checked="form.knowledgePointIds.includes(node.id)"
              @change="toggleKp(node.id)"
            />
            <b>{{ node.name }}</b>
          </label>
          <div v-if="node.children?.length" class="kp-dialog-children">
            <label v-for="c in node.children" :key="c.id" class="checkbox-label">
              <input
                type="checkbox"
                :value="c.id"
                :checked="form.knowledgePointIds.includes(c.id)"
                @change="toggleKp(c.id)"
              />
              {{ c.name }}
            </label>
          </div>
        </div>
        <AppEmpty v-if="!kpLoading && kpTree.length === 0" description="暂无知识点" />
      </div>
      <div class="form-actions">
        <AppButton variant="primary" @click="showKpDialog = false">完成</AppButton>
      </div>
    </AppModal>

    <!-- 离开确认 -->
    <AppConfirm
      v-model="leaveDialog"
      title="未保存提示"
      message="有未保存的修改，确定离开吗？"
      confirm-text="离开"
      danger
      @confirm="goBack"
    />

    <!-- 草稿恢复确认 -->
    <AppConfirm
      v-model="restoreDialog"
      title="恢复草稿"
      message="检测到未保存的草稿，是否恢复？"
      confirm-text="恢复"
      cancel-text="丢弃"
      @confirm="doRestoreDraft"
      @update:model-value="(v: boolean) => { if (!v) discardDraft() }"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, computed, watch, onMounted, onBeforeUnmount } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { questionApi, kpApi, spaceApi, type KnowledgePoint, type SpaceMemberInfo } from '@/api/client'
import LatexRender from '@/components/LatexRender.vue'
import { AppButton, AppBadge, AppModal, AppConfirm, AppEmpty, AppSelect, AppIcon } from '@/components/ui'
import { useToast } from '@/composables/useToast'
import { useSpaceStore } from '@/stores/space'
import { useAuthStore } from '@/stores/auth'

const route = useRoute()
const router = useRouter()
const toast = useToast()
const space = useSpaceStore()
const auth = useAuthStore()
const isNew = route.path.endsWith('/new')
const loading = ref(false)
const saving = ref(false)
const submitting = ref(false)
const isLoading = ref(false)
const kpLoading = ref(false)
const kpTree = ref<KnowledgePoint[]>([])
const showHistory = ref(false)
const showKpDialog = ref(false)
const grades = ['初一', '初二', '初三', '高一', '高二', '高三']

const gradeOptions = grades.map((g) => ({ label: g, value: g }))
const sourceOptions = [
  { label: '原创', value: '原创' },
  { label: '改编', value: '改编' },
  { label: '高考真题', value: '高考真题' },
  { label: '模拟题', value: '模拟题' },
  { label: '名校试卷', value: '名校试卷' },
]
const typeOptions = [
  { label: '选择题', value: 'choice' },
  { label: '填空题', value: 'fill' },
  { label: '解答题', value: 'solution' },
  { label: '判断题', value: 'judgment' },
]
const semesterOptions = [
  { label: '上学期', value: '上学期' },
  { label: '下学期', value: '下学期' },
  { label: '全学年', value: '全学年' },
]
const reviewerOptions = ref<{ label: string; value: string }[]>([])
const spaceMembers = ref<SpaceMemberInfo[]>([])

// 当前空间是否为团队空间（团队空间才显示审题人选择）
const isTeamSpace = computed(() => space.currentSpace?.kind === 'team')

// 可折叠面板
const collapse = reactive({
  source: true,
  basic: true,
  collab: true,
})
function toggleCollapse(key: keyof typeof collapse) {
  collapse[key] = !collapse[key]
}

// 可拖拽分隔条
const splitRatio = ref(0.55)
const isDragging = ref(false)
const currentRow = ref(-1)
const rowRefs = [ref<HTMLElement>(), ref<HTMLElement>(), ref<HTMLElement>()]

function startResize(rowIdx: number, _e: MouseEvent) {
  isDragging.value = true
  currentRow.value = rowIdx
  document.body.style.cursor = 'col-resize'
  document.body.style.userSelect = 'none'
  document.addEventListener('mousemove', onMouseMove)
  document.addEventListener('mouseup', stopResize)
}

function onMouseMove(e: MouseEvent) {
  if (!isDragging.value) return
  const idx = currentRow.value
  if (idx < 0 || idx >= rowRefs.length) return
  const el = rowRefs[idx]?.value
  if (!el) return
  const rect = el.getBoundingClientRect()
  const x = e.clientX - rect.left
  let ratio = x / rect.width
  ratio = Math.max(0.2, Math.min(0.8, ratio))
  splitRatio.value = ratio
}

function stopResize() {
  isDragging.value = false
  currentRow.value = -1
  document.body.style.cursor = ''
  document.body.style.userSelect = ''
  document.removeEventListener('mousemove', onMouseMove)
  document.removeEventListener('mouseup', stopResize)
}

// 已选知识点名称映射
const kpMap = ref<Record<string, string>>({})
const selectedKps = computed(() =>
  form.knowledgePointIds.map(id => ({ id, name: kpMap.value[id] || id.substring(0, 8) }))
)
function removeKp(id: string) {
  form.knowledgePointIds = form.knowledgePointIds.filter(k => k !== id)
}
function toggleKp(id: string) {
  const idx = form.knowledgePointIds.indexOf(id)
  if (idx >= 0) {
    form.knowledgePointIds.splice(idx, 1)
  } else {
    form.knowledgePointIds.push(id)
  }
}

// 难度映射
const diffMap: Record<string, number> = { easy: 1, medium: 2, hard: 3 }
const starMap: Record<number, string> = { 1: 'easy', 2: 'medium', 3: 'hard' }
const difficultyStars = computed({
  get: () => diffMap[form.difficulty] || 2,
  set: (v: number) => { form.difficulty = starMap[v] || 'medium' },
})

const form = reactive({
  stem: '',
  question_type: 'choice',
  difficulty: 'medium',
  default_score: 5,
  grade: undefined as string | undefined,
  semester: undefined as string | undefined,
  source: '原创',
  estimated_time: 5,
  analysis: '',
  options: [
    { label: 'A', content: '' },
    { label: 'B', content: '' },
    { label: 'C', content: '' },
    { label: 'D', content: '' },
  ] as { label: string; content: string }[],
  correctAnswer: '' as string | string[],
  blanks: [{ position: 1, answer: '' }] as { position: number; answer: string }[],
  solutionAnswer: '',
  gradingSteps: [] as { label: string; points: number; description: string }[],
  judgmentCorrect: true,
  knowledgePointIds: [] as string[],
  reviewer: '' as string,
  reviewer_ids: [] as string[],
  internal_note: '',
  status: '',
  version: 1,
  hasUnsaved: false,
})

// ===== 返回检测 =====
const leaveDialog = ref(false)
function handleBack() {
  if (form.hasUnsaved) {
    leaveDialog.value = true
  } else {
    goBack()
  }
}
function goBack() {
  if (isNew) router.push('/questions')
  else router.push(`/questions/${route.params.id}`)
}

// ===== AI 识别（预留） =====
function handleAi() {
  toast.info('AI 智能识别功能即将上线')
}

// ===== 选项增删 =====
function addOption() {
  const labels = 'ABCDEFGH'
  const i = form.options.length
  if (i < 8) form.options.push({ label: labels[i], content: '' })
}

// ===== 构建提交数据 =====
function buildPayload() {
  const payload: any = {
    stem: form.stem,
    question_type: form.question_type,
    difficulty: form.difficulty,
    default_score: form.default_score,
    grade: form.grade || null,
    semester: form.semester || null,
    source: form.source,
    analysis: form.analysis || null,
    knowledge_point_ids: form.knowledgePointIds.length > 0 ? form.knowledgePointIds : null,
  }
  switch (form.question_type) {
    case 'choice':
      payload.options = form.options.filter(o => o.content.trim())
      payload.correct_answer = form.correctAnswer ? [form.correctAnswer] : []
      break
    case 'fill':
      payload.correct_answer = form.blanks.filter(b => b.answer.trim()).map(b => ({ position: b.position, answer: b.answer.trim() }))
      break
    case 'solution':
      payload.correct_answer = form.solutionAnswer ? [form.solutionAnswer] : []
      if (form.gradingSteps.length > 0) payload.grading_criteria = form.gradingSteps.filter(s => s.label)
      break
    case 'judgment':
      payload.correct_answer = [form.judgmentCorrect]
      break
  }
  return payload
}

// ===== 保存 =====
async function handleSave(submitAfter: boolean) {
  if (!form.stem.trim()) { toast.warning('请输入题干'); return }
  if (form.question_type === 'choice' && !form.correctAnswer) { toast.warning('请选择正确答案'); return }
  const flag = submitAfter ? submitting : saving
  flag.value = true
  try {
    const data = buildPayload()
    const res = isNew ? await questionApi.create(data) : await questionApi.update(route.params.id as string, data)
    const qid = res.data.id
    form.hasUnsaved = false
    clearDraft()
    if (submitAfter) {
      await questionApi.submit(qid, { reviewer_ids: form.reviewer_ids.length > 0 ? form.reviewer_ids : undefined })
      toast.success('已创建并提交审核')
    }
    else { toast.success(isNew ? '草稿已保存' : '已更新') }
    router.push(`/questions/${qid}`)
  } catch (e: any) { toast.error(e.response?.data?.error || '操作失败') }
  finally { flag.value = false }
}

// ===== 自动保存草稿 =====
let autoSaveTimer: ReturnType<typeof setTimeout> | null = null
watch(() => ({ ...form }), () => {
  if (isLoading.value) return
  form.hasUnsaved = true
  if (autoSaveTimer) clearTimeout(autoSaveTimer)
  autoSaveTimer = setTimeout(() => {
    try {
      const key = isNew ? 'q-draft-new' : `q-draft-${route.params.id}`
      sessionStorage.setItem(key, JSON.stringify(form))
    } catch { /* quota exceeded */ }
  }, 3000)
}, { deep: true })

// ===== 自动草稿恢复 =====
const restoreDialog = ref(false)
let pendingDraft: any = null

function getDraftKey() {
  return isNew ? 'q-draft-new' : `q-draft-${route.params.id}`
}

function restoreDraft() {
  const key = getDraftKey()
  try {
    const saved = sessionStorage.getItem(key)
    if (!saved) return
    const draft = JSON.parse(saved)
    if (draft.stem || draft.analysis || draft.solutionAnswer) {
      pendingDraft = draft
      restoreDialog.value = true
    }
  } catch { /* ignore */ }
}

function doRestoreDraft() {
  if (!pendingDraft) return
  const fields = ['stem', 'question_type', 'difficulty', 'default_score', 'grade', 'semester',
    'source', 'analysis', 'options', 'correctAnswer', 'blanks', 'solutionAnswer',
    'gradingSteps', 'judgmentCorrect', 'knowledgePointIds', 'reviewer', 'reviewer_ids', 'internal_note']
  for (const f of fields) {
    if (pendingDraft[f] !== undefined) (form as any)[f] = pendingDraft[f]
  }
  toast.success('草稿已恢复')
  pendingDraft = null
}

function discardDraft() {
  try { sessionStorage.removeItem(getDraftKey()) } catch { /* ignore */ }
  pendingDraft = null
}

function clearDraft() {
  try { sessionStorage.removeItem(getDraftKey()) }
  catch { /* ignore */ }
}

async function loadKpTree() {
  kpLoading.value = true
  try {
    const res = await kpApi.tree(); kpTree.value = res.data
    function walk(nodes: KnowledgePoint[]) {
      for (const n of nodes) { kpMap.value[n.id] = n.name; if (n.children) walk(n.children) }
    }
    walk(res.data)
  } catch { /* handled */ }
  finally { kpLoading.value = false }
}

async function loadSpaceMembers() {
  if (!isTeamSpace.value || !space.currentSpaceId) return
  try {
    const res = await spaceApi.get(space.currentSpaceId)
    spaceMembers.value = res.data.members || []
  } catch { /* handled */ }
}

async function loadQuestion() {
  if (isNew) return
  isLoading.value = true
  loading.value = true
  try {
    const res = await questionApi.get(route.params.id as string)
    const d = res.data
    form.stem = d.stem
    form.question_type = d.question_type
    form.difficulty = d.difficulty
    form.default_score = d.default_score
    form.grade = d.grade || undefined
    form.semester = d.semester || undefined
    form.source = d.source || '原创'
    form.analysis = d.analysis || ''
    form.status = d.status
    form.version = d.version
    form.knowledgePointIds = d.knowledge_points?.map(k => k.id) || []
    form.correctAnswer = ''
    form.blanks = [{ position: 1, answer: '' }]
    form.solutionAnswer = ''
    form.gradingSteps = []
    form.judgmentCorrect = true
    if (d.question_type === 'choice' && d.options) {
      form.options = d.options as any
      if (Array.isArray(d.correct_answer)) form.correctAnswer = d.correct_answer[0] || ''
    } else if (d.question_type === 'fill' && Array.isArray(d.correct_answer)) {
      form.blanks = (d.correct_answer as any[]).map((b: any) => ({ position: b.position, answer: b.answer }))
    } else if (d.question_type === 'solution') {
      if (Array.isArray(d.correct_answer)) form.solutionAnswer = d.correct_answer[0] || ''
      if (d.grading_criteria) form.gradingSteps = d.grading_criteria as any
    } else if (d.question_type === 'judgment') {
      if (Array.isArray(d.correct_answer)) form.judgmentCorrect = d.correct_answer[0] === true
    }
    form.hasUnsaved = false
  } catch { /* handled */ }
  finally { loading.value = false; isLoading.value = false }
}

// ===== 窗口关闭检测 =====
function handleBeforeUnload(e: BeforeUnloadEvent) {
  if (form.hasUnsaved) { e.preventDefault(); e.returnValue = '' }
}
onMounted(() => {
  window.addEventListener('beforeunload', handleBeforeUnload)
  loadKpTree()
  loadSpaceMembers()
  loadQuestion().then(() => {
    if (!isNew) restoreDraft()
  })
  if (isNew) restoreDraft()
})
onBeforeUnmount(() => {
  window.removeEventListener('beforeunload', handleBeforeUnload)
  if (autoSaveTimer) clearTimeout(autoSaveTimer)
  stopResize()
})

watch(() => form.question_type, () => {
  if (isNew) {
    form.correctAnswer = ''
    form.blanks = [{ position: 1, answer: '' }]
    form.solutionAnswer = ''
    form.gradingSteps = []
    form.judgmentCorrect = true
  }
})
</script>

<style scoped>
.edit-page {
  height: 100vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  padding: 20px 24px;
}

.edit-title {
  font-size: 18px;
  font-weight: 700;
  margin-left: 4px;
  color: var(--text-primary);
}

.loading-hint {
  text-align: center;
  padding: 48px 20px;
  color: var(--text-muted);
}

/* 折叠面板 */
.collapse-section {
  background: var(--bg-card);
  border-radius: var(--radius-md);
  margin-bottom: 8px;
  box-shadow: var(--shadow-sm);
  border: 1px solid var(--border-color);
  overflow: hidden;
}

.collapse-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  width: 100%;
  padding: 12px 16px;
  background: none;
  border: none;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
  cursor: pointer;
  transition: var(--transition);
}

.collapse-header:hover {
  background: var(--bg-hover);
}

.collapse-arrow {
  transition: transform 0.2s;
  transform: rotate(-90deg);
  color: var(--text-muted);
  font-size: 12px;
}

.collapse-arrow.open {
  transform: rotate(0deg);
}

.collapse-body {
  padding: 0 16px 16px;
}

.field-label {
  display: block;
  font-size: 12px;
  font-weight: 600;
  margin-bottom: 4px;
  color: var(--text-muted);
}

.form-grid-2 {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 16px;
}

.reviewer-checkboxes {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 120px;
  overflow-y: auto;
}

.reviewer-item {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  cursor: pointer;
}

.reviewer-item input[type="checkbox"] {
  width: auto;
}

.form-grid-multi {
  display: grid;
  grid-template-columns: repeat(6, 1fr);
  gap: 12px;
}

@media (max-width: 768px) {
  .form-grid-2,
  .form-grid-multi {
    grid-template-columns: 1fr;
  }
}

.num-input {
  width: 100%;
  padding: 6px 10px;
  border-radius: var(--radius-sm);
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  color: var(--text-primary);
  font-size: 13px;
}

.num-input:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
}

/* 星级评分 */
.star-rating {
  display: flex;
  align-items: center;
  gap: 4px;
}

.star {
  font-size: 20px;
  color: var(--border-color);
  background: none;
  border: none;
  cursor: pointer;
  padding: 0;
  transition: var(--transition);
}

.star.active {
  color: var(--star-color);
}

.star-text {
  font-size: 12px;
  color: var(--text-muted);
  margin-left: 6px;
}

/* 知识点标签 */
.kp-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  align-items: center;
}

.kp-remove {
  margin-left: 4px;
  cursor: pointer;
  opacity: 0.6;
}

.kp-remove:hover {
  opacity: 1;
}

.kp-add-btn {
  background: none;
  border: 1px dashed var(--border-color);
  border-radius: 20px;
  padding: 3px 10px;
  font-size: 12px;
  color: var(--text-secondary);
  cursor: pointer;
  transition: var(--transition);
}

.kp-add-btn:hover {
  border-color: var(--accent);
  color: var(--accent);
}

/* 双栏区域滚动 */
.dual-sections {
  flex: 1;
  overflow-y: auto;
  padding-right: 4px;
}

.dual-row {
  display: flex;
  gap: 0;
  margin-bottom: 12px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  overflow: hidden;
  background: var(--bg-card);
}

.dual-edit {
  flex: 1;
  padding: 12px;
  min-width: 30%;
}

.dual-preview {
  flex: 1;
  padding: 12px;
  min-width: 30%;
  background: var(--bg-input);
  border-left: 1px solid var(--border-color);
}

.dual-divider {
  width: 4px;
  cursor: col-resize;
  background: var(--bg-hover);
  flex-shrink: 0;
  transition: background 0.15s;
}

.dual-divider:hover {
  background: var(--accent);
}

.dual-label {
  font-size: 13px;
  font-weight: 600;
  margin-bottom: 8px;
  color: var(--text-primary);
}

.preview-box {
  font-size: 14px;
  line-height: 1.8;
  min-height: 60px;
}

.edit-textarea {
  width: 100%;
  padding: 10px 12px;
  border-radius: var(--radius-sm);
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  color: var(--text-primary);
  font-size: 13px;
  font-family: 'Courier New', monospace;
  resize: vertical;
}

.edit-textarea:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
}

/* 选项行 */
.opt-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
}

.opt-input {
  flex: 1;
  padding: 6px 10px;
  border-radius: var(--radius-sm);
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  color: var(--text-primary);
  font-size: 13px;
}

.opt-input:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
}

.step-input {
  width: 130px;
  padding: 6px 10px;
  border-radius: var(--radius-sm);
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  color: var(--text-primary);
  font-size: 13px;
}

.step-input:focus {
  outline: none;
  border-color: var(--accent);
}

.blank-label {
  font-size: 12px;
  color: var(--text-muted);
  width: 40px;
  flex-shrink: 0;
}

.grading-label {
  font-size: 12px;
  font-weight: 600;
  margin-top: 8px;
  margin-bottom: 6px;
  color: var(--text-secondary);
}

/* Radio */
.radio-group {
  display: flex;
  gap: 16px;
}

.radio-label {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 14px;
  cursor: pointer;
  padding: 4px 12px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border-color);
  transition: var(--transition);
}

.radio-label.checked {
  border-color: var(--accent);
  background: var(--accent-light);
  color: var(--accent);
}

.radio-label input {
  margin: 0;
}

/* 预览选项 */
.preview-opt {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 10px;
  margin-bottom: 6px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border-color);
  font-size: 14px;
}

.preview-opt.correct {
  border-color: var(--success);
  background: var(--success-light);
}

.opt-letter {
  font-family: monospace;
  font-weight: 600;
}

/* 知识点弹窗树 */
.kp-dialog-tree {
  max-height: 300px;
  overflow-y: auto;
}

.kp-dialog-children {
  margin-left: 24px;
}

.checkbox-label {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 14px;
  padding: 4px 0;
  cursor: pointer;
}

.checkbox-label input {
  margin: 0;
}
</style>
