/** 题目展示用标签、徽章颜色与图标映射 */

export function typeLabel(t: string) {
  const map: Record<string, string> = {
    choice: '选择',
    fill: '填空',
    solution: '解答',
    judgment: '判断',
  }
  return map[t] || t
}

export function typeBadgeColor(t: string): 'blue' | 'yellow' | 'green' | 'gray' {
  const map: Record<string, 'blue' | 'yellow' | 'green' | 'gray'> = {
    choice: 'blue',
    fill: 'yellow',
    solution: 'green',
    judgment: 'gray',
  }
  return map[t] || 'blue'
}

export function diffLabel(d: string) {
  const map: Record<string, string> = {
    easy: '简单',
    medium: '中等',
    hard: '困难',
  }
  return map[d] || d
}

export function diffBadgeColor(d: string): 'green' | 'yellow' | 'red' {
  const map: Record<string, 'green' | 'yellow' | 'red'> = {
    easy: 'green',
    medium: 'yellow',
    hard: 'red',
  }
  return map[d] || 'yellow'
}

export function statusLabel(s: string) {
  const map: Record<string, string> = {
    draft: '草稿',
    pending: '待审核',
    rejected: '驳回',
    published: '已发布',
    disabled: '停用',
  }
  return map[s] || s
}

/** 状态对应的 AppIcon 名称 */
export function statusIcon(s: string): string {
  const map: Record<string, string> = {
    draft: 'pencil',
    pending: 'clock',
    rejected: 'x-circle',
    published: 'check-circle',
    disabled: 'ban',
  }
  return map[s] || 'info'
}

export function statusBadgeColor(
  s: string,
): 'gray' | 'yellow' | 'red' | 'green' | 'blue' {
  const map: Record<string, 'gray' | 'yellow' | 'red' | 'green' | 'blue'> = {
    draft: 'gray',
    pending: 'yellow',
    rejected: 'red',
    published: 'green',
    disabled: 'gray',
  }
  return map[s] || 'gray'
}

export function formatTime(t?: string) {
  return t ? t.replace('T', ' ').substring(0, 19) : ''
}
