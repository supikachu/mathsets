<template>
  <div v-loading="loading">
    <div class="flex items-center justify-between mb-4">
      <div class="flex items-center gap-3">
        <el-button text @click="$router.push('/papers')">← 返回列表</el-button>
        <h1 class="text-xl font-bold">{{ paper?.title || '试卷详情' }}</h1>
        <el-tag :type="statusTag(paper?.status || '')" size="small">{{ statusLabel(paper?.status || '') }}</el-tag>
      </div>
      <div class="flex gap-2">
        <template v-if="paper?.status === 'draft'">
          <el-button @click="showBasicDialog = true">⚙️ 编辑信息</el-button>
          <el-button type="success" @click="handlePublish" :loading="publishing">🚀 发布</el-button>
          <el-button type="danger" @click="handleDelete">🗑️ 删除</el-button>
        </template>
      </div>
    </div>

    <!-- 基本信息 -->
    <el-card shadow="never" class="mb-4">
      <div class="flex items-center gap-6 text-sm">
        <span>科目: <b>{{ paper?.subject || '数学' }}</b></span>
        <span>年级: <b>{{ paper?.grade || '—' }}</b></span>
        <span>题数: <b>{{ paper?.questions?.length || 0 }}</b></span>
        <span>总分: <b class="text-lg text-blue-600">{{ totalScore }}</b></span>
        <span v-if="paper?.duration_minutes">时长: <b>{{ paper.duration_minutes }} 分钟</b></span>
        <span class="text-gray-400">创建者: {{ paper?.creator_name || '—' }}</span>
      </div>
      <div v-if="paper?.description" class="text-sm text-gray-500 mt-2">{{ paper.description }}</div>
    </el-card>

    <!-- 题目列表 -->
    <el-card shadow="never">
      <template #header>
        <div class="flex items-center justify-between">
          <span class="font-bold">📝 题目列表</span>
          <el-button size="small" type="primary" @click="showAddDialog = true">➕ 添加题目</el-button>
        </div>
      </template>

      <el-empty v-if="!paper?.questions?.length" description="暂无题目，点击上方按钮添加" />

      <div v-else>
        <div v-for="(q, i) in paper?.questions" :key="q.id"
             class="flex items-center gap-3 py-3 border-b border-gray-100 last:border-0 group">
          <span class="text-gray-400 text-sm w-8">{{ i + 1 }}</span>
          <div class="flex-1 min-w-0">
            <div class="text-sm truncate">{{ q.stem }}</div>
            <div class="text-xs text-gray-400 mt-1">
              <el-tag size="small" class="mr-1">{{ typeLabel(q.question_type) }}</el-tag>
              {{ diffLabel(q.difficulty) }}
              <span v-if="q.section" class="ml-2">[{{ q.section }}]</span>
            </div>
          </div>
          <div class="flex items-center gap-2">
            <span class="text-gray-400 text-xs">分值</span>
            <el-input-number v-model="q.score" :min="0" :max="50" size="small" style="width:80px" @change="updateQScore(q)" />
          </div>
          <el-button text type="danger" size="small" @click="removeQ(q)" class="opacity-0 group-hover:opacity-100">✕</el-button>
        </div>
      </div>
    </el-card>

    <!-- 编辑基本信息弹窗 -->
    <el-dialog v-model="showBasicDialog" title="编辑试卷信息" width="450">
      <el-form :model="editForm" label-position="top">
        <el-form-item label="标题">
          <el-input v-model="editForm.title" />
        </el-form-item>
        <el-form-item label="描述">
          <el-input v-model="editForm.description" type="textarea" :rows="2" />
        </el-form-item>
        <el-row :gutter="12">
          <el-col :span="12">
            <el-form-item label="年级">
              <el-select v-model="editForm.grade" style="width:100%">
                <el-option v-for="g in grades" :key="g" :label="g" :value="g" />
              </el-select>
            </el-form-item>
          </el-col>
          <el-col :span="12">
            <el-form-item label="时长(分)">
              <el-input-number v-model="editForm.duration_minutes" :min="0" :max="300" style="width:100%" />
            </el-form-item>
          </el-col>
        </el-row>
      </el-form>
      <template #footer>
        <el-button @click="showBasicDialog = false">取消</el-button>
        <el-button type="primary" @click="saveBasic" :loading="saving">保存</el-button>
      </template>
    </el-dialog>

    <!-- 添加题目弹窗 -->
    <el-dialog v-model="showAddDialog" title="添加题目到试卷" width="650">
      <div class="mb-3">
        <el-input v-model="searchKeyword" placeholder="🔍 搜索题干..." clearable />
      </div>
      <el-table :data="questionPool" v-loading="searching" max-height="400" @row-click="addToPaper">
        <el-table-column label="题干" min-width="250">
          <template #default="{ row }">
            <span class="text-sm line-clamp-1">{{ row.stem }}</span>
          </template>
        </el-table-column>
        <el-table-column label="题型" width="70" align="center">
          <template #default="{ row }">{{ typeLabel(row.question_type) }}</template>
        </el-table-column>
        <el-table-column label="难度" width="70" align="center">
          <template #default="{ row }">{{ diffLabel(row.difficulty) }}</template>
        </el-table-column>
        <el-table-column label="操作" width="60" align="center">
          <template #default>
            <el-button size="small" type="primary">➕ 加入</el-button>
          </template>
        </el-table-column>
      </el-table>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, computed, watch, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import { paperApi, questionApi, type PaperDetail, type QuestionSummary } from '@/api/client'

const route = useRoute()
const router = useRouter()
const paper = ref<PaperDetail | null>(null)
const loading = ref(false)
const saving = ref(false)
const publishing = ref(false)
const showBasicDialog = ref(false)
const showAddDialog = ref(false)
const searchKeyword = ref('')
const questionPool = ref<QuestionSummary[]>([])
const searching = ref(false)
const grades = ['初一', '初二', '初三', '高一', '高二', '高三']

const editForm = reactive({ title: '', description: '', grade: '' as string | undefined, duration_minutes: undefined as number | undefined })

const totalScore = computed(() => paper.value?.questions?.reduce((s, q) => s + q.score, 0) || 0)

async function fetchPaper() {
  loading.value = true
  try {
    const res = await paperApi.get(route.params.id as string)
    paper.value = res.data
    editForm.title = res.data.title
    editForm.description = res.data.description || ''
    editForm.grade = res.data.grade || undefined
    editForm.duration_minutes = res.data.duration_minutes || undefined
  } catch { /* handled */ }
  finally { loading.value = false }
}

async function saveBasic() {
  saving.value = true
  try {
    const res = await paperApi.update(route.params.id as string, {
      title: editForm.title,
      description: editForm.description || null,
      grade: editForm.grade || null,
      duration_minutes: editForm.duration_minutes || null,
    })
    paper.value = res.data
    showBasicDialog.value = false
    ElMessage.success('已保存')
  } catch { /* handled */ }
  finally { saving.value = false }
}

async function handlePublish() {
  if (!paper.value?.questions?.length) {
    ElMessage.warning('试卷中没有题目，无法发布'); return
  }
  try {
    await ElMessageBox.confirm('确定发布试卷？发布后不可编辑题序。', '确认发布', { type: 'info' })
    await paperApi.publish(route.params.id as string)
    ElMessage.success('已发布')
    fetchPaper()
  } catch { /* cancel */ }
}

async function handleDelete() {
  try {
    await ElMessageBox.confirm('删除后不可恢复，确定删除？', '确认删除', { type: 'warning' })
    await paperApi.delete(route.params.id as string)
    ElMessage.success('已删除')
    router.push('/papers')
  } catch { /* cancel */ }
}

async function searchQuestions(keyword?: string) {
  searching.value = true
  try {
    const res = await questionApi.list({ keyword, page_size: 50, status: 'published' })
    questionPool.value = res.data.filter(q => !paper.value?.questions?.find(pq => pq.question_id === q.id))
  } catch { /* handled */ }
  finally { searching.value = false }
}

async function addToPaper(row: QuestionSummary) {
  try {
    await paperApi.addQuestion(route.params.id as string, { question_id: row.id, score: row.default_score || 5 })
    ElMessage.success('添加成功')
    fetchPaper()
    showAddDialog.value = false
  } catch (e: any) { ElMessage.error(e.response?.data?.error || '添加失败') }
}

async function updateQScore(q: any) {
  try {
    await paperApi.updateQuestion(route.params.id as string, q.question_id, { score: q.score })
  } catch { /* handled */ }
}

async function removeQ(q: any) {
  try {
    await ElMessageBox.confirm('从试卷中移除这道题？', '确认', { type: 'warning' })
    await paperApi.removeQuestion(route.params.id as string, q.question_id)
    ElMessage.success('已移除')
    fetchPaper()
  } catch { /* cancel */ }
}

watch(showAddDialog, (v) => { if (v) searchQuestions() })
watch(searchKeyword, () => { if (showAddDialog.value) searchQuestions(searchKeyword.value) })

function typeLabel(t: string) {
  const map: Record<string, string> = { choice: '选择', fill: '填空', solution: '解答', judgment: '判断' }
  return map[t] || t
}
function diffLabel(d: string) {
  const map: Record<string, string> = { easy: '🟢', medium: '🟡', hard: '🔴' }
  return map[d] || d
}
function statusLabel(s: string) {
  const map: Record<string, string> = { draft: '📝 草稿', published: '✅ 已发布', archived: '📦 归档' }
  return map[s] || s
}
function statusTag(s: string) {
  const map: Record<string, string> = { draft: 'info', published: 'success', archived: '' }
  return map[s] || ''
}

onMounted(fetchPaper)
</script>

<style scoped>
.line-clamp-1 {
  display: -webkit-box;
  -webkit-line-clamp: 1;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
</style>
