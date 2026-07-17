<script setup lang="ts">
import { nextTick } from 'vue'
import { AppIcon } from '@/components/ui'

const subAnswers = defineModel<string[]>('subAnswers', { required: true })

function addSubAnswer() {
  subAnswers.value.push('')
  nextTick(() => {
    const els = document.querySelectorAll<HTMLTextAreaElement>('.sub-answer-input')
    const last = els[els.length - 1]
    if (last) {
      resizeTextarea(last)
      last.focus()
    }
  })
}

function removeSubAnswer(i: number) {
  subAnswers.value.splice(i, 1)
  if (subAnswers.value.length === 0) subAnswers.value.push('')
}

function resizeTextarea(el: HTMLTextAreaElement) {
  el.style.height = 'auto'
  el.style.height = `${el.scrollHeight}px`
}

function handleInput(e: Event) {
  resizeTextarea(e.target as HTMLTextAreaElement)
}
</script>

<template>
  <div>
    <div class="sub-answer-list">
      <div v-for="(ans, i) in subAnswers" :key="i" class="sub-answer-card">
        <span class="sub-answer-num">({{ i + 1 }})</span>
        <textarea
          v-model="subAnswers[i]"
          rows="2"
          class="edit-textarea sub-answer-input"
          :placeholder="`小题(${i + 1})答案，支持 $...$ LaTeX`"
          @input="handleInput"
        ></textarea>
        <button v-if="subAnswers.length > 1" type="button" class="sub-answer-del" @click="removeSubAnswer(i)" title="删除此小题">
          <AppIcon name="x" :size="14" />
        </button>
      </div>
    </div>
    <button type="button" class="add-btn add-btn-sm" @click="addSubAnswer">
      <AppIcon name="plus" :size="14" /> 增加小题答案
    </button>
  </div>
</template>

<style scoped>
.sub-answer-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.sub-answer-card {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  position: relative;
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  border-radius: 12px;
  padding: 10px 12px;
  transition: border-color 0.2s;
}

.sub-answer-card:focus-within {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
}

[data-theme='dark'] .sub-answer-card {
  border-color: rgba(255, 255, 255, 0.08);
}

.sub-answer-num {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary);
  flex-shrink: 0;
  padding-top: 6px;
  min-width: 24px;
}

.sub-answer-input {
  flex: 1;
  border: none !important;
  background: transparent !important;
  padding: 4px 0 !important;
  min-height: 32px;
  font-family: var(--font-cn-isolated);
  resize: none;
  outline: none;
  color: var(--text-primary);
  font-size: 13px;
}

.sub-answer-input:focus {
  border: none !important;
  box-shadow: none !important;
}

.sub-answer-del {
  position: absolute;
  top: 6px;
  right: 6px;
  width: 22px;
  height: 22px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: 50%;
  background: rgba(0, 0, 0, 0.04);
  color: var(--text-muted);
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.2s, background 0.2s;
  flex-shrink: 0;
}

.sub-answer-card:hover .sub-answer-del {
  opacity: 1;
}

.sub-answer-del:hover {
  background: rgba(255, 59, 48, 0.1);
  color: #ff3b30;
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
