import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { authApi, type LoginRequest, type LoginResponse } from '@/api/client'

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
  const displayName = computed(() => user.value?.display_name || '')
  const userId = computed(() => user.value?.user_id || '')
  const isAdmin = computed(() => role.value === 'Admin' || role.value === 'admin')

  async function login(data: LoginRequest) {
    const res = await authApi.login(data)
    const u = res.data
    token.value = u.token
    user.value = u
    localStorage.setItem('token', u.token)
    localStorage.setItem('user', JSON.stringify(u))
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

  return {
    token,
    user,
    isLoggedIn,
    role,
    displayName,
    userId,
    isAdmin,
    login,
    logout,
  }
})
