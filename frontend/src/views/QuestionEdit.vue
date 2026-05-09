<template>
  <div v-loading="loading" class="edit-page">
    <!-- ====== 顶栏 ====== -->
    <div class="flex items-center justify-between mb-4 flex-shrink-0">
      <div class="flex items-center gap-3">
        <el-button text @click="goBack">← 返回</el-button>
        <h1 class="text-xl font-bold">{{ isNew ? '录入新题' : '编辑题目' }}</h1>
      </div>
      <div class="flex items-center gap-2">
        <el-button type="primary" @click="handleSave(false)" :loading="saving">💾 保存草稿</el-button>
        <el-button type="success" @click="handleSave(true)" :loading="saving">🚀 提交审核</el-button>
      </div>
    </div>

    <!-- ====== 主体：左编辑 / 右预览 ====== -->
    <div class="edit-body">
      <!-- 左列：编辑区 -->
      <div class="edit-left">
        <!-- 题干编辑 -->
        <el-card shadow="never" class="mb-3">
          <template #header><span class="font-bold text-sm">📖 题干</span></template>
          <el-input
            v-model="form.stem"
            type="textarea"
            :rows="6"
            placeholder="输入题目内容，LaTeX 公式用 $...$ 包裹"
          />
        </el-card>

        <!-- 答案编辑（题型相关） -->
        <el-card shadow="never" class="mb-3">
          <template #header><span class="font-bold text-sm">📝 参考答案</span></template>

          <!-- 选择题 -->
          <div v-if="form.question_type === 'choice'">
            <div v-for="(opt, i) in form.options" :key="i" class="flex items-center gap-2 mb-2">
              <el-radio v-model="form.correctAnswer" :value="opt.label" size="small">
                {{ opt.label }}
              </el-radio>
              <el-input v-model="opt.content" :placeholder="`选项 ${opt.label}`" size="small" />
              <el-button v-if="form.options.length > 2" text size="small" type="danger" @click="form.options.splice(i, 1)">✕</el-button>
            </div>
            <el-button size="small" @click="addOption">+ 添加选项</el-button>
          </div>

          <!-- 填空题 -->
          <div v-else-if="form.question_type === 'fill'">
            <div v-for="(blank, i) in form.blanks" :key="i" class="flex items-center gap-2 mb-2">
              <span class="text-gray-500 text-sm w-12">第{{ i+1 }}空</span>
              <el-input v-model="blank.answer" placeholder="填入答案" size="small" />
              <el-button v-if="form.blanks.length > 1" text size="small" type="danger" @click="form.blanks.splice(i, 1)">✕</el-button>
            </div>
            <el-button size="small" @click="form.blanks.push({ position: form.blanks.length + 1, answer: '' })">+ 添加填空位</el-button>
          </div>

          <!-- 解答题 -->
          <div v-else-if="form.question_type === 'solution'">
            <el-input v-model="form.solutionAnswer" type="textarea" :rows="3" placeholder="完整解答过程，支持 $...$ LaTeX" />
            <el-divider />
            <div class="text-xs font-medium mb-1">分步评分</div>
            <div v-for="(step, i) in form.gradingSteps" :key="i" class="flex items-center gap-2 mb-1">
              <el-input v-model="step.label" placeholder="步骤名" size="small" style="width:120px" />
              <el-input-number v-model="step.points" :min="0" :max="20" size="small" style="width:80px" />
              <span class="text-xs text-gray-400">分</span>
              <el-button v-if="form.gradingSteps.length > 1" text size="small" type="danger" @click="form.gradingSteps.splice(i, 1)">✕</el-button>
            </div>
            <el-button size="small" @click="form.gradingSteps.push({ label: '', points: 1, description: '' })">+ 添加评分步骤</el-button>
          </div>

          <!-- 判断题 -->
          <div v-else-if="form.question_type === 'judgment'">
            <el-radio-group v-model="form.judgmentCorrect" size="small">
              <el-radio :value="true">正确</el-radio>
              <el-radio :value="false">错误</el-radio>
            </el-radio-group>
          </div>
        </el-card>

        <!-- 解析编辑 -->
        <el-card shadow="never" class="mb-3">
          <template #header><span class="font-bold text-sm">💡 解析</span></template>
          <el-input
            v-model="form.analysis"
            type="textarea"
            :rows="4"
            placeholder="解题思路与易错点，支持 $...$ LaTeX"
          />
        </el-card>
      </div>

      <!-- 右列：实时预览 + 属性 -->
      <div class="edit-right">
        <!-- 实时预览 -->
        <el-card shadow="never" class="mb-3">
          <template #header><span class="font-bold text-sm">👁️ 实时预览</span></template>
          <div class="preview-area">
            <div v-if="!form.stem && !form.solutionAnswer && !form.analysis" class="text-gray-400 text-sm py-4 text-center">
              在左侧输入内容后在此处实时预览
            </div>

            <div v-if="form.stem" class="mb-3">
              <div class="text-xs text-gray-400 mb-1">📖 题干</div>
              <LatexRender :text="form.stem" />
            </div>

            <div v-if="form.question_type === 'choice' && form.options.length" class="mb-3">
              <div class="text-xs text-gray-400 mb-1">🔘 选项</div>
              <div
                v-for="opt in form.options.filter(o=>o.content)"
                :key="opt.label"
                class="py-1 px-2 mb-1 rounded border text-sm"
                :class="form.correctAnswer === opt.label ? 'border-green-400 bg-green-50' : 'border-gray-200'"
              >
                <span class="font-mono mr-1">{{ opt.label }}.</span>
                <LatexRender :text="opt.content" :inline="true" />
                <el-tag v-if="form.correctAnswer === opt.label" size="small" type="success" class="ml-1">✓</el-tag>
              </div>
            </div>

            <div v-if="form.question_type === 'judgment'" class="mb-3">
              <div class="text-xs text-gray-400 mb-1">✅ 判断</div>
              <el-tag :type="form.judgmentCorrect ? 'success' : 'danger'" size="large">
                {{ form.judgmentCorrect ? '正确' : '错误' }}
              </el-tag>
            </div>

            <div v-if="form.solutionAnswer" class="mb-3">
              <div class="text-xs text-gray-400 mb-1">📝 参考答案</div>
              <LatexRender :text="form.solutionAnswer" />
            </div>

            <div v-if="form.analysis" class="mb-3">
              <div class="text-xs text-gray-400 mb-1">💡 解析</div>
              <LatexRender :text="form.analysis" />
            </div>
          </div>
        </el-card>

        <!-- 基础属性 -->
        <el-card shadow="never" class="mb-3">
          <template #header><span class="font-bold text-sm">📋 基础属性</span></template>
          <div class="space-y-2 text-sm">
            <div class="flex items-center gap-2">
              <span class="text-gray-400 w-14">题型</span>
              <el-select v-model="form.question_type" :disabled="!isNew" style="flex:1" size="small">
                <el-option label="选择题" value="choice" />
                <el-option label="填空题" value="fill" />
                <el-option label="解答题" value="solution" />
                <el-option label="判断题" value="judgment" />
              </el-select>
            </div>
            <div class="flex items-center gap-2">
              <span class="text-gray-400 w-14">难度</span>
              <el-rate v-model="difficultyStars" :max="3" show-text :texts="['简单', '中等', '困难']" size="small" />
            </div>
            <div class="flex items-center gap-2">
              <span class="text-gray-400 w-14">年级</span>
              <el-select v-model="form.grade" clearable style="flex:1" size="small">
                <el-option v-for="g in grades" :key="g" :label="g" :value="g" />
              </el-select>
            </div>
            <div class="flex items-center gap-2">
              <span class="text-gray-400 w-14">学期</span>
              <el-select v-model="form.semester" clearable style="flex:1" size="small">
                <el-option label="上学期" value="上学期" />
                <el-option label="下学期" value="下学期" />
                <el-option label="全学年" value="全学年" />
              </el-select>
            </div>
            <div class="flex items-center gap-2">
              <span class="text-gray-400 w-14">分值</span>
              <el-input-number v-model="form.default_score" :min="1" :max="100" size="small" style="flex:1" />
            </div>
            <div class="flex items-center gap-2">
              <span class="text-gray-400 w-14">来源</span>
              <el-select v-model="form.source" style="flex:1" size="small">
                <el-option label="原创" value="原创" />
                <el-option label="改编" value="改编" />
                <el-option label="引用" value="引用" />
              </el-select>
            </div>
          </div>
        </el-card>

        <!-- 知识点 -->
        <el-card shadow="never" class="mb-3">
          <template #header><span class="font-bold text-sm">🏷️ 知识点</span></template>
          <div v-if="kpLoading" class="text-center py-2"><el-icon class="is-loading"><Loading /></el-icon></div>
          <div v-else class="text-sm">
            <div v-for="node in kpTree" :key="node.id">
              <el-checkbox v-model="form.knowledgePointIds" :label="node.id" :value="node.id" size="small">
                {{ node.name }}
              </el-checkbox>
              <div class="ml-3" v-if="node.children?.length">
                <div v-for="c in node.children" :key="c.id">
                  <el-checkbox v-model="form.knowledgePointIds" :label="c.id" :value="c.id" size="small">
                    {{ c.name }}
                  </el-checkbox>
                </div>
              </div>
            </div>
            <div v-if="!kpLoading && kpTree.length === 0" class="text-gray-400 text-xs py-1">暂无知识点</div>
          </div>
        </el-card>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, computed, watch, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ElMessage } from 'element-plus'
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
const grades = ['初一', '初二', '初三', '高一', '高二', '高三']

// 难度 <=> 星星
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
  analysis: '',
  // 选择题
  options: [
    { label: 'A', content: '' },
    { label: 'B', content: '' },
    { label: 'C', content: '' },
    { label: 'D', content: '' },
  ] as { label: string; content: string }[],
  correctAnswer: '' as string | string[],
  // 填空题
  blanks: [{ position: 1, answer: '' }] as { position: number; answer: string }[],
  // 解答题
  solutionAnswer: '',
  gradingSteps: [] as { label: string; points: number; description: string }[],
  // 判断题
  judgmentCorrect: true,
  // 知识点
  knowledgePointIds: [] as string[],
  // 编辑时保留
  status: '',
  version: 1,
})

function addOption() {
  const labels = 'ABCDEFGH'
  const i = form.options.length
  if (i < 8) form.options.push({ label: labels[i], content: '' })
}

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

async function handleSave(submitAfter: boolean) {
  if (!form.stem.trim()) { ElMessage.warning('请输入题干'); return }
  if (form.question_type === 'choice' && !form.correctAnswer) { ElMessage.warning('请选择正确答案'); return }
  saving.value = true
  try {
    const data = buildPayload()
    const res = isNew ? await questionApi.create(data) : await questionApi.update(route.params.id as string, data)
    const qid = res.data.id
    if (submitAfter) { await questionApi.submit(qid); ElMessage.success('已创建并提交审核') }
    else { ElMessage.success(isNew ? '草稿已保存' : '已更新') }
    router.push(`/questions/${qid}`)
  } catch (e: any) { ElMessage.error(e.response?.data?.error || '操作失败') }
  finally { saving.value = false }
}

function goBack() {
  if (isNew) router.push('/questions')
  else router.push(`/questions/${route.params.id}`)
}

async function loadKpTree() {
  kpLoading.value = true
  try { const res = await kpApi.tree(); kpTree.value = res.data }
  catch { /* handled */ }
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
  } catch { /* handled */ }
  finally { loading.value = false }
}

watch(() => form.question_type, () => {
  if (isNew) {
    form.correctAnswer = ''
    form.blanks = [{ position: 1, answer: '' }]
    form.solutionAnswer = ''
    form.gradingSteps = []
    form.judgmentCorrect = true
  }
})

onMounted(() => { loadKpTree(); loadQuestion() })
</script>

<style scoped>
.edit-page {
  height: calc(100vh - 120px);
  display: flex;
  flex-direction: column;
}
.edit-body {
  flex: 1;
  display: flex;
  gap: 16px;
  overflow: hidden;
}
.edit-left {
  flex: 1;
  overflow-y: auto;
  padding-right: 4px;
}
.edit-right {
  width: 380px;
  overflow-y: auto;
  flex-shrink: 0;
}
.preview-area {
  min-height: 100px;
  font-size: 14px;
  line-height: 1.8;
}
</style>
