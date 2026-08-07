<template>
  <Teleport to="body">
    <div
      v-if="visible && imageData"
      ref="panelEl"
      class="mac-panel"
      :style="panelStyle"
      :class="{ dragging: isDragging }"
      @click.stop
    >
      <!-- 头部（可拖拽，简洁极简风格） -->
      <div class="mac-header" @mousedown="startDrag">
        <div class="header-left">
          <span class="header-title">图片设置</span>
        </div>
        <div class="header-right">
          <span class="size-tag">{{ localConfig.width ? `${localConfig.width} px` : '自动宽度' }}</span>
          <button class="close-btn" @click.stop="emit('close')" aria-label="关闭">
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
              <path d="M2 2L10 10M10 2L2 10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
            </svg>
          </button>
        </div>
      </div>

      <div class="mac-body">
        <!-- 宽度调节 -->
        <div class="setting-row">
          <label class="row-label">宽度</label>
          <div class="control-group">
            <div class="slider-row">
              <input
                type="range"
                :min="50"
                :max="1200"
                :step="10"
                :value="localConfig.width || 400"
                @input="updateWidth(($event.target as HTMLInputElement).value)"
                class="mac-slider"
              />
              <div class="input-wrapper">
                <input
                  type="number"
                  :value="localConfig.width || ''"
                  :min="50"
                  :max="1200"
                  placeholder="自动"
                  @change="updateWidth(($event.target as HTMLInputElement).value)"
                  class="mac-input"
                />
                <span class="unit-label">px</span>
                <!-- 精致定制微型上下增加/减少 Stepper 按钮 -->
                <div class="custom-spinners">
                  <button type="button" class="spin-btn spin-up" @click.stop="stepWidth(10)" title="增加 10px">
                    <svg width="7" height="4" viewBox="0 0 7 4" fill="none">
                      <path d="M1 3.5L3.5 1L6 3.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>
                    </svg>
                  </button>
                  <button type="button" class="spin-btn spin-down" @click.stop="stepWidth(-10)" title="减少 10px">
                    <svg width="7" height="4" viewBox="0 0 7 4" fill="none">
                      <path d="M1 0.5L3.5 3L6 0.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>
                    </svg>
                  </button>
                </div>
              </div>
            </div>
            <div class="step-group">
              <button class="step-pill" @click="stepWidth(-50)">−50</button>
              <button class="step-pill" @click="stepWidth(-10)">−10</button>
              <button class="step-pill" @click="stepWidth(10)">+10</button>
              <button class="step-pill" @click="stepWidth(50)">+50</button>
              <button class="reset-link" @click="resetWidth">恢复默认</button>
            </div>
          </div>
        </div>

        <!-- 对齐方式：苹果分段控件 -->
        <div class="setting-row">
          <label class="row-label">对齐</label>
          <div class="segmented">
            <button
              v-for="opt in alignOptions"
              :key="opt.value"
              :class="['seg-item', { active: localConfig.align === opt.value }]"
              @click="updateAlign(opt.value)"
            >
              <svg v-if="opt.value === 'left'" width="14" height="14" viewBox="0 0 14 14"><rect x="1" y="2" width="12" height="2" rx="1" fill="currentColor"/><rect x="1" y="6" width="8" height="2" rx="1" fill="currentColor"/><rect x="1" y="10" width="10" height="2" rx="1" fill="currentColor"/></svg>
              <svg v-else-if="opt.value === 'center'" width="14" height="14" viewBox="0 0 14 14"><rect x="1" y="2" width="12" height="2" rx="1" fill="currentColor"/><rect x="3" y="6" width="8" height="2" rx="1" fill="currentColor"/><rect x="2" y="10" width="10" height="2" rx="1" fill="currentColor"/></svg>
              <svg v-else width="14" height="14" viewBox="0 0 14 14"><rect x="1" y="2" width="12" height="2" rx="1" fill="currentColor"/><rect x="5" y="6" width="8" height="2" rx="1" fill="currentColor"/><rect x="3" y="10" width="10" height="2" rx="1" fill="currentColor"/></svg>
              <span class="seg-text">{{ opt.label }}</span>
            </button>
          </div>
        </div>

        <!-- 裁剪按钮 -->
        <button class="crop-action" @click="emit('crop-request', { url: imageData.url, mdId: imageData.mdId })">
          <svg width="15" height="15" viewBox="0 0 16 16" fill="none">
            <path d="M4 1V11M4 11H14M4 11L1 8M11 4V14M11 4L14 1" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" transform="translate(0,0.5)"/>
          </svg>
          <span>裁剪图片</span>
        </button>
      </div>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { ref, watch, nextTick, onBeforeUnmount, reactive, computed } from 'vue'
import type { ImageConfig } from './LatexRender.vue'

interface Props {
  visible: boolean
  target: HTMLElement | null
  imageData: {
    url: string
    mdId: string
    config: ImageConfig
  } | null
}

const props = defineProps<Props>()

const emit = defineEmits<{
  (e: 'update-config', payload: { mdId: string; configString: string }): void
  (e: 'crop-request', payload: { url: string; mdId: string }): void
  (e: 'close'): void
}>()

const panelEl = ref<HTMLElement>()
const positionStyle = ref<Record<string, string>>({})
const localConfig = reactive<ImageConfig>({})

// ============================================================
// 拖拽逻辑：transform: translate(x, y) 平移面板
// ============================================================
const dragOffset = ref({ x: 0, y: 0 })
const isDragging = ref(false)
let dragStart = { x: 0, y: 0, offsetX: 0, offsetY: 0 }

/** 面板最终样式 = 基础定位 + 拖拽位移 */
const panelStyle = computed(() => ({
  ...positionStyle.value,
  transform: `translate(${dragOffset.value.x}px, ${dragOffset.value.y}px)`,
}))

function startDrag(e: MouseEvent) {
  // 点击关闭按钮时不触发拖拽
  if ((e.target as HTMLElement).closest('.close-btn')) return
  isDragging.value = true
  dragStart = {
    x: e.clientX,
    y: e.clientY,
    offsetX: dragOffset.value.x,
    offsetY: dragOffset.value.y,
  }
  document.addEventListener('mousemove', onDragMove)
  document.addEventListener('mouseup', stopDrag)
  e.preventDefault()
}

function onDragMove(e: MouseEvent) {
  if (!isDragging.value) return
  dragOffset.value = {
    x: dragStart.offsetX + (e.clientX - dragStart.x),
    y: dragStart.offsetY + (e.clientY - dragStart.y),
  }
}

function stopDrag() {
  isDragging.value = false
  document.removeEventListener('mousemove', onDragMove)
  document.removeEventListener('mouseup', stopDrag)
}

const alignOptions = [
  { value: 'left' as const, label: '居左' },
  { value: 'center' as const, label: '居中' },
  { value: 'right' as const, label: '居右' },
]

// ============================================================
// 滚动感知定位
// ============================================================

let scrollAncestor: HTMLElement | null = null
let rafId: number | null = null

function findScrollableAncestor(el: HTMLElement | null): HTMLElement | null {
  if (!el) return null
  let node = el.parentElement
  while (node && node !== document.body) {
    const style = window.getComputedStyle(node)
    const overflowY = style.overflowY
    if ((overflowY === 'auto' || overflowY === 'scroll') && node.scrollHeight > node.clientHeight) {
      return node
    }
    node = node.parentElement
  }
  return null
}

function updatePosition() {
  if (!props.target || !panelEl.value) return
  const rect = props.target.getBoundingClientRect()
  const panelWidth = 320
  const panelHeight = panelEl.value.offsetHeight || 370
  const gap = 12

  let left = rect.left
  if (left + panelWidth > window.innerWidth - 20) {
    left = window.innerWidth - panelWidth - 20
  }
  left = Math.max(20, left)

  let top = rect.bottom + gap
  if (top + panelHeight > window.innerHeight - 20) {
    const aboveTop = rect.top - panelHeight - gap
    top = aboveTop > 20 ? aboveTop : Math.max(20, window.innerHeight - panelHeight - 20)
  }

  positionStyle.value = {
    position: 'fixed',
    left: `${left}px`,
    top: `${top}px`,
    width: `${panelWidth}px`,
    zIndex: '9999',
  }
}

function scheduleUpdate() {
  if (rafId !== null) return
  rafId = requestAnimationFrame(() => {
    rafId = null
    updatePosition()
  })
}

function attachScrollListeners() {
  scrollAncestor = findScrollableAncestor(props.target)
  if (scrollAncestor) {
    scrollAncestor.addEventListener('scroll', scheduleUpdate, { passive: true })
  }
  window.addEventListener('resize', scheduleUpdate)
}

function detachScrollListeners() {
  if (scrollAncestor) {
    scrollAncestor.removeEventListener('scroll', scheduleUpdate)
    scrollAncestor = null
  }
  window.removeEventListener('resize', scheduleUpdate)
  if (rafId !== null) {
    cancelAnimationFrame(rafId)
    rafId = null
  }
}

watch(
  () => [props.visible, props.target],
  async ([visible]) => {
    if (!visible || !props.target) return
    await nextTick()
    if (!panelEl.value || !props.target) return
    updatePosition()
    attachScrollListeners()
  },
  { immediate: true },
)

watch(
  () => props.visible,
  (v) => {
    if (v) {
      document.addEventListener('click', handleDocumentClick)
      document.addEventListener('keydown', handleKeydown)
    } else {
      document.removeEventListener('click', handleDocumentClick)
      document.removeEventListener('keydown', handleKeydown)
      detachScrollListeners()
      dragOffset.value = { x: 0, y: 0 }
    }
  },
)

onBeforeUnmount(() => {
  document.removeEventListener('click', handleDocumentClick)
  document.removeEventListener('keydown', handleKeydown)
  detachScrollListeners()
  document.removeEventListener('mousemove', onDragMove)
  document.removeEventListener('mouseup', stopDrag)
})

watch(
  () => props.imageData,
  (data) => {
    if (data) {
      localConfig.width = data.config.width
      localConfig.align = data.config.align
    }
  },
  { immediate: true },
)

function handleDocumentClick(e: MouseEvent) {
  if (!props.visible) return
  const target = e.target as Node
  if (panelEl.value?.contains(target)) return
  if (props.target?.contains(target)) return
  emit('close')
}

function handleKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && props.visible) {
    emit('close')
  }
}

function buildConfigString(config: ImageConfig): string {
  const parts: string[] = []
  if (config.width) parts.push(`width:${config.width}`)
  if (config.align) parts.push(`align:${config.align}`)
  return parts.length > 0 ? `{${parts.join(', ')}}` : ''
}

function updateWidth(value: string | number) {
  const width = typeof value === 'string' ? parseInt(value, 10) : value
  localConfig.width = isNaN(width) || width < 50 ? undefined : Math.min(1200, width)
  emitUpdate()
}

function updateAlign(align: 'left' | 'center' | 'right') {
  localConfig.align = align
  emitUpdate()
}

function stepWidth(delta: number) {
  const current = localConfig.width || 400
  const next = Math.max(50, Math.min(1200, current + delta))
  localConfig.width = next
  emitUpdate()
}

function resetWidth() {
  localConfig.width = undefined
  emitUpdate()
}

function emitUpdate() {
  if (!props.imageData) return
  emit('update-config', {
    mdId: props.imageData.mdId,
    configString: buildConfigString(localConfig),
  })
}
</script>

<style>
/* ============================================================
 * macOS Sequoia 顶级毛玻璃透光浮点弹窗
 * ============================================================ */
.mac-panel {
  background: rgba(255, 255, 255, 0.72);
  backdrop-filter: blur(28px) saturate(210%);
  -webkit-backdrop-filter: blur(28px) saturate(210%);
  border: 1px solid rgba(255, 255, 255, 0.65);
  border-radius: 18px;
  box-shadow:
    0 16px 48px -8px rgba(0, 0, 0, 0.14),
    0 4px 16px -2px rgba(0, 0, 0, 0.06),
    0 0 0 0.5px rgba(0, 0, 0, 0.1);
  overflow: hidden;
  font-family: -apple-system, BlinkMacSystemFont, 'SF Pro Display', 'SF Pro Text', 'Helvetica Neue', Arial, sans-serif;
  font-size: 13px;
  user-select: none;
  transition: box-shadow 0.25s cubic-bezier(0.16, 1, 0.3, 1), transform 0.08s linear;
}

.mac-panel.dragging {
  box-shadow:
    0 24px 64px -12px rgba(0, 0, 0, 0.22),
    0 8px 24px -4px rgba(0, 0, 0, 0.1),
    0 0 0 0.5px rgba(0, 0, 0, 0.15);
}

/* 头部：可拖拽区域 */
.mac-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 11px 14px;
  background: rgba(255, 255, 255, 0.3);
  border-bottom: 0.5px solid rgba(0, 0, 0, 0.08);
  cursor: grab;
}

.mac-header:active,
.mac-panel.dragging .mac-header {
  cursor: grabbing;
}

.header-left {
  display: flex;
  align-items: center;
  gap: 8px;
}

.header-title {
  font-weight: 600;
  font-size: 13px;
  color: #1d1d1f;
  letter-spacing: -0.012em;
}

.header-right {
  display: flex;
  align-items: center;
  gap: 8px;
}

.size-tag {
  font-size: 11px;
  font-weight: 600;
  color: #6e6e73;
  background: rgba(0, 0, 0, 0.04);
  border: 0.5px solid rgba(0, 0, 0, 0.06);
  border-radius: 6px;
  padding: 2px 7px;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.01em;
}

.close-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.04);
  border: 0.5px solid rgba(0, 0, 0, 0.05);
  width: 20px;
  height: 20px;
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

/* 内容区 */
.mac-body {
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.setting-row {
  display: flex;
  align-items: flex-start;
  gap: 12px;
}

.row-label {
  flex-shrink: 0;
  width: 32px;
  font-size: 12px;
  font-weight: 600;
  color: #6e6e73;
  padding-top: 6px;
  letter-spacing: -0.01em;
}

.control-group {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}

/* 滑块行 */
.slider-row {
  display: flex;
  align-items: center;
  gap: 10px;
  outline: none !important;
  border: none !important;
}

/* 苹果风滑块 */
.mac-slider {
  flex: 1;
  height: 5px;
  background: rgba(0, 0, 0, 0.08);
  border-radius: 999px;
  appearance: none;
  -webkit-appearance: none;
  outline: none !important;
  border: none !important;
  cursor: pointer;
  transition: background 0.15s ease;
}

.mac-slider:hover {
  background: rgba(0, 0, 0, 0.12);
}

/* WebKit 滑块球 */
.mac-slider::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 17px;
  height: 17px;
  background: #ffffff;
  border: 0.5px solid rgba(0, 0, 0, 0.08);
  border-radius: 50%;
  cursor: pointer;
  box-shadow:
    0 2px 5px rgba(0, 0, 0, 0.24),
    0 0.5px 1.5px rgba(0, 0, 0, 0.12);
  transition: transform 0.12s cubic-bezier(0.16, 1, 0.3, 1), box-shadow 0.12s ease;
}

.mac-slider::-webkit-slider-thumb:hover {
  transform: scale(1.08);
  box-shadow:
    0 3px 8px rgba(0, 0, 0, 0.28),
    0 1px 2px rgba(0, 0, 0, 0.15);
}

.mac-slider::-webkit-slider-thumb:active {
  transform: scale(1.15);
}

/* Firefox 滑块轨与球 */
.mac-slider::-moz-range-thumb {
  width: 17px;
  height: 17px;
  background: #ffffff;
  border: 0.5px solid rgba(0, 0, 0, 0.08);
  border-radius: 50%;
  cursor: pointer;
  box-shadow:
    0 2px 5px rgba(0, 0, 0, 0.24),
    0 0.5px 1.5px rgba(0, 0, 0, 0.12);
}

.mac-slider::-moz-range-track {
  height: 5px;
  background: rgba(0, 0, 0, 0.08);
  border-radius: 999px;
  border: none;
}

.mac-slider::-moz-range-progress {
  height: 5px;
  background: #007aff;
  border-radius: 999px;
  border: none;
}

/* 彻底隐藏原生 input[type=number] 上下粗糙箭头 */
.mac-input::-webkit-inner-spin-button,
.mac-input::-webkit-outer-spin-button {
  -webkit-appearance: none;
  appearance: none;
  margin: 0;
}
.mac-input[type='number'] {
  -moz-appearance: textfield;
}

/* 数字输入框包装容器 */
.input-wrapper {
  position: relative;
  display: flex;
  align-items: center;
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.04);
  border: 0.5px solid rgba(0, 0, 0, 0.08);
  transition: all 0.15s ease;
  overflow: hidden;
}

.input-wrapper:hover {
  background: rgba(0, 0, 0, 0.06);
  border-color: rgba(0, 0, 0, 0.12);
}

.input-wrapper:focus-within {
  background: #ffffff;
  border-color: #007aff;
  box-shadow: 0 0 0 3px rgba(0, 122, 255, 0.22);
}

.mac-input {
  width: 48px;
  padding: 5px 22px 5px 8px;
  background: transparent;
  border: none;
  font-size: 12px;
  font-weight: 600;
  text-align: right;
  color: #1d1d1f;
  font-variant-numeric: tabular-nums;
  outline: none;
}

.unit-label {
  position: absolute;
  right: 18px;
  font-size: 11px;
  font-weight: 500;
  color: #86868b;
  pointer-events: none;
}

/* macOS 风格精细定制微型上下 Stepper 按钮 (Custom Spinners) */
.custom-spinners {
  display: flex;
  flex-direction: column;
  justify-content: center;
  align-items: center;
  width: 15px;
  height: 24px;
  margin-right: 2px;
  border-left: 0.5px solid rgba(0, 0, 0, 0.06);
}

.spin-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  height: 11px;
  border: none;
  background: transparent;
  color: #6e6e73;
  cursor: pointer;
  padding: 0;
  transition: color 0.15s ease, background 0.15s ease;
  border-radius: 2px;
}

.spin-btn:hover {
  color: #1d1d1f;
  background: rgba(0, 0, 0, 0.08);
}

.spin-btn:active {
  transform: scale(0.9);
}

/* 步进按钮组：苹果极简气泡 */
.step-group {
  display: flex;
  align-items: center;
  gap: 4px;
}

.step-pill {
  flex: 1;
  padding: 5px 0;
  border: 0.5px solid rgba(0, 0, 0, 0.06);
  background: rgba(0, 0, 0, 0.04);
  color: #515154;
  border-radius: 7px;
  font-size: 11px;
  font-weight: 600;
  cursor: pointer;
  transition: all 0.15s ease;
  font-variant-numeric: tabular-nums;
}

.step-pill:hover {
  background: rgba(0, 0, 0, 0.09);
  color: #1d1d1f;
  transform: translateY(-0.5px);
}

.step-pill:active {
  transform: scale(0.95);
}

.reset-link {
  border: none;
  background: none;
  color: #007aff;
  font-size: 11px;
  font-weight: 600;
  cursor: pointer;
  padding: 5px 8px;
  transition: opacity 0.15s ease;
}

.reset-link:hover {
  opacity: 0.7;
}

/* 苹果分段控件 */
.segmented {
  flex: 1;
  display: flex;
  gap: 2px;
  padding: 3px;
  background: rgba(0, 0, 0, 0.05);
  border: 0.5px solid rgba(0, 0, 0, 0.06);
  border-radius: 10px;
}

.seg-item {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  padding: 6px 8px;
  border: none;
  background: transparent;
  color: #6e6e73;
  border-radius: 7px;
  font-size: 12px;
  cursor: pointer;
  transition: all 0.18s cubic-bezier(0.16, 1, 0.3, 1);
}

.seg-item:hover:not(.active) {
  color: #1d1d1f;
  background: rgba(0, 0, 0, 0.03);
}

.seg-item.active {
  background: #ffffff;
  color: #1d1d1f;
  box-shadow:
    0 2px 6px rgba(0, 0, 0, 0.12),
    0 0.5px 1.5px rgba(0, 0, 0, 0.06);
}

.seg-text {
  font-weight: 600;
  letter-spacing: -0.01em;
}

/* 裁剪按钮 */
.crop-action {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  width: 100%;
  padding: 10px;
  border: none;
  background: linear-gradient(180deg, #007aff 0%, #0071e3 100%);
  color: #ffffff;
  border-radius: 10px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  box-shadow:
    0 4px 12px rgba(0, 122, 255, 0.3),
    inset 0 1px 0 rgba(255, 255, 255, 0.2);
  transition: all 0.15s ease;
}

.crop-action:hover {
  background: linear-gradient(180deg, #0077ff 0%, #006bd8 100%);
  box-shadow:
    0 6px 16px rgba(0, 122, 255, 0.38),
    inset 0 1px 0 rgba(255, 255, 255, 0.25);
  transform: translateY(-0.5px);
}

.crop-action:active {
  transform: scale(0.98);
  box-shadow: 0 2px 6px rgba(0, 122, 255, 0.3);
}

/* ============================================================
 * 深色模式
 * ============================================================ */
[data-theme='dark'] .mac-panel {
  background: rgba(28, 28, 30, 0.76);
  backdrop-filter: blur(30px) saturate(210%);
  -webkit-backdrop-filter: blur(30px) saturate(210%);
  border: 1px solid rgba(255, 255, 255, 0.12);
  box-shadow:
    0 20px 60px -12px rgba(0, 0, 0, 0.65),
    0 4px 20px -2px rgba(0, 0, 0, 0.4),
    0 0 0 0.5px rgba(255, 255, 255, 0.1);
}

[data-theme='dark'] .mac-panel.dragging {
  box-shadow:
    0 28px 72px -12px rgba(0, 0, 0, 0.75),
    0 8px 28px -4px rgba(0, 0, 0, 0.5),
    0 0 0 0.5px rgba(255, 255, 255, 0.15);
}

[data-theme='dark'] .mac-header {
  background: rgba(255, 255, 255, 0.03);
  border-bottom-color: rgba(255, 255, 255, 0.08);
}

[data-theme='dark'] .header-title {
  color: #f5f5f7;
}

[data-theme='dark'] .size-tag {
  background: rgba(255, 255, 255, 0.08);
  border-color: rgba(255, 255, 255, 0.1);
  color: #a1a1a6;
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

[data-theme='dark'] .row-label {
  color: #a1a1a6;
}

[data-theme='dark'] .mac-slider {
  background: rgba(255, 255, 255, 0.12);
}

[data-theme='dark'] .mac-slider:hover {
  background: rgba(255, 255, 255, 0.18);
}

[data-theme='dark'] .mac-slider::-webkit-slider-thumb {
  background: #f5f5f7;
  border-color: rgba(255, 255, 255, 0.2);
  box-shadow:
    0 2px 6px rgba(0, 0, 0, 0.5),
    0 0.5px 1.5px rgba(0, 0, 0, 0.3);
}

[data-theme='dark'] .mac-slider::-moz-range-thumb {
  background: #f5f5f7;
  border-color: rgba(255, 255, 255, 0.2);
  box-shadow:
    0 2px 6px rgba(0, 0, 0, 0.5),
    0 0.5px 1.5px rgba(0, 0, 0, 0.3);
}

[data-theme='dark'] .mac-slider::-moz-range-track {
  background: rgba(255, 255, 255, 0.12);
}

[data-theme='dark'] .mac-slider::-moz-range-progress {
  background: #0a84ff;
}

[data-theme='dark'] .input-wrapper {
  background: rgba(255, 255, 255, 0.08);
  border-color: rgba(255, 255, 255, 0.12);
}

[data-theme='dark'] .input-wrapper:hover {
  background: rgba(255, 255, 255, 0.12);
}

[data-theme='dark'] .input-wrapper:focus-within {
  background: rgba(255, 255, 255, 0.16);
  border-color: #0a84ff;
  box-shadow: 0 0 0 3px rgba(10, 132, 255, 0.3);
}

[data-theme='dark'] .mac-input {
  color: #f5f5f7;
}

[data-theme='dark'] .custom-spinners {
  border-left-color: rgba(255, 255, 255, 0.08);
}

[data-theme='dark'] .spin-btn {
  color: #a1a1a6;
}

[data-theme='dark'] .spin-btn:hover {
  color: #ffffff;
  background: rgba(255, 255, 255, 0.15);
}

[data-theme='dark'] .unit-label {
  color: #86868b;
}

[data-theme='dark'] .step-pill {
  background: rgba(255, 255, 255, 0.08);
  border-color: rgba(255, 255, 255, 0.1);
  color: #a1a1a6;
}

[data-theme='dark'] .step-pill:hover {
  background: rgba(255, 255, 255, 0.15);
  color: #ffffff;
}

[data-theme='dark'] .reset-link {
  color: #0a84ff;
}

[data-theme='dark'] .segmented {
  background: rgba(0, 0, 0, 0.25);
  border-color: rgba(255, 255, 255, 0.08);
}

[data-theme='dark'] .seg-item {
  color: #86868b;
}

[data-theme='dark'] .seg-item:hover:not(.active) {
  color: #f5f5f7;
  background: rgba(255, 255, 255, 0.05);
}

[data-theme='dark'] .seg-item.active {
  background: rgba(255, 255, 255, 0.18);
  color: #ffffff;
  box-shadow:
    0 2px 8px rgba(0, 0, 0, 0.4),
    0 0.5px 1.5px rgba(255, 255, 255, 0.1);
}

[data-theme='dark'] .crop-action {
  background: linear-gradient(180deg, #0a84ff 0%, #0071e3 100%);
  box-shadow:
    0 4px 14px rgba(10, 132, 255, 0.35),
    inset 0 1px 0 rgba(255, 255, 255, 0.2);
}

[data-theme='dark'] .crop-action:hover {
  background: linear-gradient(180deg, #1a8eff 0%, #0077ed 100%);
  box-shadow:
    0 6px 18px rgba(10, 132, 255, 0.45),
    inset 0 1px 0 rgba(255, 255, 255, 0.25);
}
</style>
