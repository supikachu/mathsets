<script setup lang="ts">
/**
 * AttributeSidePanel — 录题右侧常驻属性面板（替代 AttributeModal）
 *
 * 设计要点：
 * - 320px 宽常驻右侧，Flex 纵向布局，与编辑区/预览区并排
 * - 顶部 "✨ AI 智能打标" 按钮，调用 aiTaggingApi.tag() 一键回填
 * - 知识树标注：可折叠内联面板（Accordion），收起态仅展示已选 Tag + "展开知识树"按钮
 *   展开后原地平滑展开 Tabs（章节|知识点|解题方法）+ 动态折叠树，勾选实时同步无需确定
 *   动态拉取对应 tree（expectedCode = `${subject}_${mode}_${stage}` 严格精确匹配）
 *   实时勾选同步到 chapterNodeIds / knowledgeNodeIds / methodNodeIds
 *   面板外用 Tag 融合展示三组已选节点，支持 x 移除
 * - 标签分为核心素养 / 解题方法 / 学校来源 三组
 * - AI 回填字段加 --purple-light 边框高亮动画，用户手动修改后取消
 * - 严格使用 CSS 变量，复用 AppButton / AppIcon / AppSelect，无第三方 UI 库
 */
import { ref, reactive, computed, watch, nextTick, onMounted, onBeforeUnmount } from 'vue'
import {
  aiTaggingApi,
  tagsApi,
  paperApi,
  type Tag,
  type TagCategory,
  type QuestionType,
  type PaperBrief,
  type KnowledgeTree,
  type KnowledgeNodeTreeNode,
} from '@/api/client'
import { AppButton, AppIcon, AppSelect } from '@/components/ui'
import KnowledgeTreeCheckbox from '@/components/KnowledgeTreeCheckbox.vue'
import { useToast } from '@/composables/useToast'
import {
  getKnowledgeTreeList,
  getKnowledgeTreeData,
  buildTreeMetaIndex,
  type TreeMetaInfo,
} from '@/composables/useKnowledgeTreeCache'
import { useSpaceStore } from '@/stores/space'

// ─────────────────────────────────────────────────────────────────────
// v-model 绑定
// ─────────────────────────────────────────────────────────────────────
const tagIds = defineModel<string[]>('tagIds', { required: true })
const knowledgeNodeIds = defineModel<string[]>('knowledgeNodeIds', { required: true })
/** 章节节点 ID（前端独立维护，提交时与知识点/方法合并为统一 knowledge_node_ids） */
const chapterNodeIds = defineModel<string[]>('chapterNodeIds', { default: () => [] })
/** 解题方法节点 ID（前端独立维护，提交时合并） */
const methodNodeIds = defineModel<string[]>('methodNodeIds', { default: () => [] })
/** 主知识点节点 ID（每题最多 1 个，跨三组节点单选；后端 DTO 字段对齐） */
const primaryKnowledgeNodeId = defineModel<string | null>('primaryKnowledgeNodeId', { default: null })
/** AI 打标新增的树节点 ID（树组件浅金高亮；手动触碰即消、保存成功由父组件全清） */
const aiHighlightIds = defineModel<string[]>('aiHighlightIds', { default: () => [] })
const aiGeneratedFields = defineModel<Set<string>>('aiGeneratedFields', { required: true })
/** 面板折叠状态：父组件通过 v-model:collapsed 双向绑定 */
const collapsed = defineModel<boolean>('collapsed', { default: false })
/** 关联试卷 ID 列表：父组件通过 v-model:paperIds 双向绑定 */
const paperIds = defineModel<string[]>('paperIds', { default: () => [] })

// ─────────────────────────────────────────────────────────────────────
// Props
// ─────────────────────────────────────────────────────────────────────
const props = defineProps<{
  competenceTags: Tag[]
  methodTags: Tag[]
  schoolTags: Tag[]
  /** 父组件 form 的引用，用于 AI 打标时读取题干文本与回填字段 */
  form: {
    stem: string
    question_type: string
    sub_type: string
    difficulty: string
    difficulty_coefficient: number
    grade_semester: string
    grade?: string
    // ── 长尾维度：与 QuestionList 数据字典对齐，统一存入 metadata(JSONB) ──
    year: string
    region_province: string
    region_city: string
    source_type: string
    sub_source_type: string
    options: { label: string; content: string }[]
    sub_answers: string[]
    solutions: string[]
    // ── 知识树动态加载依赖：学段 / 学科（提交时进 metadata） ──
    stage: 'junior' | 'senior'
    subject: 'math' | 'physics'
  }
  /**
   * 初始节点名称映射（id → name），用于编辑场景下不打开弹窗也能展示 Tag 名称。
   * 父组件 loadQuestion 时从 d.knowledge_nodes 构建。
   */
  initialNodeNames?: Record<string, string>
  /**
   * 学段/学科切换的三组节点勾选缓存（父组件持有，随页面销毁）。
   * key = `${subject}_${stage}`，切走前快照、命中时瞬时恢复。
   */
  selectionCache?: Map<string, { chapter: string[]; knowledge: string[]; method: string[] }>
}>()

const toast = useToast()
const space = useSpaceStore()

// ─────────────────────────────────────────────────────────────────────
// 知识树动态加载（从 KnowledgeTreeNav.vue 移植，无 kind 兜底）
// ─────────────────────────────────────────────────────────────────────
type Stage = 'junior' | 'senior'
type Subject = 'math' | 'physics'
type TreeMode = 'chapter' | 'knowledge' | 'method'

// code 命名规则：{subject}_{mode}_{stage}，后端 high 表示高中
const STAGE_CODE: Record<Stage, string> = { junior: 'junior', senior: 'high' }
const SUBJECT_CODE: Record<Subject, string> = { math: 'math', physics: 'physics' }
const MODE_CODE: Record<TreeMode, string> = { chapter: 'chapter', knowledge: 'knowledge', method: 'method' }

const MODES: { key: TreeMode; label: string }[] = [
  { key: 'chapter', label: '章节' },
  { key: 'knowledge', label: '知识点' },
  { key: 'method', label: '解题方法' },
]

// 顶部学段 / 学科下拉选项
const stageOptions = [
  { label: '初中', value: 'junior' },
  { label: '高中', value: 'senior' },
]
const subjectOptions = [
  { label: '数学', value: 'math' },
  { label: '物理', value: 'physics' },
]

// 年级下拉选项（根据学段动态联动计算）
const gradeOptions = computed(() => {
  if (props.form.stage === 'junior') {
    return [
      { label: '七年级', value: '七年级' },
      { label: '八年级', value: '八年级' },
      { label: '九年级', value: '九年级' },
    ]
  }
  if (props.form.stage === 'senior') {
    return [
      { label: '高一', value: '高一' },
      { label: '高二', value: '高二' },
      { label: '高三', value: '高三' },
    ]
  }
  return []
})

// 监听学段切换：防脏数据，学段变更时自动重置年级
watch(
  () => props.form.stage,
  (newStage, oldStage) => {
    if (oldStage && newStage !== oldStage) {
      props.form.grade = ''
    }
  }
)

/** 当前弹窗内选中的模式（默认知识点） */
const treeMode = ref<TreeMode>('knowledge')
/** 当前模式对应的树数据（嵌套结构，直接喂给 KnowledgeTreeCheckbox） */
const treeData = ref<KnowledgeNodeTreeNode[]>([])
const activeTreeId = ref<string>('')
const treeLoading = ref(false)
/** 内联折叠面板展开态（默认收起，点击"展开知识树"后原地展开） */
const treeExpanded = ref(false)

/** 当前模式对应的已选 ID 数组（实时双向同步到对应 v-model，无需确定按钮） */
const currentModeSelectedIds = computed<string[]>({
  get: () => {
    if (treeMode.value === 'chapter') return chapterNodeIds.value
    if (treeMode.value === 'knowledge') return knowledgeNodeIds.value
    return methodNodeIds.value
  },
  set: (ids: string[]) => {
    const oldIds =
      treeMode.value === 'chapter' ? chapterNodeIds.value
      : treeMode.value === 'knowledge' ? knowledgeNodeIds.value
      : methodNodeIds.value
    if (treeMode.value === 'chapter') chapterNodeIds.value = ids
    else if (treeMode.value === 'knowledge') knowledgeNodeIds.value = ids
    else methodNodeIds.value = ids
    clearFieldHighlight('knowledge_node')
    // 用户手动触碰的节点（新旧对称差集，含级联波及项）移出 AI 高亮
    if (aiHighlightIds.value.length > 0) {
      const oldSet = new Set(oldIds)
      const newSet = new Set(ids)
      const touched = new Set<string>()
      for (const id of oldSet) if (!newSet.has(id)) touched.add(id)
      for (const id of newSet) if (!oldSet.has(id)) touched.add(id)
      if (touched.size > 0) {
        aiHighlightIds.value = aiHighlightIds.value.filter(id => !touched.has(id))
      }
    }
  },
})

/** 节点 ID → name 映射，用于面板外 Tag 展示 */
const nodeNameMap = ref<Map<string, string>>(new Map())
/** 节点 ID → { parentId, name, namePath } 扁平 meta（跨已加载树合并），用于 chips 折叠与路径悬浮 */
const nodeMetaMap = ref<Map<string, TreeMetaInfo>>(new Map())
/** 全量树元数据列表（共享缓存只拉一次），用于「解题方法」Tab 可用性判断 */
const treeList = ref<KnowledgeTree[]>([])

/** 期望的 tree code，如 'math_knowledge_high'（高中数学知识点树） */
const expectedCode = computed(() => {
  const subj = SUBJECT_CODE[props.form.subject]
  const mode = MODE_CODE[treeMode.value]
  const stage = STAGE_CODE[props.form.stage]
  return `${subj}_${mode}_${stage}`
})

/** 物理学科 / 解题方法 等后端尚未覆盖时的兜底提示 */
const emptyHint = computed(() => {
  if (props.form.subject === 'physics') return '物理学科资源敬请期待'
  if (treeMode.value === 'method') return '暂无解题方法树'
  if (treeData.value.length === 0) return '当前模式暂无知识树'
  return '无节点'
})

// ─── 三组已选节点融合展示（折叠为最高层已选节点，带 type 标签） ───────
interface SelectedNodeTag {
  id: string
  type: TreeMode
  typeLabel: string
  name: string
  /** 被折叠隐藏的下级已选节点数（级联勾选时顶层节点代管） */
  hiddenCount: number
  /** 完整知识路径（悬浮提示） */
  path: string
  /** 是否主知识点（来自 primaryKnowledgeNodeId，用于星标高亮） */
  isPrimary: boolean
}

/**
 * 每组 ID 折叠为「最高层已选节点」：沿 nodeMetaMap.parentId 向上找到未选中的最高点，
 * 其下被代管的已选子孙数记为 hiddenCount；无 meta（树未加载）的节点按最高层处理
 */
function collapseToTopmost(
  ids: string[],
  type: TreeMode,
  typeLabel: string,
  primaryId: string | null,
): SelectedNodeTag[] {
  const sel = new Set(ids)
  const topmost = new Map<string, number>() // topmostId → 代管的下级已选数
  for (const id of ids) {
    let root = id
    let cur = nodeMetaMap.value.get(id)?.parentId ?? null
    while (cur && sel.has(cur)) {
      root = cur
      cur = nodeMetaMap.value.get(cur)?.parentId ?? null
    }
    if (root === id) {
      if (!topmost.has(root)) topmost.set(root, 0)
    } else {
      topmost.set(root, (topmost.get(root) ?? 0) + 1)
    }
  }
  return [...topmost.entries()].map(([id, hiddenCount]) => {
    const meta = nodeMetaMap.value.get(id)
    const name = meta?.name ?? nodeNameMap.value.get(id)
    if (!name) {
      console.warn('[AttributeSidePanel] 已选节点缺少名称映射，id =', id)
    }
    return {
      id,
      type,
      typeLabel,
      name: name ?? '未知知识点',
      hiddenCount,
      path: meta?.namePath ?? name ?? '未知知识点',
      isPrimary: id === primaryId,
    }
  })
}

const allSelectedNodes = computed<SelectedNodeTag[]>(() => [
  ...collapseToTopmost(chapterNodeIds.value, 'chapter', '章节', primaryKnowledgeNodeId.value),
  ...collapseToTopmost(knowledgeNodeIds.value, 'knowledge', '知识点', primaryKnowledgeNodeId.value),
  ...collapseToTopmost(methodNodeIds.value, 'method', '方法', primaryKnowledgeNodeId.value),
])

const totalSelectedNodes = computed(
  () => chapterNodeIds.value.length + knowledgeNodeIds.value.length + methodNodeIds.value.length,
)

/** 各模式已选数量（Tab 徽标，0 时隐藏） */
const modeCounts = computed<Record<TreeMode, number>>(() => ({
  chapter: chapterNodeIds.value.length,
  knowledge: knowledgeNodeIds.value.length,
  method: methodNodeIds.value.length,
}))

/** 后端是否存在当前学段/学科的解题方法树（无则禁用 Tab） */
const methodTreeAvailable = computed(() => {
  const code = `${SUBJECT_CODE[props.form.subject]}_${MODE_CODE.method}_${STAGE_CODE[props.form.stage]}`
  return treeList.value.some(t => t.code === code)
})

// ─── 数据加载（共享内存缓存：列表全量只拉一次，单树按 treeId 缓存） ───
/**
 * 加载知识树列表，按 expectedCode 严格精确匹配（无 kind 兜底，避免初中物理误抓高中数学）
 */
async function loadTrees() {
  try {
    const list = await getKnowledgeTreeList()
    treeList.value = list
    const matched = list.find(t => t.code === expectedCode.value)
    activeTreeId.value = matched?.id ?? ''
  } catch (e) {
    console.error('[AttributeSidePanel] 加载知识树列表失败', e)
    activeTreeId.value = ''
  }
}

/**
 * 加载当前 activeTreeId 对应的树数据（命中缓存零请求），并填充 nodeNameMap / nodeMetaMap
 */
async function loadTreeData() {
  if (!activeTreeId.value) {
    treeData.value = []
    return
  }
  treeLoading.value = true
  try {
    const data = await getKnowledgeTreeData(activeTreeId.value)
    treeData.value = data
    // 防御性诊断：削顶后接口直返一级节点数组，此处不得再剥离任何层级；
    // 若数据意外为空，打印 warn 帮助区分"数据空"与"渲染异常"
    if (data.length === 0) {
      console.warn('[AttributeSidePanel] 树数据为空（activeTreeId=' + activeTreeId.value + '），请检查接口返回')
    }
    // 递归遍历填充 nodeNameMap（id → name）与 nodeMetaMap（parentId / namePath）
    walkTreeToNameMap(data)
    const meta = buildTreeMetaIndex(data)
    for (const [id, info] of meta) nodeMetaMap.value.set(id, info)
  } catch (e) {
    console.error('[AttributeSidePanel] 加载知识点树失败', e)
    treeData.value = []
  } finally {
    treeLoading.value = false
  }
}

/** 递归遍历树，把 id → name 写入 nodeNameMap */
function walkTreeToNameMap(nodes: KnowledgeNodeTreeNode[]) {
  for (const n of nodes) {
    nodeNameMap.value.set(n.id, n.name)
    if (n.children.length > 0) walkTreeToNameMap(n.children)
  }
}

// ─── 内联树交互 ─────────────────────────────────────────────────────
/** 切换模式：watch 会自动触发对应树的加载，无需额外操作 */
function setMode(m: TreeMode) {
  if (treeMode.value === m) return
  treeMode.value = m
}

/** 树组件实例引用（locateTreeNode 调用其 expandTo 反向定位） */
const treeCheckboxRef = ref<{ expandTo: (id: string) => Promise<boolean> } | null>(null)

/**
 * 反向定位：双击已选标签 → 自动展开面板并切换对应 Tab → 等待树数据
 * 就绪后展开祖先路径、高亮并平滑滚动到该节点（KnowledgeTreeCheckbox.expandTo）
 */
function locateTreeNode(tag: SelectedNodeTag) {
  if (!tag?.id) return
  // 1. 展开知识树面板（默认收起态，双击标签需让树可见）
  treeExpanded.value = true
  // 2. 自动切换 Tab（章节/知识点/方法）
  setMode(tag.type)
  // 3. 等目标模式的树就绪后定位：
  //    - 加载中 / 树被置空（切换中）→ 继续等，不 stop
  //    - 树就绪且定位成功（await expandTo 判定）→ stop
  //    - 兜底：15 秒内未定位成功（无树/慢网络加载失败）→ 超时 stop 防泄漏 + 提示
  const timeout = window.setTimeout(() => {
    stop()
    toast.info('未能定位到该知识点，请确认知识树已加载')
  }, 15000)
  const stop = watch([treeData, treeLoading], () => {
    if (treeLoading.value) return
    if (treeData.value.length === 0) return // 无树或加载中置空：继续等，由超时兜底
    void nextTick().then(() => {
      void treeCheckboxRef.value?.expandTo(tag.id).then((ok) => {
        if (ok) {
          stop()
          window.clearTimeout(timeout)
        }
      })
    })
  }, { immediate: true })
}

/** 面板外 Tag 移除：按 type 定位对应数组 */
function removeNode(id: string, type: TreeMode) {
  if (type === 'knowledge') {
    knowledgeNodeIds.value = knowledgeNodeIds.value.filter(x => x !== id)
  } else if (type === 'chapter') {
    chapterNodeIds.value = chapterNodeIds.value.filter(x => x !== id)
  } else {
    methodNodeIds.value = methodNodeIds.value.filter(x => x !== id)
  }
  clearFieldHighlight('knowledge_node')
  // 联动：被移除的节点若是主知识点，清空主知识点引用
  if (primaryKnowledgeNodeId.value === id) {
    primaryKnowledgeNodeId.value = null
  }
  // 手动移除的节点同步移出 AI 高亮
  if (aiHighlightIds.value.includes(id)) {
    aiHighlightIds.value = aiHighlightIds.value.filter(x => x !== id)
  }
}

/**
 * 切换主知识点：再次点击同一节点取消主知识点
 * 跨三组节点单选（chapter/knowledge/method 任选其一）
 */
function togglePrimary(id: string) {
  primaryKnowledgeNodeId.value = primaryKnowledgeNodeId.value === id ? null : id
  clearFieldHighlight('knowledge_node')
}

/**
 * 幽灵 ID 防护：当三组 NodeIds 变化（含树形多选框直接取消勾选）时，
 * 实时校验 primaryKnowledgeNodeId 是否仍存在于任一组中，否则置 null
 */
watch(
  [chapterNodeIds, knowledgeNodeIds, methodNodeIds],
  () => {
    if (!primaryKnowledgeNodeId.value) return
    const stillExists = [
      ...chapterNodeIds.value,
      ...knowledgeNodeIds.value,
      ...methodNodeIds.value,
    ].includes(primaryKnowledgeNodeId.value)
    if (!stillExists) {
      primaryKnowledgeNodeId.value = null
    }
  },
  { deep: true },
)

// ─── 侦听：activeTreeId 变化 → 自动加载树数据（与 KnowledgeTreeNav 对齐的安全网） ──
watch(activeTreeId, (id) => {
  if (id) loadTreeData()
})

// ─── 侦听：treeMode / stage / subject 变化 → 重新加载树 ──────────────
watch(
  [treeMode, () => props.form.stage, () => props.form.subject],
  async ([, newStage, newSubject], old) => {
    const stageChanged = old && old[1] !== newStage
    const subjectChanged = old && old[2] !== newSubject
    // 学段 / 学科变化：旧 key 快照三组勾选，新 key 命中缓存则瞬时恢复（无弹窗、无丢失）
    if (stageChanged || subjectChanged) {
      const cache = props.selectionCache
      if (cache) {
        const oldKey = `${old[2]}_${old[1]}`
        cache.set(oldKey, {
          chapter: [...chapterNodeIds.value],
          knowledge: [...knowledgeNodeIds.value],
          method: [...methodNodeIds.value],
        })
        const hit = cache.get(`${newSubject}_${newStage}`)
        chapterNodeIds.value = hit?.chapter ?? []
        knowledgeNodeIds.value = hit?.knowledge ?? []
        methodNodeIds.value = hit?.method ?? []
      } else {
        chapterNodeIds.value = []
        knowledgeNodeIds.value = []
        methodNodeIds.value = []
      }
      clearFieldHighlight('knowledge_node')
      // 高亮节点属于旧树语境，切换后一律清除
      aiHighlightIds.value = []
    }
    activeTreeId.value = ''
    treeData.value = []
    await loadTrees()
    // 注意：loadTreeData 由上方 watch(activeTreeId) 自动触发，避免重复调用
  },
)

// ─────────────────────────────────────────────────────────────────────
// 标签分类与限额
// ─────────────────────────────────────────────────────────────────────
const TAG_LIMITS: Record<TagCategory, number> = {
  core_competence: 3,
  method: 5,
  school: 1,
  scene: 3,
  error_prone: 2,
}

const allTagsMap = computed(() => {
  const m = new Map<string, Tag>()
  for (const t of props.methodTags) m.set(t.id, t)
  for (const t of props.competenceTags) m.set(t.id, t)
  for (const t of props.schoolTags) m.set(t.id, t)
  return m
})

const selectedTagsList = computed(() =>
  tagIds.value
    .map((id) => allTagsMap.value.get(id))
    .filter((t): t is Tag => !!t),
)

const selectedCompetenceTags = computed(() =>
  selectedTagsList.value.filter((t) => t.category === 'core_competence'),
)
const selectedMethodTags = computed(() =>
  selectedTagsList.value.filter((t) => t.category === 'method'),
)
const selectedSchoolTags = computed(() =>
  selectedTagsList.value.filter((t) => t.category === 'school'),
)

const topMethods = computed(() =>
  [...props.methodTags].sort((a, b) => b.use_count - a.use_count).slice(0, 8),
)
const topSchools = computed(() =>
  [...props.schoolTags].sort((a, b) => b.use_count - a.use_count).slice(0, 8),
)

function toggleTag(tag: Tag) {
  const idx = tagIds.value.indexOf(tag.id)
  if (idx >= 0) {
    tagIds.value.splice(idx, 1)
    return
  }
  const count = selectedTagsList.value.filter((t) => t.category === tag.category).length
  const limit = TAG_LIMITS[tag.category] ?? 99
  if (count >= limit) {
    toast.warning('已达到该类别最大可选择上限')
    return
  }
  tagIds.value.push(tag.id)
}

// ─────────────────────────────────────────────────────────────────────
// 标签搜索 / 创建
// ─────────────────────────────────────────────────────────────────────
interface SuggestState {
  query: string
  results: Tag[]
  loading: boolean
  timer: ReturnType<typeof setTimeout> | null
}

const suggestMethod = reactive<SuggestState>({ query: '', results: [], loading: false, timer: null })
const suggestSchool = reactive<SuggestState>({ query: '', results: [], loading: false, timer: null })

function onSuggestInput(state: SuggestState, category: TagCategory) {
  if (state.timer) clearTimeout(state.timer)
  const q = state.query.trim()
  if (!q) {
    state.results = []
    return
  }
  state.timer = setTimeout(async () => {
    state.loading = true
    try {
      const res = await tagsApi.suggest(q, category)
      state.results = res.data
    } catch {
      state.results = []
    } finally {
      state.loading = false
    }
  }, 200)
}

async function createNewTag(name: string, category: TagCategory, state: SuggestState) {
  try {
    const res = await tagsApi.create({ name, category })
    tagIds.value.push(res.data.id)
    toast.success(`已创建并选中标签「${name}」`)
    state.query = ''
    state.results = []
  } catch (e: any) {
    toast.error(e.response?.data?.error || '创建标签失败')
  }
}

// ─────────────────────────────────────────────────────────────────────
// 关联试卷（多选 + 搜索）
// ─────────────────────────────────────────────────────────────────────
const papers = ref<PaperBrief[]>([])
const papersLoading = ref(false)
const paperDropdownOpen = ref(false)
const paperSearch = ref('')
const paperTriggerRef = ref<HTMLElement | null>(null)
const paperPopoverRef = ref<HTMLElement | null>(null)

const filteredPapers = computed(() => {
  const q = paperSearch.value.trim().toLowerCase()
  if (!q) return papers.value
  return papers.value.filter((p) => p.title.toLowerCase().includes(q))
})

const selectedPapers = computed(() =>
  paperIds.value
    .map((id) => papers.value.find((p) => p.id === id))
    .filter((p): p is PaperBrief => !!p),
)

async function loadPapers() {
  papersLoading.value = true
  try {
    const res = await paperApi.listBrief()
    papers.value = res.data
  } catch (e: any) {
    toast.error(e.response?.data?.error || '加载试卷列表失败')
  } finally {
    papersLoading.value = false
  }
}

function togglePaperDropdown() {
  paperDropdownOpen.value = !paperDropdownOpen.value
  if (paperDropdownOpen.value) {
    paperSearch.value = ''
    nextTick(() => updatePaperPopoverPosition())
  }
}

function updatePaperPopoverPosition() {
  if (!paperTriggerRef.value) return
  const rect = paperTriggerRef.value.getBoundingClientRect()
  paperPopoverStyle.value = {
    top: `${rect.bottom + 4}px`,
    left: `${rect.left}px`,
    minWidth: `${rect.width}px`,
  }
}

const paperPopoverStyle = ref({ top: '0px', left: '0px', minWidth: '0px' })

function togglePaper(id: string) {
  const idx = paperIds.value.indexOf(id)
  if (idx >= 0) {
    paperIds.value.splice(idx, 1)
  } else {
    paperIds.value.push(id)
  }
}

function removePaper(id: string) {
  const idx = paperIds.value.indexOf(id)
  if (idx >= 0) paperIds.value.splice(idx, 1)
}

function onPaperClickOutside(e: MouseEvent) {
  const target = e.target as Node
  if (paperTriggerRef.value?.contains(target)) return
  if (paperPopoverRef.value?.contains(target)) return
  paperDropdownOpen.value = false
}

// ─────────────────────────────────────────────────────────────────────
// 基础属性选项（题型 / 学年 / 年级学期 / 考试类型）
// ─────────────────────────────────────────────────────────────────────
const typeOptions = [
  { label: '选择题', value: 'choice' },
  { label: '填空题', value: 'fill' },
  { label: '解答题', value: 'solution' },
]

// 学期（与 QuestionList.vue semesterOptions 对齐；字段名沿用 grade_semester 以兼容旧 metadata）
const gradeSemesterOptions = [
  { label: '上学期', value: '上学期' },
  { label: '下学期', value: '下学期' },
]

// ─────────────────────────────────────────────────────────────────────
// 来源 / 年份 / 省市 数据字典（与 QuestionList.vue 完全对齐）
// 长尾维度统一存入 questions.metadata(JSONB)
// ─────────────────────────────────────────────────────────────────────
// 来源（source_type）：去掉"全部"占位
const sourceTypeOptions = [
  '课前预习', '课堂例题', '随堂练习', '课后作业',
  '单元复习', '单元测试', '阶段检测', '期中', '期末',
  '高考真题', '高考模拟',
].map(v => ({ label: v, value: v }))

// 高考模拟子类型（sub_source_type）：仅当 source_type === '高考模拟' 时启用
const subSourceTypeOptions = [
  '一模', '二模', '三模', '模拟预测',
].map(v => ({ label: v, value: v }))

// 年份（year）：2020-2026
const yearOptions = ['2020', '2021', '2022', '2023', '2024', '2025', '2026']
  .map(v => ({ label: v, value: v }))

// 省份（region_province）
const regionOptions = ['北京', '上海', '浙江', '江苏', '广东', '湖北', '湖南', '四川', '山东']
  .map(v => ({ label: v, value: v }))

// 省份 → 城市级联字典（与 QuestionList.vue cityOptions 一致；其他省份用空数组兜底）
const cityOptionsMap: Record<string, string[]> = {
  '浙江': ['杭州市', '宁波市', '温州市', '绍兴市', '嘉兴市'],
  '江苏': ['南京市', '苏州市', '无锡市', '常州市', '南通市'],
  '广东': ['广州市', '深圳市', '珠海市', '佛山市', '东莞市'],
  '北京': ['东城区', '西城区', '海淀区', '朝阳区', '丰台区'],
  '上海': ['黄浦区', '徐汇区', '浦东新区', '静安区', '杨浦区'],
}

// 当前省份对应的市级选项（动态级联）
const currentCityOptions = computed(() => {
  const cities = cityOptionsMap[props.form.region_province]
  return cities ? cities.map(c => ({ label: c, value: c })) : []
})

// 是否显示城市下拉（仅当省份在 cityOptionsMap 中存在时）
const showCitySelect = computed(() => !!cityOptionsMap[props.form.region_province])

// 是否显示模拟类型下拉（仅当来源是"高考模拟"时）
const showSubSourceSelect = computed(() => props.form.source_type === '高考模拟')

// 切换省份时清空城市，避免悬挂脏数据
watch(
  () => props.form.region_province,
  () => {
    if (props.form.region_city && !cityOptionsMap[props.form.region_province]?.includes(props.form.region_city)) {
      props.form.region_city = ''
    }
  },
)

// 切换来源时清空模拟类型，避免悬挂脏数据
watch(
  () => props.form.source_type,
  (newVal) => {
    if (newVal !== '高考模拟' && props.form.sub_source_type) {
      props.form.sub_source_type = ''
    }
  },
)

// 难度星级：1-5 星 ↔ easy/medium/hard + difficulty_coefficient
const difficultyStars = computed<number>({
  get: () => {
    if (props.form.difficulty === 'easy') return props.form.difficulty_coefficient > 0.8 ? 1 : 2
    if (props.form.difficulty === 'medium') return 3
    return props.form.difficulty_coefficient < 0.3 ? 5 : 4
  },
  set: (v: number) => {
    props.form.difficulty_coefficient = [0.9, 0.75, 0.55, 0.35, 0.2][v - 1] ?? 0.55
    props.form.difficulty = v <= 2 ? 'easy' : v === 3 ? 'medium' : 'hard'
    clearFieldHighlight('difficulty')
  },
})

// ─────────────────────────────────────────────────────────────────────
// AI 智能打标
// ─────────────────────────────────────────────────────────────────────
const aiTagging = ref(false)

/** 拼接题干 + 选项 + 答案 + 解析为完整题目文本 */
function buildTaggingContent(): string {
  const parts: string[] = [props.form.stem || '']
  if (props.form.options?.length) {
    parts.push(props.form.options.map((o) => `${o.label}. ${o.content}`).join('\n'))
  }
  if (props.form.sub_answers?.length) {
    const ans = props.form.sub_answers.filter((s) => s.trim())
    if (ans.length) parts.push('参考答案：' + ans.join('；'))
  }
  if (props.form.solutions?.length) {
    const sol = props.form.solutions.filter((s) => s.trim())
    if (sol.length) parts.push('解析：' + sol.join('\n'))
  }
  return parts.filter(Boolean).join('\n\n')
}

/** 难度数值 1-5 → form 内部使用的字符串枚举 */
function difficultyNumToString(n: number | null): string {
  if (n == null) return 'medium'
  if (n <= 2) return 'easy'
  if (n === 3) return 'medium'
  return 'hard'
}

async function runAiTagging() {
  const content = buildTaggingContent()
  if (!content.trim()) {
    toast.warning('请先输入题干内容')
    return
  }
  aiTagging.value = true
  aiTaggingInProgress = true
  try {
    const res = await aiTaggingApi.tag({
      content,
      space_id: space.currentSpaceId || undefined,
    })
    const data = res.data
    const newAiFields = new Set<string>()

    // 知识点回填：三维合一分发 —— 按 tree_id → tree.kind 路由到对应数组
    if (data.knowledge_nodes?.length) {
      // 构建 tree_id → kind 映射（chapter/knowledge/ability）
      const treeKindMap = new Map<string, string>()
      for (const t of treeList.value) {
        treeKindMap.set(t.id, t.kind)
      }

      const chapterAiIds: string[] = []
      const knowledgeAiIds: string[] = []
      const methodAiIds: string[] = []

      for (const n of data.knowledge_nodes) {
        if (n.node_name) nodeNameMap.value.set(n.node_id, n.node_name)
        const kind = treeKindMap.get(n.tree_id)
        if (kind === 'chapter') chapterAiIds.push(n.node_id)
        else if (kind === 'ability') methodAiIds.push(n.node_id)
        else knowledgeAiIds.push(n.node_id) // 兜底归入知识点
      }

      // 记录新增 ID（用于 AI 高亮）
      const prevChapter = new Set(chapterNodeIds.value)
      const prevKnowledge = new Set(knowledgeNodeIds.value)
      const prevMethod = new Set(methodNodeIds.value)
      const added: string[] = []
      for (const id of chapterAiIds) if (!prevChapter.has(id)) added.push(id)
      for (const id of knowledgeAiIds) if (!prevKnowledge.has(id)) added.push(id)
      for (const id of methodAiIds) if (!prevMethod.has(id)) added.push(id)

      // 合并（并集，绝不覆盖用户已选）
      if (chapterAiIds.length) chapterNodeIds.value = [...new Set([...chapterNodeIds.value, ...chapterAiIds])]
      if (knowledgeAiIds.length) knowledgeNodeIds.value = [...new Set([...knowledgeNodeIds.value, ...knowledgeAiIds])]
      if (methodAiIds.length) methodNodeIds.value = [...new Set([...methodNodeIds.value, ...methodAiIds])]

      if (added.length > 0) {
        aiHighlightIds.value = [...new Set([...aiHighlightIds.value, ...added])]
      }
      newAiFields.add('knowledge_node')
    }

    // 核心素养 + 解题方法标签回填：与用户已选取并集，受 TAG_LIMITS 上限约束
    const aiTagIds = [
      ...(data.competency_tags ?? []).map(t => t.tag_id),
      ...(data.method_tags ?? []).map(t => t.tag_id),
    ]
    if (aiTagIds.length > 0) {
      tagIds.value = [...new Set([...tagIds.value, ...aiTagIds])]
      newAiFields.add('tag')
    }

    // 难度回填
    if (data.difficulty != null) {
      props.form.difficulty = difficultyNumToString(data.difficulty)
      const diffStars = data.difficulty
      props.form.difficulty_coefficient =
        [0.9, 0.75, 0.55, 0.35, 0.2][diffStars - 1] ?? 0.55
      newAiFields.add('difficulty')
    }

    // 题型回填
    if (data.question_type) {
      props.form.question_type = data.question_type as QuestionType
      newAiFields.add('question_type')
    }

    // 年级 / 认知层次回填（暂存到 aiGeneratedFields 标记位，由父组件决定是否使用）
    if (data.grade_level) newAiFields.add(`grade_level:${data.grade_level}`)
    if (data.cognitive_level) newAiFields.add(`cognitive_level:${data.cognitive_level}`)

    if (data.unmatched_knowledge_points?.length) {
      toast.info(`AI 识别到 ${data.unmatched_knowledge_points.length} 个未匹配知识点，请手动确认`)
    }

    aiGeneratedFields.value = newAiFields
    toast.success(`AI 打标完成，已回填 ${newAiFields.size} 个字段`)
    // 等待 watch 触发完毕后再放开标记，防止清掉刚加的高亮
    await nextTick()
  } catch (e: any) {
    toast.error(e.response?.data?.error || 'AI 打标失败，请稍后重试')
  } finally {
    aiTagging.value = false
    aiTaggingInProgress = false
  }
}

// ─────────────────────────────────────────────────────────────────────
// 手动编辑 → 取消 AI 高亮
// ─────────────────────────────────────────────────────────────────────
// AI 回填进行中的标志：避免 watch 在 AI 设置字段时立刻清掉刚加的高亮
let aiTaggingInProgress = false

function clearFieldHighlight(field: string) {
  if (aiTaggingInProgress) return
  if (!aiGeneratedFields.value.has(field)) return
  const next = new Set(aiGeneratedFields.value)
  next.delete(field)
  aiGeneratedFields.value = next
}

// 知识点手动变更 → 取消高亮
watch(knowledgeNodeIds, () => {
  clearFieldHighlight('knowledge_node')
})

// 基础属性手动变更 → 取消对应高亮
watch(
  () => props.form.question_type,
  () => clearFieldHighlight('question_type'),
)
watch(
  () => props.form.grade_semester,
  () => clearFieldHighlight('grade_semester'),
)

// 暴露给父组件：当 form 字段被用户手动修改时，可调用此方法清除对应高亮
defineExpose({ clearFieldHighlight })

// ─────────────────────────────────────────────────────────────────────
// 折叠/展开
// ─────────────────────────────────────────────────────────────────────
function toggleCollapsed() {
  collapsed.value = !collapsed.value
}

// ─────────────────────────────────────────────────────────────────────
// 初始化
// ─────────────────────────────────────────────────────────────────────
// 监听父组件传入的 initialNodeNames：编辑场景下 loadQuestion 完成后会异步更新
// 用 immediate 确保首次挂载也能捕获同步传入的值
watch(
  () => props.initialNodeNames,
  (val) => {
    if (!val) return
    for (const [id, name] of Object.entries(val)) {
      nodeNameMap.value.set(id, name)
    }
  },
  { immediate: true },
)

onMounted(() => {
  // 预加载当前 stage/subject/mode 对应的树（默认知识点模式）
  loadTrees().then(loadTreeData)

  loadPapers()
  document.addEventListener('click', onPaperClickOutside)
})

onBeforeUnmount(() => {
  document.removeEventListener('click', onPaperClickOutside)
})
</script>

<template>
  <div class="asp-wrapper" :class="{ 'is-collapsed': collapsed }">
    <aside class="attr-side-panel">
      <!-- ===== 顶部：标题 + AI 智能打标按钮 ===== -->
      <header class="asp-header">
        <div class="asp-title">
          <AppIcon name="sliders" :size="15" />
          <span>题目属性</span>
        </div>
        <div class="asp-header-actions">
          <AppButton
            variant="primary"
            size="sm"
            :loading="aiTagging"
            :disabled="aiTagging"
            @click="runAiTagging"
          >
            <AppIcon name="sparkles" :size="14" />
            <span>{{ aiTagging ? '打标中…' : 'AI 智能打标' }}</span>
          </AppButton>
        </div>
      </header>

    <!-- ===== 滚动主体 ===== -->
    <div class="asp-body">
      <!-- 基础属性：5 行双列网格（第一行：学段 | 学科） -->
      <section class="asp-section asp-section-meta">
        <div class="asp-meta-grid">
          <!-- 第一行左：学段（绑定 form.stage） -->
          <div class="asp-meta-cell">
            <AppSelect
              :model-value="props.form.stage"
              :options="stageOptions"
              placeholder="学段"
              class="asp-meta-select"
              @update:model-value="(v: string | undefined) => { props.form.stage = (v as 'junior' | 'senior') || 'senior' }"
            />
          </div>

          <!-- 第一行右：学科（绑定 form.subject） -->
          <div class="asp-meta-cell">
            <AppSelect
              :model-value="props.form.subject"
              :options="subjectOptions"
              placeholder="学科"
              class="asp-meta-select"
              @update:model-value="(v: string | undefined) => { props.form.subject = (v as 'math' | 'physics') || 'math' }"
            />
          </div>

          <!-- 第二行左：题型 -->
          <div
            class="asp-meta-cell"
            :class="{ 'ai-highlight': aiGeneratedFields.has('question_type') }"
          >
            <AppSelect
              :model-value="props.form.question_type"
              :options="typeOptions"
              placeholder="题型"
              class="asp-meta-select"
              @update:model-value="(v: string | undefined) => { props.form.question_type = v ?? ''; clearFieldHighlight('question_type') }"
            />
          </div>

          <!-- 第二行右：难度星级 -->
          <div
            class="asp-meta-cell asp-meta-cell-stars"
            :class="{ 'ai-highlight': aiGeneratedFields.has('difficulty') }"
          >
            <button
              v-for="n in 5"
              :key="n"
              type="button"
              class="asp-star"
              :class="{ active: difficultyStars >= n }"
              @click="difficultyStars = n"
            >
              <AppIcon name="star" :size="13" />
            </button>
          </div>

          <!-- 第三行左：年级（级联：根据学段动态计算） -->
          <div class="asp-meta-cell">
            <AppSelect
              :model-value="props.form.grade || undefined"
              :options="gradeOptions"
              placeholder="年级"
              clearable
              class="asp-meta-select"
              @update:model-value="(v: string | undefined) => { props.form.grade = v ?? '' }"
            />
          </div>

          <!-- 第三行右：学期 -->
          <div
            class="asp-meta-cell"
            :class="{ 'ai-highlight': aiGeneratedFields.has('grade_semester') }"
          >
            <AppSelect
              :model-value="props.form.grade_semester || undefined"
              :options="gradeSemesterOptions"
              placeholder="学期"
              clearable
              class="asp-meta-select"
              @update:model-value="(v: string | undefined) => { props.form.grade_semester = v ?? ''; clearFieldHighlight('grade_semester') }"
            />
          </div>

          <!-- 第四行左：年份 -->
          <div class="asp-meta-cell">
            <AppSelect
              :model-value="props.form.year || undefined"
              :options="yearOptions"
              placeholder="年份"
              clearable
              class="asp-meta-select"
              @update:model-value="(v: string | undefined) => { props.form.year = v ?? '' }"
            />
          </div>

          <!-- 第四行左：省份 -->
          <div class="asp-meta-cell">
            <AppSelect
              :model-value="props.form.region_province || undefined"
              :options="regionOptions"
              placeholder="省份"
              clearable
              class="asp-meta-select"
              @update:model-value="(v: string | undefined) => { props.form.region_province = v ?? '' }"
            />
          </div>

          <!-- 第四行右：市区（级联：未选省份时 disabled 占位） -->
          <div class="asp-meta-cell">
            <AppSelect
              :model-value="props.form.region_city || undefined"
              :options="currentCityOptions"
              :placeholder="showCitySelect ? '市/区' : '请先选省份'"
              :disabled="!showCitySelect"
              clearable
              class="asp-meta-select"
              @update:model-value="(v: string | undefined) => { props.form.region_city = v ?? '' }"
            />
          </div>

          <!-- 第五行左：来源类型 -->
          <div class="asp-meta-cell">
            <AppSelect
              :model-value="props.form.source_type || undefined"
              :options="sourceTypeOptions"
              placeholder="来源类型"
              clearable
              class="asp-meta-select"
              @update:model-value="(v: string | undefined) => { props.form.source_type = v ?? '' }"
            />
          </div>

          <!-- 第五行右：模拟类型（级联：未选高考模拟时 disabled 占位） -->
          <div class="asp-meta-cell">
            <AppSelect
              :model-value="props.form.sub_source_type || undefined"
              :options="subSourceTypeOptions"
              :placeholder="showSubSourceSelect ? '模拟类型' : '需选高考模拟'"
              :disabled="!showSubSourceSelect"
              clearable
              class="asp-meta-select"
              @update:model-value="(v: string | undefined) => { props.form.sub_source_type = v ?? '' }"
            />
          </div>
        </div>
      </section>

      <!-- 知识树标注（可折叠内联面板：收起态展示摘要，展开态实时勾选） -->
      <section
        class="asp-section"
        :class="{ 'ai-highlight': aiGeneratedFields.has('knowledge_node') }"
      >
        <div class="asp-section-head">
          <label class="asp-label">知识树标注</label>
          <span class="asp-counter">{{ totalSelectedNodes }}</span>
        </div>
        <!-- 已选节点 Tag 平铺（融合三组，折叠为最高层已选节点） -->
        <div v-if="allSelectedNodes.length" class="asp-node-chips">
          <span
            v-for="t in allSelectedNodes"
            :key="t.id"
            class="asp-node-chip"
            :class="['is-' + t.type, { 'is-primary': t.isPrimary }]"
            :title="t.path"
            @dblclick="locateTreeNode(t)"
          >
            <span class="asp-node-chip-type">{{ t.typeLabel }}</span>
            <span class="asp-node-chip-name">{{ t.name }}</span>
            <span v-if="t.hiddenCount > 0" class="asp-node-chip-more">+{{ t.hiddenCount }}</span>
            <!-- 主知识点星标按钮（点击切换：设为/取消主知识点） -->
            <button
              type="button"
              class="asp-node-chip-star"
              :class="{ active: t.isPrimary }"
              :title="t.isPrimary ? '取消主知识点' : '设为主知识点'"
              @click.stop="togglePrimary(t.id)"
              @dblclick.stop
            >
              <AppIcon name="star" :size="11" />
            </button>
            <button
              type="button"
              class="asp-node-chip-x"
              :title="`移除${t.typeLabel}`"
              @click.stop="removeNode(t.id, t.type)"
              @dblclick.stop
            >
              <AppIcon name="x" :size="10" />
            </button>
          </span>
        </div>
        <!-- 展开按钮（收起态：点击原地展开 Tabs + 树） -->
        <button
          v-if="!treeExpanded"
          type="button"
          class="asp-tree-toggle"
          @click="treeExpanded = true"
        >
          <AppIcon name="chevron-down" :size="14" />
          <span>展开知识树</span>
        </button>
        <!-- 可折叠内容容器（展开态：Tabs + 树 + 收起按钮，grid-rows 过渡动画，高度自然撑开） -->
        <div class="asp-tree-collapse" :class="{ 'is-expanded': treeExpanded }">
          <div class="asp-tree-collapse-inner">
            <div class="asp-tree-tabs" role="tablist" aria-label="标注模式">
              <button
                v-for="m in MODES"
                :key="m.key"
                type="button"
                class="asp-tree-tab"
                :class="{ active: treeMode === m.key }"
                :disabled="m.key === 'method' && !methodTreeAvailable"
                :title="m.key === 'method' && !methodTreeAvailable ? '当前学段/学科暂无解题方法树' : undefined"
                role="tab"
                :aria-selected="treeMode === m.key"
                @click="setMode(m.key)"
              >
                {{ m.label }}
                <span v-if="modeCounts[m.key] > 0" class="asp-tab-badge">{{ modeCounts[m.key] }}</span>
              </button>
            </div>
            <div class="asp-tree-inline">
              <div v-if="treeLoading" class="asp-tree-empty">加载中…</div>
              <div v-else-if="treeData.length === 0" class="asp-tree-empty">{{ emptyHint }}</div>
              <KnowledgeTreeCheckbox
                v-else
                ref="treeCheckboxRef"
                :nodes="treeData"
                v-model="currentModeSelectedIds"
                :highlight-ids="aiHighlightIds"
              />
            </div>
            <button
              type="button"
              class="asp-tree-toggle asp-tree-toggle-collapse"
              @click="treeExpanded = false"
            >
              <AppIcon name="chevron-down" :size="14" class="asp-tree-toggle-icon-up" />
              <span>收起 / 完成</span>
            </button>
          </div>
        </div>
      </section>

      <!-- 核心素养 -->
      <section class="asp-section">
        <div class="asp-section-head">
          <label class="asp-label">核心素养</label>
          <span class="asp-counter">{{ selectedCompetenceTags.length }}/3</span>
        </div>
        <div v-if="competenceTags.length === 0" class="asp-empty">暂无可选素养</div>
        <div v-else class="asp-chip-grid">
          <button
            v-for="t in competenceTags"
            :key="t.id"
            type="button"
            class="asp-chip"
            :class="{ active: tagIds.includes(t.id) }"
            @click="toggleTag(t)"
          >
            <span v-if="tagIds.includes(t.id)" class="asp-chip-check">✓</span>
            <span>{{ t.name }}</span>
          </button>
        </div>
      </section>

      <!-- 解题方法 -->
      <section class="asp-section">
        <div class="asp-section-head">
          <label class="asp-label">解题方法</label>
          <span class="asp-counter">{{ selectedMethodTags.length }}/5</span>
        </div>
        <div class="asp-typeahead">
          <input
            v-model="suggestMethod.query"
            class="asp-input"
            placeholder="搜索或创建方法标签…"
            @input="onSuggestInput(suggestMethod, 'method')"
          />
          <div v-if="suggestMethod.results.length" class="asp-popover">
            <button
              v-for="t in suggestMethod.results"
              :key="t.id"
              type="button"
              class="asp-popover-item"
              @click="toggleTag(t); suggestMethod.query = ''; suggestMethod.results = []"
            >
              <span>{{ t.name }}</span>
              <span class="asp-popover-count">{{ t.use_count }} 次</span>
            </button>
          </div>
          <button
            v-if="suggestMethod.query.trim() && !suggestMethod.results.some(t => t.name === suggestMethod.query.trim())"
            type="button"
            class="asp-create-btn"
            @click="createNewTag(suggestMethod.query.trim(), 'method', suggestMethod)"
          >
            <AppIcon name="plus" :size="12" />
            <span>创建「{{ suggestMethod.query.trim() }}」</span>
          </button>
        </div>
        <div v-if="topMethods.length" class="asp-chip-grid">
          <button
            v-for="t in topMethods"
            :key="t.id"
            type="button"
            class="asp-chip"
            :class="{ active: tagIds.includes(t.id) }"
            @click="toggleTag(t)"
          >
            <span v-if="tagIds.includes(t.id)" class="asp-chip-check">✓</span>
            <span>{{ t.name }}</span>
          </button>
        </div>
      </section>

      <!-- 学校来源 -->
      <section class="asp-section">
        <div class="asp-section-head">
          <label class="asp-label">学校来源</label>
          <span class="asp-counter">{{ selectedSchoolTags.length }}/1</span>
        </div>
        <div class="asp-typeahead">
          <input
            v-model="suggestSchool.query"
            class="asp-input"
            placeholder="搜索或创建学校标签…"
            @input="onSuggestInput(suggestSchool, 'school')"
          />
          <div v-if="suggestSchool.results.length" class="asp-popover">
            <button
              v-for="t in suggestSchool.results"
              :key="t.id"
              type="button"
              class="asp-popover-item"
              @click="toggleTag(t); suggestSchool.query = ''; suggestSchool.results = []"
            >
              <span>{{ t.name }}</span>
              <span class="asp-popover-count">{{ t.use_count }} 次</span>
            </button>
          </div>
          <button
            v-if="suggestSchool.query.trim() && !suggestSchool.results.some(t => t.name === suggestSchool.query.trim())"
            type="button"
            class="asp-create-btn"
            @click="createNewTag(suggestSchool.query.trim(), 'school', suggestSchool)"
          >
            <AppIcon name="plus" :size="12" />
            <span>创建「{{ suggestSchool.query.trim() }}」</span>
          </button>
        </div>
        <div v-if="topSchools.length" class="asp-chip-grid">
          <button
            v-for="t in topSchools"
            :key="t.id"
            type="button"
            class="asp-chip"
            :class="{ active: tagIds.includes(t.id) }"
            @click="toggleTag(t)"
          >
            <span v-if="tagIds.includes(t.id)" class="asp-chip-check">✓</span>
            <span>{{ t.name }}</span>
          </button>
        </div>
      </section>

      <!-- 所属/关联试卷（多选 + 搜索） -->
      <section class="asp-section">
        <div class="asp-section-head">
          <label class="asp-label">所属/关联试卷</label>
          <span class="asp-counter">{{ paperIds.length }}</span>
        </div>
        <!-- 已选试卷 chips -->
        <div v-if="selectedPapers.length" class="asp-paper-chips">
          <span
            v-for="p in selectedPapers"
            :key="p.id"
            class="asp-paper-chip"
          >
            <span class="asp-paper-chip-name">{{ p.title }}</span>
            <button
              type="button"
              class="asp-paper-chip-x"
              @click="removePaper(p.id)"
            >
              <AppIcon name="x" :size="10" />
            </button>
          </span>
        </div>
        <!-- 触发输入框 -->
        <div
          ref="paperTriggerRef"
          class="asp-paper-trigger"
          :class="{ 'is-open': paperDropdownOpen }"
          @click="togglePaperDropdown"
        >
          <span v-if="paperIds.length === 0" class="asp-paper-placeholder">
            选择试卷…
          </span>
          <span v-else class="asp-paper-summary">
            已选 {{ paperIds.length }} 份试卷
          </span>
          <AppIcon
            name="chevron-down"
            :size="14"
            class="asp-paper-caret"
            :class="{ 'is-open': paperDropdownOpen }"
          />
        </div>
        <!-- 下拉浮层（Teleport 到 body 避免 overflow 裁剪） -->
        <Teleport to="body">
          <div
            v-if="paperDropdownOpen"
            ref="paperPopoverRef"
            class="asp-paper-popover"
            :style="paperPopoverStyle"
          >
            <div class="asp-paper-search">
              <input
                v-model="paperSearch"
                class="asp-paper-search-input"
                placeholder="搜索试卷标题…"
                @click.stop
              />
            </div>
            <div v-if="papersLoading" class="asp-paper-empty">加载中…</div>
            <div v-else-if="filteredPapers.length === 0" class="asp-paper-empty">
              {{ paperSearch.trim() ? '无匹配试卷' : '暂无试卷' }}
            </div>
            <div v-else class="asp-paper-list">
              <button
                v-for="p in filteredPapers"
                :key="p.id"
                type="button"
                class="asp-paper-option"
                :class="{ active: paperIds.includes(p.id) }"
                @click.stop="togglePaper(p.id)"
              >
                <span class="asp-paper-check">
                  <AppIcon v-if="paperIds.includes(p.id)" name="check" :size="12" />
                </span>
                <span class="asp-paper-title">{{ p.title }}</span>
              </button>
            </div>
          </div>
        </Teleport>
      </section>
    </div>
    </aside>

    <!-- 左侧边缘把手 Toggle：与 KnowledgeTreeNav 右侧把手风格相同（尺寸、动画、着色统一） -->
    <button
      type="button"
      class="asp-edge-handle"
      :class="{ 'is-collapsed': collapsed }"
      :title="collapsed ? '展开属性面板' : '收起属性面板'"
      :aria-label="collapsed ? '展开属性面板' : '收起属性面板'"
      @click="toggleCollapsed"
    >
      <AppIcon
        name="chevron-right"
        class="toggle-chevron"
        :size="14"
      />
      <span class="toggle-tooltip">{{ collapsed ? '展开属性' : '收起属性' }}</span>
    </button>

  </div>
</template>

<style scoped>
/* ===== 外层 wrapper：负责宽度过渡，承载左侧把手按钮 ===== */
.asp-wrapper {
  position: relative;
  flex-shrink: 0;
  height: 100%;
  width: clamp(260px, 25vw, 320px);
  /* !important 覆盖父组件 .interactive-column 的 overflow:hidden 和 opacity:0.7。
     opacity<1 会创建独立 stacking context，导致 z-index 跨列失效，把手被预览列遮挡。
     折叠后必须 overflow:visible 让 left:-24px 的把手不被裁切。 */
  overflow: visible !important;
  opacity: 1 !important;
  transition: width 0.3s cubic-bezier(0.4, 0, 0.2, 1);
}

.asp-wrapper.is-collapsed {
  width: 0;
}

/* ===== 实际面板：填满 wrapper，折叠时透明 + 禁用交互 ===== */
.attr-side-panel {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--bg-card);
  border-radius: var(--radius-lg);
  border: 1px solid var(--border-color);
  box-shadow: var(--shadow-sm);
  overflow: hidden;
  transition: opacity 0.25s ease;
}

.asp-wrapper.is-collapsed .attr-side-panel {
  opacity: 0;
  pointer-events: none;
}

[data-theme='dark'] .attr-side-panel {
  border-color: #3a3a3c;
  box-shadow: none;
}

/* ===== 左侧边缘把手（Handle）Toggle：与 KnowledgeTreeNav 保持相同的精细化胶囊把手风格 ===== */
.asp-edge-handle {
  position: absolute;
  top: 50%;
  left: -11px;
  transform: translateY(-50%);
  width: 22px;
  height: 38px;
  border-radius: 19px;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  box-shadow: 0 2px 6px rgba(0, 0, 0, 0.06);
  color: var(--text-muted);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  /* 局部层级：仅浮过预览列（与 KnowledgeTreeNav 把手一致）。
     不可用高 z-index（如 9999）— 会穿透全局弹窗遮罩(.modal-overlay: 2000) */
  z-index: 10;
  transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
  outline: none;
}

/* Icon 旋转效果 */
.toggle-chevron {
  transition: transform 0.25s cubic-bezier(0.4, 0, 0.2, 1), color 0.15s ease;
}

.asp-edge-handle.is-collapsed .toggle-chevron {
  transform: rotate(180deg);
}

/* Hover 浮跃与色值高亮（完全贴合系统主题变量） */
.asp-edge-handle:hover {
  background: var(--bg-hover);
  color: var(--accent);
  border-color: var(--accent-light);
  box-shadow: 0 3px 10px rgba(0, 0, 0, 0.08);
  transform: translateY(-50%) scale(1.05);
}

/* Active 弹簧按压反馈 */
.asp-edge-handle:active {
  transform: translateY(-50%) scale(0.95);
}

/* 侧栏折叠时 handle 的定位适配 */
.asp-wrapper.is-collapsed .asp-edge-handle {
  left: -24px;
  box-shadow: 0 3px 10px rgba(0, 0, 0, 0.12);
}

/* 悬浮微型 Prompt Tooltip */
.toggle-tooltip {
  position: absolute;
  right: calc(100% + 6px);
  top: 50%;
  transform: translateY(-50%) translateX(4px);
  white-space: nowrap;
  background: var(--text-primary);
  color: var(--bg-card);
  font-size: 11px;
  font-weight: 500;
  padding: 3px 7px;
  border-radius: 4px;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
  pointer-events: none;
  opacity: 0;
  visibility: hidden;
  transition: all 0.18s cubic-bezier(0.4, 0, 0.2, 1);
}

.asp-edge-handle:hover .toggle-tooltip {
  opacity: 1;
  visibility: visible;
  transform: translateY(-50%) translateX(0);
}

/* ===== 顶部 ===== */
.asp-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 16px;
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
  gap: 8px;
}

.asp-title {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 13.5px;
  font-weight: 650;
  color: var(--text-primary);
  letter-spacing: -0.01em;
}

.asp-title :deep(.app-icon) {
  color: var(--text-secondary);
}

.asp-header-actions {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

/* ===== 滚动主体 ===== */
.asp-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 20px;
}

/* ===== 区块 ===== */
.asp-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 8px;
  border-radius: var(--radius-md);
  transition: box-shadow 0.4s ease;
}

/* ===== 基础属性区块：5 行双列极简扁平化栅格 ===== */
.asp-section-meta {
  padding: 4px 4px 16px;
  border-bottom: 1px solid var(--border-color);
}

.asp-meta-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 6px 6px;
}

/* cell 纯容器：无 padding/边框，让 AppSelect 自身承担"标签按钮"视觉 */
.asp-meta-cell {
  display: flex;
  align-items: center;
  min-width: 0;
  border-radius: 6px;
  transition: box-shadow 0.4s ease;
}

.asp-meta-select {
  width: 100%;
}

.asp-meta-cell :deep(.app-select-wrapper) {
  width: 100%;
  min-width: 0;
}

/* ===== 极简扁平化：淡灰背景 + 无 border + 6px 圆角 + 紧凑 padding ===== */
.asp-meta-cell :deep(.app-select-trigger) {
  width: 100%;
  min-width: 0;
  padding: 5px 10px;
  min-height: 32px;
  font-size: 12.5px;
  border: none;
  border-radius: 6px;
  background: var(--bg-input);
  transition: background 0.15s ease, box-shadow 0.15s ease;
}

.asp-meta-cell :deep(.app-select-trigger:hover:not(.open)) {
  background: var(--bg-hover);
}

.asp-meta-cell :deep(.app-select-trigger.open) {
  background: var(--bg-card);
  box-shadow: 0 0 0 2px var(--accent-light);
}

/* disabled 占位态：更淡的背景 + 不可点击 */
.asp-meta-cell :deep(.app-select-wrapper.disabled) .app-select-trigger {
  opacity: 0.55;
  cursor: not-allowed;
  background: var(--bg-input);
}

/* ===== 难度星级 cell：与 AppSelect 同款的"标签按钮"容器 ===== */
.asp-meta-cell-stars {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 2px;
  padding: 5px 8px;
  min-height: 32px;
  border-radius: 6px;
  background: var(--bg-input);
}

.asp-star {
  color: var(--border-strong, #d1d1d6);
  background: none;
  border: none;
  cursor: pointer;
  padding: 3px;
  display: inline-flex;
  transition: transform 0.15s ease, color 0.2s ease;
}

.asp-star :deep(svg),
.asp-star svg {
  pointer-events: none;
}

.asp-star:hover {
  transform: scale(1.15);
}

.asp-star.active {
  color: var(--star-color, #ff9500);
}

.asp-star.active :deep(svg),
.asp-star.active svg {
  color: var(--star-color, #ff9500) !important;
}

.asp-section-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.asp-label {
  font-size: 12.5px;
  font-weight: 650;
  color: var(--text-secondary);
  letter-spacing: 0.01em;
}

.asp-counter {
  font-size: 11px;
  color: var(--text-muted);
  font-weight: 500;
}

.asp-empty {
  font-size: 12px;
  color: var(--text-muted);
  padding: 6px 0;
}

/* ===== 输入框 ===== */
.asp-input {
  width: 100%;
  height: 32px;
  padding: 0 10px;
  border-radius: 8px;
  border: 1px solid var(--border-color);
  background: var(--bg-input);
  color: var(--text-primary);
  font-size: 12.5px;
  outline: none;
  box-sizing: border-box;
  transition: border-color 0.2s;
}

.asp-input:focus {
  border-color: var(--accent);
  background: var(--bg-card);
}

.asp-input::placeholder {
  color: var(--text-muted);
}

/* ===== Typeahead Popover ===== */
.asp-typeahead {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.asp-popover {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  right: 0;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: 8px;
  box-shadow: var(--shadow-md);
  z-index: 50;
  max-height: 200px;
  overflow-y: auto;
  padding: 4px;
}

.asp-popover-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
  padding: 7px 10px;
  border: none;
  background: transparent;
  border-radius: 6px;
  font-size: 12.5px;
  color: var(--text-primary);
  cursor: pointer;
  transition: background 0.15s;
  text-align: left;
}

.asp-popover-item:hover {
  background: var(--bg-hover);
}

.asp-popover-count {
  font-size: 11px;
  color: var(--text-muted);
}

.asp-create-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 5px 10px;
  border: 1px dashed var(--accent);
  border-radius: 6px;
  background: var(--accent-light);
  color: var(--accent);
  font-size: 11.5px;
  font-weight: 600;
  cursor: pointer;
  align-self: flex-start;
  transition: all 0.2s;
}

.asp-create-btn:hover {
  background: var(--accent);
  color: #fff;
}

/* ===== 标签 Chip 网格 ===== */
.asp-chip-grid {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.asp-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 4px 10px;
  border-radius: 9999px;
  border: 1px solid var(--border-color);
  background: var(--bg-input);
  color: var(--text-secondary);
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.18s ease;
  white-space: nowrap;
}

.asp-chip:hover {
  border-color: var(--accent);
  color: var(--accent);
  background: var(--accent-light);
}

.asp-chip.active {
  background: var(--accent);
  border-color: var(--accent);
  color: #fff;
  font-weight: 600;
}

.asp-chip-check {
  font-size: 10px;
  font-weight: 700;
  display: inline-flex;
  align-items: center;
}

/* ===== 知识树标注：已选节点 Tag + 添加按钮 ===== */
.asp-node-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.asp-node-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 3px 4px 3px 6px;
  border-radius: 9999px;
  background: var(--accent-light);
  border: 1px solid var(--accent-light);
  color: var(--accent);
  font-size: 11.5px;
  font-weight: 500;
  max-width: 100%;
  transition: background 0.15s ease, border-color 0.15s ease;
}

/* 按 type 区分色调（章节/知识点/方法） */
.asp-node-chip.is-chapter {
  background: var(--bg-active);
  border-color: var(--border-strong, #d1d1d6);
  color: var(--text-secondary);
}

.asp-node-chip.is-knowledge {
  background: var(--accent-light);
  border-color: var(--accent-light);
  color: var(--accent);
}

.asp-node-chip.is-method {
  background: var(--purple-light, #f3e8ff);
  border-color: var(--purple-light, #f3e8ff);
  color: var(--purple, #8b5cf6);
}

.asp-node-chip-type {
  font-size: 9.5px;
  font-weight: 600;
  padding: 1px 4px;
  border-radius: 9999px;
  background: rgba(255, 255, 255, 0.5);
  line-height: 1.3;
  white-space: nowrap; /* 防止"知识点/方法"前缀被挤成两行 */
  flex-shrink: 0;
}

[data-theme='dark'] .asp-node-chip-type {
  background: rgba(0, 0, 0, 0.2);
}

.asp-node-chip-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 120px;
}

/* 折叠代管的下级已选数量（如「集合的概念 +5」） */
.asp-node-chip-more {
  flex-shrink: 0;
  padding: 0 4px;
  border-radius: 9999px;
  background: rgba(0, 0, 0, 0.08);
  font-size: 10px;
  font-weight: 600;
  line-height: 1.6;
}

[data-theme='dark'] .asp-node-chip-more {
  background: rgba(255, 255, 255, 0.12);
}

.asp-node-chip-x {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border: none;
  background: transparent;
  color: inherit;
  cursor: pointer;
  border-radius: 50%;
  padding: 0;
  flex-shrink: 0;
  opacity: 0.7;
  transition: opacity 0.15s, background 0.15s;
}

.asp-node-chip-x:hover {
  opacity: 1;
  background: rgba(0, 0, 0, 0.1);
}

/* ===== 主知识点星标按钮 ===== */
.asp-node-chip-star {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border: none;
  background: transparent;
  color: var(--text-muted, #888);
  cursor: pointer;
  border-radius: 50%;
  padding: 0;
  flex-shrink: 0;
  opacity: 0.55;
  transition: opacity 0.15s, color 0.15s, transform 0.15s;
}

.asp-node-chip-star:hover {
  opacity: 1;
  color: #ff9500;
  transform: scale(1.15);
}

.asp-node-chip-star.active {
  opacity: 1;
  color: #ff9500;
}

.asp-node-chip-star.active :deep(svg),
.asp-node-chip-star.active svg {
  color: #ff9500 !important;
  fill: #ff9500;
}

/* 主知识点 chip 整体描金高亮 */
.asp-node-chip.is-primary {
  box-shadow: 0 0 0 1.5px #ff9500;
  background: rgba(255, 149, 0, 0.08);
}

[data-theme='dark'] .asp-node-chip-star {
  color: var(--text-muted, #aaa);
}

[data-theme='dark'] .asp-node-chip-star:hover,
[data-theme='dark'] .asp-node-chip-star.active {
  color: #ffaa33;
}

[data-theme='dark'] .asp-node-chip.is-primary {
  box-shadow: 0 0 0 1.5px #ffaa33;
  background: rgba(255, 170, 51, 0.12);
}

/* ===== AI 高亮动画（与 QuestionEdit.vue 的 .ai-highlight 一致） ===== */
@keyframes asp-ai-breathe {
  0%, 100% {
    box-shadow: 0 0 0 2px var(--purple);
  }
  50% {
    box-shadow: 0 0 8px 2px var(--purple-light);
  }
}

.asp-section.ai-highlight,
.asp-meta-cell.ai-highlight {
  animation: asp-ai-breathe 2s ease-in-out 3;
  border-radius: var(--radius-sm);
}

/* ===== 移动端：宽度自适应，但仍保持纵向栈 ===== */
@media (max-width: 900px) {
  .attr-side-panel {
    width: 100%;
    height: auto;
    max-height: 480px;
  }
}

/* ===== 关联试卷多选 ===== */
.asp-paper-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.asp-paper-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 3px 4px 3px 8px;
  border-radius: 9999px;
  background: var(--accent-light);
  border: 1px solid var(--accent-light);
  color: var(--accent);
  font-size: 11.5px;
  font-weight: 500;
  max-width: 100%;
}

.asp-paper-chip-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 140px;
}

.asp-paper-chip-x {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border: none;
  background: transparent;
  color: var(--accent);
  cursor: pointer;
  border-radius: 50%;
  padding: 0;
  flex-shrink: 0;
  transition: background 0.15s;
}

.asp-paper-chip-x:hover {
  background: var(--accent);
  color: #fff;
}

.asp-paper-trigger {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 32px;
  padding: 0 10px;
  border-radius: 8px;
  border: 1px solid var(--border-color);
  background: var(--bg-input);
  cursor: pointer;
  box-sizing: border-box;
  transition: border-color 0.2s;
}

.asp-paper-trigger:hover {
  border-color: var(--accent);
}

.asp-paper-trigger.is-open {
  border-color: var(--accent);
  background: var(--bg-card);
}

.asp-paper-placeholder {
  font-size: 12.5px;
  color: var(--text-muted);
}

.asp-paper-summary {
  font-size: 12.5px;
  color: var(--text-primary);
  font-weight: 500;
}

.asp-paper-caret {
  color: var(--text-muted);
  transition: transform 0.2s;
  flex-shrink: 0;
}

.asp-paper-caret.is-open {
  transform: rotate(180deg);
}

/* 下拉浮层（Teleport 到 body） */
.asp-paper-popover {
  position: fixed;
  z-index: 9999;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: 8px;
  box-shadow: var(--shadow-md);
  max-height: 280px;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.asp-paper-search {
  padding: 8px;
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
}

.asp-paper-search-input {
  width: 100%;
  height: 28px;
  padding: 0 8px;
  border-radius: 6px;
  border: 1px solid var(--border-color);
  background: var(--bg-input);
  color: var(--text-primary);
  font-size: 12.5px;
  outline: none;
  box-sizing: border-box;
}

.asp-paper-search-input:focus {
  border-color: var(--accent);
}

.asp-paper-list {
  overflow-y: auto;
  padding: 4px;
  flex: 1;
  min-height: 0;
}

.asp-paper-option {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 7px 10px;
  border: none;
  background: transparent;
  border-radius: 6px;
  font-size: 12.5px;
  color: var(--text-primary);
  cursor: pointer;
  transition: background 0.15s;
  text-align: left;
}

.asp-paper-option:hover {
  background: var(--bg-hover);
}

.asp-paper-option.active {
  background: var(--accent-light);
  color: var(--accent);
  font-weight: 500;
}

.asp-paper-check {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 14px;
  height: 14px;
  flex-shrink: 0;
  color: var(--accent);
}

.asp-paper-title {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.asp-paper-empty {
  padding: 16px;
  text-align: center;
  font-size: 12px;
  color: var(--text-muted);
}

/* ===== 知识树内联折叠树 ===== */
/* 顶部 Tabs（无缝分段控制器风格，温润圆角） */
.asp-tree-tabs {
  display: flex;
  gap: 2px;
  padding: 3px;
  background: var(--bg-active);
  border-radius: 8px;
  flex-shrink: 0;
}

.asp-tree-tab {
  flex: 1;
  padding: 6px 6px;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--text-muted);
  font-size: 11.5px;
  font-weight: 500;
  white-space: nowrap; /* Tab 文字单行排列，不换行 */
  cursor: pointer;
  transition: all 0.18s cubic-bezier(0.4, 0, 0.2, 1);
}

.asp-tree-tab:hover:not(.active) {
  color: var(--text-secondary);
}

.asp-tree-tab.active {
  background: var(--bg-card);
  color: var(--text-primary);
  font-weight: 600;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1);
}

/* Tab 禁用态（后端无对应树，如解题方法） */
.asp-tree-tab:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

/* Tab 已选数量徽标 */
.asp-tab-badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 16px;
  height: 16px;
  padding: 0 4px;
  margin-left: 4px;
  border-radius: 9999px;
  background: var(--accent);
  color: #fff;
  font-size: 10.5px;
  font-weight: 600;
  line-height: 1;
  vertical-align: 1px;
}

.asp-tree-tab:not(.active) .asp-tab-badge {
  background: var(--bg-active);
  color: var(--text-secondary);
}

[data-theme='dark'] .asp-tree-tab.active {
  background: var(--bg-input);
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.35);
}

/* 内联树容器：内容自然撑开高度（手风琴折叠，无固定高度/滚动条）；
   flex-shrink:0 使展开动画中间态不被压缩，溢出由外层 inner 裁剪 */
.asp-tree-inline {
  padding: 2px 0;
  flex-shrink: 0;
}

.asp-tree-empty {
  padding: 32px 12px;
  text-align: center;
  color: var(--text-muted);
  font-size: 12.5px;
}

/* ===== 可折叠内容容器：grid 0fr→1fr 动画（内容自然撑开高度） =====
   树区（.asp-tree-inline）无 flex 约束 → 1fr = 内容实际高度，不坍塌；
   动画中间态行高不足时，内部 flex 子项全部 flex-shrink:0 保持 DOM
   结构与边距，溢出部分由 inner overflow:hidden 从底部裁剪（tabs 先
   显示、收起按钮后显示），无挤压无重叠 */
.asp-tree-collapse {
  display: grid;
  grid-template-rows: 0fr;
  opacity: 0;
  transition: grid-template-rows 0.35s cubic-bezier(0.4, 0, 0.2, 1), opacity 0.25s ease;
}

.asp-tree-collapse-inner {
  overflow: hidden; /* 动画中间态裁剪溢出内容（不压缩子项） */
  min-height: 0; /* grid 行收缩到 0 必需 */
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.asp-tree-collapse.is-expanded {
  grid-template-rows: 1fr;
  opacity: 1;
}

/* 展开态为拟物化教研纸张卡片（样式挂内层，随内容自然增高） */
.asp-tree-collapse.is-expanded .asp-tree-collapse-inner {
  padding: 10px;
  background: linear-gradient(180deg, #fafbfc 0%, #f5f7fa 100%);
  border: 1px solid var(--border-color);
  border-radius: 10px;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.06), inset 0 1px 2px rgba(0, 0, 0, 0.03);
}

[data-theme='dark'] .asp-tree-collapse.is-expanded .asp-tree-collapse-inner {
  background: linear-gradient(180deg, var(--bg-input) 0%, var(--bg-active) 100%);
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.2), inset 0 1px 2px rgba(0, 0, 0, 0.1);
}

/* ===== 展开 / 收起 按钮（温润圆角） ===== */
.asp-tree-toggle {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  padding: 7px 14px;
  border: 1px dashed var(--accent);
  border-radius: 8px;
  background: var(--accent-light);
  color: var(--accent);
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  align-self: flex-start;
  transition: all 0.2s ease;
}

.asp-tree-toggle:hover {
  background: var(--accent);
  color: #fff;
  border-color: var(--accent);
  box-shadow: 0 2px 6px rgba(0, 0, 0, 0.08);
}

/* 收起按钮：撑满宽度，置于展开区域底部 */
.asp-tree-toggle-collapse {
  align-self: stretch;
  flex-shrink: 0; /* 动画中间态不被压缩，避免与树内容重叠 */
}

/* 收起按钮的箭头翻转 180deg 指向上 */
.asp-tree-toggle-icon-up {
  transform: rotate(180deg);
}

/* ===== 选框 Pills 优化：当下拉框有选中值时，隐藏向下小三角图标，仅保留清除 (x) 按钮 ===== */
:deep(.asp-meta-select .app-select-trigger.has-value .app-select-chevron) {
  display: none;
}
</style>
