<script setup lang="ts">
import { computed, reactive } from 'vue'
import LatexRender, { type ImageClickPayload } from '@/components/LatexRender.vue'
import {
  type QuestionPart,
  cnNum,
  flattenParts,
  isLeaf,
  isSimpleTree,
  walkLeavesWithPath,
} from '@/utils/questionParts'

const props = withDefaults(defineProps<{
  parts: QuestionPart[]
  section?: 'stems' | 'answers' | 'analyses' | 'all'
  showAnswers?: boolean
  showAnalyses?: boolean
  imageEditable?: boolean
  selectedId?: string
}>(), {
  section: 'all',
  showAnswers: true,
  showAnalyses: true,
  imageEditable: false,
  selectedId: '',
})

const emit = defineEmits<{
  select: [id: string]
  'image-click': [payload: ImageClickPayload]
}>()

const simple = computed(() => isSimpleTree(props.parts))
const nodes = computed(() => flattenParts(props.parts))
const leaves = computed(() => walkLeavesWithPath(props.parts))
const activeMap = reactive<Record<string, number>>({})

const showStems = computed(() => props.section === 'stems' || props.section === 'all')
const showAnswerSection = computed(() =>
  (props.section === 'answers' || props.section === 'all') && props.showAnswers,
)
const showAnalysisSection = computed(() =>
  (props.section === 'analyses' || props.section === 'all') && props.showAnalyses,
)
const grouped = computed(() => props.section === 'all')

function visibleAnalyses(part: QuestionPart) {
  return (part.analyses || []).filter((a) => (a.content || '').trim())
}

function solIndex(id: string) {
  return activeMap[id] ?? 0
}

function setSol(id: string, i: number) {
  activeMap[id] = i
}

function onSelect(id: string) {
  emit('select', id)
}

const stemNodes = computed(() =>
  nodes.value.filter((n) => !simple.value || n.part.stem.trim()),
)

const answerLeaves = computed(() =>
  leaves.value.filter((l) => (l.part.answer || '').trim()),
)

const analysisLeaves = computed(() =>
  leaves.value.filter((l) => visibleAnalyses(l.part).length > 0),
)
</script>

<template>
  <div class="part-tree" :class="{ simple, grouped }">
    <div v-if="showStems && stemNodes.length" class="stem-block">
      <div
        v-for="node in stemNodes"
        :key="'s-' + node.part.id"
        class="part-node"
        :class="{
          selected: selectedId === node.part.id,
          leaf: isLeaf(node.part),
        }"
        :style="{ marginLeft: simple ? '0' : `${(node.depth - 1) * 16}px` }"
        @click="onSelect(node.part.id)"
      >
        <div class="part-head">
          <span v-if="!simple" class="part-label">{{ node.part.label }}</span>
          <div v-if="node.part.stem.trim()" class="part-stem">
            <LatexRender
              :text="node.part.stem"
              :mode="imageEditable ? 'editable' : 'readonly'"
              @image-click="emit('image-click', $event)"
            />
          </div>
        </div>
      </div>
    </div>

    <div v-if="showAnswerSection" class="leaf-block">
      <div v-if="grouped && section === 'all'" class="block-title">答案</div>
      <div v-if="!answerLeaves.length" class="muted">—</div>
      <div
        v-for="leaf in answerLeaves"
        :key="'a-' + leaf.part.id"
        class="part-node leaf-row"
        :class="{ selected: selectedId === leaf.part.id }"
        @click="onSelect(leaf.part.id)"
      >
        <span v-if="leaf.pathLabel" class="part-label">{{ leaf.pathLabel }}</span>
        <div class="leaf-body">
          <LatexRender :text="leaf.part.answer || ''" />
        </div>
      </div>
    </div>

    <div v-if="showAnalysisSection" class="leaf-block">
      <div v-if="grouped && section === 'all'" class="block-title">解析</div>
      <div v-if="!analysisLeaves.length" class="muted">—</div>
      <div
        v-for="leaf in analysisLeaves"
        :key="'x-' + leaf.part.id"
        class="part-node leaf-row"
        :class="{ selected: selectedId === leaf.part.id }"
        @click="onSelect(leaf.part.id)"
      >
        <span v-if="leaf.pathLabel" class="part-label">{{ leaf.pathLabel }}</span>
        <div class="leaf-body">
          <div class="analysis-head">
            <div v-if="visibleAnalyses(leaf.part).length > 1" class="sol-seg">
              <button
                v-for="(s, i) in visibleAnalyses(leaf.part)"
                :key="s.id || i"
                type="button"
                class="sol-seg-btn"
                :class="{ active: solIndex(leaf.part.id) === i }"
                @click.stop="setSol(leaf.part.id, i)"
              >{{ s.title || `解法${cnNum(i + 1)}` }}</button>
            </div>
          </div>
          <LatexRender
            :text="visibleAnalyses(leaf.part)[solIndex(leaf.part.id)]?.content || ''"
            :mode="imageEditable ? 'editable' : 'readonly'"
            @image-click="emit('image-click', $event)"
          />
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.part-tree { display: flex; flex-direction: column; gap: 8px; }
.part-tree.grouped { gap: 14px; }
.stem-block, .leaf-block { display: flex; flex-direction: column; gap: 6px; }
.block-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-muted, #86868b);
  letter-spacing: 0.04em;
  margin-bottom: 2px;
}
.part-node {
  border-radius: 8px;
  padding: 4px 0;
  cursor: pointer;
}
.part-node.selected { outline: 1px solid var(--accent, #0071e3); outline-offset: 4px; }
.part-head { display: flex; gap: 8px; align-items: flex-start; }
.leaf-row { display: flex; gap: 8px; align-items: flex-start; }
.part-label {
  flex-shrink: 0;
  font-weight: 600;
  font-size: 13px;
  color: var(--accent, #0071e3);
  min-width: 28px;
  line-height: 1.8;
}
.part-stem, .leaf-body { flex: 1; min-width: 0; font-size: 14px; line-height: 1.8; }
.muted { color: var(--text-muted, #86868b); font-size: 13px; }
.analysis-head {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
  margin-bottom: 4px;
}
.sol-seg { display: inline-flex; gap: 2px; padding: 2px; border-radius: 999px; background: var(--bg-input, #f5f5f7); }
.sol-seg-btn {
  padding: 2px 8px; border: none; border-radius: 999px; background: transparent;
  font-size: 11px; color: var(--text-muted, #86868b); cursor: pointer;
}
.sol-seg-btn.active { background: var(--bg-card, #fff); color: var(--text-primary, #1d1d1f); }
</style>
