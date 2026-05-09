<template>
  <div class="edit-page">
    <!-- 加载骨架 -->
    <template v-if="!isNew && loading">
      <el-skeleton :rows="1" animated class="mb-4" />
      <el-skeleton :rows="12" animated />
    </template>

    <template v-else>
      <!-- ==================== 顶部操作栏 ==================== -->
      <div class="flex items-center justify-between mb-3 flex-shrink-0">
        <div class="flex items-center gap-2">
          <el-button text @click="handleBack">← 返回</el-button>
          <el-button text @click="handleAi">
            <span class="text-indigo-500">🤖</span> AI 智能识别
          </el-button>
          <h1 class="text-lg font-bold ml-1">{{ isNew ? '录入新题' : '编辑题目' }}</h1>
          <el-tag v-if="!isNew" size="small" class="ml-2">v{{ form.version }}</el-tag>
        </div>
        <div class="flex items-center gap-2">
          <el-button v-if="!isNew" text @click="showHistory = true">
            📋 历史版本
          </el-button>
          <el-button @click="handleSave(false)" :loading="saving" type="default">💾 保存</el-button>
          <el-button @click="handleSave(true)" :loading="saving" type="success">🚀 提交审核</el-button>
        </div>
      </div>

      <!-- ==================== 可折叠属性面板 ==================== -->
      <div class="mb-3 space-y-1 flex-shrink-0">
        <!-- 题目来源 & 知识点 -->
        <el-collapse v-model="activeCollapse">
          <el-collapse-item title="📌 题目来源 & 知识点标签" name="source">
            <el-row :gutter="16">
              <el-col :span="12">
                <div class="text-xs text-gray-400 mb-1">来源类型</div>
                <el-select v-model="form.source" style="width:100%" size="small">
                  <el-option label="原创" value="原创" />
                  <el-option label="改编" value="改编" />
                  <el-option label="高考真题" value="高考真题" />
                  <el-option label="模拟题" value="模拟题" />
                  <el-option label="名校试卷" value="名校试卷" />
                </el-select>
              </el-col>
              <el-col :span="12">
                <div class="text-xs text-gray-400 mb-1">知识点</div>
                <el-popover placement="bottom" :width="300" trigger="click">
                  <template #reference>
                    <el-tag v-for="kp in selectedKps" :key="kp.id" closable class="mr-1 mb-1" size="small" @close="removeKp(kp.id)">
                      {{ kp.name }}
                    </el-tag>
                    <el-button size="small" text>＋ 添加标签</el-button>
                  </template>
                  <div style="max-height:240px; overflow-y:auto">
                    <div v-for="node in kpTree" :key="node.id" class="mb-1">
                      <el-checkbox v-model="form.knowledgePointIds" :label="node.id" :value="node.id" size="small">
                        <b>{{ node.name }}</b>
                      </el-checkbox>
                      <div class="ml-4" v-if="node.children?.length">
                        <div v-for="c in node.children" :key="c.id">
                          <el-checkbox v-model="form.knowledgePointIds" :label="c.id" :value="c.id" size="small">
                            {{ c.name }}
                          </el-checkbox>
                        </div>
                      </div>
                    </div>
                    <div v-if="!kpLoading && kpTree.length === 0" class="text-gray-400 text-xs py-2 text-center">暂无知识点</div>
                  </div>
                </el-popover>
              </el-col>
            </el-row>
          </el-collapse-item>

          <!-- 基础属性 -->
          <el-collapse-item title="⚙️ 基础属性" name="basic">
            <el-row :gutter="16">
              <el-col :span="6">
                <div class="text-xs text-gray-400 mb-1">题型</div>
                <el-select v-model="form.question_type" :disabled="!isNew" style="width:100%" size="small">
                  <el-option label="选择题" value="choice" />
                  <el-option label="填空题" value="fill" />
                  <el-option label="解答题" value="solution" />
                  <el-option label="判断题" value="judgment" />
                </el-select>
              </el-col>
              <el-col :span="6">
                <div class="text-xs text-gray-400 mb-1">难度</div>
                <el-rate v-model="difficultyStars" :max="3" show-text :texts="['简单', '中等', '困难']" size="small" />
              </el-col>
              <el-col :span="4">
                <div class="text-xs text-gray-400 mb-1">年级</div>
                <el-select v-model="form.grade" clearable style="width:100%" size="small">
                  <el-option v-for="g in grades" :key="g" :label="g" :value="g" />
                </el-select>
              </el-col>
              <el-col :span="4">
                <div class="text-xs text-gray-400 mb-1">学期</div>
                <el-select v-model="form.semester" clearable style="width:100%" size="small">
                  <el-option label="上学期" value="上学期" />
                  <el-option label="下学期" value="下学期" />
                  <el-option label="全学年" value="全学年" />
                </el-select>
              </el-col>
              <el-col :span="2">
                <div class="text-xs text-gray-400 mb-1">分值</div>
                <el-input-number v-model="form.default_score" :min="1" :max="100" size="small" style="width:100%" />
              </el-col>
              <el-col :span="2">
                <div class="text-xs text-gray-400 mb-1">耗时(分)</div>
                <el-input-number v-model="form.estimated_time" :min="1" :max="60" size="small" style="width:100%" />
              </el-col>
            </el-row>
          </el-collapse-item>

          <!-- 协作设置 -->
          <el-collapse-item title="👥 协作设置" name="collab">
            <el-row :gutter="16">
              <el-col :span="12">
                <div class="text-xs text-gray-400 mb-1">审核人</div>
                <el-select v-model="form.reviewer" filterable clearable placeholder="搜索教师…" style="width:100%" size="small">
                  <el-option label="张组长 (zhanglaoshi)" value="zhanglaoshi" />
                  <el-option label="系统管理员 (admin)" value="admin" />
                </el-select>
              </el-col>
              <el-col :span="12">
                <div class="text-xs text-gray-400 mb-1">内部备注（仅审核员可见）</div>
                <el-input v-model="form.internal_note" placeholder="记录命题意图或讨论要点…" size="small" />
              </el-col>
            </el-row>
          </el-collapse-item>
        </el-collapse>
      </div>

      <!-- ==================== 三组编辑/预览双栏 ==================== -->
      <div class="dual-sections">
        <!-- 题干 -->
        <div class="dual-row" ref="rowRefs[0]">
          <div class="dual-edit" :style="{ flex: `0 0 ${splitRatio * 100}%` }">
            <div class="dual-label">📖 题干 *</div>
            <el-input v-model="form.stem" type="textarea" :rows="5" placeholder="输入题目内容，LaTeX 公式用 $...$ 包裹" class="edit-textarea" />
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
            <div class="dual-label">📝 答案</div>
            <!-- 选择题选项 -->
            <div v-if="form.question_type === 'choice'">
              <div v-for="(opt, i) in form.options" :key="i" class="flex items-center gap-1 mb-1">
                <el-radio v-model="form.correctAnswer" :value="opt.label" size="small">{{ opt.label }}</el-radio>
                <el-input v-model="opt.content" :placeholder="`选项 ${opt.label}`" size="small" />
                <el-button v-if="form.options.length > 2" text size="small" type="danger" @click="form.options.splice(i, 1)">✕</el-button>
              </div>
              <el-button size="small" @click="addOption">+ 添加选项</el-button>
            </div>
            <div v-else-if="form.question_type === 'fill'">
              <div v-for="(blank, i) in form.blanks" :key="i" class="flex items-center gap-1 mb-1">
                <span class="text-gray-500 text-xs w-10">第{{ i+1 }}空</span>
                <el-input v-model="blank.answer" placeholder="填入答案" size="small" />
                <el-button v-if="form.blanks.length > 1" text size="small" type="danger" @click="form.blanks.splice(i, 1)">✕</el-button>
              </div>
              <el-button size="small" @click="form.blanks.push({ position: form.blanks.length + 1, answer: '' })">+ 添加填空位</el-button>
            </div>
            <div v-else-if="form.question_type === 'solution'">
              <el-input v-model="form.solutionAnswer" type="textarea" :rows="3" placeholder="完整解答过程，支持 $...$ LaTeX" class="edit-textarea" />
              <div class="text-xs font-medium mt-2 mb-1">分步评分</div>
              <div v-for="(step, i) in form.gradingSteps" :key="i" class="flex items-center gap-1 mb-1">
                <el-input v-model="step.label" placeholder="步骤名" size="small" style="width:130px" />
                <el-input-number v-model="step.points" :min="0" :max="20" size="small" style="width:70px" />
                <span class="text-xs text-gray-400">分</span>
                <el-button v-if="form.gradingSteps.length > 1" text size="small" type="danger" @click="form.gradingSteps.splice(i, 1)">✕</el-button>
              </div>
              <el-button size="small" @click="form.gradingSteps.push({ label: '', points: 1, description: '' })">+ 添加评分步骤</el-button>
            </div>
            <div v-else-if="form.question_type === 'judgment'">
              <el-radio-group v-model="form.judgmentCorrect" size="small">
                <el-radio :value="true">正确</el-radio>
                <el-radio :value="false">错误</el-radio>
              </el-radio-group>
            </div>
          </div>
          <div class="dual-divider" @mousedown.prevent="startResize(1, $event)" />
          <div class="dual-preview" :style="{ flex: `0 0 ${(1 - splitRatio) * 100}%` }">
            <div class="dual-label">答案预览</div>
            <div class="preview-box">
              <!-- 选择题预览 -->
              <div v-if="form.question_type === 'choice'">
                <div v-for="opt in form.options.filter(o=>o.content)" :key="opt.label"
                     class="py-1 px-2 mb-1 rounded border text-sm"
                     :class="form.correctAnswer === opt.label ? 'border-green-400 bg-green-50' : 'border-gray-200'">
                  <span class="font-mono mr-1">{{ opt.label }}.</span>
                  <LatexRender :text="opt.content" :inline="true" />
                  <el-tag v-if="form.correctAnswer === opt.label" size="small" type="success" class="ml-1">✓</el-tag>
                </div>
              </div>
              <div v-else-if="form.question_type === 'judgment'">
                <el-tag :type="form.judgmentCorrect ? 'success' : 'danger'" size="large">
                  {{ form.judgmentCorrect ? '正确' : '错误' }}
                </el-tag>
              </div>
              <LatexRender v-else-if="form.solutionAnswer" :text="form.solutionAnswer" />
              <span v-else class="text-gray-300">（等待输入…）</span>
            </div>
          </div>
        </div>

        <!-- 解析 -->
        <div class="dual-row" ref="rowRefs[2]">
          <div class="dual-edit" :style="{ flex: `0 0 ${splitRatio * 100}%` }">
            <div class="dual-label">💡 解析</div>
            <el-input v-model="form.analysis" type="textarea" :rows="4" placeholder="解题思路与易错点，支持 $...$ LaTeX" class="edit-textarea" />
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
    <el-dialog v-model="showHistory" title="历史版本" width="500">
      <div class="text-gray-400 text-sm py-8 text-center">版本历史功能即将上线</div>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, computed, watch, onMounted, onBeforeUnmount } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Loading } from '@element-plus/icons-vue'
import { questionApi, kpApi, type KnowledgePoint } from '@/api/client'
import LatexRender from '@/components/LatexRender.vue'

const route = useRoute()
const router = useRouter()
const isNew = route.path.endsWith('/new')
const loading = ref(false)
const saving = ref(false)
const kpLoading = ref(false)
const kpTree = ref<KnowledgePoint[]>([])
const showHistory = ref(false)
const grades = ['初一', '初二', '初三', '高一', '高二', '高三']

// 可拖拽分隔条
const splitRatio = ref(0.55) // 编辑区55% 预览区45%
const isDragging = ref(false)
const currentRow = ref(-1)
const rowRefs = [ref<HTMLElement>(), ref<HTMLElement>(), ref<HTMLElement>()]

function startResize(rowIdx: number, e: MouseEvent) {
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
  ratio = Math.max(0.2, Math.min(0.8, ratio)) // 限制 20%~80%
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
const activeCollapse = ref(['source', 'basic', 'collab'])

// 已选知识点名称映射
const kpMap = ref<Record<string, string>>({})
const selectedKps = computed(() =>
  form.knowledgePointIds.map(id => ({ id, name: kpMap.value[id] || id.substring(0, 8) }))
)
function removeKp(id: string) {
  form.knowledgePointIds = form.knowledgePointIds.filter(k => k !== id)
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
  reviewer: '',
  internal_note: '',
  status: '',
  version: 1,
  hasUnsaved: false,
})

// ===== 返回检测 =====
function handleBack() {
  if (form.hasUnsaved) {
    ElMessageBox.confirm('有未保存的修改，确定离开吗？', '未保存提示', {
      confirmButtonText: '离开', cancelButtonText: '取消', type: 'warning',
    }).then(() => goBack()).catch(() => {})
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
  ElMessage.info('AI 智能识别功能即将上线')
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
  if (!form.stem.trim()) { ElMessage.warning('请输入题干'); return }
  if (form.question_type === 'choice' && !form.correctAnswer) { ElMessage.warning('请选择正确答案'); return }
  saving.value = true
  try {
    const data = buildPayload()
    const res = isNew ? await questionApi.create(data) : await questionApi.update(route.params.id as string, data)
    const qid = res.data.id
    form.hasUnsaved = false
    clearDraft()
    if (submitAfter) { await questionApi.submit(qid); ElMessage.success('已创建并提交审核') }
    else { ElMessage.success(isNew ? '草稿已保存' : '已更新') }
    router.push(`/questions/${qid}`)
  } catch (e: any) { ElMessage.error(e.response?.data?.error || '操作失败') }
  finally { saving.value = false }
}

// ===== 自动保存草稿 =====
let autoSaveTimer: ReturnType<typeof setTimeout> | null = null
watch(() => ({ ...form }), () => {
  form.hasUnsaved = true
  if (autoSaveTimer) clearTimeout(autoSaveTimer)
  autoSaveTimer = setTimeout(() => {
    try {
      const key = isNew ? 'q-draft-new' : `q-draft-${route.params.id}`
      sessionStorage.setItem(key, JSON.stringify(form))
    } catch { /* quota exceeded */ }
  }, 3000)
}, { deep: true })

// ===== 加载 =====
// ===== 自动草稿恢复 =====
function getDraftKey() {
  return isNew ? 'q-draft-new' : `q-draft-${route.params.id}`
}

function restoreDraft() {
  const key = getDraftKey()
  try {
    const saved = sessionStorage.getItem(key)
    if (!saved) return
    const draft = JSON.parse(saved)
    // 检查是否有内容且是较新的草稿
    if (draft.stem || draft.analysis || draft.solutionAnswer) {
      ElMessageBox.confirm(
        '检测到未保存的草稿，是否恢复？',
        '恢复草稿',
        { confirmButtonText: '恢复', cancelButtonText: '丢弃', type: 'info' }
      ).then(() => {
        // 复制草稿内容但不覆盖 id/version 等
        const fields = ['stem', 'question_type', 'difficulty', 'default_score', 'grade', 'semester',
          'source', 'analysis', 'options', 'correctAnswer', 'blanks', 'solutionAnswer',
          'gradingSteps', 'judgmentCorrect', 'knowledgePointIds', 'reviewer', 'internal_note']
        for (const f of fields) {
          if (draft[f] !== undefined) (form as any)[f] = draft[f]
        }
        ElMessage.success('草稿已恢复')
      }).catch(() => {
        sessionStorage.removeItem(key)
      })
    }
  } catch { /* ignore */ }
}

function clearDraft() {
  try { sessionStorage.removeItem(getDraftKey()) }
  catch { /* ignore */ }
}

async function loadKpTree() {
  kpLoading.value = true
  try {
    const res = await kpApi.tree(); kpTree.value = res.data
    // 构建知识点名称映射
    function walk(nodes: KnowledgePoint[]) {
      for (const n of nodes) { kpMap.value[n.id] = n.name; if (n.children) walk(n.children) }
    }
    walk(res.data)
  } catch { /* handled */ }
  finally { kpLoading.value = false }
}

async function loadQuestion() {
  if (isNew) return
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
  finally { loading.value = false }
}

// ===== 窗口关闭检测 =====
function handleBeforeUnload(e: BeforeUnloadEvent) {
  if (form.hasUnsaved) { e.preventDefault(); e.returnValue = '' }
}
onMounted(() => {
  window.addEventListener('beforeunload', handleBeforeUnload)
  loadKpTree()
  loadQuestion().then(() => {
    // 等题目加载完成后检查草稿
    if (!isNew) restoreDraft()
  })
  if (isNew) restoreDraft()
})
onBeforeUnmount(() => {
  window.removeEventListener('beforeunload', handleBeforeUnload)
  if (autoSaveTimer) clearTimeout(autoSaveTimer)
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
  height: calc(100vh - 100px);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
/* 双栏区域滚动 */
.dual-sections {
  flex: 1;
  overflow-y: auto;
  padding-right: 4px;
}
/* 每组双栏 */
.dual-row {
  display: flex;
  gap: 0;
  margin-bottom: 12px;
  border: 1px solid #e5e7eb;
  border-radius: 8px;
  overflow: hidden;
  background: #fff;
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
  background: #f9fafb;
  border-left: 1px solid #e5e7eb;
}
.dual-divider {
  width: 4px;
  cursor: col-resize;
  background: #f3f4f6;
  flex-shrink: 0;
  transition: background 0.15s;
}
.dual-divider:hover {
  background: #6366f1;
}
.dual-label {
  font-size: 13px;
  font-weight: 600;
  margin-bottom: 8px;
  color: #374151;
}
.preview-box {
  font-size: 14px;
  line-height: 1.8;
  min-height: 60px;
}
.edit-textarea :deep(textarea) {
  font-family: 'Courier New', monospace;
  font-size: 13px;
}
</style>
