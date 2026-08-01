<template>
  <div class="tag-mgmt">
    <h1 class="page-title mb-12"><AppIcon name="tag" :size="24" /> 标签管理</h1>

    <!-- 分类 Tab -->
    <div class="tab-bar">
      <button
        v-for="tab in tabs"
        :key="tab.value"
        class="tab-btn"
        :class="{ active: activeTab === tab.value }"
        @click="activeTab = tab.value"
      >
        {{ tab.label }}
        <span class="tab-count">{{ tab.count }}</span>
      </button>
    </div>

    <!-- 操作栏 -->
    <div class="action-bar">
      <input
        v-model="searchQuery"
        class="search-input"
        placeholder="搜索标签名称…"
      />
      <AppButton variant="primary" size="sm" @click="showCreateDialog = true">
        <AppIcon name="plus" :size="14" />
        新建标签
      </AppButton>
    </div>

    <!-- 标签列表 -->
    <div v-if="loading" class="loading-hint">加载中…</div>

    <AppEmpty v-else-if="filteredTags.length === 0" description="暂无标签" />

    <table v-else class="tag-table">
      <thead>
        <tr>
          <th>标签名称</th>
          <th>类别</th>
          <th class="th-count" @click="toggleSort">
            使用次数
            <span class="sort-arrow">{{ sortDesc ? '↓' : '↑' }}</span>
          </th>
          <th>创建时间</th>
          <th class="th-actions">操作</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="tag in filteredTags" :key="tag.id" :class="{ 'row-selected': selectedMergeSource === tag.id }">
          <td class="td-name">
            <input
              v-if="editingId === tag.id"
              v-model="editingName"
              class="inline-edit"
              @keyup.enter="saveEdit(tag)"
              @keyup.escape="cancelEdit"
            />
            <span v-else>{{ tag.name }}</span>
          </td>
          <td>
            <AppBadge :color="categoryColor(tag.category)">{{ categoryLabel(tag.category) }}</AppBadge>
          </td>
          <td class="td-count">{{ tag.use_count }}</td>
          <td class="td-time">{{ formatTime(tag.created_at) }}</td>
          <td class="td-actions">
            <button v-if="editingId !== tag.id" class="row-btn" @click="startEdit(tag)">编辑</button>
            <button v-if="editingId === tag.id" class="row-btn row-btn-primary" @click="saveEdit(tag)">保存</button>
            <button v-if="editingId === tag.id" class="row-btn" @click="cancelEdit">取消</button>
            <button class="row-btn" @click="startMerge(tag)">合并</button>
            <button class="row-btn row-btn-danger" @click="confirmDelete(tag)">删除</button>
          </td>
        </tr>
      </tbody>
    </table>

    <!-- 新建标签弹窗 -->
    <AppModal v-model="showCreateDialog" title="新建标签">
      <div class="dialog-body">
        <div class="dialog-field">
          <label>标签名称</label>
          <input v-model="createForm.name" class="dialog-input" placeholder="如：待定系数法" @keyup.enter="doCreate" />
        </div>
        <div class="dialog-field">
          <label>标签类别</label>
          <select v-model="createForm.category" class="dialog-input">
            <option value="core_competence">核心素养</option>
            <option value="method">解题方法</option>
            <option value="school">学校</option>
          </select>
        </div>
      </div>
      <div class="dialog-actions">
        <AppButton variant="outline" @click="showCreateDialog = false">取消</AppButton>
        <AppButton variant="primary" :loading="createLoading" @click="doCreate">创建</AppButton>
      </div>
    </AppModal>

    <!-- 合并标签弹窗 -->
    <AppModal v-model="showMergeDialog" title="合并标签">
      <div class="dialog-body">
        <div class="merge-hint">
          将源标签「<strong>{{ mergeSourceTag?.name }}</strong>」的题目关联全部迁移到目标标签，然后删除源标签。
        </div>
        <div class="dialog-field">
          <label>目标标签（同类别）</label>
          <select v-model="mergeTargetId" class="dialog-input">
            <option value="">请选择目标标签…</option>
            <option
              v-for="t in mergeCandidates"
              :key="t.id"
              :value="t.id"
            >{{ t.name }}（使用 {{ t.use_count }} 次）</option>
          </select>
        </div>
      </div>
      <div class="dialog-actions">
        <AppButton variant="outline" @click="showMergeDialog = false">取消</AppButton>
        <AppButton variant="danger" :loading="mergeLoading" :disabled="!mergeTargetId" @click="doMerge">执行合并</AppButton>
      </div>
    </AppModal>

    <!-- 删除确认 -->
    <AppConfirm
      v-model="showDeleteConfirm"
      title="删除标签"
      :message="`确定要删除标签「${deleteTarget?.name}」吗？关联此标签的题目将自动移除该标签。`"
      confirm-text="删除"
      danger
      @confirm="doDelete"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { tagsApi, type Tag, type TagCategory } from '@/api/client'
import { AppIcon, AppButton, AppBadge, AppEmpty, AppModal, AppConfirm } from '@/components/ui'
import { useToast } from '@/composables/useToast'

const toast = useToast()

const tags = ref<Tag[]>([])
const loading = ref(false)
const activeTab = ref<'all' | 'core_competence' | 'method' | 'school'>('all')
const searchQuery = ref('')
const sortDesc = ref(true)

const tabs = computed(() => [
  { value: 'all' as const, label: '全部', count: tags.value.length },
  { value: 'core_competence' as const, label: '核心素养', count: tags.value.filter(t => t.category === 'core_competence').length },
  { value: 'method' as const, label: '解题方法', count: tags.value.filter(t => t.category === 'method').length },
  { value: 'school' as const, label: '学校', count: tags.value.filter(t => t.category === 'school').length },
])

const filteredTags = computed(() => {
  let list = tags.value
  if (activeTab.value !== 'all') {
    list = list.filter(t => t.category === activeTab.value)
  }
  const q = searchQuery.value.trim()
  if (q) {
    list = list.filter(t => t.name.includes(q))
  }
  const sorted = [...list].sort((a, b) =>
    sortDesc.value ? b.use_count - a.use_count : a.use_count - b.use_count,
  )
  return sorted
})

function toggleSort() {
  sortDesc.value = !sortDesc.value
}

// ===== 加载 =====
async function loadTags() {
  loading.value = true
  try {
    const res = await tagsApi.list()
    tags.value = res.data
  } catch {
    toast.error('加载标签失败')
  } finally {
    loading.value = false
  }
}

// ===== 编辑 =====
const editingId = ref<string | null>(null)
const editingName = ref('')

function startEdit(tag: Tag) {
  editingId.value = tag.id
  editingName.value = tag.name
}

function cancelEdit() {
  editingId.value = null
  editingName.value = ''
}

async function saveEdit(tag: Tag) {
  const name = editingName.value.trim()
  if (!name) {
    toast.warning('标签名称不能为空')
    return
  }
  if (name === tag.name) {
    cancelEdit()
    return
  }
  try {
    await tagsApi.update(tag.id, { name })
    tag.name = name
    toast.success('已更新标签名称')
  } catch (e: any) {
    toast.error(e.response?.data?.error || '更新失败')
  } finally {
    cancelEdit()
  }
}

// ===== 创建 =====
const showCreateDialog = ref(false)
const createLoading = ref(false)
const createForm = ref<{ name: string; category: TagCategory }>({ name: '', category: 'method' })

async function doCreate() {
  const name = createForm.value.name.trim()
  if (!name) {
    toast.warning('标签名称不能为空')
    return
  }
  createLoading.value = true
  try {
    const res = await tagsApi.create({ name, category: createForm.value.category })
    tags.value = [...tags.value, res.data]
    toast.success(`已创建标签「${name}」`)
    showCreateDialog.value = false
    createForm.value = { name: '', category: 'method' }
  } catch (e: any) {
    toast.error(e.response?.data?.error || '创建失败')
  } finally {
    createLoading.value = false
  }
}

// ===== 合并 =====
const showMergeDialog = ref(false)
const mergeSourceTag = ref<Tag | null>(null)
const mergeTargetId = ref('')
const mergeLoading = ref(false)
const selectedMergeSource = ref<string | null>(null)

const mergeCandidates = computed(() => {
  if (!mergeSourceTag.value) return []
  return tags.value.filter(
    t => t.category === mergeSourceTag.value!.category && t.id !== mergeSourceTag.value!.id,
  )
})

function startMerge(tag: Tag) {
  mergeSourceTag.value = tag
  mergeTargetId.value = ''
  selectedMergeSource.value = tag.id
  showMergeDialog.value = true
}

async function doMerge() {
  if (!mergeSourceTag.value || !mergeTargetId.value) return
  mergeLoading.value = true
  try {
    const res = await tagsApi.merge(mergeSourceTag.value.id, mergeTargetId.value)
    const data = res.data
    // 更新本地列表：移除源标签，更新目标标签 use_count
    tags.value = tags.value.filter(t => t.id !== mergeSourceTag.value!.id)
    const target = tags.value.find(t => t.id === mergeTargetId.value)
    if (target) target.use_count = data.merged_use_count
    toast.success(data.message)
    showMergeDialog.value = false
    selectedMergeSource.value = null
  } catch (e: any) {
    toast.error(e.response?.data?.error || '合并失败')
  } finally {
    mergeLoading.value = false
  }
}

// ===== 删除 =====
const showDeleteConfirm = ref(false)
const deleteTarget = ref<Tag | null>(null)

function confirmDelete(tag: Tag) {
  deleteTarget.value = tag
  showDeleteConfirm.value = true
}

async function doDelete() {
  if (!deleteTarget.value) return
  try {
    await tagsApi.remove(deleteTarget.value.id)
    tags.value = tags.value.filter(t => t.id !== deleteTarget.value!.id)
    toast.success(`已删除标签「${deleteTarget.value.name}」`)
    showDeleteConfirm.value = false
  } catch (e: any) {
    toast.error(e.response?.data?.error || '删除失败')
  }
}

// ===== 辅助 =====
function categoryLabel(c: string): string {
  const map: Record<string, string> = {
    core_competence: '核心素养',
    method: '解题方法',
    school: '学校',
  }
  return map[c] || c
}

function categoryColor(c: string): 'blue' | 'green' | 'yellow' | 'gray' {
  const map: Record<string, 'blue' | 'green' | 'yellow' | 'gray'> = {
    core_competence: 'blue',
    method: 'green',
    school: 'yellow',
  }
  return map[c] || 'gray'
}

function formatTime(t: string): string {
  return t ? t.replace('T', ' ').substring(0, 16) : ''
}

onMounted(loadTags)
</script>

<style scoped>
.tag-mgmt {
  max-width: 960px;
}

.tab-bar {
  display: flex;
  gap: 4px;
  margin-bottom: 16px;
  border-bottom: 1px solid var(--border);
}

.tab-btn {
  padding: 8px 16px;
  border: none;
  border-bottom: 2px solid transparent;
  background: transparent;
  cursor: pointer;
  font-size: 14px;
  color: var(--text-muted);
  transition: color 0.15s, border-color 0.15s;
}

.tab-btn:hover {
  color: var(--text-primary);
}

.tab-btn.active {
  color: var(--accent);
  border-bottom-color: var(--accent);
  font-weight: 600;
}

.tab-count {
  display: inline-block;
  margin-left: 4px;
  padding: 1px 6px;
  border-radius: 10px;
  background: var(--bg-tertiary);
  font-size: 11px;
  color: var(--text-muted);
}

.action-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 16px;
}

.search-input {
  flex: 1;
  max-width: 320px;
  padding: 8px 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  font-size: 14px;
  background: var(--bg-primary);
  color: var(--text-primary);
}

.search-input:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
}

.loading-hint {
  text-align: center;
  padding: 48px 20px;
  color: var(--text-muted);
}

.tag-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 14px;
}

.tag-table th {
  text-align: left;
  padding: 10px 12px;
  border-bottom: 2px solid var(--border);
  color: var(--text-muted);
  font-weight: 600;
  font-size: 13px;
  white-space: nowrap;
}

.th-count {
  cursor: pointer;
  user-select: none;
}

.sort-arrow {
  font-size: 11px;
  color: var(--accent);
}

.tag-table td {
  padding: 10px 12px;
  border-bottom: 1px solid var(--border);
  color: var(--text-primary);
}

.row-selected {
  background: var(--accent-light);
}

.td-name {
  font-weight: 500;
  min-width: 120px;
}

.td-count {
  text-align: right;
  font-variant-numeric: tabular-nums;
}

.td-time {
  color: var(--text-muted);
  font-size: 13px;
  white-space: nowrap;
}

.td-actions,
.th-actions {
  text-align: right;
  white-space: nowrap;
}

.row-btn {
  padding: 4px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--bg-primary);
  color: var(--text-secondary);
  font-size: 12px;
  cursor: pointer;
  margin-left: 4px;
  transition: all 0.15s;
}

.row-btn:hover {
  border-color: var(--accent);
  color: var(--accent);
}

.row-btn-primary {
  color: var(--accent);
  border-color: var(--accent);
}

.row-btn-danger {
  color: var(--danger, #e53e3e);
}

.row-btn-danger:hover {
  border-color: var(--danger, #e53e3e);
  color: var(--danger, #e53e3e);
}

.inline-edit {
  width: 100%;
  padding: 4px 8px;
  border: 1px solid var(--accent);
  border-radius: 6px;
  font-size: 14px;
  background: var(--bg-primary);
  color: var(--text-primary);
}

/* 弹窗内容 */
.dialog-body {
  padding: 4px 0;
  min-width: 360px;
}

.dialog-field {
  margin-bottom: 16px;
}

.dialog-field label {
  display: block;
  margin-bottom: 6px;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary);
}

.dialog-input {
  width: 100%;
  padding: 8px 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  font-size: 14px;
  background: var(--bg-primary);
  color: var(--text-primary);
}

.dialog-input:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
}

.merge-hint {
  padding: 12px 16px;
  border-radius: 8px;
  background: var(--bg-tertiary);
  font-size: 13px;
  color: var(--text-secondary);
  margin-bottom: 16px;
  line-height: 1.6;
}

.dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 8px;
}
</style>
