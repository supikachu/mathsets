# -*- coding: utf-8 -*-
"""回退上一次对话（两层工具栏重构）对 QuestionList.vue 的全部改动"""
import io

path = "frontend/src/views/QuestionList.vue"
raw = io.open(path, encoding="utf-8").read()
# 统一 LF 处理（文件为 CRLF，替换用 LF 匹配），写回时恢复 CRLF
src = raw.replace("\r\n", "\n")
HAS_CRLF = "\r\n" in raw

# ── 1. 模板：两层结构 → 单行 ql-toolbar（索引切片替换，筛选面板保留） ──
start_marker = '    <!-- ===== 吸顶工具栏（上下两层：全局操作区 + 视图控制/统计区） ===== -->'
panel_marker = '      <!-- ===== 多维属性矩阵筛选面板 ===== -->'
start_idx = src.find(start_marker)
panel_idx = src.find(panel_marker, start_idx)
assert start_idx >= 0, "两层模板起始未找到"
assert panel_idx > start_idx, "筛选面板锚点未找到"

new_top = '''    <!-- ===== Apple风格吸顶工具栏 ===== -->
    <div class="ql-sticky-bar">
      <div class="ql-toolbar">
        <!-- 左侧：状态切换 Segmented Tab -->
        <div class="ql-toolbar-left">
          <div class="ql-seg-ctrl">
            <button
              v-for="tab in statusTabs"
              :key="tab.value"
              class="ql-seg-item"
              :class="{ active: currentStatus === tab.value }"
              @click="switchStatus(tab.value)"
            >
              <AppIcon :name="tab.icon" :size="14" class="ql-seg-icon" />
              <span class="ql-seg-label">{{ tab.label }}</span>
              <span
                v-if="tab.value === 'pending' && pendingReviewCount > 0"
                class="ql-seg-badge"
              >{{ pendingReviewCount > 99 ? '99+' : pendingReviewCount }}</span>
            </button>
          </div>
        </div>

        <!-- 右侧：搜索框 + 新建题目 + 主题切换 -->
        <div class="ql-toolbar-right">
          <div class="ql-search-wrap">
            <AppIcon name="search" :size="15" class="ql-search-icon" />
            <input
              v-model="query.keyword"
              class="ql-search-input"
              placeholder="搜索题目（输入即搜）"
              @input="onSearchInput"
              @keydown.enter="onSearchSubmit"
            />
            <button class="ql-search-go" @click="toggleFilter">
              <AppIcon name="filter" :size="14" />
              筛选
            </button>
          </div>
          <button
            v-if="basket.count.value > 0"
            class="ql-basket-btn"
            @click="toast.info(`试题篮中有 ${basket.count.value} 道题目`)"
          >
            <AppIcon name="shopping-cart" :size="16" />
            <span class="ql-basket-count">{{ basket.count.value }}</span>
          </button>
          <button class="ql-new-btn" @click="$router.push('/questions/new')">
            <AppIcon name="plus" :size="16" />
            新建题目
          </button>
          <ThemeToggle />
        </div>
      </div>

'''

# 替换 [start_idx, panel_idx)（两层结构）→ 新 ql-toolbar 单行结构
src = src[:start_idx] + new_top + src[panel_idx:]
print("模板回退完成（单行工具栏恢复，筛选面板保留）")

# ── 1b. 恢复 ql-sub-header 块（sticky-bar 闭合后、可滚动列表区前） ──
scroll_marker = '    <!-- ===== 可滚动列表区域 ===== -->'
scroll_idx = src.find(scroll_marker)
assert scroll_idx >= 0, "可滚动列表区域锚点未找到"
sub_header = '''    <!-- ===== 次层 Header：列表元信息副标题 ===== -->
    <div class="ql-sub-header">
      <span class="ql-sub-header-text">共找到 <strong>{{ totalCount }}</strong> 道题目</span>
    </div>

'''
src = src[:scroll_idx] + sub_header + src[scroll_idx:]
print("ql-sub-header 恢复")

# ── 2. CSS 恢复 ──
anchor1 = "/* Segmented Control — Apple 风格胶囊分段控制器 */\n.ql-seg-ctrl {"
assert anchor1 in src, "seg-ctrl 锚点未找到"
css_left = '''/* ===== 工具栏内嵌：Segmented Tab ===== */
.ql-toolbar-left {
  display: flex;
  align-items: center;
  min-width: 0;
  flex-shrink: 0;
}

.ql-toolbar-right {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-shrink: 0;
  min-width: 0;
}

/* 次层 Header：列表元信息副标题 */
.ql-sub-header {
  flex-shrink: 0;
  padding: 8px 20px 6px;
  background: var(--bg-primary);
  border-bottom: 1px solid var(--divider);
}

.ql-sub-header-text {
  font-size: 12.5px;
  color: var(--text-muted);
  letter-spacing: -0.01em;
}

.ql-sub-header-text strong {
  color: var(--text-secondary);
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  margin: 0 2px;
}

'''
src = src.replace(anchor1, css_left + anchor1, 1)
print("CSS：ql-toolbar-left/right/sub-header 系列恢复")

anchor2 = "/* 搜索框 — 固定宽度，防止 flex 拉伸挤压按钮 */"
assert anchor2 in src, "search-wrap 锚点未找到"
css_toolbar = '''/* ===== 工具栏单行布局：左 Tab + 右操作，两端对齐 ===== */
.ql-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 10px 20px;
}

/* 题库空间切换 */
/* 空间切换 — Apple分段控件 */
.ql-space-segmented {
  display: inline-flex;
  align-items: center;
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  border-radius: 10px;
  padding: 3px;
  gap: 2px;
  flex-shrink: 0;
}

.ql-space-seg {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 5px 14px;
  border: none;
  background: transparent;
  border-radius: 7px;
  font-size: 13px;
  font-weight: 500;
  color: var(--text-secondary);
  cursor: pointer;
  transition: var(--transition-fast);
  white-space: nowrap;
}

.ql-space-seg:hover:not(.active) {
  color: var(--text-primary);
  background: var(--bg-hover);
}

.ql-space-seg.active {
  background: var(--bg-elevated, var(--bg-card));
  color: var(--text-primary);
  font-weight: 600;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.08);
}

[data-theme='dark'] .ql-space-seg.active {
  background: #3a3a3c;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.3);
}

'''
src = src.replace(anchor2, css_toolbar + anchor2, 1)
print("CSS：ql-toolbar/space-segmented/seg 系列恢复")

anchor3 = "/* ===== Loading ===== */"
assert anchor3 in src, "loading 锚点未找到"
css_basket = '''/* ===== Header Actions ===== */
.header-actions {
  display: flex;
  align-items: center;
  gap: 10px;
}

.basket-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 7px 14px;
  border-radius: var(--radius-full);
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  box-shadow: var(--shadow-xs);
  color: var(--text-secondary);
  font-size: 13px;
  font-weight: 600;
  transition: var(--transition-fast);
}

.basket-btn:hover {
  background: var(--bg-hover);
  color: var(--accent);
}

.basket-count {
  display: flex;
  align-items: center;
  justify-content: center;
  min-width: 18px;
  height: 18px;
  padding: 0 5px;
  border-radius: var(--radius-full);
  background: var(--accent);
  color: #fff;
  font-size: 11px;
  font-weight: 700;
}

'''
src = src.replace(anchor3, css_basket + anchor3, 1)
print("CSS：header-actions/basket 系列恢复")

if "ql-filter-kp" not in src:
    css_filter = '''/* ===== Knowledge node filter row ===== */
.ql-filter-kp {
  display: flex;
  align-items: center;
  gap: 12px;
  flex: 1;
  min-width: 0;
}

.ql-filter-descendant {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--text-secondary);
  white-space: nowrap;
  cursor: pointer;
  user-select: none;
}

.ql-filter-descendant input {
  margin: 0;
  cursor: pointer;
}

'''
    src = src.replace(anchor3, css_filter + anchor3, 1)
    print("CSS：ql-filter-kp/descendant 恢复")

old_sb = '''.ql-sticky-bar {
  position: sticky;
  top: 0;
  z-index: 100;
  flex-shrink: 0;
  background: var(--bg-primary);
  backdrop-filter: saturate(180%) blur(20px);
  -webkit-backdrop-filter: saturate(180%) blur(20px);
  /* 底部分割线由次层 border-b（浅灰）承担，避免双线 */
}'''
new_sb = '''.ql-sticky-bar {
  position: sticky;
  top: 0;
  z-index: 100;
  flex-shrink: 0;
  background: var(--bg-primary);
  backdrop-filter: saturate(180%) blur(20px);
  -webkit-backdrop-filter: saturate(180%) blur(20px);
  border-bottom: 1px solid var(--border-color);
}'''
assert old_sb in src, "sticky-bar 注释态未找到"
src = src.replace(old_sb, new_sb)
print("CSS：sticky-bar border-bottom 恢复")

out = "\r\n".join(src.split("\n")) if HAS_CRLF else src
io.open(path, "w", encoding="utf-8", newline="").write(out)
print("\n全部回退完成")
