<template>
  <div class="um-page">
    <!-- ===== 顶部工具栏：搜索 + 新建 ===== -->
    <header class="um-header">
      <!-- Apple 风格搜索框 -->
      <div class="um-search-wrap">
        <AppIcon name="search" :size="15" class="um-search-icon" />
        <input
          v-model="searchQuery"
          class="um-search-input"
          placeholder="搜索用户名、姓名或邮箱…"
        />
        <button v-if="searchQuery" class="um-search-clear" @click="searchQuery = ''">
          <AppIcon name="x" :size="13" />
        </button>
      </div>

      <!-- 新建用户按钮 -->
      <AppButton variant="primary" size="sm" @click="openCreateDialog">
        <AppIcon name="plus" :size="15" />
        新建用户
      </AppButton>
    </header>

    <!-- ===== 用户列表 ===== -->
    <div v-if="loading" class="loading-hint">加载中…</div>

    <AppEmpty
      v-else-if="filteredList.length === 0"
      :description="searchQuery ? '没有匹配的用户' : '暂无用户'"
    />

    <div v-else class="um-list">
      <div
        v-for="row in filteredList"
        :key="row.id"
        class="um-row"
        :class="{ 'is-disabled': !row.is_active, 'is-self': row.id === auth.userId }"
      >
        <!-- 左侧：头像 + 名称 -->
        <div class="um-row-left">
          <div
            class="um-avatar"
            :class="row.global_role === 'super_admin' ? 'avatar-admin' : 'avatar-teacher'"
          >
            {{ initials(row.display_name || row.username) }}
          </div>
          <div class="um-row-info">
            <div class="um-row-name-line">
              <span class="um-display-name">{{ row.display_name }}</span>
              <span class="um-username">@{{ row.username }}</span>
              <span v-if="row.id === auth.userId" class="um-self-badge">你</span>
            </div>
            <div class="um-row-meta-line">
              <AppBadge :color="row.global_role === 'super_admin' ? 'red' : 'blue'">
                {{ globalRoleLabel(row.global_role) }}
              </AppBadge>
              <span class="um-email">{{ row.email }}</span>
              <span class="um-time">{{ formatTime(row.created_at) }} 创建</span>
            </div>
          </div>
        </div>

        <!-- 右侧：角色选择 + 状态开关 -->
        <div class="um-row-right">
          <!-- 角色修改 -->
          <div class="um-role-cell">
            <span class="um-cell-label">角色</span>
            <select
              class="um-role-select"
              :value="row.global_role"
              :disabled="actionLoading === row.id || row.id === auth.userId"
              @change="handleRoleChange(row, ($event.target as HTMLSelectElement).value)"
            >
              <option value="teacher">教师</option>
              <option value="super_admin">超级管理员</option>
            </select>
          </div>

          <!-- 状态开关（当前用户自身禁用，防止自我锁定） -->
          <div class="um-status-cell">
            <span class="um-cell-label">状态</span>
            <div
              class="um-toggle-wrapper"
              :class="{ 'is-locked': row.id === auth.userId }"
              :title="row.id === auth.userId ? '不能修改自己的状态' : ''"
            >
              <AppToggle
                :model-value="row.is_active"
                @update:model-value="() => handleStatusToggle(row)"
              />
            </div>
          </div>

          <!-- 操作列：查看 + 删除 -->
          <div class="um-actions-cell">
            <AppButton
              variant="ghost"
              size="sm"
              class="um-icon-btn"
              title="查看用户详情"
              :loading="detailLoading === row.id"
              @click="openDetail(row)"
            >
              <AppIcon name="eye" :size="15" />
            </AppButton>
            <AppButton
              variant="danger"
              size="sm"
              class="um-icon-btn"
              title="删除用户"
              :disabled="row.id === auth.userId"
              @click="openDeleteConfirm(row)"
            >
              <AppIcon name="trash" :size="15" />
            </AppButton>
          </div>
        </div>
      </div>
    </div>

    <!-- ===== 新建用户弹窗 ===== -->
    <AppModal v-model="createDialog" title="新建用户" size="sm">
      <form class="um-form" @submit.prevent="handleCreate">
        <div class="um-form-row">
          <AppInput
            v-model="createForm.display_name"
            label="姓名"
            placeholder="请输入用户姓名"
            autocomplete="off"
          />
        </div>
        <div class="um-form-row">
          <AppInput
            v-model="createForm.username"
            label="账号"
            placeholder="登录用户名"
            autocomplete="off"
          />
        </div>
        <div class="um-form-row">
          <AppInput
            v-model="createForm.email"
            label="邮箱"
            type="email"
            placeholder="user@example.com"
            autocomplete="off"
          />
        </div>
        <div class="um-form-row">
          <AppInput
            v-model="createForm.password"
            label="初始密码"
            type="password"
            placeholder="至少 6 位"
            autocomplete="new-password"
          />
        </div>
        <div class="um-form-row">
          <label class="um-form-label">初始角色</label>
          <div class="um-role-radio-group">
            <label class="um-role-radio" :class="{ active: createForm.global_role === 'teacher' }">
              <input
                v-model="createForm.global_role"
                type="radio"
                value="teacher"
                class="um-radio-input"
              />
              <AppIcon name="user" :size="14" />
              <span>教师</span>
            </label>
            <label
              class="um-role-radio"
              :class="{ active: createForm.global_role === 'super_admin' }"
            >
              <input
                v-model="createForm.global_role"
                type="radio"
                value="super_admin"
                class="um-radio-input"
              />
              <AppIcon name="shield" :size="14" />
              <span>超级管理员</span>
            </label>
          </div>
        </div>

        <div class="um-form-actions">
          <AppButton variant="ghost" type="button" @click="createDialog = false">取消</AppButton>
          <AppButton variant="primary" type="submit" :loading="creating">创建用户</AppButton>
        </div>
      </form>
    </AppModal>

    <!-- ===== 用户详情弹窗 ===== -->
    <AppModal v-model="detailDialog" title="用户详情" size="md">
      <div v-if="detailLoading" class="um-detail-loading">加载中…</div>
      <div v-else-if="detailUser" class="um-detail">
        <!-- 顶部：头像 + 主信息 -->
        <div class="um-detail-header">
          <div
            class="um-detail-avatar"
            :class="detailUser.global_role === 'super_admin' ? 'avatar-admin' : 'avatar-teacher'"
          >
            {{ initials(detailUser.display_name || detailUser.username) }}
          </div>
          <div class="um-detail-id">
            <div class="um-detail-name">{{ detailUser.display_name }}</div>
            <div class="um-detail-username">@{{ detailUser.username }}</div>
            <div class="um-detail-badges">
              <AppBadge :color="detailUser.global_role === 'super_admin' ? 'red' : 'blue'">
                {{ globalRoleLabel(detailUser.global_role) }}
              </AppBadge>
              <AppBadge :color="detailUser.is_active ? 'green' : 'gray'">
                {{ detailUser.is_active ? '已启用' : '已禁用' }}
              </AppBadge>
              <span v-if="detailUser.id === auth.userId" class="um-self-badge">你</span>
            </div>
          </div>
        </div>

        <!-- 描述列表 -->
        <dl class="um-detail-list">
          <div class="um-detail-item">
            <dt><AppIcon name="mail" :size="13" />邮箱</dt>
            <dd>{{ detailUser.email }}</dd>
          </div>
          <div class="um-detail-item">
            <dt><AppIcon name="shield-check" :size="13" />全局角色</dt>
            <dd>{{ globalRoleLabel(detailUser.global_role) }}</dd>
          </div>
          <div class="um-detail-item">
            <dt><AppIcon name="circle-dot" :size="13" />账号状态</dt>
            <dd :class="detailUser.is_active ? 'um-status-active' : 'um-status-inactive'">
              {{ detailUser.is_active ? '已启用' : '已禁用' }}
            </dd>
          </div>
          <div class="um-detail-item">
            <dt><AppIcon name="clock" :size="13" />注册时间</dt>
            <dd>{{ formatTime(detailUser.created_at) }}</dd>
          </div>
          <div class="um-detail-item">
            <dt><AppIcon name="key" :size="13" />用户 ID</dt>
            <dd class="um-detail-uuid">{{ detailUser.id }}</dd>
          </div>
        </dl>

        <div class="um-detail-footer">
          <AppButton variant="ghost" @click="detailDialog = false">关闭</AppButton>
        </div>
      </div>
    </AppModal>

    <!-- ===== 删除确认弹窗 ===== -->
    <AppModal v-model="deleteDialog" title="确认删除用户" size="sm">
      <div class="um-delete-confirm">
        <div class="um-delete-icon">
          <AppIcon name="alert" :size="22" />
        </div>
        <p class="um-delete-text">
          确定要删除用户
          <strong>{{ deleteTarget?.display_name }}</strong>
          （@{{ deleteTarget?.username }}）吗？
        </p>
        <p class="um-delete-hint">
          该操作不可撤销。用户创建的题目与审核记录将被保留，但创建者字段会被置空。
        </p>
        <div class="um-delete-actions">
          <AppButton variant="ghost" @click="deleteDialog = false">取消</AppButton>
          <AppButton variant="danger" :loading="deleting" @click="handleDelete">
            <AppIcon name="trash" :size="14" />
            确认删除
          </AppButton>
        </div>
      </div>
    </AppModal>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, computed, onMounted } from 'vue'
import { adminUserApi, type AdminUser, type CreateUserRequest } from '@/api/client'
import { AppButton, AppModal, AppInput, AppToggle, AppBadge, AppEmpty, AppIcon } from '@/components/ui'
import { useToast } from '@/composables/useToast'
import { useAuthStore } from '@/stores/auth'

const toast = useToast()
const auth = useAuthStore()

const list = ref<AdminUser[]>([])
const loading = ref(false)
const actionLoading = ref<string | null>(null)

// ── 搜索 ──
const searchQuery = ref('')
const filteredList = computed(() => {
  const q = searchQuery.value.trim().toLowerCase()
  if (!q) return list.value
  return list.value.filter(
    (u) =>
      u.username.toLowerCase().includes(q) ||
      u.display_name.toLowerCase().includes(q) ||
      u.email.toLowerCase().includes(q),
  )
})

// ── 新建用户弹窗状态 ──
const createDialog = ref(false)
const creating = ref(false)
const createForm = reactive<CreateUserRequest>({
  username: '',
  email: '',
  password: '',
  display_name: '',
  global_role: 'teacher',
})

// ── 用户详情弹窗状态 ──
const detailDialog = ref(false)
const detailUser = ref<AdminUser | null>(null)
const detailLoading = ref<string | null>(null)

// ── 删除用户确认弹窗状态 ──
const deleteDialog = ref(false)
const deleteTarget = ref<AdminUser | null>(null)
const deleting = ref(false)

// ── 数据加载 ──
async function fetchUsers() {
  loading.value = true
  try {
    const res = await adminUserApi.list()
    list.value = res.data
  } catch (e: any) {
    toast.error(e.response?.data?.error || '加载用户列表失败')
  } finally {
    loading.value = false
  }
}

// ── 新建用户 ──
function openCreateDialog() {
  createForm.username = ''
  createForm.email = ''
  createForm.password = ''
  createForm.display_name = ''
  createForm.global_role = 'teacher'
  createDialog.value = true
}

async function handleCreate() {
  if (
    !createForm.username.trim() ||
    !createForm.email.trim() ||
    !createForm.password.trim() ||
    !createForm.display_name.trim()
  ) {
    toast.warning('请填写所有必填项')
    return
  }
  if (createForm.password.length < 6) {
    toast.warning('密码至少 6 位')
    return
  }

  creating.value = true
  try {
    const res = await adminUserApi.create({ ...createForm })
    list.value.unshift(res.data)
    createDialog.value = false
    toast.success(`用户 ${res.data.display_name} 创建成功`)
  } catch (e: any) {
    toast.error(e.response?.data?.error || '创建用户失败')
  } finally {
    creating.value = false
  }
}

// ── 修改角色 ──
async function handleRoleChange(row: AdminUser, newRole: string) {
  if (newRole === row.global_role) return
  // 防止修改自己的角色（避免误降权）
  if (row.id === auth.userId) {
    toast.warning('不能修改自己的角色')
    return
  }
  actionLoading.value = row.id
  try {
    const res = await adminUserApi.updateRole(row.id, newRole)
    Object.assign(row, res.data)
    toast.success(`${row.display_name} 的角色已更新为${globalRoleLabel(res.data.global_role)}`)
  } catch (e: any) {
    toast.error(e.response?.data?.error || '修改角色失败')
  } finally {
    actionLoading.value = null
  }
}

// ── 切换启用/禁用状态 ──
async function handleStatusToggle(row: AdminUser) {
  // 前端双重保险：禁止修改自己的状态
  if (row.id === auth.userId) {
    toast.warning('不能禁用自己的账号')
    return
  }
  actionLoading.value = row.id
  const newStatus = !row.is_active
  try {
    const res = await adminUserApi.updateStatus(row.id, newStatus)
    Object.assign(row, res.data)
    toast.success(`${row.display_name} 已${newStatus ? '启用' : '禁用'}`)
  } catch (e: any) {
    toast.error(e.response?.data?.error || '修改状态失败')
  } finally {
    actionLoading.value = null
  }
}

// ── 查看用户详情 ──
async function openDetail(row: AdminUser) {
  detailLoading.value = row.id
  detailUser.value = null
  detailDialog.value = true
  try {
    const res = await adminUserApi.getUser(row.id)
    detailUser.value = res.data
  } catch (e: any) {
    toast.error(e.response?.data?.error || '加载用户详情失败')
    detailDialog.value = false
  } finally {
    detailLoading.value = null
  }
}

// ── 删除用户：打开确认弹窗 ──
function openDeleteConfirm(row: AdminUser) {
  // 前端双重保险：禁止删除自己
  if (row.id === auth.userId) {
    toast.warning('不能删除自己的账号')
    return
  }
  deleteTarget.value = row
  deleteDialog.value = true
}

// ── 删除用户：执行删除 ──
async function handleDelete() {
  if (!deleteTarget.value) return
  const target = deleteTarget.value
  deleting.value = true
  try {
    await adminUserApi.deleteUser(target.id)
    // 从列表中移除
    list.value = list.value.filter((u) => u.id !== target.id)
    toast.success(`用户 ${target.display_name} 已删除`)
    deleteDialog.value = false
    deleteTarget.value = null
  } catch (e: any) {
    toast.error(e.response?.data?.error || '删除用户失败')
  } finally {
    deleting.value = false
  }
}

// ── 工具函数 ──
function globalRoleLabel(gr: string): string {
  return gr === 'super_admin' ? '超级管理员' : '教师'
}

function initials(name: string): string {
  return name.charAt(0).toUpperCase()
}

function formatTime(t: string): string {
  return t ? t.replace('T', ' ').substring(0, 10) : ''
}

onMounted(fetchUsers)
</script>

<style scoped>
.um-page {
  max-width: 960px;
  margin: 0 auto;
}

/* ── 顶部工具栏 ── */
.um-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 24px;
}

/* ── Apple 风格搜索框 ── */
.um-search-wrap {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 14px;
  background: var(--bg-input);
  border-radius: 10px;
  border: 1px solid transparent;
  transition: border-color 0.2s, box-shadow 0.2s, background 0.2s;
  min-width: 320px;
  flex: 1;
  max-width: 420px;
}

.um-search-wrap:focus-within {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
  background: var(--bg-card);
}

.um-search-icon {
  color: var(--text-muted);
  flex-shrink: 0;
}

.um-search-input {
  flex: 1;
  border: none;
  background: transparent;
  outline: none;
  font-size: 13px;
  color: var(--text-primary);
  font-family: inherit;
}

.um-search-input::placeholder {
  color: var(--text-muted);
}

.um-search-clear {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border: none;
  border-radius: 50%;
  background: var(--bg-hover);
  color: var(--text-muted);
  cursor: pointer;
  flex-shrink: 0;
  transition: background 0.15s, color 0.15s;
}

.um-search-clear:hover {
  background: var(--border-color);
  color: var(--text-primary);
}

/* ── 用户列表 ── */
.um-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.um-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 18px;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: 12px;
  transition: border-color 0.2s, box-shadow 0.2s;
}

.um-row:hover {
  border-color: var(--text-muted);
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.04);
}

.um-row.is-disabled {
  opacity: 0.55;
}

.um-row.is-self {
  border-color: var(--accent);
  background: var(--accent-light);
}

/* ── 左侧：头像 + 信息 ── */
.um-row-left {
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
  flex: 1;
}

.um-avatar {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 38px;
  height: 38px;
  border-radius: 50%;
  font-size: 15px;
  font-weight: 600;
  color: #fff;
  flex-shrink: 0;
}

.avatar-admin {
  background: linear-gradient(135deg, #af52de 0%, #7c3aed 100%);
}

.avatar-teacher {
  background: linear-gradient(135deg, #0071e3 0%, #0051b5 100%);
}

.um-row-info {
  min-width: 0;
}

.um-row-name-line {
  display: flex;
  align-items: baseline;
  gap: 8px;
}

.um-display-name {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
}

.um-username {
  font-size: 12px;
  color: var(--text-muted);
}

.um-self-badge {
  display: inline-flex;
  align-items: center;
  padding: 1px 6px;
  font-size: 10px;
  font-weight: 600;
  color: var(--accent);
  background: var(--bg-card);
  border-radius: 4px;
  border: 1px solid var(--accent);
}

.um-row-meta-line {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 4px;
  flex-wrap: wrap;
}

.um-email {
  font-size: 11px;
  color: var(--text-muted);
}

.um-time {
  font-size: 11px;
  color: var(--text-muted);
}

/* ── 右侧：角色 + 状态 ── */
.um-row-right {
  display: flex;
  align-items: center;
  gap: 20px;
  flex-shrink: 0;
}

.um-role-cell,
.um-status-cell {
  display: flex;
  align-items: center;
  gap: 8px;
}

.um-cell-label {
  font-size: 12px;
  color: var(--text-muted);
}

/* ── 角色 select（原生，匹配系统浅灰风格） ── */
.um-role-select {
  padding: 5px 10px;
  border: 1px solid var(--border-color);
  border-radius: 8px;
  background: var(--bg-input);
  color: var(--text-primary);
  font-size: 13px;
  font-family: inherit;
  cursor: pointer;
  transition: border-color 0.2s, box-shadow 0.2s;
  outline: none;
}

.um-role-select:hover:not(:disabled) {
  border-color: var(--text-muted);
}

.um-role-select:focus {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
}

.um-role-select:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* ── Toggle 自锁包装：禁用当前用户的状态开关 ── */
.um-toggle-wrapper {
  display: inline-flex;
}

.um-toggle-wrapper.is-locked {
  opacity: 0.4;
  pointer-events: none;
  cursor: not-allowed;
}

/* ── 新建用户表单 ── */
.um-form {
  display: flex;
  flex-direction: column;
  gap: 16px;
  padding: 4px 0;
}

.um-form-row {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.um-form-label {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-primary);
}

.um-role-radio-group {
  display: flex;
  gap: 10px;
}

.um-role-radio {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 14px;
  border: 1px solid var(--border-color);
  border-radius: 8px;
  background: var(--bg-input);
  color: var(--text-secondary);
  font-size: 13px;
  cursor: pointer;
  transition: all 0.2s;
  flex: 1;
  justify-content: center;
}

.um-role-radio:hover {
  border-color: var(--text-muted);
}

.um-role-radio.active {
  border-color: var(--accent);
  background: var(--accent-light);
  color: var(--accent);
  font-weight: 600;
}

.um-radio-input {
  display: none;
}

.um-form-actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  margin-top: 8px;
  padding-top: 16px;
  border-top: 1px solid var(--divider);
}

/* ── 加载/空状态 ── */
.loading-hint {
  text-align: center;
  padding: 48px 20px;
  color: var(--text-muted);
  font-size: 14px;
}

/* ── 操作列：查看 + 删除图标按钮 ── */
.um-actions-cell {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-left: 4px;
  padding-left: 16px;
  border-left: 1px solid var(--divider);
}

.um-icon-btn {
  padding: 6px;
  line-height: 1;
  border-radius: 8px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}

.um-icon-btn:hover:not(:disabled) {
  transform: none;
  box-shadow: none;
}

.um-icon-btn:disabled {
  opacity: 0.35;
  cursor: not-allowed;
}

/* ── 用户详情弹窗 ── */
.um-detail-loading {
  text-align: center;
  padding: 40px 20px;
  color: var(--text-muted);
  font-size: 14px;
}

.um-detail {
  padding: 4px 0;
}

.um-detail-header {
  display: flex;
  align-items: center;
  gap: 16px;
  padding-bottom: 20px;
  margin-bottom: 20px;
  border-bottom: 1px solid var(--divider);
}

.um-detail-avatar {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 56px;
  height: 56px;
  border-radius: 50%;
  font-size: 22px;
  font-weight: 600;
  color: #fff;
  flex-shrink: 0;
}

.um-detail-id {
  min-width: 0;
}

.um-detail-name {
  font-size: 18px;
  font-weight: 600;
  color: var(--text-primary);
  line-height: 1.3;
}

.um-detail-username {
  font-size: 13px;
  color: var(--text-muted);
  margin-top: 2px;
}

.um-detail-badges {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 8px;
  flex-wrap: wrap;
}

.um-detail-list {
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.um-detail-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 0;
  border-bottom: 1px solid var(--divider);
}

.um-detail-item:last-child {
  border-bottom: none;
}

.um-detail-item dt {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  color: var(--text-muted);
  font-weight: 400;
}

.um-detail-item dd {
  margin: 0;
  font-size: 13px;
  color: var(--text-primary);
  font-weight: 500;
  text-align: right;
}

.um-status-active {
  color: var(--success);
}

.um-status-inactive {
  color: var(--danger);
}

.um-detail-uuid {
  font-family: 'SF Mono', 'Menlo', monospace;
  font-size: 11px;
  color: var(--text-muted);
  font-weight: 400;
}

.um-detail-footer {
  display: flex;
  justify-content: flex-end;
  margin-top: 20px;
  padding-top: 16px;
  border-top: 1px solid var(--divider);
}

/* ── 删除确认弹窗 ── */
.um-delete-confirm {
  display: flex;
  flex-direction: column;
  align-items: center;
  text-align: center;
  padding: 8px 0 4px;
}

.um-delete-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 48px;
  height: 48px;
  border-radius: 50%;
  background: var(--danger-light);
  color: var(--danger);
  margin-bottom: 16px;
}

.um-delete-text {
  font-size: 15px;
  color: var(--text-primary);
  line-height: 1.5;
  margin: 0 0 8px;
}

.um-delete-hint {
  font-size: 12px;
  color: var(--text-muted);
  line-height: 1.5;
  margin: 0 0 20px;
  max-width: 320px;
}

.um-delete-actions {
  display: flex;
  justify-content: center;
  gap: 10px;
  width: 100%;
}
</style>
