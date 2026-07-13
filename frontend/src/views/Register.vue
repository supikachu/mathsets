<template>
  <WelcomeScreen title="协同题库系统" subtitle="注册账号，加入协同题库">
    <div class="auth-logo"><AppIcon name="logo" :size="52" /></div>
    <h2 class="auth-title">注册新账号</h2>

    <form @submit.prevent="handleRegister">
      <AppInput
        v-model="form.username"
        label="用户名"
        placeholder="用于登录"
        :error="errors.username"
      />
      <AppInput
        v-model="form.display_name"
        label="显示名称"
        placeholder="真实姓名"
        :error="errors.display_name"
      />
      <AppInput
        v-model="form.email"
        label="邮箱"
        type="email"
        placeholder="email@example.com"
        :error="errors.email"
      />
      <AppInput
        v-model="form.password"
        label="密码"
        type="password"
        :error="errors.password"
      />
      <div class="form-group">
        <AppButton variant="primary" block :loading="loading" native-type="submit">
          注 册
        </AppButton>
      </div>
    </form>

    <div class="auth-footer">
      已有账号？<router-link to="/login">登录</router-link>
    </div>
  </WelcomeScreen>
</template>

<script setup lang="ts">
import { ref, reactive } from 'vue'
import { useRouter } from 'vue-router'
import WelcomeScreen from '@/components/WelcomeScreen.vue'
import { AppInput, AppButton, AppIcon } from '@/components/ui'
import { useToast } from '@/composables/useToast'
import { authApi } from '@/api/client'

const router = useRouter()
const toast = useToast()
const loading = ref(false)

const form = reactive({
  username: '',
  display_name: '',
  email: '',
  password: '',
})

const errors = reactive({
  username: '',
  display_name: '',
  email: '',
  password: '',
})

function validate() {
  errors.username = form.username ? '' : '请输入用户名'
  errors.display_name = form.display_name ? '' : '请输入显示名称'
  errors.email = /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(form.email)
    ? ''
    : form.email
      ? '邮箱格式不正确'
      : '请输入邮箱'
  errors.password = form.password.length >= 6
    ? ''
    : form.password
      ? '密码至少 6 位'
      : '请输入密码'
  return !errors.username && !errors.display_name && !errors.email && !errors.password
}

async function handleRegister() {
  if (!validate()) return

  loading.value = true
  try {
    await authApi.register(form)
    toast.success('注册成功，请登录')
    router.replace('/login')
  } catch (e: any) {
    toast.error(e.response?.data?.error || '注册失败')
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
