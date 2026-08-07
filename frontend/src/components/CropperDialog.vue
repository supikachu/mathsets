<template>
  <Teleport to="body">
    <div v-if="visible" class="cropper-overlay" @click.self="handleClose">
      <div class="cropper-dialog" @click.stop>
        <!-- 头部：macOS 极简标题栏 -->
        <div class="cropper-header">
          <span class="cropper-title">裁剪图片</span>
          <button class="close-btn" @click="handleClose" aria-label="关闭">
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
              <path d="M2 2L10 10M10 2L2 10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
            </svg>
          </button>
        </div>

        <!-- 主体：视口容器（520px 定高 + 24px 锚点安全缓冲内边距） -->
        <div class="cropper-dialog-body">
          <div v-if="!isOpened" class="cropper-loading">
            <div class="spinner"></div>
            <span>加载图片中…</span>
          </div>

          <!-- 只有在 Opened 动画/一帧完结后才渲染画布，避开尺寸计算为 0 的 Bug -->
          <cropper-canvas
            v-if="isOpened"
            :key="cropKey"
            ref="canvasEl"
            background
          >
            <!-- 添加 initial-center-size="contain"，且绝不给 cropper-image 加 CSS 宽高，完全交由 Transform 矩阵控制 -->
            <cropper-image
              ref="imageRef"
              :src="imageUrl"
              alt="Picture"
              initial-center-size="contain"
              rotatable
              scalable
              translatable
            ></cropper-image>

            <cropper-shade></cropper-shade>

            <!-- bounded 确保选区坐标严格锁在 Canvas 范围内 -->
            <cropper-selection
              initial-coverage="0.8"
              movable
              resizable
              bounded
            >
              <cropper-grid role="grid" covered></cropper-grid>
              <cropper-crosshair centered></cropper-crosshair>
              <cropper-handle action="move"></cropper-handle>
              <cropper-handle action="e-resize"></cropper-handle>
              <cropper-handle action="w-resize"></cropper-handle>
              <cropper-handle action="n-resize"></cropper-handle>
              <cropper-handle action="s-resize"></cropper-handle>
              <cropper-handle action="ne-resize"></cropper-handle>
              <cropper-handle action="nw-resize"></cropper-handle>
              <cropper-handle action="se-resize"></cropper-handle>
              <cropper-handle action="sw-resize"></cropper-handle>
            </cropper-selection>
          </cropper-canvas>
        </div>

        <!-- 底部：macOS 风格操作栏 -->
        <div class="cropper-footer">
          <button class="btn-cancel" @click="handleClose">取消</button>
          <button class="btn-confirm" @click="handleConfirm" :disabled="processing">
            <svg v-if="!processing" width="13" height="13" viewBox="0 0 14 14" fill="none">
              <path d="M2.5 7.5L5.5 10.5L11.5 3.5" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>
            </svg>
            <span>{{ processing ? '处理中...' : '应用裁剪' }}</span>
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { ref, watch, onBeforeUnmount, nextTick } from 'vue'
import 'cropperjs'

interface Props {
  visible?: boolean
  imageUrl?: string
}

const props = withDefaults(defineProps<Props>(), {
  visible: false,
  imageUrl: '',
})

const emit = defineEmits<{
  (e: 'update:visible', v: boolean): void
  (e: 'cropped', blob: Blob): void
}>()

const canvasEl = ref<HTMLElement>()
const processing = ref(false)
const isOpened = ref(false)
const cropKey = ref(0)

function handleClose() {
  isOpened.value = false
  emit('update:visible', false)
}

watch(
  () => props.visible,
  async (val) => {
    if (val) {
      cropKey.value++
      await nextTick()
      requestAnimationFrame(() => {
        isOpened.value = true
      })
      document.addEventListener('keydown', handleKeydown)
    } else {
      isOpened.value = false
      document.removeEventListener('keydown', handleKeydown)
    }
  },
  { immediate: true },
)

const handleConfirm = () => {
  if (processing.value || !canvasEl.value) return
  processing.value = true

  const selection = canvasEl.value.querySelector('cropper-selection') as any
  if (selection && typeof selection.$toCanvas === 'function') {
    selection
      .$toCanvas()
      .then((canvas: HTMLCanvasElement) => {
        canvas.toBlob(
          (blob) => {
            processing.value = false
            if (blob) {
              emit('cropped', blob)
              isOpened.value = false
              emit('update:visible', false)
            }
          },
          'image/png',
          0.95,
        )
      })
      .catch((e: any) => {
        processing.value = false
        console.error('[CropperDialog] $toCanvas failed:', e)
      })
  } else {
    processing.value = false
  }
}

function handleKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && props.visible) {
    handleClose()
  }
}

onBeforeUnmount(() => {
  document.removeEventListener('keydown', handleKeydown)
})
</script>

<style scoped>
.cropper-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.45);
  backdrop-filter: blur(16px) saturate(180%);
  -webkit-backdrop-filter: blur(16px) saturate(180%);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 10000;
  animation: cropper-fade-in 0.2s cubic-bezier(0.16, 1, 0.3, 1);
}

@keyframes cropper-fade-in {
  from {
    opacity: 0;
  }
  to {
    opacity: 1;
  }
}

.cropper-dialog {
  width: 90vw;
  max-width: 860px;
  background: rgba(255, 255, 255, 0.88);
  backdrop-filter: blur(28px) saturate(210%);
  -webkit-backdrop-filter: blur(28px) saturate(210%);
  border: 1px solid rgba(255, 255, 255, 0.65);
  border-radius: 18px;
  box-shadow:
    0 24px 64px -12px rgba(0, 0, 0, 0.28),
    0 8px 24px -4px rgba(0, 0, 0, 0.12),
    0 0 0 0.5px rgba(0, 0, 0, 0.1);
  display: flex;
  flex-direction: column;
  overflow: hidden;
  font-family: -apple-system, BlinkMacSystemFont, 'SF Pro Display', 'SF Pro Text', 'Helvetica Neue', Arial, sans-serif;
  user-select: none;
  animation: cropper-scale-in 0.22s cubic-bezier(0.16, 1, 0.3, 1);
}

@keyframes cropper-scale-in {
  from {
    opacity: 0;
    transform: scale(0.96) translateY(8px);
  }
  to {
    opacity: 1;
    transform: scale(1) translateY(0);
  }
}

.cropper-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 18px;
  background: rgba(255, 255, 255, 0.35);
  border-bottom: 0.5px solid rgba(0, 0, 0, 0.08);
  flex-shrink: 0;
}

.cropper-title {
  font-size: 14px;
  font-weight: 600;
  color: #1d1d1f;
  letter-spacing: -0.01em;
}

.close-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.04);
  border: 0.5px solid rgba(0, 0, 0, 0.05);
  width: 22px;
  height: 22px;
  border-radius: 50%;
  color: #6e6e73;
  cursor: pointer;
  transition: all 0.15s ease;
}

.close-btn:hover {
  background: rgba(0, 0, 0, 0.1);
  color: #1d1d1f;
  transform: scale(1.05);
}

/* 外层容器：设置 24px 内边距作为防溢出缓冲带 */
.cropper-dialog-body {
  width: 100%;
  height: 520px;
  background-color: #1a1a1a;
  background-image: linear-gradient(45deg, #262626 25%, transparent 25%),
    linear-gradient(-45deg, #262626 25%, transparent 25%),
    linear-gradient(45deg, transparent 75%, #262626 75%),
    linear-gradient(-45deg, transparent 75%, #262626 75%);
  background-size: 18px 18px;
  background-position: 0 0, 0 9px, 9px -9px, -9px 0px;
  border-radius: 12px;
  overflow: hidden;
  position: relative;
  padding: 24px; /* 关键：四周留出 24px 安全内边距 */
  box-sizing: border-box;
  display: flex;
  align-items: center;
  justify-content: center;
}

.cropper-loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  width: 100%;
  height: 100%;
  color: rgba(255, 255, 255, 0.7);
  font-size: 13px;
  font-weight: 500;
}

.spinner {
  width: 24px;
  height: 24px;
  border: 2px solid rgba(255, 255, 255, 0.2);
  border-top-color: #007aff;
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

/* 画布填满内层，允许锚点凸出渲染在 24px 缓冲带中 */
:deep(cropper-canvas) {
  width: 100%;
  height: 100%;
  display: block;
  overflow: visible !important; /* 关键：允许锚点凸出 Canvas */
}

/* ==========================================
   使用 :deep() 穿透样式
   ========================================== */

/* 1. 选区透明，无蓝雾 */
:deep(cropper-selection),
:deep(cropper-handle[action="move"]) {
  background-color: transparent !important;
  background: transparent !important;
}

/* 2. Stitch 风格白色高亮边框与圆角 */
:deep(cropper-selection) {
  border: 2px solid #ffffff !important;
  border-radius: 12px !important;
  box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.2), 0 8px 24px rgba(0, 0, 0, 0.4) !important;
  outline: none !important;
}

/* 3. 精致手柄：苹果蓝 + 白色外边框的圆形锚点 */
:deep(cropper-handle:not([action="move"])) {
  background-color: #007aff !important;
  border: 2px solid #ffffff !important;
  width: 12px !important;
  height: 12px !important;
  border-radius: 50% !important;
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.3) !important;
}

/* 4. 暗色遮罩透明度 */
:deep(cropper-shade) {
  background-color: rgba(0, 0, 0, 0.55) !important;
}

/* 网格辅助线 */
:deep(cropper-grid),
:deep(cropper-crosshair) {
  opacity: 0.35 !important;
  --color: rgba(255, 255, 255, 0.8) !important;
}

/* 底部操作栏 */
.cropper-footer {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 10px;
  padding: 12px 18px;
  background: rgba(255, 255, 255, 0.35);
  border-top: 0.5px solid rgba(0, 0, 0, 0.08);
  flex-shrink: 0;
}

.btn-cancel,
.btn-confirm {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 6px 16px;
  height: 32px;
  border-radius: 8px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  transition: all 0.15s ease;
  letter-spacing: -0.01em;
}

.btn-cancel {
  background: rgba(0, 0, 0, 0.05);
  border: 0.5px solid rgba(0, 0, 0, 0.08);
  color: #1d1d1f;
}

.btn-cancel:hover {
  background: rgba(0, 0, 0, 0.09);
  color: #000000;
}

.btn-cancel:active {
  transform: scale(0.97);
}

.btn-confirm {
  background: linear-gradient(180deg, #007aff 0%, #0071e3 100%);
  border: none;
  color: #ffffff;
  box-shadow:
    0 3px 10px rgba(0, 122, 255, 0.32),
    inset 0 1px 0 rgba(255, 255, 255, 0.2);
}

.btn-confirm:hover:not(:disabled) {
  background: linear-gradient(180deg, #0077ff 0%, #006bd8 100%);
  box-shadow:
    0 5px 14px rgba(0, 122, 255, 0.4),
    inset 0 1px 0 rgba(255, 255, 255, 0.25);
  transform: translateY(-0.5px);
}

.btn-confirm:active:not(:disabled) {
  transform: scale(0.97);
  box-shadow: 0 2px 6px rgba(0, 122, 255, 0.3);
}

.btn-confirm:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* 深色模式支持 */
[data-theme='dark'] .cropper-dialog {
  background: rgba(28, 28, 30, 0.82);
  border-color: rgba(255, 255, 255, 0.12);
  box-shadow:
    0 24px 72px -12px rgba(0, 0, 0, 0.75),
    0 8px 28px -4px rgba(0, 0, 0, 0.5),
    0 0 0 0.5px rgba(255, 255, 255, 0.1);
}

[data-theme='dark'] .cropper-header {
  background: rgba(255, 255, 255, 0.03);
  border-bottom-color: rgba(255, 255, 255, 0.08);
}

[data-theme='dark'] .cropper-title {
  color: #f5f5f7;
}

[data-theme='dark'] .close-btn {
  background: rgba(255, 255, 255, 0.08);
  border-color: rgba(255, 255, 255, 0.1);
  color: #a1a1a6;
}

[data-theme='dark'] .close-btn:hover {
  background: rgba(255, 255, 255, 0.16);
  color: #ffffff;
}

[data-theme='dark'] .cropper-footer {
  background: rgba(255, 255, 255, 0.03);
  border-top-color: rgba(255, 255, 255, 0.08);
}

[data-theme='dark'] .btn-cancel {
  background: rgba(255, 255, 255, 0.08);
  border-color: rgba(255, 255, 255, 0.12);
  color: #f5f5f7;
}

[data-theme='dark'] .btn-cancel:hover {
  background: rgba(255, 255, 255, 0.15);
  color: #ffffff;
}

[data-theme='dark'] .btn-confirm {
  background: linear-gradient(180deg, #0a84ff 0%, #0071e3 100%);
  box-shadow:
    0 4px 14px rgba(10, 132, 255, 0.35),
    inset 0 1px 0 rgba(255, 255, 255, 0.2);
}

[data-theme='dark'] .btn-confirm:hover:not(:disabled) {
  background: linear-gradient(180deg, #1a8eff 0%, #0077ed 100%);
  box-shadow:
    0 6px 18px rgba(10, 132, 255, 0.45),
    inset 0 1px 0 rgba(255, 255, 255, 0.25);
}
</style>
