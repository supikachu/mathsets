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

// ─── 题目相关类型 ───

export interface QuestionSummary {
  id: string
  stem: string
  question_type: 'choice' | 'fill' | 'solution' | 'judgment'
  difficulty: 'easy' | 'medium' | 'hard'
  default_score: number
  status: 'draft' | 'pending' | 'rejected' | 'published' | 'disabled'
  grade: string | null
  creator_id: string | null
  creator_name: string | null
  created_at: string
  updated_at: string
  version: number
}

export interface QuestionDetail {
  id: string
  stem: string
  question_type: string
  difficulty: string
  default_score: number
  status: string
  options: { label: string; content: string }[] | null
  correct_answer: any
  analysis: string | null
  grading_criteria: any | null
  grade: string | null
  semester: string | null
  source: string | null
  creator_id: string | null
  creator_name: string | null
  created_at: string
  updated_by: string | null
  updated_at: string
  version: number
  knowledge_points: { id: string; name: string }[]
}

export interface QuestionQuery {
  status?: string
  question_type?: string
  difficulty?: string
  grade?: string
  knowledge_point_id?: string
  creator_id?: string
  keyword?: string
  page?: number
  page_size?: number
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

export interface KnowledgePoint {
  id: string
  parent_id: string | null
  name: string
  grade: string | null
  sort_order: number
  children: KnowledgePoint[]
}

export interface QuestionStats {
  total: number
  draft: number
  pending: number
  rejected: number
  published: number
  disabled: number
}

export const questionApi = {
  list(params?: QuestionQuery) {
    return client.get<QuestionSummary[]>('/questions', { params })
  },
  get(id: string) {
    return client.get<QuestionDetail>(`/questions/${id}`)
  },
  create(data: any) {
    return client.post<QuestionDetail>('/questions', data)
  },
  update(id: string, data: any) {
    return client.put<QuestionDetail>(`/questions/${id}`, data)
  },
  submit(id: string) {
    return client.post(`/questions/${id}/submit`, {})
  },
  stats() {
    return client.get<QuestionStats>('/questions/stats')
  },
}

export const kpApi = {
  tree() {
    return client.get<KnowledgePoint[]>('/knowledge-points')
  },
}

// ─── 试卷相关 ───

export interface PaperSummary {
  id: string
  title: string
  description: string | null
  subject: string
  grade: string | null
  total_score: number
  duration_minutes: number | null
  status: string
  creator_id: string | null
  creator_name: string | null
  created_at: string
  updated_at: string
  version: number
  question_count: number
}

export interface PaperQuestionItem {
  id: string
  question_id: string
  sort_order: number
  score: number
  section: string | null
  stem: string
  question_type: string
  difficulty: string
}

export interface PaperDetail {
  id: string
  title: string
  description: string | null
  subject: string
  grade: string | null
  total_score: number
  duration_minutes: number | null
  status: string
  creator_id: string | null
  creator_name: string | null
  created_at: string
  updated_at: string
  version: number
  questions: PaperQuestionItem[]
}

export const paperApi = {
  list(params?: { page?: number; page_size?: number; status?: string }) {
    return client.get<PaperSummary[]>('/papers', { params })
  },
  get(id: string) {
    return client.get<PaperDetail>(`/papers/${id}`)
  },
  create(data: { title: string; description?: string; subject?: string; grade?: string }) {
    return client.post<PaperDetail>('/papers', data)
  },
  update(id: string, data: any) {
    return client.put<PaperDetail>(`/papers/${id}`, data)
  },
  delete(id: string) {
    return client.delete(`/papers/${id}`)
  },
  publish(id: string) {
    return client.post(`/papers/${id}/publish`, {})
  },
  addQuestion(paperId: string, data: { question_id: string; score?: number; section?: string }) {
    return client.post(`/papers/${paperId}/questions`, data)
  },
  updateQuestion(paperId: string, questionId: string, data: { score?: number; sort_order?: number }) {
    return client.put(`/papers/${paperId}/questions/${questionId}`, data)
  },
  removeQuestion(paperId: string, questionId: string) {
    return client.delete(`/papers/${paperId}/questions/${questionId}`)
  },
}
