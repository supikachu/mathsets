<template>
  <div class="layout-root">
    <div class="app-container">
      <!-- 桌面侧边栏 -->
      <nav class="sidebar">
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
      </nav>

      <!-- 知识点树（中间栏） -->
      <KpTreePanel v-model:mobile-open="showTreeMobile" />
      <div v-if="showTreeMobile" class="tree-scrim" @click="showTreeMobile = false" />

      <!-- 主内容区 -->
      <div class="main-content">
        <header class="top-bar">
          <button class="tree-toggle" @click="showTreeMobile = true">
            <AppIcon name="tag" :size="18" /><span>知识点</span>
          </button>
          <div class="space-switcher" v-if="space.spaces.length">
            <label class="space-label">题库空间</label>
            <select
              class="space-select"
              :value="space.currentSpaceId"
              @change="onSpaceChange(($event.target as HTMLSelectElement).value)"
            >
              <option v-for="s in space.spaces" :key="s.id" :value="s.id">
                {{ spaceKindLabel(s.kind) }} · {{ s.name }}
              </option>
            </select>
          </div>
          <div class="top-bar-spacer" />
          <div class="top-bar-actions">
            <ThemeToggle />
            <div class="user-menu" ref="userMenuRef">
              <button
                type="button"
                class="user-menu-trigger"
                @click="showUserMenu = !showUserMenu"
              >
                <span class="user-avatar">{{ avatarLetter }}</span>
                <span class="user-name">{{ auth.displayName }}</span>
                <AppIcon name="chevron-down" :size="15" class="user-chevron" />
              </button>
              <div v-if="showUserMenu" class="user-menu-dropdown">
                <div class="user-menu-info">
                  <div class="user-menu-name">{{ auth.displayName }}</div>
                  <div class="user-menu-role">{{ roleLabel }}</div>
                </div>
                <button type="button" class="user-menu-item" @click="handleLogout">
                  <AppIcon name="logout" :size="17" />
                  退出登录
                </button>
              </div>
            </div>
          </div>
        </header>

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
import ThemeToggle from '@/components/ThemeToggle.vue'
import KpTreePanel from '@/components/KpTreePanel.vue'
import { useSelectedKp } from '@/composables/useSelectedKp'

const route = useRoute()
const router = useRouter()
const auth = useAuthStore()
const space = useSpaceStore()
const { items } = useNavItems()
const { clear: clearSelectedKp } = useSelectedKp()

const showUserMenu = ref(false)
const showTreeMobile = ref(false)
const userMenuRef = ref<HTMLElement | null>(null)

const avatarLetter = computed(() =>
  (auth.displayName || '?').charAt(0).toUpperCase(),
)

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
  clearSelectedKp()
  // 切换空间后刷新当前列表类页面
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
  min-height: 100vh;
  background: var(--bg-primary);
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

.user-menu {
  position: relative;
}

.user-menu-trigger {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 12px 5px 5px;
  border-radius: var(--radius-full);
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  box-shadow: var(--shadow-xs);
  color: var(--text-primary);
  font-size: 14px;
  font-weight: 500;
  transition: var(--transition-fast);
}

.user-menu-trigger:hover {
  background: var(--bg-hover);
  box-shadow: var(--shadow-sm);
}

.user-avatar {
  width: 30px;
  height: 30px;
  border-radius: 50%;
  background: var(--accent-gradient);
  color: #fff;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 13px;
  font-weight: 700;
  flex-shrink: 0;
}

.user-name {
  max-width: 120px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.user-chevron {
  color: var(--text-muted);
}

.user-menu-dropdown {
  position: absolute;
  top: calc(100% + 8px);
  right: 0;
  min-width: 200px;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-lg);
  padding: 8px;
  z-index: 150;
  animation: scaleIn 0.2s cubic-bezier(0.34, 1.56, 0.64, 1);
  transform-origin: top right;
}

.user-menu-info {
  padding: 8px 12px 12px;
  border-bottom: 1px solid var(--divider);
  margin-bottom: 4px;
}

.user-menu-name {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
}

.user-menu-role {
  font-size: 12px;
  color: var(--text-secondary);
  margin-top: 2px;
}

.user-menu-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  text-align: left;
  padding: 9px 12px;
  border-radius: var(--radius-sm);
  font-size: 14px;
  color: var(--danger);
  transition: var(--transition-fast);
}

.user-menu-item:hover {
  background: var(--danger-light);
}

@media (max-width: 768px) {
  .sidebar {
    display: none;
  }

  .app-container {
    padding: 16px;
    padding-bottom: calc(var(--nav-height) + 16px);
  }

  .tree-toggle {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 14px;
    border-radius: var(--radius-sm);
    background: var(--bg-card);
    border: 1px solid var(--border-color);
    box-shadow: var(--shadow-xs);
    color: var(--text-secondary);
    font-size: 13px;
    font-weight: 500;
    transition: var(--transition-fast);
  }

  .tree-toggle:hover {
    background: var(--bg-hover);
    color: var(--accent);
  }

  .tree-scrim {
    position: fixed;
    inset: 0;
    background: var(--bg-modal);
    backdrop-filter: var(--blur-modal);
    -webkit-backdrop-filter: var(--blur-modal);
    z-index: 170;
    animation: fadeIn 0.2s ease;
  }

  .user-name {
    display: none;
  }

  .user-menu-trigger {
    padding: 4px;
  }
}

@media (min-width: 769px) {
  .tree-toggle {
    display: none;
  }

  .layout-root :deep(.bottom-nav) {
    display: none;
  }
}
</style>
