<template>
  <div class="profile-page">
    <!-- ===== Apple风格吸顶标题栏 ===== -->
    <div class="profile-sticky-bar">
      <div class="profile-header">
        <button class="back-btn" @click="$router.back()" aria-label="返回">
          <AppIcon name="chevron-left" :size="18" />
        </button>
        <h1 class="page-title">个人中心</h1>
        <ThemeToggle />
      </div>
    </div>

    <!-- ===== 主体滚动区域 ===== -->
    <div class="profile-scroll-area">
      <div class="profile-container">
        <!-- ════════════════════════════════════════
             1. 头像区
             ════════════════════════════════════════ -->
        <section class="profile-card avatar-section">
          <div class="section-title">
            <AppIcon name="camera" :size="16" />
            <span>头像</span>
          </div>

          <div class="avatar-block">
            <!-- 头像预览：128px 圆形，object-fit: cover 防拉伸 -->
            <div class="avatar-preview-wrap">
              <img
                v-if="avatarPreviewSrc"
                :src="avatarPreviewSrc"
                class="avatar-preview"
                alt="头像预览"
              />
              <div v-else class="avatar-fallback">
                {{ avatarLetter }}
              </div>

              <!-- 上传中遮罩 -->
              <div v-if="avatarState === 'uploading'" class="avatar-mask">
                <AppProgress statusText="" :size="20" />
              </div>
            </div>

            <div class="avatar-actions">
              <input
                ref="fileInputRef"
                type="file"
                accept="image/jpeg,image/png,image/webp"
                class="avatar-input-hidden"
                @change="onFileSelected"
              />
              <AppButton
                variant="outline"
                size="md"
                :loading="avatarState === 'uploading'"
                @click="fileInputRef?.click()"
              >
                <AppIcon name="upload" :size="14" />
                <span style="margin-left: 4px;">{{ avatarState === 'uploading' ? '上传中…' : '选择图片' }}</span>
              </AppButton>
              <p class="avatar-hint">
                支持 JPG / PNG / WebP，最大 2 MB；推荐正方形图片。
              </p>
              <p v-if="avatarState === 'error'" class="avatar-error">{{ avatarErrorMsg }}</p>
            </div>
          </div>
        </section>

        <!-- ════════════════════════════════════════
             2. 基础信息区
             ════════════════════════════════════════ -->
        <section class="profile-card info-section">
          <div class="section-title">
            <AppIcon name="user" :size="16" />
            <span>基础信息</span>
          </div>

          <div class="info-grid">
            <AppInput
              v-model="form.displayName"
              label="昵称"
              placeholder="请输入昵称"
              :error="errors.displayName"
            />
            <AppInput
              v-model="form.email"
              label="邮箱"
              type="email"
              placeholder="user@example.com"
              :error="errors.email"
            />
            <AppInput
              :modelValue="profile?.username || ''"
              label="账号（只读）"
              disabled
            />
            <div class="form-group">
              <label class="form-label">注册时间</label>
              <div class="readonly-field">
                {{ profile ? formatTime(profile.created_at) : '—' }}
              </div>
            </div>
            <div class="form-group">
              <label class="form-label">角色</label>
              <div class="readonly-field">
                <AppBadge :color="roleBadgeColor">{{ roleLabel }}</AppBadge>
              </div>
            </div>
          </div>

          <div class="info-actions">
            <AppButton
              variant="primary"
              :loading="savingProfile"
              :disabled="!hasProfileChanges"
              @click="onSaveProfile"
            >
              <AppIcon name="save" :size="14" />
              <span style="margin-left: 4px;">保存修改</span>
            </AppButton>
          </div>
        </section>

        <!-- ════════════════════════════════════════
             3. 安全设置区
             ════════════════════════════════════════ -->
        <section class="profile-card security-section">
          <div class="section-title">
            <AppIcon name="lock" :size="16" />
            <span>安全设置</span>
          </div>

          <div class="info-grid">
            <AppInput
              v-model="pwForm.oldPassword"
              label="当前密码"
              type="password"
              placeholder="请输入当前密码"
              autocomplete="current-password"
              :error="pwErrors.oldPassword"
            />
            <AppInput
              v-model="pwForm.newPassword"
              label="新密码"
              type="password"
              placeholder="至少 8 位"
              autocomplete="new-password"
              :error="pwErrors.newPassword"
            />
            <AppInput
              v-model="pwForm.confirmPassword"
              label="确认新密码"
              type="password"
              placeholder="再次输入新密码"
              autocomplete="new-password"
              :error="pwErrors.confirmPassword"
            />
          </div>

          <div class="info-actions">
            <AppButton
              variant="primary"
              :loading="changingPassword"
              :disabled="!canSubmitPassword"
              @click="onChangePassword"
            >
              <AppIcon name="key" :size="14" />
              <span style="margin-left: 4px;">修改密码</span>
            </AppButton>
            <p class="security-hint">
              修改成功后将自动退出登录，请使用新密码重新登录。
            </p>
          </div>
        </section>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, reactive } from 'vue'
import { useRouter } from 'vue-router'
import { userApi, type UserProfile } from '@/api/client'
import { useAuthStore } from '@/stores/auth'
import { useToast } from '@/composables/useToast'
import { compressImage, blobToFile } from '@/utils/imageCompressor'
import { AppButton, AppIcon, AppInput, AppBadge, AppProgress } from '@/components/ui'
import ThemeToggle from '@/components/ThemeToggle.vue'

const router = useRouter()
const auth = useAuthStore()
const toast = useToast()

// ---------------------------------------------------------------------------
// Profile 数据
// ---------------------------------------------------------------------------
const profile = ref<UserProfile | null>(null)
const loading = ref(false)
const savingProfile = ref(false)
const changingPassword = ref(false)

const form = reactive({
  displayName: '',
  email: '',
})

const errors = reactive({
  displayName: '',
  email: '',
})

const pwForm = reactive({
  oldPassword: '',
  newPassword: '',
  confirmPassword: '',
})

const pwErrors = reactive({
  oldPassword: '',
  newPassword: '',
  confirmPassword: '',
})

// ---------------------------------------------------------------------------
// 加载数据
// ---------------------------------------------------------------------------
async function loadProfile() {
  loading.value = true
  try {
    const res = await userApi.getMe()
    profile.value = res.data
    form.displayName = res.data.display_name
    form.email = res.data.email
  } catch (e: any) {
    toast.error(e?.response?.data?.error || '加载个人资料失败')
  } finally {
    loading.value = false
  }
}

onMounted(loadProfile)

// ---------------------------------------------------------------------------
// 头像逻辑
// ---------------------------------------------------------------------------
type AvatarState = 'idle' | 'uploading' | 'error'
const avatarState = ref<AvatarState>('idle')
const avatarErrorMsg = ref('')
const avatarPreviewSrc = ref<string>('')
const fileInputRef = ref<HTMLInputElement | null>(null)

/// 用户首字母（头像 fallback）
const avatarLetter = computed(() =>
  (auth.displayName || '?').charAt(0).toUpperCase(),
)

/// 初始化头像预览（来自后端 avatar_url）
onMounted(() => {
  if (auth.avatarUrl) {
    avatarPreviewSrc.value = auth.avatarUrl
  }
})

const MAX_AVATAR_BYTES = 2 * 1024 * 1024

async function onFileSelected(e: Event) {
  const input = e.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return

  // 重置错误状态
  avatarState.value = 'idle'
  avatarErrorMsg.value = ''

  // 1. 大小预校验（2MB）
  if (file.size > MAX_AVATAR_BYTES) {
    avatarState.value = 'error'
    avatarErrorMsg.value = '图片大小不能超过 2MB'
    toast.error('图片大小不能超过 2MB')
    // 清空 input 让用户可以重新选择同一文件
    input.value = ''
    return
  }

  // 2. MIME 类型预校验
  const allowedTypes = ['image/jpeg', 'image/png', 'image/webp']
  if (!allowedTypes.includes(file.type)) {
    avatarState.value = 'error'
    avatarErrorMsg.value = '仅支持 JPG / PNG / WebP 格式'
    toast.error('仅支持 JPG / PNG / WebP 格式')
    input.value = ''
    return
  }

  // 3. 上传（先压缩，复用题目图片压缩工具）
  avatarState.value = 'uploading'
  try {
    // 头像压缩：长边 ≤ 256 足矣，但复用 compressImage（长边 2000）也无所谓
    // 注：compressImage 会自动转 WebP，后端 Magic Bytes 校验兼容
    const compressedBlob = await compressImage(file)
    // 修正 MIME（与之前图片上传补丁一致 — 防止 blob.type 为空）
    const mimeType = compressedBlob.type || 'image/webp'
    const compressedFile = new File([compressedBlob], file.name || 'avatar.webp', {
      type: mimeType,
    })

    // 调用 store 上传 — 成功后 store 会自动同步 localStorage
    const newAvatarUrl = await auth.uploadAvatar(compressedFile)
    avatarPreviewSrc.value = newAvatarUrl
    avatarState.value = 'idle'
    toast.success('头像更新成功')
  } catch (e: any) {
    avatarState.value = 'error'
    avatarErrorMsg.value = e?.response?.data?.error || '头像上传失败'
    toast.error(e?.response?.data?.error || '头像上传失败')
  } finally {
    // 清空 input 让用户可以重新选择同一文件
    input.value = ''
  }
}

// ---------------------------------------------------------------------------
// 基础信息表单逻辑
// ---------------------------------------------------------------------------
const hasProfileChanges = computed(() => {
  if (!profile.value) return false
  return (
    form.displayName !== profile.value.display_name ||
    form.email !== profile.value.email
  )
})

function validateProfile(): boolean {
  errors.displayName = ''
  errors.email = ''

  const dn = form.displayName.trim()
  if (!dn) {
    errors.displayName = '昵称不能为空'
    return false
  }
  if (dn.length > 100) {
    errors.displayName = '昵称长度不能超过 100 字符'
    return false
  }

  const email = form.email.trim()
  if (!email) {
    errors.email = '邮箱不能为空'
    return false
  }
  // 简易邮箱正则
  const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/
  if (!emailRegex.test(email)) {
    errors.email = '邮箱格式不正确'
    return false
  }
  return true
}

async function onSaveProfile() {
  if (!validateProfile()) return

  savingProfile.value = true
  try {
    const updated = await auth.updateProfile({
      display_name: form.displayName.trim(),
      email: form.email.trim(),
    })
    profile.value = updated
    toast.success('个人资料已更新')
  } catch (e: any) {
    const msg = e?.response?.data?.error || '更新失败'
    // 字段级错误识别
    if (msg.includes('邮箱')) {
      errors.email = msg
    } else if (msg.includes('昵称')) {
      errors.displayName = msg
    }
    toast.error(msg)
  } finally {
    savingProfile.value = false
  }
}

// ---------------------------------------------------------------------------
// 修改密码逻辑
// ---------------------------------------------------------------------------
const canSubmitPassword = computed(() => {
  return (
    pwForm.oldPassword.length > 0 &&
    pwForm.newPassword.length >= 8 &&
    pwForm.confirmPassword.length > 0
  )
})

function validatePassword(): boolean {
  pwErrors.oldPassword = ''
  pwErrors.newPassword = ''
  pwErrors.confirmPassword = ''

  if (!pwForm.oldPassword) {
    pwErrors.oldPassword = '请输入当前密码'
    return false
  }
  if (pwForm.newPassword.length < 8) {
    pwErrors.newPassword = '新密码长度至少 8 位'
    return false
  }
  if (pwForm.newPassword === pwForm.oldPassword) {
    pwErrors.newPassword = '新密码不能与旧密码相同'
    return false
  }
  if (pwForm.confirmPassword !== pwForm.newPassword) {
    pwErrors.confirmPassword = '两次输入的新密码不一致'
    return false
  }
  return true
}

async function onChangePassword() {
  if (!validatePassword()) return

  changingPassword.value = true
  try {
    await userApi.changePassword({
      old_password: pwForm.oldPassword,
      new_password: pwForm.newPassword,
    })
    toast.success('密码修改成功，即将跳转登录页…')

    // 清空表单（防止密码在内存中残留）
    pwForm.oldPassword = ''
    pwForm.newPassword = ''
    pwForm.confirmPassword = ''

    // ⚠️ 安全约束：修改成功后必须强制登出 + 跳转登录页
    // 给 toast 一点展示时间再跳转
    setTimeout(() => {
      auth.logout()
    }, 1200)
  } catch (e: any) {
    const msg = e?.response?.data?.error || '密码修改失败'
    if (msg.includes('旧密码')) {
      pwErrors.oldPassword = msg
    }
    toast.error(msg)
  } finally {
    changingPassword.value = false
  }
}

// ---------------------------------------------------------------------------
// 渲染辅助
// ---------------------------------------------------------------------------
const roleLabel = computed(() => {
  if (!profile.value) return ''
  const r = profile.value.global_role || profile.value.role
  if (r === 'super_admin' || r === 'Admin' || r === 'admin') return '系统管理员'
  return '教师'
})

const roleBadgeColor = computed(() => {
  if (!profile.value) return 'gray'
  const r = profile.value.global_role || profile.value.role
  if (r === 'super_admin' || r === 'Admin' || r === 'admin') return 'purple'
  return 'teal'
})

function formatTime(iso: string): string {
  try {
    return new Date(iso).toLocaleString('zh-CN', {
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
    })
  } catch {
    return iso
  }
}

// 离开页面时清空密码字段（防止浏览器后退泄露）
onMounted(() => {
  window.addEventListener('beforeunload', () => {
    pwForm.oldPassword = ''
    pwForm.newPassword = ''
    pwForm.confirmPassword = ''
  })
})

// 路由离开时也清空
import { onBeforeUnmount } from 'vue'
onBeforeUnmount(() => {
  pwForm.oldPassword = ''
  pwForm.newPassword = ''
  pwForm.confirmPassword = ''
})
</script>

<style scoped>
/* ===== Apple风格页面骨架（与 QuestionList.vue 保持一致）===== */
.profile-page {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.profile-sticky-bar {
  position: sticky;
  top: 0;
  z-index: 100;
  flex-shrink: 0;
  background: var(--bg-primary);
  backdrop-filter: saturate(180%) blur(20px);
  -webkit-backdrop-filter: saturate(180%) blur(20px);
  border-bottom: 1px solid var(--border-color);
}

.profile-header {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 24px;
  max-width: var(--max-width);
  margin: 0 auto;
}

.back-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  border-radius: var(--radius-full);
  background: transparent;
  color: var(--text-primary);
  transition: var(--transition-fast);
}

.back-btn:hover {
  background: var(--bg-hover);
}

.page-title {
  flex: 1;
  font-size: 17px;
  font-weight: 600;
  color: var(--text-primary);
  margin: 0;
}

/* ===== 滚动区域（独立滚动域，参考 QuestionList 滚动隔离约束）===== */
.profile-scroll-area {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
  background: var(--bg-primary);
}

.profile-container {
  max-width: 720px;
  margin: 0 auto;
  padding: 24px 24px 48px;
  display: flex;
  flex-direction: column;
  gap: 20px;
}

/* ===== 卡片样式（与 QuestionList 的 .q-item 高度一致） ===== */
.profile-card {
  background: var(--bg-card);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-sm);
  padding: 20px 24px;
  display: flex;
  flex-direction: column;
  gap: 16px;
  transition: var(--transition);
}

.section-title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary);
  letter-spacing: 0.02em;
  text-transform: uppercase;
}

/* ===== 头像区 ===== */
.avatar-block {
  display: flex;
  align-items: center;
  gap: 20px;
  flex-wrap: wrap;
}

.avatar-preview-wrap {
  position: relative;
  width: 96px;
  height: 96px;
  flex-shrink: 0;
}

.avatar-preview {
  width: 96px;
  height: 96px;
  border-radius: 50%;
  object-fit: cover; /* ⚠️ 关键约束：防非正方形图片被拉伸 */
  background: var(--bg-input);
  box-shadow: var(--shadow-xs);
}

.avatar-fallback {
  width: 96px;
  height: 96px;
  border-radius: 50%;
  background: var(--accent-gradient);
  color: #fff;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 36px;
  font-weight: 700;
  box-shadow: var(--shadow-xs);
}

.avatar-mask {
  position: absolute;
  inset: 0;
  border-radius: 50%;
  background: rgba(0, 0, 0, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
  color: #fff;
}

.avatar-actions {
  display: flex;
  flex-direction: column;
  gap: 8px;
  flex: 1;
  min-width: 200px;
}

.avatar-input-hidden {
  display: none;
}

.avatar-hint {
  font-size: 12px;
  color: var(--text-muted);
  margin: 0;
}

.avatar-error {
  font-size: 12px;
  color: var(--danger);
  margin: 0;
}

/* ===== 表单区域 ===== */
.info-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 16px 20px;
}

@media (max-width: 640px) {
  .info-grid {
    grid-template-columns: 1fr;
  }
}

.form-group {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.form-label {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-secondary);
}

.readonly-field {
  padding: 9px 12px;
  background: var(--bg-input);
  border-radius: var(--radius-xs);
  font-size: 14px;
  color: var(--text-primary);
  min-height: 38px;
  display: flex;
  align-items: center;
}

/* ⚠️ AppInput 内部 <input> 的样式继承 — 与 QuestionList 保持一致 */
:deep(.form-group input) {
  width: 100%;
  padding: 9px 12px;
  border: 1px solid transparent;
  border-radius: var(--radius-xs);
  background: var(--bg-input);
  font-size: 14px;
  color: var(--text-primary);
  outline: none;
  transition: var(--transition-fast);
  font-family: inherit;
}

:deep(.form-group input:focus) {
  border-color: var(--accent);
  background: var(--bg-card);
  box-shadow: 0 0 0 3px var(--accent-light);
}

:deep(.form-group input:disabled) {
  opacity: 0.6;
  cursor: not-allowed;
}

/* ===== 操作按钮区 ===== */
.info-actions {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
  padding-top: 4px;
}

.security-hint {
  font-size: 12px;
  color: var(--text-muted);
  margin: 0;
}
</style>
