<template>
  <div>
    <div class="flex items-center justify-between mb-4">
      <h1 class="text-2xl font-bold">📝 题目管理</h1>
      <el-button type="primary" @click="$router.push('/questions/new')">
        ➕ 新建题目
      </el-button>
    </div>

    <el-card shadow="never">
      <!-- 搜索栏 -->
      <el-row :gutter="12" class="mb-4">
        <el-col :span="8">
          <el-input
            v-model="query.keyword"
            placeholder="🔍 搜索题干关键词..."
            clearable
            @input="onSearchInput"
          />
        </el-col>
        <el-col :span="12">
          <el-space wrap>
            <el-select
              v-model="query.question_type"
              placeholder="题型"
              clearable
              @change="fetchList"
              style="width:110px"
            >
              <el-option label="选择题" value="choice" />
              <el-option label="填空题" value="fill" />
              <el-option label="解答题" value="solution" />
              <el-option label="判断题" value="judgment" />
            </el-select>

            <el-select
              v-model="query.difficulty"
              placeholder="难度"
              clearable
              @change="fetchList"
              style="width:100px"
            >
              <el-option label="🟢 简单" value="easy" />
              <el-option label="🟡 中等" value="medium" />
              <el-option label="🔴 困难" value="hard" />
            </el-select>

            <el-select
              v-model="query.status"
              placeholder="状态"
              clearable
              @change="fetchList"
              style="width:120px"
            >
              <el-option label="📝 草稿" value="draft" />
              <el-option label="⏳ 待审核" value="pending" />
              <el-option label="❌ 驳回" value="rejected" />
              <el-option label="✅ 已发布" value="published" />
              <el-option label="🚫 已停用" value="disabled" />
            </el-select>

            <el-select
              v-model="query.grade"
              placeholder="年级"
              clearable
              @change="fetchList"
              style="width:100px"
            >
              <el-option v-for="g in grades" :key="g" :label="g" :value="g" />
            </el-select>
          </el-space>
        </el-col>
      </el-row>

      <!-- 表格 -->
      <el-table
        :data="list"
        v-loading="loading"
        stripe
        @row-click="goDetail"
        style="cursor:pointer"
      >
        <el-table-column label="题干" min-width="300">
          <template #default="{ row }">
            <div class="line-clamp-1 latex-inline-table">
              <LatexRender :text="row.stem" :inline="true" />
            </div>
          </template>
        </el-table-column>

        <el-table-column label="题型" width="90" align="center">
          <template #default="{ row }">
            <el-tag :type="typeTag(row.question_type)" size="small">
              {{ typeLabel(row.question_type) }}
            </el-tag>
          </template>
        </el-table-column>

        <el-table-column label="难度" width="80" align="center">
          <template #default="{ row }">
            <span>{{ diffLabel(row.difficulty) }}</span>
          </template>
        </el-table-column>

        <el-table-column label="状态" width="100" align="center">
          <template #default="{ row }">
            <el-tag :type="statusTag(row.status)" size="small">
              {{ statusLabel(row.status) }}
            </el-tag>
          </template>
        </el-table-column>

        <el-table-column label="年级" width="80" align="center">
          <template #default="{ row }">{{ row.grade || '—' }}</template>
        </el-table-column>

        <el-table-column label="版本" width="60" align="center">
          <template #default="{ row }">v{{ row.version }}</template>
        </el-table-column>

        <el-table-column label="更新时间" width="170">
          <template #default="{ row }">{{ formatTime(row.updated_at) }}</template>
        </el-table-column>
      </el-table>

      <!-- 空状态 -->
      <el-empty v-if="!loading && list.length === 0" description="没有找到匹配的题目" />

      <!-- 分页 -->
      <div class="flex justify-center mt-4" v-if="total > 0">
        <el-pagination
          v-model:current-page="page"
          :page-size="pageSize"
          :total="total"
          layout="prev, pager, next"
          @current-change="fetchList"
        />
      </div>
    </el-card>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { questionApi, type QuestionSummary, type QuestionQuery } from '@/api/client'
import LatexRender from '@/components/LatexRender.vue'

const router = useRouter()
const list = ref<QuestionSummary[]>([])
const loading = ref(false)
const total = ref(0)
const page = ref(1)
const pageSize = 20
const grades = ['初一', '初二', '初三', '高一', '高二', '高三']

const query = reactive<QuestionQuery>({
  keyword: '',
  question_type: undefined,
  difficulty: undefined,
  status: undefined,
  grade: undefined,
  page: 1,
  page_size: pageSize,
})

let searchTimer: ReturnType<typeof setTimeout> | null = null
function onSearchInput() {
  if (searchTimer) clearTimeout(searchTimer)
  searchTimer = setTimeout(() => {
    page.value = 1
    query.page = 1
    fetchList()
  }, 300)
}

async function fetchList() {
  loading.value = true
  try {
    query.page = page.value
    const res = await questionApi.list(query)
    list.value = res.data
    total.value = res.data.length >= pageSize ? pageSize * page.value + 1 : res.data.length
  } catch { /* handled by interceptor */ }
  finally { loading.value = false }
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
  return t ? t.replace('T', ' ').substring(0, 19) : ''
}

onMounted(fetchList)
</script>

<style scoped>
.line-clamp-1 {
  display: -webkit-box;
  -webkit-line-clamp: 1;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.latex-inline-table {
  font-size: 13px;
}
.latex-inline-table :deep(.katex) {
  font-size: 1em;
}
</style>
