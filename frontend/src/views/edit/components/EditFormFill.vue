<script setup lang="ts">
import { AppIcon } from '@/components/ui'

const blanks = defineModel<{ position: number; answer: string }[]>('blanks', { required: true })

function addBlank() {
  const maxPos = blanks.value.length > 0 ? Math.max(...blanks.value.map(b => b.position), 0) : 0
  blanks.value.push({ position: maxPos + 1, answer: '' })
}

function removeBlank(index: number) {
  blanks.value.splice(index, 1)
}
</script>

<template>
  <div class="blank-wrap">
    <div v-for="(blank, i) in blanks" :key="i" class="blank-item">
      <span class="blank-label">第{{ i + 1 }}空</span>
      <input
        v-model="blank.answer"
        placeholder="答案"
        class="opt-input blank-input"
      />
      <button v-if="blanks.length > 1" type="button" class="icon-btn" @click="removeBlank(i)">
        <AppIcon name="x" :size="15" />
      </button>
    </div>
    <button type="button" class="add-btn add-btn-sm" @click="addBlank">
      <AppIcon name="plus" :size="14" /> 添加填空位
    </button>
  </div>
</template>

<style scoped>
/* 填空题紧凑布局 */
.blank-wrap {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: flex-end;
}

.blank-item {
  display: flex;
  align-items: center;
  gap: 8px;
  background: var(--bg-input);
  border-radius: 8px;
  padding: 4px 8px;
  border: 1.5px solid transparent;
  transition: border-color 0.2s ease, background-color 0.2s ease;
}

.blank-item:focus-within {
  border-color: var(--accent);
  background: var(--bg-card);
}

.blank-label {
  font-size: 12px;
  color: var(--text-muted);
  width: 44px;
  flex-shrink: 0;
  font-weight: 550;
  padding-left: 4px;
}

.opt-input.blank-input {
  flex: 1;
  min-width: 120px;
  border: none !important;
  background: transparent !important;
  outline: none !important;
  color: var(--text-primary);
  font-size: 13px;
  padding: 4px 0;
}

.icon-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  padding: 4px;
  border-radius: 4px;
  transition: background-color 0.2s, color 0.2s;
}

.icon-btn:hover {
  background: var(--danger-light);
  color: var(--danger);
}

.add-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
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
