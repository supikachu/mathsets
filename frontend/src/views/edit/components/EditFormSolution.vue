<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { AppIcon } from '@/components/ui'
import {
  type QuestionPart,
  addChild,
  addSibling,
  cnNum,
  defaultLeaf,
  findPart,
  isLeaf,
  isSimpleTree,
  MAX_PART_DEPTH,
  newAnalysis,
  partDepth,
  partPath,
  relabelTree,
  removePart,
  walkLeaves,
  walkLeavesWithPath,
} from '@/utils/questionParts'

const parts = defineModel<QuestionPart[]>('parts', { required: true })
const expandedId = defineModel<string>('expandedId', { default: '' })

const props = withDefaults(defineProps<{
  section?: 'stems' | 'answers' | 'analyses' | 'all'
}>(), {
  section: 'all',
})

const simple = computed(() => isSimpleTree(parts.value))
const showStems = computed(() => props.section === 'stems' || props.section === 'all')
const showAnswers = computed(() => props.section === 'answers' || props.section === 'all')
const showAnalyses = computed(() => props.section === 'analyses' || props.section === 'all')

type FlatNode = { part: QuestionPart; depth: number }

const flatNodes = computed<FlatNode[]>(() => {
  const out: FlatNode[] = []
  const rec = (nodes: QuestionPart[], depth: number) => {
    for (const n of nodes) {
      out.push({ part: n, depth })
      if (n.children.length) rec(n.children, depth + 1)
    }
  }
  rec(parts.value, 1)
  return out
})

const leaves = computed(() => walkLeavesWithPath(parts.value))

watch(
  parts,
  () => {
    if (!parts.value.length) parts.value = [defaultLeaf()]
    const ids = new Set(walkLeaves(parts.value).map((l) => l.id))
    if (!expandedId.value || !ids.has(expandedId.value)) {
      expandedId.value = walkLeaves(parts.value)[0]?.id || ''
    }
  },
  { immediate: true, deep: true },
)

const crumbs = computed(() => {
  if (!expandedId.value) return []
  return partPath(parts.value, expandedId.value)
})

const siblingChips = computed(() => {
  const path = crumbs.value
  if (path.length <= 1) return parts.value
  return path[path.length - 2]?.children ?? parts.value
})

function selectNode(id: string) {
  const node = findPart(parts.value, id)
  if (!node) return
  if (isLeaf(node)) {
    expandedId.value = id
    return
  }
  const first = walkLeaves(node.children)[0]
  if (first) expandedId.value = first.id
}

function onAddRoot() {
  const last = parts.value[parts.value.length - 1]
  expandedId.value = addSibling(parts.value, last?.id ?? null)
}

function onAddChild(id: string) {
  const nid = addChild(parts.value, id)
  if (nid) expandedId.value = nid
}

function onAddSibling(id: string) {
  expandedId.value = addSibling(parts.value, id)
}

function onRemove(id: string) {
  removePart(parts.value, id)
}

function canNest(id: string) {
  return partDepth(parts.value, id) < MAX_PART_DEPTH
}

function addAnalysis(part: QuestionPart) {
  part.analyses.push(newAnalysis(part.analyses.length + 1))
}

function removeAnalysis(part: QuestionPart, i: number) {
  part.analyses.splice(i, 1)
  if (!part.analyses.length) part.analyses.push(newAnalysis(1))
}

function insertAt(el: HTMLTextAreaElement | undefined, current: string, insert: string, set: (v: string) => void) {
  if (!el) {
    set((current || '') + insert)
    return
  }
  const start = el.selectionStart ?? (current || '').length
  const end = el.selectionEnd ?? start
  const next = (current || '').slice(0, start) + insert + (current || '').slice(end)
  set(next)
  nextTick(() => {
    el.focus()
    const pos = start + insert.length
    el.setSelectionRange(pos, pos)
  })
}

function pickImage(cb: (url: string) => void) {
  const input = document.createElement('input')
  input.type = 'file'
  input.accept = 'image/png,image/jpeg,image/gif,image/webp'
  input.onchange = () => {
    const file = input.files?.[0]
    if (file) cb(URL.createObjectURL(file))
  }
  input.click()
}

function onLabelInput(part: QuestionPart) {
  part.labelDirty = true
}

watch(parts, () => relabelTree(parts.value))

const rootRef = ref<HTMLElement | null>(null)

function supportsFieldSizing() {
  return typeof CSS !== 'undefined' && CSS.supports('field-sizing', 'content')
}

function fitTextareas() {
  const root = rootRef.value
  if (!root || supportsFieldSizing()) return
  root.querySelectorAll('textarea').forEach((el) => {
    const ta = el as HTMLTextAreaElement
    ta.style.height = 'auto'
    ta.style.height = `${ta.scrollHeight}px`
  })
}

onMounted(() => nextTick(fitTextareas))
watch(parts, () => nextTick(fitTextareas), { deep: true })
</script>

<template>
  <div ref="rootRef" class="sol-tree">
    <div v-if="showStems && !simple" class="sol-nav">
      <div class="sol-crumbs">
        <span class="crumb muted">总干</span>
        <template v-for="c in crumbs" :key="c.id">
          <span class="crumb-sep">/</span>
          <button type="button" class="crumb" @click="selectNode(c.id)">{{ c.label }}</button>
        </template>
      </div>
      <div class="sol-chips">
        <button
          v-for="s in siblingChips"
          :key="s.id"
          type="button"
          class="chip"
          :class="{ active: crumbs.some((c) => c.id === s.id) }"
          @click="selectNode(s.id)"
        >
          {{ s.label }}
        </button>
      </div>
    </div>

    <div v-if="showStems && !simple" class="part-list">
      <article
        v-for="node in flatNodes"
        :key="node.part.id"
        class="part-card"
        :class="{
          leaf: isLeaf(node.part),
          open: expandedId === node.part.id,
        }"
        :style="{ marginLeft: `${(node.depth - 1) * 16}px` }"
      >
        <header class="part-head" @click="selectNode(node.part.id)">
          <input
            class="part-label"
            :value="node.part.label"
            @click.stop
            @input="onLabelInput(node.part); node.part.label = ($event.target as HTMLInputElement).value"
          />
          <span v-if="!isLeaf(node.part)" class="part-kind">分支</span>
          <div class="part-actions" @click.stop>
            <button
              v-if="canNest(node.part.id)"
              type="button"
              class="icon-btn"
              title="添加子问"
              @click="onAddChild(node.part.id)"
            >
              <AppIcon name="plus" :size="13" /> 子问
            </button>
            <button
              type="button"
              class="icon-btn"
              title="添加同级"
              @click="onAddSibling(node.part.id)"
            >
              <AppIcon name="plus" :size="13" /> 同级
            </button>
            <button
              v-if="flatNodes.length > 1"
              type="button"
              class="icon-btn danger"
              title="删除"
              @click="onRemove(node.part.id)"
            >
              <AppIcon name="x" :size="13" />
            </button>
          </div>
        </header>

        <div class="part-stem">
          <textarea
            v-model="node.part.stem"
            class="edit-textarea"
            :placeholder="isLeaf(node.part) ? '本问题干（可空）' : '本层局部条件，如：若 f(x) 为奇函数'"
            @focus="selectNode(node.part.id)"
          />
          <button
            type="button"
            class="img-upload-btn"
            @click="pickImage((url) => { node.part.stem += `\n![配图](${url})\n` })"
          >
            <AppIcon name="paperclip" :size="13" /> 上传配图
          </button>
        </div>
      </article>
    </div>

    <button v-if="showStems" type="button" class="add-btn" @click="onAddRoot">
      <AppIcon name="plus" :size="14" /> {{ simple ? '增加小问' : '添加大问' }}
    </button>

    <div v-if="showAnswers" class="leaf-list">
      <article
        v-for="leaf in leaves"
        :key="'ans-' + leaf.part.id"
        class="part-card leaf-card"
        :class="{ open: expandedId === leaf.part.id, simple }"
        @click="selectNode(leaf.part.id)"
      >
        <label v-if="leaf.pathLabel" class="mini-label">{{ leaf.pathLabel }}</label>
        <textarea
          :value="leaf.part.answer ?? ''"
          class="edit-textarea"
          placeholder="小题答案，支持 $...$ LaTeX"
          @focus="selectNode(leaf.part.id)"
          @input="leaf.part.answer = ($event.target as HTMLTextAreaElement).value"
        />
      </article>
    </div>

    <div v-if="showAnalyses" class="leaf-list">
      <article
        v-for="leaf in leaves"
        :key="'ana-' + leaf.part.id"
        class="part-card leaf-card"
        :class="{ open: expandedId === leaf.part.id, simple }"
        @click="selectNode(leaf.part.id)"
      >
        <label v-if="leaf.pathLabel" class="mini-label">{{ leaf.pathLabel }}</label>
        <div v-for="(sol, i) in leaf.part.analyses" :key="sol.id" class="solution-item">
          <div class="solution-head">
            <span class="solution-name">{{ sol.title || `解法${cnNum(i + 1)}` }}</span>
            <div class="solution-head-right">
              <button
                type="button"
                class="quick-tool-btn"
                @click.stop="insertAt(($event.currentTarget as HTMLElement).closest('.solution-item')?.querySelector('textarea') || undefined, sol.content, '  ', (v) => { sol.content = v })"
              >首行缩进</button>
              <button
                type="button"
                class="quick-tool-btn"
                @click.stop="insertAt(($event.currentTarget as HTMLElement).closest('.solution-item')?.querySelector('textarea') || undefined, sol.content, '\n:::img-row\n\n:::\n', (v) => { sol.content = v })"
              >并排图组</button>
              <button
                v-if="leaf.part.analyses.length > 1"
                type="button"
                class="solution-del"
                @click.stop="removeAnalysis(leaf.part, i)"
              >
                <AppIcon name="trash-2" :size="14" />
              </button>
            </div>
          </div>
          <div class="solution-textarea-wrap">
            <textarea
              v-model="sol.content"
              class="edit-textarea solution-textarea"
              :placeholder="`解法${cnNum(i + 1)}的解题思路，支持 $...$ LaTeX`"
              @focus="selectNode(leaf.part.id)"
            />
            <button
              type="button"
              class="img-upload-btn"
              @click.stop="pickImage((url) => { sol.content += `\n![解析配图](${url})\n` })"
            >
              <AppIcon name="paperclip" :size="13" /> 上传配图
            </button>
          </div>
        </div>
        <button type="button" class="add-btn add-btn-sm" @click.stop="addAnalysis(leaf.part)">
          <AppIcon name="plus" :size="14" /> 添加新解法
        </button>
        <label class="no-analysis-check" @click.stop>
          <input v-model="leaf.part.no_analysis_needed" type="checkbox" />
          <span>无需解析（如纯计算题/默写题）</span>
        </label>
      </article>
    </div>
  </div>
</template>

<style scoped>
.sol-tree { display: flex; flex-direction: column; gap: 10px; }
.sol-nav { display: flex; flex-direction: column; gap: 8px; }
.sol-crumbs {
  display: flex; align-items: center; gap: 4px;
  font-size: 12px; color: var(--text-secondary); overflow-x: auto;
}
.crumb {
  border: none; background: none; color: var(--accent); cursor: pointer;
  font: inherit; padding: 0;
}
.crumb.muted { color: var(--text-muted); cursor: default; }
.crumb-sep { color: var(--text-muted); }
.sol-chips { display: flex; gap: 6px; overflow-x: auto; }
.chip {
  flex-shrink: 0; height: 26px; padding: 0 10px; border-radius: 999px;
  border: 1px solid var(--border-color); background: var(--bg-input);
  font-size: 12px; cursor: pointer; color: var(--text-secondary);
}
.chip.active { border-color: var(--accent); color: var(--accent); background: var(--accent-light); }
.part-list, .leaf-list { display: flex; flex-direction: column; gap: 10px; }
.part-card {
  border: 1px solid var(--border-color);
  border-radius: 12px;
  background: var(--bg-input);
  padding: 10px 12px;
}
.part-card.open { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-light); }
.part-head { display: flex; align-items: center; gap: 8px; cursor: pointer; }
.part-label {
  width: 64px; height: 28px; border: 1px solid var(--border-color);
  border-radius: 8px; padding: 0 8px; font-weight: 600; font-size: 13px;
  background: var(--bg-card); color: var(--text-primary);
}
.part-kind {
  font-size: 11px; color: var(--text-muted); padding: 2px 6px;
  border-radius: 6px; background: rgba(0,0,0,0.04);
}
.part-actions { margin-left: auto; display: flex; gap: 4px; }
.icon-btn {
  border: none; background: transparent; color: var(--text-secondary);
  font-size: 12px; cursor: pointer; display: inline-flex; align-items: center; gap: 2px;
  padding: 4px 6px; border-radius: 6px;
}
.icon-btn:hover { background: var(--accent-light); color: var(--accent); }
.icon-btn.danger:hover { background: rgba(255,59,48,0.1); color: #ff3b30; }
.part-stem { margin-top: 8px; position: relative; }
.leaf-card { display: flex; flex-direction: column; gap: 8px; cursor: pointer; }
.mini-label { font-size: 12px; font-weight: 600; color: var(--text-secondary); }
.edit-textarea {
  width: 100%;
  min-height: 72px;
  resize: none;
  overflow: hidden;
  field-sizing: content;
  border: 1px solid var(--border-color);
  border-radius: 10px;
  padding: 10px 12px;
  font: inherit;
  font-size: 13px;
  line-height: 1.7;
  box-sizing: border-box;
  outline: none;
  background: var(--bg-card);
  color: var(--text-primary);
}
.solution-textarea {
  min-height: 96px;
}
.solution-item { display: flex; flex-direction: column; gap: 6px; }
.solution-head { display: flex; align-items: center; justify-content: space-between; }
.solution-name { font-size: 13px; font-weight: 600; }
.solution-head-right { display: flex; gap: 6px; align-items: center; }
.quick-tool-btn {
  border: 1px solid var(--border-color); background: transparent;
  border-radius: 6px; font-size: 12px; padding: 2px 8px; cursor: pointer;
  color: var(--text-secondary);
}
.solution-del { border: none; background: none; cursor: pointer; color: var(--text-muted); }
.solution-textarea-wrap { position: relative; }
.img-upload-btn {
  margin-top: 6px; border: none; background: transparent; color: var(--text-secondary);
  font-size: 12px; cursor: pointer; display: inline-flex; align-items: center; gap: 4px;
}
.add-btn {
  display: inline-flex; align-items: center; gap: 6px; margin-top: 4px;
  padding: 7px 14px; border: 1px dashed var(--border-strong);
  background: transparent; color: var(--text-secondary); font-size: 13px;
  border-radius: 8px; cursor: pointer; font-family: inherit;
}
.add-btn:hover { border-color: var(--accent); color: var(--accent); border-style: solid; background: var(--accent-light); }
.add-btn-sm { padding: 4px 10px; font-size: 12px; }
.no-analysis-check {
  display: flex; align-items: center; gap: 8px; font-size: 13px; color: var(--text-secondary);
}
</style>
