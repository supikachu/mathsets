import { computed } from 'vue'
import { useAuthStore } from '@/stores/auth'

export interface NavItem {
  path: string
  label: string
  icon: string
  shortLabel: string
}

export function useNavItems() {
  const auth = useAuthStore()

  const items = computed<NavItem[]>(() => {
    const list: NavItem[] = [
      { path: '/dashboard', label: '工作台', icon: 'grid', shortLabel: '工作台' },
      { path: '/questions', label: '题库', icon: 'file-text', shortLabel: '题库' },
      { path: '/review', label: '审核队列', icon: 'shield-check', shortLabel: '审核' },
    ]
    if (auth.isAdmin) {
      list.push({ path: '/users', label: '用户管理', icon: 'user', shortLabel: '用户' })
      list.push({ path: '/settings/tags', label: '标签管理', icon: 'tag', shortLabel: '标签' })
    }
    return list
  })

  return { items }
}
