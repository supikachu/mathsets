/**
 * IndexedDB 批量录题快照（断点续录数据保护）
 *
 * 补丁一：废弃 localStorage 5MB 限制，改用 IndexedDB（idb-keyval）
 * 补丁八：上传前检查旧快照，弹出强警告弹窗防止覆盖
 */

import { get, set, del } from 'idb-keyval'
import type { ParsedQuestion } from '@/api/client'

/** IndexedDB 存储键 */
const SNAPSHOT_KEY = 'ai_batch_snapshot'

/** 批量快照结构 */
export interface BatchSnapshot {
  /** 所有解析出的题目 */
  questions: ParsedQuestion[]
  /** 当前审阅索引 */
  currentIndex: number
  /** 已处理（已录入或已跳过）的题目索引集合 */
  processedIds: number[]
  /** 创建时间戳 */
  createdAt: number
  /** 来源类型 */
  source: 'image' | 'pdf'
  /** 原始页数信息（PDF 用） */
  totalPages?: number
}

/** 保存批量快照到 IndexedDB */
export async function saveBatchSnapshot(snapshot: BatchSnapshot): Promise<void> {
  try {
    await set(SNAPSHOT_KEY, snapshot)
  } catch (e) {
    console.error('IndexedDB 存储失败，快照保存异常:', e)
  }
}

/** 从 IndexedDB 加载批量快照 */
export async function loadBatchSnapshot(): Promise<BatchSnapshot | undefined> {
  try {
    return await get<BatchSnapshot>(SNAPSHOT_KEY)
  } catch (e) {
    console.error('IndexedDB 读取失败:', e)
    return undefined
  }
}

/** 从 IndexedDB 清除批量快照 */
export async function clearBatchSnapshot(): Promise<void> {
  try {
    await del(SNAPSHOT_KEY)
  } catch (e) {
    console.error('IndexedDB 清除失败:', e)
  }
}

/** 检查是否存在未完成的快照（补丁八：上传前拦截） */
export async function hasUnfinishedSnapshot(): Promise<BatchSnapshot | undefined> {
  const snapshot = await loadBatchSnapshot()
  if (!snapshot) return undefined
  // 检查是否真的未完成（有题目且不是全部已处理）
  if (
    snapshot.questions.length > 0 &&
    snapshot.processedIds.length < snapshot.questions.length
  ) {
    return snapshot
  }
  // 已全部处理完，自动清理
  await clearBatchSnapshot()
  return undefined
}
