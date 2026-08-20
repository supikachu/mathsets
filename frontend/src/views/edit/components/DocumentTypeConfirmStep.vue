<script setup lang="ts">
/**
 * V2.1.1 资料类型确认步骤（F1 步骤 3）
 *
 * AI 分类推荐卡 + 16 类单选 + 分类型元数据表单：
 * - exam / mock_exam → Paper 元数据（含"关联已有试卷"）
 * - 非试卷 → Collection 元数据（章节走知识树级联）
 * - mixed → 集合壳编辑器（增删，每项 title + collection_type）
 * - unknown → 强制用户选择
 * - other → 必填自定义类型名
 */
import { reactive, ref, computed, onMounted } from 'vue'
import { AppButton, AppIcon } from '@/components/ui'
import { useToast } from '@/composables/useToast'
import KnowledgeTreeCascader from '@/components/KnowledgeTreeCascader.vue'
import {
  documentApi,
  paperApi,
  type DocumentMeta,
  type DocumentType,
  type CollectionType,
  type CollectionMetaInput,
  type ConfirmDocumentRequest,
  type PaperBrief,
} from '@/api/client'

const props = defineProps<{
  /** 已分类的 Document（classify 之后） */
  doc: DocumentMeta
  /** confirm 请求进行中 */
  loading?: boolean
}>()

const emit = defineEmits<{
  (e: 'confirm', body: ConfirmDocumentRequest): void
  (e: 'back'): void
  (e: 'reclassify'): void
}>()

const toast = useToast()

// ─── 类型字典 ────────────────────────────────────────────────────────────
const DOCUMENT_TYPE_LABELS: Record<string, string> = {
  exam: '正式试卷',
  mock_exam: '模拟试卷',
  class_exercise: '课堂练习',
  class_example: '课堂例题',
  homework: '课后作业',
  preview_exercise: '课前预习',
  textbook_example: '教材例题',
  teaching_material: '教学讲义/资料',
  exercise_book: '教辅练习',
  chapter_exercise: '章节练习',
  unit_exercise: '单元练习',
  special_training: '专题训练',
  wrong_question: '错题整理',
  mixed: '混合资料',
  unknown: '无法判断',
  other: '其他',
}

const COLLECTION_TYPE_LABELS: Record<string, string> = {
  class_exercise: '课堂练习',
  class_example: '课堂例题',
  homework: '课后作业',
  preview_exercise: '课前预习',
  textbook_example: '教材例题',
  teaching_material: '教学讲义/资料',
  exercise_book: '教辅练习',
  chapter_exercise: '章节练习',
  unit_exercise: '单元练习',
  special_training: '专题训练',
  wrong_question: '错题整理',
  other: '其他',
}

const STAGE_LABELS = [
  { value: 'junior', label: '初中' },
  { value: 'senior', label: '高中' },
]
const SEMESTER_LABELS = [
  { value: 'first', label: '上学期' },
  { value: 'second', label: '下学期' },
  { value: 'full_year', label: '全年' },
]
const SUBJECT_LABELS = ['数学', '物理', '化学', '生物', '英语', '语文', '其他']

// ─── 表单状态 ────────────────────────────────────────────────────────────
/// AI 推荐类型（unknown 不预选，强制用户选择）
const aiType = computed(() => props.doc.ai_classification?.document_type ?? null)
const selectedType = ref<DocumentType | null>(
  aiType.value && aiType.value !== 'unknown' ? aiType.value : null,
)
const typeLabel = ref('')
const title = ref(
  props.doc.ai_classification?.title
    || props.doc.title
    || props.doc.file_name.replace(/\.[^.]+$/, ''),
)
const sourceType = ref('')
const subSourceType = ref('')

/// Paper 元数据（exam / mock_exam）
const paperForm = reactive({
  title: title.value,
  year: '',
  stage: '',
  grade: '',
  subject: '数学',
  semester: '',
  regionProvince: '',
  regionCity: '',
  schoolName: '',
  sourceType: '',
  subSourceType: '',
  paperId: '',
})

/// Collection 元数据（非试卷单集合）
const colForm = reactive({
  title: title.value,
  stage: '',
  grade: '',
  subject: '数学',
  semester: '',
  chapterId: '' as string,
  sourceType: '',
})

/// Mixed 集合壳编辑器
const mixedCollections = ref<CollectionMetaInput[]>([
  { title: '', collection_type: 'class_exercise' as CollectionType },
])

/// 关联已有试卷候选
const paperBriefs = ref<PaperBrief[]>([])

const isPaperType = computed(() => selectedType.value === 'exam' || selectedType.value === 'mock_exam')
const isMixed = computed(() => selectedType.value === 'mixed')
const isOther = computed(() => selectedType.value === 'other')

/// 章节单选（KnowledgeTreeCascader 为多选数组模型，此处转单值）
const chapterIdModel = computed({
  get: () => (colForm.chapterId ? [colForm.chapterId] : []),
  set: (v: string[]) => {
    colForm.chapterId = v[0] ?? ''
  },
})

onMounted(async () => {
  try {
    const { data } = await paperApi.listBrief()
    paperBriefs.value = data
  } catch {
    /* 关联试卷下拉失败不阻塞流程 */
  }
})

function selectType(t: DocumentType) {
  selectedType.value = t
  // 切换类型时同步元数据标题
  paperForm.title = title.value
  colForm.title = title.value
}

function addCollection() {
  mixedCollections.value.push({ title: '', collection_type: 'class_exercise' as CollectionType })
}

function removeCollection(i: number) {
  mixedCollections.value.splice(i, 1)
}

function doConfirm() {
  const t = selectedType.value
  if (!t) {
    toast.warning('请选择资料类型')
    return
  }
  if (t === 'unknown') {
    toast.warning('无法自动判断资料类型，请手动选择')
    return
  }
  if (t === 'other' && !typeLabel.value.trim()) {
    toast.warning('选择「其他」类型时必须填写自定义类型名')
    return
  }

  const body: ConfirmDocumentRequest = {
    document_type: t,
    type_label: t === 'other' ? typeLabel.value.trim() : undefined,
    title: title.value.trim() || undefined,
    source_type: sourceType.value.trim() || undefined,
    sub_source_type: subSourceType.value.trim() || undefined,
  }

  if (isPaperType.value) {
    if (!paperForm.title.trim()) {
      toast.warning('请填写试卷名称')
      return
    }
    body.paper_meta = {
      title: paperForm.title.trim(),
      year: paperForm.year ? Number(paperForm.year) : undefined,
      stage: paperForm.stage || undefined,
      grade: paperForm.grade || undefined,
      subject: paperForm.subject || undefined,
      semester: paperForm.semester || undefined,
      region_province: paperForm.regionProvince.trim() || undefined,
      region_city: paperForm.regionCity.trim() || undefined,
      school_name: paperForm.schoolName.trim() || undefined,
      source_type: paperForm.sourceType.trim() || undefined,
      sub_source_type: paperForm.subSourceType.trim() || undefined,
      paper_id: paperForm.paperId || undefined,
    }
  } else if (isMixed.value) {
    const list = mixedCollections.value.filter(c => c.title.trim())
    if (list.length === 0) {
      toast.warning('混合资料至少需要一个题目集合')
      return
    }
    body.collections = list.map(c => ({
      title: c.title.trim(),
      collection_type: c.collection_type,
      type_label: c.collection_type === 'other' ? c.type_label?.trim() || undefined : undefined,
    }))
  } else {
    // 非试卷非混合：集合元数据（后端会自动补默认集合；这里把用户填的集合信息一并提交）
    body.collections = [
      {
        title: colForm.title.trim() || title.value.trim() || '默认题目集合',
        collection_type: t as CollectionType,
        type_label: t === 'other' ? typeLabel.value.trim() : undefined,
        source_type: colForm.sourceType.trim() || undefined,
        subject: colForm.subject || undefined,
        stage: colForm.stage || undefined,
        grade: colForm.grade || undefined,
        semester: colForm.semester || undefined,
        chapter_id: colForm.chapterId || undefined,
      },
    ]
  }

  emit('confirm', body)
}
</script>

<template>
  <div class="doc-confirm-step">
    <!-- AI 推荐卡 -->
    <div v-if="props.doc.ai_classification" class="ai-recommend-card">
      <div class="ai-recommend-head">
        <AppIcon name="sparkles" :size="16" />
        <span>AI 识别结果</span>
        <span
          class="ai-confidence"
          :class="{ low: (props.doc.ai_classification.confidence || 0) < 0.6 }"
        >
          {{ Math.round((props.doc.ai_classification.confidence || 0) * 100) }}%
        </span>
      </div>
      <div class="ai-recommend-body">
        <span class="ai-recommend-type">
          {{
            props.doc.ai_classification.document_type === 'unknown'
              ? '无法确定'
              : DOCUMENT_TYPE_LABELS[props.doc.ai_classification.document_type] || props.doc.ai_classification.document_type
          }}
        </span>
        <span v-if="props.doc.ai_classification.reason" class="ai-recommend-reason">
          {{ props.doc.ai_classification.reason }}
        </span>
        <button class="ai-reclassify" type="button" @click="emit('reclassify')">重新识别</button>
      </div>
      <p v-if="props.doc.ai_classification.document_type === 'unknown'" class="ai-unknown-hint">
        无法确定该文件属于哪类资料，请选择资料类型。
      </p>
    </div>

    <!-- 资料类型单选 -->
    <div class="doc-type-section">
      <div class="doc-type-label">资料类型 <span class="doc-required">*</span></div>
      <div class="doc-type-grid">
        <button
          v-for="(label, key) in DOCUMENT_TYPE_LABELS"
          :key="key"
          type="button"
          class="doc-type-item"
          :class="{ active: selectedType === key }"
          @click="selectType(key as DocumentType)"
        >
          <span v-if="selectedType === key" class="doc-type-check"><AppIcon name="check" :size="12" /></span>
          {{ label }}
        </button>
      </div>
    </div>

    <!-- other：自定义类型名 -->
    <div v-if="isOther" class="doc-form-row">
      <label class="doc-form-label">自定义类型名 <span class="doc-required">*</span></label>
      <input v-model="typeLabel" class="doc-input" placeholder="如：校本资料 / 竞赛资料" />
    </div>

    <!-- 通用字段：资料名称 / 来源 -->
    <div class="doc-form-row">
      <label class="doc-form-label">资料名称</label>
      <input v-model="title" class="doc-input" placeholder="资料标题" />
    </div>
    <div class="doc-form-row">
      <label class="doc-form-label">来源</label>
      <input v-model="sourceType" class="doc-input" placeholder="如：teacher_created / 学校统一 / 网络" />
    </div>

    <!-- ── 试卷元数据表单（exam / mock_exam） ── -->
    <div v-if="isPaperType" class="doc-meta-card">
      <div class="doc-meta-title">试卷信息</div>
      <div class="doc-form-row">
        <label class="doc-form-label">试卷名称 <span class="doc-required">*</span></label>
        <input v-model="paperForm.title" class="doc-input" placeholder="如：2025 高一数学期中考试" />
      </div>
      <div class="doc-form-grid">
        <div class="doc-form-row">
          <label class="doc-form-label">年份</label>
          <input v-model="paperForm.year" type="number" min="2000" max="2100" class="doc-input" placeholder="如：2025" />
        </div>
        <div class="doc-form-row">
          <label class="doc-form-label">学段</label>
          <select v-model="paperForm.stage" class="doc-input">
            <option value="">未选择</option>
            <option v-for="s in STAGE_LABELS" :key="s.value" :value="s.value">{{ s.label }}</option>
          </select>
        </div>
        <div class="doc-form-row">
          <label class="doc-form-label">年级</label>
          <input v-model="paperForm.grade" class="doc-input" placeholder="如：高一 / 八年级" />
        </div>
        <div class="doc-form-row">
          <label class="doc-form-label">学科</label>
          <select v-model="paperForm.subject" class="doc-input">
            <option v-for="s in SUBJECT_LABELS" :key="s" :value="s">{{ s }}</option>
          </select>
        </div>
        <div class="doc-form-row">
          <label class="doc-form-label">学期</label>
          <select v-model="paperForm.semester" class="doc-input">
            <option value="">未选择</option>
            <option v-for="s in SEMESTER_LABELS" :key="s.value" :value="s.value">{{ s.label }}</option>
          </select>
        </div>
        <div class="doc-form-row">
          <label class="doc-form-label">地区（省）</label>
          <input v-model="paperForm.regionProvince" class="doc-input" placeholder="如：浙江省" />
        </div>
        <div class="doc-form-row">
          <label class="doc-form-label">地区（市）</label>
          <input v-model="paperForm.regionCity" class="doc-input" placeholder="如：杭州市" />
        </div>
        <div class="doc-form-row">
          <label class="doc-form-label">学校</label>
          <input v-model="paperForm.schoolName" class="doc-input" placeholder="学校名称" />
        </div>
        <div class="doc-form-row">
          <label class="doc-form-label">子来源</label>
          <input v-model="paperForm.subSourceType" class="doc-input" placeholder="如：一模 / 二模 / 联考" />
        </div>
      </div>
      <div class="doc-form-row">
        <label class="doc-form-label">关联已有试卷（可选）</label>
        <select v-model="paperForm.paperId" class="doc-input">
          <option value="">不关联，创建新试卷</option>
          <option v-for="p in paperBriefs" :key="p.id" :value="p.id">{{ p.title }}</option>
        </select>
      </div>
    </div>

    <!-- ── 非试卷单集合元数据 ── -->
    <div v-else-if="!isMixed && selectedType && selectedType !== 'unknown' && selectedType !== 'other'" class="doc-meta-card">
      <div class="doc-meta-title">资料集合信息</div>
      <div class="doc-form-grid">
        <div class="doc-form-row">
          <label class="doc-form-label">集合名称</label>
          <input v-model="colForm.title" class="doc-input" placeholder="如：二次函数课堂练习" />
        </div>
        <div class="doc-form-row">
          <label class="doc-form-label">学段</label>
          <select v-model="colForm.stage" class="doc-input">
            <option value="">未选择</option>
            <option v-for="s in STAGE_LABELS" :key="s.value" :value="s.value">{{ s.label }}</option>
          </select>
        </div>
        <div class="doc-form-row">
          <label class="doc-form-label">年级</label>
          <input v-model="colForm.grade" class="doc-input" placeholder="如：高一 / 八年级" />
        </div>
        <div class="doc-form-row">
          <label class="doc-form-label">学科</label>
          <select v-model="colForm.subject" class="doc-input">
            <option v-for="s in SUBJECT_LABELS" :key="s" :value="s">{{ s }}</option>
          </select>
        </div>
        <div class="doc-form-row">
          <label class="doc-form-label">学期</label>
          <select v-model="colForm.semester" class="doc-input">
            <option value="">未选择</option>
            <option v-for="s in SEMESTER_LABELS" :key="s.value" :value="s.value">{{ s.label }}</option>
          </select>
        </div>
        <div class="doc-form-row">
          <label class="doc-form-label">来源</label>
          <input v-model="colForm.sourceType" class="doc-input" placeholder="如：teacher_created" />
        </div>
      </div>
      <div class="doc-form-row">
        <label class="doc-form-label">章节（可选）</label>
        <KnowledgeTreeCascader v-model="chapterIdModel" :max="1" placeholder="选择章节…" />
      </div>
    </div>

    <!-- ── Mixed：集合壳编辑器 ── -->
    <div v-if="isMixed" class="doc-meta-card">
      <div class="doc-meta-title">
        题目集合（按题目分组，解析完成后分配）
        <AppButton size="sm" variant="ghost" @click="addCollection"><AppIcon name="plus" :size="14" /> 添加集合</AppButton>
      </div>
      <div v-for="(c, i) in mixedCollections" :key="i" class="mixed-collection-row">
        <input v-model="c.title" class="doc-input" :placeholder="`集合 ${i + 1} 名称（如：课堂例题）`" />
        <select v-model="c.collection_type" class="doc-input">
          <option v-for="(label, key) in COLLECTION_TYPE_LABELS" :key="key" :value="key">{{ label }}</option>
        </select>
        <input
          v-if="c.collection_type === 'other'"
          v-model="c.type_label"
          class="doc-input"
          placeholder="自定义类型名"
        />
        <button type="button" class="mixed-remove" :disabled="mixedCollections.length <= 1" @click="removeCollection(i)">
          <AppIcon name="trash" :size="14" />
        </button>
      </div>
    </div>

    <!-- 操作 -->
    <div class="doc-actions">
      <AppButton variant="ghost" @click="emit('back')">重新上传</AppButton>
      <AppButton variant="primary" :loading="props.loading" @click="doConfirm">
        <AppIcon name="check" :size="16" /> 确认资料类型
      </AppButton>
    </div>
  </div>
</template>

<style scoped>
.doc-confirm-step { display: flex; flex-direction: column; gap: 14px; }

/* AI 推荐卡 */
.ai-recommend-card {
  background: var(--bg-secondary, var(--bg-input));
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 12px 14px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.ai-recommend-head {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
}
.ai-confidence {
  margin-left: auto;
  font-size: 12px;
  font-weight: 700;
  color: var(--success);
  background: var(--success-light, rgba(52, 199, 89, 0.12));
  padding: 2px 8px;
  border-radius: 10px;
}
.ai-confidence.low { color: var(--warning); background: var(--warning-light); }
.ai-recommend-body { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
.ai-recommend-type { font-size: 15px; font-weight: 700; color: var(--purple); }
.ai-recommend-reason { font-size: 12px; color: var(--text-secondary); flex: 1; }
.ai-reclassify {
  font-size: 12px;
  color: var(--accent);
  background: none;
  border: none;
  cursor: pointer;
}
.ai-unknown-hint {
  font-size: 13px;
  color: var(--warning);
  background: var(--warning-light);
  padding: 6px 10px;
  border-radius: var(--radius);
  margin: 0;
}

/* 类型单选 */
.doc-type-section { display: flex; flex-direction: column; gap: 8px; }
.doc-type-label { font-size: 13px; font-weight: 600; color: var(--text-primary); }
.doc-required { color: var(--danger); }
.doc-type-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 6px; }
.doc-type-item {
  position: relative;
  padding: 8px 6px;
  font-size: 12px;
  text-align: center;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--bg-input);
  color: var(--text-secondary);
  cursor: pointer;
  transition: all 0.15s;
}
.doc-type-item:hover { border-color: var(--accent); color: var(--text-primary); }
.doc-type-item.active {
  border-color: var(--accent);
  background: var(--accent-light, rgba(88, 86, 214, 0.1));
  color: var(--accent);
  font-weight: 600;
}
.doc-type-check {
  position: absolute;
  top: -6px;
  right: -6px;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: var(--accent);
  color: white;
  display: flex;
  align-items: center;
  justify-content: center;
}

/* 表单 */
.doc-form-row { display: flex; flex-direction: column; gap: 4px; }
.doc-form-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
.doc-form-label { font-size: 12px; font-weight: 600; color: var(--text-secondary); }
.doc-input {
  width: 100%;
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: 8px;
  font-size: 13px;
  background: var(--bg-input);
  color: var(--text-primary);
  outline: none;
}
.doc-input:focus { border-color: var(--accent); }
.doc-meta-card {
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.doc-meta-title {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 13px;
  font-weight: 700;
  color: var(--text-primary);
}

/* Mixed 集合编辑器 */
.mixed-collection-row { display: flex; gap: 8px; align-items: center; }
.mixed-collection-row .doc-input { flex: 1; }
.mixed-remove {
  width: 32px;
  height: 32px;
  flex-shrink: 0;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: none;
  color: var(--danger);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
}
.mixed-remove:disabled { opacity: 0.4; cursor: not-allowed; }

.doc-actions { display: flex; justify-content: flex-end; gap: 8px; }
</style>
