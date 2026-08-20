import axios from 'axios'
import { useToast } from '@/composables/useToast'

// ===========================================================================
// Axios 实例 & 拦截器（保持不变）
// ===========================================================================

const client = axios.create({
  baseURL: '/api/v1',
  timeout: 10000,
  paramsSerializer: { indexes: null },
})

// ===========================================================================
// AI 额度耗尽错误识别（与后端 consume_ai_quota 的 ERR_QUOTA_EXCEEDED 对齐）
// ===========================================================================

/// 后端额度耗尽错误码 — 见 src/handlers/ai.rs::consume_ai_quota
export const AI_QUOTA_EXCEEDED_CODE = 'ERR_QUOTA_EXCEEDED'

/// 判断一个 axios 错误是否为「今日 AI 额度已耗尽」
export function isQuotaExceededError(e: unknown): boolean {
  const err = e as { response?: { status?: number; data?: { code?: string } } }
  return (
    err?.response?.status === 403 &&
    err?.response?.data?.code === AI_QUOTA_EXCEEDED_CODE
  )
}

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

// 额度耗尽 toast 去重窗口（毫秒）— 多页 PDF 并发 3 路同时 403 时只弹一次
const QUOTA_TOAST_DEDUP_MS = 5000
let lastQuotaToastAt = 0

// 响应拦截器：401 → 跳转登录；AI 额度耗尽 → 集中友好提示
client.interceptors.response.use(
  (resp) => resp,
  (error) => {
    if (error.response?.status === 401) {
      localStorage.removeItem('token')
      localStorage.removeItem('user')
      // 使用 window.location 跳转，避免引入 router/store 造成循环依赖（HMR 问题）
      window.location.href = '/login'
    } else if (isQuotaExceededError(error)) {
      // 集中弹一次友好提示，避免每个调用方都自行识别；
      // 标记 __quotaHandled 让调用方跳过重复 toast（仍 reject 以保留流程控制）。
      const now = Date.now()
      if (now - lastQuotaToastAt > QUOTA_TOAST_DEDUP_MS) {
        lastQuotaToastAt = now
        useToast().error('今日 AI 识别额度已用尽，请明天再试')
      }
      ;(error as Error & { __quotaHandled?: boolean }).__quotaHandled = true
    }
    return Promise.reject(error)
  },
)

export default client

// ===========================================================================
// 枚举类型（与后端 sqlx::Type rename_all 对齐）
// ===========================================================================

/// 题型（lowercase）
export type QuestionType = 'choice' | 'multiple' | 'fill' | 'solution'

/// 题目状态（lowercase）
export type QuestionStatus = 'draft' | 'pending' | 'rejected' | 'published' | 'disabled'

/// 难度：1-5 星制（B2 从 string enum 改为 i16 newtype）
export type Difficulty = number

/// 年级（snake_case）
export type GradeLevel =
  | 'grade_7'
  | 'grade_8'
  | 'grade_9'
  | 'grade_10'
  | 'grade_11'
  | 'grade_12'
  | 'other'

/// 学期（snake_case）
export type SemesterType = 'first' | 'second' | 'full_year'

/// 认知层次 — 布鲁姆分类法（lowercase）
export type CognitiveLevel =
  | 'remember'
  | 'understand'
  | 'apply'
  | 'analyze'
  | 'evaluate'
  | 'create'

/// 考试类型（lowercase）
export type ExamType =
  | 'midterm'
  | 'final'
  | 'gaokao'
  | 'mock'
  | 'entrance'
  | 'daily'
  | 'other'

/// 标签类别（snake_case）— B2 新增 scene + error_prone
export type TagCategory =
  | 'core_competence'
  | 'method'
  | 'school'
  | 'scene'
  | 'error_prone'

/// 知识树类型（lowercase）。ability = 题型专题树（math_method_*），非核心素养
export type KnowledgeTreeKind = 'knowledge' | 'ability' | 'chapter'

/// 知识点关联来源（lowercase）— 审计用
export type KnowledgeLinkSource = 'manual' | 'ai'

// ===========================================================================
// 认证
// ===========================================================================

export interface LoginRequest {
  username: string
  password: string
}

export interface LoginResponse {
  token: string
  user_id: string
  display_name: string
  role: string
  /// 双轨制身份：与 role 配合
  global_role?: 'super_admin' | 'teacher'
  /// 用户头像 URL
  avatar_url?: string | null
}

export interface RegisterRequest {
  username: string
  email: string
  password: string
  display_name: string
}

export const authApi = {
  login(data: LoginRequest) {
    return client.post<LoginResponse>('/auth/login', data)
  },
  register(data: RegisterRequest) {
    return client.post('/auth/register', data)
  },
  me() {
    return client.get<LoginResponse>('/auth/me')
  },
}

// ===========================================================================
// 题目（B2/B3 重构）
// ===========================================================================

/// 题目列表项（B2：移除 grade，difficulty 改为 number，新增 grade_level）
export interface QuestionSummary {
  id: string
  stem: string
  question_type: QuestionType
  /// 1-5 星制
  difficulty: Difficulty
  default_score: number
  status: QuestionStatus
  grade_level: GradeLevel | null
  creator_id: string
  creator_name: string | null
  created_at: string
  updated_at: string
  version: number
  space_id: string
}

/// 题目详情（B2：新增 stem_text/images/metadata/exam_type/cognitive_level 等，
/// 移除 grade/academic_year/grade_semester/exam_region，knowledge_points → knowledge_nodes）
export interface QuestionDetail {
  id: string

  // ── 内容 ──
  stem: string
  stem_text: string | null
  images: unknown | null

  // ── 题型与答案 ──
  question_type: QuestionType
  options: { label: string; content: string }[] | null
  correct_answer: unknown
  analysis: string | null
  grading_criteria: unknown | null

  // ── 难度与评估 ──
  difficulty: Difficulty
  difficulty_score: number | null
  default_score: number
  estimated_minutes: number | null
  cognitive_level: CognitiveLevel | null

  // ── 教研分类 ──
  grade_level: GradeLevel | null
  semester: SemesterType | null

  // ── 来源元数据 ──
  source: string | null
  exam_type: ExamType | null
  /// 长尾元数据 JSONB（academic_year, exam_region, paper_name 等）
  metadata: Record<string, unknown>

  // ── 复合题结构 ──
  parent_id: string | null
  sub_order: number | null

  // ── 统计缓存 ──
  paper_count: number
  attempt_count: number
  accuracy_rate: number | null
  favorite_count: number

  // ── 归属与审计 ──
  status: QuestionStatus
  space_id: string
  origin_question_id: string | null
  creator_id: string
  creator_name: string | null
  created_at: string
  updated_by: string | null
  updated_at: string
  version: number

  // ── 关联数据 ──
  knowledge_nodes: KnowledgeNodeSummary[]
  tags: TagSummary[]
  reviewer_ids: string[]
  can_review: boolean
}

export interface PageResult<T> {
  items: T[]
  total: number
  page: number
  page_size: number
}

/// 题目查询参数（B2：新增多知识点/多标签/范围过滤，移除 grade/knowledge_point_id）
export interface QuestionQuery {
  status?: QuestionStatus
  question_type?: QuestionType
  /// 按难度精确匹配（1-5）
  difficulty?: Difficulty
  /// 按难度范围过滤（与 difficulty 互斥）
  difficulty_min?: number
  difficulty_max?: number
  grade_level?: GradeLevel
  semester?: SemesterType
  cognitive_level?: CognitiveLevel
  exam_type?: ExamType
  /// 多知识点过滤（OR 关系）
  knowledge_node_ids?: string[]
  /// 是否包含子孙节点（LTREE 子树查询）
  include_descendants?: boolean
  /// 多标签过滤（OR 关系）
  tag_ids?: string[]
  creator_id?: string
  keyword?: string
  page?: number
  page_size?: number
  space_id?: string
  /// 学段过滤（junior / senior）
  stage?: string
  /// 学科过滤（math / physics）
  subject?: string
  reviewable_by_me?: boolean
  /// 异步补全机制：按系统标记过滤（命中 GIN 索引）
  /// 'incomplete' = pending_answer OR missing_analysis 并集（与 incomplete_count 的 total 一致）
  system_flag?: 'pending_answer' | 'missing_analysis' | 'incomplete'

  // ── V2.1.1 来源/试卷元数据过滤（P1 检索） ──
  year?: number
  region?: string
  source_type?: string
  document_type?: string
  collection_id?: string
}

/// 自建标签输入（B2：category 改为 enum，新增 parent_id）
export interface NewTagInput {
  name: string
  category: TagCategory
  parent_id?: string | null
}

/// 创建题目请求（B2：与后端 CreateQuestionRequest 对齐）
export interface CreateQuestionRequest {
  stem: string
  question_type: QuestionType
  difficulty: Difficulty
  default_score?: number
  options?: unknown
  correct_answer: unknown
  analysis?: string
  grading_criteria?: unknown
  source?: string
  exam_type?: ExamType
  metadata?: Record<string, unknown>
  grade_level?: GradeLevel
  semester?: SemesterType
  cognitive_level?: CognitiveLevel
  difficulty_score?: number
  estimated_minutes?: number
  images?: unknown
  parent_id?: string
  sub_order?: number
  tag_ids?: string[]
  new_tags?: NewTagInput[]
  knowledge_node_ids?: string[]
  primary_knowledge_node_id?: string
  space_id?: string
  input_method?: string
  /// AI 智能录入来源（暂存项 task_id + staged_index）；保存时后端据此完成容器关联/候选/标记
  ai_meta?: { task_id: string; staged_index: string }
  /// 统一打标确认：建议 ID + 勾选进入候选的 unmatched.id + 等于已有节点的映射
  ai_tagging_confirmation?: {
    suggestion_id: string
    unmatched_ids?: string[]
    alias_maps?: TaggingAliasMap[]
  }
}

/// 更新题目请求（B2：与后端 UpdateQuestionRequest 对齐，所有字段可选）
export interface UpdateQuestionRequest {
  stem?: string
  question_type?: QuestionType
  difficulty?: Difficulty
  default_score?: number
  options?: unknown
  correct_answer?: unknown
  analysis?: string
  grading_criteria?: unknown
  source?: string
  exam_type?: ExamType
  metadata?: Record<string, unknown>
  grade_level?: GradeLevel
  semester?: SemesterType
  cognitive_level?: CognitiveLevel
  difficulty_score?: number
  estimated_minutes?: number
  images?: unknown
  parent_id?: string
  sub_order?: number
  tag_ids?: string[]
  new_tags?: NewTagInput[]
  knowledge_node_ids?: string[]
  primary_knowledge_node_id?: string
  paper_ids?: string[]
  ai_tagging_confirmation?: {
    suggestion_id: string
    unmatched_ids?: string[]
    alias_maps?: TaggingAliasMap[]
  }
}

export interface QuestionStats {
  total: number
  draft: number
  pending: number
  rejected: number
  published: number
  disabled: number
}

function unwrapQuestionList(
  data: PageResult<QuestionSummary> | QuestionSummary[],
): QuestionSummary[] {
  if (Array.isArray(data)) return data
  return data?.items ?? []
}

export const questionApi = {
  async list(params?: QuestionQuery) {
    const res = await client.get<PageResult<QuestionSummary> | QuestionSummary[]>(
      '/questions',
      { params },
    )
    const raw = res.data
    const items = unwrapQuestionList(raw as any)
    // 暴露 total：PageResult.total 优先，数组回退到 length
    const total = Array.isArray(raw) ? items.length : (raw as PageResult<QuestionSummary>)?.total ?? items.length
    return { ...res, data: items, total }
  },
  get(id: string) {
    return client.get<QuestionDetail>(`/questions/${id}`)
  },
  /// V2.1.1 统一来源视图（试卷 + 集合 + Document 链路）
  getSources(id: string) {
    return client.get<QuestionSourceItem[]>(`/questions/${id}/sources`)
  },
  create(data: CreateQuestionRequest) {
    return client.post<QuestionDetail>('/questions', data)
  },
  update(id: string, data: UpdateQuestionRequest) {
    return client.put<QuestionDetail>(`/questions/${id}`, data)
  },
  delete(id: string) {
    return client.delete(`/questions/${id}`)
  },
  submit(id: string, body?: { reviewer_id?: string; reviewer_ids?: string[]; comment?: string }) {
    return client.post(`/questions/${id}/submit`, body || {})
  },
  approve(id: string, body?: { comment?: string }) {
    return client.post(`/questions/${id}/approve`, body || {})
  },
  reject(id: string, body?: { reject_reason?: string }) {
    return client.post(`/questions/${id}/reject`, body || {})
  },
  contribute(id: string) {
    return client.post<QuestionDetail>(`/questions/${id}/contribute`)
  },
  importTo(id: string, target_space_id?: string) {
    return client.post<QuestionDetail>(`/questions/${id}/import`, { target_space_id })
  },
  /// 跨空间克隆（B3 新增路由）
  clone(id: string, target_space_id?: string) {
    return client.post<QuestionDetail>(`/questions/${id}/clone`, { target_space_id })
  },
  stats(params?: { space_id?: string }) {
    return client.get<QuestionStats>('/questions/stats', { params })
  },
  /// 异步补全机制：待补全题目计数（pending_answer / missing_analysis / total）
  incompleteCount() {
    return client.get<{ pending_answer: number; missing_analysis: number; total: number }>(
      '/questions/incomplete-count',
    ).then((r) => r.data)
  },
  /// 异步补全机制：批量提交审核
  batchSubmit(questionIds: string[]) {
    return client.post<{
      total: number
      succeeded: number
      failed: number
      results: Array<{
        id: string
        status: 'success' | 'failed'
        code?: string
        missing?: string[]
      }>
    }>('/questions/batch-submit', { question_ids: questionIds }).then((r) => r.data)
  },
}

// ===========================================================================
// 试卷
// ===========================================================================

export interface PaperBrief {
  id: string
  title: string
}

export interface QuestionPaperItem {
  paper_id: string
  title: string
  sort_order: number
  score: number
  section: string | null
  created_at: string
}

/// V2.1.1 统一来源视图（GET /questions/{id}/sources）
export interface QuestionSourceItem {
  kind: 'paper' | 'collection'
  id: string
  title: string
  type_label: string | null
  question_no: string | null
  display_order: number
  score: number | null
  section: string | null
  document_id: string | null
  document_title: string | null
  document_type: string | null
}

export const paperApi = {
  /// 试卷轻量列表（仅 id + title，供下拉选择）
  listBrief() {
    return client.get<PaperBrief[]>('/papers/brief')
  },
  /// 反向查询：题目被引用的试卷列表（历史兼容；新链路用 questionApi.getSources）
  getQuestionPapers(questionId: string) {
    return client.get<QuestionPaperItem[]>(`/questions/${questionId}/papers`)
  },
}

// ===========================================================================
// 管理端（数据质量）
// ===========================================================================

export interface DataQualitySummary {
  orphan_paper_questions: number
  orphan_collection_questions: number
  papers_without_questions: number
  collections_without_questions: number
  documents_without_sources: number
  duplicate_paper_question_no_groups: number
  duplicate_collection_question_no_groups: number
  questions_without_sources: number
  generated_at: string
}

export const adminApi = {
  dataQualitySummary() {
    return client.get<DataQualitySummary>('/admin/data-quality/summary')
  },
}

// ===========================================================================
// 空间
// ===========================================================================

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
  settings: Record<string, unknown>
  members: SpaceMemberInfo[]
  created_at: string
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
  update(id: string, data: { name?: string; settings?: Record<string, unknown> }) {
    return client.put(`/spaces/${id}`, data)
  },
  delete(id: string) {
    return client.delete(`/spaces/${id}`)
  },
  addMember(spaceId: string, username: string, role?: string, duties?: string[]) {
    return client.post(`/spaces/${spaceId}/members`, { username, role, duties })
  },
  updateMember(spaceId: string, userId: string, data: { role?: string; duties?: string[] }) {
    return client.put(`/spaces/${spaceId}/members/${userId}`, data)
  },
  removeMember(spaceId: string, userId: string) {
    return client.delete(`/spaces/${spaceId}/members/${userId}`)
  },
  transferOwnership(spaceId: string, targetUserId: string) {
    return client.put(`/spaces/${spaceId}/transfer/${targetUserId}`)
  },
  leaveSpace(spaceId: string) {
    return client.delete(`/spaces/${spaceId}/leave`)
  },
}

// ===========================================================================
// 推库申请（公共题库终审流程）
// ===========================================================================

export interface PublicLibrarySubmission {
  id: string
  question_id: string
  source_space_id: string
  source_space_name: string
  submitted_by: string
  submitter_name: string
  status: 'pending' | 'approved' | 'rejected'
  review_comment: string | null
  reviewed_by: string | null
  reviewed_at: string | null
  created_at: string
  stem: string
  question_type: string
  difficulty: number
}

export const publicLibraryApi = {
  submitToPublic(questionId: string, comment?: string) {
    return client.post(`/questions/${questionId}/submit-to-public`, { comment })
  },
  withdraw(submissionId: string) {
    return client.delete(`/public-library/${submissionId}`)
  },
  listPending() {
    return client.get<PublicLibrarySubmission[]>('/public-library/pending')
  },
  review(submissionId: string, action: 'approved' | 'rejected', reviewComment?: string) {
    return client.post(`/public-library/${submissionId}/review`, { action, review_comment: reviewComment })
  },
  getSubmissionStatus(questionId: string) {
    return client.get<{ has_pending_submission: boolean; submission_id: string | null }>(
      `/questions/${questionId}/public-submission`,
    )
  },
}

// ===========================================================================
// 知识树 & 知识点节点（B2/B3 新增，替代旧 KnowledgePoint）
// ===========================================================================

/// 知识树（多树容器：知识树 / 题型专题树 / 章节树）
export interface KnowledgeTree {
  id: string
  code: string
  name: string
  kind: KnowledgeTreeKind
  /// NULL = 全局预置；非 NULL = 空间私有
  space_id: string | null
  version: number
  description: string | null
  is_active: boolean
  created_at: string
  updated_at: string
}

/// 知识点节点（数据库行）
export interface KnowledgeNode {
  id: string
  tree_id: string
  parent_id: string | null
  /// 节点独立 code（仅当前层级标识）
  code: string | null
  /// LTREE 物化路径，如 'n1.n12.n123'
  path: string
  depth: number
  name: string
  /// 同义词数组 JSONB，如 [{"alias":"抛物线函数","locale":"zh"}]
  aliases: unknown
  description: string | null
  sort_order: number
  /// 反规范化缓存：关联题目数
  question_count: number
  is_active: boolean
  created_at: string
  updated_at: string
}

/// 知识点树节点（带 children，用于前端树形展示）
export interface KnowledgeNodeTreeNode {
  id: string
  tree_id: string
  parent_id: string | null
  code: string | null
  path: string
  depth: number
  name: string
  aliases: unknown
  description: string | null
  sort_order: number
  question_count: number
  children: KnowledgeNodeTreeNode[]
}

/// 知识点摘要（用于题目详情中的关联展示）
export interface KnowledgeNodeSummary {
  id: string
  tree_id: string
  name: string
  path: string
  depth: number
  /// 所属知识树类型（chapter / knowledge / ability=题型专题）
  kind: string
  /// 是否主知识点（每题最多 1 个）
  is_primary: boolean
  /// AI 匹配置信度（0.0-1.0）
  ai_confidence: number | null
  /// 关联来源（manual / ai）
  source: KnowledgeLinkSource
}

export interface CreateKnowledgeTreeRequest {
  code: string
  name: string
  kind?: KnowledgeTreeKind
  space_id?: string
  description?: string
}

export interface UpdateKnowledgeTreeRequest {
  name?: string
  description?: string
  is_active?: boolean
}

export interface CreateKnowledgeNodeRequest {
  tree_id: string
  parent_id?: string | null
  code?: string
  name: string
  aliases?: unknown
  description?: string
  sort_order?: number
}

export interface UpdateKnowledgeNodeRequest {
  name?: string
  code?: string
  aliases?: unknown
  description?: string
  sort_order?: number
  is_active?: boolean
}

export const knowledgeTreeApi = {
  list(params?: { kind?: KnowledgeTreeKind; space_id?: string }) {
    return client.get<KnowledgeTree[]>('/knowledge-trees', { params })
  },
  create(data: CreateKnowledgeTreeRequest) {
    return client.post<KnowledgeTree>('/knowledge-trees', data)
  },
  update(id: string, data: UpdateKnowledgeTreeRequest) {
    return client.put<KnowledgeTree>(`/knowledge-trees/${id}`, data)
  },
  remove(id: string) {
    return client.delete(`/knowledge-trees/${id}`)
  },
}

export const knowledgeNodeApi = {
  /// 按知识树列出所有节点（平铺）
  listByTree(treeId: string) {
    return client.get<KnowledgeNode[]>(`/knowledge-trees/${treeId}/nodes`)
  },
  /// 获取知识树的树形结构（带 children 递归）
  getTree(treeId: string) {
    return client.get<KnowledgeNodeTreeNode[]>(`/knowledge-trees/${treeId}/nodes/tree`)
  },
  create(data: CreateKnowledgeNodeRequest) {
    return client.post<KnowledgeNode>('/knowledge-nodes', data)
  },
  get(id: string) {
    return client.get<KnowledgeNode>(`/knowledge-nodes/${id}`)
  },
  update(id: string, data: UpdateKnowledgeNodeRequest) {
    return client.put<KnowledgeNode>(`/knowledge-nodes/${id}`, data)
  },
  remove(id: string) {
    return client.delete(`/knowledge-nodes/${id}`)
  },
  /// 获取子树（所有子孙节点）
  descendants(id: string, includeSelf = false) {
    return client.get<KnowledgeNode[]>(`/knowledge-nodes/${id}/descendants`, {
      params: { include_self: includeSelf },
    })
  },
  /// 移动节点（改 parent_id，后端重算 path 与 depth）
  move(id: string, newParentId: string | null) {
    return client.post(`/knowledge-nodes/${id}/move`, { new_parent_id: newParentId })
  },
  /// V2.1.1 canonical 合并（环检测 + 审计，不物理删除）
  merge(id: string, targetId: string, reason?: string) {
    return client.post<{ message: string; migrated_relations: number }>(
      `/knowledge-nodes/${id}/merge`,
      { target_id: targetId, reason },
    )
  },
}

// ===========================================================================
// V2.1.1 标签治理：候选审核队列
// ===========================================================================

export interface TagCandidate {
  id: string
  kind: 'chapter' | 'knowledge' | 'method' | 'pattern' | 'core_competence'
  target_type: 'knowledge_node' | 'tag'
  raw_name: string
  normalized_name: string
  suggested_node_id: string | null
  suggested_tag_id: string | null
  ai_confidence: string | null
  match_score: string | null
  source_task_id: string | null
  source_question_id: string | null
  status: 'pending' | 'approved' | 'rejected' | 'merged'
  reviewed_by: string | null
  reviewed_at: string | null
  review_note: string | null
  created_at: string
}

export interface TagCandidateSuggestedNode {
  id: string
  name: string
  name_path: string
  tree_name: string
  tree_kind: string
}

export interface TagCandidateSuggestedTag {
  id: string
  name: string
  category: string
}

export interface TagCandidateSourceQuestion {
  id: string
  stem: string
  question_type: string
  options: { label: string; content: string }[] | string[] | null
}

export interface TagCandidateDetail {
  candidate: TagCandidate
  source_stem: string | null
  source_question?: TagCandidateSourceQuestion | null
  source_task_id: string | null
  suggested_node?: TagCandidateSuggestedNode | null
  suggested_tag?: TagCandidateSuggestedTag | null
}

export interface TagCandidateListResponse {
  items: TagCandidate[]
  total: number
  page: number
  page_size: number
}

export interface ApproveCandidateRequest {
  /// new_node | alias | merge
  action: 'new_node' | 'alias' | 'merge'
  tree_id?: string
  parent_id?: string
  name?: string
  target_node_id?: string
  target_tag_id?: string
  reason?: string
}

export const tagCandidateApi = {
  list(params?: { status?: string; kind?: string; target_type?: string; page?: number; page_size?: number }) {
    return client.get<TagCandidateListResponse>('/admin/tag-candidates', { params })
  },
  get(id: string) {
    return client.get<TagCandidateDetail>(`/admin/tag-candidates/${id}`)
  },
  approve(id: string, body: ApproveCandidateRequest) {
    return client.post<{
      message: string
      action: string
      status?: string
      target_node_id?: string | null
      target_tag_id?: string | null
    }>(`/admin/tag-candidates/${id}/approve`, body)
  },
  reject(id: string, reason?: string) {
    return client.post<{ message: string }>(`/admin/tag-candidates/${id}/reject`, { reason })
  },
}

// ===========================================================================
// 标签（B2/B3：增加层级 + 枚举 category + aliases）
// ===========================================================================

/// 标签摘要（用于题目详情中的关联展示）
export interface TagSummary {
  id: string
  name: string
  category: TagCategory
}

/// 标签（数据库行，B2 支持层级 + aliases）
export interface Tag {
  id: string
  parent_id: string | null
  name: string
  category: TagCategory
  /// LTREE 物化路径
  path: string
  /// 同义词数组 JSONB
  aliases: unknown
  description: string | null
  space_id: string | null
  use_count: number
  is_active: boolean
  created_at: string
}

/// 标签树节点（带 children）
export interface TagTreeNode {
  id: string
  parent_id: string | null
  name: string
  category: TagCategory
  path: string
  aliases: unknown
  use_count: number
  children: TagTreeNode[]
}

export interface CreateTagRequest {
  name: string
  category: TagCategory
  parent_id?: string | null
  aliases?: unknown
  description?: string
  space_id?: string
}

/// 更新标签（不允许修改 category 以保证树一致性）
export interface UpdateTagRequest {
  name?: string
  aliases?: unknown
  description?: string
  is_active?: boolean
}

/// 标签查询参数（B2 增强）
export interface TagQuery {
  category?: TagCategory
  space_id?: string
  /// 是否返回树形结构（带 children）
  as_tree?: boolean
  /// 按父节点过滤（null = 仅根节点）
  parent_id?: string | null
}

export const tagsApi = {
  list(params?: TagQuery) {
    return client.get<Tag[]>('/tags', { params })
  },
  suggest(q: string, category?: TagCategory, spaceId?: string) {
    return client.get<Tag[]>('/tags/suggest', {
      params: { q, category, space_id: spaceId },
    })
  },
  create(data: CreateTagRequest) {
    return client.post<Tag>('/tags', data)
  },
  update(id: string, data: UpdateTagRequest) {
    return client.put<Tag>(`/tags/${id}`, data)
  },
  remove(id: string) {
    return client.delete(`/tags/${id}`)
  },
  merge(sourceId: string, targetId: string) {
    return client.post(`/tags/${sourceId}/merge`, { target_id: targetId })
  },
  /// V2.1.1 标签使用情况
  usage(id: string) {
    return client.get<{ tag_id: string; name: string; category: string; use_count: number; question_count: number }>(
      `/tags/${id}/usage`,
    )
  },
}

// ===========================================================================
// AI 智能录入（parse-text / parse-image / settings）
// ===========================================================================

export interface SubAnswer {
  sub_id: number
  content: string
}

export interface AnalysisMethod {
  title: string
  content: string
}

export interface BlankAnswer {
  position: number
  answer: string
}

export interface ParsedOption {
  label: string
  content: string
}

export interface ParsedAnswer {
  kind: 'choice' | 'fill' | 'solution'
  value: {
    options?: string[]
    blanks?: BlankAnswer[]
    subs?: SubAnswer[]
  }
}

export interface KpMatch {
  ai_name: string
  matched_id: string | null
  matched_name: string | null
  score: number
  /// 匹配节点所属树类型（'chapter'|'knowledge'|'ability'）；
  /// AI 录入回填时由题目详情的 knowledge_nodes.kind 携带，
  /// 工作台据此分发到 章节/知识点/题型专题 三个已选数组（缺失时兜底按知识点处理）
  kind?: string
}

export interface ParsedQuestion {
  /// B3：新增 'multiple' 题型
  question_type: 'choice' | 'multiple' | 'fill' | 'solution'
  sub_type?: string
  /// AI 返回 "easy"/"medium"/"hard"，后端转换为 1-5
  difficulty?: string
  /// 异步任务路径专用：题目已落库时携带 UUID，前端据此走 update 而非 create 避免重复落库
  /// 同步路径（parseImage / parseText）不带此字段，保持 undefined
  id?: string
  stem: string
  options?: ParsedOption[]
  correct_answer: ParsedAnswer
  analysis: AnalysisMethod[]
  knowledge_points: string[]
  confidence: number
  warnings: string[]
  image_placeholders: string[]
  image_urls: string[]
  kp_matches: KpMatch[]
  /// AI 智能录入暂存来源：提供时保存走 create + ai_meta（后端从暂存项落库）；
  /// 与 id（已落库）互斥，用于"解析结果暂存、确认后入库"链路
  ai_meta?: { task_id: string; staged_index: string }
  /// 统一打标建议 ID（确认保存时回传）
  tagging_suggestion_id?: string | null
  tagging_unmatched?: TaggingUnmatched[]
  tag_matches?: TagMatch[]
  existing_question_id?: string | null
  /** 与编辑页「AI 智能打标」同一套 matches；OCR 回填时优先于 kp_matches */
  tagging_matches?: TaggingMatch[]
  grade_level?: string | null
  cognitive_level?: string | null
  tagging_difficulty?: number | null
  tagging_question_type?: string | null
  /** junior | senior，OCR worker 打标时使用的学段 */
  tagging_stage?: 'junior' | 'senior' | null
}

export interface AiSettings {
  provider: string
  has_api_key: boolean
  model_text: string | null
  model_vision: string | null
  // M3：OCR 引擎配置（脱敏）
  ocr_provider: string
  has_doc2x_key: boolean
  mineru_endpoint: string | null
  has_mineru_key: boolean
}

export const aiApi = {
  getSettings() {
    return client.get<AiSettings>('/ai/settings')
  },
  updateSettings(data: {
    provider?: string
    api_key?: string
    model_text?: string
    model_vision?: string
    ocr_provider?: string
    doc2x_api_key?: string
    mineru_endpoint?: string
    mineru_api_key?: string
  }) {
    return client.put<AiSettings>('/ai/settings', data)
  },
  /// OCR 引擎连接测试（M3 新增）
  testOcrConnection(data: { provider: string; api_key?: string; endpoint?: string }) {
    return client.post<{ ok: boolean; latency_ms: number; message: string }>(
      '/ai/ocr/test-connection',
      data,
      { timeout: 15000 },
    )
  },
}

// ===========================================================================
// AI 智能打标（B3 新增：LLM 提取 + 三级模糊匹配）
// ===========================================================================

/// AI 打标请求
export interface AiTaggingRequest {
  /// 题目文本（题干 + 选项 + 答案 + 解析，越完整越准确）
  content: string
  /// 可选空间 ID（限定在该空间的知识树 + 全局树内匹配）
  space_id?: string
  question_id?: string
  /** 学段 junior | senior，约束只召回对应学段知识树 */
  stage?: 'junior' | 'senior'
}

export type TaggingDimension =
  | 'chapter'
  | 'knowledge'
  | 'pattern'
  | 'method'
  | 'core_competence'

export type TaggingTargetType = 'knowledge_node' | 'tag'

export interface TaggingMatch {
  dimension: TaggingDimension
  target_type: TaggingTargetType
  ai_name: string
  target_id: string
  target_name: string
  tree_id?: string | null
  path?: string | null
  depth?: number | null
  category?: string | null
  score: number
  match_type: string
}

export interface TaggingUnmatched {
  id: string
  dimension: TaggingDimension
  target_type: TaggingTargetType
  raw_name: string
  normalized_name: string
  confidence: number | null
  reason: string
  eligible_for_candidate: boolean
}

/** 编辑页把未匹配指到已有节点/标签；与后端 AiTaggingConfirmation.alias_maps 对齐 */
export interface TaggingAliasMap {
  unmatched_id: string
  node_id?: string | null
  tag_id?: string | null
}

/// AI 打标返回的单个知识点匹配结果
export interface KnowledgeNodeMatch {
  /// AI 返回的原始名称
  ai_name: string
  /// 匹配到的知识点节点 UUID
  node_id: string
  /// 匹配到的知识点名称
  node_name: string
  /// 所属知识树 ID
  tree_id: string
  /// 物化路径（前端可用于展示层级）
  path: string
  /// 节点深度
  depth: number
  /// 匹配置信度（0.0-1.0）
  score: number
  /// 匹配类型：exact / alias / fuzzy
  match_type: string
}

/// AI 打标返回的单个标签匹配结果（核心素养 / 通用方法）
export interface TagMatch {
  /// AI 返回的原始名称
  ai_name: string
  /// 匹配到的标签 UUID
  tag_id: string
  /// 匹配到的标签名称
  tag_name: string
  /// 标签类别（core_competence / method）
  category: string
  /// 匹配置信度（0.0-1.0）
  score: number
  /// 匹配类型：exact / alias / fuzzy
  match_type: string
}

/// AI 打标响应
export interface AiTaggingResponse {
  /// 匹配成功的知识点节点列表
  knowledge_nodes: KnowledgeNodeMatch[]
  /// 匹配成功的核心素养标签列表
  competency_tags: TagMatch[]
  /// 匹配成功的通用方法标签列表
  method_tags: TagMatch[]
  /// AI 推断的难度（1-5）
  difficulty: number | null
  /// AI 推断的题型
  question_type: QuestionType | null
  /// AI 推断的年级
  grade_level: GradeLevel | null
  /// AI 推断的认知层次
  cognitive_level: CognitiveLevel | null
  /// AI 返回但未匹配上的知识点名称（前端可提示用户手动选择）
  unmatched_knowledge_points: string[]
  /// 统一建议 ID（确认保存时回传）
  suggestion_id?: string | null
  engine_version?: string
  needs_review?: boolean
  unmatched?: TaggingUnmatched[]
  matches?: TaggingMatch[]
}

export const aiTaggingApi = {
  /// 智能打标：分析题目文本 → LLM 提取标签 → pg_trgm 三级匹配知识点 UUID
  tag(data: AiTaggingRequest) {
    return client.post<AiTaggingResponse>('/questions/ai-tagging', data, { timeout: 180000 })
  },
  createTask(data: AiTaggingRequest) {
    return client.post<{ id: string; status: string; reused: boolean }>(
      '/questions/ai-tagging-tasks',
      data,
    )
  },
  getTask(id: string) {
    return client.get<AiTaggingTaskDetail>(`/questions/ai-tagging-tasks/${id}`)
  },
  cancelTask(id: string) {
    return client.post<{ id: string; status: string }>(`/questions/ai-tagging-tasks/${id}/cancel`)
  },
}

export type AiTaggingTaskStatus =
  | 'pending'
  | 'processing'
  | 'retrying'
  | 'success'
  | 'failed'
  | 'cancelled'
  | 'cancelling'

export interface AiTaggingTaskDetail {
  id: string
  status: AiTaggingTaskStatus
  retry_count: number
  error_message: string | null
  suggestion_id: string | null
  suggestion: AiTaggingResponse | null
  cancelling?: boolean
  created_at: string
  started_at: string | null
  completed_at: string | null
  updated_at: string
}

// ===========================================================================
// V2.1.1 AI 异步解析任务（Document → Task → Worker）
// ===========================================================================

export type AiTaskStatus =
  | 'pending'
  | 'processing'
  | 'retrying'
  | 'success'
  | 'partial_success'
  | 'failed'
  | 'cancelled'
  | 'completed'

export interface AiParseTaskDetail {
  id: string
  /// completed → success 映射后的视图状态
  status: AiTaskStatus
  error_message: string | null
  created_at: string
  updated_at: string
  total_count: number
  processed_count: number
  success_count: number
  failed_count: number
  retry_count: number
  current_page: number | null
  total_pages: number | null
  current_question_no: string | null
  started_at: string | null
  completed_at: string | null
  /// 结果关联
  paper_id: string | null
  collection_ids: string[]
  question_ids: string[]
  /** 本任务产生、待审核的未匹配标签候选数（章节/知识点/通用方法/题型专题） */
  pending_candidate_count: number
  /** 暂存题目（解析完成、待人工确认保存；按原文顺序） */
  staged_questions: AiStagedQuestion[]
}

/// AI 智能录入暂存项（对应后端 progress.staged_questions 数组元素）
export interface AiStagedQuestion {
  index: string
  /// 后端 ParsedQuestion 序列化结果（含 chapter_path / solution_methods / question_no 等）
  parsed: Record<string, unknown>
  images: string[]
  page_image_url?: string | null
  space_id: string
  paper_id?: string | null
  collection_id?: string | null
  is_mixed?: boolean
  /// hash 命中已有题目时携带（前端提示复用，不重复创建）
  existing_question_id?: string | null
  matched: AiStagedMatch[]
  unmatched?: Record<string, string[]>
  saved: boolean
  saved_question_id?: string | null
  merged_into?: string
  order?: { question_no?: string | null; display_order?: number | null }
  suggestion_id?: string | null
  engine_version?: string | null
  suggestion?: {
    suggestion_id?: string | null
    unmatched?: TaggingUnmatched[]
    matches?: TaggingMatch[]
    difficulty?: number | null
    question_type?: string | null
    grade_level?: string | null
    cognitive_level?: string | null
  } | null
  /** OCR worker 打标时使用的学段，与编辑页 AI 智能打标一致 */
  tagging_stage?: 'junior' | 'senior' | null
}

export interface AiStagedMatch {
  node_id: string
  node_name: string
  ai_name: string
  tree_id?: string
  path?: string
  depth?: number
  score: number
  match_type: string
  kind: string
}

export interface SubmitParseTaskResponse {
  task_id: string
  status: AiTaskStatus
  created_at: string
}

/// 解析模式：pdf_direct=仅 PDF 直连（失败回前端引导）/ page=仅逐页 OCR / 缺省=自动降级
export type ParseMode = 'pdf_direct' | 'page'

export const aiTaskApi = {
  createParseTask(document_id: string, parse_mode?: ParseMode) {
    return client.post<SubmitParseTaskResponse>('/ai/parse-task', {
      document_id,
      ...(parse_mode ? { parse_mode } : {}),
    })
  },
  getParseTask(task_id: string) {
    return client.get<AiParseTaskDetail>(`/ai/parse-task/${task_id}`)
  },
  cancelParseTask(task_id: string) {
    return client.post<{ message: string }>(`/ai/parse-task/${task_id}/cancel`, {})
  },
}

// ===========================================================================
// V2.1.1 资料/Document（上传页图集 → AI 分类 → 用户确认）
// ===========================================================================

/// DocumentType：文件整体是什么（与后端白名单一致）
export type DocumentType =
  | 'exam'
  | 'mock_exam'
  | 'class_exercise'
  | 'class_example'
  | 'homework'
  | 'preview_exercise'
  | 'textbook_example'
  | 'teaching_material'
  | 'exercise_book'
  | 'chapter_exercise'
  | 'unit_exercise'
  | 'special_training'
  | 'wrong_question'
  | 'mixed'
  | 'unknown'
  | 'other'

/// CollectionType：这一组题是什么（不含 exam/mock_exam/mixed/unknown）
export type CollectionType =
  | 'class_exercise'
  | 'class_example'
  | 'homework'
  | 'preview_exercise'
  | 'textbook_example'
  | 'teaching_material'
  | 'exercise_book'
  | 'chapter_exercise'
  | 'unit_exercise'
  | 'special_training'
  | 'wrong_question'
  | 'other'

export type DocumentStatus =
  | 'uploaded'
  | 'classifying'
  | 'classified'
  | 'confirmed'
  | 'parsing'
  | 'done'
  | 'failed'
  | 'cancelled'

export interface AiClassification {
  document_type: DocumentType
  title?: string
  confidence: number
  reason?: string
  level: number
  checked_pages: number
}

export interface DocumentMeta {
  id: string
  creator_id: string
  file_name: string
  file_size: number | null
  mime: string | null
  page_count: number
  document_type: DocumentType | null
  type_label: string | null
  title: string | null
  source_type: string | null
  sub_source_type: string | null
  status: DocumentStatus
  ai_classification: AiClassification | null
  metadata: Record<string, any>
  conversion_engine: string | null
  created_at: string
  updated_at: string
}

export interface PaperMetaInput {
  title: string
  year?: number
  stage?: string
  grade?: string
  subject?: string
  semester?: string
  region_province?: string
  region_city?: string
  school_name?: string
  source_type?: string
  sub_source_type?: string
  paper_id?: string
}

export interface CollectionMetaInput {
  title: string
  collection_type: CollectionType
  type_label?: string
  source_type?: string
  subject?: string
  stage?: string
  grade?: string
  semester?: string
  chapter_id?: string
}

export interface ConfirmDocumentRequest {
  document_type: DocumentType
  type_label?: string
  title?: string
  source_type?: string
  sub_source_type?: string
  paper_meta?: PaperMetaInput
  collections?: CollectionMetaInput[]
}

export const documentApi = {
  /// 上传页面图片集（PDF 前端渲染为页图后上传）
  /// pdf：原始 PDF 二进制（可选），后端保留供 Doc2X/MinerU 整档直传 OCR 快速路径
  upload(pages: File[], meta: { file_name?: string; file_type?: string; pdf?: File }) {
    const formData = new FormData()
    for (const page of pages) {
      formData.append('pages', page)
    }
    if (meta.file_name) formData.append('file_name', meta.file_name)
    if (meta.file_type) formData.append('file_type', meta.file_type)
    if (meta.pdf) formData.append('pdf', meta.pdf)
    return client.post<{ data: DocumentMeta }>('/ai/documents', formData, {
      timeout: 120000,
    })
  },
  classify(id: string) {
    return client.post<{ data: DocumentMeta; ai_classification: AiClassification }>(
      `/ai/documents/${id}/classify`,
      {},
      { timeout: 120000 },
    )
  },
  confirm(id: string, body: ConfirmDocumentRequest) {
    return client.post<{ data: DocumentMeta }>(`/ai/documents/${id}/confirm`, body)
  },
  list() {
    return client.get<{ data: DocumentMeta[] }>('/ai/documents')
  },
  get(id: string) {
    return client.get<{ data: DocumentMeta }>(`/ai/documents/${id}`)
  },
}

// ===========================================================================
// V2.1.1 题目集合（QuestionCollection / CollectionQuestion）
// ===========================================================================

export interface QuestionCollectionSummary {
  id: string
  document_id: string
  creator_id: string
  title: string
  collection_type: CollectionType
  type_label: string | null
  source_type: string | null
  subject: string | null
  stage: string | null
  grade: string | null
  semester: string | null
  chapter_id: string | null
  metadata: Record<string, any>
  created_at: string
  updated_at: string
}

export interface CollectionQuestionItem {
  id: string
  question_id: string
  question_no: string | null
  display_order: number
  score: number | null
  stem: string
  question_type: string
  difficulty: string
}

export interface CollectionDetail extends QuestionCollectionSummary {
  document_title: string | null
  document_type: string | null
  questions: CollectionQuestionItem[]
}

export interface CollectionListResponse {
  items: QuestionCollectionSummary[]
  total: number
  page: number
  page_size: number
}

export interface BatchAddQuestionsInput {
  question_id: string
  question_no?: string
  display_order?: number
  score?: number
  section?: string
}

export const collectionApi = {
  list(params?: { document_id?: string; page?: number; page_size?: number }) {
    return client.get<CollectionListResponse>('/collections', { params })
  },
  get(id: string) {
    return client.get<CollectionDetail>(`/collections/${id}`)
  },
  batchAddQuestions(id: string, questions: BatchAddQuestionsInput[]) {
    return client.post<{ inserted: number; skipped: number }>(
      `/collections/${id}/questions/batch`,
      { questions },
    )
  },
  removeQuestion(id: string, questionId: string) {
    return client.delete<{ message: string }>(`/collections/${id}/questions/${questionId}`)
  },
}

// ===========================================================================
// 用户中心（Profile）
// ===========================================================================

export interface UserQuota {
  daily_quota: number
  used_today: number
  remaining: number
  reset_at: string
}

export interface UserProfile {
  id: string
  username: string
  email: string
  display_name: string
  role: string
  global_role: 'super_admin' | 'teacher'
  is_active: boolean
  avatar_url: string | null
  quota: UserQuota
  created_at: string
  updated_at: string
}

export const userApi = {
  getMe() {
    return client.get<UserProfile>('/users/me')
  },
  updateMe(data: { display_name?: string; email?: string }) {
    return client.put<UserProfile>('/users/me', data)
  },
  changePassword(data: { old_password: string; new_password: string }) {
    return client.put<{ message: string }>('/users/password', data)
  },
  uploadAvatar(file: File) {
    const formData = new FormData()
    formData.append('avatar', file)
    return client.post<{ avatar_url: string }>('/users/avatar', formData, {
      timeout: 30000,
    })
  },
}

// ── 通用图片上传 API（题目配图等） ──────────────────────────────

export const uploadsApi = {
  /**
   * 上传题目配图等通用图片
   * @param file 浏览器 File 对象
   * @returns 持久化后的 URL（如 /uploads/questions/xxx.png）
   */
  uploadImage(file: File) {
    const formData = new FormData()
    formData.append('image', file)
    return client.post<{ url: string }>('/uploads/images', formData, {
      timeout: 60000, // 大图可能较慢，给 60s
    })
  },
}

// ── 管理员用户管理 API（阶段三新增） ──────────────────────────────

export interface AdminUser {
  id: string
  username: string
  email: string
  display_name: string
  role: string
  global_role: 'super_admin' | 'teacher'
  is_active: boolean
  avatar_url: string | null
  created_at: string
}

export interface CreateUserRequest {
  username: string
  email: string
  password: string
  display_name: string
  global_role?: 'super_admin' | 'teacher'
}

export const adminUserApi = {
  list() {
    return client.get<AdminUser[]>('/admin/users')
  },
  create(data: CreateUserRequest) {
    return client.post<AdminUser>('/admin/users', data)
  },
  getUser(id: string) {
    return client.get<AdminUser>(`/admin/users/${id}`)
  },
  updateRole(id: string, global_role: string) {
    return client.put<AdminUser>(`/admin/users/${id}/role`, { global_role })
  },
  updateStatus(id: string, is_active: boolean) {
    return client.put<AdminUser>(`/admin/users/${id}/status`, { is_active })
  },
  deleteUser(id: string) {
    return client.delete(`/admin/users/${id}`)
  },
}

// ── 通知 ──

export interface Notification {
  id: string
  user_id: string
  kind: string
  title: string
  body: string | null
  resource_type: string | null
  resource_id: string | null
  is_read: boolean
  created_at: string
}

export const notificationApi = {
  getTicket() {
    return client.post<{ ticket: string; expires_in: number }>('/notifications/ticket')
  },
  list() {
    return client.get<Notification[]>('/notifications')
  },
  markRead(id: string) {
    return client.put<Notification>(`/notifications/${id}/read`)
  },
  markAllRead() {
    return client.put<{ updated: number }>('/notifications/read-all')
  },
  delete(id: string) {
    return client.delete(`/notifications/${id}`)
  },
  getUnreadCount() {
    return client.get<{ count: number }>('/notifications/unread-count')
  },
}
