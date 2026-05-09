<template>
  <el-container class="min-h-screen">
    <!-- 侧边栏 -->
    <el-aside width="220px" class="bg-gray-800 text-white">
      <div class="h-14 flex items-center justify-center font-bold text-lg border-b border-gray-700">
        📐 协同题库
      </div>

      <el-menu
        :default-active="route.path"
        router
        background-color="#1f2937"
        text-color="#d1d5db"
        active-text-color="#fff"
      >
        <el-menu-item index="/dashboard">
          <el-icon><Odometer /></el-icon>
          <span>工作台</span>
        </el-menu-item>
        <el-menu-item index="/questions">
          <el-icon><Document /></el-icon>
          <span>题目管理</span>
        </el-menu-item>
        <el-menu-item v-if="auth.isLeader" index="/review">
          <el-icon><Check /></el-icon>
          <span>审核队列</span>
        </el-menu-item>
        <el-menu-item index="/knowledge-points">
          <el-icon><CollectionTag /></el-icon>
          <span>知识点管理</span>
        </el-menu-item>
        <el-menu-item index="/papers">
          <el-icon><Document /></el-icon>
          <span>试卷管理</span>
        </el-menu-item>
      </el-menu>
    </el-aside>

    <el-container>
      <!-- 顶栏 -->
      <el-header class="bg-white shadow-sm flex items-center justify-end px-6">
        <el-dropdown trigger="click">
          <span class="cursor-pointer flex items-center gap-2">
            {{ auth.displayName }}
            <el-icon><ArrowDown /></el-icon>
          </span>
          <template #dropdown>
            <el-dropdown-menu>
              <el-dropdown-item @click="auth.logout()">退出登录</el-dropdown-item>
            </el-dropdown-menu>
          </template>
        </el-dropdown>
      </el-header>

      <!-- 主内容 -->
      <el-main class="bg-gray-50">
        <router-view />
      </el-main>
    </el-container>
  </el-container>
</template>

<script setup lang="ts">
import { useRoute } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import {
  Odometer, Document, Check, CollectionTag, ArrowDown,
} from '@element-plus/icons-vue'

const route = useRoute()
const auth = useAuthStore()
</script>
