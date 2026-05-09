<template>
  <div class="min-h-screen flex items-center justify-center bg-gray-50">
    <div class="text-center">
      <div class="text-8xl font-bold text-gray-200 mb-4">404</div>
      <h1 class="text-2xl font-bold mb-2">页面不存在</h1>
      <p class="text-gray-500 mb-6">你访问的页面可能已被删除或地址有误</p>
      <el-button type="primary" @click="$router.push('/dashboard')">返回工作台</el-button>
    </div>
  </div>
</template>
