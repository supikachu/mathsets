<template>
  <div>
    <h1 class="text-2xl font-bold mb-4">👥 用户管理</h1>

    <el-card shadow="never" v-loading="loading">
      <el-table :data="list" stripe>
        <el-table-column label="用户名" min-width="120">
          <template #default="{ row }">{{ row.username }}</template>
        </el-table-column>
        <el-table-column label="显示名" min-width="100">
          <template #default="{ row }">{{ row.display_name }}</template>
        </el-table-column>
        <el-table-column label="角色" width="110" align="center">
          <template #default="{ row }">
            <el-tag :type="roleTag(row.role)" size="small">{{ roleLabel(row.role) }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="状态" width="80" align="center">
          <template #default="{ row }">
            <el-tag :type="row.is_active ? 'success' : 'danger'" size="small">
              {{ row.is_active ? '正常' : '禁用' }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column label="注册时间" width="170">
          <template #default="{ row }">{{ formatTime(row.created_at) }}</template>
        </el-table-column>
      </el-table>
    </el-card>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import client from '@/api/client'

interface UserInfo {
  id: string
  username: string
  display_name: string
  role: string
  is_active: boolean
  created_at: string
}

const list = ref<UserInfo[]>([])
const loading = ref(false)

async function fetchUsers() {
  loading.value = true
  try {
    const res = await client.get('/admin/users')
    list.value = res.data
  } catch { /* handled */ }
  finally { loading.value = false }
}

function roleLabel(r: string) {
  const map: Record<string, string> = { admin: '管理员', groupleader: '组长', teacher: '教师', viewer: '访客' }
  return map[r] || r
}
function roleTag(r: string) {
  const map: Record<string, string> = { admin: 'danger', groupleader: 'warning', teacher: 'info', viewer: '' }
  return map[r] || ''
}
function formatTime(t: string) {
  return t ? t.replace('T', ' ').substring(0, 16) : ''
}

onMounted(fetchUsers)
</script>
