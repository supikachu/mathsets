<template>
  <aside class="kp-tree-panel" :class="{ 'mobile-open': mobileOpen }">
    <div class="kp-tree-header">
      <span class="card-title"><AppIcon name="tag" :size="18" /> 知识点</span>
      <div class="kp-header-actions">
        <button
          class="kp-expand-toggle"
          :class="{ expanded: allExpanded }"
          @click="toggleAllExpanded"
          :title="allExpanded ? '全部折叠' : '全部展开'"
        >
          <svg
            class="kp-expand-icon"
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2.5"
            stroke-linecap="round"
            stroke-linejoin="round"
          >
            <path d="m6 9 6 6 6-6" />
          </svg>
        </button>
        <button class="kp-add-root" @click="addRoot">
          <AppIcon name="plus" :size="16" />
        </button>
      </div>
    </div>

    <!-- 学段分段控制器 -->
    <div class="kp-segmented" role="tablist">
      <div class="kp-segmented-indicator" :class="{ 'is-senior': level === 'senior' }" />
      <button
        class="kp-segment"
        :class="{ active: level === 'junior' }"
        role="tab"
        @click="switchLevel('junior')"
      >
        <AppIcon name="book-open" :size="14" :stroke="1.8" />
        <span>初中</span>
      </button>
      <button
        class="kp-segment"
        :class="{ active: level === 'senior' }"
        role="tab"
        @click="switchLevel('senior')"
      >
        <AppIcon name="compass" :size="14" :stroke="1.8" />
        <span>高中</span>
      </button>
    </div>

    <div v-if="loading" class="loading-hint">加载中…</div>
    <AppEmpty v-else-if="displayedTree.length === 0" description="暂无知识点" icon="tag" />

    <div v-else class="kp-tree">
      <KpTreeNode
        v-for="node in displayedTree"
        :key="node.id"
        :node="node"
        :level="0"
        :selected-kp-id="selectedKpId"
        :expanded="expanded"
        @select="onNodeSelect"
        @toggle-expand="toggleExpand"
        @edit="openEdit"
        @add-child="addChild"
        @delete="openDelete"
      />
    </div>

    <!-- 编辑弹窗 -->
    <AppModal v-model="editDialog" title="编辑节点">
      <div class="form-group">
        <label class="form-label">节点名称 *</label>
        <input v-model="editForm.name" placeholder="输入名称" />
      </div>
      <div class="form-group">
        <label class="form-label">适用年级</label>
        <AppSelect v-model="editForm.grade" placeholder="选择年级" clearable :options="gradeOptions" />
      </div>
      <div class="form-group">
        <label class="form-label">排序号</label>
        <input type="number" v-model.number="editForm.sort_order" min="0" max="999" />
      </div>
      <div class="form-actions">
        <AppButton variant="ghost" @click="editDialog = false">取消</AppButton>
        <AppButton variant="primary" :loading="saving" @click="saveEdit"><AppIcon name="save" :size="17" /> 保存</AppButton>
      </div>
    </AppModal>

    <!-- 添加弹窗 -->
    <AppModal v-model="addDialog" :title="addDialogTitle">
      <div class="form-group">
        <label class="form-label">名称 *</label>
        <input v-model="addForm.name" placeholder="节点名称" />
      </div>
      <div class="form-group">
        <label class="form-label">适用年级</label>
        <AppSelect v-model="addForm.grade" placeholder="选择年级" clearable :options="gradeOptions" />
      </div>
      <div class="form-group">
        <label class="form-label">排序号</label>
        <input type="number" v-model.number="addForm.sort_order" min="0" max="999" />
      </div>
      <div class="form-actions">
        <AppButton variant="ghost" @click="addDialog = false">取消</AppButton>
        <AppButton variant="primary" :loading="adding" @click="confirmAdd">确认添加</AppButton>
      </div>
    </AppModal>

    <!-- 删除确认 -->
    <AppConfirm
      v-model="deleteDialog"
      title="确认删除"
      message="确认删除此节点？子节点会一并被删除"
      confirm-text="删除"
      danger
      @confirm="deleteNode"
    />
  </aside>
</template>

<script setup lang="ts">
import { ref, reactive, computed, onMounted, watch } from 'vue'
import { kpApi, type KnowledgePoint } from '@/api/client'
import client from '@/api/client'
import { AppButton, AppSelect, AppEmpty, AppModal, AppConfirm, AppIcon } from '@/components/ui'
import KpTreeNode from '@/components/KpTreeNode.vue'
import { useToast } from '@/composables/useToast'
import { useSelectedKp } from '@/composables/useSelectedKp'
import { useSpaceStore } from '@/stores/space'

defineProps<{ mobileOpen?: boolean }>()
const emit = defineEmits<{ 'update:mobileOpen': [value: boolean] }>()

const toast = useToast()
const { selectedKpId, select, kpLevel, setLevel } = useSelectedKp()
const space = useSpaceStore()

const loading = ref(true)
const saving = ref(false)
const adding = ref(false)
const tree = ref<KnowledgePoint[]>([])
const expanded = ref<Record<string, boolean>>({})
const level = kpLevel
const grades = ['初一', '初二', '初三', '高一', '高二', '高三']
const gradeOptions = grades.map((g) => ({ label: g, value: g }))

// 编辑
const editDialog = ref(false)
const editingNode = ref<KnowledgePoint | null>(null)
const editForm = reactive({
  name: '',
  grade: undefined as string | undefined,
  sort_order: 0,
})

// 添加
const addDialog = ref(false)
const addParent = ref<KnowledgePoint | null>(null)
const addForm = reactive({
  name: '',
  grade: undefined as string | undefined,
  sort_order: 0,
})

// 删除
const deleteDialog = ref(false)
const deletingNode = ref<KnowledgePoint | null>(null)

// 递归查找节点
function findNodeByName(nodes: KnowledgePoint[], name: string): KnowledgePoint | null {
  for (const node of nodes) {
    if (node.name === name) return node
    if (node.children?.length) {
      const found = findNodeByName(node.children, name)
      if (found) return found
    }
  }
  return null
}

// 当前学段对应的节点（初中 / 高中）
const levelNode = computed<KnowledgePoint | null>(() => {
  const targetName = level.value === 'junior' ? '初中' : '高中'
  return findNodeByName(tree.value, targetName)
})

// 实际展示的树：学段节点的子树，若学段节点不存在则回退到全部根节点
const displayedTree = computed<KnowledgePoint[]>(() => {
  if (levelNode.value) {
    return levelNode.value.children ?? []
  }
  return tree.value
})

// 添加弹窗标题
const addDialogTitle = computed(() => {
  if (addParent.value) {
    return `在「${addParent.value.name}」下添加子节点`
  }
  return level.value === 'junior' ? '添加初中知识点' : '添加高中知识点'
})

async function fetchTree() {
  loading.value = true
  try {
    const res = await kpApi.tree(space.currentSpaceId || undefined)
    tree.value = res.data
    // 设置默认展开状态
    const targetName = level.value === 'junior' ? '初中' : '高中'
    const lvNode = findNodeByName(tree.value, targetName)
    setDefaultExpanded(lvNode?.children ?? tree.value)
  } catch {
    /* handled */
  } finally {
    loading.value = false
  }
}

function switchLevel(lv: 'junior' | 'senior') {
  if (level.value === lv) return
  setLevel(lv)
  // 切换学段时清除知识点选中并重置展开状态
  select(null)
  const targetName = lv === 'junior' ? '初中' : '高中'
  const lvNode = findNodeByName(tree.value, targetName)
  setDefaultExpanded(lvNode?.children ?? tree.value)
}

function toggleExpand(node: KnowledgePoint) {
  expanded.value = { ...expanded.value, [node.id]: !expanded.value[node.id] }
}

// 递归收集所有有子节点的 ID
function collectAllParentIds(nodes: KnowledgePoint[]): string[] {
  const ids: string[] = []
  for (const n of nodes) {
    if (n.children?.length) {
      ids.push(n.id)
      ids.push(...collectAllParentIds(n.children))
    }
  }
  return ids
}

const allExpanded = computed(() => {
  const allIds = collectAllParentIds(displayedTree.value)
  if (allIds.length === 0) return false
  return allIds.every((id) => expanded.value[id] === true)
})

function toggleAllExpanded() {
  const allIds = collectAllParentIds(displayedTree.value)
  if (allExpanded.value) {
    // 全部折叠
    const next: Record<string, boolean> = {}
    for (const id of allIds) next[id] = false
    expanded.value = next
  } else {
    // 全部展开
    const next: Record<string, boolean> = {}
    for (const id of allIds) next[id] = true
    expanded.value = next
  }
}

// 设置默认展开状态：仅展开第一层节点
function setDefaultExpanded(nodes: KnowledgePoint[]) {
  const next: Record<string, boolean> = {}
  for (const n of nodes) {
    if (n.children?.length) {
      next[n.id] = true
    }
  }
  expanded.value = next
}

function onNodeSelect(node: KnowledgePoint) {
  if (selectedKpId.value === node.id) {
    select(null)
  } else {
    select(node.id, node.name)
  }
  emit('update:mobileOpen', false)
}

function openEdit(node: KnowledgePoint) {
  editingNode.value = node
  editForm.name = node.name
  editForm.grade = node.grade || undefined
  editForm.sort_order = node.sort_order
  editDialog.value = true
}

function addRoot() {
  // 当学段节点存在时，添加到学段节点下；否则添加为根节点
  addParent.value = levelNode.value ?? null
  addForm.name = ''
  addForm.grade = undefined
  addForm.sort_order = 0
  addDialog.value = true
}

function addChild(parent: KnowledgePoint) {
  addParent.value = parent
  addForm.name = ''
  addForm.grade = undefined
  addForm.sort_order = 0
  addDialog.value = true
}

function openDelete(node: KnowledgePoint) {
  deletingNode.value = node
  deleteDialog.value = true
}

async function confirmAdd() {
  if (!addForm.name.trim()) {
    toast.warning('请输入名称')
    return
  }
  adding.value = true
  try {
    await client.post('/knowledge-points', {
      parent_id: addParent.value?.id || null,
      name: addForm.name.trim(),
      grade: addForm.grade || null,
      sort_order: addForm.sort_order,
      space_id: space.currentSpaceId || null,
    })
    toast.success('添加成功')
    addDialog.value = false
    await fetchTree()
  } catch (e: any) {
    toast.error(e.response?.data?.error || '添加失败')
  } finally {
    adding.value = false
  }
}

async function saveEdit() {
  if (!editingNode.value || !editForm.name.trim()) {
    toast.warning('请输入名称')
    return
  }
  saving.value = true
  try {
    await client.put(`/knowledge-points/${editingNode.value.id}`, {
      name: editForm.name.trim(),
      grade: editForm.grade || null,
      sort_order: editForm.sort_order,
    })
    toast.success('已保存')
    editDialog.value = false
    await fetchTree()
  } catch (e: any) {
    toast.error(e.response?.data?.error || '保存失败')
  } finally {
    saving.value = false
  }
}

async function deleteNode() {
  if (!deletingNode.value) return
  try {
    await client.delete(`/knowledge-points/${deletingNode.value.id}`)
    toast.success('已删除')
    if (selectedKpId.value === deletingNode.value.id) {
      select(null)
    }
    await fetchTree()
  } catch (e: any) {
    toast.error(e.response?.data?.error || '删除失败')
  }
}

watch(() => space.currentSpaceId, () => {
  fetchTree()
})

onMounted(fetchTree)
</script>

<style scoped>
.kp-tree-panel {
  display: flex;
  flex-direction: column;
}

.kp-tree-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 4px 12px;
  border-bottom: 1px solid var(--divider);
  margin-bottom: 8px;
}

.card-title {
  font-size: 14px;
  font-weight: 700;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  letter-spacing: -0.01em;
}

.kp-header-actions {
  display: flex;
  align-items: center;
  gap: 4px;
}

.kp-expand-toggle {
  width: 28px;
  height: 28px;
  border-radius: var(--radius-sm);
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  color: var(--text-muted);
  transition: background 0.2s ease, color 0.2s ease;
}

.kp-expand-icon {
  transition: transform 0.4s cubic-bezier(0.34, 1.56, 0.64, 1);
  transform: rotate(0deg);
}

.kp-expand-toggle.expanded .kp-expand-icon {
  transform: rotate(180deg);
}

.kp-expand-toggle:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.kp-expand-toggle:hover .kp-expand-icon {
  color: var(--accent);
}

.kp-expand-toggle:active {
  transform: scale(0.9);
}

.kp-add-root {
  width: 28px;
  height: 28px;
  border-radius: var(--radius-sm);
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--accent-light);
  color: var(--accent);
  transition: var(--transition-fast);
}

.kp-add-root:hover {
  background: var(--accent);
  color: #fff;
}

.kp-add-root:active {
  transform: scale(0.9);
}

/* 学段分段控制器 — iOS-style sliding indicator */
.kp-segmented {
  position: relative;
  display: flex;
  padding: 3px;
  background: var(--bg-input);
  border-radius: var(--radius-sm);
  border: 0.5px solid var(--border-color);
  margin-bottom: 12px;
  box-shadow: inset 0 0.5px 1px rgba(0, 0, 0, 0.03);
}

.kp-segmented-indicator {
  position: absolute;
  top: 3px;
  left: 3px;
  width: calc(50% - 3px);
  height: calc(100% - 6px);
  background: var(--bg-card);
  border-radius: 8px;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.08), 0 1px 1px rgba(0, 0, 0, 0.04);
  transition: transform 0.42s cubic-bezier(0.34, 1.56, 0.64, 1);
  z-index: 0;
}

.kp-segmented-indicator.is-senior {
  transform: translateX(100%);
}

.kp-segment {
  position: relative;
  z-index: 1;
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 5px;
  padding: 7px 0;
  border: none;
  background: transparent;
  border-radius: 8px;
  font-size: 13px;
  font-weight: 500;
  color: var(--text-secondary);
  transition: color 0.2s ease, font-weight 0.2s ease;
  letter-spacing: -0.01em;
  cursor: pointer;
}

.kp-segment svg {
  opacity: 0.6;
  transition: opacity 0.2s ease, transform 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
  transform: scale(0.9);
}

.kp-segment:hover:not(.active) {
  color: var(--text-primary);
}

.kp-segment:hover:not(.active) svg {
  opacity: 0.85;
  transform: scale(1);
}

.kp-segment.active {
  color: var(--accent);
  font-weight: 600;
}

.kp-segment.active svg {
  opacity: 1;
  transform: scale(1);
}

.kp-segment:active {
  transform: scale(0.96);
}

.loading-hint {
  text-align: center;
  padding: 24px 12px;
  color: var(--text-muted);
  font-size: 13px;
}

.kp-tree {
  flex: 1;
  overflow-y: auto;
  margin: 0 -4px;
  padding: 0 4px;
}
</style>
