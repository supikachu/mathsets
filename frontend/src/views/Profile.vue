<template>
  <div class="profile-page">
    <!-- ===== 吸顶顶栏 ===== -->
    <div class="profile-sticky-bar">
      <div class="profile-header">
        <button class="back-btn" @click="$router.back()" aria-label="返回">
          <AppIcon name="chevron-left" :size="18" />
        </button>
        <h1 class="page-title">个人中心与设置</h1>
      </div>
    </div>

    <!-- ===== 主体双栏内容区域 ===== -->
    <div class="profile-scroll-area">
      <div class="w-full max-w-6xl mx-auto p-4 sm:p-6 md:p-8 flex flex-col md:flex-row gap-6 md:gap-8 items-start">
        
        <!-- 左侧设置导航栏 -->
        <aside class="w-full md:w-56 shrink-0 flex flex-col gap-1.5 bg-white dark:bg-slate-900 p-3 rounded-2xl border border-gray-100 dark:border-slate-800 shadow-sm select-none">
          <div class="px-3 py-1.5 text-xs font-semibold text-gray-400 dark:text-gray-500 uppercase tracking-wider">
            设置分类
          </div>
          
          <button
            v-for="tab in navTabs"
            :key="tab.id"
            type="button"
            class="flex items-center gap-3 px-3.5 py-2.5 text-sm font-medium rounded-xl transition-all cursor-pointer text-left"
            :class="activeTab === tab.id
              ? 'bg-blue-50 dark:bg-blue-950/60 text-blue-600 dark:text-blue-400 font-semibold shadow-xs'
              : 'text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-slate-800/60 hover:text-gray-900 dark:hover:text-gray-100'"
            @click="activeTab = tab.id"
          >
            <AppIcon :name="tab.icon" :size="17" />
            <span>{{ tab.label }}</span>
          </button>
        </aside>

        <!-- 右侧动态内容区 -->
        <main class="flex-1 min-w-0 flex flex-col gap-6 w-full">
          
          <!-- TAB 1: 个人资料 -->
          <template v-if="activeTab === 'profile'">
            <!-- 1.1 顶端头像与身份卡片 -->
            <section class="flex flex-col sm:flex-row items-center justify-between gap-6 p-6 bg-white dark:bg-slate-900 rounded-2xl border border-gray-100 dark:border-slate-800 shadow-sm">
              <div class="flex items-center gap-5 min-w-0 w-full sm:w-auto">
                <div class="relative w-20 h-20 rounded-full overflow-hidden shrink-0 border-2 border-gray-100 dark:border-slate-800 shadow-sm bg-gray-100 dark:bg-slate-800">
                  <img
                    v-if="avatarPreviewSrc"
                    :src="avatarPreviewSrc"
                    class="w-full h-full object-cover"
                    alt="头像预览"
                  />
                  <div v-else class="w-full h-full bg-gradient-to-br from-blue-500 to-indigo-600 text-white flex items-center justify-center text-2xl font-bold">
                    {{ avatarLetter }}
                  </div>
                  <!-- 上传中遮罩 -->
                  <div v-if="avatarState === 'uploading'" class="absolute inset-0 bg-black/50 backdrop-blur-xs flex items-center justify-center text-white">
                    <AppProgress statusText="" :size="20" />
                  </div>
                </div>

                <div class="flex flex-col min-w-0 gap-1">
                  <div class="flex items-center gap-2.5 flex-wrap">
                    <h2 class="text-lg font-bold text-gray-900 dark:text-gray-100 truncate">{{ auth.displayName || '未命名用户' }}</h2>
                    <AppBadge :color="roleBadgeColor">{{ roleLabel }}</AppBadge>
                  </div>
                  <p class="text-xs text-gray-500 dark:text-gray-400 truncate">{{ profile?.email || '暂无邮箱' }}</p>
                </div>
              </div>

              <!-- 更载/更换头像按钮与逻辑 -->
              <div class="flex flex-col items-center sm:items-end gap-2 shrink-0 w-full sm:w-auto border-t sm:border-t-0 border-gray-100 dark:border-slate-800 pt-4 sm:pt-0">
                <input
                  ref="fileInputRef"
                  type="file"
                  accept="image/jpeg,image/png,image/webp"
                  class="hidden"
                  @change="onFileSelected"
                />
                <AppButton
                  variant="outline"
                  size="md"
                  :loading="avatarState === 'uploading'"
                  @click="fileInputRef?.click()"
                >
                  <AppIcon name="upload" :size="14" />
                  <span class="ml-1.5">{{ avatarState === 'uploading' ? '上传中…' : '更换头像' }}</span>
                </AppButton>
                <span class="text-xs text-gray-400 dark:text-gray-500">支持 JPG/PNG/WebP，最大 2MB</span>
                <span v-if="avatarState === 'error'" class="text-xs text-red-500">{{ avatarErrorMsg }}</span>
              </div>
            </section>

            <!-- 1.2 基础信息表单卡片 (两列网格布局) -->
            <section class="p-6 bg-white dark:bg-slate-900 rounded-2xl border border-gray-100 dark:border-slate-800 shadow-sm flex flex-col gap-6">
              <div class="flex items-center gap-2 text-sm font-semibold text-gray-900 dark:text-gray-100 border-b border-gray-100 dark:border-slate-800/80 pb-3">
                <AppIcon name="user" :size="16" class="text-blue-500" />
                <span>基础资料信息</span>
              </div>

              <div class="grid grid-cols-1 sm:grid-cols-2 gap-5">
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
                  label="账号用户名（只读）"
                  disabled
                />
                <div class="form-group flex flex-col gap-1.5">
                  <label class="text-xs font-medium text-gray-600 dark:text-gray-400">注册时间</label>
                  <div class="px-3 py-2 bg-gray-50 dark:bg-slate-800/60 rounded-lg text-sm text-gray-700 dark:text-gray-300 min-h-[38px] flex items-center border border-gray-100 dark:border-slate-800">
                    {{ profile ? formatTime(profile.created_at) : '—' }}
                  </div>
                </div>
                <div class="form-group flex flex-col gap-1.5">
                  <label class="text-xs font-medium text-gray-600 dark:text-gray-400">用户身份角色</label>
                  <div class="px-3 py-2 bg-gray-50 dark:bg-slate-800/60 rounded-lg text-sm text-gray-700 dark:text-gray-300 min-h-[38px] flex items-center border border-gray-100 dark:border-slate-800">
                    <AppBadge :color="roleBadgeColor">{{ roleLabel }}</AppBadge>
                  </div>
                </div>
              </div>

              <div class="flex justify-end pt-2 border-t border-gray-100 dark:border-slate-800/80">
                <AppButton
                  variant="primary"
                  :loading="savingProfile"
                  :disabled="!hasProfileChanges"
                  @click="onSaveProfile"
                >
                  <AppIcon name="save" :size="14" />
                  <span class="ml-1.5">保存修改</span>
                </AppButton>
              </div>
            </section>
          </template>

          <!-- TAB 2: 安全设置 (修改密码) -->
          <template v-else-if="activeTab === 'security'">
            <section class="p-6 bg-white dark:bg-slate-900 rounded-2xl border border-gray-100 dark:border-slate-800 shadow-sm flex flex-col gap-6">
              <div class="flex items-center gap-2 text-sm font-semibold text-gray-900 dark:text-gray-100 border-b border-gray-100 dark:border-slate-800/80 pb-3">
                <AppIcon name="lock" :size="16" class="text-blue-500" />
                <span>账号登录密码修改</span>
              </div>

              <div class="grid grid-cols-1 sm:grid-cols-2 gap-5">
                <AppInput
                  v-model="pwForm.oldPassword"
                  label="当前原密码"
                  type="password"
                  placeholder="请输入当前原密码"
                  autocomplete="current-password"
                  :error="pwErrors.oldPassword"
                  class="sm:col-span-2 max-w-md"
                />
                <AppInput
                  v-model="pwForm.newPassword"
                  label="新设置密码"
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

              <div class="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4 pt-2 border-t border-gray-100 dark:border-slate-800/80">
                <p class="text-xs text-gray-400 dark:text-gray-500">
                  修改成功后将自动注销登录，请使用新密码重新登录。
                </p>
                <AppButton
                  variant="primary"
                  :loading="changingPassword"
                  :disabled="!canSubmitPassword"
                  @click="onChangePassword"
                >
                  <AppIcon name="key" :size="14" />
                  <span class="ml-1.5">修改密码</span>
                </AppButton>
              </div>
            </section>
          </template>

          <!-- TAB 3: 外观与偏好 (主题切换) -->
          <template v-else-if="activeTab === 'appearance'">
            <section class="p-6 bg-white dark:bg-slate-900 rounded-2xl border border-gray-100 dark:border-slate-800 shadow-sm flex flex-col gap-6">
              <div class="flex items-center gap-2 text-sm font-semibold text-gray-900 dark:text-gray-100 border-b border-gray-100 dark:border-slate-800/80 pb-3">
                <AppIcon name="sun" :size="16" class="text-blue-500" />
                <span>系统界面外观偏好</span>
              </div>

              <div class="flex items-center justify-between p-4 bg-gray-50 dark:bg-slate-800/50 rounded-xl border border-gray-100 dark:border-slate-800">
                <div class="flex flex-col gap-1">
                  <span class="text-sm font-medium text-gray-900 dark:text-gray-100">明暗主题模式</span>
                  <span class="text-xs text-gray-500 dark:text-gray-400">选择您偏好的系统颜色主题（浅色 / 深色模式瞬间切换）</span>
                </div>
                <ThemeToggle />
              </div>
            </section>
          </template>

          <!-- TAB 4: AI 与 OCR 设置 -->
          <template v-else-if="activeTab === 'ai'">
            <section class="bg-white dark:bg-slate-900 rounded-2xl border border-gray-100 dark:border-slate-800 shadow-sm p-6 flex flex-col gap-5">
              <h2 class="text-lg font-semibold text-gray-900 dark:text-gray-100">AI 模型配置</h2>
              <p class="text-sm text-gray-500 dark:text-gray-400">配置用于智能录题的文本/视觉大模型 API Key。留空则使用平台默认配置。</p>

              <!-- LLM 服务商 -->
              <div class="flex flex-col gap-1.5">
                <label class="text-sm font-medium text-gray-700 dark:text-gray-300">LLM 服务商</label>
                <select v-model="aiForm.provider" class="ai-input">
                  <option value="deepseek">DeepSeek（推荐）</option>
                  <option value="qwen">通义千问</option>
                  <option value="openai">OpenAI</option>
                </select>
              </div>

              <!-- API Key -->
              <div class="flex flex-col gap-1.5">
                <label class="text-sm font-medium text-gray-700 dark:text-gray-300">
                  API Key
                  <span v-if="aiSettings?.has_api_key" class="text-xs text-green-600 ml-2">已配置</span>
                </label>
                <input v-model="aiForm.apiKey" type="password" class="ai-input" placeholder="输入新 Key 以更新，留空保持不变" />
              </div>

              <!-- 模型名 -->
              <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
                <div class="flex flex-col gap-1.5">
                  <label class="text-sm font-medium text-gray-700 dark:text-gray-300">文本模型（可选）</label>
                  <input v-model="aiForm.modelText" class="ai-input" placeholder="如 deepseek-chat" />
                </div>
                <div class="flex flex-col gap-1.5">
                  <label class="text-sm font-medium text-gray-700 dark:text-gray-300">视觉模型（可选）</label>
                  <input v-model="aiForm.modelVision" class="ai-input" placeholder="如 qwen-vl-plus" />
                </div>
              </div>
            </section>

            <!-- OCR 引擎设置卡片 -->
            <section class="bg-white dark:bg-slate-900 rounded-2xl border border-gray-100 dark:border-slate-800 shadow-sm p-6 flex flex-col gap-5">
              <div class="flex items-center justify-between">
                <h2 class="text-lg font-semibold text-gray-900 dark:text-gray-100">OCR 模型设置</h2>
                <span v-if="aiSettings?.has_doc2x_key" class="text-xs px-2 py-1 rounded-full bg-green-50 text-green-700 dark:bg-green-950 dark:text-green-400">Doc2X 已配置</span>
                <span v-else-if="aiSettings?.has_mineru_key" class="text-xs px-2 py-1 rounded-full bg-blue-50 text-blue-700 dark:bg-blue-950 dark:text-blue-400">MinerU 已配置</span>
              </div>
              <p class="text-sm text-gray-500 dark:text-gray-400">选择图片/PDF 识别引擎。Doc2X 公式精度最高但需独立 Key；MinerU 为开源私有部署方案；Qwen-VL 为通用兜底。</p>

              <!-- 引擎选择 -->
              <div class="flex flex-col gap-1.5">
                <label class="text-sm font-medium text-gray-700 dark:text-gray-300">OCR 引擎</label>
                <select v-model="aiForm.ocrProvider" class="ai-input">
                  <option value="auto">默认自动（跟随系统）</option>
                  <option value="doc2x">Doc2X 极高精公式引擎（推荐）</option>
                  <option value="mineru_local">MinerU 私有部署（开源）</option>
                  <option value="qwen_vl">Qwen-VL 通用（兜底）</option>
                </select>
              </div>

              <!-- Doc2X API Key（仅选中 doc2x 时显示） -->
              <div v-if="aiForm.ocrProvider === 'doc2x'" class="flex flex-col gap-2">
                <label class="text-sm font-medium text-gray-700 dark:text-gray-300">
                  Doc2X API Key
                  <span v-if="aiSettings?.has_doc2x_key" class="text-xs text-green-600 ml-2">已配置</span>
                </label>
                <div class="flex gap-2">
                  <input v-model="aiForm.doc2xApiKey" type="password" class="ai-input flex-1" placeholder="输入 Doc2X API Key（sk-xxx）" />
                  <AppButton variant="ghost" :loading="testingConnection" @click="testOcrConnection">
          <AppIcon name="bolt" :size="16" /> 测试连接
        </AppButton>
                </div>
                <p class="text-xs text-gray-400">未填写个人 Key 时使用平台默认 Key。可在 doc2x.noedgeai.com 获取。</p>
              </div>

              <!-- MinerU 私有部署配置（仅选中 mineru_local 时显示） -->
              <div v-if="aiForm.ocrProvider === 'mineru_local'" class="flex flex-col gap-3">
                <div class="flex flex-col gap-1.5">
                  <label class="text-sm font-medium text-gray-700 dark:text-gray-300">MinerU 服务端点</label>
                  <input v-model="aiForm.mineruEndpoint" class="ai-input" placeholder="如 http://127.0.0.1:8000" />
                  <p class="text-xs text-gray-400">填写本地或内网部署的 MinerU 服务地址（无末尾斜杠）。</p>
                </div>
                <div class="flex flex-col gap-1.5">
                  <label class="text-sm font-medium text-gray-700 dark:text-gray-300">
                    MinerU API Key（可选）
                    <span v-if="aiSettings?.has_mineru_key" class="text-xs text-blue-600 ml-2">已配置</span>
                  </label>
                  <div class="flex gap-2">
                    <input v-model="aiForm.mineruApiKey" type="password" class="ai-input flex-1" placeholder="若服务前置鉴权网关则填写，否则留空" />
                    <AppButton variant="ghost" :loading="testingConnection" @click="testOcrConnection">
                      <AppIcon name="bolt" :size="16" /> 测试连接
                    </AppButton>
                  </div>
                  <p class="text-xs text-gray-400">私有部署默认免鉴权；仅当在 MinerU 前置网关设置了 Bearer Token 时填写。</p>
                </div>
              </div>
            </section>

            <!-- 保存按钮 -->
            <div class="flex justify-end">
              <AppButton variant="primary" :loading="savingAi" @click="saveAiSettings">
                <AppIcon name="check" :size="16" /> 保存设置
              </AppButton>
            </div>
          </template>

        </main>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, reactive, onBeforeUnmount } from 'vue'
import { useRouter } from 'vue-router'
import { userApi, aiApi, type UserProfile, type AiSettings } from '@/api/client'
import { useAuthStore } from '@/stores/auth'
import { useToast } from '@/composables/useToast'
import { useTheme } from '@/composables/useTheme'
import { compressImage } from '@/utils/imageCompressor'
import { AppButton, AppIcon, AppInput, AppBadge, AppProgress } from '@/components/ui'
import ThemeToggle from '@/components/ThemeToggle.vue'

const router = useRouter()
const auth = useAuthStore()
const toast = useToast()
const { isDark } = useTheme()

// ---------------------------------------------------------------------------
// 动态 Tab 控制
// ---------------------------------------------------------------------------
type TabType = 'profile' | 'security' | 'appearance' | 'ai'
const activeTab = ref<TabType>('profile')

const navTabs = [
  { id: 'profile', label: '个人资料', icon: 'user' },
  { id: 'security', label: '安全设置', icon: 'lock' },
  { id: 'appearance', label: '外观与偏好', icon: 'sun' },
  { id: 'ai', label: 'AI 与 OCR', icon: 'sparkles' },
] as const

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
// AI 与 OCR 设置
// ---------------------------------------------------------------------------
const aiSettings = ref<AiSettings | null>(null)
const savingAi = ref(false)
const testingConnection = ref(false)

const aiForm = reactive({
  provider: 'deepseek',
  apiKey: '',           // 明文输入，保存后清空
  modelText: '',
  modelVision: '',
  ocrProvider: 'auto', // auto / doc2x / mineru_local / qwen_vl
  doc2xApiKey: '',     // 明文输入，保存后清空
  mineruEndpoint: '',
  mineruApiKey: '',    // 明文输入，保存后清空
})

async function loadAiSettings() {
  try {
    const res = await aiApi.getSettings()
    aiSettings.value = res.data
    aiForm.provider = res.data.provider
    aiForm.modelText = res.data.model_text || ''
    aiForm.modelVision = res.data.model_vision || ''
    aiForm.ocrProvider = res.data.ocr_provider || 'auto'
    aiForm.mineruEndpoint = res.data.mineru_endpoint || ''
    // apiKey / doc2xApiKey / mineruApiKey 留空（脱敏，不回显明文）
    aiForm.apiKey = ''
    aiForm.doc2xApiKey = ''
    aiForm.mineruApiKey = ''
  } catch (e: any) {
    toast.error(e?.response?.data?.error || '加载 AI 设置失败')
  }
}

async function saveAiSettings() {
  savingAi.value = true
  try {
    const payload: Parameters<typeof aiApi.updateSettings>[0] = {
      provider: aiForm.provider,
      model_text: aiForm.modelText || undefined,
      model_vision: aiForm.modelVision || undefined,
      ocr_provider: aiForm.ocrProvider,
    }
    if (aiForm.apiKey) payload.api_key = aiForm.apiKey
    if (aiForm.doc2xApiKey) payload.doc2x_api_key = aiForm.doc2xApiKey
    if (aiForm.mineruEndpoint) payload.mineru_endpoint = aiForm.mineruEndpoint
    if (aiForm.mineruApiKey) payload.mineru_api_key = aiForm.mineruApiKey
    const res = await aiApi.updateSettings(payload)
    aiSettings.value = res.data
    aiForm.apiKey = ''
    aiForm.doc2xApiKey = ''
    aiForm.mineruApiKey = ''
    toast.success('AI 设置已保存')
  } catch (e: any) {
    toast.error(e?.response?.data?.error || '保存失败')
  } finally {
    savingAi.value = false
  }
}

async function testOcrConnection() {
  testingConnection.value = true
  try {
    const payload: { provider: string; api_key?: string; endpoint?: string } = {
      provider: aiForm.ocrProvider === 'auto' ? 'qwen_vl' : aiForm.ocrProvider,
    }
    // 根据当前选中的引擎填充对应的临时 Key 与 endpoint
    if (aiForm.ocrProvider === 'doc2x' && aiForm.doc2xApiKey) {
      payload.api_key = aiForm.doc2xApiKey
    } else if (aiForm.ocrProvider === 'mineru_local') {
      if (aiForm.mineruApiKey) payload.api_key = aiForm.mineruApiKey
      if (aiForm.mineruEndpoint) payload.endpoint = aiForm.mineruEndpoint
    }
    const res = await aiApi.testOcrConnection(payload)
    const { ok, latency_ms, message } = res.data
    if (ok) {
      toast.success(`连接成功（${latency_ms}ms）：${message}`)
    } else {
      toast.error(`连接失败：${message}`)
    }
  } catch (e: any) {
    toast.error(e?.response?.data?.error || e?.message || '测试连接失败')
  } finally {
    testingConnection.value = false
  }
}

onMounted(loadAiSettings)

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

  avatarState.value = 'idle'
  avatarErrorMsg.value = ''

  if (file.size > MAX_AVATAR_BYTES) {
    avatarState.value = 'error'
    avatarErrorMsg.value = '图片大小不能超过 2MB'
    toast.error('图片大小不能超过 2MB')
    input.value = ''
    return
  }

  const allowedTypes = ['image/jpeg', 'image/png', 'image/webp']
  if (!allowedTypes.includes(file.type)) {
    avatarState.value = 'error'
    avatarErrorMsg.value = '仅支持 JPG / PNG / WebP 格式'
    toast.error('仅支持 JPG / PNG / WebP 格式')
    input.value = ''
    return
  }

  avatarState.value = 'uploading'
  try {
    const compressedBlob = await compressImage(file)
    const mimeType = compressedBlob.type || 'image/webp'
    const compressedFile = new File([compressedBlob], file.name || 'avatar.webp', {
      type: mimeType,
    })

    const newAvatarUrl = await auth.uploadAvatar(compressedFile)
    avatarPreviewSrc.value = newAvatarUrl
    avatarState.value = 'idle'
    toast.success('头像更新成功')
  } catch (e: any) {
    avatarState.value = 'error'
    avatarErrorMsg.value = e?.response?.data?.error || '头像上传失败'
    toast.error(e?.response?.data?.error || '头像上传失败')
  } finally {
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

    pwForm.oldPassword = ''
    pwForm.newPassword = ''
    pwForm.confirmPassword = ''

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
  if (profile.value.global_role === 'super_admin') return '系统管理员'
  return '教师'
})

const roleBadgeColor = computed(() => {
  if (!profile.value) return 'gray'
  if (profile.value.global_role === 'super_admin') return 'purple'
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

onMounted(() => {
  window.addEventListener('beforeunload', () => {
    pwForm.oldPassword = ''
    pwForm.newPassword = ''
    pwForm.confirmPassword = ''
  })
})

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

/* ===== 独立滚动域 ===== */
.profile-scroll-area {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
  background: var(--bg-primary);
}

/* ===== AI 与 OCR 设置表单输入（M3 新增） ===== */
.ai-input {
  width: 100%;
  padding: 8px 12px;
  border: 1px solid var(--border, #e5e7eb);
  border-radius: 8px;
  font-size: 14px;
  background: var(--bg-input, #fff);
  color: var(--text-primary, #111827);
  transition: border-color 0.2s;
}
.ai-input:focus {
  outline: none;
  border-color: var(--purple, #7c3aed);
}
.dark .ai-input {
  background: #0f172a;
  border-color: #1e293b;
  color: #e2e8f0;
}
</style>
