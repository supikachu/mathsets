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
          <AppButton variant="ghost" size="sm" @click="handleBack"><AppIcon name="chevron-left" :size="15" /> 返回</AppButton>
          <AppButton variant="ghost" size="sm" @click="handleAi"><AppIcon name="sparkles" :size="15" /> AI 智能识别</AppButton>
          <h1 class="edit-title">{{ isNew ? '录入新题' : '编辑题目' }}</h1>
          <AppBadge v-if="!isNew" color="gray">v{{ form.version }}</AppBadge>
        </div>
        <div class="top-bar-right">
          <span
            v-if="draftStatus !== 'idle'"
            class="draft-status"
            :class="draftStatus"
            :key="draftStatus"
          >
            <span v-if="draftStatus === 'saving'" class="draft-spinner" />
            <AppIcon v-else name="check" :size="13" />
            <span>{{ draftStatus === 'saving' ? '正在保存草稿…' : '已保存' }}</span>
          </span>
          <AppButton v-if="!isNew" variant="outline" size="sm" @click="showHistory = true"><AppIcon name="history" :size="15" /> 历史版本</AppButton>
          <AppButton variant="outline" size="sm" :loading="saving" :disabled="saving || submitting" @click="handleSave(false)"><AppIcon name="save" :size="15" /> 保存</AppButton>
          <AppButton variant="primary" size="sm" :loading="submitting" :disabled="saving || submitting" @click="handleSave(true)"><AppIcon name="send" :size="15" /> 提交审核</AppButton>
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

      <!-- 知识树分类失败提示条：节点暂存，可重试分类（保存时原样并入，不丢数据） -->
      <div v-if="pendingNodes.length > 0" class="classify-retry-banner">
        <AppIcon name="alert" :size="14" />
        <span class="classify-retry-text">
          知识树分类数据加载失败，{{ pendingNodes.length }} 个节点暂未分类显示（保存时会原样保留，不会丢失）
        </span>
        <AppButton variant="outline" size="sm" :loading="classifyRetrying" :disabled="classifyRetrying" @click="retryDistributePendingNodes">
          重试分类
        </AppButton>
      </div>

      <!-- ==================== 主内容 三栏：编辑 + 预览 + 属性面板 ==================== -->
      <div class="main-content">
        <!-- 左栏：编辑 -->
        <div class="edit-col interactive-column">
          <div class="edit-col-inner">
            <!-- ==================== 第二层：描述性标签流（只读概览，知识点在右侧面板编辑） ==================== -->
            <div class="question-tags-wrapper">
              <span v-if="form.region_province || form.region_city" class="attr-tag">
                <AppIcon name="pin" :size="11" />
                <span class="attr-tag-text">{{ [form.region_province, form.region_city].filter(Boolean).join(' · ') }}</span>
                <button type="button" class="attr-tag-x" @click="form.region_province = ''; form.region_city = ''"><AppIcon name="x" :size="10" /></button>
              </span>
              <span v-if="form.source_type" class="attr-tag">
                <AppIcon name="bookmark" :size="11" />
                <span class="attr-tag-text">{{ form.source_type === '高考模拟' && form.sub_source_type ? form.sub_source_type : form.source_type }}</span>
                <button type="button" class="attr-tag-x" @click="form.source_type = ''; form.sub_source_type = ''"><AppIcon name="x" :size="10" /></button>
              </span>
              <span v-if="form.year" class="attr-tag">
                <AppIcon name="clock" :size="11" />
                <span class="attr-tag-text">{{ form.year }}</span>
                <button type="button" class="attr-tag-x" @click="form.year = ''"><AppIcon name="x" :size="10" /></button>
              </span>
              <span v-if="knowledgeNodeIds.length > 0" class="attr-tag attr-tag-kp attr-tag-kp-primary">
                <AppIcon name="tag" :size="11" />
                <span class="attr-tag-text">知识点 ×{{ knowledgeNodeIds.length }}</span>
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
            </div>

            <!-- 题干 -->
            <section class="edit-section" :class="{ 'ai-highlight': aiGeneratedFields.has('stem') }">
              <div class="section-label"><AppIcon name="book-open" :size="16" /> <span>题干</span><span class="required">*</span></div>
              <div class="stem-wrap">
                <textarea ref="stemTextareaRef" v-model="form.stem" class="edit-textarea stem-textarea" placeholder="输入题目内容，LaTeX 公式用 $...$ 包裹。例如：已知集合 $A = \{x | x^2 - 2x = 0\}$..."></textarea>
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
                      class="edit-textarea solution-textarea"
                      :placeholder="`解法${cnNum(i + 1)}的解题思路，支持 $...$ LaTeX`"
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

        <!-- 中栏：试卷化预览 -->
        <LivePreviewCard
          class="interactive-column"
          tabindex="0"
          @click="focusColumn"
          :form="form"
          :image-editable="true"
          @image-click="handleImageClick"
        />

        <!-- 右栏：常驻属性面板（含 AI 智能打标） -->
        <AttributeSidePanel
          class="interactive-column"
          v-model:tagIds="form.tagIds"
          v-model:knowledgeNodeIds="knowledgeNodeIds"
          v-model:chapterNodeIds="chapterNodeIds"
          v-model:methodNodeIds="methodNodeIds"
          v-model:primaryKnowledgeNodeId="primaryKnowledgeNodeId"
          v-model:aiGeneratedFields="aiGeneratedFields"
          v-model:aiHighlightIds="aiHighlightIds"
          v-model:collapsed="panelCollapsed"
          v-model:paperIds="paperIds"
          :selection-cache="selectionCache"
          :initial-node-names="initialNodeNames"
          :competenceTags="competenceTags"
          :methodTags="methodTags"
          :schoolTags="schoolTags"
          :form="form"
        />
      </div>
    </template>

    <!-- 版本历史弹窗 -->
    <AppModal v-model="showHistory" title="历史版本">
      <div class="loading-hint">版本历史功能即将上线</div>
    </AppModal>

    <!-- AI 识别审阅面板 -->
    <AiRecognizeDialog
      ref="aiDialogRef"
      v-model="showAiDialog"
      v-model:applyingAiResult="applyingAiResult"
      v-model:knowledgeNodeIds="knowledgeNodeIds"
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

    <!-- 团队空间：审题人选择对话框 -->
    <AppModal v-model="showReviewerDialog" title="选择审题人">
      <div class="dialog-body" style="min-width: 360px">
        <p class="dialog-hint" style="margin-bottom: 12px; color: var(--text-secondary); font-size: 13px">
          团队空间需要交叉审核，请选择空间内的其他成员作为审题人
        </p>
        <select v-model="selectedReviewerId" class="dialog-input" style="width: 100%; padding: 8px 12px; border-radius: 8px; border: 1px solid var(--border-color); font-size: 14px">
          <option value="">请选择审题人…</option>
          <option v-for="m in reviewableMembers" :key="m.user_id" :value="m.user_id">
            {{ m.display_name || m.username }}（{{ m.role === 'owner' ? '拥有者' : '成员' }}）
          </option>
        </select>
      </div>
      <div style="display: flex; justify-content: flex-end; gap: 8px; margin-top: 16px">
        <AppButton variant="outline" @click="showReviewerDialog = false">取消</AppButton>
        <AppButton variant="primary" :disabled="!selectedReviewerId" :loading="submitting" @click="confirmSubmitWithReviewer">确认提交</AppButton>
      </div>
    </AppModal>

    <!-- 图片调节浮窗（编辑模式专属：宽度/对齐/裁剪） -->
    <ImageAdjustmentPanel
      :visible="imageAdjustPanelVisible"
      :target="imageAdjustTarget"
      :image-data="imageAdjustData"
      @update-config="handleUpdateConfig"
      @crop-request="handleCropRequest"
      @close="imageAdjustPanelVisible = false"
    />

    <!-- 图片裁剪弹窗 -->
    <CropperDialog
      v-model:visible="cropperDialogVisible"
      :image-url="cropperImageUrl"
      @cropped="handleCropped"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, computed, watch, onMounted, onBeforeUnmount, nextTick } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { questionApi, spaceApi, tagsApi, paperApi, type SpaceMemberInfo, type Tag, type ParsedQuestion } from '@/api/client'
import { AppButton, AppBadge, AppModal, AppConfirm, AppIcon } from '@/components/ui'
import { useToast } from '@/composables/useToast'
import { getKnowledgeTreeList } from '@/composables/useKnowledgeTreeCache'
import { useSpaceStore } from '@/stores/space'
import { useAuthStore } from '@/stores/auth'
import { hasUnfinishedSnapshot } from '@/utils/batchSnapshot'
import { processMarkdownImages, type UploadCache } from '@/utils/markdownImages'
import { uploadsApi } from '@/api/client'
import ImageAdjustmentPanel from '@/components/ImageAdjustmentPanel.vue'
import CropperDialog from '@/components/CropperDialog.vue'
import type { ImageConfig, ImageClickPayload } from '@/components/LatexRender.vue'

// Imports of child components
import EditFormChoice from './edit/components/EditFormChoice.vue'
import EditFormFill from './edit/components/EditFormFill.vue'
import EditFormSolution from './edit/components/EditFormSolution.vue'
import LivePreviewCard from './edit/components/LivePreviewCard.vue'
import AttributeSidePanel from './edit/components/AttributeSidePanel.vue'
import AiRecognizeDialog from './edit/components/AiRecognizeDialog.vue'

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

const showHistory = ref(false)
const showAiDialog = ref(false)
const aiGeneratedFields = ref<Set<string>>(new Set())
// AI 打标新增的知识树节点 ID（树组件浅金高亮；手动触碰单个即消，保存成功全清）
const aiHighlightIds = ref<string[]>([])
// 学段/学科切换的三组节点勾选缓存：key = `${subject}_${stage}`，切走快照、切回瞬恢
const selectionCache = new Map<string, { chapter: string[]; knowledge: string[]; method: string[] }>()
const applyingAiResult = ref(false)
const aiDialogRef = ref<InstanceType<typeof AiRecognizeDialog> | null>(null)

// ===== 批量录题工作台模式 =====
// questionList 存放每道题的快照（plain object），activeIndex 指向当前编辑的题目
// 当 questionList.length > 1 时显示顶部 Tab 切换栏；否则保持原有单题模式
const questionList = ref<any[]>([])
const activeIndex = ref(0)
const isSwitchingTab = ref(false)

// Selected Knowledge node IDs（与 AttributeSidePanel v-model 双向绑定）
const knowledgeNodeIds = ref<string[]>([])
// 章节 / 解题方法节点 ID（与 AttributeSidePanel v-model 双向绑定，提交时与知识点合并）
const chapterNodeIds = ref<string[]>([])
const methodNodeIds = ref<string[]>([])
// 主知识点节点 ID（每题最多 1 个，跨三组节点单选；与 AttributeSidePanel v-model 双向绑定）
const primaryKnowledgeNodeId = ref<string | null>(null)
// 初始节点名称映射（编辑场景 loadQuestion 后填充，传给 AttributeSidePanel 展示 Tag）
const initialNodeNames = ref<Record<string, string>>({})
// 知识树分类元数据（knowledgeTreeApi.list）加载失败时的待分发节点
// 保留 id/name/tree_id，重试后按 tree_id->kind 精准分发；绝不静默错分为知识点
const pendingNodes = ref<{ id: string; name: string; tree_id: string }[]>([])
const classifyRetrying = ref(false)

// 关联试卷 ID 列表（与 AttributeSidePanel v-model 双向绑定）
const paperIds = ref<string[]>([])

// Tag classification lists
const methodTags = ref<Tag[]>([])
const competenceTags = ref<Tag[]>([])
const schoolTags = ref<Tag[]>([])

// 右侧属性面板折叠状态（小屏场景把空间还给编辑器）
const panelCollapsed = ref(false)

// 草稿自动保存状态指示：'idle' | 'saving' | 'saved'
const draftStatus = ref<'idle' | 'saving' | 'saved'>('idle')
let draftStatusTimer: ReturnType<typeof setTimeout> | null = null

// 预览列无 input，点击时手动聚焦其根节点以触发 :focus-within 沉浸式高亮
function focusColumn(e: MouseEvent) {
  (e.currentTarget as HTMLElement)?.focus()
}

async function loadTags() {
  try {
    const [methodRes, compRes, schoolRes] = await Promise.all([
      tagsApi.list({ category: 'method' }),
      tagsApi.list({ category: 'core_competence' }),
      tagsApi.list({ category: 'school' }),
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
  // field-sizing: content 自动处理 textarea 高度，无需 JS 重算
}

// Main reactive form
const form = reactive({
  stem: '',
  question_type: 'choice',
  sub_type: '' as string,
  difficulty: 'medium',
  difficulty_coefficient: 0.5 as number,
  default_score: 5,
  grade: '' as string,
  semester: undefined as string | undefined,
  grade_semester: '' as string,
  // ── 长尾维度：统一存入 questions.metadata(JSONB)，与 QuestionList 数据字典对齐 ──
  year: '' as string,                  // 年份：'2020'..'2026'
  region_province: '' as string,       // 省份：'浙江'/'江苏'...
  region_city: '' as string,           // 城市：'杭州市'...（与省份级联）
  source_type: '' as string,           // 来源：'课前预习'/'高考模拟'...
  sub_source_type: '' as string,       // 来源子项：仅 source_type='高考模拟' 时启用
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
  knowledgeNodeIds: [] as string[],
  // ── 知识树动态加载依赖：学段 / 学科（提交时进 metadata） ──
  stage: 'senior' as 'junior' | 'senior',
  subject: 'math' as 'math' | 'physics',
  // ── 前端独立维护三组节点 ID，提交时合并为统一 knowledge_node_ids ──
  chapterNodeIds: [] as string[],
  methodNodeIds: [] as string[],
  // ── 主知识点（每题最多 1 个，跨三组节点单选；后端 DTO 字段对齐） ──
  primaryKnowledgeNodeId: null as string | null,
  tagIds: [] as string[],
  reviewer: '' as string,
  reviewer_ids: [] as string[],
  internal_note: '',
  status: '',
  version: 1,
  hasUnsaved: false,
})

// 同步 knowledgeNodeIds 到 form（供 buildPayload 使用）
watch(knowledgeNodeIds, (v) => {
  form.knowledgeNodeIds = v
}, { deep: true })
watch(chapterNodeIds, v => { form.chapterNodeIds = v }, { deep: true })
watch(methodNodeIds, v => { form.methodNodeIds = v }, { deep: true })
watch(primaryKnowledgeNodeId, v => { form.primaryKnowledgeNodeId = v })

// 难度字符串枚举 ↔ 数字 1-5 转换
function difficultyStringToNum(s: string): number {
  if (s === 'easy') return 2
  if (s === 'hard') return 4
  return 3
}

function difficultyNumToString(n: number | null | undefined): string {
  if (n == null) return 'medium'
  if (n <= 2) return 'easy'
  if (n === 3) return 'medium'
  return 'hard'
}

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

// Stem textarea ref —— 仅用于图片上传时的光标位置插入（高度由 CSS field-sizing 管理）
const stemTextareaRef = ref<HTMLTextAreaElement>()

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
    })
  }
  input.click()
}

// Payload construction
function buildPayload() {
  // ── metadata(JSONB)：长尾维度统一存放 ──
  // grade / grade_semester / year / region_province / region_city / source_type / sub_source_type
  const metadata: Record<string, unknown> = {}
  if (form.grade) metadata.grade = form.grade
  if (form.grade_semester) metadata.grade_semester = form.grade_semester
  if (form.year) metadata.year = form.year
  if (form.region_province) metadata.region_province = form.region_province
  if (form.region_city) metadata.region_city = form.region_city
  if (form.source_type) metadata.source_type = form.source_type
  if (form.sub_source_type) metadata.sub_source_type = form.sub_source_type
  metadata.stage = form.stage
  metadata.subject = form.subject

  // 三组节点 ID 合并去重为统一 knowledge_node_ids（后端无感知前端拆分）
  // pendingNodes：树分类元数据加载失败时暂存的节点，原样并入——不丢数据、不错分
  const mergedNodeIds = Array.from(new Set([
    ...form.chapterNodeIds,
    ...form.knowledgeNodeIds,
    ...form.methodNodeIds,
    ...pendingNodes.value.map(n => n.id),
  ]))

  // Payload 严格对齐后端 UpdateQuestionRequest / CreateQuestionRequest
  // 移除后端不识别的 sub_type、space_id 字段（space_id 由后端从用户上下文推断）
  const payload: any = {
    stem: form.stem,
    question_type: form.question_type,
    difficulty: difficultyStringToNum(form.difficulty),
    difficulty_score: Math.max(1, Math.min(10, Math.round((1 - form.difficulty_coefficient) * 9) + 1)),
    default_score: form.default_score,
    metadata: Object.keys(metadata).length > 0 ? metadata : undefined,
    analysis: form.solutions.filter(s => s.trim()).join('\n\n---\n\n') || null,
    knowledge_node_ids: mergedNodeIds.length > 0 ? mergedNodeIds : null,
    // 主知识点 ID：跨三组节点单选，null 表示取消主知识点
    primary_knowledge_node_id: form.primaryKnowledgeNodeId || null,
    tag_ids: form.tagIds,
    paper_ids: paperIds.value,
  }
  switch (form.question_type) {
    case 'choice':
      payload.options = (form.options || []).filter(o => o.content.trim())
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
  }
  return payload
}

// Save & Submit Actions

// ── 团队空间审题人选择对话框 ──
const showReviewerDialog = ref(false)
const selectedReviewerId = ref('')
const pendingQuestionId = ref<string | null>(null)

// 可选审题人：团队空间中排除自己和 viewer
const reviewableMembers = computed(() =>
  spaceMembers.value.filter(m => m.user_id !== auth.userId && m.role !== 'viewer'),
)

// ── 保存拦截器：持久化表单中所有 blob: 图片为后端永久 URL ──
// 触发场景：用户在编辑器上传本地图片后，Markdown 中存的是 blob: 临时指针，
//          保存到后端前必须转存为永久 URL，否则页面刷新后图片永久失效。
const BLOB_URL_QUICK_CHECK = /!\[[^\]]*\]\(blob:[^)]+\)/

async function persistFormImages() {
  // 快速短路：表单中没有任何 blob: URL 时跳过整个流程
  const hasBlob =
    BLOB_URL_QUICK_CHECK.test(form.stem) ||
    form.solutions.some((s) => BLOB_URL_QUICK_CHECK.test(s)) ||
    (form.options || []).some((o) => BLOB_URL_QUICK_CHECK.test(o.content))
  if (!hasBlob) return

  // 跨字段共享上传缓存：同一张图在 stem / solution / option 中只上传一次
  const cache: UploadCache = new Map()
  try {
    // 处理题干
    form.stem = await processMarkdownImages(form.stem, cache)
    // 处理解析（每条解析都可能含图）
    form.solutions = await Promise.all(
      form.solutions.map((s) => processMarkdownImages(s, cache)),
    )
    // 处理选项内容
    if (form.options && form.options.length > 0) {
      await Promise.all(
        form.options.map(async (opt) => {
          opt.content = await processMarkdownImages(opt.content, cache)
        }),
      )
    }
  } catch (e) {
    // 整体流程不应失败（单图失败已在 processMarkdownImages 内捕获），
    // 此处兜底仅记录日志，不影响后续 buildPayload / 提交
    console.error('[persistFormImages] 持久化流程异常:', e)
  }
}

async function handleSave(submitAfter: boolean) {
  if (!form.stem.trim()) { toast.warning('请输入题干'); return }
  if (form.question_type === 'choice' && !hasCorrectAnswer.value) { toast.warning('请选择正确答案'); return }
  const flag = submitAfter ? submitting : saving
  flag.value = true
  try {
    // 【保存拦截器】提交前持久化所有 blob: 图片为后端永久 URL
    // 失败不阻断：单图上传失败时保留 blob URL，由用户重试或后续保存
    await persistFormImages()

    const data = buildPayload()
    const res = isNew ? await questionApi.create(data) : await questionApi.update(route.params.id as string, data)
    const qid = res.data.id
    form.hasUnsaved = false
    clearDraft()
    // 保存成功：AI 高亮节点全部清除（手动修改阶段的视觉反馈到此为止）
    aiHighlightIds.value = []

    if (submitAfter) {
      if (isTeamSpace.value) {
        // 团队空间：弹出选人对话框
        pendingQuestionId.value = qid
        selectedReviewerId.value = ''
        showReviewerDialog.value = true
        // 不直接跳转，等用户选完审题人
        flag.value = false
        return
      } else {
        // 个人空间：自审自发，直接提交
        await questionApi.submit(qid)
        toast.success('已创建并提交审核')
      }
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
    console.error('[QuestionEdit] 保存失败:', e)
    // axum 0.8 Json 拒绝响应体可能是纯字符串（非 JSON 对象），需多级兜底
    const errData = e.response?.data
    const errMsg = typeof errData === 'string' ? errData : (errData?.error || errData?.message)
    toast.error(errMsg || e.message || '保存失败')
  } finally {
    if (!showReviewerDialog.value) {
      flag.value = false
    }
  }
}

// ── 团队空间：确认选择审题人后提交 ──
async function confirmSubmitWithReviewer() {
  if (!selectedReviewerId.value || !pendingQuestionId.value) return
  submitting.value = true
  try {
    await questionApi.submit(pendingQuestionId.value, { reviewer_id: selectedReviewerId.value })
    toast.success('已提交审核')
    showReviewerDialog.value = false
    const qid = pendingQuestionId.value
    pendingQuestionId.value = null
    selectedReviewerId.value = ''
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
    console.error('[QuestionEdit] 提交审核失败:', e)
    const errData = e.response?.data
    const errMsg = typeof errData === 'string' ? errData : (errData?.error || errData?.message)
    toast.error(errMsg || e.message || '提交审核失败')
  } finally {
    submitting.value = false
  }
}

// Draft autosave
// 【闸门】切换 Tab 期间不写草稿（避免 applyFormSnapshot 触发的批量字段变更被误判为修改）
// 用户每次改动 → 立即标记 "saving" → 3s 防抖落盘后切到 "saved" → 2s 后回到 "idle"
let autoSaveTimer: ReturnType<typeof setTimeout> | null = null
watch(() => ({ ...form }), () => {
  if (isLoading.value || isSwitchingTab.value) return
  form.hasUnsaved = true
  draftStatus.value = 'saving'
  if (draftStatusTimer) { clearTimeout(draftStatusTimer); draftStatusTimer = null }
  if (autoSaveTimer) clearTimeout(autoSaveTimer)
  autoSaveTimer = setTimeout(() => {
    try {
      const key = isNew ? 'q-draft-new' : `q-draft-${route.params.id}`
      sessionStorage.setItem(key, JSON.stringify(form))
      draftStatus.value = 'saved'
      draftStatusTimer = setTimeout(() => { draftStatus.value = 'idle' }, 2000)
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
    'solutions', 'options', 'correctAnswer', 'blanks', 'solutionAnswer', 'sub_answers',
    'gradingSteps', 'tagIds', 'difficulty_coefficient', 'grade_semester',
    'year', 'region_province', 'region_city', 'source_type', 'sub_source_type',
    'reviewer', 'reviewer_ids', 'internal_note']
  for (const f of fields) {
    if (pendingDraft[f] !== undefined) (form as any)[f] = pendingDraft[f]
  }
  // knowledgeNodeIds / chapterNodeIds / methodNodeIds 单独还原（独立 ref + form 字段同步）
  if (Array.isArray(pendingDraft.knowledgeNodeIds)) {
    knowledgeNodeIds.value = [...pendingDraft.knowledgeNodeIds]
    form.knowledgeNodeIds = [...pendingDraft.knowledgeNodeIds]
  }
  if (Array.isArray(pendingDraft.chapterNodeIds)) {
    chapterNodeIds.value = [...pendingDraft.chapterNodeIds]
    form.chapterNodeIds = [...pendingDraft.chapterNodeIds]
  }
  if (Array.isArray(pendingDraft.methodNodeIds)) {
    methodNodeIds.value = [...pendingDraft.methodNodeIds]
    form.methodNodeIds = [...pendingDraft.methodNodeIds]
  }
  // 主知识点还原（null 或 string，缺省保持 null）
  if (pendingDraft.primaryKnowledgeNodeId !== undefined) {
    primaryKnowledgeNodeId.value = pendingDraft.primaryKnowledgeNodeId
    form.primaryKnowledgeNodeId = pendingDraft.primaryKnowledgeNodeId
  }
  toast.success('草稿已恢复')
  pendingDraft = null
  restoreDialog.value = false
  await nextTick()
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
const spaceMembers = ref<SpaceMemberInfo[]>([])
const isTeamSpace = computed(() => space.currentSpace?.kind === 'team')

async function loadSpaceMembers() {
  if (!isTeamSpace.value || !space.currentSpaceId) return
  try {
    const res = await spaceApi.get(space.currentSpaceId)
    spaceMembers.value = res.data.members || []
  } catch { /* handled */ }
}

// 将扁平知识节点按 tree_id -> kind 映射分发到 章节/知识点/方法 三个数组
// 同时回填 primaryKnowledgeNodeId（来自 is_primary 字段，每题最多 1 个）
// 返回 true 表示分发成功；树列表不可用时返回 false（调用方负责兜底，绝不静默错分）
async function distributeNodesByTreeKind(
  knodes: { id: string; name: string; tree_id: string; is_primary?: boolean }[],
): Promise<boolean> {
  let treesData
  try {
    treesData = await getKnowledgeTreeList()
  } catch {
    return false
  }
  const treeKindMap = new Map(treesData.map(t => [t.id, t.kind]))
  const kIds: string[] = [], cIds: string[] = [], mIds: string[] = []
  const nameMap: Record<string, string> = {}
  let primaryId: string | null = null
  for (const k of knodes) {
    nameMap[k.id] = k.name
    const kind = treeKindMap.get(k.tree_id)
    if (kind === 'chapter') cIds.push(k.id)
    else if (kind === 'ability') mIds.push(k.id)
    else kIds.push(k.id) // 'knowledge' 或未知兜底
    if (k.is_primary) primaryId = k.id
  }
  knowledgeNodeIds.value = kIds
  chapterNodeIds.value = cIds
  methodNodeIds.value = mIds
  form.knowledgeNodeIds = kIds
  form.chapterNodeIds = cIds
  form.methodNodeIds = mIds
  primaryKnowledgeNodeId.value = primaryId
  form.primaryKnowledgeNodeId = primaryId
  initialNodeNames.value = nameMap
  return true
}

// 分类失败后的手动重试：pendingNodes 暂存的节点重新走 tree_id->kind 分发
async function retryDistributePendingNodes() {
  if (pendingNodes.value.length === 0 || classifyRetrying.value) return
  classifyRetrying.value = true
  try {
    const ok = await distributeNodesByTreeKind(pendingNodes.value)
    if (ok) {
      pendingNodes.value = []
      toast.success('知识树分类已恢复')
    } else {
      toast.error('分类数据加载仍失败，请稍后重试')
    }
  } finally {
    classifyRetrying.value = false
  }
}

async function loadQuestion() {
  if (isNew) return
  isLoading.value = true
  loading.value = true
  try {
    const res = await questionApi.get(route.params.id as string)
    const d = res.data
    const meta = (d.metadata || {}) as Record<string, any>
    form.stem = d.stem
    form.question_type = d.question_type
    form.difficulty = difficultyNumToString(d.difficulty)
    form.default_score = d.default_score
    form.grade = meta.grade || ''
    form.semester = d.semester || undefined
    form.sub_type = (d as any).sub_type || ''
    form.difficulty_coefficient = d.difficulty_score ?? 0.5
    form.grade_semester = meta.grade_semester || ''
    form.year = meta.year || ''
    form.region_province = meta.region_province || ''
    form.region_city = meta.region_city || ''
    form.source_type = meta.source_type || ''
    form.sub_source_type = meta.sub_source_type || ''
    form.stage = meta.stage === 'junior' ? 'junior' : 'senior'
    form.subject = meta.subject === 'physics' ? 'physics' : 'math'
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
    // 按类回填：d.knowledge_nodes 每项含 id/name/tree_id/is_primary，按 tree_id → kind 映射分类
    const knodes = (d.knowledge_nodes || []) as {
      id: string; name: string; tree_id: string; is_primary?: boolean
    }[]
    if (knodes.length > 0) {
      const ok = await distributeNodesByTreeKind(knodes)
      if (!ok) {
        // 树列表加载失败：暂存原始节点（含 tree_id），不静默错分；UI 提示重试，提交时原样并入 payload
        pendingNodes.value = knodes
        initialNodeNames.value = Object.fromEntries(knodes.map(k => [k.id, k.name]))
        knowledgeNodeIds.value = []
        chapterNodeIds.value = []
        methodNodeIds.value = []
        form.knowledgeNodeIds = []
        form.chapterNodeIds = []
        form.methodNodeIds = []
        // 兜底：分类失败时仍尝试回填 primary，避免主知识点信息丢失
        const primaryKnode = knodes.find(k => k.is_primary)
        primaryKnowledgeNodeId.value = primaryKnode?.id ?? null
        form.primaryKnowledgeNodeId = primaryKnode?.id ?? null
        toast.error('知识树分类数据加载失败，节点暂未分类，可点击上方提示条重试')
      }
    } else {
      knowledgeNodeIds.value = []
      chapterNodeIds.value = []
      methodNodeIds.value = []
      form.knowledgeNodeIds = []
      form.chapterNodeIds = []
      form.methodNodeIds = []
      primaryKnowledgeNodeId.value = null
      form.primaryKnowledgeNodeId = null
      initialNodeNames.value = {}
      pendingNodes.value = []
    }
    form.tagIds = d.tags?.map(t => t.id) || []

    // 加载题目已关联的试卷列表（反向查询）
    try {
      const papersRes = await paperApi.getQuestionPapers(route.params.id as string)
      paperIds.value = papersRes.data.map(p => p.paper_id)
    } catch {
      // 关联试卷加载失败不阻塞题目编辑
      paperIds.value = []
    }
    if (d.tags?.length) {
      for (const t of d.tags) {
        if (!allTagsMap.value.has(t.id)) {
          const fullTag: Tag = {
            id: t.id,
            name: t.name,
            category: t.category,
            parent_id: null,
            path: '',
            aliases: null,
            description: null,
            space_id: null,
            use_count: 0,
            is_active: true,
            created_at: '',
          }
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
    }
    form.hasUnsaved = false
  } catch { /* handled */ }
  finally {
    loading.value = false
    await nextTick()
    isLoading.value = false
    if (!isNew) {
      await nextTick()
    }
  }
}

// ============================================================
// ===== 批量录题工作台：快照捕获 / 回放 / Tab 切换 / Mock =====
// ============================================================

// 捕获当前 form 的快照（深拷贝，包含 knowledgeNodeIds 用于每题独立保存）
function captureFormSnapshot(): any {
  return {
    ...JSON.parse(JSON.stringify(form)),
    knowledgeNodeIds: JSON.parse(JSON.stringify(knowledgeNodeIds.value)),
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
  form.grade_semester = s.grade_semester ?? ''
  form.year = s.year ?? ''
  form.region_province = s.region_province ?? ''
  form.region_city = s.region_city ?? ''
  form.source_type = s.source_type ?? ''
  form.sub_source_type = s.sub_source_type ?? ''
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
  form.knowledgeNodeIds = Array.isArray(s.knowledgeNodeIds) ? [...s.knowledgeNodeIds] : []
  form.tagIds = Array.isArray(s.tagIds) ? [...s.tagIds] : []
  form.reviewer = s.reviewer ?? ''
  form.reviewer_ids = Array.isArray(s.reviewer_ids) ? [...s.reviewer_ids] : []
  form.internal_note = s.internal_note ?? ''
  form.status = s.status ?? ''
  form.version = s.version ?? 1
  form.hasUnsaved = false // 切换后的目标题视为未修改

  // 每题独立保存 knowledgeNodeIds
  knowledgeNodeIds.value = Array.isArray(s.knowledgeNodeIds) ? [...s.knowledgeNodeIds] : []
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
      knowledgeNodeIds: [],
      tagIds: [],
      reviewer_ids: [],
      grade_semester: '',
      year: '',
      region_province: '',
      region_city: '',
      source_type: '',
      sub_source_type: '',
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
      knowledgeNodeIds: [],
      tagIds: [],
      reviewer_ids: [],
      grade_semester: '',
      year: '',
      region_province: '',
      region_city: '',
      source_type: '',
      sub_source_type: '',
    },
  ]
  activeIndex.value = 0
  // 直接将第一道题应用到 form（绕过 watcher，避免误触发 autosave）
  applyFormSnapshot(questionList.value[0])
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

  // 计算 knowledgeNodeIds（高置信度匹配，沿用 kp_matches 字段）
  let knowledgeNodeIds: string[] = []
  if (q.kp_matches?.length) {
    const highConfidenceMatch = q.kp_matches.find(m => m.score >= 0.95 && m.matched_id)
    if (highConfidenceMatch) {
      knowledgeNodeIds = [highConfidenceMatch.matched_id!]
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
    grade_semester: '',
    year: '',
    region_province: '',
    region_city: '',
    source_type: '',
    sub_source_type: '',
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
    knowledgeNodeIds,
    tagIds: [],
    reviewer_ids: [],
    reviewer: '',
    internal_note: '',
    status: '',
    version: 1,
    hasUnsaved: true,
    estimated_time: 5,
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
  if (draftStatusTimer) clearTimeout(draftStatusTimer)
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
  }
})

// ============================================================
// 图片调节面板 & 裁剪弹窗集成
// ------------------------------------------------------------
// 仅题干预览区开启 editable 模式：用户点击图片后，弹出浮窗调节
// 宽度/对齐（等比例缩放，禁止 float），或触发裁剪弹窗。修改后的配置
// 精准反写到 form.stem 的 Markdown 语法 ![alt](url){config}。
// ============================================================

const imageAdjustPanelVisible = ref(false)
const imageAdjustTarget = ref<HTMLElement | null>(null)
const imageAdjustData = ref<{ url: string; mdId: string; config: ImageConfig } | null>(null)
const cropperDialogVisible = ref(false)
const cropperImageUrl = ref('')
const cropperProcessing = ref(false)

/** 处理 LivePreviewCard 转发的图片点击事件：打开调节面板 */
function handleImageClick(payload: ImageClickPayload) {
  imageAdjustTarget.value = payload.target
  imageAdjustData.value = {
    url: payload.url,
    mdId: payload.mdId,
    config: payload.config,
  }
  imageAdjustPanelVisible.value = true
}

/**
 * 精准反写 Markdown（补丁2：严格相等判断）：
 *   - 全局遍历所有 `![alt](url){oldConfig}` 或 `![alt](url)` 语法
 *   - 提取 imgUrl 后与目标 url 进行 **严格绝对相等判断** (`imgUrl.trim() === url.trim()`)
 *   - 仅匹配项替换为新 configString；不匹配项原样返回
 *   - 当 configString 为空字符串时，尾部 {} 被彻底移除（恢复默认）
 *
 * 严禁使用 .includes() 匹配 URL —— 会误杀同名后缀或子串相似的图片。
 */
function updateImageConfigInMarkdown(md: string, url: string, configString: string): string {
  const imgRegex = /!\[([^\]]*)\]\(([^)]+)\)(?:\{[^}]*\})?/g
  return md.replace(imgRegex, (match, alt, imgUrl) => {
    // 严格绝对相等判断：提取 Markdown 中的 imgUrl 与目标 url 逐字符比对
    if (imgUrl.trim() !== url.trim()) {
      return match // URL 不匹配，原样返回
    }
    return `![${alt}](${imgUrl})${configString}`
  })
}

/** 调节面板配置变化时：精准反写到 form.stem */
function handleUpdateConfig({ configString }: { mdId: string; configString: string }) {
  if (!imageAdjustData.value) return
  const url = imageAdjustData.value.url
  form.stem = updateImageConfigInMarkdown(form.stem, url, configString)
}

/** 调节面板触发裁剪：打开 CropperDialog */
function handleCropRequest({ url }: { url: string; mdId: string }) {
  cropperImageUrl.value = url
  cropperDialogVisible.value = true
}

/**
 * 裁剪完成回调（补丁4：前端不删图）：
 *   1. 将 Blob 上传到后端获取持久化 URL
 *   2. 严格相等匹配 form.stem 中的旧 URL 并替换为新 URL（保留 alt 与 {config}）
 *   3. 关闭裁剪弹窗与调节面板，避免引用过期 DOM
 *
 * 【重要】前端绝对不调用任何"删除旧图片"的 API。
 *         旧图的物理清理由后端 update_question handler 的差集比对自动完成。
 */
async function handleCropped(blob: Blob) {
  if (!imageAdjustData.value || cropperProcessing.value) return
  cropperProcessing.value = true
  const oldUrl = imageAdjustData.value.url

  try {
    const ext = (blob.type.split('/')[1] || 'png').toLowerCase()
    const file = new File([blob], `cropped.${ext}`, { type: blob.type || 'image/png' })
    const res = await uploadsApi.uploadImage(file)
    const newUrl = res.data.url

    // 严格相等匹配替换 URL（保留 alt 和 {config}，不调用任何删除 API）
    const imgRegex = /!\[([^\]]*)\]\(([^)]+)\)/g
    form.stem = form.stem.replace(imgRegex, (match, alt, imgUrl) => {
      if (imgUrl.trim() !== oldUrl.trim()) {
        return match
      }
      return `![${alt}](${newUrl})`
    })

    toast.success('图片裁剪并上传成功')
    cropperDialogVisible.value = false
    imageAdjustPanelVisible.value = false
  } catch (e) {
    console.error('[handleCropped] 裁剪图片上传失败:', e)
    toast.error('裁剪图片上传失败，请重试')
  } finally {
    cropperProcessing.value = false
  }
}
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

/* 顶部操作栏按钮统一苹果胶囊风（999px 全圆角），与 QuestionDetail.vue 保持样式一致 */
.top-bar :deep(.btn) {
  border-radius: 999px;
}

/* 草稿自动保存状态指示器 */
.draft-status {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  font-weight: 500;
  color: var(--text-muted);
  padding: 2px 8px;
  border-radius: 6px;
  background: var(--bg-muted, transparent);
  transition: color 0.2s ease, background 0.2s ease;
  animation: draft-fade-in 0.2s ease;
}

.draft-status.saving {
  color: var(--text-secondary, var(--text-muted));
}

.draft-spinner {
  display: inline-block;
  width: 12px;
  height: 12px;
  border: 1.5px solid currentColor;
  border-top-color: transparent;
  border-radius: 50%;
  animation: draft-spin 0.9s linear infinite;
}

.draft-status.saved {
  color: var(--success, #10b981);
  background: var(--success-light, rgba(16, 185, 129, 0.08));
}

@keyframes draft-fade-in {
  from { opacity: 0; transform: translateY(-1px); }
  to { opacity: 1; transform: translateY(0); }
}

@keyframes draft-spin {
  to { transform: rotate(360deg); }
}

/* ============ 主双栏布局 ============ */
.main-content {
  display: flex;
  flex: 1;
  gap: 16px;
  overflow: hidden;
  height: 100%;
  align-items: stretch;
}

/* 知识树分类失败提示条（pendingNodes 暂存重试） */
.classify-retry-banner {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
  padding: 8px 12px;
  margin-bottom: 10px;
  border-radius: 8px;
  font-size: 12.5px;
  color: #92400e;
  background: #fffbeb;
  border: 1px solid #fde68a;
}

.classify-retry-banner .classify-retry-text {
  flex: 1;
  min-width: 0;
}

[data-theme='dark'] .classify-retry-banner {
  color: #fbbf24;
  background: rgba(251, 191, 36, 0.08);
  border-color: rgba(251, 191, 36, 0.3);
}

.edit-col {
  flex: 1.2;
  min-width: 0;
  min-height: 0;
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

/* ============ 沉浸式三栏交互容器 ============ */
.interactive-column {
  height: 100%;
  min-height: 0;
  /* overflow: hidden 确保中栏(LivePreviewCard)/右栏(AttributeSidePanel)的
     内部内容不会撑破列容器高度。左栏(.edit-col)已自带 overflow:hidden。
     内部滚动由 :deep(.preview-col-inner) / :deep(.asp-body) / .edit-col-inner 处理。
     下拉/弹窗组件通常用 position:fixed 或 <Teleport>，不受此裁剪影响。 */
  overflow: hidden;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-lg);
  opacity: 0.7;
  outline: none;
  transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1);
}

.interactive-column:hover {
  opacity: 0.85;
}

.interactive-column:focus-within {
  opacity: 1;
  border-color: var(--purple);
  transform: translateY(-2px);
  box-shadow: 0 0 0 3px var(--purple-light), var(--shadow-md);
}

/* 细滚动条：Firefox + 滚动链切断（关键修复）
   overscroll-behavior: contain 阻止子容器滚动到边界时
   将滚动事件冒泡到父级，避免"页面被拉上去、底部漏出空白" */
.edit-col-inner,
.interactive-column :deep(.preview-col-inner),
.interactive-column :deep(.asp-body) {
  scrollbar-width: thin;
  overscroll-behavior: contain;
}

/* 细滚动条：WebKit（6px 极简风） */
.edit-col-inner::-webkit-scrollbar,
.interactive-column :deep(.preview-col-inner)::-webkit-scrollbar,
.interactive-column :deep(.asp-body)::-webkit-scrollbar {
  width: 6px;
  height: 6px;
}

.edit-col-inner::-webkit-scrollbar-thumb,
.interactive-column :deep(.preview-col-inner)::-webkit-scrollbar-thumb,
.interactive-column :deep(.asp-body)::-webkit-scrollbar-thumb {
  background: var(--border-color);
  border-radius: 3px;
}

.edit-col-inner::-webkit-scrollbar-track,
.interactive-column :deep(.preview-col-inner)::-webkit-scrollbar-track,
.interactive-column :deep(.asp-body)::-webkit-scrollbar-track {
  background: transparent;
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
  /* CSS 原生按内容撑开 —— 替代旧 JS scrollHeight 动态计算
     Chrome 123+ 支持；Firefox/Safari 暂不支持时会 fallback 到默认高度
     不设 max-height / overflow:hidden —— textarea 随内容无限增高，
     由外层 .edit-col-inner 的 overflow-y:auto 统一滚动，避免双重滚动条 */
  field-sizing: content;
  min-height: 120px;
  overflow: hidden;
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
  animation: ai-breathe 2s ease-in-out 3;
  border-radius: var(--radius-md);
  transition: box-shadow 0.5s ease;
}

[data-theme='dark'] .interactive-column {
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
