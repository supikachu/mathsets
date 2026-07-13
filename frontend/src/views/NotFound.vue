<template>
  <div class="not-found-page">
    <div class="not-found-content">
      <AppIcon name="compass" :size="64" class="not-found-icon" />
      <div class="not-found-code">404</div>
      <h1 class="page-title">页面不存在</h1>
      <p class="text-muted mb-12">你访问的页面可能已被删除或地址有误</p>
      <AppButton variant="primary" @click="$router.replace('/questions')">
        <AppIcon name="chevron-left" :size="17" /> 返回题库
      </AppButton>
    </div>
  </div>
</template>

<script setup lang="ts">
import { AppButton, AppIcon } from '@/components/ui'
</script>

<style scoped>
.not-found-page {
  min-height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-primary);
  padding: 24px;
}

.not-found-content {
  text-align: center;
}

.not-found-code {
  font-size: 96px;
  font-weight: 800;
  color: var(--border-color);
  line-height: 1;
  margin-bottom: 16px;
}

.not-found-icon {
  color: var(--text-muted);
  margin-bottom: 8px;
}
</style>
