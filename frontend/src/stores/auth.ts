import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { authApi, type LoginRequest, type LoginResponse } from '@/api/client'
import router from '@/router'

export const useAuthStore = defineStore('auth', () => {
  const token = ref(localStorage.getItem('token') || '')
  const user = ref<LoginResponse | null>(
    JSON.parse(localStorage.getItem('user') || 'null'),
  )

  const isLoggedIn = computed(() => !!token.value)
  const role = computed(() => user.value?.role || '')
  const displayName = computed(() => user.value?.display_name || '')
  const isLeader = computed(() => role.value === 'GroupLeader' || role.value === 'Admin')

  async function login(data: LoginRequest) {
    const res = await authApi.login(data)
    const u = res.data
    token.value = u.token
    user.value = u
    localStorage.setItem('token', u.token)
    localStorage.setItem('user', JSON.stringify(u))
    router.push('/dashboard')
  }

  function logout() {
    token.value = ''
    user.value = null
    localStorage.removeItem('token')
    localStorage.removeItem('user')
    router.push('/login')
  }

  return { token, user, isLoggedIn, role, displayName, isLeader, login, logout }
})
