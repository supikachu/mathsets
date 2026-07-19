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
 * 调用 `startPolling(rawText)` 提交任务并自动开始轮询；
 * 当任务完成时，自动拉取题目详情并填充 `result`；
 * 组件卸载时自动清理定时器，防止内存泄漏。
 */
export function useAiParsePolling() {
  /// 是否正在轮询（提交→完成/失败期间为 true）
  const isPolling = ref(false)
  /// 当前状态提示文字（用于 UI 展示）
  const statusText = ref('')
  /// 错误信息（任务失败或网络异常时填充）
  const error = ref<string | null>(null)
  /// 最终落库的题目详情（任务 completed 时填充）
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
      error.value = `任务轮询超时（${MAX_ATTEMPTS * POLL_INTERVAL_MS / 1000}s）`
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
          // 拉取题目详情
          if (data.question_id) {
            try {
              const { data: question } = await questionApi.get(data.question_id)
              result.value = question
              statusText.value = '解析完成'
            } catch (e: any) {
              error.value = `题目已生成但加载失败: ${e?.message ?? e}`
              statusText.value = '加载题目失败'
            }
          } else {
            error.value = '任务标记为已完成，但未返回 question_id'
            statusText.value = '数据异常'
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

  /// 入口：提交任务并开始轮询
  async function startPolling(rawText: string) {
    // 重置上一次的状态（避免污染）
    reset()

    isPolling.value = true
    statusText.value = '正在提交任务...'
    attempts = 0

    // 1. 提交任务
    let submitTaskId: string
    try {
      const { data } = await aiTaskApi.submitParseTask(rawText)
      submitTaskId = data.task_id
      taskId.value = submitTaskId
      statusText.value = '正在排队...'
    } catch (e: any) {
      isPolling.value = false
      error.value = `提交任务失败: ${e?.response?.data?.error ?? e?.message ?? e}`
      statusText.value = '提交失败'
      return
    }

    // 2. 立即触发一次轮询（避免等待 2s 才看到首次状态）
    await pollOnce(submitTaskId)

    // 如果首次轮询已经结束任务（completed/failed），不再启动定时器
    if (!isPolling.value) return

    // 3. 启动 2 秒定时器
    timer = setInterval(() => {
      pollOnce(submitTaskId)
    }, POLL_INTERVAL_MS)
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
    result,
    taskId,
    taskDetail,
    // 方法
    startPolling,
    stopPolling,
    reset,
  }
}
