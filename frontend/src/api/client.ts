import axios from 'axios'

// ===========================================================================
// Axios 实例 & 拦截器（保持不变）
// ===========================================================================

const client = axios.create({
  baseURL: '/api/v1',
  timeout: 10000,
  paramsSerializer: { indexes: null },
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
  (error) => {
    if (error.response?.status === 401) {
      localStorage.removeItem('token')
      localStorage.removeItem('user')
      // 使用 window.location 跳转，避免引入 router/store 造成循环依赖（HMR 问题）
      window.location.href = '/login'
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

/// 知识树类型（lowercase）
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
  reviewable_by_me?: boolean
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
    return { ...res, data: unwrapQuestionList(res.data as any) }
  },
  get(id: string) {
    return client.get<QuestionDetail>(`/questions/${id}`)
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

export const paperApi = {
  /// 试卷轻量列表（仅 id + title，供下拉选择）
  listBrief() {
    return client.get<PaperBrief[]>('/papers/brief')
  },
  /// 反向查询：题目被引用的试卷列表
  getQuestionPapers(questionId: string) {
    return client.get<QuestionPaperItem[]>(`/questions/${questionId}/papers`)
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

/// 知识树（多树容器：知识树 / 能力树 / 章节树）
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
  list() {
    return client.get<KnowledgeTree[]>('/knowledge-trees')
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
}

export interface ParsedQuestion {
  /// B3：新增 'multiple' 题型
  question_type: 'choice' | 'multiple' | 'fill' | 'solution'
  sub_type?: string
  /// AI 返回 "easy"/"medium"/"hard"，后端转换为 1-5
  difficulty?: string
  stem: string
  options?: ParsedOption[]
  correct_answer: ParsedAnswer
  analysis: AnalysisMethod[]
  knowledge_points: string[]
  confidence: number
  warnings: string[]
  image_placeholders: string[]
  kp_matches: KpMatch[]
}

export interface AiSettings {
  provider: string
  has_api_key: boolean
  model_text: string | null
  model_vision: string | null
}

export const aiApi = {
  parseText(text: string) {
    return client.post<{ data: ParsedQuestion }>('/ai/parse-text', { text })
  },
  parseImage(file: File) {
    const formData = new FormData()
    formData.append('image', file)
    // 不要手动设置 Content-Type — axios + FormData 需要自动生成 boundary
    return client.post<{ data: ParsedQuestion[] }>('/ai/parse-image', formData, {
      timeout: 120000,
    })
  },
  getSettings() {
    return client.get<AiSettings>('/ai/settings')
  },
  updateSettings(data: {
    provider?: string
    api_key?: string
    model_text?: string
    model_vision?: string
  }) {
    return client.put<AiSettings>('/ai/settings', data)
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

/// AI 打标响应
export interface AiTaggingResponse {
  /// 匹配成功的知识点节点列表
  knowledge_nodes: KnowledgeNodeMatch[]
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
}

export const aiTaggingApi = {
  /// 智能打标：分析题目文本 → LLM 提取标签 → pg_trgm 三级匹配知识点 UUID
  tag(data: AiTaggingRequest) {
    return client.post<AiTaggingResponse>('/questions/ai-tagging', data)
  },
}

// ===========================================================================
// AI 异步解析任务队列
// ===========================================================================

export type AiTaskStatus = 'pending' | 'processing' | 'completed' | 'failed'

export interface SubmitParseTaskResponse {
  task_id: string
  status: AiTaskStatus
  created_at: string
}

export interface AiParseTaskDetail {
  id: string
  status: AiTaskStatus
  question_id: string | null
  error_message: string | null
  created_at: string
  updated_at: string
}

export const aiTaskApi = {
  submitParseTask(raw_text: string) {
    return client.post<SubmitParseTaskResponse>('/ai/parse', { raw_text })
  },
  getTaskStatus(task_id: string) {
    return client.get<AiParseTaskDetail>(`/ai/parse/${task_id}`)
  },
}

// ===========================================================================
// 用户中心（Profile）
// ===========================================================================

export interface UserQuota {
  ocr_quota_daily: number
  ocr_quota_used: number
  ocr_quota_remaining: number
  ocr_quota_reset_at: string
  ai_token_quota: number
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
