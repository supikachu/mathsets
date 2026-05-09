<template>
  <div v-loading="loading">
    <div class="flex items-center gap-3 mb-4">
      <el-button text @click="goBack">← 返回</el-button>
      <h1 class="text-2xl font-bold">{{ isNew ? '创建新题目' : '编辑题目' }}</h1>
    </div>

    <el-form :model="form" label-position="top" size="large">
      <el-row :gutter="20">
        <!-- 左侧：主要内容 -->
        <el-col :span="17">

          <!-- 基本信息 -->
          <el-card shadow="never" class="mb-4">
            <template #header><span class="font-bold">📋 基本信息</span></template>
            <el-row :gutter="16">
              <el-col :span="8">
                <el-form-item label="题型" required>
                  <el-select v-model="form.question_type" :disabled="!isNew" style="width:100%">
                    <el-option label="选择题" value="choice" />
                    <el-option label="填空题" value="fill" />
                    <el-option label="解答题" value="solution" />
                    <el-option label="判断题" value="judgment" />
                  </el-select>
                </el-form-item>
              </el-col>
              <el-col :span="6">
                <el-form-item label="难度" required>
                  <el-select v-model="form.difficulty" style="width:100%">
                    <el-option label="🟢 简单" value="easy" />
                    <el-option label="🟡 中等" value="medium" />
                    <el-option label="🔴 困难" value="hard" />
                  </el-select>
                </el-form-item>
              </el-col>
              <el-col :span="5">
                <el-form-item label="分值">
                  <el-input-number v-model="form.default_score" :min="1" :max="100" />
                </el-form-item>
              </el-col>
              <el-col :span="5">
                <el-form-item label="来源">
                  <el-select v-model="form.source" style="width:100%">
                    <el-option label="原创" value="原创" />
                    <el-option label="改编" value="改编" />
                    <el-option label="引用" value="引用" />
                  </el-select>
                </el-form-item>
              </el-col>
            </el-row>
            <el-row :gutter="16">
              <el-col :span="8">
                <el-form-item label="年级">
                  <el-select v-model="form.grade" clearable style="width:100%">
                    <el-option v-for="g in grades" :key="g" :label="g" :value="g" />
                  </el-select>
                </el-form-item>
              </el-col>
              <el-col :span="8">
                <el-form-item label="学期">
                  <el-select v-model="form.semester" clearable style="width:100%">
                    <el-option label="上学期" value="上学期" />
                    <el-option label="下学期" value="下学期" />
                    <el-option label="全学年" value="全学年" />
                  </el-select>
                </el-form-item>
              </el-col>
            </el-row>
          </el-card>

          <!-- 题干 -->
          <el-card shadow="never" class="mb-4">
            <template #header><span class="font-bold">📖 题干</span></template>
            <el-form-item required>
              <el-input
                v-model="form.stem"
                type="textarea"
                :rows="5"
                placeholder="请输入题目内容，支持 LaTeX 公式（如 $\\frac{1}{2}$）"
              />
            </el-form-item>
          </el-card>

          <!-- 题型特有区域 -->
          <!-- 选择题 -->
          <el-card v-if="form.question_type === 'choice'" shadow="never" class="mb-4">
            <template #header><span class="font-bold">🔘 选项</span></template>
            <div v-for="(opt, i) in form.options" :key="i" class="flex items-center gap-2 mb-2">
              <el-radio v-model="form.correctAnswer" :value="opt.label" size="large">
                {{ opt.label }}.
              </el-radio>
              <el-input v-model="opt.content" :placeholder="`选项 ${opt.label} 内容`" />
              <el-button v-if="form.options.length > 2" text type="danger" @click="removeOption(i)">✕</el-button>
            </div>
            <el-button size="small" @click="addOption">+ 添加选项</el-button>
            <div class="mt-2 text-xs text-gray-400">选中 radio 标记正确答案</div>
          </el-card>

          <!-- 填空题 -->
          <el-card v-if="form.question_type === 'fill'" shadow="never" class="mb-4">
            <template #header><span class="font-bold">📝 填空答案</span></template>
            <div v-for="(blank, i) in form.blanks" :key="i" class="flex items-center gap-2 mb-2">
              <span class="text-gray-500 w-8">第{{ i+1 }}空</span>
              <el-input v-model="blank.answer" placeholder="填入正确答案" />
              <el-button v-if="form.blanks.length > 1" text type="danger" @click="form.blanks.splice(i, 1)">✕</el-button>
            </div>
            <el-button size="small" @click="form.blanks.push({ position: form.blanks.length + 1, answer: '' })">
              + 添加填空位
            </el-button>
          </el-card>

          <!-- 解答题 -->
          <el-card v-if="form.question_type === 'solution'" shadow="never" class="mb-4">
            <template #header><span class="font-bold">📝 参考答案与评分标准</span></template>
            <el-form-item label="参考答案">
              <el-input v-model="form.solutionAnswer" type="textarea" :rows="4" placeholder="完整解答过程" />
            </el-form-item>
            <el-divider />
            <div class="font-medium mb-2">分步评分</div>
            <div v-for="(step, i) in form.gradingSteps" :key="i" class="flex items-center gap-2 mb-2">
              <el-input v-model="step.label" placeholder="步骤名" style="width:150px" />
              <el-input-number v-model="step.points" :min="0" :max="20" style="width:100px" />
              <span class="text-gray-400 text-sm">分</span>
              <el-button v-if="form.gradingSteps.length > 1" text type="danger" @click="form.gradingSteps.splice(i, 1)">✕</el-button>
            </div>
            <el-button size="small" @click="form.gradingSteps.push({ label: '', points: 1, description: '' })">
              + 添加评分步骤
            </el-button>
          </el-card>

          <!-- 判断题 -->
          <el-card v-if="form.question_type === 'judgment'" shadow="never" class="mb-4">
            <template #header><span class="font-bold">✅ 判断</span></template>
            <el-radio-group v-model="form.judgmentCorrect">
              <el-radio :value="true" size="large">正确</el-radio>
              <el-radio :value="false" size="large">错误</el-radio>
            </el-radio-group>
          </el-card>

          <!-- 解析 -->
          <el-card shadow="never" class="mb-4">
            <template #header><span class="font-bold">💡 解析</span></template>
            <el-input
              v-model="form.analysis"
              type="textarea"
              :rows="4"
              placeholder="解题思路和易错点分析"
            />
          </el-card>

          <!-- 操作按钮 -->
          <div class="flex gap-3 mb-6">
            <el-button type="primary" size="large" @click="handleSave(false)" :loading="saving">
              💾 保存草稿
            </el-button>
            <el-button type="success" size="large" @click="handleSave(true)" :loading="saving">
              🚀 提交审核
            </el-button>
          </div>
        </el-col>

        <!-- 右侧：知识点 -->
        <el-col :span="7">
          <el-card shadow="never" class="mb-4">
            <template #header><span class="font-bold">🏷️ 知识点</span></template>
            <div v-if="kpLoading" class="text-center py-4">
              <el-icon class="is-loading"><Loading /></el-icon>
            </div>
            <div v-else class="max-h-96 overflow-y-auto">
              <div v-for="node in kpTree" :key="node.id" class="ml-0">
                <el-checkbox
                  v-model="form.knowledgePointIds"
                  :label="node.id"
                  :value="node.id"
                  @change="(v: any) => onKpChange(node, v)"
                >
                  {{ node.name }}
                </el-checkbox>
                <div v-if="node.children?.length" class="ml-4">
                  <div v-for="child in node.children" :key="child.id">
                    <el-checkbox
                      v-model="form.knowledgePointIds"
                      :label="child.id"
                      :value="child.id"
                    >
                      {{ child.name }}
                    </el-checkbox>
                    <div v-if="child.children?.length" class="ml-4">
                      <div v-for="c2 in child.children" :key="c2.id">
                        <el-checkbox
                          v-model="form.knowledgePointIds"
                          :label="c2.id"
                          :value="c2.id"
                        >
                          {{ c2.name }}
                        </el-checkbox>
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            </div>
            <div v-if="!kpLoading && kpTree.length === 0" class="text-gray-400 text-sm py-2">
              暂无知识点，请先在知识点管理中添加
            </div>
          </el-card>

          <el-card v-if="!isNew" shadow="never">
            <template #header><span class="font-bold">ℹ️ 信息</span></template>
            <div class="text-sm space-y-2">
              <div><span class="text-gray-400">状态：</span>{{ form.status || '—' }}</div>
              <div><span class="text-gray-400">版本：</span>v{{ form.version || 1 }}</div>
            </div>
          </el-card>
        </el-col>
      </el-row>
    </el-form>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, onMounted, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ElMessage } from 'element-plus'
import { Loading } from '@element-plus/icons-vue'
import { questionApi, kpApi, type KnowledgePoint } from '@/api/client'

const route = useRoute()
const router = useRouter()
const isNew = route.path.endsWith('/new')
const loading = ref(false)
const saving = ref(false)
const kpLoading = ref(false)
const kpTree = ref<KnowledgePoint[]>([])
const grades = ['初一', '初二', '初三', '高一', '高二', '高三']

// 表单数据
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

// 题型变化时重置特有字段
watch(() => form.question_type, () => {
  if (isNew) {
    form.correctAnswer = ''
    form.blanks = [{ position: 1, answer: '' }]
    form.solutionAnswer = ''
    form.gradingSteps = []
    form.judgmentCorrect = true
  }
})

function addOption() {
  const labels = 'ABCDEFGH'
  const i = form.options.length
  if (i < 8) form.options.push({ label: labels[i], content: '' })
}

function removeOption(i: number) {
  form.options.splice(i, 1)
  if (form.correctAnswer === form.options[i]?.label) {
    form.correctAnswer = ''
  }
}

function onKpChange(node: KnowledgePoint, checked: any) {
  if (checked && node.children) {
    for (const c of node.children) {
      if (!form.knowledgePointIds.includes(c.id)) {
        form.knowledgePointIds.push(c.id)
      }
    }
  }
}

// 构建提交数据
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
      if (form.correctAnswer) {
        payload.correct_answer = [form.correctAnswer]
      }
      break
    case 'fill':
      payload.correct_answer = form.blanks.filter(b => b.answer.trim()).map(b => ({
        position: b.position,
        answer: b.answer.trim(),
      }))
      break
    case 'solution':
      payload.correct_answer = [form.solutionAnswer]
      if (form.gradingSteps.length > 0) {
        payload.grading_criteria = form.gradingSteps.filter(s => s.label)
      }
      break
    case 'judgment':
      payload.correct_answer = [form.judgmentCorrect]
      break
  }

  return payload
}

async function handleSave(submitAfter: boolean) {
  // 基本校验
  if (!form.stem.trim()) {
    ElMessage.warning('请输入题干')
    return
  }
  if (form.question_type === 'choice' && !form.correctAnswer) {
    ElMessage.warning('请选择正确答案')
    return
  }

  saving.value = true
  try {
    const data = buildPayload()
    let res
    if (isNew) {
      res = await questionApi.create(data)
    } else {
      res = await questionApi.update(route.params.id as string, data)
    }

    const qid = res.data.id

    if (submitAfter) {
      await questionApi.submit(qid)
      ElMessage.success('创建成功，已提交审核')
    } else {
      ElMessage.success(isNew ? '草稿已保存' : '已更新')
    }

    router.push(`/questions/${qid}`)
  } catch (e: any) {
    ElMessage.error(e.response?.data?.error || '操作失败')
  } finally {
    saving.value = false
  }
}

function goBack() {
  if (isNew) router.push('/questions')
  else router.push(`/questions/${route.params.id}`)
}

async function loadKpTree() {
  kpLoading.value = true
  try {
    const res = await kpApi.tree()
    kpTree.value = res.data
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
      form.blanks = (d.correct_answer as any[]).map((b: any) => ({
        position: b.position,
        answer: b.answer,
      }))
    } else if (d.question_type === 'solution') {
      if (Array.isArray(d.correct_answer)) form.solutionAnswer = d.correct_answer[0] || ''
      if (d.grading_criteria) form.gradingSteps = d.grading_criteria as any
    } else if (d.question_type === 'judgment') {
      if (Array.isArray(d.correct_answer)) form.judgmentCorrect = d.correct_answer[0] === true
    }
  } catch { /* handled */ }
  finally { loading.value = false }
}

onMounted(() => {
  loadKpTree()
  loadQuestion()
})
</script>
