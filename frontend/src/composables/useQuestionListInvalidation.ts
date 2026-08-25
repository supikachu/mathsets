/**
 * 题库列表失效登记。
 *
 * QuestionList 被 keep-alive 缓存；从详情/编辑返回时用这里的标记决定
 * 是跳过请求、静默刷新，还是只丢掉被改过的详情缓存。
 */

export type QuestionListInvalidation = {
  fullRefresh: boolean
  refreshCounts: boolean
  dirtyIds: string[]
  deletedIds: string[]
}

const dirtyIds = new Set<string>()
const deletedIds = new Set<string>()
let fullRefresh = false
let refreshCounts = false

/** 单题被编辑 / 审核 / 提交后，列表需更新该卡 */
export function markQuestionDirty(id: string | null | undefined) {
  if (!id) {
    fullRefresh = true
  } else {
    dirtyIds.add(id)
  }
  refreshCounts = true
}

/** 题目已删除，列表需去掉该卡并补位 */
export function markQuestionDeleted(id: string) {
  deletedIds.add(id)
  dirtyIds.delete(id)
  refreshCounts = true
}

/** 批量录入 / 丢弃等，整表重拉 */
export function markQuestionListStale() {
  fullRefresh = true
  refreshCounts = true
}

export function consumeQuestionListInvalidation(): QuestionListInvalidation {
  const snapshot: QuestionListInvalidation = {
    fullRefresh,
    refreshCounts,
    dirtyIds: [...dirtyIds],
    deletedIds: [...deletedIds],
  }
  dirtyIds.clear()
  deletedIds.clear()
  fullRefresh = false
  refreshCounts = false
  return snapshot
}

export function hasQuestionListWork(inv: QuestionListInvalidation): boolean {
  return inv.fullRefresh || inv.dirtyIds.length > 0 || inv.deletedIds.length > 0
}
