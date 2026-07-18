<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { AppIcon } from '@/components/ui'
import LatexRender from '@/components/LatexRender.vue'
import { useOptionsLayout } from '@/composables/useOptionsLayout'

const props = defineProps<{
  form: {
    stem: string
    question_type: string
    options: { label: string; content: string }[]
    correctAnswer: any
    blanks: { position: number; answer: string }[]
    sub_answers: string[]
    solutions: string[]
    difficulty: string
    difficulty_coefficient: number
  }
}>()

// Writable options layout tracking
const previewOptionsContainerRef = ref<HTMLElement | null>(null)
const previewOptions = computed(() => {
  if (!Array.isArray(props.form.options)) return []
  return props.form.options.filter(o => o.content)
})

const { layout: previewLayout } = useOptionsLayout(previewOptionsContainerRef, previewOptions, '.paper-opt')

const optionsLayout = computed(() => {
  if (previewOptions.value.some(opt => opt.content.includes('!['))) return '1col'
  if (previewLayout.value === 'grid-4') return '4col'
  if (previewLayout.value === 'grid-2') return '2col'
  return '1col'
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

watch(() => previewSolutions.value.length, (newLen) => {
  if (activeSolution.value >= newLen) {
    activeSolution.value = Math.max(0, newLen - 1)
  }
})

function isOptionCorrect(label: string): boolean {
  if (Array.isArray(props.form.correctAnswer)) return props.form.correctAnswer.includes(label)
  return props.form.correctAnswer === label
}

const hasCorrectAnswer = computed(() => {
  if (Array.isArray(props.form.correctAnswer)) return props.form.correctAnswer.length > 0
  return !!props.form.correctAnswer
})

const displayCorrectAnswer = computed(() => {
  if (Array.isArray(props.form.correctAnswer)) return props.form.correctAnswer.join('、')
  return props.form.correctAnswer || ''
})

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
  <div class="preview-col border-none bg-transparent">
    <div class="preview-col-inner p-0">
      <!-- 骨架屏（无输入时） -->
      <div v-if="!form.stem && !form.solutions.some(s => s.trim()) && form.options.every(o => !o.content)" class="preview-skeleton">
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
          <LatexRender :text="form.stem || ''" />
        </div>

        <!-- 选择题选项 -->
        <div v-if="form.question_type === 'choice' && previewOptions.length" ref="previewOptionsContainerRef" class="paper-options" :class="'paper-options-' + optionsLayout">
          <div
            v-for="opt in previewOptions"
            :key="opt.label"
            class="paper-opt"
            :class="{ correct: isOptionCorrect(opt.label) }"
          >
            <span class="paper-opt-letter">{{ opt.label }}.</span>
            <LatexRender :text="opt.content" :inline="true" />
          </div>
        </div>

        <!-- 答案 & 解析 -->
        <div class="paper-answer-block">
          <div class="paper-answer-label">答案</div>
          <div class="paper-answer-content">
            <template v-if="form.question_type === 'choice' && hasCorrectAnswer">
              <span class="paper-correct-answer">{{ displayCorrectAnswer }}</span>
            </template>
            <template v-else-if="form.question_type === 'fill' && form.blanks.some(b => b.answer)">
              <span v-for="(blank, i) in form.blanks.filter(b => b.answer)" :key="i">
                {{ form.blanks.indexOf(blank) + 1 }}. <LatexRender :text="blank.answer" :inline="true" />&nbsp;
              </span>
            </template>
            <template v-else-if="form.question_type === 'solution' && form.sub_answers.some(a => a.trim())">
              <div v-for="(ans, i) in form.sub_answers" :key="i" class="paper-sub-answer">
                <span class="paper-sub-num">({{ i + 1 }})</span>
                <LatexRender :text="ans" :inline="false" />
              </div>
            </template>
            <span class="paper-muted" v-else>—</span>
          </div>
        </div>

        <div v-if="previewSolutions.length" class="paper-answer-block">
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
              <LatexRender :key="activeSolution" :text="splitSolution(previewSolutions[activeSolution]).body" />
            </Transition>
          </div>
          <div v-if="splitSolution(previewSolutions[activeSolution]).conclusion" class="paper-conclusion">
            <LatexRender :text="splitSolution(previewSolutions[activeSolution]).conclusion" />
          </div>
        </div>
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
}

/* 试卷卡片 - 悬浮纸张效果 */
.paper-card {
  background: var(--bg-card);
  border-radius: 16px;
  padding: 24px 28px 128px 28px;
  box-shadow: var(--shadow-md);
  border: none;
  height: calc(100vh - 120px);
  overflow-y: auto;
}

[data-theme='dark'] .paper-card {
  border: 1px solid #3a3a3c;
}

.paper-card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 16px;
  padding-bottom: 12px;
  border-bottom: 1px solid #f0f0f0;
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
}

[data-theme='dark'] .paper-stem {
  color: #f5f5f7;
}

.paper-options {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px 24px;
  margin-bottom: 14px;
}

/* 4列横排 — 短选项紧凑排列 */
.paper-options-4col {
  grid-template-columns: repeat(4, 1fr);
  gap: 12px;
}

/* 2列双排 — 默认布局 */
.paper-options-2col {
  grid-template-columns: repeat(2, 1fr);
  gap: 12px 24px;
}

/* 1列竖排 — 长选项或含图片 */
.paper-options-1col {
  grid-template-columns: 1fr;
  gap: 8px;
}

.paper-opt {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 13px;
  line-height: 1.7;
  color: #3a3a3c;
  padding: 4px 0;
  font-family: var(--font-cn-isolated);
}

.paper-opt.correct {
  color: var(--accent);
}

[data-theme='dark'] .paper-opt {
  color: #d1d1d6;
}

.paper-opt-letter {
  font-weight: 600;
  flex-shrink: 0;
}

/* 选项内图片样式 */
.paper-opt img.latex-img {
  max-height: 80px;
  width: auto;
  display: inline-block;
  vertical-align: middle;
  margin: 4px 0;
  border-radius: 4px;
}

/* 答案/解析区块 */
.paper-answer-block {
  background: #f5f5f7;
  border-radius: 8px;
  padding: 12px 16px;
  margin-top: 10px;
}

[data-theme='dark'] .paper-answer-block {
  background: rgba(255, 255, 255, 0.04);
}

.paper-answer-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.04em;
  margin-bottom: 4px;
}

.paper-answer-content {
  font-size: 13px;
  line-height: 1.7;
  color: var(--text-primary);
  font-family: var(--font-cn-isolated);
}

.paper-correct-answer {
  font-weight: 700;
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
}

.sol-seg-btn {
  padding: 3px 10px;
  border: none;
  border-radius: var(--radius-full);
  background: transparent;
  font-size: 11px;
  font-weight: 500;
  color: var(--text-muted);
  cursor: pointer;
  transition: all 0.2s ease;
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
  font-weight: 700;
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
