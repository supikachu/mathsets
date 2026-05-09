<template>
  <div>
    <div class="flex items-center justify-between mb-4">
      <h1 class="text-2xl font-bold">📝 题目管理</h1>
      <el-button type="primary" @click="$router.push('/questions/new')">
        ➕ 新建题目
      </el-button>
    </div>

    <el-card>
      <div class="text-center text-gray-400 py-12">
        题目列表页 — 开发中
      </div>
    </el-card>
  </div>
</template>
