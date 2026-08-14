import { ref, computed } from 'vue'

export type Theme = 'light' | 'dark'

const STORAGE_KEY = 'mathset_theme'

const theme = ref<Theme>('light')

function getSystemTheme(): Theme {
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

function applyTheme(value: Theme) {
  // ---- 主题切换瞬闪防护（GitHub / Linear / Vercel 方案）----
  // 1. 切换前：注入全局样式，强制禁用所有元素的 transition
  //    这样 Tailwind 的 transition-colors 等组件级过渡不会在
  //    data-theme 属性变更时产生 150ms 的缓动动画，避免阶梯式延迟闪烁
  const css = document.createElement('style')
  css.setAttribute('data-theme-transition', '')
  css.textContent = '*, *::before, *::after { transition: none !important; }'
  document.head.appendChild(css)

  // 2. 执行主题属性切换
  document.documentElement.setAttribute('data-theme', value)

  // 3. 等待浏览器完成新主题的 Style Recalc + Paint（双 rAF 保证一帧完整渲染），
  //    然后移除过渡抑制，恢复正常的 hover/focus 交互过渡
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      css.remove()
    })
  })
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
