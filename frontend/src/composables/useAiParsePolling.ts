import { ref, onUnmounted } from 'vue'
import { aiTaskApi, type AiParseTaskDetail } from '@/api/client'
import { questionApi } from '@/api/client'
import type { QuestionDetail } from '@/api/client'

/// 轮询间隔（毫秒）
const POLL_INTERVAL_MS = 2000

/// 最大轮询次数（防止无限轮询；90 次 = 3 分钟）
const MAX_ATTEMPTS = 90

/**
 * AI 异步解析任务轮询 Hook
 *
 * - `startPolling(rawText)` 提交文本任务并自动开始轮询
 * - `startPollingMedia(file, ocrProvider?)` 提交图片/PDF 任务（Multipart）并自动开始轮询
 *
 * 任务完成时自动拉取题目详情填充 `results`（数组，支持多题批处理）；
 * `result` 为首题（向后兼容单题场景）。组件卸载时自动清理定时器。
 */
export function useAiParsePolling() {
  /// 是否正在轮询（提交→完成/失败期间为 true）
  const isPolling = ref(false)
  /// 当前状态提示文字（用于 UI 展示）
  const statusText = ref('')
  /// 错误信息（任务失败或网络异常时填充）
  const error = ref<string | null>(null)
  /// 所有落库的题目详情（任务 completed 时填充，支持多题批处理）
  const results = ref<QuestionDetail[]>([])
  /// 首题详情（向后兼容单题场景，等价于 results[0]）
  const result = ref<QuestionDetail | null>(null)
  /// 当前任务 ID（提交后填充，可用于调试）
  const taskId = ref<string | null>(null)
  /// 当前任务详情（最后一次轮询返回的完整数据）
  const taskDetail = ref<AiParseTaskDetail | null>(null)

  let timer: ReturnType<typeof setInterval> | null = null
  let attempts = 0

  /// 清理定时器
  function clearTimer() {
    if (timer !== null) {
      clearInterval(timer)
      timer = null
    }
  }

  /// 重置所有状态
  function reset() {
    clearTimer()
    isPolling.value = false
    statusText.value = ''
    error.value = null
    results.value = []
    result.value = null
    taskId.value = null
    taskDetail.value = null
    attempts = 0
  }

  /// 单次轮询：查询任务状态并处理状态流转
  async function pollOnce(id: string) {
    attempts += 1

    // 超出最大轮询次数 — 终止并标记超时
    if (attempts > MAX_ATTEMPTS) {
      clearTimer()
      isPolling.value = false
      statusText.value = '任务超时，请稍后重试'
      error.value = `任务轮询超时（${(MAX_ATTEMPTS * POLL_INTERVAL_MS) / 1000}s）`
      return
    }

    try {
      const { data } = await aiTaskApi.getTaskStatus(id)
      taskDetail.value = data

      switch (data.status) {
        case 'pending':
          statusText.value = '正在排队...'
          break
        case 'processing':
          statusText.value = 'AI 正在燃烧算力解析中...'
          break
        case 'completed': {
          clearTimer()
          isPolling.value = false
          statusText.value = '解析完成，正在加载题目...'

          // 收集所有题目 ID：优先 question_ids（多题），回退 question_id（单题）
          const ids: string[] = data.question_ids?.length
            ? data.question_ids
            : data.question_id
              ? [data.question_id]
              : []

          if (ids.length === 0) {
            error.value = '任务标记为已完成，但未返回 question_id'
            statusText.value = '数据异常'
            break
          }

          // 并发拉取所有题目详情
          const settled = await Promise.allSettled(
            ids.map((qid) => questionApi.get(qid).then((r) => r.data)),
          )
          const ok: QuestionDetail[] = []
          const failed: string[] = []
          settled.forEach((s, i) => {
            if (s.status === 'fulfilled') {
              ok.push(s.value)
            } else {
              failed.push(ids[i])
            }
          })

          results.value = ok
          result.value = ok[0] ?? null

          if (ok.length === 0) {
            error.value = `题目已生成但加载失败（${failed.length} 题均失败）`
            statusText.value = '加载题目失败'
          } else if (failed.length > 0) {
            statusText.value = `解析完成（${ok.length}/${ids.length} 题加载成功）`
          } else {
            statusText.value = `解析完成（共 ${ok.length} 题）`
          }
          break
        }
        case 'failed':
          clearTimer()
          isPolling.value = false
          error.value = data.error_message ?? 'AI 解析失败（未提供详细原因）'
          statusText.value = '解析失败'
          break
        default:
          // 未知状态，保持轮询
          statusText.value = `未知状态: ${data.status}`
          break
      }
    } catch (e: any) {
      // 网络异常：不立即终止，等待下一次重试（避免偶发抖动）
      // 但如果连续异常次数过多，最终会触发 MAX_ATTEMPTS 超时退出
      statusText.value = `查询中遇到网络问题，正在重试...`
      console.warn('[useAiParsePolling] 查询任务状态失败:', e)
    }
  }

  /// 通用提交 + 轮询启动（提交动作由 submitFn 完成）
  async function submitAndPoll(submitFn: () => Promise<string>) {
    reset()
    isPolling.value = true
    statusText.value = '正在提交任务...'
    attempts = 0

    let submitTaskId: string
    try {
      submitTaskId = await submitFn()
      taskId.value = submitTaskId
      statusText.value = '正在排队...'
    } catch (e: any) {
      isPolling.value = false
      error.value = `提交任务失败: ${e?.response?.data?.error ?? e?.message ?? e}`
      statusText.value = '提交失败'
      return
    }

    // 立即触发一次轮询（避免等待 2s 才看到首次状态）
    await pollOnce(submitTaskId)

    // 如果首次轮询已经结束任务（completed/failed），不再启动定时器
    if (!isPolling.value) return

    timer = setInterval(() => {
      pollOnce(submitTaskId)
    }, POLL_INTERVAL_MS)
  }

  /// 入口 1：提交文本任务并开始轮询
  function startPolling(rawText: string) {
    return submitAndPoll(async () => {
      const { data } = await aiTaskApi.submitParseTask(rawText)
      return data.task_id
    })
  }

  /// 入口 2：提交图片/PDF 任务（Multipart）并开始轮询
  ///
  /// 大图或多页 PDF 走异步队列，避免同步接口超时。
  function startPollingMedia(file: File | Blob, ocrProvider?: string) {
    return submitAndPoll(async () => {
      const { data } = await aiTaskApi.submitParseTaskMedia(file, ocrProvider)
      return data.task_id
    })
  }

  /// 主动取消轮询（用户点击"取消"按钮时调用）
  function stopPolling() {
    clearTimer()
    isPolling.value = false
    statusText.value = '已取消'
  }

  // 组件卸载时自动清理定时器（防止内存泄漏）
  onUnmounted(() => {
    clearTimer()
  })

  return {
    // 状态
    isPolling,
    statusText,
    error,
    results,
    result,
    taskId,
    taskDetail,
    // 方法
    startPolling,
    startPollingMedia,
    stopPolling,
    reset,
  }
}
