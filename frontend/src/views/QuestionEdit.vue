<template>
  <div>
    <h1 class="text-2xl font-bold mb-4">{{ isNew ? '创建新题目' : '编辑题目' }}</h1>
    <el-card>
      <div class="text-center text-gray-400 py-12">
        题目编辑页 — 开发中（含四种题型动态表单 + LaTeX 编辑器）
      </div>
    </el-card>
  </div>
</template>

<script setup lang="ts">
import { useRoute } from 'vue-router'
const route = useRoute()
const isNew = route.path.endsWith('/new')
</script>
