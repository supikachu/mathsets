import axios from 'axios'
import { useToast } from '@/composables/useToast'

const client = axios.create({
  baseURL: '/api/v1',
  timeout: 10000,
})

// 请求拦截器：自动注入 Bearer token（login/register 不带）
client.interceptors.request.use((config) => {
  const isAuthRoute = config.url === '/auth/login' || config.url === '/auth/register'
  if (!isAuthRoute) {
    const token = localStorage.getItem('token')
    if (token) {
      config.headers.Authorization = `Bearer ${token}`
    }
  }
  return config
})

// 响应拦截器：401 → 跳转登录
client.interceptors.response.use(
  (resp) => resp,
  async (error) => {
    if (error.response?.status === 401) {
      localStorage.removeItem('token')
      localStorage.removeItem('user')
      // 动态导入避免循环依赖
      const { useAuthStore } = await import('@/stores/auth')
      const auth = useAuthStore()
      auth.token = ''
      auth.user = null
      const { default: router } = await import('@/router')
      router.push('/login')
      useToast().error('登录已过期，请重新登录')
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
  space_id?: string
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
  space_id: string
  origin_question_id?: string | null
  knowledge_points: { id: string; name: string }[]
  reviewer_ids?: string[]
  can_review?: boolean
}

export interface PageResult<T> {
  items: T[]
  total: number
  page: number
  page_size: number
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
  space_id?: string
  reviewable_by_me?: boolean
}

export interface SpaceSummary {
  id: string
  kind: 'personal' | 'team' | 'public'
  name: string
  owner_user_id: string | null
  member_count: number | null
  my_role: string | null
  created_at: string
}

export interface SpaceMemberInfo {
  user_id: string
  username: string
  display_name: string
  role: string
  duties: string[]
  joined_at: string
}

export interface SpaceDetail {
  id: string
  kind: 'personal' | 'team' | 'public'
  name: string
  owner_user_id: string | null
  settings: Record<string, any>
  members: SpaceMemberInfo[]
  created_at: string
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

function unwrapQuestionList(data: PageResult<QuestionSummary> | QuestionSummary[]): QuestionSummary[] {
  if (Array.isArray(data)) return data
  return data?.items ?? []
}

export const questionApi = {
  async list(params?: QuestionQuery) {
    const res = await client.get<PageResult<QuestionSummary> | QuestionSummary[]>('/questions', { params })
    return { ...res, data: unwrapQuestionList(res.data as any) }
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
  submit(id: string, body?: { reviewer_ids?: string[]; comment?: string }) {
    return client.post(`/questions/${id}/submit`, body || {})
  },
  review(id: string, body: { action: string; comment?: string }) {
    return client.post(`/questions/${id}/review`, body)
  },
  contribute(id: string) {
    return client.post<QuestionDetail>(`/questions/${id}/contribute`)
  },
  importTo(id: string, target_space_id?: string) {
    return client.post<QuestionDetail>(`/questions/${id}/import`, { target_space_id })
  },
  stats(params?: { space_id?: string }) {
    return client.get<QuestionStats>('/questions/stats', { params })
  },
}

export const spaceApi = {
  list() {
    return client.get<SpaceSummary[]>('/spaces')
  },
  createTeam(name: string) {
    return client.post('/spaces', { name })
  },
  get(id: string) {
    return client.get<SpaceDetail>(`/spaces/${id}`)
  },
  addMember(spaceId: string, userId: string, duties?: string[]) {
    return client.post(`/spaces/${spaceId}/members`, { user_id: userId, duties })
  },
}

export const kpApi = {
  tree(spaceId?: string) {
    const params = spaceId ? { space_id: spaceId } : {}
    return client.get<KnowledgePoint[]>('/knowledge-points', { params })
  },
}


