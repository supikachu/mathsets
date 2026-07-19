<template>
  <div class="edit-page">
    <!-- 加载提示 -->
    <template v-if="!isNew && loading">
      <div class="loading-hint">加载中…</div>
    </template>

    <template v-else>
      <!-- ==================== 顶部操作栏 ==================== -->
      <header class="top-bar">
        <div class="top-bar-left">
          <AppButton variant="ghost" size="sm" @click="handleBack"><AppIcon name="chevron-left" :size="17" /> 返回</AppButton>
          <AppButton variant="ghost" size="sm" @click="handleAi"><AppIcon name="sparkles" :size="17" /> AI 智能识别</AppButton>
          <h1 class="edit-title">{{ isNew ? '录入新题' : '编辑题目' }}</h1>
          <AppBadge v-if="!isNew" color="gray">v{{ form.version }}</AppBadge>
        </div>
        <div class="top-bar-right">
          <AppButton v-if="!isNew" variant="ghost" size="sm" @click="showHistory = true"><AppIcon name="history" :size="17" /> 历史版本</AppButton>
          <AppButton variant="outline" size="sm" :loading="saving" :disabled="saving || submitting" @click="handleSave(false)"><AppIcon name="save" :size="17" /> 保存</AppButton>
          <AppButton variant="success" size="sm" :loading="submitting" :disabled="saving || submitting" @click="handleSave(true)"><AppIcon name="send" :size="17" /> 提交审核</AppButton>
        </div>
      </header>

      <!-- ==================== 批量录题 Tab 切换栏（仅 questionList.length > 1 显示）==================== -->
      <div v-if="questionList.length > 1" class="batch-tabs">
        <div class="batch-tabs-track">
          <button
            v-for="(item, idx) in questionList"
            :key="idx"
            class="batch-tab"
            :class="{ 'is-active': idx === activeIndex }"
            @click="switchToTab(idx)"
          >
            <span class="batch-tab-index">题目 {{ idx + 1 }}</span>
            <span v-if="item.stem" class="batch-tab-preview">{{ stripStemPreview(item.stem) }}</span>
            <span v-else class="batch-tab-empty">（空）</span>
          </button>
        </div>
      </div>

      <!-- ==================== 第一层：核心控制元数据栏 ==================== -->
      <MetaBar
        v-model:questionType="form.question_type"
        v-model:difficulty="form.difficulty"
        v-model:difficultyCoefficient="form.difficulty_coefficient"
        v-model:academicYear="form.academic_year"
        v-model:gradeSemester="form.grade_semester"
        v-model:examType="form.exam_type"
        v-model:examRegion="form.exam_region"
      />

      <!-- ==================== 主内容 双栏 ==================== -->
      <div class="main-content">
        <!-- 左栏：编辑 -->
        <div class="edit-col">
          <div class="edit-col-inner">
            <!-- ==================== 第二层：描述性标签流 ==================== -->
            <div class="question-tags-wrapper">
              <span v-if="form.exam_region" class="attr-tag">
                <AppIcon name="pin" :size="11" />
                <span class="attr-tag-text">{{ form.exam_region }}</span>
                <button type="button" class="attr-tag-x" @click="form.exam_region = ''"><AppIcon name="x" :size="10" /></button>
              </span>
              <span
                v-for="(kp, idx) in attrSelectedKps"
                :key="'kp-tag-' + kp.id"
                class="attr-tag attr-tag-kp"
                :class="{ 'attr-tag-kp-primary': idx === 0 }"
              >
                <AppIcon name="tag" :size="11" />
                <span class="attr-tag-text">{{ kp.name }}</span>
                <button type="button" class="attr-tag-x" @click="removeAttrKp(kp.id)"><AppIcon name="x" :size="10" /></button>
              </span>
              <span v-for="t in selectedCompetenceTags" :key="'comp-' + t.id" class="attr-tag attr-tag-literacy">
                <AppIcon name="award" :size="11" />
                <span class="attr-tag-text">{{ t.name }}</span>
                <button type="button" class="attr-tag-x" @click="toggleTagById(t)"><AppIcon name="x" :size="10" /></button>
              </span>
              <span v-for="t in selectedMethodTags" :key="'method-' + t.id" class="attr-tag attr-tag-method">
                <AppIcon name="bookmark" :size="11" />
                <span class="attr-tag-text">{{ t.name }}</span>
                <button type="button" class="attr-tag-x" @click="toggleTagById(t)"><AppIcon name="x" :size="10" /></button>
              </span>
              <span v-for="t in selectedSchoolTags" :key="'school-' + t.id" class="attr-tag attr-tag-method">
                <AppIcon name="bookmark" :size="11" />
                <span class="attr-tag-text">{{ t.name }}</span>
                <button type="button" class="attr-tag-x" @click="toggleTagById(t)"><AppIcon name="x" :size="10" /></button>
              </span>
              <button type="button" class="attr-add-btn" @click="showAttrDialog = true">
                <AppIcon name="plus" :size="13" />
                <span>添加属性</span>
              </button>
            </div>

            <!-- 题干 -->
            <section class="edit-section" :class="{ 'ai-highlight': aiGeneratedFields.has('stem') }">
              <div class="section-label"><AppIcon name="book-open" :size="16" /> <span>题干</span><span class="required">*</span></div>
              <div class="stem-wrap">
                <textarea ref="stemTextareaRef" v-model="form.stem" rows="4" class="edit-textarea stem-textarea" placeholder="输入题目内容，LaTeX 公式用 $...$ 包裹。例如：已知集合 $A = \{x | x^2 - 2x = 0\}$..." @input="autoResize"></textarea>
                <button type="button" class="img-upload-btn" @click="handleImageUpload">
                  <AppIcon name="paperclip" :size="13" />
                  <span>上传配图</span>
                </button>
              </div>
            </section>

            <!-- 答案 -->
            <section class="edit-section" :class="{ 'ai-highlight': aiGeneratedFields.has('options') || aiGeneratedFields.has('blanks') || aiGeneratedFields.has('sub_answers') }">
              <div class="section-label">
                <AppIcon name="file-text" :size="16" /> <span>答案</span>
                <div v-if="form.question_type === 'choice'" class="seg-toggle">
                  <button type="button" class="seg-btn" :class="{ active: form.sub_type !== 'multi' }" @click="switchChoiceMode('single')">单选</button>
                  <button type="button" class="seg-btn" :class="{ active: form.sub_type === 'multi' }" @click="switchChoiceMode('multi')">多选</button>
                </div>
              </div>
              <!-- 选择题选项 -->
              <EditFormChoice
                v-if="form.question_type === 'choice'"
                v-model:options="form.options"
                v-model:correctAnswer="form.correctAnswer"
                v-model:subType="form.sub_type"
              />
              <!-- 填空题 -->
              <EditFormFill
                v-else-if="form.question_type === 'fill'"
                v-model:blanks="form.blanks"
              />
              <!-- 解答题 -->
              <EditFormSolution
                v-else-if="form.question_type === 'solution'"
                v-model:subAnswers="form.sub_answers"
              />
            </section>

            <!-- 解析（多解法） -->
            <section class="edit-section" :class="{ 'ai-highlight': aiGeneratedFields.has('solutions') }">
              <div class="section-label"><AppIcon name="lightbulb" :size="16" /> <span>解析</span></div>
              <div class="solutions-list">
                <div v-for="(sol, i) in form.solutions" :key="i" class="solution-item">
                  <div class="solution-head">
                    <span class="solution-name">解法{{ cnNum(i + 1) }}</span>
                    <button v-if="form.solutions.length > 1" class="solution-del" @click="removeSolution(i)" title="删除此解法">
                      <AppIcon name="trash-2" :size="14" />
                    </button>
                  </div>
                  <div class="solution-textarea-wrap">
                    <textarea
                      v-model="form.solutions[i]"
                      rows="6"
                      class="edit-textarea solution-textarea"
                      :placeholder="`解法${cnNum(i + 1)}的解题思路，支持 $...$ LaTeX`"
                      @input="autoResize"
                    ></textarea>
                    <button type="button" class="img-upload-btn" @click="handleSolutionImageUpload(i)">
                      <AppIcon name="paperclip" :size="13" />
                      <span>上传配图</span>
                    </button>
                  </div>
                </div>
              </div>
              <button class="add-solution-btn" @click="addSolution">
                <AppIcon name="plus" :size="15" /> 添加新解法
              </button>
            </section>

            <!-- 高级设置 -->
            <section class="advanced-section">
              <button class="advanced-header" @click="toggleCollapse('collab')">
                <span class="advanced-title"><AppIcon name="users" :size="16" /> 高级设置 · 协作</span>
                <span class="collapse-arrow" :class="{ open: !collapse.collab }"><AppIcon name="chevron-down" :size="16" /></span>
              </button>
              <div v-show="!collapse.collab" class="advanced-body">
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
                      <div class="text-sm text-muted hint-line">不选则由团队其他成员审题</div>
                    </template>
                    <div v-else class="text-sm text-muted">个人空间默认自审，无需指定</div>
                  </div>
                  <div>
                    <label class="field-label">内部备注（仅审核员可见）</label>
                    <input v-model="form.internal_note" placeholder="记录命题意图或讨论要点…" class="text-input" />
                  </div>
                </div>
              </div>
            </section>
          </div>
        </div>

        <!-- 右栏：试卷化预览 -->
        <LivePreviewCard :form="form" />
      </div>
    </template>

    <!-- 版本历史弹窗 -->
    <AppModal v-model="showHistory" title="历史版本">
      <div class="loading-hint">版本历史功能即将上线</div>
    </AppModal>

    <!-- 属性编辑面板 -->
    <AttributeModal
      v-model="showAttrDialog"
      v-model:tagIds="form.tagIds"
      v-model:attrSelectedKps="attrSelectedKps"
      :kpTree="kpTree"
      :competenceTags="competenceTags"
      :methodTags="methodTags"
      :schoolTags="schoolTags"
      :kpLoading="kpLoading"
    />

    <!-- AI 识别审阅面板 -->
    <AiRecognizeDialog
      ref="aiDialogRef"
      v-model="showAiDialog"
      v-model:applyingAiResult="applyingAiResult"
      v-model:attrSelectedKps="attrSelectedKps"
      v-model:aiGeneratedFields="aiGeneratedFields"
      :form="form"
      @applied="onAiApplied"
      @batch-parsed="handleBatchParsed"
    />

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
import { ref, reactive, computed, watch, onMounted, onBeforeUnmount, nextTick } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { questionApi, kpApi, spaceApi, tagsApi, type KnowledgePoint, type SpaceMemberInfo, type Tag, type ParsedQuestion } from '@/api/client'
import { AppButton, AppBadge, AppModal, AppConfirm, AppIcon } from '@/components/ui'
import { useToast } from '@/composables/useToast'
import { useSpaceStore } from '@/stores/space'
import { useAuthStore } from '@/stores/auth'
import { useSelectedKp } from '@/composables/useSelectedKp'
import { useLayoutState } from '@/composables/useLayoutState'
import { hasUnfinishedSnapshot } from '@/utils/batchSnapshot'

// Imports of child components
import MetaBar from './edit/components/MetaBar.vue'
import EditFormChoice from './edit/components/EditFormChoice.vue'
import EditFormFill from './edit/components/EditFormFill.vue'
import EditFormSolution from './edit/components/EditFormSolution.vue'
import LivePreviewCard from './edit/components/LivePreviewCard.vue'
import AttributeModal from './edit/components/AttributeModal.vue'
import AiRecognizeDialog from './edit/components/AiRecognizeDialog.vue'

const route = useRoute()
const router = useRouter()
const toast = useToast()
const space = useSpaceStore()
const auth = useAuthStore()
const { select: selectKp, clear: clearKp } = useSelectedKp()
const { isKpTreeCollapsed } = useLayoutState()

const isNew = route.path.endsWith('/new')
const loading = ref(false)
const saving = ref(false)
const submitting = ref(false)
const isLoading = ref(false)
const kpLoading = ref(false)
const kpTree = ref<KnowledgePoint[]>([])

const showHistory = ref(false)
const showAttrDialog = ref(false)
const showAiDialog = ref(false)
const aiGeneratedFields = ref<Set<string>>(new Set())
const applyingAiResult = ref(false)
const aiDialogRef = ref<InstanceType<typeof AiRecognizeDialog> | null>(null)

// ===== 批量录题工作台模式 =====
// questionList 存放每道题的快照（plain object），activeIndex 指向当前编辑的题目
// 当 questionList.length > 1 时显示顶部 Tab 切换栏；否则保持原有单题模式
const questionList = ref<any[]>([])
const activeIndex = ref(0)
const isSwitchingTab = ref(false)

// Selected Knowledge Points list
const attrSelectedKps = ref<{ id: string; name: string }[]>([])

// Synchronize knowledge point state to parent form and global sidebar filters
watch(attrSelectedKps, (newVal) => {
  form.knowledgePointIds = newVal.map(k => k.id)
  if (newVal.length > 0) {
    selectKp(newVal[0].id, newVal[0].name)
  } else {
    clearKp()
  }
}, { deep: true })

// AI Dialog visibility collapse layout helper
watch([showAiDialog, showAttrDialog], ([aiOpen, attrOpen]) => {
  if (aiOpen || attrOpen) {
    isKpTreeCollapsed.value = true
  }
})

// Tag classification lists
const methodTags = ref<Tag[]>([])
const competenceTags = ref<Tag[]>([])
const schoolTags = ref<Tag[]>([])

async function loadTags() {
  try {
    const [methodRes, compRes, schoolRes] = await Promise.all([
      tagsApi.list('method'),
      tagsApi.list('core_competence'),
      tagsApi.list('school'),
    ])
    methodTags.value = methodRes.data
    competenceTags.value = compRes.data
    schoolTags.value = schoolRes.data
  } catch { /* handled */ }
}

const allTagsMap = computed(() => {
  const m = new Map<string, Tag>()
  for (const t of methodTags.value) m.set(t.id, t)
  for (const t of competenceTags.value) m.set(t.id, t)
  for (const t of schoolTags.value) m.set(t.id, t)
  return m
})

const form_tagList = computed(() => {
  return form.tagIds
    .map(id => allTagsMap.value.get(id))
    .filter((t): t is Tag => !!t)
})

const selectedCompetenceTags = computed(() => form_tagList.value.filter(t => t.category === 'core_competence'))
const selectedMethodTags = computed(() => form_tagList.value.filter(t => t.category === 'method'))
const selectedSchoolTags = computed(() => form_tagList.value.filter(t => t.category === 'school'))

const TAG_LIMITS: Record<string, number> = {
  core_competence: 3,
  method: 5,
  knowledge_point: 3,
  school: 1,
}

function toggleTagById(tag: Tag) {
  const idx = form.tagIds.indexOf(tag.id)
  if (idx >= 0) {
    form.tagIds.splice(idx, 1)
    return
  }
  const count = form_tagList.value.filter(t => t.category === tag.category).length
  const limit = TAG_LIMITS[tag.category] ?? 99
  if (count >= limit) {
    toast.warning('已达到该类别最大可选择上限')
    return
  }
  form.tagIds.push(tag.id)
}

function removeAttrKp(id: string) {
  attrSelectedKps.value = attrSelectedKps.value.filter(k => k.id !== id)
}

// Navigation back checks
const leaveDialog = ref(false)
function handleBack() {
  if (form.hasUnsaved) {
    leaveDialog.value = true
  } else {
    goBack()
  }
}
function goBack() {
  if (window.history.state?.back) {
    router.back()
  } else {
    if (isNew) router.replace('/questions')
    else router.replace(`/questions/${route.params.id}`)
  }
}

// AI trigger
function handleAi() {
  showAiDialog.value = true
}

function onAiApplied() {
  nextTick(() => {
    resizeAllTextareas()
  })
}

// Main reactive form
const form = reactive({
  stem: '',
  question_type: 'choice',
  sub_type: '' as string,
  difficulty: 'medium',
  difficulty_coefficient: 0.5 as number,
  default_score: 5,
  grade: undefined as string | undefined,
  semester: undefined as string | undefined,
  academic_year: '' as string,
  grade_semester: '' as string,
  exam_region: '' as string,
  exam_type: '' as string,
  source: '原创',
  estimated_time: 5,
  solutions: [''] as string[],
  options: [
    { label: 'A', content: '' },
    { label: 'B', content: '' },
    { label: 'C', content: '' },
    { label: 'D', content: '' },
  ] as { label: string; content: string }[],
  correctAnswer: '' as string | string[],
  blanks: [{ position: 1, answer: '' }] as { position: number; answer: string }[],
  solutionAnswer: '',
  sub_answers: [''] as string[],
  gradingSteps: [] as { label: string; points: number; description: string }[],
  judgmentCorrect: true,
  knowledgePointIds: [] as string[],
  tagIds: [] as string[],
  reviewer: '' as string,
  reviewer_ids: [] as string[],
  internal_note: '',
  status: '',
  version: 1,
  hasUnsaved: false,
})

const hasCorrectAnswer = computed(() => {
  if (Array.isArray(form.correctAnswer)) return form.correctAnswer.length > 0
  return !!form.correctAnswer
})

function switchChoiceMode(mode: 'single' | 'multi') {
  if (mode === 'multi') {
    form.sub_type = 'multi'
    if (form.correctAnswer && !Array.isArray(form.correctAnswer)) {
      form.correctAnswer = [form.correctAnswer]
    } else if (!form.correctAnswer) {
      form.correctAnswer = []
    }
  } else {
    form.sub_type = ''
    if (Array.isArray(form.correctAnswer)) {
      form.correctAnswer = form.correctAnswer[0] || ''
    }
  }
}

// Collapsible Panels
const collapse = reactive({
  source: true,
  basic: true,
  collab: true,
})
function toggleCollapse(key: keyof typeof collapse) {
  collapse[key] = !collapse[key]
}

// Multi-solutions helpers
const cnNums = ['一', '二', '三', '四', '五', '六', '七', '八', '九', '十']
function cnNum(n: number): string {
  return cnNums[n - 1] || String(n)
}

function addSolution() {
  form.solutions.push('')
  nextTick(() => {
    const els = document.querySelectorAll<HTMLTextAreaElement>('.solution-textarea')
    els[els.length - 1]?.focus()
  })
}

function removeSolution(i: number) {
  form.solutions.splice(i, 1)
  if (form.solutions.length === 0) form.solutions.push('')
}

// Textarea height auto-resizers
const stemTextareaRef = ref<HTMLTextAreaElement>()

function resizeTextarea(el: HTMLTextAreaElement) {
  el.style.height = 'auto'
  el.style.height = el.scrollHeight + 'px'
}
function autoResize(e: Event) {
  resizeTextarea(e.target as HTMLTextAreaElement)
}
function resizeAllTextareas() {
  document.querySelectorAll<HTMLTextAreaElement>('.edit-textarea').forEach(el => {
    resizeTextarea(el)
  })
}

// Image Uploaders
function handleImageUpload() {
  const input = document.createElement('input')
  input.type = 'file'
  input.accept = 'image/png,image/jpeg,image/gif,image/webp'
  input.onchange = async () => {
    const file = input.files?.[0]
    if (!file) return
    if (file.size > 5 * 1024 * 1024) {
      toast.error('图片不能超过 5MB')
      return
    }
    const imageUrl = URL.createObjectURL(file)
    const ta = stemTextareaRef.value
    if (!ta) {
      form.stem += `\n![题干配图](${imageUrl})\n`
      return
    }
    const pos = ta.selectionStart
    const before = form.stem.substring(0, pos)
    const after = form.stem.substring(ta.selectionEnd)
    const insert = `\n![题干配图](${imageUrl})\n`
    form.stem = before + insert + after
    nextTick(() => {
      ta.focus()
      const newPos = pos + insert.length
      ta.setSelectionRange(newPos, newPos)
      resizeTextarea(ta)
    })
  }
  input.click()
}

function handleSolutionImageUpload(index: number) {
  const input = document.createElement('input')
  input.type = 'file'
  input.accept = 'image/png,image/jpeg,image/gif,image/webp'
  input.onchange = async () => {
    const file = input.files?.[0]
    if (!file) return
    if (file.size > 5 * 1024 * 1024) {
      toast.error('图片不能超过 5MB')
      return
    }
    const imageUrl = URL.createObjectURL(file)
    const ta = document.querySelectorAll<HTMLTextAreaElement>('.solution-textarea')[index]
    if (!ta) {
      form.solutions[index] += `\n![解析配图](${imageUrl})\n`
      return
    }
    const pos = ta.selectionStart
    const before = form.solutions[index].substring(0, pos)
    const after = form.solutions[index].substring(ta.selectionEnd)
    const insert = `\n![解析配图](${imageUrl})\n`
    form.solutions[index] = before + insert + after
    nextTick(() => {
      ta.focus()
      const newPos = pos + insert.length
      ta.setSelectionRange(newPos, newPos)
      resizeTextarea(ta)
    })
  }
  input.click()
}

// Payload construction
function buildPayload() {
  const kpIds = attrSelectedKps.value.map(k => k.id)
  const payload: any = {
    stem: form.stem,
    question_type: form.question_type,
    sub_type: form.sub_type || null,
    difficulty: form.difficulty,
    difficulty_coefficient: form.difficulty_coefficient,
    default_score: form.default_score,
    grade: form.grade || null,
    semester: form.semester || null,
    academic_year: form.academic_year || null,
    grade_semester: form.grade_semester || null,
    exam_region: form.exam_region || null,
    exam_type: form.exam_type || null,
    source: form.source,
    analysis: form.solutions.filter(s => s.trim()).join('\n\n---\n\n') || null,
    knowledge_point_ids: kpIds.length > 0 ? kpIds : null,
    tag_ids: form.tagIds,
  }
  switch (form.question_type) {
    case 'choice':
      payload.options = (form.options || []).filter(o => o.content.trim())
      payload.sub_type = form.sub_type || null
      if (Array.isArray(form.correctAnswer)) {
        payload.correct_answer = form.correctAnswer
      } else {
        payload.correct_answer = form.correctAnswer ? [form.correctAnswer] : []
      }
      break
    case 'fill':
      payload.correct_answer = form.blanks.filter(b => b.answer.trim()).map(b => ({ position: b.position, answer: b.answer.trim() }))
      break
    case 'solution':
      payload.correct_answer = form.sub_answers.filter(a => a.trim())
      break
    case 'judgment':
      payload.correct_answer = [form.judgmentCorrect]
      break
  }
  return payload
}

// Save & Submit Actions
async function handleSave(submitAfter: boolean) {
  if (!form.stem.trim()) { toast.warning('请输入题干'); return }
  if (form.question_type === 'choice' && !hasCorrectAnswer.value) { toast.warning('请选择正确答案'); return }
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
    } else {
      toast.success(isNew ? '草稿已保存' : '已更新')
    }
    if (isNew) {
      router.replace(`/questions/${qid}`)
    } else {
      if (window.history.state?.back) {
        router.back()
      } else {
        router.replace(`/questions/${qid}`)
      }
    }
  } catch (e: any) {
    toast.error(e.response?.data?.error || '操作失败')
  } finally {
    flag.value = false
  }
}

// Draft autosave
// 【闸门】切换 Tab 期间不写草稿（避免 applyFormSnapshot 触发的批量字段变更被误判为修改）
let autoSaveTimer: ReturnType<typeof setTimeout> | null = null
watch(() => ({ ...form }), () => {
  if (isLoading.value || isSwitchingTab.value) return
  form.hasUnsaved = true
  if (autoSaveTimer) clearTimeout(autoSaveTimer)
  autoSaveTimer = setTimeout(() => {
    try {
      const key = isNew ? 'q-draft-new' : `q-draft-${route.params.id}`
      sessionStorage.setItem(key, JSON.stringify(form))
    } catch { /* quota exceeded */ }
  }, 3000)
}, { deep: true })

// ===== 批量模式：Tab 切换时保存当前题快照 → 加载目标题快照 =====
watch(activeIndex, async (newIdx, oldIdx) => {
  if (isSwitchingTab.value) return
  if (newIdx === oldIdx) return
  if (questionList.value.length <= 1) return

  isSwitchingTab.value = true
  try {
    // 1. 保存当前题到旧索引槽位
    if (oldIdx >= 0 && oldIdx < questionList.value.length) {
      questionList.value[oldIdx] = captureFormSnapshot()
    }
    // 2. 加载目标题到 form
    const target = questionList.value[newIdx]
    if (target) {
      applyFormSnapshot(target)
      // 等待响应式更新与被闸门屏蔽的 watcher 完成
      await nextTick()
      resizeAllTextareas()
    }
  } finally {
    isSwitchingTab.value = false
  }
})

// Draft restore
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
    if (draft.stem || draft.solutions?.some((s: string) => s?.trim()) || draft.solutionAnswer) {
      pendingDraft = draft
      restoreDialog.value = true
    }
  } catch { /* ignore */ }
}

async function doRestoreDraft() {
  if (!pendingDraft) return
  const fields = ['stem', 'question_type', 'sub_type', 'difficulty', 'default_score', 'grade', 'semester',
    'source', 'solutions', 'options', 'correctAnswer', 'blanks', 'solutionAnswer', 'sub_answers',
    'gradingSteps', 'judgmentCorrect', 'knowledgePointIds', 'tagIds', 'difficulty_coefficient', 'academic_year', 'grade_semester', 'exam_region', 'exam_type', 'reviewer', 'reviewer_ids', 'internal_note']
  for (const f of fields) {
    if (pendingDraft[f] !== undefined) (form as any)[f] = pendingDraft[f]
  }
  toast.success('草稿已恢复')
  pendingDraft = null
  restoreDialog.value = false
  await nextTick()
  resizeAllTextareas()
  document.querySelectorAll('textarea').forEach(el => {
    el.dispatchEvent(new Event('input'))
  })
}

function discardDraft() {
  try { sessionStorage.removeItem(getDraftKey()) } catch { /* ignore */ }
  pendingDraft = null
}

function clearDraft() {
  try { sessionStorage.removeItem(getDraftKey()) } catch { /* ignore */ }
}

// Data loaders
async function loadKpTree() {
  kpLoading.value = true
  try {
    const res = await kpApi.tree()
    kpTree.value = res.data
  } catch { /* handled */ }
  finally { kpLoading.value = false }
}

const spaceMembers = ref<SpaceMemberInfo[]>([])
const isTeamSpace = computed(() => space.currentSpace?.kind === 'team')

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
    form.sub_type = (d as any).sub_type || ''
    form.difficulty_coefficient = (d as any).difficulty_coefficient ?? 0.5
    form.academic_year = d.academic_year || ''
    form.grade_semester = d.grade_semester || ''
    form.exam_region = d.exam_region || ''
    form.exam_type = d.exam_type || ''
    form.source = d.source || '原创'
    const raw = d.analysis || ''
    if (raw.includes('\n\n---\n\n')) {
      form.solutions = raw.split(/\n\n---\n\n/)
    } else if (/\n解法[二三四五六七八九十]/.test(raw)) {
      form.solutions = raw.split(/\n(?=解法[二三四五六七八九十])/).map(s => s.trim())
    } else {
      form.solutions = raw ? [raw] : ['']
    }
    form.status = d.status
    form.version = d.version
    form.knowledgePointIds = d.knowledge_points?.map(k => k.id) || []
    attrSelectedKps.value = (d.knowledge_points || []).map(k => ({ id: k.id, name: k.name }))
    if (attrSelectedKps.value.length > 0) {
      selectKp(attrSelectedKps.value[0].id, attrSelectedKps.value[0].name)
    }
    form.tagIds = d.tags?.map(t => t.id) || []
    if (d.tags?.length) {
      for (const t of d.tags) {
        if (!allTagsMap.value.has(t.id)) {
          const fullTag: Tag = { ...t, space_id: null, use_count: 0, created_at: '' }
          if (t.category === 'core_competence') competenceTags.value = [...competenceTags.value, fullTag]
          else if (t.category === 'method') methodTags.value = [...methodTags.value, fullTag]
          else if (t.category === 'school') schoolTags.value = [...schoolTags.value, fullTag]
        }
      }
    }
    form.correctAnswer = ''
    form.blanks = [{ position: 1, answer: '' }]
    form.solutionAnswer = ''
    form.sub_answers = ['']
    form.gradingSteps = []
    form.judgmentCorrect = true
    if (d.question_type === 'choice' && d.options) {
      let opts = d.options
      if (typeof opts === 'string') { try { opts = JSON.parse(opts) } catch { opts = [] } }
      if (Array.isArray(opts)) {
        form.options = opts.map((opt: any) => {
          if (typeof opt === 'string') return { label: opt[0] || '', content: opt.slice(1).trim() }
          if (opt && typeof opt === 'object' && opt.label) return { label: opt.label, content: opt.content || '' }
          if (opt && typeof opt === 'object') return { label: Object.keys(opt)[0], content: Object.values(opt)[0] as string }
          return { label: '', content: String(opt) }
        })
      }
      if (Array.isArray(d.correct_answer)) {
        if ((d as any).sub_type === 'multi' || d.correct_answer.length > 1) {
          form.sub_type = 'multi'
          form.correctAnswer = d.correct_answer as string[]
        } else {
          form.correctAnswer = d.correct_answer[0] || ''
        }
      }
    } else if (d.question_type === 'fill' && Array.isArray(d.correct_answer)) {
      form.blanks = (d.correct_answer as any[]).map((b: any) => ({ position: b.position, answer: b.answer }))
    } else if (d.question_type === 'solution') {
      if (Array.isArray(d.correct_answer) && d.correct_answer.length > 0) {
        form.sub_answers = d.correct_answer.map((a: any) => typeof a === 'string' ? a : String(a))
      }
    } else if (d.question_type === 'judgment') {
      if (Array.isArray(d.correct_answer)) form.judgmentCorrect = d.correct_answer[0] === true
    }
    form.hasUnsaved = false
  } catch { /* handled */ }
  finally {
    loading.value = false
    await nextTick()
    isLoading.value = false
    if (!isNew) {
      await nextTick()
      resizeAllTextareas()
    }
  }
}

// ============================================================
// ===== 批量录题工作台：快照捕获 / 回放 / Tab 切换 / Mock =====
// ============================================================

// 捕获当前 form 的快照（深拷贝，包含 attrSelectedKps 用于每题独立保存）
function captureFormSnapshot(): any {
  return {
    ...JSON.parse(JSON.stringify(form)),
    attrSelectedKps: JSON.parse(JSON.stringify(attrSelectedKps.value)),
  }
}

// 将快照应用回 form（每个字段显式赋值，避免 delete+assign 引发响应式抖动）
function applyFormSnapshot(s: any) {
  form.stem = s.stem ?? ''
  form.question_type = s.question_type ?? 'choice'
  form.sub_type = s.sub_type ?? ''
  form.difficulty = s.difficulty ?? 'medium'
  form.difficulty_coefficient = s.difficulty_coefficient ?? 0.5
  form.default_score = s.default_score ?? 5
  form.grade = s.grade
  form.semester = s.semester
  form.academic_year = s.academic_year ?? ''
  form.grade_semester = s.grade_semester ?? ''
  form.exam_region = s.exam_region ?? ''
  form.exam_type = s.exam_type ?? ''
  form.source = s.source ?? '原创'
  form.estimated_time = s.estimated_time ?? 5
  form.solutions = Array.isArray(s.solutions) ? [...s.solutions] : ['']
  form.options = Array.isArray(s.options)
    ? s.options.map((o: any) => ({ ...o }))
    : [
        { label: 'A', content: '' },
        { label: 'B', content: '' },
        { label: 'C', content: '' },
        { label: 'D', content: '' },
      ]
  form.correctAnswer = s.correctAnswer ?? ''
  form.blanks = Array.isArray(s.blanks)
    ? s.blanks.map((b: any) => ({ ...b }))
    : [{ position: 1, answer: '' }]
  form.solutionAnswer = s.solutionAnswer ?? ''
  form.sub_answers = Array.isArray(s.sub_answers) ? [...s.sub_answers] : ['']
  form.gradingSteps = Array.isArray(s.gradingSteps) ? [...s.gradingSteps] : []
  form.judgmentCorrect = s.judgmentCorrect ?? true
  form.knowledgePointIds = Array.isArray(s.knowledgePointIds) ? [...s.knowledgePointIds] : []
  form.tagIds = Array.isArray(s.tagIds) ? [...s.tagIds] : []
  form.reviewer = s.reviewer ?? ''
  form.reviewer_ids = Array.isArray(s.reviewer_ids) ? [...s.reviewer_ids] : []
  form.internal_note = s.internal_note ?? ''
  form.status = s.status ?? ''
  form.version = s.version ?? 1
  form.hasUnsaved = false // 切换后的目标题视为未修改

  // 每题独立保存 attrSelectedKps
  attrSelectedKps.value = Array.isArray(s.attrSelectedKps)
    ? s.attrSelectedKps.map((k: any) => ({ ...k }))
    : []
}

// 切换到指定 Tab（保存当前题 → 加载目标题）
function switchToTab(idx: number) {
  if (idx === activeIndex.value) return
  if (idx < 0 || idx >= questionList.value.length) return
  activeIndex.value = idx
}

// Tab 预览文本：去掉 LaTeX 标记 / Markdown 图片 / 换行，截断到 14 字符
function stripStemPreview(stem: string): string {
  if (!stem) return ''
  return stem
    .replace(/\$\$[\s\S]+?\$\$/g, '')
    .replace(/\$[^$]+\$/g, '')
    .replace(/!\[[^\]]*\]\([^)]+\)/g, '')
    .replace(/[\n\r]/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
    .slice(0, 14)
}

// 临时 Mock：在 URL 加 ?batch=test 即可注入两道测试题，立即看到 Tab 切换效果
function loadBatchMockData() {
  questionList.value = [
    {
      stem: '【题目 1】已知集合 $A = \\{x \\mid x^2 - 5x + 6 = 0\\}$，集合 $B = \\{2, 3, 4\\}$，求 $A \\cap B$。',
      question_type: 'solution',
      sub_type: '',
      difficulty: 'easy',
      difficulty_coefficient: 0.85,
      default_score: 5,
      solutions: ['由 $x^2 - 5x + 6 = 0$ 解得 $x = 2$ 或 $x = 3$，故 $A = \\{2, 3\\}$，$A \\cap B = \\{2, 3\\}$。'],
      sub_answers: ['$A \\cap B = \\{2, 3\\}$'],
      correctAnswer: '',
      options: [
        { label: 'A', content: '' },
        { label: 'B', content: '' },
        { label: 'C', content: '' },
        { label: 'D', content: '' },
      ],
      blanks: [{ position: 1, answer: '' }],
      attrSelectedKps: [],
      knowledgePointIds: [],
      tagIds: [],
      reviewer_ids: [],
      source: '原创',
      academic_year: '',
      grade_semester: '',
      exam_region: '',
      exam_type: '',
    },
    {
      stem: '【题目 2】化简 $\\sqrt{12} - \\sqrt{3}$，并求其值。',
      question_type: 'solution',
      sub_type: '',
      difficulty: 'medium',
      difficulty_coefficient: 0.55,
      default_score: 5,
      solutions: ['$\\sqrt{12} - \\sqrt{3} = 2\\sqrt{3} - \\sqrt{3} = \\sqrt{3}$。'],
      sub_answers: ['$\\sqrt{3}$'],
      correctAnswer: '',
      options: [
        { label: 'A', content: '' },
        { label: 'B', content: '' },
        { label: 'C', content: '' },
        { label: 'D', content: '' },
      ],
      blanks: [{ position: 1, answer: '' }],
      attrSelectedKps: [],
      knowledgePointIds: [],
      tagIds: [],
      reviewer_ids: [],
      source: '原创',
      academic_year: '',
      grade_semester: '',
      exam_region: '',
      exam_type: '',
    },
  ]
  activeIndex.value = 0
  // 直接将第一道题应用到 form（绕过 watcher，避免误触发 autosave）
  applyFormSnapshot(questionList.value[0])
  nextTick(() => resizeAllTextareas())
}

// ============================================================
// 对接 AI 批量识别：把 ParsedQuestion[] 转换为 form 快照数组
// ============================================================
function parsedQuestionToSnapshot(q: ParsedQuestion): any {
  // 计算 difficulty_coefficient（与 AiRecognizeDialog.doApplyAiResult 保持一致）
  let difficultyCoefficient = 0.5
  if (q.difficulty) {
    const diffMap: Record<string, number> = { easy: 2, medium: 3, hard: 4 }
    const diffStars = diffMap[q.difficulty] || 3
    difficultyCoefficient = [0.9, 0.75, 0.55, 0.35, 0.2][diffStars - 1] ?? 0.55
  }

  // 计算 correctAnswer / blanks / sub_answers
  let correctAnswer: any = ''
  let blanks: { position: number; answer: string }[] = [{ position: 1, answer: '' }]
  let sub_answers: string[] = ['']

  if (q.question_type === 'choice' && q.correct_answer.kind === 'choice' && q.correct_answer.value.options) {
    const opts = q.correct_answer.value.options
    if (q.sub_type === 'multi' || opts.length > 1) {
      correctAnswer = opts
    } else {
      correctAnswer = opts[0] || ''
    }
  } else if (q.question_type === 'fill' && q.correct_answer.kind === 'fill' && q.correct_answer.value.blanks) {
    blanks = q.correct_answer.value.blanks.map(b => ({ position: b.position, answer: b.answer }))
  } else if (q.question_type === 'solution' && q.correct_answer.kind === 'solution' && q.correct_answer.value.subs) {
    sub_answers = q.correct_answer.value.subs.map(s => s.content)
  }

  // 计算 knowledgePointIds / attrSelectedKps（高置信度匹配）
  let knowledgePointIds: string[] = []
  let attrSelectedKps: { id: string; name: string }[] = []
  if (q.kp_matches?.length) {
    const highConfidenceMatch = q.kp_matches.find(m => m.score >= 0.95 && m.matched_id)
    if (highConfidenceMatch) {
      knowledgePointIds = [highConfidenceMatch.matched_id!]
      attrSelectedKps = [{ id: highConfidenceMatch.matched_id!, name: highConfidenceMatch.matched_name! }]
    }
  }

  return {
    stem: q.stem,
    question_type: q.question_type,
    sub_type: q.sub_type || '',
    difficulty: q.difficulty || 'medium',
    difficulty_coefficient: difficultyCoefficient,
    default_score: 5,
    grade: undefined,
    semester: undefined,
    academic_year: '',
    grade_semester: '',
    exam_region: '',
    exam_type: '',
    options: q.question_type === 'choice' && q.options
      ? q.options.map(o => ({ label: o.label, content: o.content }))
      : [
          { label: 'A', content: '' },
          { label: 'B', content: '' },
          { label: 'C', content: '' },
          { label: 'D', content: '' },
        ],
    correctAnswer,
    blanks,
    sub_answers,
    solutionAnswer: '',
    solutions: q.analysis.map(a => a.content),
    gradingSteps: [],
    judgmentCorrect: true,
    knowledgePointIds,
    tagIds: [],
    reviewer_ids: [],
    reviewer: '',
    internal_note: '',
    status: '',
    version: 1,
    hasUnsaved: true,
    source: '原创',
    estimated_time: 5,
    attrSelectedKps,
  }
}

// 接收 AiRecognizeDialog 的批量识别结果，填充 questionList 进入多题工作台
function handleBatchParsed(questions: ParsedQuestion[]) {
  if (!questions || questions.length === 0) {
    toast.warning('未识别到任何题目')
    return
  }

  // 把 ParsedQuestion[] 转换为 form 快照数组
  questionList.value = questions.map(q => parsedQuestionToSnapshot(q))
  activeIndex.value = 0

  // 应用第一题到 form（通过 isSwitchingTab 闸住副作用 watcher）
  isSwitchingTab.value = true
  try {
    applyFormSnapshot(questionList.value[0])
    nextTick(() => {
      resizeAllTextareas()
      isSwitchingTab.value = false
    })
  } catch (e) {
    isSwitchingTab.value = false
    console.error('[handleBatchParsed] 应用第一题失败:', e)
  }

  toast.success(`已加载 ${questions.length} 道题，进入批量录入工作台`)
}

// Window unload checks
function handleBeforeUnload(e: BeforeUnloadEvent) {
  if (form.hasUnsaved) { e.preventDefault(); e.returnValue = '' }
}

onMounted(() => {
  window.addEventListener('beforeunload', handleBeforeUnload)
  loadKpTree()
  loadSpaceMembers()
  loadTags()
  loadQuestion().then(() => {
    if (!isNew) restoreDraft()
  })
  if (isNew) restoreDraft()

  // ===== 临时测试入口：URL 加 ?batch=test 即可注入两道 Mock 题目，立即看到 Tab 切换效果 =====
  if (route.query.batch === 'test') {
    loadBatchMockData()
    return
  }

  // ===== 从其他页面跳转过来时，读取 history.state.parsedQuestions 进入多题工作台 =====
  // 用法：router.push({ path: '/questions/new', state: { parsedQuestions: [...] } })
  const stateQuestions = (window.history.state as any)?.parsedQuestions
  if (Array.isArray(stateQuestions) && stateQuestions.length > 0) {
    handleBatchParsed(stateQuestions as ParsedQuestion[])
    // 消费后清理 state，避免刷新时重复加载
    try {
      window.history.replaceState({ ...window.history.state, parsedQuestions: undefined }, '')
    } catch { /* ignore */ }
  }
})

onMounted(async () => {
  const snapshot = await hasUnfinishedSnapshot()
  if (snapshot) {
    aiDialogRef.value?.triggerSnapshotRestore(snapshot)
  }
})

onBeforeUnmount(() => {
  window.removeEventListener('beforeunload', handleBeforeUnload)
  if (autoSaveTimer) clearTimeout(autoSaveTimer)
  clearKp()
})

watch(() => form.question_type, () => {
  // 【闸门】切换 Tab 期间不重置答案字段（applyFormSnapshot 已显式赋值，避免被覆盖）
  if (isSwitchingTab.value) return
  if (isNew && !applyingAiResult.value) {
    form.sub_type = ''
    form.correctAnswer = ''
    form.blanks = [{ position: 1, answer: '' }]
    form.solutionAnswer = ''
    form.sub_answers = ['']
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
  padding: 16px 24px;
  gap: 12px;
  background: var(--bg-primary);
}

.edit-title {
  font-size: 17px;
  font-weight: 650;
  margin: 0 0 0 2px;
  color: var(--text-primary);
  letter-spacing: -0.01em;
}

.loading-hint {
  text-align: center;
  padding: 48px 20px;
  color: var(--text-muted);
  font-size: 14px;
}

/* ============ 顶部操作栏 ============ */
.top-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-shrink: 0;
  gap: 12px;
}

.top-bar-left,
.top-bar-right {
  display: flex;
  align-items: center;
  gap: 8px;
}

/* ============ 主双栏布局 ============ */
.main-content {
  display: flex;
  flex: 1;
  gap: 16px;
  overflow: hidden;
  height: 100%;
}

.edit-col {
  flex: 1.2;
  min-width: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-card);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-sm);
  border: 1px solid var(--border-color);
  overflow: hidden;
}

.edit-col-inner {
  flex: 1;
  overflow-y: auto;
  padding: 20px 24px;
  display: flex;
  flex-direction: column;
  gap: 20px;
}

/* ============ 第二层：描述性标签流 ============ */
.question-tags-wrapper {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  align-items: center;
}

.attr-tag {
  height: 24px;
  padding: 0 6px 0 8px;
  border-radius: 6px;
  font-size: 12px;
  font-weight: 550;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  color: var(--text-secondary);
  background: var(--bg-input);
  border: 1px solid var(--border-color);
}

.attr-tag-x {
  border: none;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  padding: 1px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 4px;
}

.attr-tag-x:hover {
  background: rgba(0,0,0,0.06);
  color: var(--text-primary);
}

.attr-tag-kp {
  background: rgba(0, 122, 255, 0.04);
  border-color: rgba(0, 122, 255, 0.12);
  color: var(--accent);
}

.attr-tag-kp-primary {
  background: var(--accent);
  border-color: var(--accent);
  color: #ffffff;
}

.attr-tag-kp-primary .attr-tag-x {
  color: rgba(255, 255, 255, 0.8);
}

.attr-tag-kp-primary .attr-tag-x:hover {
  background: rgba(255, 255, 255, 0.2);
  color: #ffffff;
}

.attr-tag-literacy {
  background: rgba(88, 86, 214, 0.04);
  border-color: rgba(88, 86, 214, 0.12);
  color: #5856d6;
}

.attr-tag-method {
  background: rgba(52, 199, 89, 0.04);
  border-color: rgba(52, 199, 89, 0.12);
  color: #34c759;
}

.attr-tag-text {
  max-width: 120px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.attr-add-btn {
  height: 24px;
  padding: 0 10px;
  border-radius: 6px;
  font-size: 11.5px;
  font-weight: 600;
  display: inline-flex;
  align-items: center;
  gap: 3px;
  color: var(--accent);
  background: var(--accent-light);
  border: none;
  cursor: pointer;
  transition: all 0.2s ease;
}

.attr-add-btn:hover {
  background: rgba(0, 122, 255, 0.15);
}

/* ============ 编辑区块通用 ============ */
.edit-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.section-label {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13.5px;
  font-weight: 650;
  color: var(--text-primary);
  margin-bottom: 2px;
}

.section-label span {
  letter-spacing: -0.01em;
}

.required {
  color: var(--danger);
  margin-left: 2px;
}

/* 分段切换按钮（单选/多选） */
.seg-toggle {
  display: inline-flex;
  gap: 2px;
  padding: 2px;
  border-radius: 6px;
  background: var(--bg-input);
  margin-left: 8px;
}

.seg-btn {
  padding: 2px 10px;
  border: none;
  border-radius: 4px;
  background: transparent;
  font-size: 11px;
  font-weight: 600;
  color: var(--text-muted);
  cursor: pointer;
  transition: all 0.2s;
}

.seg-btn.active {
  background: var(--bg-card);
  color: var(--text-primary);
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.05);
}

/* 文本输入框与配图上传容器 */
.stem-wrap,
.solution-textarea-wrap {
  position: relative;
  background: var(--bg-input);
  border-radius: var(--radius-md);
  border: 1px solid var(--border-color);
  overflow: hidden;
  transition: border-color 0.2s;
}

.stem-wrap:focus-within,
.solution-textarea-wrap:focus-within {
  border-color: var(--accent);
}

.edit-textarea {
  width: 100%;
  padding: 12px 14px 40px;
  border: none;
  background: transparent;
  color: var(--text-primary);
  font-size: 14px;
  line-height: 1.7;
  font-family: inherit;
  resize: none;
  outline: none;
  box-sizing: border-box;
}

.img-upload-btn {
  position: absolute;
  left: 12px;
  bottom: 10px;
  height: 24px;
  padding: 0 8px;
  border-radius: 6px;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  color: var(--text-secondary);
  font-size: 11px;
  font-weight: 550;
  display: flex;
  align-items: center;
  gap: 4px;
  cursor: pointer;
  box-shadow: 0 1px 2px rgba(0,0,0,0.02);
  transition: all 0.2s;
}

.img-upload-btn:hover {
  background: var(--bg-hover);
  color: var(--accent);
  border-color: var(--accent-light);
}

/* ============ 解析多解法列表 ============ */
.solutions-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.solution-item {
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  padding: 14px;
  background: var(--bg-card);
}

.solution-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 10px;
}

.solution-name {
  font-size: 13px;
  font-weight: 650;
  color: var(--text-primary);
}

.solution-del {
  border: none;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  padding: 4px;
  border-radius: 4px;
  display: inline-flex;
  align-items: center;
  transition: all 0.2s;
}

.solution-del:hover {
  background: var(--danger-light);
  color: var(--danger);
}

.add-solution-btn {
  height: 32px;
  width: 100%;
  border: 1px dashed var(--border-color);
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--text-secondary);
  font-size: 12.5px;
  font-weight: 550;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  cursor: pointer;
  transition: all 0.2s;
  margin-top: 4px;
}

.add-solution-btn:hover {
  border-color: var(--accent);
  color: var(--accent);
  background: var(--accent-light);
}

/* ============ 高级折叠面板 ============ */
.advanced-section {
  border-top: 1px solid var(--border-color);
  padding-top: 16px;
}

.advanced-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  width: 100%;
  border: none;
  background: transparent;
  cursor: pointer;
  padding: 6px 0;
  color: var(--text-secondary);
}

.advanced-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13.5px;
  font-weight: 600;
}

.collapse-arrow {
  display: inline-flex;
  transition: transform 0.25s ease;
}

.collapse-arrow.open {
  transform: rotate(-90deg);
}

.advanced-body {
  padding-top: 14px;
}

.form-grid-2 {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 20px;
}

.field-label {
  display: block;
  font-size: 12.5px;
  font-weight: 600;
  color: var(--text-secondary);
  margin-bottom: 6px;
}

.text-input {
  width: 100%;
  padding: 8px 10px;
  border-radius: 6px;
  border: 1px solid var(--border-color);
  background: var(--bg-input);
  color: var(--text-primary);
  font-size: 13.5px;
  outline: none;
  box-sizing: border-box;
}

.text-input:focus {
  border-color: var(--accent);
  background: var(--bg-card);
}

.reviewer-checkboxes {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 120px;
  overflow-y: auto;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  padding: 8px 10px;
  background: var(--bg-input);
}

.reviewer-item {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  color: var(--text-primary);
  cursor: pointer;
}

.reviewer-item input[type='checkbox'] {
  border-radius: 4px;
}

.hint-line {
  margin-top: 4px;
}

/* ============ AI 痕迹高亮 ============ */
@keyframes ai-breathe {
  0%, 100% {
    box-shadow: 0 0 0 2px var(--purple);
  }
  50% {
    box-shadow: 0 0 8px 2px var(--purple-light);
  }
}

.ai-highlight {
  animation: ai-breathe 2s ease-in-out infinite;
  border-radius: var(--radius-md);
  transition: box-shadow 0.5s ease;
}

[data-theme='dark'] .edit-col {
  border-color: #3a3a3c;
  box-shadow: none;
}

/* ============ 批量录题 Tab 切换栏 ============ */
.batch-tabs {
  flex-shrink: 0;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  padding: 4px;
  display: flex;
  overflow-x: auto;
  overscroll-behavior: contain;
}

.batch-tabs-track {
  display: inline-flex;
  gap: 2px;
  flex-shrink: 0;
}

.batch-tab {
  padding: 6px 14px;
  border: none;
  background: transparent;
  border-radius: 6px;
  font-size: 12.5px;
  font-weight: 550;
  color: var(--text-secondary);
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  white-space: nowrap;
  transition: background 0.18s ease, color 0.18s ease, box-shadow 0.18s ease;
}

.batch-tab:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.batch-tab.is-active {
  background: var(--accent);
  color: #ffffff;
  box-shadow: 0 1px 4px rgba(0, 122, 255, 0.25);
}

.batch-tab-index {
  font-weight: 700;
  letter-spacing: -0.01em;
}

.batch-tab-preview {
  font-size: 11.5px;
  opacity: 0.75;
  max-width: 140px;
  overflow: hidden;
  text-overflow: ellipsis;
}

.batch-tab.is-active .batch-tab-preview {
  opacity: 0.9;
}

.batch-tab-empty {
  font-size: 11.5px;
  opacity: 0.5;
  font-style: italic;
}

[data-theme='dark'] .batch-tab.is-active {
  box-shadow: 0 1px 4px rgba(10, 132, 255, 0.35);
}
</style>
