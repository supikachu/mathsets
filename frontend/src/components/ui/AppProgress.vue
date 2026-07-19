<script setup lang="ts">
import { computed } from 'vue'
import { AppIcon } from '@/components/ui'

const props = withDefaults(
  defineProps<{
    /// 当前状态提示文字（例如"正在排队..."、"AI 正在燃烧算力解析中..."）
    statusText?: string
    /// 是否为错误状态（true 时显示错误图标，文字变红）
    isError?: boolean
    /// 是否为成功状态（true 时显示对勾图标，文字变绿）
    isSuccess?: boolean
    /// 图标尺寸
    size?: number | string
  }>(),
  {
    statusText: '',
    isError: false,
    isSuccess: false,
    size: 18,
  },
)

/// 根据状态选择图标名（参考 AppIcon 已有图标）
const iconName = computed<string>(() => {
  if (props.isError) return 'x-circle'
  if (props.isSuccess) return 'check-circle'
  return 'sparkles'
})

/// 根据状态选择 CSS 类（控制颜色）
const stateClass = computed<string>(() => {
  if (props.isError) return 'state-error'
  if (props.isSuccess) return 'state-success'
  return 'state-loading'
})
</script>

<template>
  <div class="app-progress" :class="stateClass">
    <!-- 旋转动画（仅在非成功/错误状态下显示） -->
    <div v-if="!isError && !isSuccess" class="spinner" :style="{ width: `${size}px`, height: `${size}px` }">
      <svg :width="size" :height="size" viewBox="0 0 24 24" fill="none">
        <circle
          cx="12"
          cy="12"
          r="9"
          stroke="currentColor"
          stroke-width="2.4"
          stroke-linecap="round"
          opacity="0.2"
        />
        <path
          d="M21 12a9 9 0 0 0-9-9"
          stroke="currentColor"
          stroke-width="2.4"
          stroke-linecap="round"
        />
      </svg>
    </div>
    <!-- 成功/错误状态：显示静态图标 -->
    <AppIcon v-else :name="iconName" :size="size" class="status-icon" />

    <!-- 状态文字（可选） -->
    <span v-if="statusText" class="status-text">{{ statusText }}</span>
  </div>
</template>

<style scoped>
.app-progress {
  display: inline-flex;
  align-items: center;
  gap: 0.625rem;
  font-size: 0.875rem;
  line-height: 1.25rem;
  color: var(--text-primary, #1f2937);
}

/* 旋转动画 */
.spinner {
  flex-shrink: 0;
  animation: app-progress-spin 0.9s linear infinite;
}

@keyframes app-progress-spin {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}

/* 加载中状态：使用主题色（紫色主调，呼应 AI 特性） */
.state-loading {
  color: var(--primary, #6366f1);
}

.state-loading .status-text {
  color: var(--text-secondary, #4b5563);
}

/* 错误状态：红色 */
.state-error {
  color: var(--danger, #ef4444);
}

.state-error .status-text {
  color: var(--danger, #ef4444);
}

/* 成功状态：绿色 */
.state-success {
  color: var(--success, #10b981);
}

.state-success .status-text {
  color: var(--success, #10b981);
}

.status-icon {
  flex-shrink: 0;
}

.status-text {
  font-weight: 500;
  word-break: break-word;
}

/* 尊重用户的动画偏好（无障碍） */
@media (prefers-reduced-motion: reduce) {
  .spinner {
    animation: none;
  }
}
</style>
