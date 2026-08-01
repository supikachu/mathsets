<template>
  <Teleport to="body">
    <Transition name="drawer-fade">
      <div v-if="open" class="notif-overlay" @click="$emit('update:open', false)">
        <Transition name="drawer-slide">
          <div v-if="open" class="notif-drawer" @click.stop>
            <!-- 头部 -->
            <div class="drawer-header">
              <h2 class="drawer-title">通知</h2>
              <button
                v-if="unreadCount > 0"
                class="drawer-mark-all"
                @click="markAllRead"
              >
                全部标记已读
              </button>
            </div>

            <!-- 通知列表 -->
            <div class="drawer-body">
              <div v-if="notifications.length === 0" class="drawer-empty">
                <AppIcon name="bell" :size="36" :stroke="1.5" />
                <p>暂无通知</p>
              </div>

              <div
                v-for="n in notifications"
                :key="n.id"
                class="notif-item"
                :class="{ unread: !n.is_read }"
                @click="handleClick(n)"
              >
                <!-- 未读指示点 -->
                <span class="notif-dot" :class="{ active: !n.is_read }"></span>

                <div class="notif-content">
                  <div class="notif-title-row">
                    <span class="notif-title">{{ n.title }}</span>
                    <span class="notif-time">{{ formatTime(n.created_at) }}</span>
                  </div>
                  <p v-if="n.body" class="notif-body">{{ n.body }}</p>
                </div>

                <!-- 删除按钮：点击删除并阻止冒泡（不触发 handleClick） -->
                <button
                  class="notif-delete"
                  title="删除"
                  @click.stop="deleteNotification(n)"
                >
                  <AppIcon name="trash" :size="14" />
                </button>
              </div>
            </div>
          </div>
        </Transition>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { AppIcon } from '@/components/ui'
import { useNotification } from '@/composables/useNotification'
import type { Notification } from '@/api/client'

defineProps<{
  open: boolean
}>()

defineEmits<{
  'update:open': [value: boolean]
}>()

const { notifications, unreadCount, markAllRead, handleClick, deleteNotification } = useNotification()

function formatTime(iso: string): string {
  const d = new Date(iso)
  const now = new Date()
  const diff = now.getTime() - d.getTime()
  const min = Math.floor(diff / 60000)
  const hour = Math.floor(diff / 3600000)
  const day = Math.floor(diff / 86400000)

  if (min < 1) return '刚刚'
  if (min < 60) return `${min} 分钟前`
  if (hour < 24) return `${hour} 小时前`
  if (day < 7) return `${day} 天前`
  return d.toLocaleDateString('zh-CN')
}
</script>

<style scoped>
.notif-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.2);
  backdrop-filter: blur(2px);
  z-index: 200;
}

.notif-drawer {
  position: fixed;
  top: 0;
  right: 0;
  width: 380px;
  max-width: 90vw;
  height: 100%;
  background: var(--bg-card);
  display: flex;
  flex-direction: column;
  box-shadow: -4px 0 24px rgba(0, 0, 0, 0.08);
}

/* ===== Header ===== */
.drawer-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 18px 20px 14px;
  border-bottom: 1px solid var(--border-light);
  flex-shrink: 0;
}

.drawer-title {
  font-size: 17px;
  font-weight: 700;
  color: var(--text-primary);
  margin: 0;
  letter-spacing: -0.02em;
}

.drawer-mark-all {
  font-size: 12px;
  font-weight: 600;
  color: var(--accent);
  background: transparent;
  transition: var(--transition-fast);
}

.drawer-mark-all:hover {
  opacity: 0.7;
}

/* ===== Body ===== */
.drawer-body {
  flex: 1;
  overflow-y: auto;
  overscroll-behavior: contain;
}

/* ===== Empty ===== */
.drawer-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 60px 20px;
  color: var(--text-muted);
}

.drawer-empty p {
  font-size: 13px;
  margin: 0;
}

/* ===== Notification item ===== */
.notif-item {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  width: 100%;
  text-align: left;
  padding: 14px 20px;
  border-bottom: 1px solid var(--border-lighter);
  transition: var(--transition-fast);
}

.notif-item:hover {
  background: var(--bg-hover);
}

.notif-item.unread {
  background: var(--accent-lighter, rgba(88, 132, 255, 0.04));
}

/* 未读指示点 */
.notif-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: transparent;
  flex-shrink: 0;
  margin-top: 5px;
}

.notif-dot.active {
  background: var(--accent);
}

/* 通知内容 */
.notif-content {
  flex: 1;
  min-width: 0;
}

.notif-title-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 2px;
}

.notif-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
}

.notif-time {
  font-size: 11px;
  color: var(--text-muted);
  flex-shrink: 0;
}

.notif-body {
  font-size: 13px;
  color: var(--text-secondary);
  line-height: 1.5;
  margin: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
}

/* 删除按钮 */
.notif-delete {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border-radius: var(--radius-xs, 6px);
  color: var(--text-muted);
  background: transparent;
  flex-shrink: 0;
  margin-top: 2px;
  transition: var(--transition-fast);
  opacity: 0;
}

.notif-item:hover .notif-delete {
  opacity: 1;
}

.notif-delete:hover {
  background: var(--danger-light);
  color: var(--danger);
}

/* ===== Transitions ===== */
.drawer-fade-enter-active,
.drawer-fade-leave-active {
  transition: opacity 0.2s ease;
}

.drawer-fade-enter-from,
.drawer-fade-leave-to {
  opacity: 0;
}

.drawer-slide-enter-active {
  transition: transform 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
}

.drawer-slide-leave-active {
  transition: transform 0.2s ease;
}

.drawer-slide-enter-from,
.drawer-slide-leave-to {
  transform: translateX(100%);
}
</style>
