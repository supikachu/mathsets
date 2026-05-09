<template>
  <div v-loading="loading">
    <!-- 头部导航 -->
    <div class="flex items-center justify-between mb-4">
      <div class="flex items-center gap-3">
        <el-button text @click="$router.push('/questions')">← 返回列表</el-button>
        <h1 class="text-2xl font-bold">题目详情</h1>
      </div>
      <div class="flex gap-2">
        <!-- 草稿 → 编辑 / 提交审核 / 删除 -->
        <template v-if="q?.status === 'draft'">
          <el-button type="primary" @click="$router.push(`/questions/${q!.id}/edit`)">编辑</el-button>
          <el-button type="success" @click="submitReview" :loading="submitting">提交审核</el-button>
          <el-button type="danger" @click="confirmDelete">🗑️ 删除</el-button>
        </template>
        <!-- 驳回 → 编辑 -->
        <template v-else-if="q?.status === 'rejected'">
          <el-button type="primary" @click="$router.push(`/questions/${q!.id}/edit`)">重新编辑</el-button>
        </template>
        <!-- 待审核 → 审核（仅组长） -->
        <template v-else-if="q?.status === 'pending' && auth.isLeader">
          <el-button type="success" @click="handleReview('approved')">✅ 通过</el-button>
          <el-button type="danger" @click="handleReview('rejected')">❌ 驳回</el-button>
        </template>
        <!-- 已发布 → 停用（仅组长） -->
        <template v-else-if="q?.status === 'published' && auth.isLeader">
          <el-button type="warning">🚫 停用</el-button>
        </template>
      </div>
    </div>

    <el-row :gutter="20">
      <!-- 主内容 -->
      <el-col :span="17">
        <!-- 状态标签 -->
        <div class="mb-3 flex items-center gap-2">
          <el-tag :type="statusTag(q?.status || '')" size="large">
            {{ statusLabel(q?.status || '') }}
          </el-tag>
          <el-tag>{{ typeLabel(q?.question_type || '') }}</el-tag>
          <span>{{ diffLabel(q?.difficulty || '') }}</span>
          <span class="text-gray-400 text-sm">{{ q?.default_score }}分</span>
          <span class="text-gray-400 text-sm" v-if="q?.grade">· {{ q.grade }}</span>
          <span class="text-gray-400 text-sm" v-if="q?.semester">{{ q.semester }}</span>
        </div>

        <!-- 题干 -->
        <el-card shadow="never" class="mb-4">
          <template #header><span class="font-bold">📖 题干</span></template>
          <LatexRender :text="q?.stem || ''" />
        </el-card>

        <!-- 选项（选择题） -->
        <el-card v-if="q?.question_type === 'choice' && q?.options" shadow="never" class="mb-4">
          <template #header><span class="font-bold">🔘 选项</span></template>
          <div v-for="opt in q!.options" :key="opt.label"
               class="py-2 px-3 mb-2 rounded border"
               :class="isCorrect(opt.label) ? 'border-green-400 bg-green-50' : 'border-gray-200'">
            <span class="font-mono mr-2">{{ opt.label }}.</span>
            <LatexRender :text="opt.content" :inline="true" />
            <el-tag v-if="isCorrect(opt.label)" size="small" type="success" class="ml-2">正确答案</el-tag>
          </div>
        </el-card>

        <!-- 判断题 -->
        <el-card v-else-if="q?.question_type === 'judgment'" shadow="never" class="mb-4">
          <template #header><span class="font-bold">✅ 答案</span></template>
          <div>
            <el-tag :type="q?.correct_answer?.[0] === true ? 'success' : 'danger'" size="large">
              {{ q?.correct_answer?.[0] === true ? '正确' : '错误' }}
            </el-tag>
          </div>
        </el-card>

        <!-- 填空题答案 -->
        <el-card v-else-if="q?.question_type === 'fill' && q?.correct_answer" shadow="never" class="mb-4">
          <template #header><span class="font-bold">📝 参考答案</span></template>
          <div v-for="(item, i) in q!.correct_answer as any[]" :key="i" class="mb-2">
            <span class="text-gray-500 text-sm">第{{ i+1 }}空：</span>
            <LatexRender :text="item.answer || item" :inline="true" />
          </div>
        </el-card>

        <!-- 解答题答案 -->
        <el-card v-else-if="q?.question_type === 'solution' && q?.correct_answer" shadow="never" class="mb-4">
          <template #header><span class="font-bold">📝 参考答案</span></template>
          <LatexRender v-for="(ans, i) in q!.correct_answer as string[]" :key="i" :text="ans" />
        </el-card>

        <!-- 解析 -->
        <el-card v-if="q?.analysis" shadow="never" class="mb-4">
          <template #header><span class="font-bold">💡 解析</span></template>
          <LatexRender :text="q.analysis" />
        </el-card>
      </el-col>

      <!-- 侧边栏 -->
      <el-col :span="7">
        <!-- 知识点 -->
        <el-card shadow="never" class="mb-4">
          <template #header><span class="font-bold">🏷️ 知识点</span></template>
          <div v-if="q?.knowledge_points?.length">
            <el-tag v-for="kp in q!.knowledge_points" :key="kp.id" size="small" class="mb-1 mr-1">
              {{ kp.name }}
            </el-tag>
          </div>
          <div v-else class="text-gray-400 text-sm">未关联知识点</div>
        </el-card>

        <!-- 元信息 -->
        <el-card shadow="never" class="mb-4">
          <template #header><span class="font-bold">ℹ️ 元信息</span></template>
          <div class="text-sm space-y-2">
            <div><span class="text-gray-400">创建者：</span>{{ q?.creator_name || q?.creator_id?.substring(0, 8) || '—' }}</div>
            <div><span class="text-gray-400">版本：</span>v{{ q?.version }}</div>
            <div><span class="text-gray-400">创建时间：</span>{{ formatTime(q?.created_at) }}</div>
            <div><span class="text-gray-400">更新时间：</span>{{ formatTime(q?.updated_at) }}</div>
            <div v-if="q?.source"><span class="text-gray-400">来源：</span>{{ q.source }}</div>
          </div>
        </el-card>
      </el-col>
    </el-row>

    <!-- 驳回弹窗 -->
    <el-dialog v-model="rejectDialog" title="驳回原因" width="400">
      <el-input v-model="rejectComment" type="textarea" :rows="4" placeholder="请输入驳回原因..." />
      <template #footer>
        <el-button @click="rejectDialog = false">取消</el-button>
        <el-button type="primary" @click="confirmReject">确认驳回</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import { questionApi, type QuestionDetail } from '@/api/client'
import client from '@/api/client'
import { useAuthStore } from '@/stores/auth'
import LatexRender from '@/components/LatexRender.vue'

const route = useRoute()
const router = useRouter()
const auth = useAuthStore()
const q = ref<QuestionDetail | null>(null)
const loading = ref(false)
const submitting = ref(false)
const rejectDialog = ref(false)
const rejectComment = ref('')

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
    ElMessage.success('已提交审核')
    fetchDetail()
  } catch { /* handled */ }
  finally { submitting.value = false }
}

async function confirmDelete() {
  try {
    await ElMessageBox.confirm(
      '删除后不可恢复，确定要删除这道题吗？',
      '确认删除',
      { confirmButtonText: '删除', cancelButtonText: '取消', type: 'warning' }
    )
    await client.delete(`/questions/${route.params.id}`)
    ElMessage.success('已删除')
    router.push('/questions')
  } catch (e: any) {
    if (e !== 'cancel') { /* actual error, not cancel */ }
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
  await confirmReview('rejected', rejectComment.value)
  rejectDialog.value = false
}

async function confirmReview(action: string, comment?: string) {
  try {
    await client.post(`/questions/${route.params.id}/review`, { action, comment })
    ElMessage.success(action === 'approved' ? '已通过' : '已驳回')
    fetchDetail()
  } catch { /* handled */ }
}

function isCorrect(label: string): boolean {
  if (!q.value?.correct_answer) return false
  const ans = q.value.correct_answer
  if (Array.isArray(ans)) return ans.includes(label)
  return ans === label
}

function typeLabel(t: string) {
  const map: Record<string, string> = { choice: '选择题', fill: '填空题', solution: '解答题', judgment: '判断题' }
  return map[t] || t
}
function diffLabel(d: string) {
  const map: Record<string, string> = { easy: '🟢 简单', medium: '🟡 中等', hard: '🔴 困难' }
  return map[d] || d
}
function statusLabel(s: string) {
  const map: Record<string, string> = { draft: '📝 草稿', pending: '⏳ 待审核', rejected: '❌ 驳回', published: '✅ 已发布', disabled: '🚫 停用' }
  return map[s] || s
}
function statusTag(s: string) {
  const map: Record<string, string> = { draft: 'info', pending: 'warning', rejected: 'danger', published: 'success', disabled: '' }
  return map[s] || ''
}
function formatTime(t?: string) {
  return t ? t.replace('T', ' ').substring(0, 19) : ''
}

onMounted(fetchDetail)
</script>
