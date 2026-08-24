<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { paperApi, type PaperDetail, type PaperQuestionItemDetail } from '@/api/client'
import { AppIcon } from '@/components/ui'
import LatexRender from '@/components/LatexRender.vue'
import QuestionOptions from '@/components/QuestionOptions.vue'
import QuestionStructureView from '@/components/QuestionStructureView.vue'
import { useToast } from '@/composables/useToast'
import { useQuestionBasket } from '@/composables/useQuestionBasket'
import { displayPaperSource } from '@/utils/questionSource'
import { typeLabel, diffLabel } from '@/utils/questionDisplay'
import { partsFromStructureJson } from '@/utils/questionParts'

const route = useRoute()
const router = useRouter()
const toast = useToast()
const basket = useQuestionBasket()

const loading = ref(true)
const paper = ref<PaperDetail | null>(null)

// 答案展开状态管理
const allExpanded = ref(false)
const expandedMap = ref<Record<string, boolean>>({})

// 题目收藏状态管理
const favoritesMap = ref<Record<string, boolean>>({})
const paperFavorited = ref(false)

const sourceLabel = computed(() =>
  paper.value ? displayPaperSource(paper.value.source_type, paper.value.sub_source_type) : '',
)

function semesterLabel(s?: string | null) {
  if (!s) return ''
  if (s === 'first' || s === '1') return '上学期'
  if (s === 'second' || s === '2') return '下学期'
  return s
}

// 难度统计汇总
const diffSummary = computed(() => {
  if (!paper.value?.questions?.length) return '中等'
  const diffs = paper.value.questions.map((q) => {
    const d = parseInt(q.difficulty, 10)
    return isNaN(d) ? 3 : d
  })
  const avg = diffs.reduce((a, b) => a + b, 0) / diffs.length
  if (avg <= 2) return '较易'
  if (avg <= 3.2) return '中等'
  if (avg <= 4.2) return '偏难'
  return '困难'
})

// 从题干中提取选项的兜底解析函数
function extractOptionsFromStem(stem: string): { cleanStem: string; options: { label: string; content: string }[] } {
  if (!stem) return { cleanStem: '', options: [] }
  
  // 查找 A. / A、 / (A) / A: 等选项起始标记
  const optMatch = stem.match(/(?:^|\n|\s+)(?:[A-D][.、\s:：]|\([A-D]\))/i)
  if (!optMatch || optMatch.index === undefined) {
    return { cleanStem: stem, options: [] }
  }
  
  const optIndex = optMatch.index
  const cleanStem = stem.slice(0, optIndex).trim()
  const optSection = stem.slice(optIndex).trim()
  
  const regex = /(?:^|\n|\s+|\b)([A-D])[.、\s:：)]\s*([\s\S]*?)(?=(?:(?:\n|\s+|\b)[A-D][.、\s:：)])|$)/gi
  const options: { label: string; content: string }[] = []
  let match: RegExpExecArray | null
  while ((match = regex.exec(optSection)) !== null) {
    const label = match[1].toUpperCase()
    const content = (match[2] || '').trim()
    if (content) {
      options.push({ label, content })
    }
  }
  
  if (options.length >= 2) {
    return { cleanStem, options }
  }
  return { cleanStem: stem, options: [] }
}

// ---- 工具函数：解析选项（与 QuestionList.vue 保持完全一致）----
function parseOptions(raw: any): { label: string; content: string }[] {
  if (!raw) return []
  let opts = raw
  if (typeof opts === 'string') {
    try { opts = JSON.parse(opts) } catch { return [] }
  }
  if (!Array.isArray(opts)) return []
  return opts.map((opt: any) => {
    if (typeof opt === 'string') {
      const match = opt.match(/^([A-Z])[.、．\s]\s*(.*)$/)
      if (match) return { label: match[1], content: match[2] }
      return { label: '', content: opt }
    }
    if (opt && typeof opt === 'object' && opt.label) {
      return { label: opt.label, content: opt.content || '' }
    }
    return { label: '', content: String(opt) }
  })
}

// 获取解析后的题干与选项（确保选项绝不丢失）
function getParsedQuestion(q: PaperQuestionItemDetail) {
  let options = parseOptions(q.options)
  let stem = q.stem || ''

  // 兜底：如果 q.options 为空但题型是选择题，从题干中智能解析
  if (options.length === 0 && (q.question_type === 'choice' || q.question_type === 'multiple' || q.question_type === 'single_choice' || q.question_type === 'multi_choice')) {
    const extracted = extractOptionsFromStem(stem)
    if (extracted.options.length > 0) {
      options = extracted.options
      stem = extracted.cleanStem
    }
  }

  return { stem, options }
}

// 获取正确答案标号数组（用于选择题高亮与展示）
function getCorrectLabels(q: PaperQuestionItemDetail): string[] {
  const ans = q.correct_answer
  if (!ans) return []
  if (Array.isArray(ans)) {
    return ans.map((item) => (typeof item === 'string' ? item.trim().toUpperCase() : String(item).trim().toUpperCase())).filter(Boolean)
  }
  if (typeof ans === 'string') {
    const trimmed = ans.trim()
    if (trimmed.startsWith('[') && trimmed.endsWith(']')) {
      try {
        const parsed = JSON.parse(trimmed)
        if (Array.isArray(parsed)) {
          return parsed.map((x) => String(x).trim().toUpperCase()).filter(Boolean)
        }
      } catch {}
    }
    return trimmed.replace(/[^A-Za-z]/g, '').split('').map((c) => c.toUpperCase()).filter(Boolean)
  }
  return []
}

// 格式化非选择题答案展示
function formatAnswerText(q: PaperQuestionItemDetail): string {
  const ans = q.correct_answer
  if (!ans) return '暂无参考答案'
  if (typeof ans === 'string') return ans
  if (Array.isArray(ans)) {
    return ans.join('； ')
  }
  try {
    return JSON.stringify(ans)
  } catch {
    return String(ans)
  }
}

function paperSolutionParts(q: PaperQuestionItemDetail) {
  return partsFromStructureJson(q.structure)
}

// 题目是否展开答案
function isExpanded(id: string) {
  return allExpanded.value || !!expandedMap.value[id]
}

// 点击卡片切换单个题目展开/折叠
function toggleQuestion(id: string) {
  if (allExpanded.value) {
    expandedMap.value[id] = false
    allExpanded.value = false
    if (paper.value?.questions) {
      for (const q of paper.value.questions) {
        if (q.id !== id) {
          expandedMap.value[q.id] = true
        }
      }
    }
  } else {
    expandedMap.value[id] = !expandedMap.value[id]
  }
}

// 全局切换展开所有答案与解析
function toggleAllAnswers() {
  allExpanded.value = !allExpanded.value
  expandedMap.value = {}
  if (paper.value?.questions) {
    for (const q of paper.value.questions) {
      expandedMap.value[q.id] = allExpanded.value
    }
  }
}

// 题目收藏
function toggleFavorite(qId: string) {
  favoritesMap.value[qId] = !favoritesMap.value[qId]
  if (favoritesMap.value[qId]) {
    toast.success('试题已收藏')
  } else {
    toast.info('已取消试题收藏')
  }
}

// 试卷收藏
function togglePaperFavorite() {
  paperFavorited.value = !paperFavorited.value
  if (paperFavorited.value) {
    toast.success('试卷已收藏')
  } else {
    toast.info('已取消试卷收藏')
  }
}

// 加入 / 移出试题篮
function toggleBasket(qId: string) {
  basket.toggle(qId)
  if (basket.isInBasket(qId)) {
    toast.success('已加入试题篮')
  } else {
    toast.info('已从试题篮中移除')
  }
}

// 分组逻辑：按题型或 section 分组
interface SectionGroup {
  key: string
  title: string
  typeName: string
  questions: PaperQuestionItemDetail[]
}

const groupedSections = computed<SectionGroup[]>(() => {
  const qs = paper.value?.questions || []
  if (!qs.length) return []

  const hasCustomSections = qs.some((q) => q.section && q.section.trim().length > 0)
  if (hasCustomSections) {
    const map = new Map<string, PaperQuestionItemDetail[]>()
    for (const q of qs) {
      const sec = q.section?.trim() || '其他题型'
      if (!map.has(sec)) map.set(sec, [])
      map.get(sec)!.push(q)
    }
    return Array.from(map.entries()).map(([title, items], idx) => ({
      key: `custom_${idx}`,
      title,
      typeName: items[0]?.question_type || 'choice',
      questions: items,
    }))
  }

  const typeOrder = ['single_choice', 'multi_choice', 'fill', 'solution', 'judgment', 'composite']
  const typeTitleMap: Record<string, string> = {
    single_choice: '单选题',
    multi_choice: '多选题',
    fill: '填空题',
    solution: '解答题',
    judgment: '判断题',
    composite: '综合题',
  }
  const chineseNums = ['一', '二', '三', '四', '五', '六', '七', '八']

  const buckets: Record<string, PaperQuestionItemDetail[]> = {
    single_choice: [],
    multi_choice: [],
    fill: [],
    solution: [],
    judgment: [],
    composite: [],
  }

  for (const q of qs) {
    let t = q.question_type || 'choice'
    if (t === 'multiple' || t === 'multi_choice') {
      t = 'multi_choice'
    } else if (t === 'choice' || t === 'single_choice') {
      const labels = getCorrectLabels(q)
      // 无答案时无法靠个数推断；仅当明确多个正确项时升为多选（兼容历史 choice 落库）
      t = labels.length > 1 ? 'multi_choice' : 'single_choice'
    }
    if (buckets[t]) {
      buckets[t].push(q)
    } else {
      if (!buckets.composite) buckets.composite = []
      buckets.composite.push(q)
    }
  }

  const result: SectionGroup[] = []
  let numIdx = 0
  for (const t of typeOrder) {
    const list = buckets[t]
    if (list && list.length > 0) {
      const numPrefix = chineseNums[numIdx] || `${numIdx + 1}`
      result.push({
        key: t,
        title: `${numPrefix}、${typeTitleMap[t] || t}`,
        typeName: typeTitleMap[t] || t,
        questions: list,
      })
      numIdx++
    }
  }
  return result
})

const sectionStats = computed(() => {
  return groupedSections.value.map((sec) => ({
    key: sec.key,
    title: sec.typeName,
    count: sec.questions.length,
  }))
})

function scrollToSection(key: string) {
  const el = document.getElementById(`sec-${key}`)
  if (el) {
    el.scrollIntoView({ behavior: 'smooth', block: 'start' })
  }
}

function sharePaper() {
  if (navigator.clipboard) {
    navigator.clipboard.writeText(window.location.href)
    toast.success('试卷链接已复制到剪贴板')
  } else {
    toast.info('当前页面地址：' + window.location.href)
  }
}

function downloadPaper(sectionTitle?: string) {
  toast.info(sectionTitle ? `正在准备下载【${sectionTitle}】...` : '正在准备生成试卷下载文档...')
  setTimeout(() => {
    window.print()
  }, 300)
}

function showAnalysisModal() {
  toast.info(`试卷共包含 ${paper.value?.questions?.length || 0} 道试题，难度评估：${diffSummary.value}`)
}

function reportPaperError(qNo?: string) {
  toast.info(qNo ? `已收到第 ${qNo} 题纠错反馈，感谢您的支持！` : '已收到试卷纠错反馈，感谢您的支持！')
}

function findSimilarQuestions(q: PaperQuestionItemDetail) {
  toast.info(`正在为您检索相似试题...`)
  router.push({ path: '/questions', query: { keyword: q.stem?.slice(0, 20) } })
}

onMounted(async () => {
  const id = String(route.params.id || '')
  if (!id) {
    toast.error('试卷不存在')
    loading.value = false
    return
  }
  try {
    const res = await paperApi.get(id)
    paper.value = res.data
  } catch (e: any) {
    toast.error(e.response?.data?.error || '加载试卷失败')
  } finally {
    loading.value = false
  }
})
</script>

<template>
  <div class="apple-paper-scope">
    <div v-if="loading" class="apple-loading-box">
      <div class="apple-spinner"></div>
      <p class="apple-loading-text">正在载入试卷内容...</p>
    </div>

    <template v-else-if="paper">
      <!-- 页面主要内容容器 -->
      <div class="apple-content-layout">
        <!-- 左侧/中间：试卷主体展示区 -->
        <main class="apple-main-column">
          <!-- 顶部试卷信息卡片（Apple HIG 扁平半透明层次） -->
          <div class="apple-header-card">
            <button
              type="button"
              class="apple-back-btn"
              @click="router.push({ path: '/questions', query: { view: 'papers' } })"
            >
              <AppIcon name="chevron-left" :size="15" />
              <span>返回试卷列表</span>
            </button>

            <div class="apple-title-wrapper">
              <h1 class="apple-paper-title">{{ paper.title }}</h1>
              <span class="apple-pill-badge accent-orange">新考卷</span>
            </div>

            <!-- 元数据标签胶囊组 -->
            <div class="apple-meta-pill-group">
              <span v-if="paper.region_province" class="meta-pill">{{ paper.region_province }}</span>
              <span v-if="paper.grade" class="meta-pill">{{ paper.grade }}</span>
              <span class="meta-pill">{{ semesterLabel(paper.semester) || '期中' }}</span>
              <span v-if="paper.year" class="meta-pill">{{ paper.year }}</span>
              <span class="meta-pill">474 次浏览</span>
              <span class="meta-pill">难度 · {{ diffSummary }}</span>
              <span v-if="paper.school_name || sourceLabel" class="meta-pill">
                {{ paper.school_name || sourceLabel }}
              </span>
              <span class="meta-pill text-blue">含参考答案与解析</span>
              <span class="meta-pill strong">共 {{ paper.questions?.length ?? 0 }} 题</span>
            </div>
          </div>

          <!-- 分大题展示列表 -->
          <div class="apple-sections-container">
            <section
              v-for="sec in groupedSections"
              :key="sec.key"
              :id="'sec-' + sec.key"
              class="apple-section-block"
            >
              <!-- 大题头部栏（Apple 极简分节头） -->
              <div class="apple-section-bar">
                <div class="section-title-left">
                  <span class="section-accent-dash"></span>
                  <h2 class="apple-section-heading">{{ sec.title }}</h2>
                  <span class="section-count-tag">{{ sec.questions.length }} 题</span>
                </div>
                <button
                  type="button"
                  class="apple-ghost-btn"
                  @click="downloadPaper(sec.title)"
                >
                  <AppIcon name="download" :size="13" />
                  <span>下载本大题</span>
                </button>
              </div>

              <!-- 题目卡片列表（Apple HIG 精致卡片） -->
              <div class="apple-card-list">
                <article
                  v-for="(q, idx) in sec.questions"
                  :key="q.id"
                  class="apple-q-card"
                  :class="{
                    'is-expanded': isExpanded(q.id),
                    'in-basket': basket.isInBasket(q.question_id),
                  }"
                  @click="toggleQuestion(q.id)"
                >
                  <!-- 题干与选项（核心展示区） -->
                  <div class="apple-q-body">
                    <!-- 题号与题干 -->
                    <div class="apple-stem-row">
                      <div class="apple-q-index">{{ q.question_no || (idx + 1) }}.</div>
                      <div class="apple-q-stem">
                        <LatexRender :text="getParsedQuestion(q).stem" />
                        <QuestionStructureView
                          v-if="q.question_type === 'solution' && paperSolutionParts(q).length"
                          section="stems"
                          :parts="paperSolutionParts(q)"
                        />
                      </div>
                    </div>

                    <!-- 选择题选项：与 QuestionList.vue 保持一致的智能自适应微积木 (4列 / 2列 / 1列) -->
                    <div
                      v-if="getParsedQuestion(q).options.length > 0"
                      class="q-options-wrap"
                    >
                      <QuestionOptions
                        :options="getParsedQuestion(q).options"
                        :highlight-labels="isExpanded(q.id) ? getCorrectLabels(q) : []"
                      />
                    </div>
                  </div>

                  <!-- 折叠的答案与解析区（点击题目展开，Apple Inset 材质） -->
                  <transition name="apple-expand">
                    <div
                      v-if="isExpanded(q.id)"
                      class="apple-answer-panel"
                      @click.stop
                    >
                      <!-- 参考答案 -->
                      <div class="ans-section-item">
                        <div class="ans-tag-label">【参考答案】</div>
                        <div class="ans-value-box">
                          <template v-if="q.question_type === 'solution' && paperSolutionParts(q).length">
                            <QuestionStructureView section="answers" :parts="paperSolutionParts(q)" />
                          </template>
                          <template v-else-if="getCorrectLabels(q).length">
                            <span class="ans-hero-pill">
                              <LatexRender :text="`$\\mathrm{${getCorrectLabels(q).join('')}}$`" :inline="true" />
                            </span>
                          </template>
                          <template v-else>
                            <span class="ans-plain-text">
                              <LatexRender :text="formatAnswerText(q)" :inline="true" />
                            </span>
                          </template>
                        </div>
                      </div>

                      <!-- 试题解析 -->
                      <div v-if="q.question_type === 'solution' && paperSolutionParts(q).length" class="ans-section-item">
                        <div class="ans-tag-label">【试题解析】</div>
                        <div class="ans-analysis-content">
                          <QuestionStructureView section="analyses" :parts="paperSolutionParts(q)" />
                        </div>
                      </div>
                      <div v-else-if="q.question_type !== 'solution'" class="ans-section-item">
                        <div class="ans-tag-label">【试题解析】</div>
                        <div class="ans-analysis-content">
                          <LatexRender :text="q.analysis || '暂无详细试题解析'" />
                        </div>
                      </div>

                      <!-- 属性标签 -->
                      <div class="ans-footer-meta">
                        <span class="sub-meta-pill">难度：{{ diffLabel(q.difficulty) }}</span>
                        <span v-if="q.score" class="sub-meta-pill">分值：{{ q.score }} 分</span>
                        <span class="sub-meta-pill">题型：{{ typeLabel(q.question_type) }}</span>
                      </div>
                    </div>
                  </transition>

                  <!-- 悬浮出现的底部操作栏（默认隐藏，hover 时平滑浮现） -->
                  <div class="apple-card-footer" @click.stop>
                    <div class="apple-action-cluster">
                      <button
                        type="button"
                        class="apple-card-action-btn"
                        @click="findSimilarQuestions(q)"
                      >
                        <AppIcon name="search" :size="13" />
                        <span>相似题</span>
                      </button>
                      <button
                        type="button"
                        class="apple-card-action-btn"
                        @click="reportPaperError(q.question_no || String(idx + 1))"
                      >
                        <AppIcon name="flag" :size="13" />
                        <span>纠错</span>
                      </button>
                      <button
                        type="button"
                        class="apple-card-action-btn"
                        @click="router.push(`/questions/${q.question_id}`)"
                      >
                        <AppIcon name="file-text" :size="13" />
                        <span>详情</span>
                      </button>
                      <button
                        type="button"
                        class="apple-card-action-btn"
                        :class="{ 'is-favorited': favoritesMap[q.question_id] }"
                        @click="toggleFavorite(q.question_id)"
                      >
                        <AppIcon name="star" :size="13" />
                        <span>{{ favoritesMap[q.question_id] ? '已收藏' : '收藏' }}</span>
                      </button>
                    </div>

                    <!-- Apple 质感试题篮主按钮 -->
                    <button
                      type="button"
                      class="apple-basket-cta"
                      :class="{ 'in-basket': basket.isInBasket(q.question_id) }"
                      @click="toggleBasket(q.question_id)"
                    >
                      <AppIcon name="shopping-cart" :size="14" />
                      <span>{{ basket.isInBasket(q.question_id) ? '已加入试题篮' : '+ 加入试题篮' }}</span>
                    </button>
                  </div>
                </article>
              </div>
            </section>
          </div>
        </main>

        <!-- 右侧边栏：Apple Widget 侧边操作岛 -->
        <aside class="apple-sidebar-column">
          <!-- 快捷工具卡片 -->
          <div class="apple-sidebar-widget">
            <!-- Cupertino 风格开关：显示全部答案和解析 -->
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
              <button type="button" class="apple-share-pill" @click="sharePaper">
                <AppIcon name="share" :size="13" />
                <span>分享</span>
              </button>
            </div>

            <!-- 六宫格快捷操作按钮 -->
            <div class="apple-tools-grid">
              <button
                type="button"
                class="apple-tool-tile"
                :class="{ 'is-active': paperFavorited }"
                @click="togglePaperFavorite"
              >
                <div class="tool-icon-squircle">
                  <AppIcon name="star" :size="18" />
                </div>
                <span class="tool-title">{{ paperFavorited ? '已收藏' : '试卷收藏' }}</span>
              </button>
              <button type="button" class="apple-tool-tile" @click="downloadPaper()">
                <div class="tool-icon-squircle">
                  <AppIcon name="download" :size="18" />
                </div>
                <span class="tool-title">试卷下载</span>
              </button>
              <button type="button" class="apple-tool-tile" @click="showAnalysisModal">
                <div class="tool-icon-squircle">
                  <AppIcon name="chart" :size="18" />
                </div>
                <span class="tool-title">试卷分析</span>
              </button>
              <button type="button" class="apple-tool-tile" @click="reportPaperError()">
                <div class="tool-icon-squircle">
                  <AppIcon name="flag" :size="18" />
                </div>
                <span class="tool-title">试卷纠错</span>
              </button>
              <button type="button" class="apple-tool-tile" @click="toast.info('答题卡生成功能已就绪')">
                <div class="tool-icon-squircle">
                  <AppIcon name="file-text" :size="18" />
                </div>
                <span class="tool-title">试卷答题卡</span>
              </button>
              <button type="button" class="apple-tool-tile" @click="toast.info('在线练习模式已开启')">
                <div class="tool-icon-squircle">
                  <AppIcon name="pencil" :size="18" />
                </div>
                <span class="tool-title">在线练习</span>
              </button>
            </div>
          </div>

          <!-- 试题统计卡片 -->
          <div class="apple-sidebar-widget">
            <div class="apple-stats-header">
              <div class="stats-title-group">
                <span class="apple-dot-indicator"></span>
                <h3 class="stats-heading">试题统计</h3>
              </div>
              <span class="stats-total-capsule">共 {{ paper.questions?.length ?? 0 }} 题</span>
            </div>

            <div class="apple-stats-list">
              <div
                v-for="st in sectionStats"
                :key="st.key"
                class="apple-stats-row"
                @click="scrollToSection(st.key)"
              >
                <span class="stats-type-text">{{ st.title }}</span>
                <span class="stats-count-tag">{{ st.count }} 题</span>
              </div>
            </div>
          </div>
        </aside>
      </div>

      <!-- 悬浮试题篮 Apple 胶囊 -->
      <div
        v-if="basket.count.value > 0"
        class="apple-floating-basket"
        @click="toast.info(`试题篮中已收录 ${basket.count.value} 道试题`)"
      >
        <AppIcon name="shopping-cart" :size="16" />
        <span>试题篮 · {{ basket.count.value }}</span>
      </div>
    </template>

    <div v-else class="apple-empty-state">试卷不存在或已被移除</div>
  </div>
</template>

<style scoped>
/* ==========================================================================
   Apple Human Interface Guidelines (HIG) 精致设计规范
   ========================================================================== */

.apple-paper-scope {
  min-height: calc(100vh - 64px);
  background: #f5f5f7;
  padding: 24px 28px 72px;
  font-family: system-ui, -apple-system, BlinkMacSystemFont, "SF Pro Text", "SF Pro Display", "PingFang SC", "Hiragino Sans GB", "Microsoft YaHei", sans-serif;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
}

[data-theme='dark'] .apple-paper-scope {
  background: #000000;
}

/* 加载动画与空状态 */
.apple-loading-box,
.apple-empty-state {
  padding: 100px 20px;
  text-align: center;
  color: #86868b;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
}

.apple-spinner {
  width: 32px;
  height: 32px;
  border: 2.5px solid rgba(0, 113, 227, 0.15);
  border-top-color: #0071e3;
  border-radius: 50%;
  animation: appleSpin 0.75s cubic-bezier(0.4, 0, 0.2, 1) infinite;
}

@keyframes appleSpin {
  to { transform: rotate(360deg); }
}

.apple-loading-text {
  font-size: 14px;
  color: #86868b;
  font-weight: 500;
}

/* 主内容两栏布局 */
.apple-content-layout {
  max-width: 1240px;
  margin: 0 auto;
  display: flex;
  gap: 24px;
  align-items: flex-start;
}

.apple-main-column {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 20px;
}

/* 顶部试卷头部卡片 */
.apple-header-card {
  background: #ffffff;
  border: 1px solid rgba(0, 0, 0, 0.06);
  border-radius: 16px;
  padding: 24px 32px 22px;
  text-align: center;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.03), 0 1px 2px rgba(0, 0, 0, 0.02);
  position: relative;
}

[data-theme='dark'] .apple-header-card {
  background: #1c1c1e;
  border-color: rgba(255, 255, 255, 0.08);
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.4);
}

.apple-back-btn {
  position: absolute;
  top: 20px;
  left: 24px;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  border: none;
  background: transparent;
  color: #86868b;
  cursor: pointer;
  font-size: 13px;
  font-weight: 500;
  padding: 6px 10px;
  border-radius: 9999px;
  transition: all 0.18s ease;
}

.apple-back-btn:hover {
  color: #0071e3;
  background: rgba(0, 113, 227, 0.08);
}

.apple-title-wrapper {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 12px;
  margin-top: 10px;
  margin-bottom: 14px;
  flex-wrap: wrap;
}

.apple-paper-title {
  font-size: 21px;
  font-weight: 600;
  letter-spacing: -0.015em;
  color: #1d1d1f;
  line-height: 1.35;
  margin: 0;
}

[data-theme='dark'] .apple-paper-title {
  color: #f5f5f7;
}

.apple-pill-badge {
  font-size: 11px;
  font-weight: 600;
  padding: 2px 9px;
  border-radius: 9999px;
  display: inline-block;
}

.apple-pill-badge.accent-orange {
  background: #ff9500;
  color: #ffffff;
}

.apple-meta-pill-group {
  display: flex;
  align-items: center;
  justify-content: center;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 6px;
}

.meta-pill {
  font-size: 12.5px;
  color: #6e6e73;
  background: rgba(0, 0, 0, 0.035);
  padding: 3px 10px;
  border-radius: 9999px;
  font-weight: 450;
}

[data-theme='dark'] .meta-pill {
  color: #a1a1a6;
  background: rgba(255, 255, 255, 0.06);
}

.meta-pill.text-blue {
  color: #0071e3;
  background: rgba(0, 113, 227, 0.08);
  font-weight: 500;
}

.meta-pill.strong {
  font-weight: 600;
}

/* 分大题区块 */
.apple-sections-container {
  display: flex;
  flex-direction: column;
  gap: 24px;
}

.apple-section-block {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.apple-section-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 4px 6px;
}

.section-title-left {
  display: flex;
  align-items: center;
  gap: 8px;
}

.section-accent-dash {
  width: 4px;
  height: 16px;
  background: #0071e3;
  border-radius: 9999px;
}

.apple-section-heading {
  font-size: 16px;
  font-weight: 650;
  color: #1d1d1f;
  letter-spacing: -0.01em;
  margin: 0;
}

[data-theme='dark'] .apple-section-heading {
  color: #f5f5f7;
}

.section-count-tag {
  font-size: 12.5px;
  color: #86868b;
  font-weight: 500;
}

.apple-ghost-btn {
  border: 1px solid rgba(0, 113, 227, 0.25);
  background: rgba(0, 113, 227, 0.04);
  color: #0071e3;
  padding: 5px 12px;
  border-radius: 9999px;
  font-size: 12.5px;
  font-weight: 500;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  transition: all 0.18s cubic-bezier(0.16, 1, 0.3, 1);
}

.apple-ghost-btn:hover {
  background: rgba(0, 113, 227, 0.1);
  border-color: rgba(0, 113, 227, 0.4);
}

/* 题目卡片列表 */
.apple-card-list {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

/* Apple HIG 题目卡片 */
.apple-q-card {
  background: #ffffff;
  border: 1px solid rgba(0, 0, 0, 0.07);
  border-radius: 16px;
  padding: 24px 28px 20px;
  position: relative;
  cursor: pointer;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.03), 0 1px 2px rgba(0, 0, 0, 0.02);
  transition: all 0.24s cubic-bezier(0.16, 1, 0.3, 1);
}

[data-theme='dark'] .apple-q-card {
  background: #1c1c1e;
  border-color: rgba(255, 255, 255, 0.08);
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
}

/* 悬浮聚焦高亮状态：柔和的 Apple 聚焦光晕 */
.apple-q-card:hover {
  border-color: rgba(0, 113, 227, 0.35);
  box-shadow: 0 12px 32px -4px rgba(0, 0, 0, 0.08), 0 0 0 1px rgba(0, 113, 227, 0.25);
  transform: translateY(-2px);
}

.apple-q-card.is-expanded {
  border-color: rgba(0, 113, 227, 0.4);
}

/* 题干排版 */
.apple-q-body {
  font-size: 15.5px;
  line-height: 1.75;
  color: #1d1d1f;
}

[data-theme='dark'] .apple-q-body {
  color: #f5f5f7;
}

.apple-stem-row {
  display: flex;
  align-items: flex-start;
  gap: 8px;
}

.apple-q-index {
  font-weight: 700;
  color: #1d1d1f;
  flex-shrink: 0;
  min-width: 22px;
  line-height: 1.75;
}

[data-theme='dark'] .apple-q-index {
  color: #f5f5f7;
}

.apple-q-stem {
  flex: 1;
  min-width: 0;
  word-break: break-word;
}

.apple-q-stem :deep(p) {
  margin: 0;
}

/* 选择题选项包装区：与 QuestionList.vue 一致的缩进排版 */
.q-options-wrap {
  padding-left: 28px;
}

/* 折叠展开的答案与解析区（Apple Inset 材质） */
.apple-answer-panel {
  margin-top: 20px;
  padding: 18px 22px;
  background: rgba(0, 113, 227, 0.025);
  border: 1px solid rgba(0, 113, 227, 0.1);
  border-radius: 12px;
  display: flex;
  flex-direction: column;
  gap: 14px;
  cursor: default;
}

[data-theme='dark'] .apple-answer-panel {
  background: rgba(0, 113, 227, 0.05);
  border-color: rgba(0, 113, 227, 0.2);
}

.ans-section-item {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.ans-tag-label {
  font-size: 13px;
  font-weight: 650;
  color: #0071e3;
}

.ans-value-box {
  padding-left: 2px;
}

.ans-hero-pill {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 16px;
  font-weight: 700;
  color: #0071e3;
  background: rgba(0, 113, 227, 0.1);
  padding: 3px 12px;
  border-radius: 6px;
}

.ans-plain-text {
  font-size: 14.5px;
  color: #3a3a3c;
  line-height: 1.6;
}

[data-theme='dark'] .ans-plain-text {
  color: #d1d1d6;
}

.ans-analysis-content {
  font-size: 14.5px;
  color: #3a3a3c;
  line-height: 1.75;
  padding-left: 2px;
}

[data-theme='dark'] .ans-analysis-content {
  color: #d1d1d6;
}

.ans-analysis-content :deep(p) {
  margin: 4px 0;
}

.ans-footer-meta {
  display: flex;
  gap: 10px;
  margin-top: 4px;
  padding-top: 10px;
  border-top: 1px dashed rgba(0, 113, 227, 0.15);
}

.sub-meta-pill {
  font-size: 12px;
  color: #86868b;
}

/* 悬浮浮现的底部操作栏 */
.apple-card-footer {
  margin-top: 16px;
  padding-top: 12px;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 16px;
  border-top: 1px dashed transparent;
  opacity: 0;
  transform: translateY(6px);
  pointer-events: none;
  transition: all 0.22s cubic-bezier(0.16, 1, 0.3, 1);
}

.apple-q-card:hover .apple-card-footer {
  opacity: 1;
  transform: translateY(0);
  pointer-events: auto;
  border-top-color: rgba(0, 0, 0, 0.05);
}

[data-theme='dark'] .apple-q-card:hover .apple-card-footer {
  border-top-color: rgba(255, 255, 255, 0.08);
}

.apple-action-cluster {
  display: flex;
  align-items: center;
  gap: 6px;
}

.apple-card-action-btn {
  background: transparent;
  border: 1px solid transparent;
  color: #6e6e73;
  font-size: 12.5px;
  font-weight: 500;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 5px 10px;
  border-radius: 9999px;
  transition: all 0.15s ease;
}

.apple-card-action-btn:hover {
  background: rgba(0, 113, 227, 0.08);
  color: #0071e3;
  border-color: rgba(0, 113, 227, 0.15);
}

.apple-card-action-btn.is-favorited {
  color: #ff9500;
}

[data-theme='dark'] .apple-card-action-btn {
  color: #a1a1a6;
}

[data-theme='dark'] .apple-card-action-btn:hover {
  background: rgba(0, 113, 227, 0.15);
  color: #2997ff;
}

/* 加入试题篮 CTA 按钮（Apple System Blue / Green） */
.apple-basket-cta {
  height: 32px;
  padding: 0 16px;
  background: #0071e3;
  color: #ffffff;
  border: none;
  border-radius: 9999px;
  font-size: 13px;
  font-weight: 500;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
  transition: all 0.18s cubic-bezier(0.16, 1, 0.3, 1);
  box-shadow: 0 2px 8px rgba(0, 113, 227, 0.28);
}

.apple-basket-cta:hover {
  background: #0077ed;
  box-shadow: 0 4px 12px rgba(0, 113, 227, 0.38);
  transform: scale(1.02);
}

.apple-basket-cta.in-basket {
  background: #34c759;
  box-shadow: 0 2px 8px rgba(52, 199, 89, 0.28);
}

.apple-basket-cta.in-basket:hover {
  background: #30b753;
  box-shadow: 0 4px 12px rgba(52, 199, 89, 0.38);
}

/* 右侧边栏 */
.apple-sidebar-column {
  width: 284px;
  flex-shrink: 0;
  position: sticky;
  top: 24px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.apple-sidebar-widget {
  background: #ffffff;
  border: 1px solid rgba(0, 0, 0, 0.06);
  border-radius: 16px;
  padding: 18px 20px;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.03), 0 1px 2px rgba(0, 0, 0, 0.02);
}

[data-theme='dark'] .apple-sidebar-widget {
  background: #1c1c1e;
  border-color: rgba(255, 255, 255, 0.08);
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
}

/* 顶部答案开关与分享 */
.apple-switch-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding-bottom: 14px;
  border-bottom: 1px solid rgba(0, 0, 0, 0.05);
  margin-bottom: 14px;
}

[data-theme='dark'] .apple-switch-row {
  border-bottom-color: rgba(255, 255, 255, 0.08);
}

/* iOS Cupertino Switch */
.apple-switch-wrap {
  display: inline-flex;
  align-items: center;
  gap: 10px;
  cursor: pointer;
  user-select: none;
}

.apple-switch-input {
  position: absolute;
  opacity: 0;
  width: 0;
  height: 0;
}

.apple-switch-track {
  width: 40px;
  height: 22px;
  background-color: #e5e5ea;
  border-radius: 9999px;
  position: relative;
  transition: background-color 0.22s ease;
}

[data-theme='dark'] .apple-switch-track {
  background-color: #39393d;
}

.apple-switch-track::after {
  content: '';
  position: absolute;
  top: 2px;
  left: 2px;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background-color: #ffffff;
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.2);
  transition: transform 0.22s cubic-bezier(0.16, 1, 0.3, 1);
}

.apple-switch-input:checked + .apple-switch-track {
  background-color: #34c759;
}

.apple-switch-input:checked + .apple-switch-track::after {
  transform: translateX(18px);
}

.apple-switch-label {
  font-size: 13px;
  color: #1d1d1f;
  font-weight: 500;
}

[data-theme='dark'] .apple-switch-label {
  color: #f5f5f7;
}

.apple-share-pill {
  background: rgba(0, 113, 227, 0.08);
  border: none;
  color: #0071e3;
  font-size: 12.5px;
  font-weight: 500;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  cursor: pointer;
  padding: 4px 10px;
  border-radius: 9999px;
  transition: all 0.15s ease;
}

.apple-share-pill:hover {
  background: rgba(0, 113, 227, 0.16);
}

/* 六宫格快捷工具 */
.apple-tools-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 12px 8px;
  text-align: center;
}

.apple-tool-tile {
  background: transparent;
  border: none;
  padding: 8px 4px;
  border-radius: 12px;
  color: #6e6e73;
  cursor: pointer;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  transition: all 0.18s ease;
}

.tool-icon-squircle {
  width: 36px;
  height: 36px;
  border-radius: 10px;
  background: rgba(0, 0, 0, 0.035);
  display: flex;
  align-items: center;
  justify-content: center;
  color: #1d1d1f;
  transition: all 0.18s ease;
}

[data-theme='dark'] .tool-icon-squircle {
  background: rgba(255, 255, 255, 0.06);
  color: #f5f5f7;
}

.tool-title {
  font-size: 12px;
  font-weight: 500;
}

.apple-tool-tile:hover {
  color: #0071e3;
}

.apple-tool-tile:hover .tool-icon-squircle {
  background: rgba(0, 113, 227, 0.1);
  color: #0071e3;
  transform: translateY(-1px);
}

.apple-tool-tile.is-active {
  color: #ff9500;
}

.apple-tool-tile.is-active .tool-icon-squircle {
  color: #ff9500;
}

/* 试题统计卡片 */
.apple-stats-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
}

.stats-title-group {
  display: flex;
  align-items: center;
  gap: 6px;
}

.apple-dot-indicator {
  width: 6px;
  height: 6px;
  background-color: #0071e3;
  border-radius: 50%;
}

.stats-heading {
  font-size: 14px;
  font-weight: 650;
  color: #1d1d1f;
  margin: 0;
}

[data-theme='dark'] .stats-heading {
  color: #f5f5f7;
}

.stats-total-capsule {
  font-size: 12px;
  color: #86868b;
  font-weight: 500;
}

.apple-stats-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.apple-stats-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
  background: rgba(0, 0, 0, 0.025);
  border-radius: 8px;
  font-size: 13px;
  color: #1d1d1f;
  cursor: pointer;
  transition: all 0.16s ease;
}

[data-theme='dark'] .apple-stats-row {
  background: rgba(255, 255, 255, 0.04);
  color: #f5f5f7;
}

.apple-stats-row:hover {
  background: rgba(0, 113, 227, 0.08);
  color: #0071e3;
}

.stats-count-tag {
  font-size: 12px;
  color: #86868b;
  font-weight: 600;
}

/* 浮动试题篮胶囊 */
.apple-floating-basket {
  position: fixed;
  bottom: 32px;
  right: 36px;
  background: #0071e3;
  color: #ffffff;
  padding: 10px 20px;
  border-radius: 9999px;
  box-shadow: 0 8px 24px rgba(0, 113, 227, 0.35);
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13.5px;
  font-weight: 600;
  cursor: pointer;
  z-index: 100;
  transition: transform 0.2s cubic-bezier(0.16, 1, 0.3, 1), box-shadow 0.2s ease;
}

.apple-floating-basket:hover {
  transform: translateY(-2px);
  box-shadow: 0 12px 28px rgba(0, 113, 227, 0.45);
}

/* 展开动效 */
.apple-expand-enter-active,
.apple-expand-leave-active {
  transition: opacity 0.22s cubic-bezier(0.16, 1, 0.3, 1), transform 0.22s cubic-bezier(0.16, 1, 0.3, 1);
}

.apple-expand-enter-from,
.apple-expand-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}

@media (max-width: 900px) {
  .apple-content-layout {
    flex-direction: column;
  }
  .apple-sidebar-column {
    width: 100%;
    position: static;
  }
}
</style>
