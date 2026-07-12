<template>
  <div>
    <h1 class="page-title mb-12"><AppIcon name="users" :size="24" /> 用户管理</h1>

    <div v-if="loading" class="loading-hint">加载中…</div>

    <AppEmpty v-else-if="list.length === 0" description="暂无用户" />

    <template v-else>
      <div
        v-for="row in list"
        :key="row.id"
        class="q-item"
      >
        <div class="q-item-header">
          <div class="q-item-meta">
            <span class="user-display-name">{{ row.display_name }}</span>
            <AppBadge :color="roleBadgeColor(row.role)">{{ roleLabel(row.role) }}</AppBadge>
            <AppBadge :color="row.is_active ? 'green' : 'red'">
              {{ row.is_active ? '正常' : '禁用' }}
            </AppBadge>
          </div>
          <span class="text-sm text-muted">{{ formatTime(row.created_at) }}</span>
        </div>
        <div class="user-info">
          <span class="text-sm text-muted">@{{ row.username }}</span>
        </div>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import client from '@/api/client'
import { AppBadge, AppEmpty, AppIcon } from '@/components/ui'

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
  const map: Record<string, string> = { Admin: '管理员', User: '普通用户' }
  return map[r] || r
}
function roleBadgeColor(r: string): 'red' | 'blue' | 'gray' {
  const map: Record<string, 'red' | 'blue' | 'gray'> = { Admin: 'red', User: 'blue' }
  return map[r] || 'gray'
}
function formatTime(t: string) {
  return t ? t.replace('T', ' ').substring(0, 16) : ''
}

onMounted(fetchUsers)
</script>

<style scoped>
.loading-hint {
  text-align: center;
  padding: 48px 20px;
  color: var(--text-muted);
}

.user-display-name {
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary);
}

.user-info {
  font-size: 13px;
}
</style>
