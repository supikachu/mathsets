import axios from 'axios'
import { ElMessage } from 'element-plus'
import router from '@/router'

const client = axios.create({
  baseURL: '/api/v1',
  timeout: 10000,
})

// 请求拦截器：自动注入 Bearer token
client.interceptors.request.use((config) => {
  const token = localStorage.getItem('token')
  if (token) {
    config.headers.Authorization = `Bearer ${token}`
  }
  return config
})

// 响应拦截器：401 → 跳转登录
client.interceptors.response.use(
  (resp) => resp,
  (error) => {
    if (error.response?.status === 401) {
      localStorage.removeItem('token')
      localStorage.removeItem('user')
      router.push('/login')
      ElMessage.error('登录已过期，请重新登录')
    }
    return Promise.reject(error)
  },
)

export default client

// ─── 类型定义 ───

export interface LoginRequest {
  username: string
  password: string
}

export interface LoginResponse {
  token: string
  user_id: string
  display_name: string
  role: string
}

export interface RegisterRequest {
  username: string
  email: string
  password: string
  display_name: string
}

// ─── API 函数 ───

export const authApi = {
  login(data: LoginRequest) {
    return client.post<LoginResponse>('/auth/login', data)
  },
  register(data: RegisterRequest) {
    return client.post('/auth/register', data)
  },
}
