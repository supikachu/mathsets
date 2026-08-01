<template>
  <div class="ss-page">
    <!-- 返回按钮 -->
    <button class="ss-back" @click="$router.back()">
      <AppIcon name="arrow-left" :size="16" />
      返回
    </button>

    <!-- 空间信息头部 -->
    <div class="ss-header" v-if="spaceDetail">
      <div class="ss-header-icon" :class="`ss-header-icon--${spaceDetail.kind}`">
        <AppIcon :name="kindIcon(spaceDetail.kind)" :size="22" />
      </div>
      <div class="ss-header-info">
        <h1 class="ss-header-name">{{ spaceDetail.name }}</h1>
        <span class="ss-header-kind">{{ kindLabel(spaceDetail.kind) }}空间</span>
      </div>
    </div>

    <div v-if="loading" class="ss-loading">加载中…</div>

    <template v-else-if="spaceDetail">
      <!-- ========== 公共空间：仅展示提示，隐藏成员管理 ========== -->
      <div v-if="spaceDetail.kind === 'public'" class="ss-public-notice">
        <div class="ss-public-icon">
          <AppIcon name="info" :size="22" />
        </div>
        <p class="ss-public-text">
          公共题库为系统级资产，成员管理仅限超级管理员通过系统配置操作。
        </p>
      </div>

      <!-- ========== 团队空间：成员管理面板 ========== -->
      <template v-else>
        <div class="ss-section">
          <div class="ss-section-header">
            <h2 class="ss-section-title">成员管理</h2>
            <button v-if="isOwner" class="ss-add-btn" @click="openAddModal">
              <AppIcon name="plus" :size="14" />
              添加成员
            </button>
          </div>

          <!-- 成员列表 -->
          <div class="ss-member-list" v-if="members.length > 0">
            <div
              v-for="m in members"
              :key="m.user_id"
              class="ss-member-row"
            >
              <!-- 头像 -->
              <div class="ss-member-avatar">
                {{ (m.display_name || m.username || '?').charAt(0).toUpperCase() }}
              </div>

              <!-- 姓名 + 用户名 -->
              <div class="ss-member-info">
                <span class="ss-member-name">{{ m.display_name }}</span>
                <span class="ss-member-username">@{{ m.username }}</span>
              </div>

              <!-- 角色选择 -->
              <div class="ss-member-role">
                <!-- Owner 显示纯文本标签（不可通过下拉框修改，防多 Owner） -->
                <span v-if="m.role === 'owner'" class="ss-role-tag ss-role-tag--owner">拥有者</span>
                <!-- 非 Owner 显示下拉框（仅 Member/Viewer 选项） -->
                <AppSelect
                  v-else
                  :model-value="m.role"
                  :options="roleOptions"
                  :disabled="!isOwner || m.user_id === currentUserId"
                  @update:model-value="(v) => handleRoleChange(m, v!)"
                />
              </div>

              <!-- 设为拥有者按钮（仅 Owner 可见，不能对自己操作） -->
              <button
                v-if="isOwner && m.user_id !== currentUserId"
                class="ss-member-transfer"
                @click="handleTransfer(m)"
              >
                <AppIcon name="crown" :size="14" />
                设为拥有者
              </button>

              <!-- 移除按钮 -->
              <button
                v-if="isOwner"
                class="ss-member-remove"
                :disabled="m.user_id === currentUserId"
                @click="openRemoveConfirm(m)"
              >
                <AppIcon name="trash" :size="14" />
              </button>
            </div>
          </div>

          <!-- 空状态 -->
          <div v-else class="ss-member-empty">
            <AppIcon name="users" :size="32" :stroke="1.5" />
            <p>暂无成员</p>
          </div>
        </div>

        <!-- ========== 危险操作区：退出 / 解散 ========== -->
        <div class="ss-danger-zone">
          <!-- Owner：解散团队空间 -->
          <button
            v-if="isOwner"
            class="ss-danger-btn ss-danger-btn--disband"
            @click="confirmDisband"
          >
            <AppIcon name="trash" :size="14" />
            解散团队空间
          </button>
          <!-- Member/Viewer：退出该空间 -->
          <button
            v-else
            class="ss-danger-btn ss-danger-btn--leave"
            @click="confirmLeave"
          >
            <AppIcon name="logout" :size="14" />
            退出该空间
          </button>
        </div>
      </template>
    </template>

    <!-- ========== 添加成员弹窗 ========== -->
    <AppModal :model-value="showAddModal" @update:model-value="showAddModal = $event" title="添加成员" size="md">
      <div class="ss-add-form">
        <!-- 用户名输入 -->
        <div class="ss-field">
          <label class="ss-field-label">用户名</label>
          <input
            v-model="addUsername"
            class="ss-search-input"
            placeholder="输入用户名（如 zhang_san）"
            @keyup.enter="confirmAddMember"
          />
          <span class="ss-field-hint">输入登录用户名，后端将自动解析为用户 ID</span>
        </div>

        <!-- 角色选择（始终显示） -->
        <div class="ss-field">
          <label class="ss-field-label">分配角色</label>
          <AppSelect
            v-model="selectedRole"
            :options="roleOptions"
            placeholder="选择角色"
          />
        </div>

        <!-- 操作按钮 -->
        <div class="ss-modal-actions">
          <button class="ss-btn ss-btn--ghost" @click="showAddModal = false">取消</button>
          <button
            class="ss-btn ss-btn--primary"
            :disabled="!addUsername.trim() || !selectedRole || addLoading"
            @click="confirmAddMember"
          >
            {{ addLoading ? '添加中…' : '确认添加' }}
          </button>
        </div>
      </div>
    </AppModal>

    <!-- ========== 移除确认弹窗 ========== -->
    <AppConfirm
      v-model="showRemoveConfirm"
      title="确认移除成员"
      :message="`确定要将 ${removeTarget?.display_name} 从该空间移除吗？移除后该用户将无法访问此空间。`"
      confirm-text="确认移除"
      danger
      :loading="removeLoading"
      @confirm="confirmRemove"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { spaceApi, type SpaceDetail, type SpaceMemberInfo } from '@/api/client'
import { AppIcon, AppModal, AppSelect, AppConfirm } from '@/components/ui'
import { useToast } from '@/composables/useToast'
import { useAuthStore } from '@/stores/auth'
import { useSpaceStore } from '@/stores/space'

const route = useRoute()
const router = useRouter()
const toast = useToast()
const auth = useAuthStore()
const spaceStore = useSpaceStore()

const spaceId = computed(() => route.params.id as string)
const currentUserId = computed(() => auth.userId)

// 仅当前空间的 Owner 才有人员管理权限（与后端 add/update/remove_member 对齐）
const isOwner = computed(() =>
  members.value.some(m => m.user_id === currentUserId.value && m.role === 'owner'),
)

const loading = ref(true)
const spaceDetail = ref<SpaceDetail | null>(null)
const members = ref<SpaceMemberInfo[]>([])

// 角色选项：移除 Owner —— 防止通过下拉菜单设置多个 Owner
// Owner 只能通过【设为拥有者】转让产生
const roleOptions = [
  { label: 'Member', value: 'member' },
  { label: 'Viewer', value: 'viewer' },
]

function kindIcon(kind: string): string {
  if (kind === 'personal') return 'user'
  if (kind === 'team') return 'users'
  if (kind === 'public') return 'globe'
  return 'folder'
}

function kindLabel(kind: string): string {
  if (kind === 'personal') return '个人'
  if (kind === 'team') return '团队'
  if (kind === 'public') return '公共'
  return kind
}

// ── 加载空间详情 ──
async function loadSpace() {
  loading.value = true
  try {
    const res = await spaceApi.get(spaceId.value)
    spaceDetail.value = res.data
    members.value = res.data.members || []
  } catch (e: any) {
    toast.error(e.response?.data?.error || '加载空间信息失败')
  } finally {
    loading.value = false
  }
}

// ── 修改角色 ──
async function handleRoleChange(m: SpaceMemberInfo, newRole: string) {
  try {
    await spaceApi.updateMember(spaceId.value, m.user_id, { role: newRole })
    m.role = newRole
    toast.success('角色已更新')
  } catch (e: any) {
    toast.error(e.response?.data?.error || '修改角色失败')
  }
}

// ── 添加成员 ──
const showAddModal = ref(false)
const addUsername = ref('')
const selectedRole = ref('')
const addLoading = ref(false)

function openAddModal() {
  showAddModal.value = true
  addUsername.value = ''
  selectedRole.value = 'member'
}

async function confirmAddMember() {
  const username = addUsername.value.trim()
  if (!username || !selectedRole.value) return
  addLoading.value = true
  try {
    await spaceApi.addMember(spaceId.value, username, selectedRole.value)
    toast.success('成员添加成功')
    showAddModal.value = false
    await loadSpace()
  } catch (e: any) {
    toast.error(e.response?.data?.error || e.response?.data?.message || e.message || '添加成员失败')
  } finally {
    addLoading.value = false
  }
}

// ── 移除成员 ──
const showRemoveConfirm = ref(false)
const removeTarget = ref<SpaceMemberInfo | null>(null)
const removeLoading = ref(false)

function openRemoveConfirm(m: SpaceMemberInfo) {
  removeTarget.value = m
  showRemoveConfirm.value = true
}

async function confirmRemove() {
  if (!removeTarget.value) return
  removeLoading.value = true
  try {
    await spaceApi.removeMember(spaceId.value, removeTarget.value.user_id)
    toast.success('成员已移除')
    showRemoveConfirm.value = false
    members.value = members.value.filter(m => m.user_id !== removeTarget.value!.user_id)
  } catch (e: any) {
    toast.error(e.response?.data?.error || e.response?.data?.message || e.message || '移除成员失败')
  } finally {
    removeLoading.value = false
  }
}

// ── 转让所有权 ──
async function handleTransfer(m: SpaceMemberInfo) {
  if (!confirm(`确定要将空间所有权转让给 ${m.display_name || m.username} 吗？\n转让后您将降级为 Member，无法再管理成员。`)) return
  try {
    await spaceApi.transferOwnership(spaceId.value, m.user_id)
    toast.success('权限转让成功')
    await loadSpace()
  } catch (e: any) {
    toast.error(e.response?.data?.error || e.response?.data?.message || e.message || '转让失败')
  }
}

// ── 退出空间（Member/Viewer） ──
async function confirmLeave() {
  if (!confirm('确定要退出该空间吗？\n退出后将无法访问此空间的题目。')) return
  try {
    await spaceApi.leaveSpace(spaceId.value)
    toast.success('已退出空间')
    // 强制刷新空间列表（废弃空间从侧边栏消失），再跳转回首页
    await spaceStore.fetchSpaces()
    router.push('/')
  } catch (e: any) {
    toast.error(e.response?.data?.error || e.response?.data?.message || e.message || '退出失败')
  }
}

// ── 解散空间（Owner） ──
async function confirmDisband() {
  if (!confirm('确定要解散该团队空间吗？\n此操作不可恢复，空间内所有题目将自动转移到您的个人空间。')) return
  try {
    await spaceApi.delete(spaceId.value)
    toast.success('空间已解散，题目已转移至个人空间')
    // 强制刷新空间列表（废弃空间从侧边栏消失），再跳转回首页
    await spaceStore.fetchSpaces()
    router.push('/')
  } catch (e: any) {
    toast.error(e.response?.data?.error || e.response?.data?.message || e.message || '解散失败')
  }
}

onMounted(loadSpace)
</script>

<style scoped>
.ss-page {
  max-width: 720px;
  margin: 0 auto;
  padding: 24px 20px 40px;
}

/* ===== Back button ===== */
.ss-back {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  color: var(--text-muted);
  background: transparent;
  padding: 4px 0;
  margin-bottom: 16px;
  transition: var(--transition-fast);
}

.ss-back:hover {
  color: var(--text-primary);
}

/* ===== Header ===== */
.ss-header {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 20px 24px;
  background: var(--bg-card);
  border-radius: var(--radius-md);
  margin-bottom: 24px;
}

.ss-header-icon {
  width: 48px;
  height: 48px;
  border-radius: 12px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: #fff;
  flex-shrink: 0;
}

.ss-header-icon--personal {
  background: linear-gradient(135deg, #5b8def, #4178d6);
}

.ss-header-icon--team {
  background: linear-gradient(135deg, #34c759, #2da44e);
}

.ss-header-icon--public {
  background: linear-gradient(135deg, #ff9500, #e68600);
}

.ss-header-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.ss-header-name {
  font-size: 20px;
  font-weight: 700;
  color: var(--text-primary);
  margin: 0;
  letter-spacing: -0.02em;
}

.ss-header-kind {
  font-size: 12px;
  color: var(--text-muted);
}

/* ===== Public notice ===== */
.ss-public-notice {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 20px 24px;
  background: var(--bg-card);
  border-radius: var(--radius-md);
}

.ss-public-icon {
  color: var(--text-muted);
  flex-shrink: 0;
  margin-top: 1px;
}

.ss-public-text {
  font-size: 14px;
  color: var(--text-secondary);
  line-height: 1.6;
  margin: 0;
}

/* ===== Member section ===== */
.ss-section {
  background: var(--bg-card);
  border-radius: var(--radius-md);
  overflow: hidden;
}

.ss-section-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px 20px;
  border-bottom: 1px solid var(--border-light);
}

.ss-section-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary);
  margin: 0;
}

.ss-add-btn {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 13px;
  font-weight: 600;
  color: var(--accent);
  background: var(--accent-light);
  padding: 6px 12px;
  border-radius: var(--radius-xs);
  transition: var(--transition-fast);
}

.ss-add-btn:hover {
  background: var(--accent-lighter);
}

/* ===== Member row ===== */
.ss-member-list {
  display: flex;
  flex-direction: column;
}

.ss-member-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 20px;
  border-bottom: 1px solid var(--border-lighter);
  transition: var(--transition-fast);
}

.ss-member-row:last-child {
  border-bottom: none;
}

.ss-member-row:hover {
  background: var(--bg-hover);
}

.ss-member-avatar {
  width: 36px;
  height: 36px;
  border-radius: 50%;
  background: var(--accent-gradient);
  color: #fff;
  font-size: 14px;
  font-weight: 700;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.ss-member-info {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
}

.ss-member-name {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ss-member-username {
  font-size: 12px;
  color: var(--text-muted);
}

.ss-member-role {
  flex-shrink: 0;
  min-width: 110px;
  display: flex;
  align-items: center;
}

.ss-role-tag {
  display: inline-flex;
  align-items: center;
  padding: 4px 10px;
  border-radius: var(--radius-xs);
  font-size: 12px;
  font-weight: 600;
}

.ss-role-tag--owner {
  background: var(--warning-light);
  color: var(--warning);
}

.ss-member-remove {
  width: 30px;
  height: 30px;
  border-radius: var(--radius-xs);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-muted);
  background: transparent;
  transition: var(--transition-fast);
  flex-shrink: 0;
}

.ss-member-remove:hover:not(:disabled) {
  background: var(--danger-light);
  color: var(--danger);
}

.ss-member-remove:disabled {
  opacity: 0.3;
  cursor: not-allowed;
}

/* ===== Empty state ===== */
.ss-member-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 40px 20px;
  color: var(--text-muted);
}

.ss-member-empty p {
  font-size: 13px;
  margin: 0;
}

/* ===== Add modal form ===== */
.ss-add-form {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.ss-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.ss-field-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-muted);
}

.ss-field-hint {
  font-size: 11px;
  color: var(--text-muted);
  line-height: 1.5;
}

.ss-search-input {
  width: 100%;
  padding: 10px 14px;
  border-radius: var(--radius-xs);
  background: var(--bg-input);
  border: 1px solid transparent;
  font-size: 14px;
  color: var(--text-primary);
  outline: none;
  transition: var(--transition-fast);
}

.ss-search-input:focus {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-ring);
}

.ss-search-results {
  max-height: 200px;
  overflow-y: auto;
  overscroll-behavior: contain;
  border-radius: var(--radius-xs);
  background: var(--bg-input);
}

.ss-user-option {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  text-align: left;
  padding: 8px 12px;
  border-radius: var(--radius-xs);
  transition: var(--transition-fast);
}

.ss-user-option:hover {
  background: var(--bg-hover);
}

.ss-user-option.selected {
  background: var(--accent-light);
}

.ss-user-option-avatar {
  width: 28px;
  height: 28px;
  border-radius: 50%;
  background: var(--accent-gradient);
  color: #fff;
  font-size: 12px;
  font-weight: 700;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.ss-user-option-info {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 0;
  min-width: 0;
}

.ss-user-option-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
}

.ss-user-option-username {
  font-size: 11px;
  color: var(--text-muted);
}

.ss-user-option-check {
  color: var(--accent);
  flex-shrink: 0;
}

/* ===== Modal actions ===== */
.ss-modal-actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  padding-top: 4px;
}

/* ===== Modal buttons ===== */
.ss-btn {
  font-size: 13px;
  font-weight: 600;
  padding: 8px 20px;
  border-radius: var(--radius-xs);
  transition: var(--transition-fast);
}

.ss-btn--ghost {
  color: var(--text-secondary);
  background: transparent;
}

.ss-btn--ghost:hover {
  background: var(--bg-hover);
}

.ss-btn--primary {
  color: #fff;
  background: var(--accent);
}

.ss-btn--primary:hover:not(:disabled) {
  background: var(--accent-hover);
}

.ss-btn--primary:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

/* ===== Loading ===== */
.ss-loading {
  text-align: center;
  padding: 60px 20px;
  color: var(--text-muted);
  font-size: 14px;
}

/* ===== Transfer button ===== */
.ss-member-transfer {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  font-weight: 600;
  color: var(--warning);
  background: var(--warning-light);
  padding: 5px 10px;
  border-radius: var(--radius-xs);
  transition: var(--transition-fast);
  flex-shrink: 0;
}

.ss-member-transfer:hover {
  background: var(--warning);
  color: #fff;
}

/* ===== Danger zone ===== */
.ss-danger-zone {
  margin-top: 24px;
  padding: 20px;
  background: var(--bg-card);
  border-radius: var(--radius-md);
  border: 1px solid var(--danger-light);
}

.ss-danger-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  font-weight: 600;
  padding: 8px 16px;
  border-radius: var(--radius-xs);
  transition: var(--transition-fast);
}

.ss-danger-btn--disband {
  color: #fff;
  background: var(--danger);
}

.ss-danger-btn--disband:hover {
  background: var(--danger-hover);
}

.ss-danger-btn--leave {
  color: var(--danger);
  background: var(--danger-light);
}

.ss-danger-btn--leave:hover {
  background: var(--danger);
  color: #fff;
}
</style>
