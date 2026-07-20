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
      { path: '/profile', label: '我的', icon: 'user', shortLabel: '我的' },
    ]
    if (auth.isAdmin) {
      // 管理员在「我的」之前插入：用户管理 + 标签管理 + 知识树管理（移动端会自动折行/缩小）
      list.splice(3, 0, { path: '/users', label: '用户管理', icon: 'users', shortLabel: '用户' })
      list.splice(4, 0, { path: '/settings/tags', label: '标签管理', icon: 'tag', shortLabel: '标签' })
      list.splice(5, 0, { path: '/settings/knowledge-trees', label: '知识树', icon: 'book-open', shortLabel: '知识树' })
    }
    return list
  })

  return { items }
}
