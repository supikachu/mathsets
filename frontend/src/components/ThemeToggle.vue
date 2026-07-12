<template>
  <button
    type="button"
    class="theme-toggle"
    :class="{ dark: isDark }"
    :title="isDark ? '切换到浅色模式' : '切换到深色模式'"
    @click="toggle"
  >
    <span class="theme-toggle-track">
      <AppIcon :name="isDark ? 'moon' : 'sun'" :size="16" class="theme-toggle-icon" />
    </span>
  </button>
</template>

<script setup lang="ts">
import { ref, watch, onMounted } from 'vue'
import { AppIcon } from '@/components/ui'

const isDark = ref(false)

function applyTheme(dark: boolean) {
  document.documentElement.setAttribute('data-theme', dark ? 'dark' : 'light')
  isDark.value = dark
  localStorage.setItem('theme', dark ? 'dark' : 'light')
}

function toggle() {
  applyTheme(!isDark.value)
}

onMounted(() => {
  const saved = localStorage.getItem('theme')
  const prefersDark =
    window.matchMedia &&
    window.matchMedia('(prefers-color-scheme: dark)').matches
  applyTheme(saved ? saved === 'dark' : prefersDark)
})

watch(
  () => window.matchMedia('(prefers-color-scheme: dark)'),
  () => {},
)
</script>

<style scoped>
.theme-toggle {
  width: 38px;
  height: 38px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  box-shadow: var(--shadow-xs);
  color: var(--text-secondary);
  transition: var(--transition-fast);
}

.theme-toggle:hover {
  background: var(--bg-hover);
  color: var(--accent);
  box-shadow: var(--shadow-sm);
}

.theme-toggle:active {
  transform: scale(0.92);
}

.theme-toggle.dark .theme-toggle-icon {
  color: var(--warning);
}

.theme-toggle-icon {
  transition: var(--transition-bounce);
}
</style>
