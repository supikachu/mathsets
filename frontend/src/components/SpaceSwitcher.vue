<template>
  <div class="space-switcher relative inline-block shrink-0" ref="switcherRef">
    <!-- 极简 Avatar 纯圆形触发按钮（带有手写自定义 Tooltip） -->
    <div class="group relative inline-block">
      <button
        type="button"
        class="ss-trigger w-9 h-9 rounded-full bg-gray-100 dark:bg-slate-800 hover:bg-gray-200 dark:hover:bg-slate-700 flex items-center justify-center shrink-0 cursor-pointer transition-colors border border-transparent"
        :class="{ 'bg-gray-200 dark:bg-slate-700': open }"
        @click="open = !open"
      >
        <!-- 空间 Icon/头像 -->
        <span class="ss-icon flex items-center justify-center shrink-0" :class="`ss-icon--${currentKind}`">
          <AppIcon :name="kindIcon(currentKind)" :size="16" />
        </span>
      </button>

      <!-- 自定义现代 Tooltip：下拉未打开且 Hover 时显现 -->
      <div
        v-if="!open"
        class="absolute right-0 top-full mt-2 pointer-events-none px-2.5 py-1.5 bg-gray-900/90 dark:bg-slate-800/95 text-white text-xs font-medium rounded-lg opacity-0 invisible group-hover:opacity-100 group-hover:visible transition-all duration-200 z-50 shadow-xl whitespace-nowrap backdrop-blur-sm"
      >
        {{ '当前空间：' + (currentName || '切换空间') }}
      </div>
    </div>

    <!-- 现代化的下拉面板 (Vercel / Notion 极简质感) -->
    <Transition name="ss-pop">
      <div
        v-if="open"
        class="ss-dropdown absolute right-0 mt-2 w-64 bg-white dark:bg-slate-900 rounded-xl shadow-xl border border-gray-100 dark:border-slate-800 overflow-hidden z-50 py-1"
      >
        <template v-for="group in groupedSpaces" :key="group.label">
          <div v-if="group.items.length > 0" class="ss-section">
            <!-- 分组标题：极简大写微型字体 -->
            <div class="px-3 pt-3 pb-1 text-xs font-semibold text-gray-400 uppercase tracking-wider">
              {{ group.label }}
            </div>
            <button
              v-for="s in group.items"
              :key="s.id"
              type="button"
              class="ss-item group/item flex items-center justify-between w-full px-3 py-2 text-sm text-gray-700 dark:text-gray-200 cursor-pointer hover:bg-gray-50 dark:hover:bg-slate-800/60 transition-colors rounded-md"
              :class="{ 'font-semibold': s.id === currentId }"
              @click="select(s.id)"
            >
              <div class="flex items-center gap-2.5 min-w-0 pr-2">
                <span class="ss-item-icon flex-shrink-0" :class="`ss-item-icon--${s.kind}`">
                  <AppIcon :name="kindIcon(s.kind)" :size="14" />
                </span>
                <span class="truncate">{{ s.name }}</span>
              </div>
              <div class="flex items-center gap-1.5 flex-shrink-0">
                <span
                  v-if="s.kind === 'team'"
                  class="ss-item-settings opacity-0 group-hover/item:opacity-100 p-1 hover:text-blue-500 rounded transition-opacity"
                  title="空间设置"
                  @click.stop="goSettingsById(s.id)"
                >
                  <AppIcon name="settings" :size="13" />
                </span>
                <AppIcon
                  v-if="s.id === currentId"
                  name="check"
                  :size="15"
                  class="text-blue-500 dark:text-blue-400"
                />
              </div>
            </button>
          </div>
        </template>

        <!-- 底部固定操作：创建团队空间 -->
        <div class="border-t border-gray-100 dark:border-slate-800 bg-gray-50/60 dark:bg-slate-800/40 p-1 mt-1">
          <button
            type="button"
            class="flex items-center gap-2 w-full px-3 py-2 text-sm text-gray-600 dark:text-gray-300 hover:text-gray-900 dark:hover:text-white hover:bg-gray-200/50 dark:hover:bg-slate-700/50 rounded-lg transition-colors"
            @click="showCreateModal = true"
          >
            <AppIcon name="plus" :size="14" />
            创建团队空间
          </button>
        </div>
      </div>
    </Transition>

    <!-- 创建团队空间弹窗 -->
    <Teleport to="body">
      <Transition name="ss-modal-fade">
        <div v-if="showCreateModal" class="ss-modal-overlay" @click="showCreateModal = false">
          <div class="ss-modal" @click.stop>
            <h3 class="ss-modal-title">创建团队空间</h3>
            <input
              v-model="newSpaceName"
              class="ss-modal-input"
              placeholder="输入空间名称（如：高三数学组）"
              @keyup.enter="createTeam"
              ref="createInputRef"
            />
            <div class="ss-modal-actions">
              <button class="ss-modal-btn ss-modal-btn--ghost" @click="showCreateModal = false">取消</button>
              <button
                class="ss-modal-btn ss-modal-btn--primary"
                :disabled="!newSpaceName.trim() || creating"
                @click="createTeam"
              >
                {{ creating ? '创建中…' : '创建' }}
              </button>
            </div>
          </div>
        </div>
      </Transition>
    </Teleport>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, nextTick, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useSpaceStore } from '@/stores/space'
import { spaceApi } from '@/api/client'
import { AppIcon } from '@/components/ui'
import { useToast } from '@/composables/useToast'
import type { SpaceSummary } from '@/api/client'

const router = useRouter()
const space = useSpaceStore()
const toast = useToast()
const open = ref(false)
const switcherRef = ref<HTMLElement | null>(null)

const currentId = computed(() => space.currentSpaceId)
const currentSpace = computed(() => space.currentSpace)
const currentName = computed(() => currentSpace.value?.name || '未选择')
const currentKind = computed(() => currentSpace.value?.kind || 'personal')

function kindIcon(kind: string): string {
  if (kind === 'personal') return 'user'
  if (kind === 'team') return 'users'
  if (kind === 'public') return 'globe'
  return 'folder'
}

interface SpaceGroup {
  label: string
  items: SpaceSummary[]
}

const groupedSpaces = computed<SpaceGroup[]>(() => {
  const personal = space.spaces.filter((s) => s.kind === 'personal')
  const team = space.spaces.filter((s) => s.kind === 'team')
  const publicSpace = space.spaces.filter((s) => s.kind === 'public')
  return [
    { label: '个人', items: personal },
    { label: '团队', items: team },
    { label: '公共', items: publicSpace },
  ]
})

function select(id: string) {
  space.setCurrentSpace(id)
  open.value = false
}

function goSettingsById(id: string) {
  open.value = false
  router.push(`/spaces/${id}/settings`)
}

// ── 创建团队空间 ──
const showCreateModal = ref(false)
const newSpaceName = ref('')
const creating = ref(false)
const createInputRef = ref<HTMLInputElement | null>(null)

async function createTeam() {
  const name = newSpaceName.value.trim()
  if (!name) return
  creating.value = true
  try {
    await spaceApi.createTeam(name)
    toast.success('团队空间创建成功')
    showCreateModal.value = false
    newSpaceName.value = ''
    await space.fetchSpaces()
  } catch (e: any) {
    toast.error(e.response?.data?.error || e.response?.data?.message || e.message || '创建失败')
  } finally {
    creating.value = false
  }
}

watch(showCreateModal, (v) => {
  if (v) {
    nextTick(() => createInputRef.value?.focus())
  }
})

function onDocumentClick(e: MouseEvent) {
  if (!open.value) return
  const el = switcherRef.value
  if (el && !el.contains(e.target as Node)) {
    open.value = false
  }
}

onMounted(() => document.addEventListener('click', onDocumentClick))
onUnmounted(() => document.removeEventListener('click', onDocumentClick))
</script>

<style scoped>
.ss-icon {
  width: 26px;
  height: 26px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  color: #fff;
}

.ss-item-icon {
  width: 22px;
  height: 22px;
  border-radius: 6px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  color: #fff;
}

.ss-icon--personal,
.ss-item-icon--personal {
  background: linear-gradient(135deg, #5b8def, #4178d6);
}

.ss-icon--team,
.ss-item-icon--team {
  background: linear-gradient(135deg, #34c759, #2da44e);
}

.ss-icon--public,
.ss-item-icon--public {
  background: linear-gradient(135deg, #ff9500, #e68600);
}

/* ===== Create modal ===== */
.ss-modal-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.25);
  backdrop-filter: blur(2px);
  z-index: 300;
  display: flex;
  align-items: center;
  justify-content: center;
}

.ss-modal {
  width: 360px;
  max-width: 90vw;
  background: var(--bg-card);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-lg);
  padding: 24px;
}

.ss-modal-title {
  font-size: 17px;
  font-weight: 700;
  color: var(--text-primary);
  margin: 0 0 16px;
  letter-spacing: -0.02em;
}

.ss-modal-input {
  width: 100%;
  padding: 10px 14px;
  border-radius: var(--radius-xs);
  background: var(--bg-input);
  border: 1px solid transparent;
  font-size: 14px;
  color: var(--text-primary);
  outline: none;
  transition: var(--transition-fast);
}

.ss-modal-input:focus {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-ring);
}

.ss-modal-actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  margin-top: 16px;
}

.ss-modal-btn {
  font-size: 13px;
  font-weight: 600;
  padding: 8px 20px;
  border-radius: var(--radius-xs);
  transition: var(--transition-fast);
}

.ss-modal-btn--ghost {
  color: var(--text-secondary);
  background: transparent;
}

.ss-modal-btn--ghost:hover {
  background: var(--bg-hover);
}

.ss-modal-btn--primary {
  color: #fff;
  background: var(--accent);
}

.ss-modal-btn--primary:hover:not(:disabled) {
  background: var(--accent-hover);
}

.ss-modal-btn--primary:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

/* ===== Modal transition ===== */
.ss-modal-fade-enter-active,
.ss-modal-fade-leave-active {
  transition: opacity 0.2s ease;
}

.ss-modal-fade-enter-from,
.ss-modal-fade-leave-to {
  opacity: 0;
}

/* ===== Transition ===== */
.ss-pop-enter-active {
  transition: opacity 0.18s ease, transform 0.18s cubic-bezier(0.34, 1.56, 0.64, 1);
}
.ss-pop-leave-active {
  transition: opacity 0.12s ease, transform 0.12s ease;
}
.ss-pop-enter-from,
.ss-pop-leave-to {
  opacity: 0;
  transform: translateY(-4px) scale(0.97);
}
</style>
