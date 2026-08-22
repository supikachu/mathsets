/**
 * useKnowledgeTreeCache — 知识树数据的前端内存级共享缓存
 *
 * 设计要点：
 * - knowledgeTreeApi.list() 全量树元数据在整个页面生命周期内只拉一次（含并发去重），
 *   供 AttributeSidePanel（Tab/学段切换）与 QuestionEdit（tree_id→kind 分类分发）共享
 * - 单棵树数据按 treeId 缓存：Tab 来回切换、学段切出再切回均零请求
 * - 失败不进缓存，下次调用自动重试
 * - buildTreeMetaIndex：从嵌套树构建扁平 meta（parentId / namePath / childrenIds），
 *   供已选 chips 折叠（最高层已选节点 +N）、悬浮完整路径与移除时清理子孙使用
 */
import {
  knowledgeTreeApi,
  knowledgeNodeApi,
  type KnowledgeTree,
  type KnowledgeNodeTreeNode,
} from '@/api/client'

// ─── 树列表缓存（全量元数据，含并发去重） ─────────────────────────────
let treeListCache: KnowledgeTree[] | null = null
let treeListPromise: Promise<KnowledgeTree[]> | null = null

export async function getKnowledgeTreeList(): Promise<KnowledgeTree[]> {
  if (treeListCache) return treeListCache
  if (!treeListPromise) {
    treeListPromise = knowledgeTreeApi.list()
      .then((res) => {
        treeListCache = res.data
        return res.data
      })
      .finally(() => {
        treeListPromise = null
      })
  }
  return treeListPromise
}

// ─── 单棵树数据缓存（key = treeId，全局唯一） ─────────────────────────
const treeDataCache = new Map<string, KnowledgeNodeTreeNode[]>()

/**
 * 安全解包 API 响应：兼容后端直接返回数组或包裹在 { data: [...] } 两种格式
 * 削顶后后端直接返回数组，但历史上部分接口使用 { data: ... } 包裹，此处统一处理
 */
export function unwrapTreeResponse(raw: unknown): KnowledgeNodeTreeNode[] {
  if (Array.isArray(raw)) return raw as KnowledgeNodeTreeNode[]
  if (raw && typeof raw === 'object' && Array.isArray((raw as any).data)) {
    return (raw as any).data as KnowledgeNodeTreeNode[]
  }
  console.warn('[KnowledgeTreeCache] 意外的树响应格式，期望数组或 { data: [...] }', raw)
  return []
}

export async function getKnowledgeTreeData(treeId: string): Promise<KnowledgeNodeTreeNode[]> {
  const hit = treeDataCache.get(treeId)
  if (hit) return hit
  const res = await knowledgeNodeApi.getTree(treeId)
  const data = unwrapTreeResponse(res.data)
  treeDataCache.set(treeId, data)
  return data
}

// ─── 扁平 meta 索引（供 chips 折叠与路径悬浮提示） ────────────────────
export interface TreeMetaInfo {
  parentId: string | null
  name: string
  /** 完整知识路径（如「集合与常用逻辑用语 / 集合 / 集合的概念」） */
  namePath: string
  /** 直接子节点 ID；移除折叠 chip 时据此连带清掉被代管的已选子孙 */
  childrenIds: string[]
}

export function buildTreeMetaIndex(nodes: KnowledgeNodeTreeNode[]): Map<string, TreeMetaInfo> {
  const map = new Map<string, TreeMetaInfo>()
  const walk = (list: KnowledgeNodeTreeNode[], parentId: string | null, parentPath: string) => {
    for (const n of list) {
      const namePath = parentPath ? `${parentPath} / ${n.name}` : n.name
      map.set(n.id, {
        parentId,
        name: n.name,
        namePath,
        childrenIds: n.children.map((c) => c.id),
      })
      if (n.children.length > 0) walk(n.children, n.id, namePath)
    }
  }
  walk(nodes, null, '')
  return map
}
