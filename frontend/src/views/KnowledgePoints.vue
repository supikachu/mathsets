<template>
  <div>
    <div class="flex items-center justify-between mb-4">
      <h1 class="text-2xl font-bold">🏷️ 知识点管理</h1>
      <el-button type="primary" @click="addRoot">➕ 添加根节点</el-button>
    </div>

    <el-row :gutter="20">
      <!-- 左侧树 -->
      <el-col :span="12">
        <el-card shadow="never" v-loading="loading">
          <template #header><span class="font-bold">📂 知识点树</span></template>

          <div v-if="!loading && tree.length === 0" class="text-center text-gray-400 py-8">
            暂无知识点，添加第一个根节点
          </div>

          <el-tree
            :data="tree"
            :props="{ children: 'children', label: 'name' }"
            node-key="id"
            default-expand-all
            highlight-current
            @node-click="onNodeClick"
          >
            <template #default="{ node, data }">
              <span class="flex items-center gap-2 py-1">
                <span v-if="data.children?.length">📁</span>
                <span v-else>📄</span>
                <span>{{ data.name }}</span>
                <span v-if="data.grade" class="text-xs text-gray-400">({{ data.grade }})</span>
              </span>
            </template>
          </el-tree>
        </el-card>
      </el-col>

      <!-- 右侧编辑 -->
      <el-col :span="12">
        <el-card shadow="never">
          <template #header>
            <span class="font-bold">{{ editingNode ? '编辑节点' : '选择节点' }}</span>
          </template>

          <div v-if="!editingNode" class="text-center text-gray-400 py-12">
            点击左侧树中的节点进行编辑
          </div>

          <el-form v-else :model="editForm" label-position="top">
            <el-form-item label="节点名称" required>
              <el-input v-model="editForm.name" placeholder="输入名称" />
            </el-form-item>

            <el-form-item label="适用年级">
              <el-select v-model="editForm.grade" clearable style="width:100%">
                <el-option v-for="g in grades" :key="g" :label="g" :value="g" />
              </el-select>
            </el-form-item>

            <el-form-item label="排序号">
              <el-input-number v-model="editForm.sort_order" :min="0" :max="999" />
            </el-form-item>

            <div class="flex gap-3">
              <el-button type="primary" @click="saveEdit" :loading="saving">💾 保存</el-button>
              <el-button @click="addChild">+ 添加子节点</el-button>
              <el-popconfirm
                title="确认删除此节点？子节点会一并被删除"
                @confirm="deleteNode"
              >
                <template #reference>
                  <el-button type="danger" :disabled="!canDelete">🗑️ 删除</el-button>
                </template>
              </el-popconfirm>
            </div>
          </el-form>
        </el-card>
      </el-col>
    </el-row>

    <!-- 添加子节点/根节点弹窗 -->
    <el-dialog v-model="addDialog" :title="addParent ? '添加子节点' : '添加根节点'" width="400">
      <el-form :model="addForm" label-position="top">
        <el-form-item label="名称" required>
          <el-input v-model="addForm.name" placeholder="节点名称" />
        </el-form-item>
        <el-form-item label="适用年级">
          <el-select v-model="addForm.grade" clearable style="width:100%">
            <el-option v-for="g in grades" :key="g" :label="g" :value="g" />
          </el-select>
        </el-form-item>
        <el-form-item label="排序号">
          <el-input-number v-model="addForm.sort_order" :min="0" :max="999" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="addDialog = false">取消</el-button>
        <el-button type="primary" @click="confirmAdd" :loading="adding">确认添加</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, computed, onMounted } from 'vue'
import { ElMessage } from 'element-plus'
import { kpApi, type KnowledgePoint } from '@/api/client'
import client from '@/api/client'

const loading = ref(true)
const saving = ref(false)
const adding = ref(false)
const tree = ref<KnowledgePoint[]>([])
const editingNode = ref<KnowledgePoint | null>(null)
const grades = ['初一', '初二', '初三', '高一', '高二', '高三']

const editForm = reactive({
  name: '',
  grade: undefined as string | undefined,
  sort_order: 0,
})

// 添加弹窗
const addDialog = ref(false)
const addParent = ref<KnowledgePoint | null>(null)
const addForm = reactive({
  name: '',
  grade: undefined as string | undefined,
  sort_order: 0,
})

const canDelete = computed(() => editingNode.value !== null)

async function fetchTree() {
  loading.value = true
  try {
    const res = await kpApi.tree()
    tree.value = res.data
  } catch { /* handled */ }
  finally { loading.value = false }
}

function onNodeClick(data: KnowledgePoint) {
  editingNode.value = data
  editForm.name = data.name
  editForm.grade = data.grade || undefined
  editForm.sort_order = data.sort_order
}

function addRoot() {
  addParent.value = null
  addForm.name = ''
  addForm.grade = undefined
  addForm.sort_order = 0
  addDialog.value = true
}

function addChild() {
  addParent.value = editingNode.value
  addForm.name = ''
  addForm.grade = undefined
  addForm.sort_order = 0
  addDialog.value = true
}

async function confirmAdd() {
  if (!addForm.name.trim()) {
    ElMessage.warning('请输入名称')
    return
  }
  adding.value = true
  try {
    await client.post('/knowledge-points', {
      parent_id: addParent.value?.id || null,
      name: addForm.name.trim(),
      grade: addForm.grade || null,
      sort_order: addForm.sort_order,
    })
    ElMessage.success('添加成功')
    addDialog.value = false
    editingNode.value = null
    await fetchTree()
  } catch (e: any) {
    ElMessage.error(e.response?.data?.error || '添加失败')
  } finally {
    adding.value = false
  }
}

async function saveEdit() {
  if (!editingNode.value || !editForm.name.trim()) {
    ElMessage.warning('请输入名称')
    return
  }
  saving.value = true
  try {
    await client.put(`/knowledge-points/${editingNode.value.id}`, {
      name: editForm.name.trim(),
      grade: editForm.grade || null,
      sort_order: editForm.sort_order,
    })
    ElMessage.success('已保存')
    await fetchTree()
  } catch (e: any) {
    ElMessage.error(e.response?.data?.error || '保存失败')
  } finally {
    saving.value = false
  }
}

async function deleteNode() {
  if (!editingNode.value) return
  try {
    await client.delete(`/knowledge-points/${editingNode.value.id}`)
    ElMessage.success('已删除')
    editingNode.value = null
    await fetchTree()
  } catch (e: any) {
    ElMessage.error(e.response?.data?.error || '删除失败')
  }
}

onMounted(fetchTree)
</script>
