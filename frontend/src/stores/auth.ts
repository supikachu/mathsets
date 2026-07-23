import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { authApi, userApi, type LoginRequest, type LoginResponse } from '@/api/client'

function safeParseUser(): LoginResponse | null {
  try {
    return JSON.parse(localStorage.getItem('user') || 'null')
  } catch {
    localStorage.removeItem('user')
    return null
  }
}

export const useAuthStore = defineStore('auth', () => {
  const token = ref(localStorage.getItem('token') || '')
  const user = ref<LoginResponse | null>(safeParseUser())

  const isLoggedIn = computed(() => !!token.value)
  const role = computed(() => user.value?.role || '')
  const globalRole = computed(() => user.value?.global_role || '')
  const displayName = computed(() => user.value?.display_name || '')
  const userId = computed(() => user.value?.user_id || '')
  /// 用户头像 URL（null 时调用方应 fallback 到首字母）
  const avatarUrl = computed(() => user.value?.avatar_url || null)

  // ── 双轨制角色判定 ──────────────────────────────────────────────
  // 旧轨道：role = "admin" / "user"
  const isAdmin = computed(() => role.value === 'Admin' || role.value === 'admin')
  // 新轨道：global_role = "super_admin" / "teacher"
  const isSuperAdmin = computed(() => globalRole.value === 'super_admin')
  const isTeacher = computed(() => globalRole.value === 'teacher')
  /// 统一管理员判定：任一轨道命中即视为管理员（与后端 is_admin_user 对齐）
  const isAdminUnified = computed(() => isAdmin.value || isSuperAdmin.value)

  /// 将当前 user 状态持久化到 localStorage（保持 token 与 user 的一致性）
  function persistUser() {
    if (user.value) {
      localStorage.setItem('user', JSON.stringify(user.value))
    } else {
      localStorage.removeItem('user')
    }
  }

  async function login(data: LoginRequest) {
    const res = await authApi.login(data)
    const u = res.data
    token.value = u.token
    user.value = u
    localStorage.setItem('token', u.token)
    persistUser()
    // 登录后刷新空间列表
    try {
      const { useSpaceStore } = await import('@/stores/space')
      await useSpaceStore().fetchSpaces()
    } catch {
      /* ignore */
    }
    // 使用 window.location 跳转，避免引入 router 造成循环依赖（HMR 问题）
    const params = new URLSearchParams(window.location.search)
    const redirect = params.get('redirect')
    window.location.href = redirect || '/dashboard'
  }

  async function logout() {
    token.value = ''
    user.value = null
    localStorage.removeItem('token')
    localStorage.removeItem('user')
    localStorage.removeItem('currentSpaceId')
    window.location.href = '/login'
  }

  /// 更新个人资料（昵称 / 邮箱）
  /// 成功后同步更新本地的 user 状态 + localStorage，让导航栏热更新
  async function updateProfile(data: { display_name?: string; email?: string }) {
    const res = await userApi.updateMe(data)
    const profile = res.data
    // 同步更新本地 LoginResponse 状态（仅 display_name / avatar_url，role 等保持不变）
    if (user.value) {
      user.value = {
        ...user.value,
        display_name: profile.display_name,
        // avatar_url 可能不在原 LoginResponse 中，这里一并同步
        avatar_url: profile.avatar_url,
      }
      persistUser()
    }
    return profile
  }

  /// 上传头像并同步更新本地状态
  /// 调用方传入已压缩好的 File 对象
  async function uploadAvatar(file: File) {
    const res = await userApi.uploadAvatar(file)
    const avatarUrl = res.data.avatar_url
    if (user.value) {
      user.value = { ...user.value, avatar_url: avatarUrl }
      persistUser()
    }
    return avatarUrl
  }

  return {
    token,
    user,
    isLoggedIn,
    role,
    globalRole,
    displayName,
    userId,
    isAdmin,
    isSuperAdmin,
    isTeacher,
    isAdminUnified,
    avatarUrl,
    login,
    logout,
    updateProfile,
    uploadAvatar,
  }
})
