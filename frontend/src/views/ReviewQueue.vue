<template>
  <div>
    <div class="flex items-center justify-between mb-4">
      <h1 class="text-2xl font-bold">🔍 审核队列</h1>
      <el-tag v-if="!loading" :type="list.length > 0 ? 'warning' : 'info'" size="large">
        待审核: {{ list.length }} 题
      </el-tag>
    </div>

    <el-card shadow="never" v-loading="loading">
      <el-empty v-if="!loading && list.length === 0" description="🎉 所有题目已审核完毕" />

      <div v-else>
        <div
          v-for="q in list"
          :key="q.id"
          class="flex items-center gap-4 py-4 px-3 border-b border-gray-100 last:border-0 hover:bg-gray-50 rounded transition cursor-pointer"
          @click="$router.push(`/questions/${q.id}`)"
        >
          <!-- 题干 -->
          <div class="flex-1 min-w-0">
            <div class="text-base truncate">{{ q.stem }}</div>
            <div class="flex items-center gap-3 mt-1 text-xs text-gray-400">
              <el-tag size="small">{{ typeLabel(q.question_type) }}</el-tag>
              <span>{{ diffLabel(q.difficulty) }}</span>
              <span>创建者: {{ q.creator_id?.substring(0, 8) || '—' }}</span>
              <span>{{ formatTime(q.updated_at) }}</span>
            </div>
          </div>

          <!-- 操作 -->
          <el-button type="success" size="small" @click.stop="handleReview(q, 'approved')">通过</el-button>
          <el-button type="danger" size="small" @click.stop="handleReview(q, 'rejected')">驳回</el-button>
        </div>
      </div>
    </el-card>

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
import { ElMessage } from 'element-plus'
import { questionApi, type QuestionSummary } from '@/api/client'
import client from '@/api/client'

const list = ref<QuestionSummary[]>([])
const loading = ref(true)
const rejectDialog = ref(false)
const rejectComment = ref('')
const currentQ = ref<QuestionSummary | null>(null)

async function fetchList() {
  loading.value = true
  try {
    const res = await questionApi.list({ status: 'pending', page_size: 50 })
    list.value = res.data
  } catch { /* handled */ }
  finally { loading.value = false }
}

function handleReview(q: QuestionSummary, action: string) {
  currentQ.value = q
  if (action === 'rejected') {
    rejectDialog.value = true
  } else {
    confirmReview(q, action)
  }
}

async function confirmReject() {
  if (currentQ.value) {
    await confirmReview(currentQ.value, 'rejected', rejectComment.value)
    rejectComment.value = ''
  }
  rejectDialog.value = false
}

async function confirmReview(q: QuestionSummary, action: string, comment?: string) {
  try {
    await client.post(`/questions/${q.id}/review`, { action, comment })
    ElMessage.success(action === 'approved' ? '已通过' : '已驳回')
    list.value = list.value.filter(item => item.id !== q.id)
  } catch (e: any) {
    ElMessage.error(e.response?.data?.error || '操作失败')
  }
}

function typeLabel(t: string) {
  const map: Record<string, string> = { choice: '选择', fill: '填空', solution: '解答', judgment: '判断' }
  return map[t] || t
}
function diffLabel(d: string) {
  const map: Record<string, string> = { easy: '🟢 简单', medium: '🟡 中等', hard: '🔴 困难' }
  return map[d] || d
}
function formatTime(t: string) {
  return t ? t.replace('T', ' ').substring(0, 16) : ''
}

onMounted(fetchList)
</script>
