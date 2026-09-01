<template>
  <div class="layout-root" :class="{ 'is-immersive': isImmersive }">
    <div class="app-container">
      <!-- 悬浮圆弧 64px 极简 Slim 胶囊侧边栏 (Icon-only 纯图标 + Tooltip + 底部 ThemeToggle) -->
      <aside class="sidebar w-16 my-4 ml-4 rounded-2xl bg-white dark:bg-slate-900 shadow-sm border border-gray-100 dark:border-slate-800 flex flex-col items-center justify-between py-4 shrink-0 z-40 select-none">
        <!-- 顶部：Logo + 纯图标核心导航 -->
        <div class="flex flex-col items-center w-full gap-5">
          <!-- 顶端 Logo -->
          <router-link
            to="/dashboard"
            class="w-10 h-10 rounded-xl bg-blue-50 dark:bg-blue-950/50 text-blue-600 dark:text-blue-400 flex items-center justify-center shrink-0 hover:scale-105 transition-transform"
            title="协同题库"
          >
            <AppIcon name="logo" :size="22" />
          </router-link>

          <!-- 纯图标导航列表（无文本节点，右侧悬浮 Tooltip 气泡） -->
          <nav class="flex flex-col items-center gap-2.5 w-full px-2">
            <router-link
              v-for="item in items"
              :key="item.path"
              :to="item.path"
              class="group relative w-10 h-10 flex items-center justify-center rounded-xl text-gray-500 dark:text-gray-400 hover:text-blue-600 dark:hover:text-blue-400 hover:bg-gray-100 dark:hover:bg-slate-800 transition-colors cursor-pointer"
              :class="{ '!bg-blue-500 !text-white shadow-sm shadow-blue-500/30': isActive(item.path) }"
            >
              <AppIcon :name="item.icon" :size="20" />

              <!-- 右侧悬浮提示框 (Tooltip) -->
              <div class="absolute left-full ml-4 px-2.5 py-1 bg-gray-800 dark:bg-slate-700 text-white text-xs rounded-md shadow-md opacity-0 invisible group-hover:opacity-100 group-hover:visible whitespace-nowrap z-50 pointer-events-none transition-all duration-200">
                {{ item.label }}
              </div>
            </router-link>
          </nav>
        </div>

        <!-- 底端工具区：试题篮（会话工具）+ 主题，与上方路由图标分开 -->
        <div class="sidebar-tools">
          <router-link
            to="/basket"
            class="sidebar-tool-btn group relative text-gray-500 dark:text-gray-400 hover:text-blue-600 dark:hover:text-blue-400 hover:bg-gray-100 dark:hover:bg-slate-800"
            :class="{ 'is-current text-blue-600 dark:text-blue-400 bg-blue-50 dark:bg-blue-950/40': isBasketPage }"
            title="试题篮"
          >
            <AppIcon name="shopping-cart" :size="20" />
            <span v-if="basket.count.value > 0" class="sidebar-tool-badge">{{ basketBadge }}</span>
            <div class="absolute left-full ml-4 px-2.5 py-1 bg-gray-800 dark:bg-slate-700 text-white text-xs rounded-md shadow-md opacity-0 invisible group-hover:opacity-100 group-hover:visible whitespace-nowrap z-50 pointer-events-none transition-all duration-200">
              试题篮
            </div>
          </router-link>
          <div class="group relative flex items-center justify-center shrink-0">
            <ThemeToggle />
            <div class="absolute left-full ml-4 px-2.5 py-1 bg-gray-800 dark:bg-slate-700 text-white text-xs rounded-md shadow-md opacity-0 invisible group-hover:opacity-100 group-hover:visible whitespace-nowrap z-50 pointer-events-none transition-all duration-200">
              切换主题
            </div>
          </div>
        </div>
      </aside>

      <!-- 主内容区 -->
      <div class="main-content">
        <div class="view active">
          <!-- keep-alive：仅缓存题库列表页，避免从详情页返回时整页重挂载+重请数据
               其它页面（详情/编辑/设置）不缓存，保证进入时拿到最新数据 -->
          <router-view v-slot="{ Component }">
            <keep-alive :include="['QuestionList']">
              <component :is="Component" />
            </keep-alive>
          </router-view>
        </div>
      </div>
    </div>

    <!-- 移动端底部导航 -->
    <BottomNav />

    <!-- 通知抽屉 -->
    <NotificationDrawer v-model:open="showNotifDrawer" />
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useRoute } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import { useSpaceStore } from '@/stores/space'
import { useNavItems } from '@/composables/useNavItems'
import { AppIcon } from '@/components/ui'
import BottomNav from '@/components/BottomNav.vue'
import ThemeToggle from '@/components/ThemeToggle.vue'
import NotificationDrawer from '@/components/NotificationDrawer.vue'
import { useNotification } from '@/composables/useNotification'
import { useQuestionBasket } from '@/composables/useQuestionBasket'

const route = useRoute()
const auth = useAuthStore()
const space = useSpaceStore()
const { items } = useNavItems()
const basket = useQuestionBasket()

const isBasketPage = computed(() => route.path === '/basket' || route.path.startsWith('/basket/'))
const basketBadge = computed(() =>
  basket.count.value > 99 ? '99+' : String(basket.count.value),
)

const showNotifDrawer = ref(false)
const { init: initNotifications } = useNotification()

/** 沉浸式录题模式：路由 meta.immersive=true 时隐藏左侧系统导航，
 *  让 QuestionEdit 独享 100% 横向屏幕空间 */
const isImmersive = computed(() => route.meta.immersive === true)

function isActive(path: string) {
  return route.path === path || route.path.startsWith(path + '/')
}

onMounted(() => {
  if (auth.isLoggedIn) {
    space.fetchSpaces()
    initNotifications()
  }
})
</script>

<style scoped>
.layout-root {
  width: 100vw;
  height: 100vh;
  overflow: hidden;
  background: var(--bg-primary);
}

/* ===== Sidebar nav card ===== */
.sidebar-nav-card {
  display: flex;
  flex-direction: column;
}

.sidebar-brand {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 17px;
  font-weight: 700;
  padding: 6px 10px 18px;
  color: var(--text-primary);
  letter-spacing: -0.02em;
}

.brand-left {
  display: flex;
  align-items: center;
  gap: 9px;
}

.brand-icon {
  color: var(--accent);
}

/* ===== Sidebar user card ===== */
.sidebar-tools {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  padding-top: 12px;
  margin-top: 8px;
  border-top: 1px solid var(--border-color, rgba(0, 0, 0, 0.06));
}

.sidebar-tool-btn {
  position: relative;
  width: 40px;
  height: 40px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 12px;
  transition: background-color 0.15s ease, color 0.15s ease;
}

.sidebar-tool-badge {
  position: absolute;
  top: 2px;
  right: 2px;
  min-width: 16px;
  height: 16px;
  padding: 0 4px;
  border-radius: 8px;
  background: #2563eb;
  color: #fff;
  font-size: 10px;
  font-weight: 700;
  line-height: 16px;
  text-align: center;
}

.sidebar-user-card {
  position: relative;
}

/* 主题切换行：紧贴用户卡片上方 */
.sidebar-theme-row {
  display: flex;
  justify-content: flex-end;
  padding: 0 4px 8px;
}

.sidebar-user-trigger {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  padding: 6px 8px;
  border-radius: var(--radius-xs);
  background: transparent;
  transition: var(--transition-fast);
}

.sidebar-user-trigger:hover {
  background: var(--bg-hover);
}

.user-avatar-wrap {
  width: 32px;
  height: 32px;
  border-radius: 50%;
  overflow: hidden;
  flex-shrink: 0;
  background: var(--accent-gradient);
  display: flex;
  align-items: center;
  justify-content: center;
}

.user-avatar-img {
  width: 100%;
  height: 100%;
  object-fit: cover; /* ⚠️ 防非正方形图片被拉伸 */
  border-radius: 50%;
}

.user-avatar-letter {
  color: #fff;
  font-size: 14px;
  font-weight: 700;
}

.sidebar-user-info {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 1px;
  overflow: hidden;
}

.sidebar-user-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 100%;
}

.sidebar-user-role {
  font-size: 11px;
  color: var(--text-muted);
}

.sidebar-user-chevron {
  color: var(--text-muted);
  flex-shrink: 0;
  transition: transform 0.25s cubic-bezier(0.4, 0, 0.2, 1);
}

.sidebar-user-chevron.rotated {
  transform: rotate(180deg);
}

.sidebar-user-dropdown {
  position: absolute;
  bottom: calc(100% + 6px);
  left: 0;
  right: 0;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  box-shadow: var(--shadow-md);
  padding: 6px;
  z-index: 150;
}

.user-menu-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  text-align: left;
  padding: 9px 12px;
  border-radius: var(--radius-xs);
  font-size: 13px;
  color: var(--text-primary);
  transition: var(--transition-fast);
}

.menu-item-logout {
  color: var(--danger);
}

.user-menu-item:hover {
  background: var(--bg-hover);
}

.menu-item-logout:hover {
  background: var(--danger-light);
}

/* ===== User menu transitions ===== */
.user-pop-enter-active {
  transition: opacity 0.2s ease, transform 0.2s cubic-bezier(0.34, 1.56, 0.64, 1);
}
.user-pop-leave-active {
  transition: opacity 0.15s ease, transform 0.15s ease;
}
.user-pop-enter-from,
.user-pop-leave-to {
  opacity: 0;
  transform: translateY(6px);
}

/* ===== Top bar ===== */
.top-bar {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  margin-bottom: 20px;
  min-height: 40px;
}

.top-bar-spacer {
  flex: 1;
}

.top-bar-actions {
  display: flex;
  align-items: center;
  gap: 10px;
}

@media (max-width: 768px) {
  .layout-root {
    width: 100%;
    height: 100vh;
    height: 100dvh; /* 动态视口高度，排除移动端地址栏 */
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .sidebar {
    display: none;
  }

  .app-container {
    padding: 0; /* 移动端取消外层 padding，由各页面自行控制间距 */
    padding-bottom: var(--nav-height);
    width: 100%;
    flex: 1;
    min-height: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
}

@media (min-width: 769px) {
  .layout-root :deep(.bottom-nav) {
    display: none;
  }
}

/* ===== 沉浸式录题模式：隐藏左侧系统导航，主区独享 100% 宽度 ===== */
@media (min-width: 769px) {
  .layout-root.is-immersive .sidebar {
    display: none;
  }

  .layout-root.is-immersive .app-container {
    padding: 0;
  }
}

</style>
