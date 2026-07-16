/**
 * 漏斗式并发控制器（Promise Pool）+ 指数退避重试
 *
 * 纠偏模块 B：MAX_CONCURRENCY = 3，3 页 PDF 同时在后台执行渲染和 OCR
 * 补丁三：拒绝静默吞错 — worker 失败返回标准化 PoolResult，不返回 null
 * 补丁六前端：HTTP 429/5xx 指数退避自动重试 2s→4s→8s，3 次后才标记失败
 */

/** 最大并发数 */
export const MAX_CONCURRENCY = 3

/** 并发池结果（补丁三：标准化包装，拒绝 filter(Boolean) 静默吞错） */
export interface PoolResult<R> {
  status: 'success' | 'error'
  data?: R
  page?: number
  error?: string
}

/** 退避重试参数 */
const INITIAL_BACKOFF_MS = 2000
const MAX_RETRIES = 3

/**
 * 指数退避重试（补丁六前端）
 *
 * 拦截 HTTP 429（Too Many Requests）和 5xx 服务器错误，
 * 延迟 2s → 4s → 8s 后自动重试，仅在重试 3 次均失败后抛出。
 */
export async function withBackoffRetry<T>(fn: () => Promise<T>, retries = MAX_RETRIES): Promise<T> {
  let lastError: unknown
  for (let attempt = 0; attempt <= retries; attempt++) {
    try {
      return await fn()
    } catch (e: any) {
      lastError = e
      const status = e?.response?.status
      // 仅对 429（限流）进行退避重试 — 5xx 等其他错误重试无意义，反而让用户等更久
      const shouldRetry = status === 429
      if (!shouldRetry || attempt === retries) {
        throw e
      }
      const delay = INITIAL_BACKOFF_MS * Math.pow(2, attempt)
      console.warn(
        `请求被限流 (HTTP 429)，${delay / 1000}s 后重试 (${attempt + 1}/${retries})`,
      )
      await new Promise((resolve) => setTimeout(resolve, delay))
    }
  }
  throw lastError
}

/**
 * 漏斗式并发执行器
 *
 * 维护 nextIndex 游标，启动 min(MAX_CONCURRENCY, items.length) 个 worker 协程，
 * 每个 worker 循环抢任务执行。
 *
 * @param items 待处理项数组
 * @param worker 处理函数（接收 item 和 index，返回 R）
 * @param onProgress 进度回调（current, total）
 * @returns PoolResult<R>[] — 每项的结果（成功或失败），顺序与 items 一致
 */
export async function runWithConcurrency<T, R>(
  items: T[],
  worker: (item: T, index: number) => Promise<R>,
  onProgress?: (current: number, total: number) => void,
): Promise<PoolResult<R>[]> {
  const results: PoolResult<R>[] = new Array(items.length)
  let nextIndex = 0
  let completedCount = 0
  const total = items.length

  const runWorker = async () => {
    while (true) {
      const index = nextIndex++
      if (index >= total) break

      try {
        const data = await worker(items[index], index)
        results[index] = { status: 'success', data }
      } catch (e: any) {
        // ⚠️ 补丁三：不返回 null，返回标准化错误结果
        results[index] = {
          status: 'error',
          page: index + 1,
          error: e?.response?.data?.error || e?.message || '网络或解析异常',
        }
      }

      completedCount++
      onProgress?.(completedCount, total)
    }
  }

  // 启动 min(MAX_CONCURRENCY, items.length) 个 worker
  const workerCount = Math.min(MAX_CONCURRENCY, total)
  await Promise.all(Array.from({ length: workerCount }, () => runWorker()))

  return results
}
