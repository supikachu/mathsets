<template>
  <div>
    <h1 class="text-2xl font-bold mb-6">欢迎回来，{{ auth.displayName }} 👋</h1>

    <el-row :gutter="20" class="mb-8">
      <el-col :span="6" v-for="stat in stats" :key="stat.label">
        <el-card shadow="hover">
          <div class="text-3xl font-bold" :style="{ color: stat.color }">
            {{ stat.value }}
          </div>
          <div class="text-sm text-gray-500 mt-1">{{ stat.label }}</div>
        </el-card>
      </el-col>
    </el-row>

    <el-card>
      <template #header>
        <span>快速操作</span>
      </template>
      <div class="flex gap-4">
        <el-button type="primary" @click="$router.push('/questions/new')">
          ➕ 创建新题目
        </el-button>
        <el-button @click="$router.push('/questions')">📝 浏览题库</el-button>
        <el-button v-if="auth.isLeader" @click="$router.push('/review')">
          🔍 审核队列
        </el-button>
      </div>
    </el-card>
  </div>
</template>

<script setup lang="ts">
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()

const stats = [
  { label: '总题目', value: '—', color: '#409eff' },
  { label: '待审核', value: '—', color: '#e6a23c' },
  { label: '已发布', value: '—', color: '#67c23a' },
  { label: '草稿', value: '—', color: '#909399' },
]
</script>
