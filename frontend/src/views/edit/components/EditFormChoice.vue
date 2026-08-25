<script setup lang="ts">
import { computed, nextTick } from 'vue'
import { AppIcon } from '@/components/ui'
import { useToast } from '@/composables/useToast'

const options = defineModel<{ label: string; content: string }[]>('options', { required: true })
const correctAnswer = defineModel<any>('correctAnswer', { required: true })
const subType = defineModel<string>('subType', { required: true })

const toast = useToast()

const isMultiChoice = computed(() => subType.value === 'multi')

// 当前选中的 label 集合（统一为数组处理）
const selectedLabels = computed<string[]>({
  get() {
    if (Array.isArray(correctAnswer.value)) return [...correctAnswer.value]
    return correctAnswer.value ? [correctAnswer.value] : []
  },
  set(val: string[]) {
    const sorted = [...val].sort()
    if (isMultiChoice.value) {
      correctAnswer.value = sorted
    } else {
      correctAnswer.value = sorted[0] || ''
    }
  },
})

function isAnswerSelected(label: string): boolean {
  return selectedLabels.value.includes(label)
}

// 点击 A/B/C/D 快捷按钮切换答案
function toggleAnswer(label: string) {
  const arr = selectedLabels.value
  if (isMultiChoice.value) {
    if (arr.includes(label)) {
      selectedLabels.value = arr.filter(l => l !== label)
    } else {
      selectedLabels.value = [...arr, label]
    }
  } else {
    selectedLabels.value = [label]
  }
}

// 答案文本框：统一包裹在单个 $\mathrm{...}$ 中
// 单选 B → $\mathrm{B}$，多选 A+C → $\mathrm{AC}$
const answerText = computed({
  get() {
    const labels = selectedLabels.value
    if (labels.length === 0) return ''
    return `$\\mathrm{${labels.join('')}}$`
  },
  set(val: string) {
    // 解析用户手动输入：从 $\mathrm{XY}$ 中提取字母
    const match = val.match(/\\mathrm\{([A-Za-z]+)\}/)
    if (match) {
      const letters = match[1].toUpperCase().split('')
      selectedLabels.value = letters
      return
    }
    // 兜底：直接按逗号/空格分割原始字母
    const parts = val.trim().split(/[,，、\s]+/).filter(Boolean)
    if (parts.length === 0) {
      selectedLabels.value = []
    } else {
      selectedLabels.value = parts.map(p => p.trim().toUpperCase().charAt(0))
    }
  },
})

function addOption() {
  const nextLabel = String.fromCharCode(65 + options.value.length)
  options.value.push({ label: nextLabel, content: '' })
}

// 粘贴图片自动分割
function onOptionPaste(e: ClipboardEvent, index: number) {
  const items = e.clipboardData?.items
  if (!items) return
  for (const item of items) {
    if (item.type.startsWith('image/')) {
      e.preventDefault()
      const file = item.getAsFile()
      if (!file) return
      if (file.size > 5 * 1024 * 1024) {
        toast.error('图片不能超过 5MB')
        return
      }
      const imageUrl = URL.createObjectURL(file)
      const inp = e.target as HTMLInputElement
      const pos = inp.selectionStart ?? 0
      const before = options.value[index].content.substring(0, pos)
      const after = options.value[index].content.substring(inp.selectionEnd ?? 0)
      const insert = `![选项配图](${imageUrl})`
      options.value[index].content = before + insert + after
      nextTick(() => {
        inp.focus()
        const newPos = pos + insert.length
        inp.setSelectionRange(newPos, newPos)
      })
      break
    }
  }
}

// 选项图片上传
function handleOptionImageUpload(index: number) {
  const input = document.createElement('input')
  input.type = 'file'
  input.accept = 'image/png,image/jpeg,image/gif,image/webp'
  input.onchange = async () => {
    const file = input.files?.[0]
    if (!file) return
    if (file.size > 5 * 1024 * 1024) {
      toast.error('图片不能超过 5MB')
      return
    }
    const imageUrl = URL.createObjectURL(file)
    const inp = document.querySelectorAll<HTMLInputElement>('.opt-card-input')[index]
    if (!inp) {
      options.value[index].content += `![选项配图](${imageUrl})`
      return
    }
    const pos = inp.selectionStart ?? 0
    const before = options.value[index].content.substring(0, pos)
    const after = options.value[index].content.substring(inp.selectionEnd ?? 0)
    const insert = `![选项配图](${imageUrl})`
    options.value[index].content = before + insert + after
    nextTick(() => {
      inp.focus()
      const newPos = pos + insert.length
      inp.setSelectionRange(newPos, newPos)
    })
  }
  input.click()
}
</script>

<template>
  <div class="choice-editor">
    <!-- 1. 选项编辑网格 (2列) -->
    <div class="options-grid">
      <div
        v-for="(opt, i) in options"
        :key="i"
        class="opt-card"
        :class="{ correct: isAnswerSelected(opt.label) }"
      >
        <div class="opt-header">
          <span class="opt-title">选项 {{ opt.label }}</span>
          <div class="opt-actions">
            <button type="button" class="opt-icon-btn" @click="handleOptionImageUpload(i)" title="上传配图">
              <AppIcon name="paperclip" :size="13" />
            </button>
            <button v-if="options.length > 2" type="button" class="opt-icon-btn opt-delete-btn" @click="options.splice(i, 1)" title="删除选项">
              <AppIcon name="x" :size="13" />
            </button>
          </div>
        </div>
        <input
          v-model="opt.content"
          :placeholder="`选项 ${opt.label} 内容...`"
          class="opt-card-input"
          @paste="onOptionPaste($event, i)"
        />
      </div>
    </div>

    <button type="button" class="add-btn" @click="addOption">
      <AppIcon name="plus" :size="14" /> 添加选项
    </button>

    <!-- 2. 答案（结果）模块 -->
    <div class="answer-card">
      <div class="answer-header">
        <span class="answer-title">答案（结果）</span>
        <div class="quick-answer-btns">
          <button
            v-for="(opt, i) in options"
            :key="i"
            type="button"
            class="quick-btn"
            :class="{ active: isAnswerSelected(opt.label) }"
            @click="toggleAnswer(opt.label)"
          >{{ opt.label }}</button>
        </div>
      </div>
      <input
        v-model="answerText"
        :placeholder="isMultiChoice ? '$\\mathrm{AC}$' : '$\\mathrm{B}$'"
        class="answer-input"
      />
      <p class="answer-hint">
        <template v-if="isMultiChoice">多选题：点击字母多选，自动合并为 $\mathrm{AC}$ 格式</template>
        <template v-else>单选题：点击字母选择，自动生成 $\mathrm{B}$ 格式</template>
      </p>
    </div>
  </div>
</template>

<style scoped>
.choice-editor {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

/* ============ 选项网格 ============ */
.options-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
}

.opt-card {
  background: var(--bg-input);
  border-radius: 12px;
  padding: 12px 14px;
  border: 1.5px solid transparent;
  transition: box-shadow 0.2s cubic-bezier(0.4, 0, 0.2, 1), background 0.2s cubic-bezier(0.4, 0, 0.2, 1);
}

[data-theme='dark'] .opt-card {
  border-color: rgba(255, 255, 255, 0.08);
}

.opt-card:focus-within {
  border-color: var(--accent);
  box-shadow: none;
}

[data-theme='dark'] .opt-card:focus-within {
  box-shadow: none;
}

.opt-card.correct {
  border-color: var(--accent);
  background: var(--accent-light);
}

[data-theme='dark'] .opt-card.correct {
  background: rgba(0, 122, 255, 0.1);
}

.opt-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 6px;
}

/* 选项标题：使用系统主题色变量 */
.opt-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--accent, #007aff);
}

.opt-actions {
  display: flex;
  gap: 4px;
}

.opt-icon-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  border-radius: 5px;
  opacity: 0;
  transition: opacity 0.2s, color 0.2s, background-color 0.2s;
}

.opt-card:hover .opt-icon-btn {
  opacity: 0.5;
}

.opt-icon-btn:hover {
  opacity: 1 !important;
  color: var(--accent);
  background: var(--accent-light);
}

.opt-delete-btn:hover {
  color: var(--danger);
  background: var(--danger-light);
}

.opt-card-input {
  width: 100%;
  border: none;
  background: transparent;
  box-shadow: none;
  outline: none;
  color: var(--text-primary);
  font-size: 13px;
  line-height: 1.5;
  font-family: inherit;
  padding: 2px 0;
}

.opt-card-input::placeholder {
  color: var(--text-muted);
}

.add-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 8px 14px;
  border: none;
  background: var(--bg-input);
  color: var(--text-secondary);
  font-size: 13px;
  font-weight: 400;
  border-radius: 12px;
  cursor: pointer;
  transition: transform 0.2s cubic-bezier(0.4, 0, 0.2, 1), background 0.2s, color 0.2s;
  font-family: inherit;
  align-self: flex-start;
}

.add-btn:hover {
  color: var(--accent);
  background: var(--accent-light);
  transform: translateY(-0.5px);
}

/* ============ 答案（结果）卡片 ============ */
.answer-card {
  background: var(--bg-input);
  border-radius: 12px;
  padding: 12px 16px;
  border: 1px solid transparent;
}

[data-theme='dark'] .answer-card {
  border-color: rgba(255, 255, 255, 0.08);
}

.answer-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 8px;
}

/* 答案标题：使用系统主题色变量 */
.answer-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--accent, #007aff);
}

/* A/B/C/D 快捷按钮组：使用系统主题色 */
.quick-answer-btns {
  display: flex;
  gap: 6px;
}

.quick-btn {
  width: 32px;
  height: 32px;
  border-radius: 10px;
  background: var(--accent-light, #ecf5ff);
  border: 1px solid transparent;
  color: var(--accent, #007aff);
  font-weight: 600;
  font-size: 13px;
  cursor: pointer;
  transition: transform 0.2s cubic-bezier(0.4, 0, 0.2, 1);
}

[data-theme='dark'] .quick-btn {
  background: rgba(0, 122, 255, 0.1);
  border-color: rgba(255, 255, 255, 0.1);
  color: var(--accent, #0a84ff);
}

.quick-btn:hover {
  opacity: 0.85;
}

/* 选中态：主题色背景 + 白字 */
.quick-btn.active {
  background: var(--accent, #007aff) !important;
  color: #ffffff !important;
  border-color: var(--accent, #007aff) !important;
  box-shadow: 0 2px 6px var(--accent-light);
}

/* 答案文本输入框 */
.answer-input {
  width: 100%;
  border: 1px solid transparent;
  background: var(--bg-card);
  border-radius: 12px;
  padding: 10px 12px;
  font-size: 15px;
  font-weight: 400;
  color: var(--text-primary);
  outline: none;
  transition: box-shadow 0.2s cubic-bezier(0.4, 0, 0.2, 1);
  font-family: inherit;
}

[data-theme='dark'] .answer-input {
  background: rgba(255, 255, 255, 0.06);
}

.answer-input:focus {
  border-color: var(--accent);
  box-shadow: none;
}

.answer-input::placeholder {
  color: var(--text-muted);
}

.answer-hint {
  margin: 6px 0 0;
  font-size: 11px;
  color: var(--text-muted);
}
</style>
