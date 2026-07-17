<script setup lang="ts">
import { ref, reactive, computed, watch, nextTick } from 'vue'
import { tagsApi, type KnowledgePoint, type Tag } from '@/api/client'
import { AppIcon, AppButton, AppModal } from '@/components/ui'
import KpPickerNode from '@/components/KpPickerNode.vue'
import { useToast } from '@/composables/useToast'

const show = defineModel<boolean>({ required: true })
const tagIds = defineModel<string[]>('tagIds', { required: true })
const attrSelectedKps = defineModel<{ id: string; name: string }[]>('attrSelectedKps', { required: true })

const props = defineProps<{
  kpTree: KnowledgePoint[]
  competenceTags: Tag[]
  methodTags: Tag[]
  schoolTags: Tag[]
  kpLoading?: boolean
}>()

const toast = useToast()

const attrSelectedKpIds = computed(() => attrSelectedKps.value.map(k => k.id))

const TAG_LIMITS: Record<string, number> = {
  core_competence: 3,
  method: 5,
  knowledge_point: 3,
  school: 1,
}

// Filter tags by category for footer summary display
const allTagsMap = computed(() => {
  const m = new Map<string, Tag>()
  for (const t of props.methodTags) m.set(t.id, t)
  for (const t of props.competenceTags) m.set(t.id, t)
  for (const t of props.schoolTags) m.set(t.id, t)
  return m
})

const selectedTagsList = computed(() => {
  return tagIds.value
    .map(id => allTagsMap.value.get(id))
    .filter((t): t is Tag => !!t)
})

const selectedCompetenceTags = computed(() => selectedTagsList.value.filter(t => t.category === 'core_competence'))
const selectedMethodTags = computed(() => selectedTagsList.value.filter(t => t.category === 'method'))
const selectedSchoolTags = computed(() => selectedTagsList.value.filter(t => t.category === 'school'))

// Tab switching state
const attrPanelTab = ref<'kp' | 'competence' | 'method' | 'school'>('kp')
const attrNavRefs = ref<HTMLElement[]>([])
const attrSliderOffset = ref(0)
const attrTabIndex: Record<string, number> = { kp: 0, competence: 1, method: 2, school: 3 }

function setAttrTab(tab: 'kp' | 'competence' | 'method' | 'school') {
  attrPanelTab.value = tab
  nextTick(() => {
    const idx = attrTabIndex[tab]
    const el = attrNavRefs.value[idx]
    if (el) {
      const nav = el.parentElement
      const navPaddingTop = nav ? parseInt(getComputedStyle(nav).paddingTop) : 0
      attrSliderOffset.value = el.offsetTop - navPaddingTop
    }
  })
}

watch(show, (open) => {
  if (open) {
    nextTick(() => setAttrTab(attrPanelTab.value))
  }
})

// Knowledge Point Search and Tree
const kpSearchQuery = ref('')
const kpExpanded = ref<Set<string>>(new Set())

function filterKpTree(nodes: KnowledgePoint[], query: string): KnowledgePoint[] {
  if (!query.trim()) return nodes
  const q = query.trim().toLowerCase()
  function filterNode(node: KnowledgePoint): KnowledgePoint | null {
    const nameMatch = node.name.toLowerCase().includes(q)
    const filteredChildren = (node.children || [])
      .map(child => filterNode(child))
      .filter((c): c is KnowledgePoint => c !== null)
    if (nameMatch || filteredChildren.length > 0) {
      return { ...node, children: filteredChildren }
    }
    return null
  }
  return nodes
    .map(n => filterNode(n))
    .filter((n): n is KnowledgePoint => n !== null)
}

const filteredKpTree = computed(() => filterKpTree(props.kpTree, kpSearchQuery.value))

const kpExpandedRecord = computed<Record<string, boolean>>(() => {
  const rec: Record<string, boolean> = {}
  kpExpanded.value.forEach(id => { rec[id] = true })
  return rec
})

function onKpToggleExpand(node: KnowledgePoint) {
  if (kpExpanded.value.has(node.id)) {
    kpExpanded.value.delete(node.id)
  } else {
    kpExpanded.value.add(node.id)
  }
  kpExpanded.value = new Set(kpExpanded.value)
}

watch(kpSearchQuery, (q) => {
  if (q.trim()) {
    function collectIds(nodes: KnowledgePoint[], set: Set<string>) {
      for (const n of nodes) {
        set.add(n.id)
        if (n.children?.length) collectIds(n.children, set)
      }
    }
    const ids = new Set<string>()
    collectIds(filteredKpTree.value, ids)
    kpExpanded.value = ids
  }
})

function onKpNodeToggle(node: KnowledgePoint) {
  const idx = attrSelectedKps.value.findIndex(k => k.id === node.id)
  if (idx >= 0) {
    attrSelectedKps.value.splice(idx, 1)
  } else {
    if (attrSelectedKps.value.length >= TAG_LIMITS.knowledge_point) {
      toast.warning('已达到该类别最大可选择上限')
      return
    }
    attrSelectedKps.value.push({ id: node.id, name: node.name })
  }
}

function removeAttrKp(id: string) {
  attrSelectedKps.value = attrSelectedKps.value.filter(k => k.id !== id)
}

// Tags management
function toggleTagById(tag: Tag) {
  const idx = tagIds.value.indexOf(tag.id)
  if (idx >= 0) {
    tagIds.value.splice(idx, 1)
    return
  }
  const count = selectedTagsList.value.filter(t => t.category === tag.category).length
  const limit = TAG_LIMITS[tag.category] ?? 99
  if (count >= limit) {
    toast.warning('已达到该类别最大可选择上限')
    return
  }
  tagIds.value.push(tag.id)
}

// Typeahead suggestions
interface SuggestState {
  query: string
  results: Tag[]
  loading: boolean
  timer: ReturnType<typeof setTimeout> | null
}

const suggestMethod = reactive<SuggestState>({ query: '', results: [], loading: false, timer: null })
const suggestSchool = reactive<SuggestState>({ query: '', results: [], loading: false, timer: null })

function onSuggestInput(state: SuggestState, category: 'method' | 'school') {
  if (state.timer) clearTimeout(state.timer)
  const q = state.query.trim()
  if (!q) {
    state.results = []
    return
  }
  state.timer = setTimeout(async () => {
    state.loading = true
    try {
      const res = await tagsApi.suggest(q, category)
      state.results = res.data
    } catch { state.results = [] }
    finally { state.loading = false }
  }, 200)
}

async function createNewTag(name: string, category: 'method' | 'school', state: SuggestState) {
  try {
    const res = await tagsApi.create(name, category)
    // Add to selected list
    tagIds.value.push(res.data.id)
    toast.success(`已创建并选中标签「${name}」`)
    state.query = ''
    state.results = []
  } catch (e: any) {
    toast.error(e.response?.data?.error || '创建标签失败')
  }
}

// Top recommended tags
const topMethods = computed(() => [...props.methodTags].sort((a, b) => b.use_count - a.use_count).slice(0, 8))
const topSchools = computed(() => [...props.schoolTags].sort((a, b) => b.use_count - a.use_count).slice(0, 8))

function handleFooterWheel(event: WheelEvent) {
  const container = event.currentTarget as HTMLElement
  if (container) {
    event.preventDefault()
    container.scrollLeft += event.deltaY
  }
}
</script>

<template>
  <AppModal v-model="show" title="属性面板" width="820px" height="580px" class="apple-modal-spec">
    <!-- 中层 Body — 左右双栏 -->
    <div class="attr-panel-body">
      <!-- 左侧分类导航 -->
      <nav class="attr-panel-nav">
        <!-- 苹果风弹性滑块 -->
        <div class="attr-nav-slider" :style="{ transform: `translateY(${attrSliderOffset}px)` }" />
        <button
          ref="attrNavRefs"
          class="attr-nav-item"
          :class="{ active: attrPanelTab === 'kp' }"
          @click="setAttrTab('kp')"
        >
          <AppIcon name="tag" :size="15" />
          <span>知识点</span>
          <span v-if="attrSelectedKps.length" class="attr-nav-badge">{{ attrSelectedKps.length }}</span>
        </button>
        <button
          ref="attrNavRefs"
          class="attr-nav-item"
          :class="{ active: attrPanelTab === 'competence' }"
          @click="setAttrTab('competence')"
        >
          <AppIcon name="award" :size="15" />
          <span>核心素养</span>
          <span v-if="selectedCompetenceTags.length" class="attr-nav-badge">{{ selectedCompetenceTags.length }}</span>
        </button>
        <button
          ref="attrNavRefs"
          class="attr-nav-item"
          :class="{ active: attrPanelTab === 'method' }"
          @click="setAttrTab('method')"
        >
          <AppIcon name="bookmark" :size="15" />
          <span>解题方法</span>
          <span v-if="selectedMethodTags.length" class="attr-nav-badge">{{ selectedMethodTags.length }}</span>
        </button>
        <button
          ref="attrNavRefs"
          class="attr-nav-item"
          :class="{ active: attrPanelTab === 'school' }"
          @click="setAttrTab('school')"
        >
          <AppIcon name="pin" :size="15" />
          <span>学校来源</span>
          <span v-if="selectedSchoolTags.length" class="attr-nav-badge">{{ selectedSchoolTags.length }}</span>
        </button>
      </nav>

      <!-- 右侧内容画布 -->
      <div class="attr-panel-content">
        <!-- 知识点面板 — 内嵌搜索 + 级联树 -->
        <div v-show="attrPanelTab === 'kp'" class="attr-canvas attr-canvas-kp">
          <input
            v-model="kpSearchQuery"
            class="attr-dialog-input kp-search-input"
            placeholder="搜索知识点…"
          />
          <div class="kp-canvas-tree">
            <div v-if="kpLoading" class="loading-hint">加载中…</div>
            <KpPickerNode
              v-for="node in filteredKpTree"
              :key="node.id"
              :node="node"
              :level="0"
              :selected-kp-ids="attrSelectedKpIds"
              :primary-kp-id="attrSelectedKps[0]?.id ?? null"
              :expanded="kpExpandedRecord"
              @select="onKpNodeToggle"
              @toggle-expand="onKpToggleExpand"
            />
          </div>
        </div>

        <!-- 核心素养面板 — 实体胶囊网格 -->
        <div v-show="attrPanelTab === 'competence'" class="attr-canvas">
          <div class="competence-grid">
            <button
              v-for="t in competenceTags"
              :key="t.id"
              type="button"
              class="competence-chip"
              :class="{ active: tagIds.includes(t.id) }"
              @click="toggleTagById(t)"
            >
              <span v-if="tagIds.includes(t.id)" class="competence-check">✓</span>
              <span>{{ t.name }}</span>
            </button>
          </div>
        </div>

        <!-- 解题方法面板 — 输入框 + 常用推荐 -->
        <div v-show="attrPanelTab === 'method'" class="attr-canvas">
          <div class="typeahead-wrap">
            <input
              v-model="suggestMethod.query"
              class="attr-dialog-input"
              placeholder="搜索或创建方法标签…"
              @input="onSuggestInput(suggestMethod, 'method')"
            />
            <div v-if="suggestMethod.results.length" class="typeahead-popover">
              <button
                v-for="t in suggestMethod.results"
                :key="t.id"
                type="button"
                class="typeahead-item"
                @click="toggleTagById(t); suggestMethod.query = ''; suggestMethod.results = []"
              >
                <span>{{ t.name }}</span>
                <span class="typeahead-count">{{ t.use_count }} 次</span>
              </button>
            </div>
            <button
              v-if="suggestMethod.query.trim() && !suggestMethod.results.some(t => t.name === suggestMethod.query.trim())"
              type="button"
              class="typeahead-create"
              @click="createNewTag(suggestMethod.query.trim(), 'method', suggestMethod)"
            >+ 创建新标签「{{ suggestMethod.query.trim() }}」</button>
          </div>
          <div v-if="topMethods.length" class="recommend-section">
            <div class="recommend-label">常用方法</div>
            <div class="recommend-chips">
              <button
                v-for="t in topMethods"
                :key="t.id"
                type="button"
                class="recommend-chip"
                :class="{ active: tagIds.includes(t.id) }"
                @click="toggleTagById(t)"
              >
                <span v-if="tagIds.includes(t.id)" class="recommend-check">✓</span>
                <span>{{ t.name }}</span>
              </button>
            </div>
          </div>
        </div>

        <!-- 学校来源面板 — 输入框 + 常用推荐 -->
        <div v-show="attrPanelTab === 'school'" class="attr-canvas">
          <div class="typeahead-wrap">
            <input
              v-model="suggestSchool.query"
              class="attr-dialog-input"
              placeholder="搜索或创建学校标签…"
              @input="onSuggestInput(suggestSchool, 'school')"
            />
            <div v-if="suggestSchool.results.length" class="typeahead-popover">
              <button
                v-for="t in suggestSchool.results"
                :key="t.id"
                type="button"
                class="typeahead-item"
                @click="toggleTagById(t); suggestSchool.query = ''; suggestSchool.results = []"
              >
                <span>{{ t.name }}</span>
                <span class="typeahead-count">{{ t.use_count }} 次</span>
              </button>
            </div>
            <button
              v-if="suggestSchool.query.trim() && !suggestSchool.results.some(t => t.name === suggestSchool.query.trim())"
              type="button"
              class="typeahead-create"
              @click="createNewTag(suggestSchool.query.trim(), 'school', suggestSchool)"
            >+ 创建新标签「{{ suggestSchool.query.trim() }}」</button>
          </div>
          <div v-if="topSchools.length" class="recommend-section">
            <div class="recommend-label">常用学校</div>
            <div class="recommend-chips">
              <button
                v-for="t in topSchools"
                :key="t.id"
                type="button"
                class="recommend-chip"
                :class="{ active: tagIds.includes(t.id) }"
                @click="toggleTagById(t)"
              >
                <span v-if="tagIds.includes(t.id)" class="recommend-check">✓</span>
                <span>{{ t.name }}</span>
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- 底部全局已选预览条 -->
    <div class="modal-footer-row">
      <div class="selected-flow-wrapper">
        <div class="selected-tags-preview-flow" @wheel="handleFooterWheel">
          <span
            v-for="(kp, idx) in attrSelectedKps"
            :key="'pv-kp-' + kp.id"
            class="attr-tag preview-pill preview-pill-kp"
            :class="{ 'preview-pill-primary': idx === 0 }"
          >
            <span v-if="idx === 0" class="preview-pill-icon">主</span>
            <span v-else class="preview-pill-icon">KP</span>
            <span class="preview-pill-text">{{ kp.name }}</span>
            <button type="button" class="preview-pill-x" @click="removeAttrKp(kp.id)"><AppIcon name="x" :size="10" /></button>
          </span>
          <span v-for="t in selectedCompetenceTags" :key="'pv-comp-' + t.id" class="attr-tag preview-pill preview-pill-comp">
            <span class="preview-pill-text">{{ t.name }}</span>
            <button type="button" class="preview-pill-x" @click="toggleTagById(t)"><AppIcon name="x" :size="10" /></button>
          </span>
          <span v-for="t in selectedMethodTags" :key="'pv-method-' + t.id" class="attr-tag preview-pill preview-pill-method">
            <span class="preview-pill-text">{{ t.name }}</span>
            <button type="button" class="preview-pill-x" @click="toggleTagById(t)"><AppIcon name="x" :size="10" /></button>
          </span>
          <span v-for="t in selectedSchoolTags" :key="'pv-school-' + t.id" class="attr-tag preview-pill preview-pill-school">
            <span class="preview-pill-text">{{ t.name }}</span>
            <button type="button" class="preview-pill-x" @click="toggleTagById(t)"><AppIcon name="x" :size="10" /></button>
          </span>
          <span v-if="attrSelectedKps.length === 0 && selectedTagsList.length === 0" class="preview-empty">暂未选择任何属性</span>
        </div>
      </div>
      <div class="footer-action-area">
        <AppButton class="modal-footer-submit-btn" variant="primary" size="sm" @click="show = false">完成</AppButton>
      </div>
    </div>
  </AppModal>
</template>

<style scoped>
/* 中层 Body — 左右双栏，高度扣除 header(54px)+footer(64px) */
.attr-panel-body {
  display: flex;
  flex-direction: row;
  height: calc(580px - 54px - 64px);
  background: var(--bg-primary);
}

/* 左侧分类导航轨 */
.attr-panel-nav {
  width: 140px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  padding: 12px 8px;
  gap: 4px;
  background: var(--bg-secondary);
  border-right: 1px solid var(--border-color);
  position: relative;
}

[data-theme='dark'] .attr-panel-nav {
  background: rgba(255, 255, 255, 0.02);
}

/* 苹果风侧导航弹性背底滑块 */
.attr-nav-slider {
  position: absolute;
  left: 8px;
  right: 8px;
  top: 12px;
  height: 36px;
  background: var(--bg-card);
  border-radius: 8px;
  box-shadow: 0 1px 3px rgba(0,0,0,0.04), 0 1px 2px rgba(0,0,0,0.02);
  transition: transform 0.3s cubic-bezier(0.25, 1, 0.5, 1);
  z-index: 1;
}

[data-theme='dark'] .attr-nav-slider {
  box-shadow: 0 1px 3px rgba(0,0,0,0.3);
}

.attr-nav-item {
  position: relative;
  z-index: 2;
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  height: 36px;
  padding: 0 12px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  border-radius: 8px;
  transition: color 0.25s;
}

.attr-nav-item.active {
  color: var(--accent);
  font-weight: 600;
}

.attr-nav-badge {
  margin-left: auto;
  min-width: 16px;
  height: 16px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 9999px;
  background: var(--accent-light);
  color: var(--accent);
  font-size: 10px;
  font-weight: 600;
  padding: 0 4px;
}

.attr-panel-content {
  flex: 1;
  min-width: 0;
  height: 100%;
  overflow: hidden;
  position: relative;
}

/* 右侧独立画布 */
.attr-canvas {
  height: 100%;
  overflow-y: auto;
  padding: 16px 20px;
  box-sizing: border-box;
}

.attr-canvas-kp {
  display: flex;
  flex-direction: column;
  padding: 14px 16px;
  gap: 10px;
}

/* 知识点树型区域滚动限制 */
.kp-canvas-tree {
  flex: 1;
  overflow-y: auto;
  border-radius: 8px;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  padding: 8px;
}

.competence-grid {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: 8px;
}

/* 素养实体卡片 — 极淡莫兰迪色底色 */
.competence-chip {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 14px;
  border-radius: 10px;
  background: var(--bg-card);
  border: 1.5px solid var(--border-color);
  color: var(--text-primary);
  font-size: 13px;
  font-weight: 550;
  cursor: pointer;
  transition: all 0.2s cubic-bezier(0.25, 1, 0.5, 1);
  text-align: left;
}

.competence-chip.active {
  background: var(--accent-light);
  border-color: var(--accent);
  color: var(--accent);
  box-shadow: var(--shadow-xs);
}

.competence-check {
  font-weight: 700;
  color: var(--accent);
}

/* typeahead 级联输入框 */
.typeahead-wrap {
  position: relative;
  margin-bottom: 16px;
  width: 100%;
}

.attr-dialog-input {
  width: 100%;
  padding: 8px 12px;
  border-radius: 8px;
  border: 1px solid var(--border-color);
  background: var(--bg-card);
  color: var(--text-primary);
  font-size: 13px;
  outline: none;
  box-sizing: border-box;
  transition: all 0.2s;
}

.attr-dialog-input:focus {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
}

.kp-search-input {
  flex-shrink: 0;
}

/* 常用推荐 */
.recommend-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.recommend-label {
  font-size: 11px;
  font-weight: 650;
  text-transform: uppercase;
  color: var(--text-muted);
  letter-spacing: 0.02em;
}

.recommend-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.recommend-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 5px 12px;
  border-radius: 9999px;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  color: var(--text-secondary);
  font-size: 12px;
  cursor: pointer;
  transition: all 0.2s;
}

.recommend-chip:hover {
  border-color: var(--accent);
}

.recommend-chip.active {
  background: var(--accent-light);
  border-color: var(--accent);
  color: var(--accent);
}

.recommend-check {
  font-weight: 700;
}

/* typeahead 浮动 Pop-over 面板 */
.typeahead-popover {
  position: absolute;
  top: 100%;
  left: 0;
  right: 0;
  margin-top: 4px;
  background: rgba(255, 255, 255, 0.8);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  border: 1px solid rgba(0, 0, 0, 0.08);
  border-radius: 12px;
  box-shadow: 0 10px 25px rgba(0,0,0,0.08);
  z-index: 50;
  max-height: 200px;
  overflow-y: auto;
  padding: 4px;
}

[data-theme='dark'] .typeahead-popover {
  background: rgba(28, 28, 30, 0.8);
  border-color: rgba(255, 255, 255, 0.08);
  box-shadow: 0 10px 25px rgba(0,0,0,0.3);
}

.typeahead-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
  padding: 8px 12px;
  border: none;
  background: transparent;
  color: var(--text-primary);
  font-size: 13px;
  text-align: left;
  border-radius: 8px;
  cursor: pointer;
}

.typeahead-item:hover {
  background: rgba(0, 122, 255, 0.08);
  color: var(--accent);
}

.typeahead-count {
  font-size: 11px;
  color: var(--text-muted);
}

.typeahead-create {
  display: block;
  width: 100%;
  padding: 8px 12px;
  border: none;
  background: transparent;
  color: var(--accent);
  font-size: 13px;
  font-weight: 500;
  text-align: left;
  cursor: pointer;
  margin-top: 4px;
}

.typeahead-create:hover {
  text-decoration: underline;
}

/* 底部全局已选预览条 */
.modal-footer-row {
  height: 64px;
  border-top: 1px solid var(--border-color);
  background: var(--bg-card);
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 18px;
  box-sizing: border-box;
}

.selected-flow-wrapper {
  flex: 1;
  min-width: 0;
  margin-right: 14px;
  overflow: hidden;
  position: relative;
}

.selected-tags-preview-flow {
  display: flex;
  align-items: center;
  gap: 6px;
  overflow-x: auto;
  white-space: nowrap;
  height: 100%;
  scrollbar-width: none;
}

.selected-tags-preview-flow::-webkit-scrollbar {
  display: none;
}

.preview-pill {
  height: 26px;
  padding: 0 8px 0 10px;
  border-radius: 9999px;
  font-size: 11px;
  font-weight: 550;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  color: var(--text-secondary);
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  box-sizing: border-box;
  flex-shrink: 0;
}

.preview-pill-x {
  border: none;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  padding: 2px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
}

.preview-pill-x:hover {
  background: rgba(0,0,0,0.06);
  color: var(--text-primary);
}

.preview-pill-kp {
  background: rgba(0, 122, 255, 0.04);
  border-color: rgba(0, 122, 255, 0.15);
  color: var(--accent);
}

.preview-pill-primary {
  background: var(--accent);
  border-color: var(--accent);
  color: #ffffff;
}

.preview-pill-primary .preview-pill-x {
  color: rgba(255, 255, 255, 0.8);
}

.preview-pill-primary .preview-pill-x:hover {
  background: rgba(255, 255, 255, 0.2);
  color: #ffffff;
}

.preview-pill-icon {
  font-size: 8px;
  font-weight: 800;
  background: rgba(255, 255, 255, 0.2);
  padding: 1px 3px;
  border-radius: 3px;
  line-height: 1;
}

.preview-pill-primary .preview-pill-icon {
  background: rgba(255, 255, 255, 0.25);
}

.preview-pill-comp {
  background: rgba(88, 86, 214, 0.04);
  border-color: rgba(88, 86, 214, 0.15);
  color: #5856d6;
}

.preview-pill-method {
  background: rgba(52, 199, 89, 0.04);
  border-color: rgba(52, 199, 89, 0.15);
  color: #34c759;
}

.preview-pill-school {
  background: rgba(255, 149, 0, 0.04);
  border-color: rgba(255, 149, 0, 0.15);
  color: #ff9500;
}

.preview-pill-text {
  max-width: 100px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.preview-empty {
  font-size: 12px;
  color: var(--text-muted);
  font-style: italic;
}

.footer-action-area {
  flex-shrink: 0;
}

.modal-footer-submit-btn {
  min-width: 72px;
}

.loading-hint {
  text-align: center;
  padding: 32px 0;
  color: var(--text-muted);
  font-size: 13px;
}

.attr-tag {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}
</style>
