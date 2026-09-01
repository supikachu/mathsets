<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { questionApi, type QuestionDetail } from '@/api/client'
import { AppIcon } from '@/components/ui'
import LatexRender from '@/components/LatexRender.vue'
import QuestionOptions from '@/components/QuestionOptions.vue'
import QuestionStructureView from '@/components/QuestionStructureView.vue'
import { useQuestionBasket } from '@/composables/useQuestionBasket'
import { useToast } from '@/composables/useToast'
import { typeLabel, diffLabel } from '@/utils/questionDisplay'
import { partsFromStructureJson } from '@/utils/questionParts'
import { extractChoiceLetters, extractFillBlanks } from '@/utils/choiceAnswer'

const router = useRouter()
const toast = useToast()
const basket = useQuestionBasket()

const loading = ref(false)
const items = ref<QuestionDetail[]>([])
const ids = computed(() => basket.getAll())

const allExpanded = ref(false)
const expandedMap = ref<Record<string, boolean>>({})
const selectedId = ref('')
const sortMode = ref<'type' | 'added'>('type')
/** 按题型时：组内按加入顺序 / 由易到难 */
const typeSubSort = ref<'added' | 'easy'>('added')
/** 按加入顺序时：正序 / 倒序 */
const addedDir = ref<'asc' | 'desc'>('asc')

function difficultyNum(q: QuestionDetail): number {
  const d = typeof q.difficulty === 'number' ? q.difficulty : parseInt(String(q.difficulty), 10)
  return Number.isNaN(d) ? 3 : d
}

function addedIndexMap() {
  return new Map(basket.getAll().map((id, i) => [id, i]))
}

function sortByAddedOrder(list: QuestionDetail[], reverse = false) {
  const idx = addedIndexMap()
  const sorted = [...list].sort((a, b) => (idx.get(a.id) ?? 0) - (idx.get(b.id) ?? 0))
  return reverse ? sorted.reverse() : sorted
}

function sortByEase(list: QuestionDetail[]) {
  const idx = addedIndexMap()
  return [...list].sort((a, b) => {
    const diff = difficultyNum(a) - difficultyNum(b)
    if (diff !== 0) return diff
    return (idx.get(a.id) ?? 0) - (idx.get(b.id) ?? 0)
  })
}

async function load() {
  const current = basket.getAll()
  if (!current.length) {
    items.value = []
    selectedId.value = ''
    loading.value = false
    return
  }

  const cached = new Map(items.value.map((q) => [q.id, q]))
  const missing = current.filter((id) => !cached.has(id))
  const isFirstPaint = items.value.length === 0
  if (isFirstPaint) loading.value = true

  const fetched = new Map<string, QuestionDetail>()
  if (missing.length) {
    const rows = await Promise.all(
      missing.map((id) => questionApi.get(id).then((r) => r.data).catch(() => null)),
    )
    missing.forEach((id, i) => {
      const row = rows[i]
      if (row) fetched.set(id, row)
      else basket.remove(id)
    })
  }

  const next: QuestionDetail[] = []
  for (const id of basket.getAll()) {
    const q = cached.get(id) || fetched.get(id)
    if (q) next.push(q)
  }
  items.value = next
  if (!next.some((q) => q.id === selectedId.value)) {
    selectedId.value = next[0]?.id ?? ''
  }
  loading.value = false
}

watch(() => Array.from(basket.basketIds.value).join(','), load, { immediate: true })

function extractOptionsFromStem(stem: string): {
  cleanStem: string
  options: { label: string; content: string }[]
} {
  if (!stem) return { cleanStem: '', options: [] }
  const optMatch = stem.match(/(?:^|\n|\s+)(?:[A-D][.、\s:：]|\([A-D]\))/i)
  if (!optMatch || optMatch.index === undefined) {
    return { cleanStem: stem, options: [] }
  }
  const cleanStem = stem.slice(0, optMatch.index).trim()
  const optSection = stem.slice(optMatch.index).trim()
  const regex =
    /(?:^|\n|\s+|\b)([A-D])[.、\s:：)]\s*([\s\S]*?)(?=(?:(?:\n|\s+|\b)[A-D][.、\s:：)])|$)/gi
  const options: { label: string; content: string }[] = []
  let match: RegExpExecArray | null
  while ((match = regex.exec(optSection)) !== null) {
    const label = match[1].toUpperCase()
    const content = (match[2] || '').trim()
    if (content) options.push({ label, content })
  }
  if (options.length >= 2) return { cleanStem, options }
  return { cleanStem: stem, options: [] }
}

function parseOptions(raw: unknown): { label: string; content: string }[] {
  if (!raw) return []
  let opts: unknown = raw
  if (typeof opts === 'string') {
    try {
      opts = JSON.parse(opts)
    } catch {
      return []
    }
  }
  if (!Array.isArray(opts)) return []
  return opts.map((opt: unknown) => {
    if (typeof opt === 'string') {
      const m = opt.match(/^([A-Z])[.、．\s]\s*(.*)$/)
      if (m) return { label: m[1], content: m[2] }
      return { label: '', content: opt }
    }
    if (opt && typeof opt === 'object' && 'label' in opt) {
      const o = opt as { label: string; content?: string }
      return { label: o.label, content: o.content || '' }
    }
    return { label: '', content: String(opt) }
  })
}

function getParsedQuestion(q: QuestionDetail) {
  let options = parseOptions(q.options)
  let stem = q.stem || ''
  if (
    options.length === 0 &&
    (q.question_type === 'choice' || q.question_type === 'multiple')
  ) {
    const extracted = extractOptionsFromStem(stem)
    if (extracted.options.length) {
      options = extracted.options
      stem = extracted.cleanStem
    }
  }
  return { stem, options }
}

function getCorrectLabels(q: QuestionDetail): string[] {
  return extractChoiceLetters(q.correct_answer)
}

function formatAnswerText(q: QuestionDetail): string {
  const letters = extractChoiceLetters(q.correct_answer)
  if (letters.length) return letters.join('')
  const blanks = extractFillBlanks(q.correct_answer)
  if (blanks.length) return blanks.map((b) => b.answer).join('； ')
  if (!q.correct_answer) return '暂无参考答案'
  if (typeof q.correct_answer === 'string') return q.correct_answer
  return '暂无参考答案'
}

function solutionParts(q: QuestionDetail) {
  return partsFromStructureJson(q.structure)
}

function analysisText(q: QuestionDetail): string {
  const raw = q.analysis
  if (!raw) return '暂无详细试题解析'
  if (typeof raw === 'string') return raw
  return '暂无详细试题解析'
}

function isExpanded(id: string) {
  return allExpanded.value || !!expandedMap.value[id]
}

function selectQuestion(id: string) {
  selectedId.value = id
}

function toggleQuestion(id: string) {
  selectQuestion(id)
  if (allExpanded.value) {
    expandedMap.value[id] = false
    allExpanded.value = false
    for (const q of items.value) {
      if (q.id !== id) expandedMap.value[q.id] = true
    }
  } else {
    expandedMap.value[id] = !expandedMap.value[id]
  }
}

function toggleAllAnswers() {
  allExpanded.value = !allExpanded.value
  expandedMap.value = {}
  for (const q of items.value) expandedMap.value[q.id] = allExpanded.value
}

function bucketType(q: QuestionDetail): string {
  let t = q.question_type || 'choice'
  if (t === 'multiple' || t === 'multi_choice') return 'multi_choice'
  if (t === 'choice' || t === 'single_choice') {
    return getCorrectLabels(q).length > 1 ? 'multi_choice' : 'single_choice'
  }
  if (t === 'fill' || t === 'solution') return t
  return 'composite'
}

interface SectionGroup {
  key: string
  title: string
  typeName: string
  questions: QuestionDetail[]
}

const groupedSections = computed<SectionGroup[]>(() => {
  const qs = items.value
  if (!qs.length) return []

  if (sortMode.value === 'added') {
    return [
      {
        key: 'added',
        title: addedDir.value === 'desc' ? '按加入顺序（倒序）' : '按加入顺序',
        typeName: '加入顺序',
        questions: sortByAddedOrder(qs, addedDir.value === 'desc'),
      },
    ]
  }

  const typeOrder = ['single_choice', 'multi_choice', 'fill', 'solution', 'composite']
  const typeTitleMap: Record<string, string> = {
    single_choice: '单选题',
    multi_choice: '多选题',
    fill: '填空题',
    solution: '解答题',
    composite: '综合题',
  }
  const chineseNums = ['一', '二', '三', '四', '五', '六', '七', '八']
  const buckets: Record<string, QuestionDetail[]> = {
    single_choice: [],
    multi_choice: [],
    fill: [],
    solution: [],
    composite: [],
  }
  for (const q of qs) {
    const t = bucketType(q)
    ;(buckets[t] || buckets.composite).push(q)
  }

  const result: SectionGroup[] = []
  let numIdx = 0
  for (const t of typeOrder) {
    const list = buckets[t]
    if (!list?.length) continue
    const ordered = typeSubSort.value === 'easy' ? sortByEase(list) : sortByAddedOrder(list)
    const numPrefix = chineseNums[numIdx] || `${numIdx + 1}`
    result.push({
      key: t,
      title: `${numPrefix}、${typeTitleMap[t] || t}`,
      typeName: typeTitleMap[t] || t,
      questions: ordered,
    })
    numIdx++
  }
  return result
})

const displayNoMap = computed(() => {
  const map: Record<string, number> = {}
  let n = 0
  for (const sec of groupedSections.value) {
    for (const q of sec.questions) map[q.id] = ++n
  }
  return map
})

const totalScore = computed(() =>
  items.value.reduce((sum, q) => sum + (Number(q.default_score) || 0), 0),
)

const diffSummary = computed(() => {
  if (!items.value.length) return '—'
  const diffs = items.value.map((q) => {
    const d = typeof q.difficulty === 'number' ? q.difficulty : parseInt(String(q.difficulty), 10)
    return Number.isNaN(d) ? 3 : d
  })
  const avg = diffs.reduce((a, b) => a + b, 0) / diffs.length
  if (avg <= 2) return '较易'
  if (avg <= 3.2) return '中等'
  if (avg <= 4.2) return '偏难'
  return '困难'
})

function scrollToQuestion(id: string) {
  selectQuestion(id)
  document.getElementById(`q-${id}`)?.scrollIntoView({ behavior: 'smooth', block: 'center' })
}

function shareBasket() {
  if (navigator.clipboard) {
    navigator.clipboard.writeText(window.location.href)
    toast.success('试题篮链接已复制到剪贴板')
  } else {
    toast.info('当前页面地址：' + window.location.href)
  }
}

function downloadPaper(sectionTitle?: string) {
  if (!items.value.length) {
    toast.info('试题篮是空的，先去题库选题')
    return
  }
  toast.info(sectionTitle ? `正在准备下载【${sectionTitle}】...` : '正在准备生成试卷下载文档...')
  setTimeout(() => window.print(), 300)
}

function savePaper() {
  toast.info('组卷保存即将开放，当前可先下载预览')
}

function showAnalysis() {
  toast.info(`试题篮共 ${items.value.length} 道题，难度评估：${diffSummary.value}，总分 ${totalScore.value}`)
}

function removeOne(id: string) {
  if (selectedId.value === id) {
    const remaining = items.value.filter((q) => q.id !== id)
    selectedId.value = remaining[0]?.id ?? ''
  }
  delete expandedMap.value[id]
  basket.remove(id)
  toast.info('已从试题篮中移除')
}

function clearAll() {
  if (!items.value.length) return
  basket.clear()
  toast.info('试题篮已清空')
}

function goDetail(id: string) {
  router.push(`/questions/${id}`)
}
</script>

<template>
  <div class="apple-paper-scope">
    <div v-if="loading" class="apple-loading-box">
      <div class="apple-spinner"></div>
      <p class="apple-loading-text">正在载入试题篮...</p>
    </div>

    <div v-else class="apple-content-layout">
      <main class="apple-main-column">
        <div class="apple-header-card">
          <router-link to="/questions" class="apple-back-btn">
            <AppIcon name="chevron-left" :size="15" />
            <span>返回题库</span>
          </router-link>

          <div class="apple-title-wrapper">
            <h1 class="apple-paper-title">试题篮</h1>
          </div>

          <div class="apple-meta-pill-group">
            <span class="meta-pill strong">已选 {{ items.length }} 道</span>
            <span class="meta-pill">难度 · {{ diffSummary }}</span>
            <span class="meta-pill">总分 · {{ totalScore }}</span>
            <span class="meta-pill text-blue">含参考答案与解析</span>
          </div>
        </div>

        <div v-if="!items.length" class="apple-empty-state bk-empty">
          <p>试题篮是空的</p>
          <p class="bk-empty-hint">从题库卡片加入题目后，会按组卷样式展示在这里</p>
          <router-link to="/questions" class="apple-basket-cta">
            去题库选题
          </router-link>
        </div>

        <div v-else class="apple-sections-container">
          <section
            v-for="sec in groupedSections"
            :key="sec.key"
            :id="'sec-' + sec.key"
            class="apple-section-block"
          >
            <div class="apple-section-bar">
              <div class="section-title-left">
                <span class="section-accent-dash"></span>
                <h2 class="apple-section-heading">{{ sec.title }}</h2>
                <span class="section-count-tag">{{ sec.questions.length }} 题</span>
              </div>
              <button type="button" class="apple-ghost-btn" @click="downloadPaper(sec.title)">
                <AppIcon name="download" :size="13" />
                <span>下载本大题</span>
              </button>
            </div>

            <TransitionGroup name="bk-card" tag="div" class="apple-card-list">
              <article
                v-for="q in sec.questions"
                :id="'q-' + q.id"
                :key="q.id"
                class="apple-q-card"
                :class="{
                  'is-expanded': isExpanded(q.id),
                  'is-selected': selectedId === q.id,
                }"
                @click="toggleQuestion(q.id)"
              >
                <div class="apple-q-body">
                  <div class="apple-stem-row">
                    <div class="apple-q-index">{{ displayNoMap[q.id] }}.</div>
                    <div class="apple-q-stem">
                      <LatexRender :text="getParsedQuestion(q).stem" />
                      <QuestionStructureView
                        v-if="q.question_type === 'solution' && solutionParts(q).length"
                        section="stems"
                        :parts="solutionParts(q)"
                      />
                    </div>
                  </div>

                  <div v-if="getParsedQuestion(q).options.length > 0" class="q-options-wrap">
                    <QuestionOptions
                      :options="getParsedQuestion(q).options"
                      :highlight-labels="isExpanded(q.id) ? getCorrectLabels(q) : []"
                    />
                  </div>
                </div>

                <transition name="apple-expand">
                  <div v-if="isExpanded(q.id)" class="apple-answer-panel" @click.stop>
                    <div class="ans-section-item">
                      <div class="ans-tag-label">【参考答案】</div>
                      <div class="ans-value-box">
                        <template v-if="q.question_type === 'solution' && solutionParts(q).length">
                          <QuestionStructureView section="answers" :parts="solutionParts(q)" />
                        </template>
                        <template v-else-if="getCorrectLabels(q).length">
                          <span class="ans-hero-pill">
                            <LatexRender
                              :text="`$\\mathrm{${getCorrectLabels(q).join('')}}$`"
                              :inline="true"
                            />
                          </span>
                        </template>
                        <template v-else>
                          <span class="ans-plain-text">
                            <LatexRender :text="formatAnswerText(q)" :inline="true" />
                          </span>
                        </template>
                      </div>
                    </div>

                    <div
                      v-if="q.question_type === 'solution' && solutionParts(q).length"
                      class="ans-section-item"
                    >
                      <div class="ans-tag-label">【试题解析】</div>
                      <div class="ans-analysis-content">
                        <QuestionStructureView section="analyses" :parts="solutionParts(q)" />
                      </div>
                    </div>
                    <div v-else-if="q.question_type !== 'solution'" class="ans-section-item">
                      <div class="ans-tag-label">【试题解析】</div>
                      <div class="ans-analysis-content">
                        <LatexRender :text="analysisText(q)" />
                      </div>
                    </div>

                    <div class="ans-footer-meta">
                      <span class="sub-meta-pill">难度：{{ diffLabel(q.difficulty) }}</span>
                      <span v-if="q.default_score" class="sub-meta-pill">分值：{{ q.default_score }} 分</span>
                      <span class="sub-meta-pill">题型：{{ typeLabel(q.question_type) }}</span>
                    </div>
                  </div>
                </transition>

                <div class="apple-card-footer" @click.stop>
                  <div class="apple-action-cluster">
                    <button type="button" class="apple-card-action-btn" @click="goDetail(q.id)">
                      <AppIcon name="file-text" :size="13" />
                      <span>详情</span>
                    </button>
                  </div>
                  <button
                    type="button"
                    class="apple-basket-cta is-remove"
                    @click="removeOne(q.id)"
                  >
                    <AppIcon name="x" :size="14" />
                    <span>移出试题篮</span>
                  </button>
                </div>
              </article>
            </TransitionGroup>
          </section>
        </div>
      </main>

      <aside class="apple-sidebar-column">
        <div class="apple-sidebar-widget">
          <div class="bk-metric-row">
            <div class="bk-metric">
              <div class="bk-metric-val">{{ items.length }}</div>
              <div class="bk-metric-label">题目</div>
            </div>
            <div class="bk-metric">
              <div class="bk-metric-val">{{ diffSummary }}</div>
              <div class="bk-metric-label">难度</div>
            </div>
            <div class="bk-metric">
              <div class="bk-metric-val">{{ totalScore }}</div>
              <div class="bk-metric-label">分值</div>
            </div>
          </div>

          <div class="apple-switch-row">
            <label class="apple-switch-wrap">
              <input
                type="checkbox"
                class="apple-switch-input"
                :checked="allExpanded"
                @change="toggleAllAnswers"
              />
              <span class="apple-switch-track"></span>
              <span class="apple-switch-label">显示全部答案与解析</span>
            </label>
          </div>

          <div class="apple-tools-grid bk-tools-grid">
            <button type="button" class="apple-tool-tile" @click="savePaper">
              <div class="tool-icon-squircle">
                <AppIcon name="save" :size="18" />
              </div>
              <span class="tool-title">保存组卷</span>
            </button>
            <button type="button" class="apple-tool-tile" @click="downloadPaper()">
              <div class="tool-icon-squircle">
                <AppIcon name="download" :size="18" />
              </div>
              <span class="tool-title">试卷下载</span>
            </button>
            <button type="button" class="apple-tool-tile" @click="shareBasket">
              <div class="tool-icon-squircle">
                <AppIcon name="share" :size="18" />
              </div>
              <span class="tool-title">分享试卷</span>
            </button>
            <button type="button" class="apple-tool-tile" @click="showAnalysis">
              <div class="tool-icon-squircle">
                <AppIcon name="chart" :size="18" />
              </div>
              <span class="tool-title">试卷分析</span>
            </button>
          </div>

          <button type="button" class="bk-download-cta" @click="downloadPaper()">
            <AppIcon name="download" :size="15" />
            <span>下载试卷</span>
          </button>
        </div>

        <div class="apple-sidebar-widget">
          <div class="apple-stats-header">
            <div class="stats-title-group">
              <span class="apple-dot-indicator"></span>
              <h3 class="stats-heading">排列方式</h3>
            </div>
          </div>
          <div class="bk-sort-seg">
            <button
              type="button"
              class="bk-sort-btn"
              :class="{ 'is-active': sortMode === 'type' }"
              @click="sortMode = 'type'"
            >
              按题型
            </button>
            <button
              type="button"
              class="bk-sort-btn"
              :class="{ 'is-active': sortMode === 'added' }"
              @click="sortMode = 'added'"
            >
              按加入顺序
            </button>
          </div>
          <div
            v-if="sortMode === 'type'"
            class="bk-radio-group"
            role="radiogroup"
            aria-label="题型内排序"
          >
            <label class="bk-radio">
              <input v-model="typeSubSort" type="radio" name="basket-type-sub" value="added" />
              <span class="bk-radio-control" aria-hidden="true"></span>
              <span class="bk-radio-label">加入顺序</span>
            </label>
            <label class="bk-radio">
              <input v-model="typeSubSort" type="radio" name="basket-type-sub" value="easy" />
              <span class="bk-radio-control" aria-hidden="true"></span>
              <span class="bk-radio-label">由易到难</span>
            </label>
          </div>
          <div
            v-else
            class="bk-radio-group"
            role="radiogroup"
            aria-label="加入顺序方向"
          >
            <label class="bk-radio">
              <input v-model="addedDir" type="radio" name="basket-added-dir" value="asc" />
              <span class="bk-radio-control" aria-hidden="true"></span>
              <span class="bk-radio-label">正序</span>
            </label>
            <label class="bk-radio">
              <input v-model="addedDir" type="radio" name="basket-added-dir" value="desc" />
              <span class="bk-radio-control" aria-hidden="true"></span>
              <span class="bk-radio-label">倒序</span>
            </label>
          </div>
        </div>

        <div v-if="groupedSections.length" class="apple-sidebar-widget">
          <div class="apple-stats-header">
            <div class="stats-title-group">
              <span class="apple-dot-indicator"></span>
              <h3 class="stats-heading">题目导航</h3>
            </div>
          </div>
          <div v-for="sec in groupedSections" :key="'nav-' + sec.key" class="bk-nav-block">
            <div class="bk-nav-label">{{ sec.title }}</div>
            <div class="bk-nav-grid">
              <button
                v-for="q in sec.questions"
                :key="q.id"
                type="button"
                class="bk-nav-cell"
                :class="{ 'is-active': selectedId === q.id }"
                @click="scrollToQuestion(q.id)"
              >
                {{ displayNoMap[q.id] }}
              </button>
            </div>
          </div>
        </div>

        <button
          v-if="items.length"
          type="button"
          class="bk-clear-btn"
          @click="clearAll"
        >
          清空试题
        </button>
      </aside>
    </div>
  </div>
</template>

<style src="@/styles/apple-paper.css"></style>
<style scoped>
.apple-q-card.is-selected {
  border-color: rgba(0, 113, 227, 0.55);
  box-shadow: 0 0 0 1px rgba(0, 113, 227, 0.28), 0 8px 24px -6px rgba(0, 113, 227, 0.18);
}

a.apple-basket-cta {
  text-decoration: none;
}

.apple-basket-cta.is-remove {
  background: #ffffff;
  color: #ff3b30;
  border: 1px solid rgba(255, 59, 48, 0.35);
  box-shadow: none;
}

.apple-basket-cta.is-remove:hover {
  background: rgba(255, 59, 48, 0.08);
  box-shadow: none;
  transform: none;
}

[data-theme='dark'] .apple-basket-cta.is-remove {
  background: transparent;
  color: #ff6961;
}

.bk-metric-row {
  display: flex;
  align-items: stretch;
  margin-bottom: 14px;
  padding-bottom: 14px;
  border-bottom: 1px solid rgba(0, 0, 0, 0.05);
}

[data-theme='dark'] .bk-metric-row {
  border-bottom-color: rgba(255, 255, 255, 0.08);
}

.bk-metric {
  flex: 1;
  text-align: center;
}

.bk-metric + .bk-metric {
  border-left: 1px solid rgba(0, 0, 0, 0.05);
}

[data-theme='dark'] .bk-metric + .bk-metric {
  border-left-color: rgba(255, 255, 255, 0.08);
}

.bk-metric-val {
  font-size: 18px;
  font-weight: 700;
  color: #1d1d1f;
  letter-spacing: -0.02em;
  line-height: 1.2;
}

[data-theme='dark'] .bk-metric-val {
  color: #f5f5f7;
}

.bk-metric-label {
  margin-top: 4px;
  font-size: 11px;
  color: #86868b;
  font-weight: 500;
}

.bk-tools-grid {
  grid-template-columns: repeat(2, 1fr);
  margin-bottom: 14px;
}

.bk-download-cta {
  width: 100%;
  height: 40px;
  border: none;
  border-radius: 12px;
  background: #0071e3;
  color: #ffffff;
  font-size: 14px;
  font-weight: 600;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  cursor: pointer;
  box-shadow: 0 2px 8px rgba(0, 113, 227, 0.28);
}

.bk-download-cta:hover {
  background: #0077ed;
}

.bk-sort-seg {
  display: flex;
  padding: 3px;
  border-radius: 10px;
  background: rgba(0, 0, 0, 0.04);
}

.bk-sort-seg + .bk-radio-group {
  margin-top: 12px;
}

[data-theme='dark'] .bk-sort-seg {
  background: rgba(255, 255, 255, 0.06);
}

.bk-radio-group {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px 16px;
}

.bk-radio {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  user-select: none;
}

.bk-radio input {
  position: absolute;
  opacity: 0;
  width: 0;
  height: 0;
}

.bk-radio-control {
  width: 20px;
  height: 20px;
  flex-shrink: 0;
  border-radius: 50%;
  border: 2px solid #d4d4d8;
  background: #ffffff;
  position: relative;
  transition: border-color 0.18s ease, box-shadow 0.18s ease;
}

.bk-radio-control::after {
  content: '';
  position: absolute;
  inset: 3px;
  border-radius: 50%;
  background: #0071e3;
  transform: scale(0);
  transition: transform 0.18s cubic-bezier(0.16, 1, 0.3, 1);
}

.bk-radio:hover .bk-radio-control {
  border-color: #0071e3;
}

.bk-radio input:checked + .bk-radio-control {
  border-color: #0071e3;
}

.bk-radio input:checked + .bk-radio-control::after {
  transform: scale(1);
}

.bk-radio input:focus-visible + .bk-radio-control {
  box-shadow: 0 0 0 3px rgba(0, 113, 227, 0.22);
}

.bk-radio-label {
  font-size: 13px;
  font-weight: 500;
  color: #1d1d1f;
  line-height: 1.2;
}

[data-theme='dark'] .bk-radio-control {
  background: #2c2c2e;
  border-color: #3a3a3c;
}

[data-theme='dark'] .bk-radio-label {
  color: #f5f5f7;
}

.bk-sort-btn {
  flex: 1;
  height: 30px;
  border: none;
  background: transparent;
  border-radius: 8px;
  font-size: 12.5px;
  font-weight: 500;
  color: #6e6e73;
  cursor: pointer;
}

.bk-sort-btn.is-active {
  background: #ffffff;
  color: #1d1d1f;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.08);
}

[data-theme='dark'] .bk-sort-btn.is-active {
  background: #2c2c2e;
  color: #f5f5f7;
}

.bk-nav-block + .bk-nav-block {
  margin-top: 12px;
}

.bk-nav-label {
  font-size: 12px;
  font-weight: 600;
  color: #86868b;
  margin-bottom: 8px;
}

.bk-nav-grid {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.bk-nav-cell {
  width: 28px;
  height: 28px;
  padding: 0;
  border-radius: 6px;
  border: 1px solid rgba(0, 0, 0, 0.1);
  background: #ffffff;
  color: #1d1d1f;
  font-size: 12px;
  font-weight: 650;
  cursor: pointer;
}

[data-theme='dark'] .bk-nav-cell {
  background: #2c2c2e;
  border-color: rgba(255, 255, 255, 0.1);
  color: #f5f5f7;
}

.bk-nav-cell:hover,
.bk-nav-cell.is-active {
  background: #0071e3;
  border-color: #0071e3;
  color: #ffffff;
}

.bk-clear-btn {
  width: 100%;
  height: 40px;
  border-radius: 12px;
  border: 1px solid rgba(255, 59, 48, 0.28);
  background: #ffffff;
  color: #ff3b30;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
}

.bk-clear-btn:hover {
  background: rgba(255, 59, 48, 0.06);
}

[data-theme='dark'] .bk-clear-btn {
  background: #1c1c1e;
}

.bk-sidebar-empty,
.bk-empty-hint {
  margin: 0;
  font-size: 13px;
  color: #86868b;
}

.bk-empty {
  background: #ffffff;
  border: 1px solid rgba(0, 0, 0, 0.06);
  border-radius: 16px;
  padding: 64px 24px;
}

[data-theme='dark'] .bk-empty {
  background: #1c1c1e;
  border-color: rgba(255, 255, 255, 0.08);
}

.bk-empty p:first-child {
  font-size: 16px;
  font-weight: 600;
  color: #1d1d1f;
}

[data-theme='dark'] .bk-empty p:first-child {
  color: #f5f5f7;
}

.apple-card-list {
  position: relative;
}

.apple-card-list > .apple-q-card {
  transition: border-color 0.24s ease, box-shadow 0.24s ease;
}

.bk-card-move {
  transition: transform 0.34s cubic-bezier(0.22, 1, 0.36, 1) !important;
}

.bk-card-leave-active {
  position: absolute;
  left: 0;
  right: 0;
  z-index: 0;
  pointer-events: none;
  transition:
    opacity 0.28s cubic-bezier(0.22, 1, 0.36, 1),
    transform 0.28s cubic-bezier(0.22, 1, 0.36, 1);
}

.bk-card-leave-to {
  opacity: 0;
  transform: translateY(-10px) scale(0.98);
}

.bk-card-enter-active {
  transition:
    opacity 0.28s cubic-bezier(0.22, 1, 0.36, 1),
    transform 0.28s cubic-bezier(0.22, 1, 0.36, 1);
}

.bk-card-enter-from {
  opacity: 0;
  transform: translateY(12px);
}

@media (max-width: 900px) {
  .apple-q-card .apple-card-footer {
    opacity: 1;
    transform: none;
    pointer-events: auto;
    border-top-color: rgba(0, 0, 0, 0.05);
  }
}
</style>
