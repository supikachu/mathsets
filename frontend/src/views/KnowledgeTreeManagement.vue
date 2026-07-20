<script setup lang="ts">
/**
 * KnowledgeTreeManagement — 知识树管理后台
 *
 * 布局：顶部知识树切换 Tab + 左侧节点树列表 + 右侧节点详情编辑面板
 *
 * 严格遵循项目布局规范：
 * - 页面容器使用 `position: absolute; inset: 0; display: flex; flex-direction: column`
 * - 滚动区域使用 `flex: 1; min-height: 0; overflow-y: auto; overscroll-behavior: contain`
 * - 复用 AppButton / AppIcon / AppModal / AppConfirm / AppEmpty，不引入第三方 UI
 * - 全部使用 CSS 变量（--bg-card / --border-color / --text-primary 等）
 */
import { ref, computed, reactive, watch, onMounted } from 'vue'
import {
  knowledgeTreeApi,
  knowledgeNodeApi,
  type KnowledgeTree,
  type KnowledgeNodeTreeNode,
  type KnowledgeTreeKind,
} from '@/api/client'
import { AppIcon, AppButton, AppModal, AppConfirm, AppEmpty } from '@/components/ui'
import { useToast } from '@/composables/useToast'

const toast = useToast()

// ─── 知识树列表 ────────────────────────────────────────────────────────
const trees = ref<KnowledgeTree[]>([])
const activeTreeId = ref<string>('')
const loading = ref(false)

const activeTree = computed(() =>
  trees.value.find((t) => t.id === activeTreeId.value),
)

async function loadTrees() {
  loading.value = true
  try {
    const res = await knowledgeTreeApi.list()
    trees.value = res.data
    if (trees.value.length > 0 && !activeTreeId.value) {
      activeTreeId.value = trees.value[0].id
    }
  } catch (e: any) {
    toast.error(e.response?.data?.error || '加载知识树失败')
  } finally {
    loading.value = false
  }
}

// ─── 节点树（左侧） ────────────────────────────────────────────────────
const treeData = ref<KnowledgeNodeTreeNode[]>([])
const searchText = ref('')
const expandedIds = ref<Set<string>>(new Set())
const selectedNodeId = ref<string>('')

interface FlatItem {
  node: KnowledgeNodeTreeNode
  depth: number
}

const filteredNodes = computed<FlatItem[]>(() => {
  const result: FlatItem[] = []
  const q = searchText.value.trim().toLowerCase()

  function walk(nodes: KnowledgeNodeTreeNode[], depth: number, ancestorMatched: boolean) {
    for (const n of nodes) {
      const name = n.name.toLowerCase()
      const code = (n.code || '').toLowerCase()
      const matched = !q || name.includes(q) || code.includes(q)
      if (matched || ancestorMatched) {
        result.push({ node: n, depth })
      }
      const shouldRecurse =
        matched || ancestorMatched || (q ? false : expandedIds.value.has(n.id))
      if (shouldRecurse && n.children.length > 0) {
        walk(n.children, depth + 1, matched || ancestorMatched)
      }
    }
  }

  // 非搜索态：尊重 expandedIds
  if (!q) {
    function walkNormal(nodes: KnowledgeNodeTreeNode[], depth: number) {
      for (const n of nodes) {
        result.push({ node: n, depth })
        if (expandedIds.value.has(n.id) && n.children.length > 0) {
          walkNormal(n.children, depth + 1)
        }
      }
    }
    walkNormal(treeData.value, 0)
  } else {
    walk(treeData.value, 0, false)
  }
  return result
})

async function loadTreeData() {
  if (!activeTreeId.value) return
  loading.value = true
  try {
    const res = await knowledgeNodeApi.getTree(activeTreeId.value)
    treeData.value = res.data
    // 默认展开根节点
    expandedIds.value = new Set(
      treeData.value.filter((n) => n.children.length > 0).map((n) => n.id),
    )
    // 默认选中第一个根节点
    if (treeData.value.length > 0 && !selectedNodeId.value) {
      selectedNodeId.value = treeData.value[0].id
    }
  } catch (e: any) {
    toast.error(e.response?.data?.error || '加载节点树失败')
  } finally {
    loading.value = false
  }
}

function toggleExpand(id: string) {
  if (expandedIds.value.has(id)) expandedIds.value.delete(id)
  else expandedIds.value.add(id)
}

function selectNode(id: string) {
  selectedNodeId.value = id
}

// ─── 当前选中节点 & 编辑表单 ──────────────────────────────────────────
const selectedNode = computed<KnowledgeNodeTreeNode | null>(() => {
  function find(nodes: KnowledgeNodeTreeNode[]): KnowledgeNodeTreeNode | null {
    for (const n of nodes) {
      if (n.id === selectedNodeId.value) return n
      if (n.children.length > 0) {
        const found = find(n.children)
        if (found) return found
      }
    }
    return null
  }
  return find(treeData.value)
})

const parentName = computed<string>(() => {
  if (!selectedNode.value?.parent_id) return ''
  function find(nodes: KnowledgeNodeTreeNode[]): string {
    for (const n of nodes) {
      if (n.id === selectedNode.value!.parent_id) return n.name
      if (n.children.length > 0) {
        const r = find(n.children)
        if (r) return r
      }
    }
    return ''
  }
  return find(treeData.value)
})

interface EditForm {
  name: string
  code: string
  aliasesText: string
  description: string
  sort_order: number
  parent_id: string
}

const editForm = reactive<EditForm>({
  name: '',
  code: '',
  aliasesText: '',
  description: '',
  sort_order: 0,
  parent_id: '',
})

const originalForm = ref<string>('')

const formDirty = computed(() => JSON.stringify(editForm) !== originalForm.value)

// 候选父节点列表（排除自身及子孙，防止环）
const movableParents = computed<KnowledgeNodeTreeNode[]>(() => {
  if (!selectedNode.value) return []
  const selfAndDescendants = new Set<string>()
  function collect(n: KnowledgeNodeTreeNode) {
    selfAndDescendants.add(n.id)
    n.children.forEach(collect)
  }
  collect(selectedNode.value)

  const list: { node: KnowledgeNodeTreeNode; depth: number }[] = []
  function walk(nodes: KnowledgeNodeTreeNode[], depth: number) {
    for (const n of nodes) {
      if (!selfAndDescendants.has(n.id)) {
        list.push({ node: n, depth })
        if (n.children.length > 0) walk(n.children, depth + 1)
      }
    }
  }
  walk(treeData.value, 0)
  return list.map((x) => x.node)
})

function syncFormFromNode() {
  const n = selectedNode.value
  if (!n) return
  editForm.name = n.name
  editForm.code = n.code || ''
  editForm.aliasesText = aliasesToText(n.aliases)
  editForm.description = n.description || ''
  editForm.sort_order = n.sort_order
  editForm.parent_id = n.parent_id || ''
  originalForm.value = JSON.stringify(editForm)
}

// aliases JSONB ↔ text 互转（格式：[{"alias":"...","locale":"zh"}]）
function aliasesToText(data: unknown): string {
  if (!Array.isArray(data)) return ''
  return data
    .map((x: any) => (x && typeof x === 'object' ? x.alias : String(x)))
    .filter(Boolean)
    .join(', ')
}

function aliasesFromText(text: string): unknown {
  const arr = text
    .split(/[,,]/)
    .map((s) => s.trim())
    .filter(Boolean)
  if (arr.length === 0) return null
  return arr.map((alias) => ({ alias, locale: 'zh' }))
}

watch(selectedNode, syncFormFromNode, { immediate: true })

function resetForm() {
  syncFormFromNode()
}

// ─── 保存节点 ──────────────────────────────────────────────────────────
const saving = ref(false)

async function saveNode() {
  if (!selectedNode.value) return
  const name = editForm.name.trim()
  if (!name) {
    toast.warning('节点名称不能为空')
    return
  }
  saving.value = true
  try {
    const originalParent = selectedNode.value.parent_id || ''
    const newParent = editForm.parent_id || ''

    // 1. 若 parent_id 变化，先调 move 接口（后端重算 path/depth）
    if (originalParent !== newParent) {
      await knowledgeNodeApi.move(selectedNode.value.id, newParent || null)
    }

    // 2. 更新其他字段
    await knowledgeNodeApi.update(selectedNode.value.id, {
      name,
      code: editForm.code.trim() || undefined,
      aliases: aliasesFromText(editForm.aliasesText),
      description: editForm.description.trim() || undefined,
      sort_order: editForm.sort_order,
    })

    toast.success('节点已保存')
    await loadTreeData()
  } catch (e: any) {
    toast.error(e.response?.data?.error || '保存失败')
  } finally {
    saving.value = false
  }
}

// ─── 添加节点 ──────────────────────────────────────────────────────────
const showAddDialog = ref(false)
const addForm = reactive({
  name: '',
  code: '',
  sort_order: 0,
  parentId: '' as string, // '' = 根节点
})
const adding = ref(false)

const addDialogTitle = computed(() =>
  addForm.parentId ? '添加子节点' : '添加根节点',
)

function openAddRoot() {
  addForm.name = ''
  addForm.code = ''
  addForm.sort_order = 0
  addForm.parentId = ''
  showAddDialog.value = true
}

function openAddChild() {
  if (!selectedNode.value) return
  addForm.name = ''
  addForm.code = ''
  addForm.sort_order = 0
  addForm.parentId = selectedNode.value.id
  showAddDialog.value = true
}

async function confirmAdd() {
  const name = addForm.name.trim()
  if (!name) {
    toast.warning('名称不能为空')
    return
  }
  if (!activeTreeId.value) return
  adding.value = true
  try {
    const res = await knowledgeNodeApi.create({
      tree_id: activeTreeId.value,
      parent_id: addForm.parentId || null,
      code: addForm.code.trim() || undefined,
      name,
      sort_order: addForm.sort_order,
    })
    toast.success('节点已添加')
    showAddDialog.value = false
    await loadTreeData()
    selectedNodeId.value = res.data.id
    // 自动展开父节点
    if (addForm.parentId) expandedIds.value.add(addForm.parentId)
  } catch (e: any) {
    toast.error(e.response?.data?.error || '添加失败')
  } finally {
    adding.value = false
  }
}

// ─── 删除节点 ──────────────────────────────────────────────────────────
const showDeleteConfirm = ref(false)

function confirmDelete() {
  if (!selectedNode.value) return
  showDeleteConfirm.value = true
}

async function deleteNode() {
  if (!selectedNode.value) return
  try {
    await knowledgeNodeApi.remove(selectedNode.value.id)
    toast.success(`已删除节点「${selectedNode.value.name}」`)
    selectedNodeId.value = ''
    await loadTreeData()
  } catch (e: any) {
    toast.error(e.response?.data?.error || '删除失败')
  }
}

// ─── 新建知识树 ────────────────────────────────────────────────────────
const showCreateTreeDialog = ref(false)
const newTree = reactive({
  code: '',
  name: '',
  kind: 'knowledge' as KnowledgeTreeKind,
  description: '',
})
const creatingTree = ref(false)

async function createTree() {
  if (!newTree.code.trim() || !newTree.name.trim()) {
    toast.warning('编码和名称必填')
    return
  }
  creatingTree.value = true
  try {
    const res = await knowledgeTreeApi.create({
      code: newTree.code.trim(),
      name: newTree.name.trim(),
      kind: newTree.kind,
      description: newTree.description.trim() || undefined,
    })
    trees.value = [...trees.value, res.data]
    activeTreeId.value = res.data.id
    toast.success(`已创建知识树「${res.data.name}」`)
    showCreateTreeDialog.value = false
    newTree.code = ''
    newTree.name = ''
    newTree.kind = 'knowledge'
    newTree.description = ''
  } catch (e: any) {
    toast.error(e.response?.data?.error || '创建知识树失败')
  } finally {
    creatingTree.value = false
  }
}

// ─── 辅助 ──────────────────────────────────────────────────────────────
function treeKindIcon(kind: KnowledgeTreeKind): string {
  if (kind === 'ability') return 'bolt'
  if (kind === 'chapter') return 'book-open'
  return 'tag'
}

function treeKindLabel(kind: KnowledgeTreeKind): string {
  if (kind === 'ability') return '能力'
  if (kind === 'chapter') return '章节'
  return '知识'
}

// ─── 生命周期 ──────────────────────────────────────────────────────────
watch(activeTreeId, () => {
  selectedNodeId.value = ''
  loadTreeData()
})

onMounted(loadTrees)
</script>

<template>
  <div class="kt-page">
    <!-- 顶部：标题 + 知识树 Tab -->
    <header class="kt-header">
      <div class="kt-title-row">
        <h1 class="page-title">
          <AppIcon name="tag" :size="22" /> 知识树管理
        </h1>
        <AppButton variant="outline" size="sm" @click="showCreateTreeDialog = true">
          <AppIcon name="plus" :size="14" /> 新建知识树
        </AppButton>
      </div>

      <div class="kt-tree-tabs">
        <button
          v-for="t in trees"
          :key="t.id"
          class="kt-tree-tab"
          :class="{ active: activeTreeId === t.id }"
          @click="activeTreeId = t.id"
        >
          <AppIcon :name="treeKindIcon(t.kind)" :size="14" />
          <span class="tab-label">{{ t.name }}</span>
          <span class="tab-kind">{{ treeKindLabel(t.kind) }}</span>
        </button>
      </div>
    </header>

    <!-- 主体：左树 + 右详情 -->
    <div class="kt-body">
      <!-- 左侧：节点树 -->
      <aside class="kt-tree-pane">
        <div class="pane-toolbar">
          <div class="search-wrap">
            <AppIcon name="search" :size="14" class="search-icon" />
            <input
              v-model="searchText"
              placeholder="搜索节点…"
              class="search-input"
            />
          </div>
          <button
            class="icon-btn"
            title="新建根节点"
            @click="openAddRoot"
          >
            <AppIcon name="plus" :size="16" />
          </button>
        </div>

        <div class="pane-tree">
          <div v-if="loading" class="pane-loading">加载中…</div>
          <AppEmpty
            v-else-if="filteredNodes.length === 0"
            description="暂无节点，点击右上角 + 创建"
            icon="tag"
          />
          <template v-else>
            <div
              v-for="item in filteredNodes"
              :key="item.node.id"
              class="tree-row"
              :class="{ selected: selectedNodeId === item.node.id }"
              :style="{ paddingLeft: 8 + item.depth * 18 + 'px' }"
              @click="selectNode(item.node.id)"
            >
              <button
                v-if="item.node.children.length > 0"
                type="button"
                class="row-expand"
                @click.stop="toggleExpand(item.node.id)"
              >
                <AppIcon
                  :name="expandedIds.has(item.node.id) ? 'chevron-down' : 'chevron-right'"
                  :size="11"
                />
              </button>
              <span v-else class="row-expand-spacer" />

              <span class="row-name">{{ item.node.name }}</span>
              <span v-if="item.node.code" class="row-code">{{ item.node.code }}</span>
              <span
                v-if="item.node.question_count > 0"
                class="row-count"
              >{{ item.node.question_count }}</span>
            </div>
          </template>
        </div>
      </aside>

      <!-- 右侧：详情编辑 -->
      <main class="kt-detail-pane">
        <div v-if="!selectedNode" class="empty-detail">
          <AppIcon name="info" :size="36" />
          <p>从左侧选择一个节点查看详情</p>
        </div>

        <template v-else>
          <div class="detail-header">
            <div class="detail-breadcrumb">
              <span class="crumb-root">{{ activeTree?.name }}</span>
              <span v-if="parentName" class="crumb-sep">/</span>
              <span v-if="parentName" class="crumb-parent">{{ parentName }}</span>
              <span class="crumb-sep">/</span>
              <span class="crumb-current">{{ selectedNode.name }}</span>
            </div>
            <div class="detail-actions">
              <button class="row-btn" @click="openAddChild">
                <AppIcon name="plus" :size="13" /> 添加子节点
              </button>
              <button class="row-btn danger" @click="confirmDelete">
                <AppIcon name="trash" :size="13" /> 删除
              </button>
            </div>
          </div>

          <div class="detail-body">
            <div class="form-group">
              <label>节点名称 <span class="required">*</span></label>
              <input
                v-model="editForm.name"
                placeholder="如：二次函数"
                class="form-input"
              />
            </div>

            <div class="form-row">
              <div class="form-group">
                <label>节点编码</label>
                <input
                  v-model="editForm.code"
                  placeholder="如：2.1.3"
                  class="form-input"
                />
              </div>
              <div class="form-group">
                <label>排序号</label>
                <input
                  v-model.number="editForm.sort_order"
                  type="number"
                  min="0"
                  class="form-input"
                />
              </div>
            </div>

            <div class="form-group">
              <label>同义词 aliases</label>
              <input
                v-model="editForm.aliasesText"
                placeholder="多个用英文逗号分隔，如：抛物线函数, 一元二次函数"
                class="form-input"
              />
              <span class="form-hint">用于 AI 智能打标的精确匹配（score = 0.95）</span>
            </div>

            <div class="form-group">
              <label>描述</label>
              <textarea
                v-model="editForm.description"
                rows="3"
                placeholder="选填"
                class="form-input textarea"
              ></textarea>
            </div>

            <div class="form-group" v-if="movableParents.length > 0">
              <label>移动到父节点</label>
              <select v-model="editForm.parent_id" class="form-input">
                <option value="">— 根节点 —</option>
                <option
                  v-for="p in movableParents"
                  :key="p.id"
                  :value="p.id"
                >{{ '　'.repeat(p.depth) }}{{ p.name }}</option>
              </select>
              <span class="form-hint">变更后保存将触发后端 path/depth 重算</span>
            </div>

            <div class="meta-grid">
              <div class="meta-item">
                <span class="meta-label">物化路径</span>
                <code class="meta-value">{{ selectedNode.path }}</code>
              </div>
              <div class="meta-item">
                <span class="meta-label">深度</span>
                <span class="meta-value">{{ selectedNode.depth }}</span>
              </div>
              <div class="meta-item">
                <span class="meta-label">关联题目</span>
                <span class="meta-value">{{ selectedNode.question_count }}</span>
              </div>
              <div class="meta-item">
                <span class="meta-label">状态</span>
                <span class="meta-value">{{ selectedNode.id ? '启用' : '—' }}</span>
              </div>
            </div>

            <div class="form-actions">
              <AppButton
                variant="ghost"
                :disabled="!formDirty || saving"
                @click="resetForm"
              >放弃修改</AppButton>
              <AppButton
                variant="primary"
                :loading="saving"
                :disabled="!formDirty"
                @click="saveNode"
              >
                <AppIcon name="save" :size="14" /> 保存
              </AppButton>
            </div>
          </div>
        </template>
      </main>
    </div>

    <!-- 新建知识树弹窗 -->
    <AppModal v-model="showCreateTreeDialog" title="新建知识树" size="sm">
      <div class="dialog-body">
        <div class="form-group">
          <label>编码 <span class="required">*</span></label>
          <input v-model="newTree.code" placeholder="如：math_knowledge" class="form-input" />
        </div>
        <div class="form-group">
          <label>名称 <span class="required">*</span></label>
          <input v-model="newTree.name" placeholder="如：数学知识树" class="form-input" />
        </div>
        <div class="form-group">
          <label>类型</label>
          <select v-model="newTree.kind" class="form-input">
            <option value="knowledge">知识树</option>
            <option value="ability">能力树</option>
            <option value="chapter">章节树</option>
          </select>
        </div>
        <div class="form-group">
          <label>描述</label>
          <textarea v-model="newTree.description" rows="2" class="form-input textarea"></textarea>
        </div>
      </div>
      <div class="dialog-actions">
        <AppButton variant="ghost" @click="showCreateTreeDialog = false">取消</AppButton>
        <AppButton variant="primary" :loading="creatingTree" @click="createTree">创建</AppButton>
      </div>
    </AppModal>

    <!-- 添加节点弹窗 -->
    <AppModal v-model="showAddDialog" :title="addDialogTitle" size="sm">
      <div class="dialog-body">
        <div class="form-group">
          <label>名称 <span class="required">*</span></label>
          <input
            v-model="addForm.name"
            placeholder="节点名称"
            class="form-input"
            @keyup.enter="confirmAdd"
          />
        </div>
        <div class="form-row">
          <div class="form-group">
            <label>编码</label>
            <input v-model="addForm.code" placeholder="选填" class="form-input" />
          </div>
          <div class="form-group">
            <label>排序号</label>
            <input
              v-model.number="addForm.sort_order"
              type="number"
              min="0"
              class="form-input"
            />
          </div>
        </div>
      </div>
      <div class="dialog-actions">
        <AppButton variant="ghost" @click="showAddDialog = false">取消</AppButton>
        <AppButton variant="primary" :loading="adding" @click="confirmAdd">添加</AppButton>
      </div>
    </AppModal>

    <!-- 删除确认 -->
    <AppConfirm
      v-model="showDeleteConfirm"
      title="删除节点"
      :message="`确定要删除节点「${selectedNode?.name}」吗？所有子节点会一并被删除。`"
      confirm-text="删除"
      danger
      @confirm="deleteNode"
    />
  </div>
</template>

<style scoped>
/* ── 页面容器：满屏 Flex，锁定视口高度 ── */
.kt-page {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

/* ── 顶部 Header ── */
.kt-header {
  flex-shrink: 0;
  padding: 16px 24px 0;
  border-bottom: 1px solid var(--border-color);
  background: var(--bg-canvas);
}

.kt-title-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 12px;
}

.page-title {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0;
  font-size: 18px;
  font-weight: 600;
  color: var(--text-primary);
}

.kt-tree-tabs {
  display: flex;
  gap: 2px;
  overflow-x: auto;
}

.kt-tree-tab {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 8px 14px;
  border: none;
  border-bottom: 2px solid transparent;
  background: transparent;
  color: var(--text-muted);
  font-size: 13px;
  cursor: pointer;
  white-space: nowrap;
  transition: color 0.15s, border-color 0.15s;
}

.kt-tree-tab:hover {
  color: var(--text-primary);
}

.kt-tree-tab.active {
  color: var(--accent);
  border-bottom-color: var(--accent);
  font-weight: 600;
}

.tab-kind {
  padding: 1px 6px;
  border-radius: var(--radius-full);
  background: var(--bg-active);
  font-size: 11px;
  color: var(--text-muted);
}

.kt-tree-tab.active .tab-kind {
  background: var(--accent-light);
  color: var(--accent);
}

/* ── 主体：左 + 右 ── */
.kt-body {
  flex: 1;
  min-height: 0;
  display: flex;
  gap: 1px;
  background: var(--divider);
}

/* ── 左侧节点树 ── */
.kt-tree-pane {
  flex-shrink: 0;
  width: clamp(240px, 24%, 320px);
  display: flex;
  flex-direction: column;
  background: var(--bg-canvas);
}

.pane-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px;
  border-bottom: 1px solid var(--border-color);
}

.search-wrap {
  position: relative;
  flex: 1;
  display: flex;
  align-items: center;
}

.search-icon {
  position: absolute;
  left: 10px;
  color: var(--text-muted);
  pointer-events: none;
}

.search-input {
  width: 100%;
  padding: 7px 10px 7px 30px;
  border: 1px solid var(--border-color);
  border-radius: 8px;
  background: var(--bg-input);
  color: var(--text-primary);
  font-size: 13px;
  font-family: inherit;
  transition: border-color 0.15s, box-shadow 0.15s;
}

.search-input:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
  background: var(--bg-card);
}

.icon-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  border: 1px solid var(--border-color);
  border-radius: 8px;
  background: var(--bg-card);
  color: var(--text-secondary);
  cursor: pointer;
  flex-shrink: 0;
  transition: all 0.15s;
}

.icon-btn:hover {
  border-color: var(--accent);
  color: var(--accent);
}

.pane-tree {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
  padding: 6px;
}

.pane-loading {
  padding: 32px 12px;
  text-align: center;
  color: var(--text-muted);
  font-size: 13px;
}

.tree-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 8px;
  border-radius: 6px;
  cursor: pointer;
  font-size: 13px;
  color: var(--text-primary);
  transition: background 0.12s;
}

.tree-row:hover {
  background: var(--bg-hover);
}

.tree-row.selected {
  background: var(--accent-light);
  color: var(--accent);
}

.row-expand {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  background: none;
  border: none;
  padding: 0;
  color: var(--text-muted);
  cursor: pointer;
  flex-shrink: 0;
}

.row-expand-spacer {
  width: 16px;
  height: 16px;
  flex-shrink: 0;
}

.row-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.row-code {
  font-size: 11px;
  color: var(--text-muted);
  font-family: var(--font-mono);
  flex-shrink: 0;
}

.row-count {
  flex-shrink: 0;
  padding: 0 6px;
  border-radius: var(--radius-full);
  background: var(--bg-active);
  color: var(--text-secondary);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
}

.tree-row.selected .row-count {
  background: var(--accent-light);
  color: var(--accent);
}

/* ── 右侧详情 ── */
.kt-detail-pane {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-canvas);
}

.empty-detail {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  color: var(--text-muted);
  font-size: 14px;
}

.detail-header {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 14px 24px;
  border-bottom: 1px solid var(--border-color);
}

.detail-breadcrumb {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  color: var(--text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.crumb-root {
  color: var(--text-secondary);
}

.crumb-parent {
  color: var(--text-secondary);
}

.crumb-current {
  color: var(--text-primary);
  font-weight: 600;
}

.crumb-sep {
  color: var(--text-muted);
  opacity: 0.6;
}

.detail-actions {
  display: flex;
  gap: 6px;
  flex-shrink: 0;
}

.row-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 5px 10px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-card);
  color: var(--text-secondary);
  font-size: 12px;
  cursor: pointer;
  transition: all 0.15s;
}

.row-btn:hover {
  border-color: var(--accent);
  color: var(--accent);
}

.row-btn.danger {
  color: var(--danger);
}

.row-btn.danger:hover {
  border-color: var(--danger);
  color: var(--danger);
}

.detail-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
  padding: 20px 24px;
}

.form-group {
  margin-bottom: 16px;
}

.form-group label {
  display: block;
  margin-bottom: 6px;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
  letter-spacing: 0.02em;
}

.required {
  color: var(--danger);
}

.form-input {
  width: 100%;
  padding: 8px 12px;
  border: 1px solid var(--border-color);
  border-radius: 8px;
  background: var(--bg-input);
  color: var(--text-primary);
  font-size: 13px;
  font-family: inherit;
  box-sizing: border-box;
  transition: border-color 0.15s, box-shadow 0.15s, background 0.15s;
}

.form-input:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
  background: var(--bg-card);
}

.form-input.textarea {
  resize: vertical;
  min-height: 64px;
  font-family: inherit;
}

.form-hint {
  display: block;
  margin-top: 5px;
  font-size: 11px;
  color: var(--text-muted);
  line-height: 1.5;
}

.form-row {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
}

.meta-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
  gap: 10px;
  margin: 20px 0;
  padding: 14px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--bg-primary);
}

.meta-item {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.meta-label {
  font-size: 11px;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.meta-value {
  font-size: 13px;
  color: var(--text-primary);
  font-variant-numeric: tabular-nums;
  word-break: break-all;
}

.meta-value code {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-secondary);
}

.form-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding-top: 16px;
  border-top: 1px solid var(--divider);
}

/* ── 弹窗 ── */
.dialog-body {
  padding: 4px 0;
  min-width: 320px;
}

.dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 16px;
}

/* ── 滚动条 ── */
.pane-tree::-webkit-scrollbar,
.detail-body::-webkit-scrollbar {
  width: 5px;
}
.pane-tree::-webkit-scrollbar-track,
.detail-body::-webkit-scrollbar-track {
  background: transparent;
}
.pane-tree::-webkit-scrollbar-thumb,
.detail-body::-webkit-scrollbar-thumb {
  background: var(--border-color);
  border-radius: 3px;
}

/* ── 响应式：窄屏堆叠 ── */
@media (max-width: 768px) {
  .kt-header {
    padding: 12px 16px 0;
  }

  .kt-body {
    flex-direction: column;
  }

  .kt-tree-pane {
    width: 100%;
    height: 40%;
    border-bottom: 1px solid var(--border-color);
  }

  .kt-detail-pane {
    height: 60%;
  }

  .detail-header {
    padding: 12px 16px;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
  }

  .detail-body {
    padding: 16px;
  }

  .form-row {
    grid-template-columns: 1fr;
  }
}
</style>

<style>
/* 父级 .view.active 高度链打通，配合 .kt-page 的 absolute/inset:0 撑满 */
.view.active {
  height: 100%;
  position: relative;
}
</style>
