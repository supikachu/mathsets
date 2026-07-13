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

      <!-- ==================== 所有属性同一行 ==================== -->
      <div class="meta-row">
        <div class="meta-field meta-field-type">
          <label class="field-label">题型</label>
          <AppSelect v-model="form.question_type" :options="typeOptions" />
        </div>
        <div class="meta-field meta-field-diff">
          <label class="field-label">难度</label>
          <div class="diff-row">
            <button
              v-for="n in 5"
              :key="n"
              type="button"
              class="star"
              :class="{ active: difficultyStars >= n }"
              @click="difficultyStars = n"
            ><AppIcon name="star" :size="15" /></button>
          </div>
        </div>
        <div class="meta-field">
          <label class="field-label">学年</label>
          <AppSelect v-model="form.academic_year" :options="academicYearOptions" clearable />
        </div>
        <div class="meta-field">
          <label class="field-label">年级学期</label>
          <AppSelect v-model="form.grade_semester" :options="gradeSemesterOptions" clearable />
        </div>
        <div class="meta-field">
          <label class="field-label">地区/学校</label>
          <input v-model="form.region" placeholder="如 湖北襄阳" class="text-input" />
        </div>
        <div class="meta-field">
          <label class="field-label">考试类型</label>
          <AppSelect v-model="form.exam_type" :options="examTypeOptions" clearable />
        </div>
        <div class="meta-field">
          <label class="field-label">知识点</label>
          <div class="kp-display">
            <AppIcon name="tag" :size="15" />
            <span v-if="selectedKpName">{{ selectedKpName }}</span>
            <span v-else class="kp-empty">左侧选择</span>
          </div>
        </div>
        <div class="meta-field">
          <label class="field-label">核心素养</label>
          <button type="button" class="kp-btn" @click="showLiteracyDialog = true">
            <AppIcon name="award" :size="15" />
            <span>{{ form.literacy_tags.length ? `${form.literacy_tags.length}个` : '添加' }}</span>
          </button>
        </div>
        <div class="meta-field">
          <label class="field-label">解题方法</label>
          <button type="button" class="kp-btn" @click="showTagDialog = true">
            <AppIcon name="bookmark" :size="15" />
            <span>{{ form.tags.length ? `${form.tags.length}个` : '添加' }}</span>
          </button>
        </div>
      </div>

      <!-- ==================== 主内容 双栏 ==================== -->
      <div class="main-content">
        <!-- 左栏：编辑 -->
        <div class="edit-col">
          <div class="edit-col-inner">
            <!-- 题干 -->
            <section class="edit-section">
              <div class="section-label"><AppIcon name="book-open" :size="16" /> <span>题干</span><span class="required">*</span></div>
              <div class="stem-wrap">
                <textarea v-model="form.stem" rows="4" class="edit-textarea stem-textarea" placeholder="输入题目内容，LaTeX 公式用 $...$ 包裹。例如：已知集合 $A = \{x | x^2 - 2x = 0\}$..." @input="autoResize"></textarea>
                <button type="button" class="img-upload-btn" @click="handleImageUpload">
                  <AppIcon name="paperclip" :size="13" />
                  <span>上传配图</span>
                </button>
              </div>
            </section>

            <!-- 答案 -->
            <section class="edit-section">
              <div class="section-label"><AppIcon name="file-text" :size="16" /> <span>答案</span></div>
              <!-- 选择题选项 -->
              <div v-if="form.question_type === 'choice'" class="choice-grid">
                <div v-for="(opt, i) in form.options" :key="i" class="opt-row">
                  <label class="radio-label" :class="{ checked: form.correctAnswer === opt.label }">
                    <input type="radio" :value="opt.label" v-model="form.correctAnswer" />
                    {{ opt.label }}
                  </label>
                  <input v-model="opt.content" :placeholder="`选项 ${opt.label}`" class="opt-input" />
                  <button v-if="form.options.length > 2" type="button" class="icon-btn" @click="form.options.splice(i, 1)"><AppIcon name="x" :size="15" /></button>
                </div>
                <button type="button" class="add-btn add-btn-sm" @click="addOption"><AppIcon name="plus" :size="14" /> 添加选项</button>
              </div>
              <!-- 填空题 -->
              <div v-else-if="form.question_type === 'fill'" class="blank-wrap">
                <div v-for="(blank, i) in form.blanks" :key="i" class="blank-item">
                  <span class="blank-label">第{{ i+1 }}空</span>
                  <input v-model="blank.answer" placeholder="答案" class="opt-input blank-input" />
                  <button v-if="form.blanks.length > 1" type="button" class="icon-btn" @click="form.blanks.splice(i, 1)"><AppIcon name="x" :size="15" /></button>
                </div>
                <button type="button" class="add-btn add-btn-sm" @click="form.blanks.push({ position: Math.max(...form.blanks.map(b => b.position), 0) + 1, answer: '' })"><AppIcon name="plus" :size="14" /> 添加填空位</button>
              </div>
              <!-- 解答题 -->
              <div v-else-if="form.question_type === 'solution'">
                <textarea v-model="form.solutionAnswer" rows="3" class="edit-textarea" placeholder="完整解答过程，支持 $...$ LaTeX"></textarea>
                <div class="grading-label">分步评分</div>
                <div v-for="(step, i) in form.gradingSteps" :key="i" class="opt-row">
                  <input v-model="step.label" placeholder="步骤名" class="step-input" />
                  <input type="number" v-model.number="step.points" min="0" max="20" class="num-input num-input-sm" />
                  <span class="text-muted text-sm">分</span>
                  <button v-if="form.gradingSteps.length > 1" type="button" class="icon-btn" @click="form.gradingSteps.splice(i, 1)"><AppIcon name="x" :size="15" /></button>
                </div>
                <button type="button" class="add-btn add-btn-sm" @click="form.gradingSteps.push({ label: '', points: 1, description: '' })"><AppIcon name="plus" :size="14" /> 添加评分步骤</button>
              </div>
            </section>

            <!-- 解析 -->
            <section class="edit-section">
              <div class="section-label"><AppIcon name="lightbulb" :size="16" /> <span>解析</span></div>
              <textarea v-model="form.analysis" rows="6" class="edit-textarea analysis-textarea" placeholder="解题思路与易错点，支持 $...$ LaTeX" @input="autoResize"></textarea>
            </section>

            <!-- 高级设置（默认折叠） -->
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
        <div class="preview-col">
          <div class="preview-col-inner">
            <!-- 骨架屏（无输入时） -->
            <div v-if="!form.stem && !form.solutionAnswer && !form.analysis && form.options.every(o => !o.content)" class="preview-skeleton">
              <div class="skeleton-line skeleton-title"></div>
              <div class="skeleton-line skeleton-text"></div>
              <div class="skeleton-line skeleton-text skeleton-short"></div>
              <div class="skeleton-line skeleton-text"></div>
              <div class="skeleton-gap"></div>
              <div class="skeleton-line skeleton-opt"></div>
              <div class="skeleton-line skeleton-opt"></div>
              <div class="skeleton-line skeleton-opt"></div>
              <div class="skeleton-line skeleton-opt"></div>
              <div class="skeleton-gap"></div>
              <div class="skeleton-line skeleton-answer"></div>
              <div class="skeleton-line skeleton-text skeleton-short"></div>
            </div>

            <!-- 试卷卡片（有输入时） -->
            <div v-else class="paper-card">
              <div class="paper-card-header">
                <span class="paper-type-badge">{{ typeOptions.find(t => t.value === form.question_type)?.label }}</span>
                <span class="paper-difficulty">
                  <AppIcon v-for="n in 5" :key="n" name="star" :size="12" :class="{ active: difficultyStars >= n }" class="paper-star" />
                </span>
              </div>

              <!-- 题干 -->
              <div class="paper-stem">
                <LatexRender :text="form.stem || ''" />
              </div>

              <!-- 选择题选项 -->
              <div v-if="form.question_type === 'choice' && form.options.some(o => o.content)" class="paper-options">
                <div
                  v-for="opt in form.options.filter(o => o.content)"
                  :key="opt.label"
                  class="paper-opt"
                  :class="{ correct: form.correctAnswer === opt.label }"
                >
                  <span class="paper-opt-letter">{{ opt.label }}.</span>
                  <LatexRender :text="opt.content" :inline="true" />
                </div>
              </div>

              <!-- 填空题答案 -->
              <div v-else-if="form.question_type === 'fill' && form.blanks.some(b => b.answer)" class="paper-blanks">
                <div v-for="(blank, i) in form.blanks.filter(b => b.answer)" :key="i" class="paper-blank">
                  <span class="paper-blank-label">第{{ form.blanks.indexOf(blank) + 1 }}空：</span>
                  <LatexRender :text="blank.answer" :inline="true" />
                </div>
              </div>

              <!-- 解答题答案 -->
              <div v-else-if="form.question_type === 'solution' && form.solutionAnswer" class="paper-solution">
                <LatexRender :text="form.solutionAnswer" />
              </div>

              <!-- 答案 & 解析 -->
              <div class="paper-answer-block">
                <div class="paper-answer-label">答案</div>
                <div class="paper-answer-content">
                  <template v-if="form.question_type === 'choice' && form.correctAnswer">
                    <span class="paper-correct-answer">{{ form.correctAnswer }}</span>
                  </template>
                  <template v-else-if="form.question_type === 'fill' && form.blanks.some(b => b.answer)">
                    <span v-for="(blank, i) in form.blanks.filter(b => b.answer)" :key="i">
                      {{ form.blanks.indexOf(blank) + 1 }}. <LatexRender :text="blank.answer" :inline="true" />&nbsp;
                    </span>
                  </template>
                  <template v-else-if="form.question_type === 'solution' && form.solutionAnswer">
                    <LatexRender :text="form.solutionAnswer" />
                  </template>
                  <span v-else class="paper-muted">—</span>
                </div>
              </div>

              <div v-if="form.analysis" class="paper-answer-block">
                <div class="paper-answer-label">解析</div>
                <div class="paper-answer-content">
                  <LatexRender :text="form.analysis" />
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </template>

    <!-- 版本历史弹窗 -->
    <AppModal v-model="showHistory" title="历史版本">
      <div class="loading-hint">版本历史功能即将上线</div>
    </AppModal>

    <!-- 标签选择弹窗 -->
    <AppModal v-model="showTagDialog" title="选择解题方法/思想标签">
      <div class="tag-dialog-body">
        <p class="tag-dialog-hint">选择题目涉及的解题方法与数学思想标签</p>
        <div v-for="cat in tagCategories" :key="cat.name" class="tag-category">
          <div class="tag-category-title">{{ cat.name }}</div>
          <div class="tag-chips">
            <button
              v-for="tag in cat.tags"
              :key="tag"
              type="button"
              class="tag-chip"
              :class="{ active: form.tags.includes(tag) }"
              @click="toggleTag(tag)"
            >{{ tag }}</button>
          </div>
        </div>
      </div>
      <div class="form-actions">
        <AppButton variant="ghost" @click="showTagDialog = false">取消</AppButton>
        <AppButton variant="primary" @click="showTagDialog = false">完成（{{ form.tags.length }}）</AppButton>
      </div>
    </AppModal>

    <!-- 核心素养标签弹窗 -->
    <AppModal v-model="showLiteracyDialog" title="选择核心素养标签">
      <div class="tag-dialog-body">
        <p class="tag-dialog-hint">选择题目考查的数学核心素养</p>
        <div class="tag-category">
          <div class="tag-category-title">数学核心素养</div>
          <div class="tag-chips">
            <button
              v-for="lit in literacyTags"
              :key="lit"
              type="button"
              class="tag-chip"
              :class="{ active: form.literacy_tags.includes(lit) }"
              @click="toggleLiteracy(lit)"
            >{{ lit }}</button>
          </div>
        </div>
      </div>
      <div class="form-actions">
        <AppButton variant="ghost" @click="showLiteracyDialog = false">取消</AppButton>
        <AppButton variant="primary" @click="showLiteracyDialog = false">完成（{{ form.literacy_tags.length }}）</AppButton>
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
import { useSelectedKp } from '@/composables/useSelectedKp'

const route = useRoute()
const router = useRouter()
const toast = useToast()
const space = useSpaceStore()
const auth = useAuthStore()
const { selectedKpId, selectedKpName } = useSelectedKp()
const isNew = route.path.endsWith('/new')
const loading = ref(false)
const saving = ref(false)
const submitting = ref(false)
const isLoading = ref(false)
const kpLoading = ref(false)
const kpTree = ref<KnowledgePoint[]>([])
const showHistory = ref(false)
const showTagDialog = ref(false)
const showLiteracyDialog = ref(false)
const grades = ['初一', '初二', '初三', '高一', '高二', '高三']

// 标签分类数据：解题方法与数学思想
const tagCategories = [
  {
    name: '解题方法',
    tags: ['反证法', '数学归纳法', '枚举法', '构造法', '换元法', '配方法', '待定系数法', '面积法', '定义法', '综合法', '分析法'],
  },
  {
    name: '数学思想',
    tags: ['数形结合', '分类讨论', '化归与转化', '函数与方程', '整体思想', '极限思想', '模型思想', '统计思想'],
  },
  {
    name: '常见技巧',
    tags: ['极值点偏移', '隐零点', '零点分段', '放缩法', '参变分离', '齐次化', '设而不求', '韦达定理', '判别式法', '单调性分析'],
  },
  {
    name: '分析方法',
    tags: ['逆向分析', '正向推导', '穷举法', '图形分析', '代数变形', '三角代换', '向量法', '坐标法'],
  },
]

// 核心素养标签
const literacyTags = [
  '数学抽象', '逻辑推理', '数学建模', '直观想象', '数学运算', '数据分析',
]

function toggleTag(tag: string) {
  const idx = form.tags.indexOf(tag)
  if (idx >= 0) { form.tags.splice(idx, 1) } else { form.tags.push(tag) }
}

function toggleLiteracy(lit: string) {
  const idx = form.literacy_tags.indexOf(lit)
  if (idx >= 0) { form.literacy_tags.splice(idx, 1) } else { form.literacy_tags.push(lit) }
}

const gradeOptions = grades.map((g) => ({ label: g, value: g }))

// 子题型已移除

// 学年选项
const currentYear = new Date().getFullYear()
const academicYearOptions = [
  { label: `${currentYear - 1}-${String(currentYear).slice(2)}`, value: `${currentYear - 1}-${String(currentYear).slice(2)}` },
  { label: `${currentYear}-${String(currentYear + 1).slice(2)}`, value: `${currentYear}-${String(currentYear + 1).slice(2)}` },
  { label: `${currentYear + 1}-${String(currentYear + 2).slice(2)}`, value: `${currentYear + 1}-${String(currentYear + 2).slice(2)}` },
]

// 年级学期选项（合并年级+学期）
const gradeSemesterOptions = [
  ...['初一', '初二', '初三'].flatMap(g => [
    { label: `${g}上`, value: `${g}上` },
    { label: `${g}下`, value: `${g}下` },
  ]),
  ...['高一', '高二', '高三'].flatMap(g => [
    { label: `${g}上`, value: `${g}上` },
    { label: `${g}下`, value: `${g}下` },
  ]),
]

// 考试类型选项
const examTypeOptions = [
  { label: '期末', value: '期末' },
  { label: '期中', value: '期中' },
  { label: '月考', value: '月考' },
  { label: '周测', value: '周测' },
  { label: '模拟', value: '模拟' },
  { label: '高考', value: '高考' },
  { label: '中考', value: '中考' },
  { label: '竞赛', value: '竞赛' },
]

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

// 已选知识点（来自左侧知识树，非表单内选择）
const selectedKps = computed(() => {
  if (selectedKpId.value && selectedKpName.value) {
    return [{ id: selectedKpId.value, name: selectedKpName.value }]
  }
  return []
})

// 难度映射
const diffLabels = ['简单', '较易', '中等', '较难', '困难']
const _diffStars = ref(3)
const difficultyStars = computed({
  get: () => _diffStars.value,
  set: (v: number) => {
    _diffStars.value = v
    // 5星难度系数: 1→0.9, 2→0.75, 3→0.55, 4→0.35, 5→0.2
    form.difficulty_coefficient = [0.9, 0.75, 0.55, 0.35, 0.2][v - 1] ?? 0.55
  },
})

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
  region: '' as string,
  exam_type: '' as string,
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
  tags: [] as string[],
  literacy_tags: [] as string[],
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

// 题干配图上传（占位，后续对接文件上传API）
function handleImageUpload() {
  toast.info('图片上传功能即将上线')
}

// textarea 自适应高度
function autoResize(e: Event) {
  const el = e.target as HTMLTextAreaElement
  el.style.height = 'auto'
  el.style.height = el.scrollHeight + 'px'
}

// ===== 构建提交数据 =====
function buildPayload() {
  // 知识点来自左侧知识树选中项
  const kpIds = selectedKpId.value ? [selectedKpId.value] : (form.knowledgePointIds.length > 0 ? form.knowledgePointIds : [])
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
    region: form.region || null,
    exam_type: form.exam_type || null,
    source: form.source,
    analysis: form.analysis || null,
    knowledge_point_ids: kpIds.length > 0 ? kpIds : null,
    tags: form.tags.length > 0 ? form.tags : null,
    literacy_tags: form.literacy_tags.length > 0 ? form.literacy_tags : null,
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
    'gradingSteps', 'judgmentCorrect', 'knowledgePointIds', 'tags', 'literacy_tags', 'difficulty_coefficient', 'academic_year', 'grade_semester', 'region', 'exam_type', 'reviewer', 'reviewer_ids', 'internal_note']
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
    form.sub_type = (d as any).sub_type || ''
    form.difficulty_coefficient = (d as any).difficulty_coefficient ?? 0.5
    form.academic_year = (d as any).academic_year || ''
    form.grade_semester = (d as any).grade_semester || ''
    form.region = (d as any).region || ''
    form.exam_type = (d as any).exam_type || ''
    form.source = d.source || '原创'
    form.analysis = d.analysis || ''
    form.status = d.status
    form.version = d.version
    form.knowledgePointIds = d.knowledge_points?.map(k => k.id) || []
    form.tags = (d as any).tags || []
    form.literacy_tags = (d as any).literacy_tags || []
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

/* ============ 题型分段控件 + 难度 ============ */
/* 题型字段宽度 */
.meta-field-type {
  min-width: 80px;
}

/* 难度字段 */
.meta-field-diff {
  min-width: 100px;
}

.diff-row {
  display: flex;
  align-items: center;
  gap: 2px;
  min-height: 36px;
}

.star {
  color: var(--border-strong);
  background: none;
  border: none;
  cursor: pointer;
  padding: 0;
  display: inline-flex;
  transition: var(--transition-fast);
}

.star:hover {
  transform: scale(1.12);
}

.star.active {
  color: var(--star-color);
}

/* 难度系数输入 */
.diff-coef-input {
  width: 48px;
  padding: 7px 6px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-input);
  color: var(--text-primary);
  font-size: 12px;
  text-align: center;
  margin-left: 6px;
  font-family: inherit;
  box-sizing: border-box;
}

.diff-coef-input:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
}

/* ============ 元数据工具栏 ============ */
.meta-row {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-end;
  gap: 12px 8px;
  flex-shrink: 0;
  padding: 10px 14px;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-xs);
}

.meta-field {
  flex: 1;
  min-width: 0;
}

.meta-field-sm {
  flex: 1;
  min-width: 80px;
}

.field-label {
  display: block;
  font-size: 11px;
  font-weight: 600;
  margin-bottom: 5px;
  color: var(--text-muted);
  letter-spacing: 0.02em;
}

.num-input,
.text-input {
  width: 100%;
  padding: 7px 12px;
  border-radius: 8px;
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  color: var(--text-primary);
  font-size: 13px;
  line-height: 1.4;
  transition: var(--transition-fast);
  font-family: inherit;
  box-sizing: border-box;
  min-height: 36px;
}

.num-input:focus,
.text-input:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
  background: var(--bg-card);
}

.num-input-sm {
  flex: 0 0 76px;
  width: auto;
}

/* 让元数据栏内的下拉与输入框尺寸统一 */
.meta-field :deep(.app-select-wrapper) {
  width: 100%;
}

.kp-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  width: 100%;
  padding: 7px 12px;
  border-radius: 8px;
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  color: var(--text-secondary);
  font-size: 13px;
  cursor: pointer;
  box-sizing: border-box;
  min-height: 36px;
  transition: var(--transition-fast);
  font-family: inherit;
  box-sizing: border-box;
}

.kp-btn:hover {
  border-color: var(--accent);
  color: var(--accent);
  background: var(--accent-light);
}

/* ============ 主内容双栏 ============ */
.main-content {
  flex: 1;
  overflow: hidden;
  display: flex;
  gap: 14px;
  min-height: 0;
}

.edit-col {
  flex: 0 0 55%;
  min-width: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-xs);
  overflow: hidden;
}

.edit-col-inner {
  flex: 1;
  overflow-y: auto;
  padding: 14px 16px;
}

.preview-col {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  overflow: hidden;
}

.preview-col-inner {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  padding: 20px;
  box-sizing: border-box;
}

/* ============ 编辑区段 ============ */
.edit-section {
  margin-bottom: 14px;
}

.edit-section:last-child {
  margin-bottom: 0;
}

.section-label {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  font-weight: 650;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--text-secondary);
  margin-bottom: 10px;
}

.section-label .required {
  color: var(--danger);
  margin-left: 2px;
}

.edit-textarea {
  width: 100%;
  padding: 10px 12px;
  border-radius: var(--radius-sm);
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  color: var(--text-primary);
  font-size: 13px;
  line-height: 1.6;
  font-family: 'SF Mono', 'Menlo', 'Consolas', 'Courier New', monospace;
  resize: none;
  overflow-y: hidden;
  transition: var(--transition-fast);
  box-sizing: border-box;
}

.edit-textarea:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
  background: var(--bg-card);
}

/* 题干输入框 - 最低高度120px */
.stem-textarea {
  min-height: 120px;
}

/* 解析输入框 - 最低高度160px */
.analysis-textarea {
  min-height: 160px;
}

/* 题干容器 - 图片按钮挂载在右下角 */
.stem-wrap {
  position: relative;
}

.stem-wrap .edit-textarea {
  padding-bottom: 35px;
}

/* 图片上传按钮 - 挂载在题干右下角内部 */
.img-upload-btn {
  position: absolute;
  bottom: 5px;
  right: 8px;
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 1px 8px;
  border: 1px dashed var(--border-strong);
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.8);
  color: var(--text-muted);
  font-size: 11px;
  font-family: inherit;
  cursor: pointer;
  transition: var(--transition-fast);
  line-height: 1.5;
  z-index: 1;
}

[data-theme='dark'] .img-upload-btn {
  background: rgba(28, 28, 30, 0.8);
}

.img-upload-btn:hover {
  border-color: var(--accent);
  color: var(--accent);
  background: var(--accent-light);
}

/* 选择题选项 Grid */
.choice-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 6px;
}

.choice-grid .add-btn-sm {
  grid-column: 1;
  justify-self: start;
  width: fit-content;
  max-width: 200px;
}

/* 填空题紧凑布局 */
.blank-wrap {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  align-items: flex-end;
}

.blank-item {
  display: flex;
  align-items: center;
  gap: 4px;
}

.blank-input {
  width: 100px !important;
  flex: none !important;
}

/* 选项行 */
.opt-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 6px;
}

.opt-input {
  flex: 1;
  min-width: 0;
  padding: 6px 32px 6px 10px;
  border-radius: 8px;
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  color: var(--text-primary);
  font-size: 13px;
  line-height: 1.4;
  transition: var(--transition-fast);
  font-family: inherit;
}

.opt-input:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
  background: var(--bg-card);
}

.step-input {
  flex: 1;
  min-width: 0;
  padding: 8px 12px;
  border-radius: 8px;
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  color: var(--text-primary);
  font-size: 13px;
  line-height: 1.4;
  transition: var(--transition-fast);
  font-family: inherit;
}

.step-input:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
  background: var(--bg-card);
}

.blank-label {
  font-size: 12px;
  color: var(--text-muted);
  width: 44px;
  flex-shrink: 0;
  font-weight: 550;
}

.grading-label {
  font-size: 12px;
  font-weight: 600;
  margin-top: 10px;
  margin-bottom: 8px;
  color: var(--text-secondary);
}

.icon-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  align-self: center;
  width: 30px;
  height: 30px;
  flex-shrink: 0;
  border: 1px solid var(--border-color);
  background: var(--bg-card);
  color: var(--text-muted);
  border-radius: 8px;
  cursor: pointer;
  transition: var(--transition-fast);
}

.icon-btn:hover {
  border-color: var(--danger);
  color: var(--danger);
  background: var(--danger-light);
}

.add-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  margin-top: 4px;
  padding: 7px 14px;
  border: 1px dashed var(--border-strong);
  background: transparent;
  color: var(--text-secondary);
  font-size: 13px;
  font-weight: 500;
  border-radius: 8px;
  cursor: pointer;
  transition: var(--transition-fast);
  font-family: inherit;
}

.add-btn:hover {
  border-color: var(--accent);
  color: var(--accent);
  border-style: solid;
  background: var(--accent-light);
}

.add-btn-sm {
  padding: 4px 10px;
  font-size: 12px;
  gap: 4px;
}

/* Radio */
.radio-group {
  display: flex;
  gap: 12px;
}

.radio-label {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 14px;
  cursor: pointer;
  padding: 8px 16px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border-color);
  background: var(--bg-input);
  color: var(--text-secondary);
  transition: var(--transition-fast);
  user-select: none;
}

.radio-label:hover {
  border-color: var(--border-strong);
}

.radio-label.checked {
  border-color: var(--accent);
  background: var(--accent-light);
  color: var(--accent);
  font-weight: 600;
}

.radio-label input {
  margin: 0;
  accent-color: var(--accent);
}

/* ============ 高级设置折叠 ============ */
.advanced-section {
  width: 100%;
  margin-top: 6px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  overflow: hidden;
  background: var(--bg-input);
}

.advanced-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  width: 100%;
  padding: 12px 16px;
  background: none;
  border: none;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary);
  cursor: pointer;
  transition: var(--transition-fast);
  font-family: inherit;
}

.advanced-header:hover {
  background: var(--bg-hover);
}

.advanced-title {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.collapse-arrow {
  transition: transform 0.2s ease;
  transform: rotate(-90deg);
  color: var(--text-muted);
  display: inline-flex;
}

.collapse-arrow.open {
  transform: rotate(0deg);
}

.advanced-body {
  padding: 4px 16px 16px;
  border-top: 1px solid var(--border-color);
}

.form-grid-2 {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 16px;
  margin-top: 12px;
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
  color: var(--text-secondary);
}

.reviewer-item input[type="checkbox"] {
  width: auto;
  accent-color: var(--accent);
}

.hint-line {
  margin-top: 6px;
}

/* ============ 试卷化预览 ============ */

/* 骨架屏 - 同试卷卡片样式 */
.preview-skeleton {
  background: #ffffff;
  border-radius: 8px;
  padding: 32px 28px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.06);
  border: 1px solid rgba(0, 0, 0, 0.04);
}

[data-theme='dark'] .preview-skeleton {
  background: #1c1c1e;
  border-color: rgba(255, 255, 255, 0.06);
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.25);
}

.skeleton-line {
  height: 14px;
  border-radius: 6px;
  background: linear-gradient(90deg, var(--bg-input) 25%, var(--bg-hover) 50%, var(--bg-input) 75%);
  background-size: 200% 100%;
  animation: skeleton-shimmer 1.5s ease-in-out infinite;
  margin-bottom: 10px;
}

.skeleton-title { width: 35%; height: 20px; margin-bottom: 20px; }
.skeleton-text { width: 100%; }
.skeleton-short { width: 70%; }
.skeleton-opt { width: 45%; height: 16px; }
.skeleton-answer { width: 30%; height: 16px; }
.skeleton-gap { height: 16px; }

@keyframes skeleton-shimmer {
  0% { background-position: 200% 0; }
  100% { background-position: -200% 0; }
}

/* 试卷卡片 - 悬浮纸张效果 */
.paper-card {
  background: #ffffff;
  border-radius: 8px;
  padding: 24px 28px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.06);
  border: 1px solid rgba(0, 0, 0, 0.04);
}

[data-theme='dark'] .paper-card {
  background: #1c1c1e;
  border-color: rgba(255, 255, 255, 0.06);
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.25);
}

.paper-card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 16px;
  padding-bottom: 12px;
  border-bottom: 1px solid #f0f0f0;
}

[data-theme='dark'] .paper-card-header {
  border-bottom-color: rgba(255, 255, 255, 0.06);
}

.paper-type-badge {
  font-size: 13px;
  font-weight: 600;
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

.paper-stem {
  font-size: 14px;
  line-height: 1.8;
  color: #1d1d1f;
  margin-bottom: 14px;
  word-break: break-word;
}

[data-theme='dark'] .paper-stem {
  color: #f5f5f7;
}

.paper-options {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px 24px;
  margin-bottom: 14px;
}

.paper-opt {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  font-size: 13px;
  line-height: 1.7;
  color: #3a3a3c;
  padding: 4px 0;
}

.paper-opt.correct {
  color: var(--accent);
  font-weight: 600;
}

[data-theme='dark'] .paper-opt {
  color: #d1d1d6;
}

.paper-opt-letter {
  font-weight: 600;
  flex-shrink: 0;
}

.paper-blanks,
.paper-solution {
  margin-bottom: 14px;
  font-size: 14px;
  line-height: 1.7;
}

.paper-blank {
  display: inline;
  margin-right: 12px;
}

.paper-blank-label {
  font-weight: 600;
  color: var(--text-secondary);
}

/* 答案/解析区块 */
.paper-answer-block {
  background: #f5f5f7;
  border-radius: 8px;
  padding: 12px 16px;
  margin-top: 10px;
}

[data-theme='dark'] .paper-answer-block {
  background: rgba(255, 255, 255, 0.04);
}

.paper-answer-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.04em;
  margin-bottom: 4px;
}

.paper-answer-content {
  font-size: 13px;
  line-height: 1.7;
  color: var(--text-primary);
}

.paper-correct-answer {
  font-weight: 700;
  font-size: 16px;
  color: var(--accent);
}

.paper-muted {
  color: var(--text-muted);
}

/* 知识点显示框（只读，来自左侧树选中） */
.kp-display {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  padding: 7px 12px;
  border-radius: 8px;
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  color: var(--text-secondary);
  font-size: 13px;
  box-sizing: border-box;
  min-height: 36px;
}

.kp-display .kp-empty {
  color: var(--text-muted);
  font-style: italic;
}

/* 标签弹窗 */
.tag-dialog-body {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.tag-dialog-hint {
  font-size: 13px;
  color: var(--text-muted);
  margin: 0 0 4px 0;
}

.tag-category-title {
  font-size: 12px;
  font-weight: 650;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--text-secondary);
  margin-bottom: 8px;
}

.tag-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.tag-chip {
  padding: 6px 14px;
  border-radius: 18px;
  border: 1px solid var(--border-color);
  background: var(--bg-input);
  color: var(--text-secondary);
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: var(--transition-fast);
  font-family: inherit;
}

.tag-chip:hover:not(.active) {
  border-color: var(--accent);
  color: var(--accent);
  background: var(--accent-light);
}

.tag-chip.active {
  background: var(--accent);
  color: #fff;
  border-color: var(--accent);
  font-weight: 600;
  box-shadow: 0 1px 3px rgba(0, 122, 255, 0.3);
}

.form-actions {
  display: flex;
  justify-content: flex-end;
  margin-top: 16px;
}

/* ============ 工具类（局部兜底，全局已存在） ============ */
.text-sm {
  font-size: 13px;
}

.text-muted {
  color: var(--text-muted);
}

/* ============ 响应式 ============ */
@media (max-width: 1500px) {
  .main-content {
    flex-direction: column;
  }
  .edit-col {
    flex: none;
    width: 100%;
  }
  .preview-col {
    flex: none;
    width: 100%;
    min-height: 400px;
  }
  .meta-row {
    flex-wrap: wrap;
  }
  .meta-field,
  .meta-field-sm {
    flex: 1 1 calc(33.33% - 14px);
  }
}

@media (max-width: 768px) {
  .choice-grid {
    grid-template-columns: 1fr;
  }
  .blank-wrap {
    flex-direction: column;
    align-items: stretch;
  }
  .blank-input {
    width: 100% !important;
  }
}

@media (max-width: 640px) {
  .edit-page {
    padding: 12px;
  }
  .meta-row {
    flex-wrap: wrap;
  }
  .form-grid-2 {
    grid-template-columns: 1fr;
  }
  .meta-field,
  .meta-field-sm {
    flex: 1 1 100%;
  }
  .top-bar-left,
  .top-bar-right {
    flex-wrap: wrap;
  }
}
</style>
