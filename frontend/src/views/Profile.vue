<template>
  <div class="profile-page">
    <!-- ===== 吸顶顶栏 ===== -->
    <header class="profile-sticky-bar">
      <div class="profile-header">
        <button class="back-btn" @click="$router.back()" aria-label="返回">
          <AppIcon name="chevron-left" :size="18" />
        </button>
        <h1 class="page-title">个人中心与设置</h1>
      </div>
    </header>

    <!-- ===== 主体双栏内容区域 (Apple Split View) ===== -->
    <div class="profile-scroll-area">
      <div class="profile-layout-container">
        
        <!-- 左侧固定宽度设置侧边栏 (240px) -->
        <aside class="profile-sidebar">
          <div class="sidebar-section-title">
            设置分类
          </div>
          
          <nav class="sidebar-nav" aria-label="设置导航">
            <button
              v-for="tab in navTabs"
              :key="tab.id"
              type="button"
              class="sidebar-tab-item"
              :class="{ 'is-active': activeTab === tab.id }"
              @click="activeTab = tab.id"
            >
              <AppIcon :name="tab.icon" :size="17" class="sidebar-tab-icon" />
              <span class="sidebar-tab-label">{{ tab.label }}</span>
            </button>
          </nav>
        </aside>

        <!-- 右侧主内容区 (居中限制 680px 呼吸感对齐) -->
        <main class="profile-content-column">
          
          <!-- ============================================================ -->
          <!-- TAB 1: 个人资料                                              -->
          <!-- ============================================================ -->
          <template v-if="activeTab === 'profile'">
            <!-- 1.1 顶端头像与身份卡片 -->
            <section class="apple-inset-card p-6 flex flex-col sm:flex-row items-center justify-between gap-6">
              <div class="flex items-center gap-5 min-w-0 w-full sm:w-auto">
                <div class="relative w-20 h-20 rounded-full overflow-hidden shrink-0 border-2 border-[var(--border-color)] shadow-xs bg-[var(--bg-input)]">
                  <img
                    v-if="avatarPreviewSrc"
                    :src="avatarPreviewSrc"
                    class="w-full h-full object-cover"
                    alt="头像预览"
                  />
                  <div v-else class="w-full h-full bg-gradient-to-br from-[#007AFF] to-[#5856D6] text-white flex items-center justify-center text-2xl font-semibold">
                    {{ avatarLetter }}
                  </div>
                  <!-- 上传中遮罩 -->
                  <div v-if="avatarState === 'uploading'" class="absolute inset-0 bg-black/50 backdrop-blur-xs flex items-center justify-center text-white">
                    <AppProgress statusText="" :size="20" />
                  </div>
                </div>

                <div class="flex flex-col min-w-0 gap-1">
                  <div class="flex items-center gap-2.5 flex-wrap">
                    <h2 class="text-lg font-semibold text-[var(--text-primary)] truncate">{{ auth.displayName || '未命名用户' }}</h2>
                    <span class="apple-badge-role" :class="roleBadgeClass">{{ roleLabel }}</span>
                  </div>
                  <p class="text-[13px] text-[var(--text-muted)] truncate">{{ profile?.email || '暂无邮箱' }}</p>
                </div>
              </div>

              <!-- 上传/更换头像按钮 -->
              <div class="flex flex-col items-center sm:items-end gap-1.5 shrink-0 w-full sm:w-auto border-t sm:border-t-0 border-[var(--border-color)] pt-4 sm:pt-0">
                <input
                  ref="fileInputRef"
                  type="file"
                  accept="image/jpeg,image/png,image/webp"
                  class="hidden"
                  @change="onFileSelected"
                />
                <button
                  type="button"
                  class="apple-secondary-btn"
                  :disabled="avatarState === 'uploading'"
                  @click="fileInputRef?.click()"
                >
                  <AppIcon name="upload" :size="14" class="mr-1.5" />
                  <span>{{ avatarState === 'uploading' ? '上传中…' : '更换头像' }}</span>
                </button>
                <span class="text-[12px] text-[var(--text-muted)]">支持 JPG/PNG/WebP，最大 2MB</span>
                <span v-if="avatarState === 'error'" class="text-[12px] text-[var(--danger)]">{{ avatarErrorMsg }}</span>
              </div>
            </section>

            <!-- 1.2 基础信息卡片 -->
            <section class="apple-block">
              <div class="apple-section-head">
                <div class="apple-section-icon bg-[#007AFF]">
                  <AppIcon name="user" :size="15" />
                </div>
                <div class="apple-section-copy">
                  <h2>基础资料</h2>
                  <p>查看与修改您的公开个人信息</p>
                </div>
              </div>

              <div class="apple-inset-card">
                <div class="apple-list-row">
                  <span class="apple-row-label">昵称</span>
                  <div class="apple-input-wrap">
                    <input
                      v-model="form.displayName"
                      class="apple-control-input"
                      placeholder="请输入昵称"
                    />
                  </div>
                </div>
                <div class="apple-list-row">
                  <span class="apple-row-label">邮箱</span>
                  <div class="apple-input-wrap">
                    <input
                      v-model="form.email"
                      type="email"
                      class="apple-control-input"
                      placeholder="user@example.com"
                    />
                  </div>
                </div>
                <div class="apple-list-row">
                  <span class="apple-row-label">账号用户名</span>
                  <span class="text-[14px] text-[var(--text-muted)] font-mono">{{ profile?.username || '—' }}</span>
                </div>
                <div class="apple-list-row">
                  <span class="apple-row-label">注册时间</span>
                  <span class="text-[14px] text-[var(--text-muted)]">{{ profile ? formatTime(profile.created_at) : '—' }}</span>
                </div>
                <div class="apple-list-row">
                  <span class="apple-row-label">用户身份</span>
                  <span class="apple-badge-role" :class="roleBadgeClass">{{ roleLabel }}</span>
                </div>
              </div>

              <div class="flex justify-end pt-3">
                <button
                  type="button"
                  class="apple-save-btn"
                  :disabled="!hasProfileChanges || savingProfile"
                  @click="onSaveProfile"
                >
                  <AppProgress v-if="savingProfile" statusText="" :size="15" class="mr-1.5 text-white" />
                  <span>{{ savingProfile ? '保存中…' : '保存修改' }}</span>
                </button>
              </div>
            </section>
          </template>

          <!-- ============================================================ -->
          <!-- TAB 2: 安全设置 (修改密码)                                   -->
          <!-- ============================================================ -->
          <template v-else-if="activeTab === 'security'">
            <section class="apple-block">
              <div class="apple-section-head">
                <div class="apple-section-icon bg-[#5856D6]">
                  <AppIcon name="lock" :size="15" />
                </div>
                <div class="apple-section-copy">
                  <h2>登录与安全</h2>
                  <p>定期更新密码以保障您的账号安全</p>
                </div>
              </div>

              <div class="apple-inset-card">
                <div class="apple-list-row">
                  <span class="apple-row-label">当前原密码</span>
                  <div class="apple-input-wrap">
                    <input
                      v-model="pwForm.oldPassword"
                      :type="showOldPassword ? 'text' : 'password'"
                      class="apple-control-input"
                      :class="{ 'font-mono text-[13px]': showOldPassword, 'apple-password-input': !showOldPassword }"
                      placeholder="请输入当前密码"
                      autocomplete="current-password"
                    />
                    <button
                      type="button"
                      class="apple-eye-btn"
                      :aria-label="showOldPassword ? '隐藏密码' : '显示密码'"
                      @click="showOldPassword = !showOldPassword"
                    >
                      <AppIcon :name="showOldPassword ? 'eye-off' : 'eye'" :size="15" />
                    </button>
                  </div>
                </div>
                <div class="apple-list-row">
                  <span class="apple-row-label">新设密码</span>
                  <div class="apple-input-wrap">
                    <input
                      v-model="pwForm.newPassword"
                      :type="showNewPassword ? 'text' : 'password'"
                      class="apple-control-input"
                      :class="{ 'font-mono text-[13px]': showNewPassword, 'apple-password-input': !showNewPassword }"
                      placeholder="至少 8 位字符"
                      autocomplete="new-password"
                    />
                    <button
                      type="button"
                      class="apple-eye-btn"
                      :aria-label="showNewPassword ? '隐藏密码' : '显示密码'"
                      @click="showNewPassword = !showNewPassword"
                    >
                      <AppIcon :name="showNewPassword ? 'eye-off' : 'eye'" :size="15" />
                    </button>
                  </div>
                </div>
                <div class="apple-list-row">
                  <span class="apple-row-label">确认新密码</span>
                  <div class="apple-input-wrap">
                    <input
                      v-model="pwForm.confirmPassword"
                      :type="showConfirmPassword ? 'text' : 'password'"
                      class="apple-control-input"
                      :class="{ 'font-mono text-[13px]': showConfirmPassword, 'apple-password-input': !showConfirmPassword }"
                      placeholder="再次输入新密码"
                      autocomplete="new-password"
                    />
                    <button
                      type="button"
                      class="apple-eye-btn"
                      :aria-label="showConfirmPassword ? '隐藏密码' : '显示密码'"
                      @click="showConfirmPassword = !showConfirmPassword"
                    >
                      <AppIcon :name="showConfirmPassword ? 'eye-off' : 'eye'" :size="15" />
                    </button>
                  </div>
                </div>
              </div>
              <p class="apple-card-footer">
                修改成功后将自动注销当前登录状态，请使用新设置的密码重新登录。
              </p>

              <div class="flex justify-end pt-3">
                <button
                  type="button"
                  class="apple-save-btn"
                  :disabled="!canSubmitPassword || changingPassword"
                  @click="onChangePassword"
                >
                  <AppProgress v-if="changingPassword" statusText="" :size="15" class="mr-1.5 text-white" />
                  <span>{{ changingPassword ? '修改中…' : '更新密码' }}</span>
                </button>
              </div>
            </section>
          </template>

          <!-- ============================================================ -->
          <!-- TAB 3: 外观与偏好                                            -->
          <!-- ============================================================ -->
          <template v-else-if="activeTab === 'appearance'">
            <section class="apple-block">
              <div class="apple-section-head">
                <div class="apple-section-icon bg-[#FF9500]">
                  <AppIcon name="sun" :size="15" />
                </div>
                <div class="apple-section-copy">
                  <h2>界面外观</h2>
                  <p>个性化定制系统配色与主题风格</p>
                </div>
              </div>

              <div class="apple-inset-card">
                <div class="apple-list-row">
                  <div class="flex flex-col gap-0.5">
                    <span class="apple-row-label">明暗主题模式</span>
                    <span class="text-[12px] text-[var(--text-muted)]">在浅色与深色模式之间自由切换</span>
                  </div>
                  <ThemeToggle />
                </div>
              </div>
            </section>
          </template>

          <!-- ============================================================ -->
          <!-- TAB 4: AI 与 OCR 设置 (Apple HIG Inset Grouped Card)        -->
          <!-- ============================================================ -->
          <template v-else-if="activeTab === 'ai'">
            <div class="apple-settings-flow">
              
              <!-- 4.1 解析模块 (Parse) -->
              <section class="apple-block">
                <div class="apple-section-head">
                  <div class="apple-section-icon bg-[#007AFF]">
                    <AppIcon name="sparkles" :size="15" />
                  </div>
                  <div class="apple-section-copy">
                    <h2>解析</h2>
                    <p>试卷分类与 OCR 后拆题</p>
                  </div>
                </div>

                <div class="apple-inset-card">
                  <!-- 服务商选择 -->
                  <div
                    class="apple-list-row apple-select-trigger cursor-pointer"
                    @click="openPopover('llmProvider', $event)"
                  >
                    <span class="apple-row-label">服务商</span>
                    <div class="flex items-center gap-1.5 select-none">
                      <span class="text-[14px] text-[var(--accent)] font-medium">{{ currentLlmProviderLabel }}</span>
                      <AppIcon name="chevron-right" :size="14" class="text-[var(--text-muted)]" />
                    </div>
                  </div>

                  <!-- API 地址 (仅自定义/OpenRouter 时显示) -->
                  <div v-if="isCustomLlm" class="apple-list-row">
                    <span class="apple-row-label">API 地址</span>
                    <div class="apple-input-wrap">
                      <input
                        v-model="aiForm.llmBaseUrl"
                        class="apple-control-input font-mono text-[13px]"
                        placeholder="https://openrouter.ai/api/v1"
                        spellcheck="false"
                        autocomplete="off"
                      />
                    </div>
                  </div>

                  <!-- API Key -->
                  <div class="apple-list-row">
                    <div class="flex items-center gap-2 shrink-0">
                      <span class="apple-row-label">API Key</span>
                      <span v-if="aiSettings?.has_api_key" class="apple-badge-success">已配置</span>
                    </div>
                    <div class="flex items-center gap-2 flex-1 justify-end min-w-0">
                      <div class="apple-input-wrap flex-1 max-w-[280px]">
                        <input
                          v-model="aiForm.apiKey"
                          :type="showApiKey ? 'text' : 'password'"
                          class="apple-control-input"
                          :class="{ 'font-mono text-[13px]': showApiKey, 'apple-password-input': !showApiKey }"
                          placeholder="输入新 Key，留空保持不变"
                          autocomplete="off"
                        />
                        <button
                          type="button"
                          class="apple-eye-btn"
                          :aria-label="showApiKey ? '隐藏 API Key' : '显示 API Key'"
                          @click="showApiKey = !showApiKey"
                        >
                          <AppIcon :name="showApiKey ? 'eye-off' : 'eye'" :size="15" />
                        </button>
                      </div>
                      <button
                        v-if="isCustomLlm"
                        type="button"
                        class="apple-text-btn"
                        :disabled="testingLlm"
                        @click="testLlmConnection"
                      >
                        {{ testingLlm ? '测试中…' : '测试' }}
                      </button>
                    </div>
                  </div>

                  <!-- 文本模型 -->
                  <div class="apple-list-row">
                    <div class="flex items-center gap-1 shrink-0">
                      <span class="apple-row-label">文本模型</span>
                      <span v-if="!isCustomLlm" class="text-[12px] text-[var(--text-muted)]">（可选）</span>
                    </div>
                    <div class="apple-input-wrap">
                      <input
                        v-model="aiForm.modelText"
                        class="apple-control-input font-mono text-[13px]"
                        :placeholder="textModelPlaceholder"
                        spellcheck="false"
                      />
                    </div>
                  </div>

                  <!-- 视觉模型 -->
                  <div class="apple-list-row">
                    <div class="flex items-center gap-1 shrink-0">
                      <span class="apple-row-label">视觉模型</span>
                      <span class="text-[12px] text-[var(--text-muted)]">（可选）</span>
                    </div>
                    <div class="apple-input-wrap">
                      <input
                        v-model="aiForm.modelVision"
                        class="apple-control-input font-mono text-[13px]"
                        placeholder="如 qwen-vl-plus"
                        spellcheck="false"
                      />
                    </div>
                  </div>

                  <!-- 拆题并发 -->
                  <div class="apple-list-row">
                    <span class="apple-row-label">拆题并发</span>
                    <div class="apple-stepper">
                      <button
                        type="button"
                        class="apple-stepper-btn"
                        :disabled="aiForm.stage2Concurrency <= 1"
                        aria-label="减少拆题并发"
                        @click="bumpConcurrency('stage2Concurrency', -1)"
                      >
                        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round">
                          <path d="M5 12h14" />
                        </svg>
                      </button>
                      <span class="apple-stepper-val">{{ aiForm.stage2Concurrency }}</span>
                      <button
                        type="button"
                        class="apple-stepper-btn"
                        :disabled="aiForm.stage2Concurrency >= 16"
                        aria-label="增加拆题并发"
                        @click="bumpConcurrency('stage2Concurrency', 1)"
                      >
                        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round">
                          <path d="M12 5v14M5 12h14" />
                        </svg>
                      </button>
                    </div>
                  </div>
                </div>

                <!-- 模块下方说明 (Footer Style) -->
                <p class="apple-card-footer">
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

              <!-- 4.2 智能打标模块 (Tagging) -->
              <section class="apple-block">
                <div class="apple-section-head">
                  <div class="apple-section-icon bg-[#FF9500]">
                    <AppIcon name="tag" :size="15" />
                  </div>
                  <div class="apple-section-copy">
                    <h2>智能打标</h2>
                    <p>分类任务，适合便宜的快模型</p>
                  </div>
                </div>

                <div class="apple-inset-card">
                  <!-- 独立服务商 Switch -->
                  <div
                    class="apple-list-row cursor-pointer"
                    @click="aiForm.taggingIndependent = !aiForm.taggingIndependent"
                  >
                    <div class="flex flex-col gap-0.5">
                      <span class="apple-row-label">独立服务商</span>
                      <span class="text-[12px] text-[var(--text-muted)]">解析与打标可使用不同厂商</span>
                    </div>
                    <AppToggle v-model="aiForm.taggingIndependent" @click.stop />
                  </div>

                  <!-- 独立服务商展开字段 -->
                  <template v-if="aiForm.taggingIndependent">
                    <div
                      class="apple-list-row apple-select-trigger cursor-pointer"
                      @click="openPopover('taggingProvider', $event)"
                    >
                      <span class="apple-row-label">服务商</span>
                      <div class="flex items-center gap-1.5 select-none">
                        <span class="text-[14px] text-[var(--accent)] font-medium">{{ currentTaggingProviderLabel }}</span>
                        <AppIcon name="chevron-right" :size="14" class="text-[var(--text-muted)]" />
                      </div>
                    </div>

                    <div v-if="isCustomTagging" class="apple-list-row">
                      <span class="apple-row-label">API 地址</span>
                      <div class="apple-input-wrap">
                        <input
                          v-model="aiForm.taggingLlmBaseUrl"
                          class="apple-control-input font-mono text-[13px]"
                          placeholder="https://openrouter.ai/api/v1"
                          spellcheck="false"
                          autocomplete="off"
                        />
                      </div>
                    </div>

                    <div class="apple-list-row">
                      <div class="flex items-center gap-2 shrink-0">
                        <span class="apple-row-label">API Key</span>
                        <span v-if="aiSettings?.has_tagging_api_key" class="apple-badge-success">已配置</span>
                        <span v-else-if="!isCustomTagging" class="text-[12px] text-[var(--text-muted)]">可留空</span>
                      </div>
                      <div class="apple-input-wrap">
                        <input
                          v-model="aiForm.taggingApiKey"
                          :type="showTaggingApiKey ? 'text' : 'password'"
                          class="apple-control-input"
                          :class="{ 'font-mono text-[13px]': showTaggingApiKey, 'apple-password-input': !showTaggingApiKey }"
                          placeholder="输入新 Key，留空保持不变"
                          autocomplete="off"
                        />
                        <button
                          type="button"
                          class="apple-eye-btn"
                          :aria-label="showTaggingApiKey ? '隐藏 API Key' : '显示 API Key'"
                          @click="showTaggingApiKey = !showTaggingApiKey"
                        >
                          <AppIcon :name="showTaggingApiKey ? 'eye-off' : 'eye'" :size="15" />
                        </button>
                      </div>
                    </div>
                  </template>

                  <!-- 打标模型 -->
                  <div class="apple-list-row">
                    <div class="flex items-center gap-1 shrink-0">
                      <span class="apple-row-label">打标模型</span>
                      <span class="text-[12px] text-[var(--text-muted)]">（可选）</span>
                    </div>
                    <div class="apple-input-wrap">
                      <input
                        v-model="aiForm.modelTagging"
                        class="apple-control-input font-mono text-[13px]"
                        :placeholder="taggingModelPlaceholder"
                        spellcheck="false"
                      />
                    </div>
                  </div>

                  <!-- 打标并发 -->
                  <div class="apple-list-row">
                    <span class="apple-row-label">打标并发</span>
                    <div class="apple-stepper">
                      <button
                        type="button"
                        class="apple-stepper-btn"
                        :disabled="aiForm.taggingConcurrency <= 1"
                        aria-label="减少打标并发"
                        @click="bumpConcurrency('taggingConcurrency', -1)"
                      >
                        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round">
                          <path d="M5 12h14" />
                        </svg>
                      </button>
                      <span class="apple-stepper-val">{{ aiForm.taggingConcurrency }}</span>
                      <button
                        type="button"
                        class="apple-stepper-btn"
                        :disabled="aiForm.taggingConcurrency >= 16"
                        aria-label="增加打标并发"
                        @click="bumpConcurrency('taggingConcurrency', 1)"
                      >
                        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round">
                          <path d="M12 5v14M5 12h14" />
                        </svg>
                      </button>
                    </div>
                  </div>
                </div>

                <!-- 模块下方说明 (Footer Style) -->
                <p class="apple-card-footer">
                  {{ aiForm.taggingIndependent
                    ? '例如解析走 OpenRouter，打标走官方 DeepSeek。'
                    : '关闭时沿用上方解析服务商与 Key。模型可单独指定。' }}
                  并发为同一账号同时打标的题目数（1–16）。
                </p>
              </section>

              <!-- 4.2b 向量召回（全站，仅管理员） -->
              <section v-if="auth.isAdminUnified" class="apple-block">
                <div class="apple-section-head">
                  <div class="apple-section-icon bg-[#5856D6]">
                    <AppIcon name="search" :size="15" />
                  </div>
                  <div class="apple-section-copy">
                    <h2>向量召回（全站）</h2>
                    <p>知识树节点 embedding，全站共用</p>
                  </div>
                </div>

                <div class="apple-inset-card">
                  <div class="apple-list-row">
                    <span class="apple-row-label">模型</span>
                    <select
                      v-model="aiForm.embeddingModel"
                      class="apple-control-select"
                    >
                      <option
                        v-for="m in embeddingModelOptions"
                        :key="m"
                        :value="m"
                      >{{ m }}</option>
                    </select>
                  </div>
                  <div class="apple-list-row">
                    <span class="apple-row-label">维度</span>
                    <span class="text-[14px] text-[var(--text-muted)] select-none">1024（与库表一致）</span>
                  </div>
                </div>
                <p class="apple-card-footer">
                  密钥沿用服务器 QWEN_API_KEY，与上方解析/打标 Key 无关。换模型后会按新模型重嵌全部知识树节点。
                </p>
              </section>

              <!-- 4.3 OCR 识别引擎模块 -->
              <section class="apple-block">
                <div class="apple-section-head">
                  <div class="apple-section-icon bg-[#30B0C7]">
                    <AppIcon name="image" :size="15" />
                  </div>
                  <div class="apple-section-copy">
                    <h2>OCR</h2>
                    <p>图片与 PDF 识别引擎</p>
                  </div>
                  <span v-if="aiSettings?.has_doc2x_key" class="apple-badge-success">Doc2X 已配置</span>
                  <span v-else-if="aiSettings?.has_mineru_key" class="apple-badge-accent">MinerU 已配置</span>
                </div>

                <div class="apple-inset-card">
                  <!-- 引擎选择 -->
                  <div
                    class="apple-list-row apple-select-trigger cursor-pointer"
                    @click="openPopover('ocrProvider', $event)"
                  >
                    <span class="apple-row-label">引擎</span>
                    <div class="flex items-center gap-1.5 select-none">
                      <span class="text-[14px] text-[var(--accent)] font-medium">{{ currentOcrProviderLabel }}</span>
                      <AppIcon name="chevron-right" :size="14" class="text-[var(--text-muted)]" />
                    </div>
                  </div>

                  <!-- Doc2X 专有设置 -->
                  <template v-if="aiForm.ocrProvider === 'doc2x'">
                    <div class="apple-list-row">
                      <div class="flex items-center gap-2 shrink-0">
                        <span class="apple-row-label">Doc2X API Key</span>
                        <span v-if="aiSettings?.has_doc2x_key" class="apple-badge-success">已配置</span>
                      </div>
                      <div class="flex items-center gap-2 flex-1 justify-end min-w-0">
                        <div class="apple-input-wrap flex-1 max-w-[280px]">
                          <input
                            v-model="aiForm.doc2xApiKey"
                            :type="showDoc2xApiKey ? 'text' : 'password'"
                            class="apple-control-input"
                            :class="{ 'font-mono text-[13px]': showDoc2xApiKey, 'apple-password-input': !showDoc2xApiKey }"
                            placeholder="sk-xxx，留空使用平台默认"
                            autocomplete="off"
                          />
                          <button
                            type="button"
                            class="apple-eye-btn"
                            :aria-label="showDoc2xApiKey ? '隐藏 API Key' : '显示 API Key'"
                            @click="showDoc2xApiKey = !showDoc2xApiKey"
                          >
                            <AppIcon :name="showDoc2xApiKey ? 'eye-off' : 'eye'" :size="15" />
                          </button>
                        </div>
                        <button
                          type="button"
                          class="apple-text-btn"
                          :disabled="testingConnection"
                          @click="testOcrConnection"
                        >
                          {{ testingConnection ? '测试中…' : '测试' }}
                        </button>
                      </div>
                    </div>
                  </template>

                  <!-- MinerU 私有部署专有设置 -->
                  <template v-else-if="aiForm.ocrProvider === 'mineru_local'">
                    <div class="apple-list-row">
                      <span class="apple-row-label">服务端点</span>
                      <div class="apple-input-wrap">
                        <input
                          v-model="aiForm.mineruEndpoint"
                          class="apple-control-input font-mono text-[13px]"
                          placeholder="http://127.0.0.1:8000"
                          spellcheck="false"
                        />
                      </div>
                    </div>
                    <div class="apple-list-row">
                      <div class="flex items-center gap-2 shrink-0">
                        <span class="apple-row-label">API Key</span>
                        <span class="text-[12px] text-[var(--text-muted)]">（可选）</span>
                        <span v-if="aiSettings?.has_mineru_key" class="apple-badge-accent">已配置</span>
                      </div>
                      <div class="flex items-center gap-2 flex-1 justify-end min-w-0">
                        <div class="apple-input-wrap flex-1 max-w-[280px]">
                          <input
                            v-model="aiForm.mineruApiKey"
                            :type="showMineruApiKey ? 'text' : 'password'"
                            class="apple-control-input"
                            :class="{ 'font-mono text-[13px]': showMineruApiKey, 'apple-password-input': !showMineruApiKey }"
                            placeholder="仅网关鉴权时填写"
                            autocomplete="off"
                          />
                          <button
                            type="button"
                            class="apple-eye-btn"
                            :aria-label="showMineruApiKey ? '隐藏 API Key' : '显示 API Key'"
                            @click="showMineruApiKey = !showMineruApiKey"
                          >
                            <AppIcon :name="showMineruApiKey ? 'eye-off' : 'eye'" :size="15" />
                          </button>
                        </div>
                        <button
                          type="button"
                          class="apple-text-btn"
                          :disabled="testingConnection"
                          @click="testOcrConnection"
                        >
                          {{ testingConnection ? '测试中…' : '测试' }}
                        </button>
                      </div>
                    </div>
                  </template>
                </div>

                <!-- 模块下方说明 (Footer Style) -->
                <p class="apple-card-footer">
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

              <!-- 4.4 底部操作按钮 -->
              <div class="flex justify-end pt-2 pb-8">
                <button
                  type="button"
                  class="apple-save-btn"
                  :disabled="savingAi"
                  @click="saveAiSettings"
                >
                  <AppProgress v-if="savingAi" statusText="" :size="16" class="mr-2 text-white" />
                  <span>{{ savingAi ? '保存中…' : '保存设置' }}</span>
                </button>
              </div>
            </div>
          </template>

        </main>
      </div>
    </div>

    <!-- ============================================================ -->
    <!-- Apple HIG Context Popover Menu (Teleported to body)          -->
    <!-- ============================================================ -->
    <Teleport to="body">
      <Transition name="popover">
        <div
          v-if="activePopover"
          class="apple-popover-menu"
          :style="popoverStyle"
          @click.stop
        >
          <!-- LLM 服务商选项 -->
          <template v-if="activePopover === 'llmProvider'">
            <button
              v-for="opt in llmProviderOptions"
              :key="opt.value"
              type="button"
              class="apple-popover-item"
              :class="{ 'is-selected': (aiForm.provider === 'openrouter' ? 'custom' : aiForm.provider) === opt.value }"
              @click="selectLlmProvider(opt.value)"
            >
              <span>{{ opt.label }}</span>
              <AppIcon
                v-if="(aiForm.provider === 'openrouter' ? 'custom' : aiForm.provider) === opt.value"
                name="check"
                :size="15"
                class="apple-popover-check"
              />
            </button>
          </template>

          <!-- 智能打标服务商选项 -->
          <template v-else-if="activePopover === 'taggingProvider'">
            <button
              v-for="opt in llmProviderOptions"
              :key="opt.value"
              type="button"
              class="apple-popover-item"
              :class="{ 'is-selected': (aiForm.taggingProvider === 'openrouter' ? 'custom' : aiForm.taggingProvider) === opt.value }"
              @click="selectTaggingProvider(opt.value)"
            >
              <span>{{ opt.label }}</span>
              <AppIcon
                v-if="(aiForm.taggingProvider === 'openrouter' ? 'custom' : aiForm.taggingProvider) === opt.value"
                name="check"
                :size="15"
                class="apple-popover-check"
              />
            </button>
          </template>

          <!-- OCR 引擎选项 -->
          <template v-else-if="activePopover === 'ocrProvider'">
            <button
              v-for="opt in ocrProviderOptions"
              :key="opt.value"
              type="button"
              class="apple-popover-item"
              :class="{ 'is-selected': aiForm.ocrProvider === opt.value }"
              @click="selectOcrProvider(opt.value)"
            >
              <span>{{ opt.label }}</span>
              <AppIcon
                v-if="aiForm.ocrProvider === opt.value"
                name="check"
                :size="15"
                class="apple-popover-check"
              />
            </button>
          </template>
        </div>
      </Transition>
    </Teleport>
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
import { AppIcon, AppProgress, AppToggle } from '@/components/ui'
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
// 密码与 API Key 明文/密文查看控制
// ---------------------------------------------------------------------------
const showApiKey = ref(false)
const showTaggingApiKey = ref(false)
const showDoc2xApiKey = ref(false)
const showMineruApiKey = ref(false)
const showOldPassword = ref(false)
const showNewPassword = ref(false)
const showConfirmPassword = ref(false)

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
  embeddingModel: 'text-embedding-v3',
})

const llmProviderOptions = [
  { value: 'deepseek', label: 'DeepSeek' },
  { value: 'qwen', label: '通义千问' },
  { value: 'glm', label: '智谱 GLM' },
  { value: 'gemini', label: 'Google Gemini' },
  { value: 'openai', label: 'OpenAI' },
  { value: 'custom', label: '自定义 / OpenRouter' },
] as const

const ocrProviderOptions = [
  { value: 'auto', label: '自动（跟随系统）' },
  { value: 'doc2x', label: 'Doc2X 公式引擎' },
  { value: 'mineru_local', label: 'MinerU 私有部署' },
  { value: 'qwen_vl', label: 'Qwen-VL 通用' },
] as const

const TEXT_MODEL_DEFAULTS: Record<string, string> = {
  deepseek: 'deepseek-chat',
  qwen: 'qwen-plus',
  glm: 'glm-4-flash',
  gemini: 'gemini-3.7-flash',
  openai: 'gpt-4o-mini',
  custom: 'stealth/ox-alpha',
}

const DEFAULT_EMBEDDING_MODELS = ['text-embedding-v3', 'qwen3.7-text-embedding'] as const
const embeddingModelOptions = ref<string[]>([...DEFAULT_EMBEDDING_MODELS])

const isCustomLlm = computed(() => aiForm.provider === 'custom' || aiForm.provider === 'openrouter')
const isCustomTagging = computed(
  () => aiForm.taggingProvider === 'custom' || aiForm.taggingProvider === 'openrouter',
)

const currentLlmProviderLabel = computed(() => {
  const p = aiForm.provider === 'openrouter' ? 'custom' : aiForm.provider
  return llmProviderOptions.find(o => o.value === p)?.label || 'DeepSeek'
})

const currentTaggingProviderLabel = computed(() => {
  const p = aiForm.taggingProvider === 'openrouter' ? 'custom' : aiForm.taggingProvider
  return llmProviderOptions.find(o => o.value === p)?.label || 'DeepSeek'
})

const currentOcrProviderLabel = computed(() => {
  return ocrProviderOptions.find(o => o.value === aiForm.ocrProvider)?.label || '自动（跟随系统）'
})

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
    if (res.data.embedding_models?.length) {
      embeddingModelOptions.value = res.data.embedding_models
    }
    if (res.data.embedding_model) {
      aiForm.embeddingModel = res.data.embedding_model
    }
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
    const prevEmbedding = aiSettings.value?.embedding_model || 'text-embedding-v3'
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
    if (auth.isAdminUnified) {
      payload.embedding_model = aiForm.embeddingModel
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
    if (res.data.embedding_model) aiForm.embeddingModel = res.data.embedding_model
    if (res.data.embedding_models?.length) {
      embeddingModelOptions.value = res.data.embedding_models
    }
    toast.success(
      auth.isAdminUnified && payload.embedding_model && payload.embedding_model !== prevEmbedding
        ? '已保存；后台将按新模型重嵌全部知识树节点'
        : 'AI 与 OCR 设置已成功保存',
    )
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
    if (aiForm.ocrProvider === 'doc2x' && aiForm.doc2xApiKey) {
      payload.api_key = aiForm.doc2xApiKey
    } else if (aiForm.ocrProvider === 'mineru_local') {
      if (aiForm.mineruApiKey) payload.api_key = aiForm.mineruApiKey
      if (aiForm.mineruEndpoint) payload.endpoint = aiForm.mineruEndpoint
    }
    const res = await aiApi.testOcrConnection(payload)
    const { ok, latency_ms, message } = res.data
    if (ok) {
      toast.success(`OCR 连接成功（${latency_ms}ms）：${message}`)
    } else {
      toast.error(`OCR 连接失败：${message}`)
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
      toast.success(`LLM 连接成功（${latency_ms}ms）：${message}`)
    } else {
      toast.error(`LLM 连接失败：${message}`)
    }
  } catch (e: any) {
    toast.error(e?.response?.data?.error || e?.message || '测试连接失败')
  } finally {
    testingLlm.value = false
  }
}

onMounted(loadAiSettings)

// ---------------------------------------------------------------------------
// Apple HIG Popover 选择器交互
// ---------------------------------------------------------------------------
type ActivePopoverType = 'llmProvider' | 'taggingProvider' | 'ocrProvider' | null
const activePopover = ref<ActivePopoverType>(null)
const popoverStyle = ref({ top: '0px', left: '0px', width: '220px' })

function openPopover(type: ActivePopoverType, event: MouseEvent) {
  if (activePopover.value === type) {
    activePopover.value = null
    return
  }
  const target = event.currentTarget as HTMLElement
  if (!target) return
  const rect = target.getBoundingClientRect()
  
  const popoverWidth = 230
  let left = rect.right - popoverWidth
  if (left < 16) left = 16
  if (left + popoverWidth > window.innerWidth - 16) {
    left = window.innerWidth - popoverWidth - 16
  }

  popoverStyle.value = {
    top: `${rect.bottom + 6}px`,
    left: `${left}px`,
    width: `${popoverWidth}px`,
  }
  activePopover.value = type
}

function closePopover() {
  activePopover.value = null
}

function selectLlmProvider(val: string) {
  aiForm.provider = val
  closePopover()
}

function selectTaggingProvider(val: string) {
  aiForm.taggingProvider = val
  closePopover()
}

function selectOcrProvider(val: string) {
  aiForm.ocrProvider = val
  closePopover()
}

function handleGlobalClick(e: MouseEvent) {
  if (!activePopover.value) return
  const target = e.target as HTMLElement
  if (target.closest('.apple-popover-menu') || target.closest('.apple-select-trigger')) {
    return
  }
  closePopover()
}

function handleGlobalKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && activePopover.value) {
    closePopover()
  }
}

function handleScrollOrResize() {
  if (activePopover.value) {
    closePopover()
  }
}

onMounted(() => {
  document.addEventListener('click', handleGlobalClick)
  document.addEventListener('keydown', handleGlobalKeydown)
  window.addEventListener('scroll', handleScrollOrResize, true)
  window.addEventListener('resize', handleScrollOrResize)
})

onBeforeUnmount(() => {
  document.removeEventListener('click', handleGlobalClick)
  document.removeEventListener('keydown', handleGlobalKeydown)
  window.removeEventListener('scroll', handleScrollOrResize, true)
  window.removeEventListener('resize', handleScrollOrResize)
})

// ---------------------------------------------------------------------------
// 头像逻辑
// ---------------------------------------------------------------------------
type AvatarState = 'idle' | 'uploading' | 'error'
const avatarState = ref<AvatarState>('idle')
const avatarErrorMsg = ref('')
const avatarPreviewSrc = ref<string>('')
const fileInputRef = ref<HTMLInputElement | null>(null)

const avatarLetter = computed(() =>
  (auth.displayName || '?').charAt(0).toUpperCase(),
)

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

const roleBadgeClass = computed(() => {
  if (!profile.value) return 'apple-badge-gray'
  if (profile.value.global_role === 'super_admin') return 'apple-badge-purple'
  return 'apple-badge-accent'
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
/* ============================================================
   Apple HIG 风格视觉规范体系 (macOS / iOS Settings)
   深度对齐全局 Tokens 与深浅主题系统 ([data-theme='dark'])
   ============================================================ */

.profile-page {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background-color: var(--bg-primary, #f5f5f7);
  font-family: -apple-system, BlinkMacSystemFont, 'SF Pro Text', 'SF Pro Display', 'PingFang SC', 'Helvetica Neue', sans-serif;
  color: var(--text-primary, #1d1d1f);
}

/* ===== 1. 吸顶导航栏 ===== */
.profile-sticky-bar {
  position: sticky;
  top: 0;
  z-index: 100;
  flex-shrink: 0;
  background: var(--bg-nav, rgba(255, 255, 255, 0.82));
  backdrop-filter: saturate(180%) blur(20px);
  -webkit-backdrop-filter: saturate(180%) blur(20px);
  border-bottom: 1px solid var(--border-color, rgba(0, 0, 0, 0.08));
}

.profile-header {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 24px;
  max-width: 1080px;
  margin: 0 auto;
}

.back-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  border-radius: 9999px;
  background: transparent;
  border: none;
  color: var(--text-primary, #1d1d1f);
  cursor: pointer;
  transition: background-color 0.15s ease;
}

.back-btn:hover {
  background: var(--bg-hover, rgba(0, 0, 0, 0.06));
}

.page-title {
  font-size: 17px;
  font-weight: 600;
  color: var(--text-primary, #1d1d1f);
  margin: 0;
  letter-spacing: -0.015em;
}

/* ===== 2. 独立滚动容器与 Split View 布局 ===== */
.profile-scroll-area {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
  background-color: var(--bg-primary, #f5f5f7);
}

.profile-layout-container {
  width: 100%;
  max-width: 1080px;
  margin: 0 auto;
  padding: 24px 20px 48px;
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 32px;
}

@media (min-width: 768px) {
  .profile-layout-container {
    flex-direction: row;
    align-items: flex-start;
    padding: 32px 24px 64px;
  }
}

/* ===== 3. 左侧固定宽度侧边栏 (240px) ===== */
.profile-sidebar {
  width: 100%;
  flex-shrink: 0;
  background: var(--bg-card, #ffffff);
  backdrop-filter: blur(16px);
  -webkit-backdrop-filter: blur(16px);
  border: 1px solid var(--border-color, rgba(0, 0, 0, 0.06));
  border-radius: 18px;
  padding: 12px;
  box-shadow: var(--shadow-xs, 0 1px 3px rgba(0, 0, 0, 0.02));
  user-select: none;
}

@media (min-width: 768px) {
  .profile-sidebar {
    width: 240px;
    position: sticky;
    top: 24px;
  }
}

.sidebar-section-title {
  padding: 6px 12px 8px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-muted, #86868b);
}

.sidebar-nav {
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.sidebar-tab-item {
  display: flex;
  align-items: center;
  gap: 12px;
  width: 100%;
  padding: 9px 12px;
  font-size: 14px;
  font-weight: 500;
  border-radius: 10px;
  border: none;
  background: transparent;
  color: var(--text-primary, #1d1d1f);
  cursor: pointer;
  text-align: left;
  transition: background-color 0.15s ease, color 0.15s ease, transform 0.1s ease;
}

.sidebar-tab-item:hover:not(.is-active) {
  background: var(--bg-hover, rgba(0, 0, 0, 0.04));
}

.sidebar-tab-item.is-active {
  background: var(--accent, #007aff);
  color: #ffffff !important;
  font-weight: 600;
  box-shadow: 0 1px 4px rgba(0, 122, 255, 0.3);
}

.sidebar-tab-item.is-active .sidebar-tab-icon {
  color: #ffffff !important;
}

.sidebar-tab-icon {
  color: var(--text-muted, #86868b);
  flex-shrink: 0;
  transition: color 0.15s ease;
}

.sidebar-tab-label {
  flex: 1;
  truncate: true;
}

/* ===== 4. 右侧主内容区 (居中限制 680px 呼吸感) ===== */
.profile-content-column {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  width: 100%;
}

.profile-content-column > * {
  width: 100%;
  max-width: 680px;
}

.apple-settings-flow {
  display: flex;
  flex-direction: column;
  gap: 28px;
}

/* ===== 5. 分组与 Header (Inset Grouped Card Structure) ===== */
.apple-block {
  display: flex;
  flex-direction: column;
}

.apple-section-head {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 0 4px 10px;
}

.apple-section-icon {
  width: 28px;
  height: 28px;
  border-radius: 7px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: #ffffff;
  flex-shrink: 0;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.08);
}

.apple-section-copy {
  flex: 1;
  min-width: 0;
}

.apple-section-copy h2 {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
  letter-spacing: -0.015em;
  color: var(--text-primary, #1d1d1f);
  line-height: 1.25;
}

.apple-section-copy p {
  margin: 2px 0 0;
  font-size: 12px;
  color: var(--text-muted, #86868b);
  line-height: 1.35;
}

/* ===== 6. Apple Inset Grouped Card 容器与行列表 ===== */
.apple-inset-card {
  background: var(--bg-card, #ffffff);
  border-radius: 14px;
  border: 1px solid var(--border-color, rgba(0, 0, 0, 0.06));
  box-shadow: var(--shadow-xs, 0 1px 3px rgba(0, 0, 0, 0.03));
  overflow: hidden;
}

.apple-list-row {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 48px;
  padding: 8px 16px;
  gap: 12px;
  background: transparent;
  transition: background-color 0.12s ease;
}

.apple-list-row:not(:last-child)::after {
  content: '';
  position: absolute;
  left: 16px;
  right: 0;
  bottom: 0;
  height: 1px;
  background: var(--divider, rgba(0, 0, 0, 0.07));
}

.apple-list-row.cursor-pointer:hover {
  background: var(--bg-hover, rgba(0, 0, 0, 0.02));
}

.apple-row-label {
  font-size: 14px;
  font-weight: 400;
  color: var(--text-primary, #1d1d1f);
  letter-spacing: -0.01em;
  flex-shrink: 0;
}

.apple-control-select {
  max-width: min(70%, 280px);
  min-height: 32px;
  margin: 0;
  padding: 4px 8px;
  border: none;
  border-radius: 6px;
  background: rgba(0, 0, 0, 0.03);
  font-size: 13.5px;
  font-weight: 500;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  color: var(--accent, #0a84ff);
  text-align: right;
  outline: none;
  cursor: pointer;
}

:global([data-theme='dark']) .apple-control-select {
  background: rgba(255, 255, 255, 0.06);
}

/* ===== 6.1 macOS 风格内嵌控件框 (Apple Inset Control Wrap) ===== */
.apple-input-wrap {
  display: flex;
  align-items: center;
  width: 100%;
  max-width: 320px;
  height: 32px;
  background: rgba(0, 0, 0, 0.03);
  border: 1px solid rgba(0, 0, 0, 0.08);
  border-radius: 6px;
  padding: 0 8px;
  transition: background-color 0.15s ease, border-color 0.15s ease, box-shadow 0.15s ease;
}

:global([data-theme='dark']) .apple-input-wrap {
  background: rgba(255, 255, 255, 0.06);
  border-color: rgba(255, 255, 255, 0.08);
}

.apple-input-wrap:focus-within {
  background: #ffffff;
  border-color: var(--accent, #007aff);
  box-shadow: 0 0 0 3px rgba(0, 122, 255, 0.18);
}

:global([data-theme='dark']) .apple-input-wrap:focus-within {
  background: #1c1c1e;
  border-color: var(--accent, #0a84ff);
  box-shadow: 0 0 0 3px rgba(10, 132, 255, 0.25);
}

.apple-control-input {
  flex: 1;
  min-width: 0;
  height: 100%;
  border: none !important;
  background: transparent !important;
  padding: 0;
  margin: 0;
  font-size: 13.5px;
  line-height: 22px;
  color: var(--text-primary, #1d1d1f);
  text-align: left;
  outline: none !important;
  box-shadow: none !important;
  caret-color: var(--accent, #007aff);
  -webkit-appearance: none;
  appearance: none;
}

/* 密码掩码态：采用 Apple 原生 SF Pro 字体，饱满圆点居中，字符间距紧凑 */
.apple-password-input {
  font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", sans-serif;
  letter-spacing: 0.15em;
  font-size: 14px;
}

.apple-control-input::placeholder {
  color: var(--text-muted, #86868b);
  opacity: 0.65;
  letter-spacing: normal;
}

:global([data-theme='dark']) .apple-control-input::placeholder {
  color: rgba(255, 255, 255, 0.35);
  opacity: 1;
}

.apple-control-input:-webkit-autofill,
.apple-control-input:-webkit-autofill:hover,
.apple-control-input:-webkit-autofill:focus {
  -webkit-text-fill-color: var(--text-primary, #1d1d1f);
  -webkit-box-shadow: 0 0 0px 1000px transparent inset !important;
  transition: background-color 5000s ease-in-out 0s;
}

/* 密码/Key 显隐眼睛图标按钮 */
.apple-eye-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border-radius: 4px;
  border: none;
  background: transparent;
  color: var(--text-muted, #86868b);
  cursor: pointer;
  padding: 0;
  margin-left: 4px;
  flex-shrink: 0;
  transition: color 0.15s ease, background-color 0.15s ease;
}

.apple-eye-btn:hover {
  color: var(--text-primary, #1d1d1f);
  background: var(--bg-hover, rgba(0, 0, 0, 0.04));
}

:global([data-theme='dark']) .apple-eye-btn:hover {
  background: rgba(255, 255, 255, 0.08);
}

.apple-card-footer {
  margin: 7px 16px 0;
  font-size: 12px;
  line-height: 1.45;
  color: var(--text-muted, #86868b);
}

.apple-card-footer a {
  color: var(--accent, #007aff);
  text-decoration: none;
}

.apple-card-footer a:hover {
  text-decoration: underline;
}

/* ===== 7. 交互组件标准化 ===== */

/* 7.1 分段式步进器 (Apple Stepper) */
.apple-stepper {
  display: inline-flex;
  align-items: center;
  background: #e5e5ea;
  border-radius: 7px;
  padding: 2px;
  user-select: none;
}

:global([data-theme='dark']) .apple-stepper {
  background: #3a3a3c;
}

.apple-stepper-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 24px;
  border-radius: 5px;
  background: transparent;
  border: none;
  color: var(--accent, #007aff);
  cursor: pointer;
  transition: transform 0.1s ease, background-color 0.15s ease;
}

:global([data-theme='dark']) .apple-stepper-btn {
  color: #ffffff;
}

.apple-stepper-btn:hover:not(:disabled) {
  background: rgba(0, 0, 0, 0.05);
}

:global([data-theme='dark']) .apple-stepper-btn:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.15);
}

.apple-stepper-btn:active:not(:disabled) {
  transform: scale(0.92);
}

.apple-stepper-btn:disabled {
  color: var(--text-muted, #8e8e93);
  opacity: 0.35;
  cursor: not-allowed;
}

.apple-stepper-val {
  min-width: 24px;
  text-align: center;
  font-size: 14px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  color: var(--text-primary, #1d1d1f);
  padding: 0 4px;
}

/* 7.2 状态徽章 (Status Badges) */
.apple-badge-success {
  font-size: 12px;
  font-weight: 500;
  color: var(--success, #34c759);
  background: var(--success-light, rgba(52, 199, 89, 0.12));
  padding: 2px 8px;
  border-radius: 9999px;
  white-space: nowrap;
}

:global([data-theme='dark']) .apple-badge-success {
  color: #30d158;
  background: rgba(48, 209, 88, 0.15);
}

.apple-badge-accent {
  font-size: 12px;
  font-weight: 500;
  color: var(--accent, #007aff);
  background: var(--accent-light, rgba(0, 122, 255, 0.12));
  padding: 2px 8px;
  border-radius: 9999px;
  white-space: nowrap;
}

:global([data-theme='dark']) .apple-badge-accent {
  color: #0a84ff;
  background: rgba(10, 132, 255, 0.18);
}

.apple-badge-role {
  font-size: 12px;
  font-weight: 500;
  padding: 2px 8px;
  border-radius: 9999px;
  white-space: nowrap;
}

.apple-badge-purple {
  color: var(--purple, #af52de);
  background: var(--purple-light, rgba(175, 82, 222, 0.12));
}

:global([data-theme='dark']) .apple-badge-purple {
  color: #bf5af2;
  background: rgba(191, 90, 242, 0.18);
}

.apple-badge-gray {
  color: var(--text-muted, #86868b);
  background: var(--bg-hover, rgba(134, 134, 139, 0.12));
}

/* 7.3 文字操作按钮 (如测试连接) */
.apple-text-btn {
  border: none;
  background: none;
  padding: 2px 6px;
  font-size: 13px;
  font-weight: 500;
  color: var(--accent, #007aff);
  cursor: pointer;
  flex-shrink: 0;
  border-radius: 4px;
  transition: opacity 0.15s ease, transform 0.1s ease;
}

.apple-text-btn:hover:not(:disabled) {
  opacity: 0.8;
}

.apple-text-btn:active:not(:disabled) {
  transform: scale(0.95);
}

.apple-text-btn:disabled {
  opacity: 0.35;
  cursor: not-allowed;
}

/* 7.4 次级操作按钮 (如更换头像) */
.apple-secondary-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 6px 14px;
  font-size: 13px;
  font-weight: 500;
  border-radius: 8px;
  border: 1px solid var(--border-color, rgba(0, 0, 0, 0.1));
  background: var(--bg-card, #ffffff);
  color: var(--text-primary, #1d1d1f);
  cursor: pointer;
  transition: background-color 0.15s ease, border-color 0.15s ease;
  box-shadow: var(--shadow-xs, 0 1px 2px rgba(0, 0, 0, 0.03));
}

.apple-secondary-btn:hover:not(:disabled) {
  background: var(--bg-hover, rgba(0, 0, 0, 0.04));
}

/* 7.5 主操作保存按钮 (Apple Blue Button) */
.apple-save-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 110px;
  height: 40px;
  padding: 0 20px;
  border-radius: 10px;
  border: none;
  background: var(--accent, #007aff);
  color: #ffffff;
  font-size: 14px;
  font-weight: 600;
  letter-spacing: -0.01em;
  cursor: pointer;
  box-shadow: 0 1px 3px rgba(0, 122, 255, 0.3);
  transition: background-color 0.15s ease, transform 0.1s ease, box-shadow 0.15s ease;
}

.apple-save-btn:hover:not(:disabled) {
  background: var(--accent-hover, #0071e3);
  box-shadow: 0 2px 6px rgba(0, 122, 255, 0.4);
}

.apple-save-btn:active:not(:disabled) {
  transform: scale(0.98);
}

.apple-save-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
  box-shadow: none;
}

/* ===== 8. Apple Popover 磨砂下拉菜单 ===== */
.apple-popover-menu {
  position: fixed;
  z-index: 9999;
  background: rgba(255, 255, 255, 0.88);
  backdrop-filter: saturate(180%) blur(25px);
  -webkit-backdrop-filter: saturate(180%) blur(25px);
  border: 1px solid var(--border-color, rgba(0, 0, 0, 0.08));
  border-radius: 12px;
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.16), 0 2px 6px rgba(0, 0, 0, 0.06);
  padding: 4px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

:global([data-theme='dark']) .apple-popover-menu {
  background: rgba(36, 36, 38, 0.94);
  border-color: rgba(255, 255, 255, 0.12);
  box-shadow: 0 16px 40px rgba(0, 0, 0, 0.6), 0 2px 8px rgba(0, 0, 0, 0.3);
}

.apple-popover-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
  padding: 8px 12px;
  border-radius: 8px;
  font-size: 13.5px;
  font-weight: 400;
  color: var(--text-primary, #1d1d1f);
  background: transparent;
  border: none;
  cursor: pointer;
  text-align: left;
  transition: background-color 0.12s ease, color 0.12s ease;
  user-select: none;
}

.apple-popover-item:hover {
  background: var(--accent, #007aff);
  color: #ffffff !important;
}

.apple-popover-item:hover .apple-popover-check {
  color: #ffffff !important;
}

.apple-popover-check {
  color: var(--accent, #007aff);
  flex-shrink: 0;
}

/* Popover 弹入弹出动效 */
.popover-enter-active,
.popover-leave-active {
  transition: opacity 0.15s cubic-bezier(0.16, 1, 0.3, 1), transform 0.15s cubic-bezier(0.16, 1, 0.3, 1);
  transform-origin: top right;
}

.popover-enter-from,
.popover-leave-to {
  opacity: 0;
  transform: scale(0.95) translateY(-4px);
}
</style>
