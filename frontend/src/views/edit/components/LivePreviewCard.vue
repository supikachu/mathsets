<script setup lang="ts">
import { computed, ref, watch, onMounted, onBeforeUnmount, nextTick } from 'vue'
import { AppIcon } from '@/components/ui'
import LatexRender, { type ImageClickPayload } from '@/components/LatexRender.vue'
import QuestionOptions from '@/components/QuestionOptions.vue'
import QuestionStructureView from '@/components/QuestionStructureView.vue'
import { isSimpleTree, partsHaveContent, type QuestionPart } from '@/utils/questionParts'
import { choiceAnswerLatex, extractChoiceLetters } from '@/utils/choiceAnswer'

const props = defineProps<{
  form: {
    stem: string
    question_type: string
    options: { label: string; content: string }[]
    correctAnswer: any
    blanks: { position: number; answer: string }[]
    sub_answers: string[]
    solutions: string[]
    parts?: QuestionPart[]
    difficulty: string
    difficulty_coefficient: number
  }
  imageEditable?: boolean
}>()

const emit = defineEmits<{
  (e: 'image-click', payload: ImageClickPayload): void
}>()

const previewOptions = computed(() => {
  if (!Array.isArray(props.form.options)) return []
  return props.form.options.filter(o => o.content)
})

const highlightLabels = computed(() => extractChoiceLetters(props.form.correctAnswer))

const previewRootRef = ref<HTMLElement | null>(null)
const optionsRef = ref<InstanceType<typeof QuestionOptions> | null>(null)
let visibilityObserver: IntersectionObserver | null = null

function schedulePreviewLayout() {
  nextTick(() => {
    setTimeout(() => optionsRef.value?.computeLayout(), 280)
  })
}

watch(
  () => [props.form.stem, props.form.options, props.form.question_type],
  () => schedulePreviewLayout(),
  { deep: true },
)

onMounted(() => {
  const root = previewRootRef.value
  if (!root) return
  visibilityObserver = new IntersectionObserver((entries) => {
    if (entries.some(e => e.isIntersecting && e.intersectionRatio > 0)) {
      schedulePreviewLayout()
    }
  }, { threshold: 0.01 })
  visibilityObserver.observe(root)
})

onBeforeUnmount(() => {
  visibilityObserver?.disconnect()
  visibilityObserver = null
})

const typeOptions = [
  { label: '选择题', value: 'choice' },
  { label: '填空题', value: 'fill' },
  { label: '解答题', value: 'solution' },
]

const difficultyStars = computed(() => {
  if (props.form.difficulty === 'easy') return props.form.difficulty_coefficient > 0.8 ? 1 : 2
  if (props.form.difficulty === 'medium') return 3
  return props.form.difficulty_coefficient < 0.3 ? 5 : 4
})

const activeSolution = ref(0)
const previewSolutions = computed(() => props.form.solutions.filter(s => s.trim()))
const solutionParts = computed(() => props.form.parts || [])
const isPreviewEmpty = computed(() =>
  !props.form.stem
  && !previewSolutions.value.length
  && props.form.options.every(o => !o.content)
  && !partsHaveContent(solutionParts.value),
)

watch(() => previewSolutions.value.length, (newLen) => {
  if (activeSolution.value >= newLen) {
    activeSolution.value = Math.max(0, newLen - 1)
  }
})

const hasCorrectAnswer = computed(() => extractChoiceLetters(props.form.correctAnswer).length > 0)

// 答案预览：统一包裹在单个 $\mathrm{...}$ 中渲染；已是 $\mathrm{B}$ 时不再套一层
const displayCorrectAnswer = computed(() => choiceAnswerLatex(props.form.correctAnswer))

const cnNums = ['一', '二', '三', '四', '五', '六', '七', '八', '九', '十']
function cnNum(n: number): string {
  return cnNums[n - 1] || String(n)
}

function splitSolution(text: string): { body: string; conclusion: string } {
  if (!text) return { body: '', conclusion: '' }
  const patterns = [
    /(?:故|因此|所以|综上)[选答]\s*[A-Z](?:[、,，]\s*[A-Z])*\s*。?\s*$/,
    /(?:故|因此|所以|综上)[^。\n]*答案[^。\n]*[。]?\s*$/,
    /(?:故|因此|所以|综上)[^。\n]*[。]?\s*$/,
    /故选\s*[A-Z](?:[、,，]\s*[A-Z])*\s*。?\s*$/,
  ]
  for (const p of patterns) {
    const m = text.match(p)
    if (m) {
      const idx = text.lastIndexOf(m[0])
      return { body: text.substring(0, idx).trim(), conclusion: m[0].trim() }
    }
  }
  return { body: text.trim(), conclusion: '' }
}
</script>

<template>
  <div ref="previewRootRef" class="preview-col border-none bg-transparent">
    <div class="preview-col-inner p-0">
      <!-- 骨架屏（无输入时） -->
      <div v-if="isPreviewEmpty" class="preview-skeleton">
        <div class="skeleton-line skeleton-title"></div>
        <div class="skeleton-line skeleton-text"></div>
        <div class="skeleton-line skeleton-text skeleton-short"></div>
        <div class="skeleton-line skeleton-text"></div>
        <div class="skeleton-gap"></div>
        <div class="skeleton-line skeleton-opt"></div>
        <div class="skeleton-line skeleton-opt"></div>
        <div class="skeleton-line skeleton-opt"></div>
        <div class="skeleton-line skeleton-opt"></div>
        <div class="skeleton-gap"></div>
        <div class="skeleton-line skeleton-answer"></div>
        <div class="skeleton-line skeleton-text skeleton-short"></div>
      </div>

      <!-- 试卷卡片（有输入时） -->
      <div v-else class="paper-card math-content pb-32 bg-[var(--bg-card)] rounded-2xl shadow-md dark:shadow-none border border-transparent dark:border-[#3A3A3C]">
        <div class="paper-card-header">
          <span class="paper-type-badge">{{ typeOptions.find(t => t.value === form.question_type)?.label }}</span>
          <span class="paper-difficulty">
            <AppIcon v-for="n in 5" :key="n" name="star" :size="12" :class="{ active: difficultyStars >= n }" class="paper-star" />
          </span>
        </div>

        <!-- 题干 -->
        <div class="paper-stem">
          <LatexRender
            :text="form.stem || ''"
            :mode="imageEditable ? 'editable' : 'readonly'"
            @image-click="emit('image-click', $event)"
          />
        </div>

        <!-- 选择题选项：与题库列表卡片同一套 QuestionOptions 智能 4/2/1 列 -->
        <QuestionOptions
          v-if="form.question_type === 'choice' && previewOptions.length"
          ref="optionsRef"
          :options="previewOptions"
          :image-editable="imageEditable"
          :highlight-labels="highlightLabels"
          @image-click="emit('image-click', $event)"
        />

        <!-- 解答题：小问题干紧跟总前提 -->
        <QuestionStructureView
          v-if="form.question_type === 'solution' && solutionParts.length"
          section="stems"
          :parts="solutionParts"
          :image-editable="imageEditable"
          @image-click="emit('image-click', $event)"
        />

        <!-- 解答题：答案、解析分块 -->
        <template v-if="form.question_type === 'solution' && solutionParts.length && (!isSimpleTree(solutionParts) || partsHaveContent(solutionParts) || form.stem)">
          <div class="paper-answer-block">
            <div class="paper-answer-label">答案</div>
            <div class="paper-answer-content">
              <QuestionStructureView
                section="answers"
                :parts="solutionParts"
                :image-editable="imageEditable"
                @image-click="emit('image-click', $event)"
              />
            </div>
          </div>
          <div class="paper-answer-block paper-analysis">
            <div class="paper-answer-label">解析</div>
            <div class="paper-answer-content">
              <QuestionStructureView
                section="analyses"
                :parts="solutionParts"
                :image-editable="imageEditable"
                @image-click="emit('image-click', $event)"
              />
            </div>
          </div>
        </template>

        <!-- 选择题/填空题：答案 & 解析 -->
        <template v-else-if="form.question_type !== 'solution'">
          <div class="paper-answer-block">
            <div class="paper-answer-label">答案</div>
            <div class="paper-answer-content">
              <template v-if="form.question_type === 'choice' && hasCorrectAnswer">
                <LatexRender :text="displayCorrectAnswer" :inline="true" />
              </template>
              <template v-else-if="form.question_type === 'fill' && form.blanks.some(b => b.answer)">
                <span v-for="(blank, i) in form.blanks.filter(b => b.answer)" :key="i">
                  {{ form.blanks.indexOf(blank) + 1 }}. <LatexRender :text="blank.answer" :inline="true" />&nbsp;
                </span>
              </template>
              <span class="paper-muted" v-else>—</span>
            </div>
          </div>

          <div v-if="previewSolutions.length" class="paper-answer-block paper-analysis">
            <div class="paper-answer-label flex justify-between items-center">
              <span>解析</span>
              <div v-if="previewSolutions.length > 1" class="sol-seg">
                <button
                  v-for="(s, i) in previewSolutions"
                  :key="i"
                  class="sol-seg-btn"
                  :class="{ active: activeSolution === i }"
                  @click="activeSolution = i"
                >解法{{ cnNum(i + 1) }}</button>
              </div>
            </div>
            <div class="paper-answer-content">
              <Transition name="sol-fade" mode="out-in">
                <LatexRender
                  :key="activeSolution"
                  :text="splitSolution(previewSolutions[activeSolution]).body"
                  :mode="imageEditable ? 'editable' : 'readonly'"
                  @image-click="emit('image-click', $event)"
                />
              </Transition>
            </div>
            <div v-if="splitSolution(previewSolutions[activeSolution]).conclusion" class="paper-conclusion">
              <LatexRender
                :text="splitSolution(previewSolutions[activeSolution]).conclusion"
                :mode="imageEditable ? 'editable' : 'readonly'"
                @image-click="emit('image-click', $event)"
              />
            </div>
          </div>
        </template>
      </div>
    </div>
  </div>
</template>

<style scoped>
.preview-col {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.preview-col-inner {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 24px;
}

/* 试卷卡片 - 悬浮纸张效果 */
.paper-card {
  background: var(--bg-card);
  border-radius: 16px;
  padding: 24px;
  box-shadow: 0 1px 3px rgb(0 0 0 / 0.05);
  border: 1px solid hsl(0 0% 91%);
}

[data-theme='dark'] .paper-card {
  border: 1px solid rgba(255, 255, 255, 0.08);
}

.paper-card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 16px;
  padding-bottom: 12px;
  border-bottom: 1px solid hsl(0 0% 91%);
}

[data-theme='dark'] .paper-card-header {
  border-bottom-color: rgba(255, 255, 255, 0.06);
}

.paper-type-badge {
  font-size: 13px;
  font-weight: 600;
  color: var(--accent);
}

.paper-difficulty {
  display: flex;
  gap: 1px;
}

.paper-star {
  color: #d1d1d6;
  transition: color 0.2s;
}

.paper-star.active {
  color: #ff9500;
}

.paper-stem {
  font-size: 14px;
  line-height: 1.8;
  color: #1d1d1f;
  margin-bottom: 14px;
  word-break: break-word;
  font-family: var(--font-cn-isolated);
  max-width: 100%;
  min-width: 0;
  overflow-x: auto;
}

[data-theme='dark'] .paper-stem {
  color: #f5f5f7;
}

/* 答案卡片 — 与详情页参考答案一致：莫兰迪极淡蓝底 */
.paper-answer-block {
  background: #f4f8fc;
  border-radius: 16px;
  padding: 20px 24px;
  margin-top: 24px;
  border: none;
}

.paper-answer-block:hover {
  background: #edf3f9;
}

[data-theme='dark'] .paper-answer-block {
  background: rgba(100, 160, 220, 0.08);
}

[data-theme='dark'] .paper-answer-block:hover {
  background: rgba(100, 160, 220, 0.12);
}

/* 解析卡片 — 与详情页解析一致：系统柔和灰底 */
.paper-answer-block.paper-analysis {
  background: #f5f5f7;
}

.paper-answer-block.paper-analysis:hover {
  background: #ebebef;
}

[data-theme='dark'] .paper-answer-block.paper-analysis {
  background: rgba(255, 255, 255, 0.05);
}

[data-theme='dark'] .paper-answer-block.paper-analysis:hover {
  background: rgba(255, 255, 255, 0.08);
}

.paper-answer-label {
  font-size: 14px;
  font-weight: 600;
  color: #1d1d1f;
  letter-spacing: -0.01em;
  margin-bottom: 16px;
  text-transform: none;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  min-width: 0;
}

[data-theme='dark'] .paper-answer-label {
  color: #f5f5f7;
}

.paper-answer-content {
  font-size: 13px;
  line-height: 1.7;
  color: var(--text-primary);
  font-family: var(--font-cn-isolated);
  max-width: 100%;
  min-width: 0;
  overflow-x: auto;
}

.paper-correct-answer {
  font-weight: 600;
  font-size: 16px;
  color: var(--accent);
}

.paper-muted {
  color: var(--text-muted);
}

/* 预览端分段切换 */
.sol-seg {
  display: inline-flex;
  gap: 2px;
  padding: 2px;
  border-radius: var(--radius-full);
  background: var(--bg-input);
  max-width: 100%;
  min-width: 0;
  overflow-x: auto;
  flex-shrink: 1;
  flex-wrap: nowrap;
}

.sol-seg-btn {
  padding: 4px 12px;
  border: none;
  border-radius: var(--radius-full);
  background: transparent;
  font-size: 12px;
  font-weight: 400;
  color: var(--text-muted);
  cursor: pointer;
  transition: transform 0.2s cubic-bezier(0.4, 0, 0.2, 1);
}

.sol-seg-btn.active {
  background: var(--bg-card);
  color: var(--accent);
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.08);
}

[data-theme='dark'] .sol-seg-btn.active {
  background: rgba(255, 255, 255, 0.12);
}

/* 结论区 - 极简引用样式（去除大面积背景框，改用轻量左侧边框高亮） */
.paper-conclusion {
  margin-top: 14px;
  padding: 4px 0 4px 12px;
  border-left: 3px solid var(--accent);
  background: transparent;
  font-size: 13.5px;
  line-height: 1.6;
  color: var(--text-primary);
  font-weight: 500;
}

/* 淡入淡出过渡 */
.sol-fade-enter-active,
.sol-fade-leave-active {
  transition: opacity 0.2s ease;
}

.sol-fade-enter-from,
.sol-fade-leave-to {
  opacity: 0;
}

.paper-sub-answer {
  display: flex;
  align-items: flex-start;
  gap: 4px;
  margin-bottom: 4px;
}

.paper-sub-num {
  font-weight: 600;
  flex-shrink: 0;
}

/* 骨架屏样式 */
.preview-skeleton {
  padding: 24px 28px;
  background: var(--bg-card);
  border-radius: 16px;
  box-shadow: var(--shadow-sm);
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.skeleton-line {
  height: 16px;
  background: linear-gradient(90deg, var(--bg-input) 25%, var(--border-color) 37%, var(--bg-input) 63%);
  background-size: 400% 100%;
  animation: skeleton-loading 1.4s ease infinite;
  border-radius: 4px;
}

.skeleton-title {
  width: 30%;
  height: 20px;
  margin-bottom: 12px;
}

.skeleton-text {
  width: 90%;
}

.skeleton-text.skeleton-short {
  width: 50%;
}

.skeleton-gap {
  height: 12px;
}

.skeleton-opt {
  width: 40%;
  height: 14px;
}

.skeleton-answer {
  width: 25%;
  height: 18px;
  margin-bottom: 8px;
}

@keyframes skeleton-loading {
  0% { background-position: 200% 0; }
  100% { background-position: -200% 0; }
}
</style>
