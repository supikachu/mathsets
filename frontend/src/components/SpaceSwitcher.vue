<template>
  <div class="space-switcher" ref="switcherRef">
    <button
      type="button"
      class="ss-trigger"
      :class="{ active: open }"
      @click="open = !open"
    >
      <span class="ss-icon" :class="`ss-icon--${currentKind}`">
        <AppIcon :name="kindIcon(currentKind)" :size="15" />
      </span>
      <div class="ss-info">
        <span class="ss-label">当前空间</span>
        <span class="ss-name">{{ currentName }}</span>
      </div>
      <!-- 团队空间：显示设置入口 -->
      <span
        v-if="currentKind === 'team'"
        class="ss-settings-btn"
        title="空间设置"
        @click.stop="goSettings"
      >
        <AppIcon name="settings" :size="14" />
      </span>
      <AppIcon
        name="chevron-down"
        :size="13"
        class="ss-chevron"
        :class="{ rotated: open }"
      />
    </button>

    <Transition name="ss-pop">
      <div v-if="open" class="ss-dropdown">
        <template v-for="group in groupedSpaces" :key="group.label">
          <div v-if="group.items.length > 0" class="ss-section">
            <div class="ss-section-label">{{ group.label }}</div>
            <button
              v-for="s in group.items"
              :key="s.id"
              type="button"
              class="ss-item"
              :class="{ active: s.id === currentId }"
              @click="select(s.id)"
            >
              <span class="ss-item-icon" :class="`ss-item-icon--${s.kind}`">
                <AppIcon :name="kindIcon(s.kind)" :size="14" />
              </span>
              <span class="ss-item-name">{{ s.name }}</span>
              <!-- 团队空间项：hover 显示设置图标 -->
              <span
                v-if="s.kind === 'team'"
                class="ss-item-settings"
                title="空间设置"
                @click.stop="goSettingsById(s.id)"
              >
                <AppIcon name="settings" :size="12" />
              </span>
              <AppIcon
                v-if="s.id === currentId"
                name="check"
                :size="13"
                class="ss-item-check"
              />
            </button>
          </div>
        </template>

        <!-- 创建团队空间 -->
        <div class="ss-create-section">
          <button type="button" class="ss-create-btn" @click="showCreateModal = true">
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

// ── 跳转空间设置 ──
function goSettings() {
  if (!currentId.value) return
  open.value = false
  router.push(`/spaces/${currentId.value}/settings`)
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

// 弹窗打开时自动聚焦输入框
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
.space-switcher {
  position: relative;
  margin: 0 6px 14px;
}

/* ===== Trigger ===== */
.ss-trigger {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  padding: 8px 10px;
  border-radius: var(--radius-xs);
  background: var(--bg-hover);
  border: 1px solid transparent;
  cursor: pointer;
  transition: var(--transition-fast);
}

.ss-trigger:hover {
  background: var(--bg-secondary);
}

.ss-trigger.active {
  background: var(--bg-secondary);
  border-color: var(--border-color);
}

.ss-icon {
  width: 28px;
  height: 28px;
  border-radius: 7px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  color: #fff;
}

.ss-icon--personal {
  background: linear-gradient(135deg, #5b8def, #4178d6);
}

.ss-icon--team {
  background: linear-gradient(135deg, #34c759, #2da44e);
}

.ss-icon--public {
  background: linear-gradient(135deg, #ff9500, #e68600);
}

.ss-info {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 0;
  overflow: hidden;
  min-width: 0;
}

.ss-label {
  font-size: 10px;
  color: var(--text-muted);
  letter-spacing: 0.02em;
  line-height: 1.2;
}

.ss-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 100%;
  line-height: 1.3;
}

.ss-chevron {
  color: var(--text-muted);
  flex-shrink: 0;
  transition: transform 0.25s cubic-bezier(0.4, 0, 0.2, 1);
}

.ss-chevron.rotated {
  transform: rotate(180deg);
}

/* ===== Dropdown ===== */
.ss-dropdown {
  position: absolute;
  top: calc(100% + 6px);
  left: 0;
  right: 0;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  box-shadow: var(--shadow-md);
  padding: 6px;
  z-index: 150;
  max-height: 360px;
  overflow-y: auto;
  overscroll-behavior: contain;
}

.ss-section + .ss-section {
  margin-top: 4px;
  padding-top: 4px;
  border-top: 1px solid var(--border-light);
}

.ss-section-label {
  font-size: 10px;
  font-weight: 600;
  color: var(--text-muted);
  padding: 4px 8px 3px;
  letter-spacing: 0.04em;
  text-transform: uppercase;
}

.ss-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  text-align: left;
  padding: 7px 8px;
  border-radius: var(--radius-xs);
  font-size: 13px;
  color: var(--text-primary);
  transition: var(--transition-fast);
  cursor: pointer;
}

.ss-item:hover {
  background: var(--bg-hover);
}

.ss-item.active {
  background: var(--accent-light);
  color: var(--accent);
  font-weight: 600;
}

.ss-item-icon {
  width: 22px;
  height: 22px;
  border-radius: 5px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  color: #fff;
}

.ss-item-icon--personal {
  background: linear-gradient(135deg, #5b8def, #4178d6);
}

.ss-item-icon--team {
  background: linear-gradient(135deg, #34c759, #2da44e);
}

.ss-item-icon--public {
  background: linear-gradient(135deg, #ff9500, #e68600);
}

.ss-item-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ss-item-check {
  color: var(--accent);
  flex-shrink: 0;
}

/* ===== Settings gear in trigger ===== */
.ss-settings-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border-radius: var(--radius-xs);
  color: var(--text-muted);
  flex-shrink: 0;
  transition: var(--transition-fast);
}

.ss-settings-btn:hover {
  background: var(--bg-secondary);
  color: var(--accent);
}

/* ===== Settings icon in dropdown items ===== */
.ss-item-settings {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border-radius: 4px;
  color: var(--text-muted);
  opacity: 0;
  transition: var(--transition-fast);
  flex-shrink: 0;
}

.ss-item:hover .ss-item-settings {
  opacity: 1;
}

.ss-item-settings:hover {
  background: var(--accent-light);
  color: var(--accent);
}

/* ===== Create team space ===== */
.ss-create-section {
  margin-top: 4px;
  padding-top: 6px;
  border-top: 1px solid var(--border-light);
}

.ss-create-btn {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 8px 8px;
  border-radius: var(--radius-xs);
  font-size: 13px;
  font-weight: 600;
  color: var(--accent);
  transition: var(--transition-fast);
}

.ss-create-btn:hover {
  background: var(--accent-light);
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
