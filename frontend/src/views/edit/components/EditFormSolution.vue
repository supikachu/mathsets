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
        <div class="part-field" @click="selectNode(node.part.id)">
          <input
            class="part-label"
            :value="node.part.label"
            @click.stop
            @input="onLabelInput(node.part); node.part.label = ($event.target as HTMLInputElement).value"
          />
          <span v-if="!isLeaf(node.part)" class="part-kind">分支</span>
          <textarea
            v-model="node.part.stem"
            class="edit-textarea"
            :placeholder="isLeaf(node.part) ? '本问题干（可空）' : '本层局部条件，如：若 f(x) 为奇函数'"
            @focus="selectNode(node.part.id)"
          />
          <button
            type="button"
            class="img-upload-btn"
            @click.stop="pickImage((url) => { node.part.stem += `\n![配图](${url})\n` })"
          >
            <AppIcon name="paperclip" :size="13" />
            <span>上传配图</span>
          </button>
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
              <AppIcon name="trash" :size="13" />
            </button>
          </div>
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
        <div class="part-field">
          <span v-if="leaf.pathLabel" class="inline-num">{{ leaf.pathLabel }}</span>
          <textarea
            :value="leaf.part.answer ?? ''"
            class="edit-textarea"
            placeholder="小题答案，支持 $...$ LaTeX"
            @focus="selectNode(leaf.part.id)"
            @input="leaf.part.answer = ($event.target as HTMLTextAreaElement).value"
          />
        </div>
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
          <div class="part-field solution-textarea-wrap">
            <span v-if="i === 0 && leaf.pathLabel" class="inline-num">{{ leaf.pathLabel }}</span>
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
              <AppIcon name="paperclip" :size="13" />
              <span>上传配图</span>
            </button>
          </div>
        </div>
        <button type="button" class="add-btn add-btn-sm" @click.stop="addAnalysis(leaf.part)">
          <AppIcon name="plus" :size="14" /> 添加新解法
        </button>
        <label class="no-analysis-check" @click.stop>
          <span class="no-analysis-copy">
            <span class="no-analysis-title">无需解析</span>
            <span class="no-analysis-caption">如纯计算题 / 默写题</span>
          </span>
          <input v-model="leaf.part.no_analysis_needed" type="checkbox" />
        </label>
      </article>
    </div>
  </div>
</template>

<style scoped>
.sol-tree { display: flex; flex-direction: column; gap: 16px; }
.sol-nav { display: flex; flex-direction: column; gap: 8px; }
.sol-crumbs {
  display: flex; align-items: center; gap: 4px;
  font-size: 12px; color: var(--text-secondary); overflow-x: auto;
}
.crumb {
  border: none; background: none; color: var(--accent); cursor: pointer;
  font: inherit; font-weight: 400; padding: 0;
}
.crumb.muted { color: var(--text-muted); cursor: default; }
.crumb-sep { color: var(--text-muted); }
.sol-chips { display: flex; gap: 8px; overflow-x: auto; }
.chip {
  flex-shrink: 0; height: 28px; padding: 0 12px; border-radius: 999px;
  border: 1px solid transparent; background: var(--bg-input);
  font-size: 12px; font-weight: 400; cursor: pointer; color: var(--text-secondary);
  transition: transform 0.2s cubic-bezier(0.4, 0, 0.2, 1), background 0.2s, color 0.2s;
}
.chip:hover { transform: translateY(-0.5px); }
.chip.active { color: var(--accent); background: var(--accent-light); }
.part-list, .leaf-list { display: flex; flex-direction: column; gap: 12px; }
.part-card {
  border: 1px solid transparent;
  border-radius: 12px;
  background: transparent;
  padding: 0;
}
.part-card.open .part-field {
  background: var(--bg-card);
}
.part-field {
  position: relative;
  display: flex;
  align-items: flex-start;
  background: var(--bg-input);
  border-radius: 12px;
  border: 1px solid transparent;
}
.part-field:focus-within {
  background: var(--bg-card);
  border-color: var(--accent);
}
.part-label,
.inline-num {
  flex-shrink: 0;
  margin: 12px 0 0 12px;
  font-weight: 600;
  font-size: 15px;
  line-height: 1.5;
  color: var(--text-primary);
  letter-spacing: -0.01em;
}
.part-label {
  width: auto;
  min-width: 0;
  max-width: 6em;
  height: 24px;
  padding: 0 2px 0 0;
  border: none;
  border-radius: 0;
  background: transparent;
  outline: none;
  field-sizing: content;
  box-shadow: none;
}
.part-label:focus {
  box-shadow: none;
  border: none;
  background: transparent;
}
.inline-num {
  display: block;
  white-space: nowrap;
}
.part-kind {
  flex-shrink: 0;
  margin-top: 14px;
  font-size: 11px;
  color: var(--text-muted);
  padding: 2px 8px;
  border-radius: 999px;
  background: rgba(0,0,0,0.04);
  font-weight: 400;
}
.part-actions {
  position: absolute;
  top: auto;
  right: 12px;
  bottom: 10px;
  display: flex;
  align-items: center;
  gap: 4px;
  z-index: 1;
}
.icon-btn {
  box-sizing: border-box;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font-size: 12px;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  height: 28px;
  min-height: 28px;
  padding: 0 10px;
  border-radius: 8px;
  line-height: 1;
}
.icon-btn:hover { background: var(--accent-light); color: var(--accent); }
.icon-btn.danger:hover { background: rgba(255,59,48,0.1); color: #ff3b30; }
.leaf-card { display: flex; flex-direction: column; gap: 8px; cursor: pointer; }
.edit-textarea {
  flex: 1;
  min-width: 0;
  width: 100%;
  min-height: 72px;
  resize: none;
  overflow: hidden;
  field-sizing: content;
  border: none;
  border-radius: 12px;
  padding: 12px 14px 12px 4px;
  font: inherit;
  font-size: 15px;
  line-height: 1.5;
  font-weight: 400;
  box-sizing: border-box;
  outline: none;
  background: transparent;
  color: var(--text-primary);
}
.part-field:has(.img-upload-btn) .edit-textarea {
  padding-bottom: 40px;
}
.part-field:not(:has(.inline-num)):not(:has(.part-label)) .edit-textarea {
  padding-left: 14px;
}
.edit-textarea:focus {
  border: none;
  box-shadow: none;
  background: transparent;
}
.solution-textarea {
  min-height: 96px;
}
.solution-item { display: flex; flex-direction: column; gap: 6px; }
.solution-head { display: flex; align-items: center; justify-content: space-between; }
.solution-name { font-size: 13px; font-weight: 600; }
.solution-head-right { display: flex; gap: 6px; align-items: center; }
.quick-tool-btn {
  border: none; background: var(--bg-input);
  border-radius: 8px; font-size: 12px; font-weight: 400; padding: 4px 10px; cursor: pointer;
  color: var(--accent); min-height: 28px;
}
.solution-del { border: none; background: none; cursor: pointer; color: var(--text-muted); border-radius: 8px; padding: 4px; }
.solution-textarea-wrap { position: relative; }
.img-upload-btn {
  position: absolute;
  left: 12px;
  bottom: 10px;
  height: 28px;
  padding: 0 10px;
  border-radius: 8px;
  background: transparent;
  border: 1px solid transparent;
  color: var(--accent);
  font-size: 12px;
  font-weight: 400;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  cursor: pointer;
  z-index: 1;
  transition: transform 0.2s cubic-bezier(0.4, 0, 0.2, 1), background 0.2s;
}
.img-upload-btn:hover {
  background: var(--accent-light);
  transform: translateY(-0.5px);
}
.add-btn {
  display: inline-flex; align-items: center; gap: 6px; margin-top: 4px;
  padding: 8px 14px; border: none;
  background: var(--bg-input); color: var(--text-secondary); font-size: 13px; font-weight: 400;
  border-radius: 12px; cursor: pointer; font-family: inherit;
  transition: transform 0.2s cubic-bezier(0.4, 0, 0.2, 1), background 0.2s, color 0.2s;
}
.add-btn:hover { color: var(--accent); background: var(--accent-light); transform: translateY(-0.5px); }
.add-btn-sm { padding: 6px 12px; font-size: 12px; }
.no-analysis-check {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  margin-top: 8px;
  padding: 12px 16px;
  min-height: 44px;
  border-radius: 12px;
  background: var(--bg-input);
  cursor: pointer;
  user-select: none;
}
.no-analysis-copy {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.no-analysis-title {
  font-size: 15px;
  font-weight: 400;
  line-height: 1.3;
  color: var(--text-primary);
  letter-spacing: -0.01em;
}
.no-analysis-caption {
  font-size: 12px;
  font-weight: 400;
  line-height: 1.4;
  color: var(--text-muted);
}
.no-analysis-check input[type='checkbox'] {
  appearance: none;
  -webkit-appearance: none;
  flex-shrink: 0;
  width: 51px;
  height: 31px;
  margin: 0;
  padding: 0;
  border: none;
  border-radius: 999px;
  background: #e9e9ea;
  cursor: pointer;
  position: relative;
  box-shadow: none;
  transition: background 0.2s cubic-bezier(0.4, 0, 0.2, 1);
}
.no-analysis-check input[type='checkbox']::after {
  content: '';
  position: absolute;
  top: 2px;
  left: 2px;
  width: 27px;
  height: 27px;
  border-radius: 50%;
  background: #fff;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.16), 0 1px 1px rgba(0, 0, 0, 0.06);
  transition: transform 0.2s cubic-bezier(0.4, 0, 0.2, 1);
}
.no-analysis-check input[type='checkbox']:checked {
  background: var(--success, #34c759);
}
.no-analysis-check input[type='checkbox']:checked::after {
  transform: translateX(20px);
}
.no-analysis-check input[type='checkbox']:focus,
.no-analysis-check input[type='checkbox']:focus-visible {
  outline: none;
  border: none;
  box-shadow: none;
  background: #e9e9ea;
}
.no-analysis-check input[type='checkbox']:checked:focus,
.no-analysis-check input[type='checkbox']:checked:focus-visible {
  background: var(--success, #34c759);
}
[data-theme='dark'] .no-analysis-check input[type='checkbox'],
[data-theme='dark'] .no-analysis-check input[type='checkbox']:focus {
  background: #39393d;
}
[data-theme='dark'] .no-analysis-check input[type='checkbox']:checked,
[data-theme='dark'] .no-analysis-check input[type='checkbox']:checked:focus {
  background: var(--success, #30d158);
}
@media (prefers-reduced-motion: reduce) {
  .chip, .add-btn, .no-analysis-check input[type='checkbox'],
  .no-analysis-check input[type='checkbox']::after { transition: none; }
  .chip:hover, .add-btn:hover { transform: none; }
}
</style>
