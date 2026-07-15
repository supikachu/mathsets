import { ref } from 'vue'

// 全局单例状态 — 控制录题工作台的侧边栏折叠
const _isKpTreeCollapsed = ref(false)
const _isNavCollapsed = ref(false)

export function useLayoutState() {
  return {
    isKpTreeCollapsed: _isKpTreeCollapsed,
    isNavCollapsed: _isNavCollapsed,
  }
}
