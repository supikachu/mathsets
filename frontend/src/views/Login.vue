<template>
  <WelcomeScreen title="协同题库系统" subtitle="多教师协同的数学题库平台">
    <div class="auth-logo"><AppIcon name="logo" :size="52" /></div>
    <h2 class="auth-title">欢迎回来</h2>

    <form @submit.prevent="handleLogin">
      <AppInput
        v-model="form.username"
        label="用户名"
        placeholder="请输入用户名"
        :error="errors.username"
        autocomplete="username"
      />
      <AppInput
        v-model="form.password"
        label="密码"
        type="password"
        placeholder="请输入密码"
        :error="errors.password"
        autocomplete="current-password"
      />
      <div class="form-group">
        <AppButton variant="primary" block :loading="loading" native-type="submit">
          登 录
        </AppButton>
      </div>
    </form>

    <div class="auth-footer">
      还没有账号？
      <router-link to="/register">注册</router-link>
    </div>
  </WelcomeScreen>
</template>

<script setup lang="ts">
import { ref, reactive } from 'vue'
import WelcomeScreen from '@/components/WelcomeScreen.vue'
import { AppInput, AppButton, AppIcon } from '@/components/ui'
import { useToast } from '@/composables/useToast'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()
const toast = useToast()
const loading = ref(false)

const form = reactive({
  username: '',
  password: '',
})

const errors = reactive({
  username: '',
  password: '',
})

function validate() {
  errors.username = form.username ? '' : '请输入用户名'
  errors.password = form.password ? '' : '请输入密码'
  return !errors.username && !errors.password
}

async function handleLogin() {
  if (!validate()) return

  loading.value = true
  try {
    await auth.login({ username: form.username, password: form.password })
    toast.success('登录成功')
  } catch (e: any) {
    toast.error(e.response?.data?.error || '登录失败')
  } finally {
    loading.value = false
  }
}
</script>

<style scoped>
.auth-logo {
  display: flex;
  justify-content: center;
  margin-bottom: 16px;
  color: #ffffff;
}
</style>
