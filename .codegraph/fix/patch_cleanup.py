# -*- coding: utf-8 -*-
"""清理 QuestionList.vue 中 review 指出的废弃代码（CSS 与未用函数）"""
import io

path = "frontend/src/views/QuestionList.vue"
src = io.open(path, encoding="utf-8").read()

# 1. 删除 GradeLevel/Semester/ExamType label 三个未使用函数（gradeLevelLabel 保留？检查）
#    review 说三个均无调用点 → 全部删除
old_funcs = '''// GradeLevel 枚举 → 中文标签
function gradeLevelLabel(g: GradeLevel | null | undefined): string {
  if (!g) return ''
  const map: Record<GradeLevel, string> = {
    grade_7: '初一',
    grade_8: '初二',
    grade_9: '初三',
    grade_10: '高一',
    grade_11: '高二',
    grade_12: '高三',
    other: '其他',
  }
  return map[g] || g
}

function semesterLabel(s: SemesterType | null | undefined): string {
  if (!s) return ''
  const map: Record<SemesterType, string> = {
    first: '上学期',
    second: '下学期',
    full_year: '全年',
  }
  return map[s] || s
}

function examTypeLabel(t: ExamType | null | undefined): string {
  if (!t) return ''
  const map: Record<ExamType, string> = {
    midterm: '期中',
    final: '期末',
    gaokao: '高考',
    mock: '模拟',
    entrance: '中考',
    daily: '日常',
    other: '其他',
  }
  return map[t] || t
}

'''
assert old_funcs in src, "label 函数块未找到"
src = src.replace(old_funcs, "")
print("label 函数已删除")

# 2. 删除 .q-kp-inline CSS 块（知识点移入 footer 后模板不再引用）
old_kp = '''.q-kp-inline {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex-wrap: nowrap;
  min-width: 0;
  overflow: hidden;
}

'''
assert old_kp in src, "q-kp-inline CSS 未找到"
src = src.replace(old_kp, "")
print("q-kp-inline CSS 已删除")

# 3. 删除 .q-tag 系列 CSS（旧样式，无模板引用）
old_tag = '''.q-tag {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 3px 10px;
  border-radius: var(--radius-full);
  font-size: 11.5px;
  font-weight: 600;
  letter-spacing: 0.01em;
  white-space: nowrap;
}

/* Type tags */
.q-tag--choice {
  background: rgba(0, 113, 227, 0.1);
  color: var(--accent);
}
.q-tag--fill {
  background: var(--warning-light);
  color: var(--warning);
}
.q-tag--solution {
  background: var(--success-light);
  color: var(--success);
}

/* Difficulty tags */
.q-tag--easy {
  background: var(--success-light);
  color: var(--success);
}
.q-tag--medium {
  background: var(--warning-light);
  color: var(--warning);
}
.q-tag--hard {
  background: var(--danger-light);
  color: var(--danger);
}

/* Neutral / status tag */
.q-tag--neutral {
  background: var(--bg-active);
  color: var(--text-muted);
}

'''
assert old_tag in src, "q-tag 系列 CSS 未找到"
src = src.replace(old_tag, "")
print("q-tag 系列 CSS 已删除")

io.open(path, "w", encoding="utf-8").write(src)
print("清理完成")
