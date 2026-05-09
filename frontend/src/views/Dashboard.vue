<template>
  <div>
    <h1 class="text-2xl font-bold mb-2">欢迎回来，{{ auth.displayName }} 👋</h1>
    <p class="text-gray-500 mb-6">{{ today }}</p>

    <!-- 统计卡片 -->
    <el-row :gutter="20" class="mb-6">
      <el-col :span="6" v-for="s in statCards" :key="s.label">
        <el-card shadow="hover" :body-style="{ padding: '20px' }">
          <div class="flex items-center justify-between">
            <div>
              <div class="text-3xl font-bold" :style="{ color: s.color }">
                {{ loading ? '…' : s.value }}
              </div>
              <div class="text-sm text-gray-400 mt-1">{{ s.label }}</div>
            </div>
            <div class="text-3xl" :style="{ color: s.color }">{{ s.icon }}</div>
          </div>
        </el-card>
      </el-col>
    </el-row>

    <el-row :gutter="20">
      <!-- 最近更新 -->
      <el-col :span="auth.isLeader ? 14 : 24">
        <el-card shadow="never">
          <template #header>
            <div class="flex items-center justify-between">
              <span class="font-bold">🕐 最近更新</span>
              <el-button text type="primary" @click="$router.push('/questions')">
                查看全部 →
              </el-button>
            </div>
          </template>

          <div v-if="loading" v-loading="loading" class="h-32" />

          <div v-else-if="recentList.length === 0" class="text-center text-gray-400 py-8">
            还没有题目，点击上方按钮创建第一道题
          </div>

          <el-table v-else :data="recentList" stripe @row-click="goDetail" style="cursor:pointer">
            <el-table-column label="题干" min-width="200">
              <template #default="{ row }">
                <span class="line-clamp-1">{{ row.stem }}</span>
              </template>
            </el-table-column>
            <el-table-column label="题型" width="70" align="center">
              <template #default="{ row }">
                <el-tag :type="typeTag(row.question_type)" size="small">{{ typeLabel(row.question_type) }}</el-tag>
              </template>
            </el-table-column>
            <el-table-column label="难度" width="70" align="center">
              <template #default="{ row }">{{ diffLabel(row.difficulty) }}</template>
            </el-table-column>
            <el-table-column label="状态" width="90" align="center">
              <template #default="{ row }">
                <el-tag :type="statusTag(row.status)" size="small">{{ statusLabel(row.status) }}</el-tag>
              </template>
            </el-table-column>
            <el-table-column label="更新时间" width="160">
              <template #default="{ row }">{{ formatTime(row.updated_at) }}</template>
            </el-table-column>
          </el-table>
        </el-card>
      </el-col>

      <!-- 待审核（仅组长可见） -->
      <el-col :span="10" v-if="auth.isLeader">
        <el-card shadow="never">
          <template #header>
            <div class="flex items-center justify-between">
              <span class="font-bold">⏳ 待审核</span>
              <el-button text type="primary" @click="$router.push('/review')">
                全部 →
              </el-button>
            </div>
          </template>

          <div v-if="loadingPending" v-loading="loadingPending" class="h-32" />

          <div v-else-if="pendingList.length === 0" class="text-center text-gray-400 py-8">
            暂无待审核题目
          </div>

          <div v-else>
            <div
              v-for="q in pendingList"
              :key="q.id"
              class="flex items-center justify-between py-3 border-b border-gray-100 last:border-0 cursor-pointer hover:bg-gray-50 px-2 rounded"
              @click="goDetail(q)"
            >
              <div class="flex-1 min-w-0 mr-2">
                <div class="text-sm truncate">{{ q.stem }}</div>
                <div class="text-xs text-gray-400 mt-1">
                  {{ typeLabel(q.question_type) }} · {{ diffLabel(q.difficulty) }}
                </div>
              </div>
              <div class="text-xs text-gray-400 whitespace-nowrap">
                {{ formatTime(q.updated_at).substring(5, 16) }}
              </div>
            </div>
          </div>
        </el-card>
      </el-col>
    </el-row>

    <!-- 快速操作 -->
    <el-card shadow="never" class="mt-6">
      <template #header><span class="font-bold">⚡ 快速操作</span></template>
      <div class="flex flex-wrap gap-3">
        <el-button type="primary" size="large" @click="$router.push('/questions/new')">
          ➕ 创建新题目
        </el-button>
        <el-button size="large" @click="$router.push('/questions')">
          📝 浏览题库
        </el-button>
        <el-button v-if="auth.isLeader" size="large" @click="$router.push('/review')">
          🔍 审核队列
        </el-button>
        <el-button size="large" @click="$router.push('/knowledge-points')">
          🏷️ 知识点管理
        </el-button>
      </div>
    </el-card>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import { questionApi, type QuestionSummary } from '@/api/client'

const router = useRouter()
const auth = useAuthStore()
const loading = ref(true)
const loadingPending = ref(true)
const recentList = ref<QuestionSummary[]>([])
const pendingList = ref<QuestionSummary[]>([])

// 统计
const statCards = ref([
  { label: '总题目', value: 0 as number | string, color: '#409eff', icon: '📊' },
  { label: '已发布', value: 0 as number | string, color: '#67c23a', icon: '✅' },
  { label: '待审核', value: 0 as number | string, color: '#e6a23c', icon: '⏳' },
  { label: '草稿', value: 0 as number | string, color: '#909399', icon: '📝' },
])

async function fetchStats() {
  try {
    const res = await questionApi.stats()
    const s = res.data
    statCards.value = [
      { label: '总题目', value: s.total, color: '#409eff', icon: '📊' },
      { label: '已发布', value: s.published, color: '#67c23a', icon: '✅' },
      { label: '待审核', value: s.pending, color: '#e6a23c', icon: '⏳' },
      { label: '草稿', value: s.draft, color: '#909399', icon: '📝' },
    ]
  } catch { /* handled */ }
}

const today = new Date().toLocaleDateString('zh-CN', {
  year: 'numeric', month: 'long', day: 'numeric', weekday: 'long',
})

async function fetchData() {
  loading.value = true
  try {
    const [allRes] = await Promise.all([
      questionApi.list({ page_size: 20 }),
      fetchStats(),
    ])
    recentList.value = allRes.data.slice(0, 10)
  } catch { /* handled */ }
  finally { loading.value = false }
}

async function fetchPending() {
  if (!auth.isLeader) return
  loadingPending.value = true
  try {
    const res = await questionApi.list({ status: 'pending', page_size: 10 })
    pendingList.value = res.data
  } catch { /* handled */ }
  finally { loadingPending.value = false }
}

function goDetail(row: QuestionSummary) {
  router.push(`/questions/${row.id}`)
}

function typeLabel(t: string) {
  const map: Record<string, string> = { choice: '选择', fill: '填空', solution: '解答', judgment: '判断' }
  return map[t] || t
}
function typeTag(t: string) {
  const map: Record<string, string> = { choice: '', fill: 'warning', solution: 'success', judgment: 'info' }
  return map[t] || ''
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
function formatTime(t: string) {
  return t ? t.replace('T', ' ').substring(0, 16) : ''
}

onMounted(() => {
  fetchData()
  fetchPending()
})
</script>

<style scoped>
.line-clamp-1 {
  display: -webkit-box;
  -webkit-line-clamp: 1;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
</style>
