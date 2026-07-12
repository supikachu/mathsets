import { ref, computed } from 'vue'

export type Theme = 'light' | 'dark'

const STORAGE_KEY = 'mathset_theme'

const theme = ref<Theme>('light')

function getSystemTheme(): Theme {
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

function applyTheme(value: Theme) {
  document.documentElement.setAttribute('data-theme', value)
}

/** 在 main.ts 挂载前调用，避免主题闪烁 */
export function initTheme() {
  const stored = localStorage.getItem(STORAGE_KEY)
  const initial: Theme =
    stored === 'dark' || stored === 'light' ? stored : getSystemTheme()
  theme.value = initial
  applyTheme(initial)
}

export function useTheme() {
  const isDark = computed(() => theme.value === 'dark')

  function setTheme(value: Theme) {
    theme.value = value
    applyTheme(value)
    localStorage.setItem(STORAGE_KEY, value)
  }

  function toggleTheme() {
    setTheme(theme.value === 'light' ? 'dark' : 'light')
  }

  return {
    theme,
    isDark,
    setTheme,
    toggleTheme,
  }
}
