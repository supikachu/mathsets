import { ref, onUnmounted } from 'vue'
import { aiTaskApi, type AiParseTaskDetail, type ParseMode } from '@/api/client'

/// 轮询间隔（毫秒）
const POLL_INTERVAL_MS = 2000
/// 最大轮询次数（600 次 = 20 分钟，防止无限轮询）
const MAX_ATTEMPTS = 600

/// 终态集合
const TERMINAL: ReadonlySet<string> = new Set([
  'success',
  'partial_success',
  'failed',
  'cancelled',
])

/**
 * V2.1.1 AI 解析任务轮询 Hook
 *
 * `startPolling(documentId)`：创建任务并轮询进度；
 * 任务到达终态时自动停止，`task.value` 为最终任务详情
 * （含 success_count/failed_count/current_page/question_ids 等）。
 */
export function useAiParsePolling() {
  /// 是否正在轮询
  const isPolling = ref(false)
  /// 状态提示文字
  const statusText = ref('')
  /// 错误信息
  const error = ref<string | null>(null)
  /// 最近一次任务详情
  const task = ref<AiParseTaskDetail | null>(null)
  /// 当前任务 ID
  const taskId = ref<string | null>(null)

  let timer: ReturnType<typeof setInterval> | null = null
  let attempts = 0

  function clearTimer() {
    if (timer !== null) {
      clearInterval(timer)
      timer = null
    }
  }

  function reset() {
    clearTimer()
    isPolling.value = false
    statusText.value = ''
    error.value = null
    task.value = null
    taskId.value = null
    attempts = 0
  }

  function handleTask(data: AiParseTaskDetail) {
    task.value = data
    switch (data.status) {
      case 'pending':
        statusText.value = '正在排队…'
        break
      case 'processing':
        statusText.value = 'AI 正在解析（可随时取消）…'
        break
      case 'retrying':
        statusText.value = '解析遇到波动，正在重试…'
        break
      case 'success':
        statusText.value = '解析完成'
        break
      case 'partial_success':
        statusText.value = `部分成功（${data.success_count}/${data.total_count} 题）`
        break
      case 'failed':
        statusText.value = '解析失败'
        error.value = data.error_message ?? 'AI 解析失败'
        break
      case 'cancelled':
        statusText.value = '已取消'
        break
      case 'completed':
        statusText.value = '解析完成'
        break
      default:
        statusText.value = `未知状态: ${data.status}`
        break
    }
  }

  async function pollOnce(id: string) {
    attempts += 1
    if (attempts > MAX_ATTEMPTS) {
      clearTimer()
      isPolling.value = false
      statusText.value = '任务超时，请稍后重试'
      error.value = `任务轮询超时（${MAX_ATTEMPTS * POLL_INTERVAL_MS / 1000 / 60} 分钟）`
      return
    }
    try {
      const { data } = await aiTaskApi.getParseTask(id)
      handleTask(data)
      if (TERMINAL.has(data.status)) {
        clearTimer()
        isPolling.value = false
      }
    } catch (e: any) {
      // 网络抖动：继续重试
      statusText.value = '查询中遇到网络问题，正在重试…'
      console.warn('[useAiParsePolling] 查询任务状态失败:', e)
    }
  }

  /// 入口：为已确认 Document 创建任务并开始轮询
  /// parseMode：pdf_direct=仅 PDF 直连（失败由调用方引导回退）/ page=仅逐页 / 缺省=自动降级
  async function startPolling(documentId: string, parseMode?: ParseMode) {
    reset()
    isPolling.value = true
    statusText.value = '正在提交任务…'
    attempts = 0

    let submitTaskId: string
    try {
      const { data } = await aiTaskApi.createParseTask(documentId, parseMode)
      submitTaskId = data.task_id
      taskId.value = submitTaskId
      statusText.value = '正在排队…'
    } catch (e: any) {
      isPolling.value = false
      error.value = e?.response?.data?.error ?? e?.message ?? '提交任务失败'
      statusText.value = '提交失败'
      return
    }

    await pollOnce(submitTaskId)
    if (!isPolling.value) return

    timer = setInterval(() => {
      pollOnce(submitTaskId)
    }, POLL_INTERVAL_MS)
  }

  /// 取消任务（已落库题目保留）
  async function cancel() {
    if (!taskId.value) return
    try {
      await aiTaskApi.cancelParseTask(taskId.value)
      statusText.value = '已请求取消…'
    } catch (e: any) {
      error.value = e?.response?.data?.error ?? e?.message ?? '取消失败'
    }
  }

  /// 主动停止轮询（不取消任务）
  function stopPolling() {
    clearTimer()
    isPolling.value = false
  }

  onUnmounted(() => {
    clearTimer()
  })

  return {
    isPolling,
    statusText,
    error,
    task,
    taskId,
    startPolling,
    cancel,
    stopPolling,
    reset,
  }
}
