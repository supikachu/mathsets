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
            <div class="ai-settings">
              <!-- 解析 -->
              <section class="ai-block">
                <div class="ai-section-head">
                  <div class="ai-section-icon ai-section-icon--parse">
                    <AppIcon name="sparkles" :size="15" />
                  </div>
                  <div class="ai-section-copy">
                    <h2>解析</h2>
                    <p>试卷分类与 OCR 后拆题</p>
                  </div>
                </div>

                <div class="ios-group">
                  <div class="ios-row">
                    <span class="ios-label">服务商</span>
                    <select v-model="aiForm.provider" class="ios-select" aria-label="解析服务商">
                      <option v-for="opt in llmProviderOptions" :key="opt.value" :value="opt.value">{{ opt.label }}</option>
                    </select>
                  </div>

                  <div v-if="isCustomLlm" class="ios-row ios-row--stack">
                    <span class="ios-label">API 地址</span>
                    <input
                      v-model="aiForm.llmBaseUrl"
                      class="ios-input"
                      placeholder="https://openrouter.ai/api/v1"
                      spellcheck="false"
                      autocomplete="off"
                    />
                  </div>

                  <div class="ios-row ios-row--stack">
                    <div class="ios-row-top">
                      <span class="ios-label">API Key</span>
                      <span v-if="aiSettings?.has_api_key" class="ios-status">已配置</span>
                    </div>
                    <div class="ios-inline">
                      <input
                        v-model="aiForm.apiKey"
                        type="password"
                        class="ios-input"
                        placeholder="输入新 Key，留空保持不变"
                        autocomplete="off"
                      />
                      <button
                        v-if="isCustomLlm"
                        type="button"
                        class="ios-text-btn"
                        :disabled="testingLlm"
                        @click="testLlmConnection"
                      >{{ testingLlm ? '测试中…' : '测试' }}</button>
                    </div>
                  </div>

                  <div class="ios-row ios-row--stack">
                    <span class="ios-label">文本模型{{ isCustomLlm ? '' : '（可选）' }}</span>
                    <input
                      v-model="aiForm.modelText"
                      class="ios-input ios-input--mono"
                      :placeholder="textModelPlaceholder"
                      spellcheck="false"
                    />
                  </div>

                  <div class="ios-row ios-row--stack">
                    <span class="ios-label">视觉模型（可选）</span>
                    <input
                      v-model="aiForm.modelVision"
                      class="ios-input ios-input--mono"
                      placeholder="如 qwen-vl-plus"
                      spellcheck="false"
                    />
                  </div>

                  <div class="ios-row">
                    <div class="ios-label-block">
                      <span class="ios-label">拆题并发</span>
                    </div>
                    <div class="ai-stepper">
                      <button type="button" :disabled="aiForm.stage2Concurrency <= 1" @click="bumpConcurrency('stage2Concurrency', -1)">−</button>
                      <span>{{ aiForm.stage2Concurrency }}</span>
                      <button type="button" :disabled="aiForm.stage2Concurrency >= 16" @click="bumpConcurrency('stage2Concurrency', 1)">+</button>
                    </div>
                  </div>
                </div>
                <p class="ios-footer">
                  <template v-if="aiForm.provider === 'gemini'">
                    免费档按 AI Studio 额度：Flash 约 5 次/分钟、20 次/天。并发建议设为 1。详见
                    <a href="https://aistudio.google.com/rate-limit" target="_blank" rel="noreferrer">Rate Limit</a>。
                  </template>
                  <template v-else-if="isCustomLlm">
                    填写 OpenRouter 完整模型 ID，例如 stealth/ox-alpha。列表见
                    <a href="https://openrouter.ai/models" target="_blank" rel="noreferrer">OpenRouter Models</a>。
                    免费档建议并发为 1。
                  </template>
                  <template v-else>
                    同时解析的切块数，范围 1–16。智谱 / Gemini 建议设为 1，避免限流。
                  </template>
                </p>
              </section>

              <!-- 智能打标 -->
              <section class="ai-block">
                <div class="ai-section-head">
                  <div class="ai-section-icon ai-section-icon--tag">
                    <AppIcon name="tag" :size="15" />
                  </div>
                  <div class="ai-section-copy">
                    <h2>智能打标</h2>
                    <p>分类任务，适合便宜的快模型</p>
                  </div>
                </div>

                <div class="ios-group">
                  <div class="ios-row ios-row--toggle" @click="aiForm.taggingIndependent = !aiForm.taggingIndependent">
                    <div class="ios-label-block">
                      <span class="ios-label">独立服务商</span>
                      <span class="ios-sub">解析与打标可使用不同厂商</span>
                    </div>
                    <AppToggle v-model="aiForm.taggingIndependent" @click.stop />
                  </div>

                  <template v-if="aiForm.taggingIndependent">
                    <div class="ios-row">
                      <span class="ios-label">服务商</span>
                      <select v-model="aiForm.taggingProvider" class="ios-select" aria-label="打标服务商">
                        <option v-for="opt in llmProviderOptions" :key="opt.value" :value="opt.value">{{ opt.label }}</option>
                      </select>
                    </div>

                    <div v-if="isCustomTagging" class="ios-row ios-row--stack">
                      <span class="ios-label">API 地址</span>
                      <input
                        v-model="aiForm.taggingLlmBaseUrl"
                        class="ios-input"
                        placeholder="https://openrouter.ai/api/v1"
                        spellcheck="false"
                        autocomplete="off"
                      />
                    </div>

                    <div class="ios-row ios-row--stack">
                      <div class="ios-row-top">
                        <span class="ios-label">API Key</span>
                        <span v-if="aiSettings?.has_tagging_api_key" class="ios-status">已配置</span>
                        <span v-else-if="!isCustomTagging" class="ios-hint">可留空，使用平台默认</span>
                      </div>
                      <input
                        v-model="aiForm.taggingApiKey"
                        type="password"
                        class="ios-input"
                        placeholder="输入新 Key，留空保持不变"
                        autocomplete="off"
                      />
                    </div>
                  </template>

                  <div class="ios-row ios-row--stack">
                    <span class="ios-label">打标模型（可选）</span>
                    <input
                      v-model="aiForm.modelTagging"
                      class="ios-input ios-input--mono"
                      :placeholder="taggingModelPlaceholder"
                      spellcheck="false"
                    />
                  </div>

                  <div class="ios-row">
                    <div class="ios-label-block">
                      <span class="ios-label">打标并发</span>
                    </div>
                    <div class="ai-stepper">
                      <button type="button" :disabled="aiForm.taggingConcurrency <= 1" @click="bumpConcurrency('taggingConcurrency', -1)">−</button>
                      <span>{{ aiForm.taggingConcurrency }}</span>
                      <button type="button" :disabled="aiForm.taggingConcurrency >= 16" @click="bumpConcurrency('taggingConcurrency', 1)">+</button>
                    </div>
                  </div>
                </div>
                <p class="ios-footer">
                  {{ aiForm.taggingIndependent
                    ? '例如解析走 OpenRouter，打标走官方 DeepSeek。'
                    : '关闭时沿用上方解析服务商与 Key。模型可单独指定。' }}
                  并发为同一账号同时打标的题目数（1–16）。
                </p>
              </section>

              <!-- OCR -->
              <section class="ai-block">
                <div class="ai-section-head">
                  <div class="ai-section-icon ai-section-icon--ocr">
                    <AppIcon name="image" :size="15" />
                  </div>
                  <div class="ai-section-copy">
                    <h2>OCR</h2>
                    <p>图片与 PDF 识别引擎</p>
                  </div>
                  <span v-if="aiSettings?.has_doc2x_key" class="ios-status">Doc2X 已配置</span>
                  <span v-else-if="aiSettings?.has_mineru_key" class="ios-status ios-status--accent">MinerU 已配置</span>
                </div>

                <div class="ios-group">
                  <div class="ios-row">
                    <span class="ios-label">引擎</span>
                    <select v-model="aiForm.ocrProvider" class="ios-select" aria-label="OCR 引擎">
                      <option value="auto">自动（跟随系统）</option>
                      <option value="doc2x">Doc2X 公式引擎</option>
                      <option value="mineru_local">MinerU 私有部署</option>
                      <option value="qwen_vl">Qwen-VL 通用</option>
                    </select>
                  </div>

                  <template v-if="aiForm.ocrProvider === 'doc2x'">
                    <div class="ios-row ios-row--stack">
                      <div class="ios-row-top">
                        <span class="ios-label">Doc2X API Key</span>
                        <span v-if="aiSettings?.has_doc2x_key" class="ios-status">已配置</span>
                      </div>
                      <div class="ios-inline">
                        <input
                          v-model="aiForm.doc2xApiKey"
                          type="password"
                          class="ios-input"
                          placeholder="sk-xxx，留空使用平台默认"
                          autocomplete="off"
                        />
                        <button type="button" class="ios-text-btn" :disabled="testingConnection" @click="testOcrConnection">
                          {{ testingConnection ? '测试中…' : '测试' }}
                        </button>
                      </div>
                    </div>
                  </template>

                  <template v-else-if="aiForm.ocrProvider === 'mineru_local'">
                    <div class="ios-row ios-row--stack">
                      <span class="ios-label">服务端点</span>
                      <input
                        v-model="aiForm.mineruEndpoint"
                        class="ios-input"
                        placeholder="http://127.0.0.1:8000"
                        spellcheck="false"
                      />
                    </div>
                    <div class="ios-row ios-row--stack">
                      <div class="ios-row-top">
                        <span class="ios-label">API Key（可选）</span>
                        <span v-if="aiSettings?.has_mineru_key" class="ios-status ios-status--accent">已配置</span>
                      </div>
                      <div class="ios-inline">
                        <input
                          v-model="aiForm.mineruApiKey"
                          type="password"
                          class="ios-input"
                          placeholder="仅网关鉴权时填写"
                          autocomplete="off"
                        />
                        <button type="button" class="ios-text-btn" :disabled="testingConnection" @click="testOcrConnection">
                          {{ testingConnection ? '测试中…' : '测试' }}
                        </button>
                      </div>
                    </div>
                  </template>
                </div>
                <p class="ios-footer">
                  <template v-if="aiForm.ocrProvider === 'doc2x'">
                    公式精度最高。未填个人 Key 时使用平台默认。可在 doc2x.noedgeai.com 获取。
                  </template>
                  <template v-else-if="aiForm.ocrProvider === 'mineru_local'">
                    填写本地或内网 MinerU 地址，不要带末尾斜杠。私有部署默认免鉴权。
                  </template>
                  <template v-else>
                    Doc2X 公式最准；MinerU 适合私有化；Qwen-VL 为通用兜底。
                  </template>
                </p>
              </section>

              <div class="ai-save">
                <AppButton variant="primary" :loading="savingAi" @click="saveAiSettings">
                  保存设置
                </AppButton>
              </div>
            </div>
          </template>

        </main>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, reactive, onBeforeUnmount, watch } from 'vue'
import { useRouter } from 'vue-router'
import { userApi, aiApi, type UserProfile, type AiSettings } from '@/api/client'
import { useAuthStore } from '@/stores/auth'
import { useToast } from '@/composables/useToast'
import { useTheme } from '@/composables/useTheme'
import { compressImage } from '@/utils/imageCompressor'
import { AppButton, AppIcon, AppInput, AppBadge, AppProgress, AppToggle } from '@/components/ui'
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
const testingLlm = ref(false)

const DEFAULT_OPENROUTER_URL = 'https://openrouter.ai/api/v1'

const aiForm = reactive({
  provider: 'deepseek',
  apiKey: '',           // 明文输入，保存后清空
  modelText: '',
  modelVision: '',
  modelTagging: '',
  llmBaseUrl: '',
  ocrProvider: 'auto', // auto / doc2x / mineru_local / qwen_vl
  doc2xApiKey: '',     // 明文输入，保存后清空
  mineruEndpoint: '',
  mineruApiKey: '',    // 明文输入，保存后清空
  taggingIndependent: false,
  taggingProvider: 'deepseek',
  taggingApiKey: '',
  taggingLlmBaseUrl: '',
  stage2Concurrency: 4,
  taggingConcurrency: 4,
})

const llmProviderOptions = [
  { value: 'deepseek', label: 'DeepSeek' },
  { value: 'qwen', label: '通义千问' },
  { value: 'glm', label: '智谱 GLM' },
  { value: 'gemini', label: 'Google Gemini' },
  { value: 'openai', label: 'OpenAI' },
  { value: 'custom', label: '自定义 / OpenRouter' },
] as const

const TEXT_MODEL_DEFAULTS: Record<string, string> = {
  deepseek: 'deepseek-chat',
  qwen: 'qwen-plus',
  glm: 'glm-4-flash',
  gemini: 'gemini-3.7-flash',
  openai: 'gpt-4o-mini',
  custom: 'stealth/ox-alpha',
}

const isCustomLlm = computed(() => aiForm.provider === 'custom' || aiForm.provider === 'openrouter')
const isCustomTagging = computed(
  () => aiForm.taggingProvider === 'custom' || aiForm.taggingProvider === 'openrouter',
)

const textModelPlaceholder = computed(() => {
  const name = TEXT_MODEL_DEFAULTS[aiForm.provider] || 'deepseek-chat'
  return `如 ${name}`
})

const taggingModelPlaceholder = computed(() => {
  if (!aiForm.taggingIndependent) {
    return aiForm.modelText ? `留空则用 ${aiForm.modelText}` : '留空则沿用解析文本模型'
  }
  const name = TEXT_MODEL_DEFAULTS[aiForm.taggingProvider] || 'deepseek-chat'
  return `如 ${name}`
})

function clampConcurrency(n: unknown, fallback = 4) {
  const v = Math.round(Number(n))
  if (!Number.isFinite(v)) return fallback
  return Math.min(16, Math.max(1, v))
}

function bumpConcurrency(field: 'stage2Concurrency' | 'taggingConcurrency', delta: number) {
  aiForm[field] = clampConcurrency((Number(aiForm[field]) || 4) + delta)
}

watch(() => aiForm.provider, (next, prev) => {
  if (next === prev) return
  const known = Object.values(TEXT_MODEL_DEFAULTS)
  if (!aiForm.modelText || known.includes(aiForm.modelText)) {
    aiForm.modelText = TEXT_MODEL_DEFAULTS[next] || ''
  }
  if ((next === 'custom' || next === 'openrouter') && !aiForm.llmBaseUrl) {
    aiForm.llmBaseUrl = DEFAULT_OPENROUTER_URL
  }
})

watch(() => aiForm.taggingIndependent, (on, wasOn) => {
  if (on === wasOn) return
  if (on && !aiForm.taggingProvider) {
    aiForm.taggingProvider = aiForm.provider === 'custom' || aiForm.provider === 'openrouter'
      ? 'deepseek'
      : aiForm.provider
  }
  if (on && isCustomTagging.value && !aiForm.taggingLlmBaseUrl) {
    aiForm.taggingLlmBaseUrl = DEFAULT_OPENROUTER_URL
  }
})

watch(() => aiForm.taggingProvider, (next, prev) => {
  if (next === prev) return
  const prevDefault = TEXT_MODEL_DEFAULTS[prev] || ''
  if (!aiForm.modelTagging || aiForm.modelTagging === prevDefault) {
    aiForm.modelTagging = TEXT_MODEL_DEFAULTS[next] || ''
  }
  if ((next === 'custom' || next === 'openrouter') && !aiForm.taggingLlmBaseUrl) {
    aiForm.taggingLlmBaseUrl = DEFAULT_OPENROUTER_URL
  }
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
    aiForm.llmBaseUrl = res.data.llm_base_url || ''
    aiForm.taggingIndependent = !!res.data.tagging_provider
    aiForm.taggingProvider = res.data.tagging_provider || 'deepseek'
    aiForm.taggingLlmBaseUrl = res.data.tagging_llm_base_url || ''
    aiForm.modelTagging = res.data.model_tagging || ''
    aiForm.stage2Concurrency = clampConcurrency(res.data.stage2_concurrency, 4)
    aiForm.taggingConcurrency = clampConcurrency(res.data.tagging_concurrency, 4)
    aiForm.apiKey = ''
    aiForm.doc2xApiKey = ''
    aiForm.mineruApiKey = ''
    aiForm.taggingApiKey = ''
  } catch (e: any) {
    toast.error(e?.response?.data?.error || '加载 AI 设置失败')
  }
}

async function saveAiSettings() {
  if (isCustomLlm.value) {
    if (!aiForm.apiKey && !aiSettings.value?.has_api_key) {
      toast.error('自定义服务商需要填写 API Key')
      return
    }
    if (!aiForm.modelText.trim()) {
      toast.error('请填写模型 ID（如 stealth/ox-alpha）')
      return
    }
  }
  if (aiForm.taggingIndependent && isCustomTagging.value) {
    const parseProv = aiForm.provider === 'openrouter' ? 'custom' : aiForm.provider
    const tagProv = aiForm.taggingProvider === 'openrouter' ? 'custom' : aiForm.taggingProvider
    const canInheritParseKey =
      parseProv === 'custom' &&
      tagProv === 'custom' &&
      (!!aiForm.apiKey || !!aiSettings.value?.has_api_key)
    if (!aiForm.taggingApiKey && !aiSettings.value?.has_tagging_api_key && !canInheritParseKey) {
      toast.error('打标自定义服务商需要填写 API Key')
      return
    }
  }
  savingAi.value = true
  try {
    const payload: Parameters<typeof aiApi.updateSettings>[0] = {
      provider: aiForm.provider === 'openrouter' ? 'custom' : aiForm.provider,
      model_text: aiForm.modelText || TEXT_MODEL_DEFAULTS[aiForm.provider],
      model_vision: aiForm.modelVision || undefined,
      model_tagging: aiForm.modelTagging.trim() || undefined,
      ocr_provider: aiForm.ocrProvider,
      tagging_provider: aiForm.taggingIndependent
        ? (aiForm.taggingProvider === 'openrouter' ? 'custom' : aiForm.taggingProvider)
        : '',
      stage2_concurrency: clampConcurrency(aiForm.stage2Concurrency, 4),
      tagging_concurrency: clampConcurrency(aiForm.taggingConcurrency, 4),
    }
    if (isCustomLlm.value) {
      payload.llm_base_url = aiForm.llmBaseUrl.trim() || DEFAULT_OPENROUTER_URL
    }
    if (aiForm.taggingIndependent && isCustomTagging.value) {
      payload.tagging_llm_base_url = aiForm.taggingLlmBaseUrl.trim() || DEFAULT_OPENROUTER_URL
    }
    if (aiForm.apiKey) payload.api_key = aiForm.apiKey
    if (aiForm.taggingApiKey) payload.tagging_api_key = aiForm.taggingApiKey
    if (aiForm.doc2xApiKey) payload.doc2x_api_key = aiForm.doc2xApiKey
    if (aiForm.mineruEndpoint) payload.mineru_endpoint = aiForm.mineruEndpoint
    if (aiForm.mineruApiKey) payload.mineru_api_key = aiForm.mineruApiKey
    const res = await aiApi.updateSettings(payload)
    aiSettings.value = res.data
    aiForm.apiKey = ''
    aiForm.doc2xApiKey = ''
    aiForm.mineruApiKey = ''
    aiForm.taggingApiKey = ''
    if (res.data.llm_base_url) aiForm.llmBaseUrl = res.data.llm_base_url
    if (res.data.tagging_llm_base_url) aiForm.taggingLlmBaseUrl = res.data.tagging_llm_base_url
    aiForm.taggingIndependent = !!res.data.tagging_provider
    aiForm.taggingProvider = res.data.tagging_provider || aiForm.taggingProvider
    aiForm.stage2Concurrency = clampConcurrency(res.data.stage2_concurrency, 4)
    aiForm.taggingConcurrency = clampConcurrency(res.data.tagging_concurrency, 4)
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

async function testLlmConnection() {
  testingLlm.value = true
  try {
    const payload: { api_key?: string; endpoint?: string } = {}
    if (aiForm.apiKey) payload.api_key = aiForm.apiKey
    if (aiForm.llmBaseUrl.trim()) payload.endpoint = aiForm.llmBaseUrl.trim()
    const res = await aiApi.testLlmConnection(payload)
    const { ok, latency_ms, message } = res.data
    if (ok) {
      toast.success(`连接成功（${latency_ms}ms）：${message}`)
    } else {
      toast.error(`连接失败：${message}`)
    }
  } catch (e: any) {
    toast.error(e?.response?.data?.error || e?.message || '测试连接失败')
  } finally {
    testingLlm.value = false
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

/* ===== AI 与 OCR：iOS Settings 分组列表 ===== */
.ai-settings {
  display: flex;
  flex-direction: column;
  gap: 28px;
  padding-bottom: 32px;
  font-family: -apple-system, BlinkMacSystemFont, 'SF Pro Text', 'Segoe UI', sans-serif;
}

.ai-block {
  display: flex;
  flex-direction: column;
}

.ai-section-head {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 0 4px 10px;
}

.ai-section-icon {
  width: 29px;
  height: 29px;
  border-radius: 7px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: #fff;
  flex-shrink: 0;
}

.ai-section-icon--parse { background: var(--accent); }
.ai-section-icon--tag { background: var(--warning); }
.ai-section-icon--ocr { background: var(--teal); }

.ai-section-copy {
  flex: 1;
  min-width: 0;
}

.ai-section-copy h2 {
  margin: 0;
  font-size: 20px;
  font-weight: 700;
  letter-spacing: -0.022em;
  color: var(--text-primary);
  line-height: 1.2;
}

.ai-section-copy p {
  margin: 2px 0 0;
  font-size: 13px;
  color: var(--text-secondary);
  line-height: 1.35;
}

.ios-group {
  background: var(--bg-card);
  border-radius: var(--radius-md);
  overflow: hidden;
  box-shadow: var(--shadow-xs);
  border: 0.5px solid var(--border-color);
}

.ios-row {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  min-height: 44px;
  padding: 10px 16px;
}

.ios-row:not(:last-child)::after {
  content: '';
  position: absolute;
  left: 16px;
  right: 0;
  bottom: 0;
  height: 0.5px;
  background: var(--divider);
}

.ios-row--stack {
  flex-direction: column;
  align-items: stretch;
  gap: 6px;
  padding-top: 10px;
  padding-bottom: 12px;
}

.ios-row--toggle {
  min-height: 52px;
  cursor: pointer;
}

.ios-row:active {
  background: var(--bg-hover);
}

.ios-row-top {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.ios-label {
  font-size: 17px;
  font-weight: 400;
  color: var(--text-primary);
  letter-spacing: -0.022em;
  line-height: 1.25;
  flex-shrink: 0;
}

.ios-label-block {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.ios-sub,
.ios-hint {
  font-size: 13px;
  color: var(--text-secondary);
  line-height: 1.3;
}

.ios-status {
  font-size: 13px;
  font-weight: 500;
  color: var(--success);
  letter-spacing: -0.01em;
  white-space: nowrap;
}

.ios-status--accent {
  color: var(--accent);
}

.ios-select {
  appearance: none;
  -webkit-appearance: none;
  -moz-appearance: none;
  color-scheme: inherit;
  flex: 1;
  width: auto;
  min-width: 0;
  max-width: none;
  border: none;
  border-radius: 0;
  background-color: transparent;
  background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%2386868b' stroke-width='2.4' stroke-linecap='round' stroke-linejoin='round'><path d='m9 18 6-6-6-6'/></svg>");
  background-repeat: no-repeat;
  background-position: right 0 center;
  padding: 0 18px 0 8px;
  margin: 0;
  font-size: 17px;
  font-family: inherit;
  color: var(--text-secondary);
  text-align: right;
  cursor: pointer;
  line-height: 1.3;
  box-shadow: none;
}

.ios-input {
  width: 100%;
  border: none;
  border-radius: 0;
  background: transparent;
  box-shadow: none;
  padding: 0;
  margin: 0;
  font-size: 17px;
  font-family: inherit;
  color: var(--text-primary);
  letter-spacing: -0.022em;
  line-height: 1.35;
}

.ios-input--mono {
  font-family: var(--font-mono);
  font-size: 15px;
  letter-spacing: -0.01em;
}

.ios-input::placeholder {
  color: var(--text-muted);
}

.ios-input:focus {
  outline: none;
  border: none;
  box-shadow: none;
  background: transparent;
}

.ios-select:focus {
  outline: none;
  border: none;
  box-shadow: none;
  background-color: transparent;
  background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%230071e3' stroke-width='2.4' stroke-linecap='round' stroke-linejoin='round'><path d='m9 18 6-6-6-6'/></svg>");
  background-repeat: no-repeat;
  background-position: right 0 center;
  color: var(--accent);
}

.ios-inline {
  display: flex;
  align-items: center;
  gap: 10px;
}

.ios-text-btn {
  flex-shrink: 0;
  border: none;
  background: none;
  padding: 0;
  font-size: 15px;
  font-weight: 500;
  font-family: inherit;
  color: var(--accent);
  cursor: pointer;
}

.ios-text-btn:hover {
  color: var(--accent-hover);
}

.ios-text-btn:disabled {
  opacity: 0.45;
  cursor: default;
}

.ios-footer {
  margin: 8px 16px 0;
  font-size: 13px;
  line-height: 1.4;
  color: var(--text-secondary);
}

.ios-footer a {
  color: var(--accent);
  text-decoration: none;
}

.ios-footer a:hover {
  text-decoration: underline;
}

.ai-stepper {
  display: inline-flex;
  align-items: center;
  background: var(--bg-input);
  border-radius: 9px;
  overflow: hidden;
}

.ai-stepper button {
  width: 32px;
  height: 32px;
  border: none;
  background: transparent;
  color: var(--accent);
  font-size: 18px;
  font-weight: 500;
  font-family: inherit;
  line-height: 1;
  cursor: pointer;
  flex-shrink: 0;
}

.ai-stepper button:hover:not(:disabled) {
  background: var(--bg-hover);
}

.ai-stepper button:disabled {
  color: var(--text-muted);
  cursor: default;
}

.ai-stepper span {
  min-width: 28px;
  text-align: center;
  font-size: 17px;
  font-variant-numeric: tabular-nums;
  color: var(--text-primary);
}

.ai-save {
  display: flex;
  justify-content: flex-end;
  padding: 4px 0 8px;
}

.ai-save :deep(.btn) {
  min-width: 120px;
  min-height: 44px;
  border-radius: 12px;
  font-size: 17px;
  font-weight: 600;
  letter-spacing: -0.022em;
}
</style>
