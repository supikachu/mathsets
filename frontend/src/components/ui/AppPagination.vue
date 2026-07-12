<script setup lang="ts">
import AppIcon from './AppIcon.vue'

const props = defineProps<{
  page: number
  hasMore: boolean
}>()

const emit = defineEmits<{ 'update:page': [page: number] }>()

function prev() {
  if (props.page > 1) emit('update:page', props.page - 1)
}

function next() {
  if (props.hasMore) emit('update:page', props.page + 1)
}
</script>

<template>
  <div class="pagination">
    <button class="btn btn-ghost btn-sm" :disabled="page <= 1" @click="prev">
      <AppIcon name="chevron-left" :size="16" /> 上一页
    </button>
    <span class="page-info">第 {{ page }} 页</span>
    <button class="btn btn-ghost btn-sm" :disabled="!hasMore" @click="next">
      下一页 <AppIcon name="chevron-right" :size="16" />
    </button>
  </div>
</template>
