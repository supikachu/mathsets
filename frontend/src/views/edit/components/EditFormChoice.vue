<script setup lang="ts">
import { computed, nextTick } from 'vue'
import { AppIcon } from '@/components/ui'
import { useToast } from '@/composables/useToast'

const options = defineModel<{ label: string; content: string }[]>('options', { required: true })
const correctAnswer = defineModel<any>('correctAnswer', { required: true })
const subType = defineModel<string>('subType', { required: true })

const toast = useToast()

const isMultiChoice = computed(() => subType.value === 'multi')

// Sync multiCorrectAnswers computed with correctAnswer model value
const multiCorrectAnswers = computed({
  get: () => Array.isArray(correctAnswer.value) ? correctAnswer.value : [],
  set: (val: string[]) => { correctAnswer.value = [...val].sort() },
})

function isOptionCorrect(label: string): boolean {
  if (Array.isArray(correctAnswer.value)) return correctAnswer.value.includes(label)
  return correctAnswer.value === label
}

function addOption() {
  const nextLabel = String.fromCharCode(65 + options.value.length) // A, B, C, D...
  options.value.push({ label: nextLabel, content: '' })
}

// Auto split and paste options text
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

// Option image upload helper
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
  <div class="choice-grid">
    <div
      v-for="(opt, i) in options"
      :key="i"
      class="opt-card"
      :class="{ correct: isOptionCorrect(opt.label) }"
    >
      <label class="opt-prefix" :class="{ checked: isOptionCorrect(opt.label) }">
        <input v-if="isMultiChoice" type="checkbox" :value="opt.label" v-model="multiCorrectAnswers" />
        <input v-else type="radio" :value="opt.label" v-model="correctAnswer" />
        <span class="opt-letter">{{ opt.label }}</span>
      </label>
      <input
        v-model="opt.content"
        :placeholder="`选项 ${opt.label}`"
        class="opt-card-input"
        @paste="onOptionPaste($event, i)"
      />
      <button type="button" class="opt-img-btn" @click="handleOptionImageUpload(i)" title="上传配图">
        <AppIcon name="paperclip" :size="14" />
      </button>
      <button v-if="options.length > 2" type="button" class="opt-delete" @click="options.splice(i, 1)">
        <AppIcon name="x" :size="15" />
      </button>
    </div>
    <button type="button" class="add-btn add-btn-sm" @click="addOption">
      <AppIcon name="plus" :size="14" /> 添加选项
    </button>
  </div>
</template>

<style scoped>
/* 选择题选项 Grid */
.choice-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
}

.choice-grid .add-btn-sm {
  grid-column: 1;
  justify-self: start;
  width: fit-content;
  max-width: 200px;
}

/* 选项卡片（一体化胶囊） */
.opt-card {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 12px;
  border-radius: 10px;
  background: var(--bg-input);
  border: 1.5px solid transparent;
  transition: border-color 0.2s ease, background-color 0.2s ease, box-shadow 0.2s ease;
}

[data-theme='dark'] .opt-card {
  border-color: rgba(255, 255, 255, 0.08);
}

.opt-card:focus-within {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
}

[data-theme='dark'] .opt-card:focus-within {
  box-shadow: 0 0 0 3px rgba(0, 122, 255, 0.15);
}

.opt-card.correct {
  background: var(--accent-light);
  border-color: var(--accent);
}

[data-theme='dark'] .opt-card.correct {
  background: rgba(0, 122, 255, 0.12);
  border-color: var(--accent);
}

/* 前缀（单选/多选 + 字母） */
.opt-prefix {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
  cursor: pointer;
  user-select: none;
}

.opt-prefix input {
  margin: 0;
  accent-color: var(--accent);
}

.opt-letter {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-secondary);
}

.opt-prefix.checked .opt-letter {
  color: var(--accent);
}

/* 隐形输入框 */
.opt-card-input {
  flex: 1;
  min-width: 0;
  border: none;
  background: transparent;
  box-shadow: none;
  outline: none;
  color: var(--text-primary);
  font-size: 13px;
  line-height: 1.4;
  font-family: inherit;
  padding: 2px 0;
}

.opt-card-input::placeholder {
  color: var(--text-muted);
}

/* 删除按钮（hover 淡入） */
.opt-delete {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  border-radius: 6px;
  opacity: 0;
  transition: opacity 0.2s ease, color 0.2s ease, background-color 0.2s ease;
}

.opt-card:hover .opt-delete {
  opacity: 0.6;
}

.opt-delete:hover {
  opacity: 1 !important;
  color: var(--danger);
  background: var(--danger-light);
}

/* 选项配图按钮（hover/focus 淡入） */
.opt-img-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  border-radius: 6px;
  opacity: 0;
  transition: opacity 0.2s ease, color 0.2s ease, background-color 0.2s ease;
}

.opt-card:hover .opt-img-btn,
.opt-card:focus-within .opt-img-btn {
  opacity: 0.6;
}

.opt-img-btn:hover {
  opacity: 1 !important;
  color: var(--accent);
  background: var(--accent-light);
}

.add-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  margin-top: 4px;
  padding: 7px 14px;
  border: 1px dashed var(--border-strong);
  background: transparent;
  color: var(--text-secondary);
  font-size: 13px;
  font-weight: 500;
  border-radius: 8px;
  cursor: pointer;
  transition: var(--transition-fast);
  font-family: inherit;
}

.add-btn:hover {
  border-color: var(--accent);
  color: var(--accent);
  border-style: solid;
  background: var(--accent-light);
}

.add-btn-sm {
  padding: 4px 10px;
  font-size: 12px;
  gap: 4px;
}
</style>
