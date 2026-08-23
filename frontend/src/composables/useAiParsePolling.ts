import { ref, onUnmounted } from 'vue'
import { aiTaskApi, type AiParseTaskDetail, type ParseMode, type ParsePipeline } from '@/api/client'

/// 轮询间隔（毫秒）
const POLL_INTERVAL_MS = 2000
/// 最大轮询次数（1800 次 = 60 分钟：长 PDF 逐题打标常超过 20 分钟）
const MAX_ATTEMPTS = 1800

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
  let cancelRequested = false

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
    cancelRequested = false
  }

  function handleTask(data: AiParseTaskDetail) {
    task.value = data
    switch (data.status) {
      case 'pending':
        statusText.value = cancelRequested ? '正在停止…' : '正在排队…'
        break
      case 'processing':
        statusText.value = cancelRequested ? '正在停止解析…' : 'AI 正在解析（可随时取消）…'
        break
      case 'retrying':
        statusText.value = cancelRequested ? '正在停止…' : '解析遇到波动，正在重试…'
        break
      case 'success':
        statusText.value =
          data.pipeline === 'ocr_export' && !(data.staged_questions ?? []).length
            ? 'OCR 完成，请查看文本并导入 JSON'
            : '解析完成'
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
      const taggingPending = (data.staged_questions ?? []).some(
        (s) => s.tagging_status === 'pending',
      )
      if (cancelRequested || (TERMINAL.has(data.status) && !taggingPending)) {
        clearTimer()
        isPolling.value = false
      } else if (TERMINAL.has(data.status) && taggingPending) {
        statusText.value = '题目已识别，正在填充标签…'
      }
    } catch (e: any) {
      // 网络抖动：继续重试
      statusText.value = '查询中遇到网络问题，正在重试…'
      console.warn('[useAiParsePolling] 查询任务状态失败:', e)
    }
  }

  /// 入口：为已确认 Document 创建任务并开始轮询
  /// parseMode：pdf_direct=仅 PDF 直连（失败由调用方引导回退）/ page=仅逐页 / 缺省=自动降级
  async function startPolling(
    documentId: string,
    parseMode?: ParseMode,
    pipeline?: ParsePipeline,
  ) {
    reset()
    isPolling.value = true
    statusText.value = pipeline === 'ocr_export' ? '正在提交 OCR 任务…' : '正在提交任务…'
    attempts = 0

    let submitTaskId: string
    try {
      const { data } = await aiTaskApi.createParseTask(documentId, parseMode, pipeline)
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
    const id = taskId.value
    cancelRequested = true
    clearTimer()
    isPolling.value = false
    try {
      await aiTaskApi.cancelParseTask(id)
      statusText.value = '已取消'
      if (task.value && task.value.status !== 'cancelled') {
        handleTask({ ...task.value, status: 'cancelled' })
      }
    } catch (e: any) {
      cancelRequested = false
      error.value = e?.response?.data?.error ?? e?.message ?? '取消失败'
    }
  }

  /// 主动停止轮询（不取消任务）
  function stopPolling() {
    clearTimer()
    isPolling.value = false
  }

  /// 恢复已有解析任务的轮询（离开页面后再进来，不重新 create）
  async function resumePolling(existingTaskId: string) {
    if (!existingTaskId) return
    if (isPolling.value && taskId.value === existingTaskId && timer !== null) return
    clearTimer()
    cancelRequested = false
    isPolling.value = true
    taskId.value = existingTaskId
    error.value = null
    attempts = 0
    statusText.value = '正在同步识别进度…'
    await pollOnce(existingTaskId)
    if (!isPolling.value) return
    timer = setInterval(() => {
      pollOnce(existingTaskId)
    }, POLL_INTERVAL_MS)
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
    resumePolling,
    reset,
  }
}
