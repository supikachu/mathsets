<template>
  <div class="layout-root">
    <div class="app-container">
      <!-- 桌面侧边栏（导航卡片 + 用户卡片，两个独立卡片透出背景色） -->
      <nav class="sidebar">
        <!-- 上方：导航卡片 -->
        <div class="sidebar-nav-card">
          <div class="sidebar-brand">
            <AppIcon name="logo" :size="24" class="brand-icon" />
            <span>协同题库</span>
          </div>
          <router-link
            v-for="item in items"
            :key="item.path"
            :to="item.path"
            class="nav-item"
            :class="{ active: isActive(item.path) }"
          >
            <AppIcon :name="item.icon" :size="19" />
            <span>{{ item.label }}</span>
          </router-link>
        </div>

        <!-- 下方：用户信息卡片（独立，与导航卡片之间透出背景色） -->
        <div class="sidebar-user-card" ref="userMenuRef">
          <button
            type="button"
            class="sidebar-user-trigger"
            @click="showUserMenu = !showUserMenu"
          >
            <span class="user-avatar-wrap">
              <img
                v-if="avatarSrc"
                :src="avatarSrc"
                class="user-avatar-img"
                alt="头像"
              />
              <span v-else class="user-avatar-letter">{{ avatarLetter }}</span>
            </span>
            <div class="sidebar-user-info">
              <span class="sidebar-user-name">{{ auth.displayName }}</span>
              <span class="sidebar-user-role">{{ roleLabel }}</span>
            </div>
            <AppIcon name="chevron-down" :size="14" class="sidebar-user-chevron" :class="{ rotated: showUserMenu }" />
          </button>

          <Transition name="user-pop">
            <div v-if="showUserMenu" class="sidebar-user-dropdown">
              <button type="button" class="user-menu-item menu-item-profile" @click="goProfile">
                <AppIcon name="user" :size="16" />
                个人中心
              </button>
              <button type="button" class="user-menu-item menu-item-logout" @click="handleLogout">
                <AppIcon name="logout" :size="16" />
                退出登录
              </button>
            </div>
          </Transition>
        </div>
      </nav>

      <!-- 主内容区 -->
      <div class="main-content">
        <div class="view active">
          <router-view />
        </div>
      </div>
    </div>

    <!-- 移动端底部导航 -->
    <BottomNav />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import { useSpaceStore } from '@/stores/space'
import { useNavItems } from '@/composables/useNavItems'
import { AppIcon } from '@/components/ui'
import BottomNav from '@/components/BottomNav.vue'

const route = useRoute()
const router = useRouter()
const auth = useAuthStore()
const space = useSpaceStore()
const { items } = useNavItems()

const showUserMenu = ref(false)
const userMenuRef = ref<HTMLElement | null>(null)

const avatarLetter = computed(() =>
  (auth.displayName || '?').charAt(0).toUpperCase(),
)

/// 头像 URL — 来自 Pinia store，profile 页修改后自动热更新
const avatarSrc = computed(() => auth.avatarUrl)

const roleLabel = computed(() => {
  const map: Record<string, string> = {
    Admin: '系统管理员',
    admin: '系统管理员',
    User: '用户',
    user: '用户',
  }
  return map[auth.role] || auth.role || '用户'
})

function spaceKindLabel(kind: string) {
  if (kind === 'personal') return '个人'
  if (kind === 'team') return '团队'
  if (kind === 'public') return '公共'
  return kind
}

function onSpaceChange(id: string) {
  space.setCurrentSpace(id)
  if (route.path.startsWith('/questions') || route.path === '/review') {
    router.replace({ path: route.path, query: { ...route.query, _sp: id.slice(0, 8) } })
  }
}

function isActive(path: string) {
  return route.path === path || route.path.startsWith(path + '/')
}

function handleLogout() {
  showUserMenu.value = false
  auth.logout()
}

function goProfile() {
  showUserMenu.value = false
  router.push('/profile')
}

function onDocumentClick(e: MouseEvent) {
  if (!showUserMenu.value) return
  const el = userMenuRef.value
  if (el && !el.contains(e.target as Node)) {
    showUserMenu.value = false
  }
}

onMounted(() => {
  document.addEventListener('click', onDocumentClick)
  if (auth.isLoggedIn) {
    space.fetchSpaces()
  }
})
watch(
  () => auth.isLoggedIn,
  (v) => {
    if (v) space.fetchSpaces()
  },
)
onUnmounted(() => document.removeEventListener('click', onDocumentClick))
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
  gap: 9px;
  font-size: 17px;
  font-weight: 700;
  padding: 6px 10px 18px;
  color: var(--text-primary);
  letter-spacing: -0.02em;
}

.brand-icon {
  color: var(--accent);
}

/* ===== Sidebar user card ===== */
.sidebar-user-card {
  position: relative;
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
    height: auto;
    overflow: visible;
  }

  .sidebar {
    display: none;
  }

  .app-container {
    padding: 16px;
    padding-bottom: calc(var(--nav-height) + 16px);
    width: 100%;
    height: auto;
    overflow: visible;
  }
}

@media (min-width: 769px) {
  .layout-root :deep(.bottom-nav) {
    display: none;
  }
}
</style>
