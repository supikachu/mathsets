<template>
  <div>
    <div class="flex items-center justify-between mb-4">
      <h1 class="text-2xl font-bold">📄 试卷管理</h1>
      <el-button type="primary" :loading="creating" @click="handleCreate">➕ 新建试卷</el-button>
    </div>

    <el-card shadow="never" v-loading="loading">
      <el-empty v-if="!loading && list.length === 0" description="还没有试卷，创建第一份试卷吧" />
      <el-table v-else :data="list" stripe @row-click="goDetail" style="cursor:pointer">
        <el-table-column label="标题" min-width="200">
          <template #default="{ row }">
            <div class="font-medium">{{ row.title }}</div>
            <div v-if="row.description" class="text-xs text-gray-400 truncate">{{ row.description }}</div>
          </template>
        </el-table-column>
        <el-table-column label="科目" width="70" align="center">
          <template #default="{ row }">{{ row.subject }}</template>
        </el-table-column>
        <el-table-column label="年级" width="70" align="center">
          <template #default="{ row }">{{ row.grade || '—' }}</template>
        </el-table-column>
        <el-table-column label="题数" width="60" align="center">
          <template #default="{ row }">{{ row.question_count }}</template>
        </el-table-column>
        <el-table-column label="总分" width="70" align="center">
          <template #default="{ row }">{{ row.total_score }}</template>
        </el-table-column>
        <el-table-column label="状态" width="90" align="center">
          <template #default="{ row }">
            <el-tag :type="statusTag(row.status)" size="small">{{ statusLabel(row.status) }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="创建者" width="100">
          <template #default="{ row }">{{ row.creator_name || '—' }}</template>
        </el-table-column>
        <el-table-column label="更新时间" width="160">
          <template #default="{ row }">{{ formatTime(row.updated_at) }}</template>
        </el-table-column>
      </el-table>
    </el-card>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { ElMessage } from 'element-plus'
import { paperApi, type PaperSummary } from '@/api/client'

const router = useRouter()
const list = ref<PaperSummary[]>([])
const loading = ref(false)
const creating = ref(false)

async function fetchList() {
  loading.value = true
  try {
    const res = await paperApi.list()
    list.value = res.data
  } catch { /* handled */ }
  finally { loading.value = false }
}

async function handleCreate() {
  creating.value = true
  try {
    const res = await paperApi.create({ title: '新建试卷', grade: '初三' })
    ElMessage.success('试卷已创建')
    router.push(`/papers/${res.data.id}`)
  } catch { /* handled */ }
  finally { creating.value = false }
}

function goDetail(row: PaperSummary) {
  router.push(`/papers/${row.id}`)
}

function statusLabel(s: string) {
  const map: Record<string, string> = { draft: '📝 草稿', published: '✅ 已发布', archived: '📦 归档' }
  return map[s] || s
}
function statusTag(s: string) {
  const map: Record<string, string> = { draft: 'info', published: 'success', archived: '' }
  return map[s] || ''
}
function formatTime(t: string) {
  return t ? t.replace('T', ' ').substring(0, 16) : ''
}

onMounted(fetchList)
</script>
