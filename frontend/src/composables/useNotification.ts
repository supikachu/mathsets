import { ref, onUnmounted } from 'vue'
import { useRouter } from 'vue-router'
import { notificationApi, type Notification } from '@/api/client'
import { useToast } from '@/composables/useToast'
import { useAuthStore } from '@/stores/auth'

/**
 * 通知中心 composable
 *
 * 职责：
 * 1. 管理通知列表 + 未读数（全局单例状态）
 * 2. 通过一次性 Ticket 安全建立 SSE 连接
 * 3. 收到实时消息时：追加列表 + 递增未读 + Toast 提示
 * 4. 点击通知：标记已读 + 按 resource_type 路由跳转
 * 5. 断线自动重连（指数退避）
 */

// ── 全局单例状态（所有组件共享同一份） ──
const notifications = ref<Notification[]>([])
const unreadCount = ref(0)
let eventSource: EventSource | null = null
let reconnectTimer: ReturnType<typeof setTimeout> | null = null
let reconnectAttempts = 0
let initialized = false

export function useNotification() {
  const router = useRouter()
  const toast = useToast()
  const auth = useAuthStore()

  // ── Ticket 换取 + SSE 建立 ──
  async function connect() {
    if (!auth.isLoggedIn) return

    try {
      // 1. 用 JWT 换取一次性 ticket（30s 过期）
      const ticketRes = await notificationApi.getTicket()
      const ticket = ticketRes.data.ticket

      // 2. 用 ticket 建立 SSE 连接（JWT 不暴露在 URL 中）
      if (eventSource) {
        eventSource.close()
      }
      eventSource = new EventSource(`/api/v1/notifications/stream?ticket=${ticket}`)

      eventSource.onopen = () => {
        reconnectAttempts = 0
      }

      // 3. 收到消息：追加列表 + 递增未读 + Toast
      eventSource.onmessage = (ev) => {
        try {
          const n: Notification = JSON.parse(ev.data)
          notifications.value.unshift(n)
          unreadCount.value++
          toast.info(n.title)
        } catch {
          /* 忽略解析异常 */
        }
      }

      // 4. 断线自动重连（指数退避：1s → 2s → 4s → …，上限 30s）
      eventSource.onerror = () => {
        eventSource?.close()
        eventSource = null

        if (!auth.isLoggedIn) return

        reconnectAttempts++
        const delay = Math.min(1000 * 2 ** reconnectAttempts, 30000)
        if (reconnectTimer) clearTimeout(reconnectTimer)
        reconnectTimer = setTimeout(() => connect(), delay)
      }
    } catch {
      /* ticket 获取失败，稍后重试 */
      if (!auth.isLoggedIn) return
      reconnectAttempts++
      const delay = Math.min(1000 * 2 ** reconnectAttempts, 30000)
      if (reconnectTimer) clearTimeout(reconnectTimer)
      reconnectTimer = setTimeout(() => connect(), delay)
    }
  }

  // ── 初始化：加载历史 + 拉取未读数 + 建立 SSE ──
  async function init() {
    if (initialized) return
    initialized = true

    try {
      const [listRes, countRes] = await Promise.all([
        notificationApi.list(),
        notificationApi.getUnreadCount(),
      ])
      notifications.value = listRes.data || []
      unreadCount.value = countRes.data.count || 0
    } catch {
      /* 静默失败，不阻断 UI */
    }

    connect()
  }

  // ── 断开 SSE ──
  function disconnect() {
    if (eventSource) {
      eventSource.close()
      eventSource = null
    }
    if (reconnectTimer) {
      clearTimeout(reconnectTimer)
      reconnectTimer = null
    }
    reconnectAttempts = 0
  }

  // ── 点击通知：标记已读 + 路由跳转 ──
  async function handleClick(n: Notification) {
    if (!n.is_read) {
      try {
        await notificationApi.markRead(n.id)
        n.is_read = true
        unreadCount.value = Math.max(0, unreadCount.value - 1)
      } catch {
        /* 标记失败不阻断跳转 */
      }
    }

    // 按 resource_type 路由映射
    if (n.resource_type && n.resource_id) {
      const routeMap: Record<string, string> = {
        question: `/questions/${n.resource_id}`,
        question_edit: `/questions/${n.resource_id}/edit`,
        space: `/spaces/${n.resource_id}/settings`,
      }
      const path = routeMap[n.resource_type]
      if (path) router.push(path)
    }
  }

  // ── 全部标记已读 ──
  async function markAllRead() {
    try {
      await notificationApi.markAllRead()
      notifications.value.forEach((n) => (n.is_read = true))
      unreadCount.value = 0
    } catch (e: any) {
      toast.error('标记失败')
    }
  }

  // ── 删除单条通知 ──
  async function deleteNotification(n: Notification) {
    try {
      await notificationApi.delete(n.id)
      // 本地移除；如果是未读通知，同步递减未读数
      if (!n.is_read) {
        unreadCount.value = Math.max(0, unreadCount.value - 1)
      }
      notifications.value = notifications.value.filter((item) => item.id !== n.id)
    } catch (e: any) {
      toast.error(e.response?.data?.error || e.message || '删除失败')
    }
  }

  // ── 组件卸载时清理（仅当 auth 变为 false 时真正断开） ──
  onUnmounted(() => {
    // 全局单例：组件卸载不断开 SSE，仅在登出时断开
    // disconnect() 由 auth store logout 显式调用
  })

  return {
    notifications,
    unreadCount,
    init,
    disconnect,
    connect,
    handleClick,
    markAllRead,
    deleteNotification,
  }
}
