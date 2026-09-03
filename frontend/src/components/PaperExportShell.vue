<script setup lang="ts">
/**
 * PaperExportShell — 试卷导出 + 预览的薄壳组件
 *
 * 统一封装 ExportDialog（格式/模式/版面参数）+ TypesetPreview（Typst SVG 实时预览），
 * 供 PaperDetail、Basket 等任何需要导出能力的视图复用。
 *
 * 宿主只需传入 sections（ExamSectionRequest[]）和基础元信息。
 */
import { ref } from 'vue'
import type { ExamSectionRequest } from '@/api/client'
import ExportDialog from '@/components/ExportDialog.vue'

const props = withDefaults(
  defineProps<{
    sections: ExamSectionRequest[]
    questionCount: number
    defaultTitle?: string
    scopeLabel?: string
  }>(),
  { defaultTitle: '未命名试卷', scopeLabel: '' },
)

const emit = defineEmits<{
  /** print 兜底回调 */
  print: []
}>()

const showExport = ref(false)

function open() {
  showExport.value = true
}

function close() {
  showExport.value = false
}

defineExpose({ open, close })
</script>

<template>
  <ExportDialog
    v-model="showExport"
    :sections="props.sections"
    :question-count="props.questionCount"
    :default-title="props.defaultTitle"
    :scope-label="props.scopeLabel"
    @print="emit('print')"
  />
</template>
