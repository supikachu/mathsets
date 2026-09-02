# 开发日记

> 项目：协同题库系统 (mathsets)
> 技术栈：Rust + Axum 0.8 + PostgreSQL 17 | Vue 3 + Element Plus + Tailwind CSS

---

## 2026-05-08（第一天）

### 今日完成

#### 1. 项目初始化
- 创建 Rust 项目骨架，选定 **Axum 0.8** 作为 Web 框架
- 搭建模块结构：`auth/` `handlers/` `models/` `config.rs` `db.rs`
- 配置 Cargo 依赖：axum / tokio / serde / sqlx / jsonwebtoken / bcrypt 等

#### 2. 业务需求文档
- 输出 `docs/requirements.md`，覆盖：
  - 用户角色体系（管理员 / 教研组长 / 教师 / 浏览者）
  - 题目模型（4 种题型特有结构）
  - 5 种状态流转（草稿 → 待审核 → 已发布/驳回 → 已停用）
  - 知识点分类树、组卷功能、搜索与检索

#### 3. 数据库搭建
- 安装 PostgreSQL 17.9 本地服务
- 配置 `pg_hba.conf` 为 trust 认证
- 创建数据库 `mathsets`
- 编写 SQLx 迁移：`users` / `groups` / `group_members` / `questions` / `knowledge_points` 等 8 张表

#### 4. 后端 API 实现
- `POST /api/v1/auth/register` — 教师注册
- `POST /api/v1/auth/login` — 登录返回 JWT
- `GET/POST /api/v1/knowledge-points` — 知识点树 CRUD
- `PUT/DELETE /api/v1/knowledge-points/:id` — 知识点节点管理
- `GET/POST /api/v1/questions` — 题目列表（支持多维度搜索过滤）+ 创建
- `GET/PUT/DELETE /api/v1/questions/:id` — 题目详情/编辑/删除
- `POST /api/v1/questions/:id/submit` — 提交审核
- `POST /api/v1/questions/:id/review` — 审核通过/驳回

#### 5. JWT 认证中间件
- `src/auth/middleware.rs` — `require_auth` 中间件
- 提取 `Authorization: Bearer <token>` → 验证 → 注入 `AuthUser`
- 路由分组：公开（health/auth）和保护（questions/knowledge-points）
- Handler 中使用 `Extension<AuthUser>` 获取当前用户

#### 6. 测试
- 13 项测试全部通过：
  - JWT 单元测试 4 项（签发/验证/错误密钥/无效 token）
  - 认证集成测试 3 项（注册/登录/字段缺失/不存在用户）
  - 知识点集成测试 1 项（完整 CRUD + 树结构）
  - 题目集成测试 4 项（完整生命周期/搜索/驳回/异常场景）
  - 健康检查 1 项

#### 7. 前端 UI/UX 设计
- 输出 `docs/ui-ux-design.md`，包含：
  - 8 个核心页面线框图（登录/注册/工作台/题目列表/题目编辑/题目详情/审核队列/知识点管理）
  - 前端路由规划 + 权限守卫
  - 14 个全局 UI 组件清单
- 确认技术栈：Vue 3 + Vite + Element Plus + Tailwind CSS + Pinia + TipTap + KaTeX + Yjs

### 技术债 / 遗留问题

| 问题 | 优先级 | 说明 |
|------|--------|------|
| `list_questions` 存在 15 个编译器警告 | 🟡 中 | 动态 SQL 构建中的未使用变量，需重构为 `sqlx::QueryBuilder` |
| 审核权限未校验 | 🔴 高 | 目前任何人都可以审核，需 JWT 角色判断 + 创建者回避 |
| `creator_id` 外键临时允许 NULL | 🟡 中 | JWT 中间件已上线，后续可改回 NOT NULL |
| `user_role` 未附加 `#[serde(rename_all = "lowercase")]` | 🟢 低 | 当前序列化为 "Teacher" 而非 "teacher" |

### 项目结构总览

```
mathsets/
├── Cargo.toml
├── .env
├── .cargo/config.toml       ← PATH 持久化（MinGW + PostgreSQL）
│
├── docs/
│   ├── requirements.md      ← 业务需求文档
│   └── ui-ux-design.md      ← 前端 UI/UX 设计
│
├── migrations/              ← 8 张数据库迁移表
│
├── src/
│   ├── main.rs              ← 启动入口
│   ├── lib.rs               ← Router 构建 + AppState
│   ├── config.rs            ← 环境变量配置
│   ├── db.rs                ← 连接池 + SQLx 迁移
│   │
│   ├── auth/                ← 认证模块
│   │   ├── mod.rs
│   │   ├── jwt.rs           ← JWT 签发/验证 + 4 项单元测试
│   │   └── middleware.rs    ← require_auth 中间件
│   │
│   ├── handlers/            ← API 处理器
│   │   ├── mod.rs
│   │   ├── health.rs
│   │   ├── auth.rs
│   │   ├── knowledge_points.rs
│   │   └── questions.rs
│   │
│   └── models/              ← 数据模型
│       ├── mod.rs
│       ├── user.rs
│       └── question.rs      ← 题目/知识点/审核 DTO
│
└── tests/
    └── api.rs               ← 9 项集成测试
```

### 提交记录（6 次）

```
85df936 docs: 前端 UI/UX 设计 — 确认技术栈
047935d docs: 前端 UI/UX 设计规划
b62f165 feat: JWT 认证中间件 — 保护 API 并传递用户信息
0aa6901 feat: 题目模块 — CRUD + 知识点树 + 审核状态机
4248fb6 feat: 集成 PostgreSQL 数据库 & 修复注册流程类型问题
f342827 feat: 初始化协同题库系统项目骨架
```

---

## 2026-05-09

### 今日完成

#### P0 — 审核权限校验 ✅
- `submit_question` 校验仅创建者可提交
- `review_question` 校验仅 GroupLeader/Admin 可审核
- 创建者回避：组长不能审核自己的题目

#### P0 — 教研组 API ✅
- `src/models/group.rs` + `src/handlers/groups.rs`
- 8 个端点：列表（含成员数）/ 创建 / 详情（含成员列表）/ 更新 / 删除 / 添加成员（UPSERT）/ 移除成员 / 设置组长
- 2 个集成测试

#### P1 — 重构 list_questions ✅
- 改用 `sqlx::QueryBuilder` 替代字符串拼接
- 消除全部 14 个编译器 warnings
- 后端源码零 warnings

#### P2 — Vue 3 前端脚手架 ✅
- 初始化 frontend/ 目录
- Element Plus + Tailwind CSS + Pinia + Vue Router + axios
- 登录页 + 注册页 + AppLayout 侧边栏
- 7 个占位页面，路由权限守卫
- `npm run build` 构建通过

### 项目状态
- **后端测试**: 11 项全部通过
- **后端 warnings**: 0（仅测试文件有少量未使用变量）
- **前端构建**: 通过
- **Git**: 9 次提交

### 任务一：修复技术债 🔧

| 任务 | 预估 | 详情 |
|------|------|------|
| 重构 `list_questions` 动态查询 | 1h | 用 `sqlx::QueryBuilder` 替换字符串拼接，消除 15 个 warnings |
| 修复 `list_questions` 的 `QuestionSummary` 缺失字段 | 0.5h | SELECT 漏掉了 `correct_answer` 等字段（虽然不返回但 `FromRow` 需要） |
| `user_role` 添加 `#[serde(rename_all = "lowercase")]` | 5min | 与 QuestionType/Difficulty 保持一致 |

### 任务二：后端教研组 API 👥

| 任务 | 预估 | 详情 |
|------|------|------|
| 教研组 CRUD handler | 1.5h | `groups` 表的增删改查（创建/列表/详情/更新/删除） |
| 成员管理 handler | 1h | `group_members` 的添加/移除/组长设置 |
| 集成测试 | 1h | 组创建 → 添加成员 → 设置组长 → 列表查询 |

### 任务三：审核权限 + 创建者回避 🔐

| 任务 | 预估 | 详情 |
|------|------|------|
| `submit_question` 校验创建者 | 0.5h | 只能提交自己创建的题目 |
| `review_question` 校验组长角色 | 0.5h | 仅 `groupleader` / `admin` 可审核 |
| `review_question` 创建者回避 | 0.5h | 组长不能审核自己的题目 |
| 更新测试 | 1h | 使用不同角色验证权限 |

### 任务四（可选）：前端脚手架 🚀

| 任务 | 预估 | 详情 |
|------|------|------|
| 初始化 Vue 3 + Vite 项目 | 0.5h | `pnpm create vue@latest` |
| 安装并配置 Element Plus + Tailwind | 0.5h | 全局引入 + 按需加载 |
| 配置 axios 拦截器 | 0.5h | 自动注入 Bearer token + 401 跳转 |
| 实现登录页 | 2h | 表单 + 调用 API + 持久化 token + 跳转工作台 |

### 优先级建议

```
高优先级（核心体验）:
  审核权限校验 + 创建者回避
  教研组 API

中优先级（代码质量）:
  重构 list_questions 消除 warnings

低优先级（业务扩展）:
  前端脚手架 + 登录页（取决于是否决定开始前端开发）
```

### 当前断点（5/9 晚上结束 — 无新进展）

#### ✅ 已完成
- P0: 审核权限校验 + 教研组 API + 统计/me API + creator_name
- P1: list_questions 重构（0 warnings）+ 404 页面 + 加载骨架 + 删除确认
- P2: 7 个前端页面全部完成 + KaTeX 渲染 + 编辑页分栏/拖拽分隔条/自动保存草稿
- P2: 试卷管理（后端迁移 + 8个API + 前端列表/详情/组卷）
- P2: 用户管理页面（管理员查看用户列表）
- P3: 后端测试补全（16 个测试全部通过）
- 批量导入 10 道题目
- 三个测试账号（admin/zhanglaoshi/wanglaoshi）

### 今天（5/9）Git 历史

```
3d240ba feat: 编辑页草稿自动保存与恢复
8102e0f feat: 编辑页可拖拽分隔条
cbbb4a4 P2: 用户管理页面
c8a01d2 P2: 试卷管理模块（前端）
10419ef P2: 试卷管理模块（后端）
4b8367f feat: 重构题目编辑页为纵向叠层布局
dc3f436 P1: 前端细节打磨
47c0a0a P3: 后端测试补全 — 新增 5 项测试
aa38f84 P1: 添加 404 页面
9890cd8 P0: 统计API + /auth/me + creator_name
```

#### ⏳ 待办（明天）
| # | 任务 | 优先级 | 预估 | 说明 |
|---|------|--------|------|------|
| 1 | **Docker 部署配置** | 🟢 P3 | 1h | docker-compose.yml + 多阶段构建（PostgreSQL + Rust后端 + Nginx前端） |
| 2 | **知识点管理页增强** | 🟡 P2 | 1.5h | 编辑已有题目时知识点加载完成后再显示，拖拽排序，批量关联 |
| 3 | **试卷导出/打印** | 🟡 P2 | 2h | 试卷详情页添加导出为 PDF/打印排版 |
| 4 | **题目批量导入增强** | 🟡 P2 | 1h | Web 端上传文件解析导入，支持 Word/LaTeX 格式 |
| 5 | **前端性能优化** | 🟢 P3 | 1h | 路由懒加载优化、Element Plus 按需导入、chunk 拆分 |
| 6 | **用户管理增强** | 🟡 P2 | 1h | 添加禁用/启用用户操作，角色修改 |

## 2026-05-09（下午）

### 上午完成

#### 1. 批量导入题目
- 从 `questions/ques.md` 导入 10 道初中数学题（3 选择 + 2 填空 + 5 解答）
- 使用 Node.js 脚本通过 API 批量导入
- 涵盖幂运算、等腰三角形、二次函数、因式分解、概率、方程、统计、平均数和二次函数综合

#### 2. KaTeX LaTeX 公式渲染
- 创建 `LatexRender.vue` 组件：将 `$...$` 和 `$$...$$` 替换为 KaTeX 渲染
- 引入 KaTeX CSS 字体样式
- 题目列表题干列：LatexRender 内联渲染
- 题目详情页：题干/选项/答案/解析全部自动 KaTeX 渲染
- 题目编辑页：重写为分栏布局（左侧编辑/右侧实时 KaTeX 预览）
- 修复填空题和解答题参考答案的渲染（之前用 JSON.stringify 纯文本）

#### 3. 编辑页重构
```
┌────────────────────────────────────────────────────┐
│ ← 返回         录入新题          [保存草稿] [提交审核] │
├─────────────────────┬──────────────────────────────┤
│  📖 题干编辑区       │  👁️ 实时预览 (KaTeX)         │
│  📝 答案编辑区       │  📋 基础属性                 │
│  (题型动态切换)       │  题型/难度⭐/年级/学期/分值/来源 │
│  💡 解析编辑区       │  🏷️ 知识点 (树形多选)       │
└─────────────────────┴──────────────────────────────┘
```

#### 4. 环境与测试账号
- 三个账号已稳定运行：admin(管理员) / zhanglaoshi(组长) / wanglaoshi(教师)
- 后端 API 运行于 localhost:3000
- 前端 Vite 运行于 localhost:5173

### 下午完成

#### P0 — 后端补充 ✅
- `GET /api/v1/questions/stats` — 按状态统计
- `GET /api/v1/auth/me` — 当前用户信息
- 题目列表/详情返回 `creator_name`
- Dashboard 改用 stats API

#### P1 — 前端体验 ✅
- 404 页面（NotFound.vue + 通配路由）

###  Git 历史（今日）

```
643c8a0 fix: 填空题/解答题参考答案改用 KaTeX 渲染
eb87168 feat: 题目页面全量 KaTeX 渲染 + 编辑页分栏布局
ba9b312 feat: 添加 KaTeX LaTeX 公式渲染
d342c1c feat: 实现审核队列 + 知识点管理页面
159b4f7 feat: 实现工作台 Dashboard 页面
1bc3505 feat: 实现题目编辑页 + API 层扩展
5329455 feat: 实现题目列表 + 题目详情前端页面
```

### 今日无新进展（5/9 晚上结束 — 任务暂停）

本次会话没有执行新的开发任务，所有待办事项保持原样。

#### 待办（按优先级）
| # | 任务 | 优先级 | 预估 | 说明 |
|---|------|--------|------|------|
| 1 | **Docker 部署配置** | 🟢 P3 | 1h | docker-compose.yml + 多阶段构建（PostgreSQL + Rust后端 + Nginx前端） |
| 2 | **知识点管理页增强** | 🟡 P2 | 1.5h | 拖拽排序、批量关联知识点到题目 |
| 3 | **试卷导出/打印** | 🟡 P2 | 2h | 试卷详情页添加导出 PDF/打印排版 |
| 4 | **题目批量导入增强** | 🟡 P2 | 1h | Web 端上传文件解析导入，支持 Word/LaTeX |
| 5 | **前端性能优化** | 🟢 P3 | 1h | chunk 拆分、路由懒加载、按需导入 |
| 6 | **用户管理增强** | 🟡 P2 | 1h | 禁用/启用用户、角色修改 |

---

## 2026-07-12（UI/UX 优化迭代）

### 今日完成

#### 1. 选择题选项自适应布局（Ghost Rendering）
- 实现 `computeOptionLayout()` 函数，通过预渲染测量 KaTeX 渲染后的选项真实宽度
- 临时切换 `display: block` + `inline-flex` + `white-space: nowrap` 测量 `scrollWidth`
- 根据测量结果动态选择布局：`grid-4`（一行四列）/ `grid-2`（两行两列）/ `grid-1`（四行一列）
- `ResizeObserver` + 防抖（150ms）实现响应式重算
- 阈值：`slot * 4 <= containerWidth` → grid-4；`slot * 2 <= containerWidth` → grid-2

#### 2. 答案解析区 KaTeX 渲染
- 题目卡片展开后，正确答案通过 `<LatexRender>` 组件渲染
- 覆盖选择题、填空题、解答题三种题型的答案显示

#### 3. 深色模式修复
- 统一 `localStorage` 主题键为 `mathset_theme`（原 `theme` 不一致）
- `ThemeToggle.vue` 改用 `useTheme` composable，移除独立主题逻辑
- `index.html` 添加 `theme-color` meta 标签
- 修复原生 `<select>` 在深色模式下 option 白底问题（`color-scheme: dark`）

#### 4. 题目编辑页（QuestionEdit）重构
- Apple 简约风格双栏布局：左栏编辑 + 右栏实时预览
- 题型分段控件（选择题/填空题/解答题/判断题）+ 3 星难度评级
- 元数据工具栏：年级/学期/分值/耗时/来源/知识点 水平排列
- 题干下方预留题目图片占位区域（虚线边框 + 图标 + 提示文字）
- 知识点选择弹窗：学段切换 + 递归 `findNodeByName` 三级树
- 高级设置折叠区：指定审题人 + 内部备注

#### 5. 个人/公共题库切换 → Apple 分段控件
- 替代原生 `<select>`，pill 背景 + 白色激活态 + 阴影
- 图标 + 文字组合（user 图标→个人，users 图标→公共）

#### 6. 筛选面板 Apple 简约风格
- 大写标签、圆角标签按钮、激活态阴影
- `grid-template-rows: 0fr → 1fr` 展开动画 + opacity 过渡

#### 7. 知识点树 ↔ 筛选菜单年级联动
- 创建 `useSelectedKp` singleton composable，共享 `kpLevel` 状态
- `KpTreePanel` 切换学段时通过 `setLevel()` 更新共享状态
- `QuestionList` 的 `gradeOptions` 改为 `computed`，根据 `kpLevel` 动态返回年级选项
- `watch(kpLevel)` 监听学段变化，自动重置无效的年级筛选

### 技术要点

- **Singleton Composable 模式**：`useSelectedKp.ts` 在模块级别声明 `ref`，实现跨组件状态共享
- **Ghost Rendering 测量法**：临时修改 DOM 样式测量真实渲染宽度，测量后恢复原样式
- **CSS Grid 动画**：`grid-template-rows: 0fr → 1fr` 实现高度自适应的展开动画
- **递归树遍历**：`findNodeByName` 递归查找知识点树中的学段节点

### 已知 Bug（待明天处理）

| BUG ID | 描述 | 优先级 |
|--------|------|--------|
| BUG-001 | 新建题目页「添加标签」知识点弹窗内容不正确 | 🔴 高 |
| BUG-002 | 录题页面题目必要属性（年级/学期/分值等）布局和联动需优化 | 🟡 中 |
| BUG-003 | 知识点弹窗业务逻辑需重新梳理（学段同步、数据源、回显） | 🔴 高 |

> 详细描述见 `docs/requirements.md` 第 14 节。

### Git 历史（本次迭代）

```
（待提交）feat: 个人/公共分段控件 + 筛选联动 + QuestionEdit优化
e405629 feat: 深色模式空间切换器修复 + 新建题目界面重构
6f5333e fix: 统一主题键名 + 添加theme-color meta标签
b821376 feat: 选项自适应布局 - 基于KaTeX真实宽度测量(Ghost Rendering)
9ba6178 fix: 选择题选项自适应阈值调整, 短选项正确显示为一行四列
29f37cc feat: 选择题选项自动布局 + 答案KaTeX渲染
e680b33 fix: 优化筛选动画平滑度 + 工作台全屏布局比例调整
d0c0db9 feat: Apple风格吸顶工具栏 + 筛选面板弹出式
3459155 fix: 内容区域铺满全屏, 覆盖 base.css 的 max-width 居中限制
1cf6851 feat: 全屏铺满布局 + 固定顶栏 + 标签式筛选面板
6a46c51 refactor: 侧边栏拆分为双卡片, 三栏布局滚动锁定
```

### 明日待办

| # | 任务 | 优先级 | 说明 |
|---|------|--------|------|
| 1 | 修复 BUG-001 + BUG-003：知识点弹窗业务逻辑重新梳理 | 🔴 高 | 确保弹窗内容与知识树一致，学段同步、数据源统一、已选回显 |
| 2 | 修复 BUG-002：录题页必要属性优化 | 🟡 中 | 年级联动、学期关联、视觉层级、交互优化 |
| 3 | 题目图片上传功能实现 | 🟡 中 | 将占位区域替换为实际图片上传组件 |

---

## 2026-07-14 选择题多选支持 + 选项卡片重构 + 预览净化

### 一、选择题单选/多选切换功能 (QuestionEdit.vue)

新增单选/多选切换能力，数据模型通过 `sub_type` 字段区分：

- 新增 `isMultiChoice` / `multiCorrectAnswers` / `isOptionCorrect` / `hasCorrectAnswer` / `displayCorrectAnswer` 等 computed
- 新增 `switchChoiceMode()` 函数：单选→多选时将 `correctAnswer` 包装为数组，多选→单选时取第一个元素
- 编辑器选项区：多选模式下 radio 自动切换为 checkbox
- 保存逻辑：payload 增加 `sub_type` 字段，`correct_answer` 始终以数组提交
- 加载逻辑：根据 `sub_type === 'multi'` 或答案数量 > 1 自动恢复多选模式
- 草稿恢复：`sub_type` 纳入草稿存储字段列表
- `question_type` watcher 切换题型时重置 `sub_type`

### 二、顶栏布局一致性修复 (QuestionEdit.vue)

解决选择题顶栏因单选/多选按钮导致与填空题布局不一致的问题：

- 从"题型"下拉框旁移除单选/多选块状按钮
- 清理 `.meta-field-type` 的 `inline-block` / `vertical-align` 补丁样式
- 所有 9 个顶栏字段高度统一为 57.5px，完美单行对齐
- 将切换器下迁至"答案"模块标题右侧，改为精简分段控制器 `.seg-toggle`（字号 11px，浅灰底容器 + 蓝色选中态）

### 三、选项输入行 Apple 风格胶囊卡片重构 (QuestionEdit.vue)

将碎片化的选项行重构为一体化胶囊卡片：

- 结构：`.opt-row` + `.radio-label` + `.opt-input` + `.icon-btn` → 统一 `.opt-card` 容器
- 默认态：`border-radius: 10px`，`background: #f5f5f7`，`border: 1.5px solid transparent`，`padding: 8px 12px`
- 输入框：`border: none; background: transparent; box-shadow: none; outline: none`，完全融入背景
- 删除按钮：默认 `opacity: 0`，hover 整个卡片时 `opacity: 0.6` 淡入，hover 按钮自身时 `opacity: 1` + 红色高亮
- 聚焦态：`:focus-within` 触发 `border-color: var(--accent)` + `box-shadow: 0 0 0 3px var(--accent-light)`
- 选中态：`.correct` 类触发 `background: var(--accent-light)` + `border-color: var(--accent)`
- 保留 `.opt-input` CSS 供填空题使用

### 四、Chrome 预览抖动修复 (QuestionEdit.vue)

修复预览区切换正确答案时的布局抖动：

- `.paper-opt.correct` 移除 `font-weight: 600`，所有选项统一保持 `font-weight: 400`
- 切换正确答案时不再发生字体粗细变化导致的布局位移

### 五、题目详情页预览样式净化 (QuestionDetail.vue)

统一详情页选项样式为纯净试卷排版：

- `.paper-opt` 移除卡片属性（`background`、`border`、`border-radius`、`padding`、`hover`、`transition`），改为纯文本 `padding: 4px 0`
- `.paper-opt-letter` 移除圆形徽章（`width/height/border-radius/background`），改为普通加粗文字
- `.paper-opt.correct` 从"绿底+绿边框"简化为仅文字变色 `color: var(--success)`
- 新增 `isMultiChoice` computed 支持多选题识别
- 字母格式从 `A` 改为 `A.`（补上点号，与编辑器预览一致）

### 六、预览区选项前缀净化 (QuestionEdit.vue + QuestionDetail.vue)

移除预览区选项前错误出现的 ○/□ 前缀符号：

- 移除 `<span class="paper-opt-prefix">{{ isMultiChoice ? '□' : '○' }}</span>`
- 清理两个文件中残留的 `.paper-opt-prefix` 和 `.paper-opt.correct .paper-opt-prefix` CSS 规则
- 选项前缀现在纯净显示为 `A.` `B.` `C.` `D.`，复刻真实纸质试卷排版

### 七、其他修复（已有改动一并提交）

- `api/client.ts`：401 拦截器改用 `window.location.href` 跳转，避免 router/store 循环依赖导致 HMR 问题
- `stores/auth.ts`：login/logout 跳转同样改用 `window.location.href`，消除循环依赖
- `components/LatexRender.vue`：新增 `\emptyset` → `\varnothing` 宏映射 + Unicode ∅ (U+2205) 预处理，符合国内教材椭圆空集符号

---

## 2026-09-02 导出引擎 M1 收尾（Markdown 端到端 + 前端接入）

对应 `docs/导出引擎与排版系统_开发任务分解.md` 的 T1.6 补漏与 T1.7-T1.10。

### 一、修复 crate 编译断裂（T1.6 遗留）

`markdown.rs`（965 行）随 T1.6 提交时漏了 `pub mod markdown;` 声明，导致 `src/export/mod.rs` 未导出该模块、
`cargo check` 自那次提交起即 E0432 失败，markdown 生成器的单测从未真正跑过。补齐声明后连带修掉：

- `build_zip` 两处 E0308：`ZipWriter::start_file` 返 `ZipError`、`write_all` 返 `io::Error`，不能 `and_then` 串接，拆成逐语句
- 测试模块导入：`AnalysisBlock` 应从 `crate::models::question_structure` 取（`model.rs` 只做私有 re-export，E0603）、补 `use std::io::Read`
- 删除恒等死函数 `indent_multiline`

### 二、`X-Export-Warnings` 截断口径修正（T1.7）

原实现按**原始 JSON 字节数**判断是否超 8000，而响应头承载的是 **percent-编码后**的字符串——中文 3 字节编码成 9 字符，
实际阈值被放大近 9 倍，B3 约定的截断形同虚设（旧断言写成 `<= 12000` 也正好把这个 bug 藏住了）。
改为按编码后长度逐条试探回退，超限时保留前缀 + `truncated:true` 哨兵；断言收紧到 `<= WARNINGS_HEADER_LIMIT`。

### 三、导出端点集成测试（T1.7）

新增 `tests/export_markdown_api.rs`（6 例，tower oneshot + 真实 `DATABASE_URL_TEST`）：
未认证 401、RFC 5987 中文文件名、frontmatter/大题分节/连续题号、B2 留白与公式原样保留、
三模式答案与解析位置矩阵、不可见题目降级为 `field=other` 警告、`?bundle=true` 返回 zip 且图片重写为 `images/`、空 sections。

### 四、`exportApi` 与类型（T1.8）

`frontend/src/api/client.ts` 新增 `exportApi.markdown/docx/pdf`：`responseType: 'blob'` + 单请求 `timeout: 60000`
（全局实例仍是 10s），解析 `Content-Disposition`（优先 `filename*=UTF-8''`）与 `X-Export-Warnings`，
并在 blob 模式下把后端 JSON 错误体读回文本再抛错。类型一律复用 ts-rs 产物 `api/types/exam.ts`，不手写。

### 五、`ExportDialog.vue` 首版 + 试题篮接入（T1.9）

- 新建 `components/ExportDialog.vue`：格式分段控件（Word/PDF 置灰并标 M2/M3 交付）、三张模式卡（学生练习/教师讲义/标准考卷，
  切换即联动答案/解析/卷末默认值）、内容开关组、教师提示框开关（非讲义模式禁用）、Markdown 的 zip 打包开关、
  loading 导出按钮、黄色可展开警告条（含截断提示）、`改用浏览器打印` 兜底入口
- `views/Basket.vue`：`downloadPaper()` 的 `window.print()` 占位移除，改为按页面所见 `groupedSections` 序列化
  `ExamSectionRequest` 打开面板；整卷、单大题两个入口都接上，print 兜底保留在面板内
- 分值序列化只在 `default_score > 0` 时下发，避免前端 0 覆盖后端 `metadata.default_score → 兜底 5` 的回退链

### 六、M1 手工验收记录（T1.10）

真实数据（4 道跨题型 + 1 道缺图题）走通浏览器全流程：

| 验收项 | 结果 |
| --- | --- |
| 三种排序模式与页面所见一致 | ✅ 按题型（一/二/三大题）与按加入顺序倒序（单组「按加入顺序（倒序）」）均与页面题号逐一对应 |
| 三模式内容矩阵 | ✅ 学生卷无答案无解析；教师卷内嵌 `**答案**`/`**解析**`；考卷卷末 `## 参考答案` + `## 试题解析` |
| 单大题导出 | ✅ 面板显示「导出范围：一、单选题（1 题）」，文件仅含该大题且题号从 1 起 |
| zip 打包与图片重写 | ✅ `application/zip`，缺图降级为警告不中断整卷 |
| 警告条 | ✅ 展开显示「第 2 题 · 图片 … 处理失败：本地图片不存在」 |
| 空试题篮 | ✅ 不开面板，toast「试题篮是空的，先去题库选题」 |
| print 兜底 | ✅ `window.print()` 调用 1 次且面板关闭 |

回归：`cargo test` 569 passed / 0 failed（15 个测试二进制，其中 `export::*` 单测 85 例）、`npm run build` 绿。

### 七、已知边界

- Word/PDF 入口置灰，分别随 M2/M3 交付
- 警告超过 8000 编码字符只保留前缀 + 哨兵，完整清单需等 T3.x 的预检/预览接口
- 面板打开时对分组做一次快照，期间不能改排序（遮罩已挡住页面），故无「改排序后面板内容不同步」的路径
- 验收用的 `export_ui_probe` 账号与其 4 道样题已从本地开发库 `mathset` 清除（连带其个人题库空间）
- 「按加入顺序」排法只有一个分组，分组名会原样成为大题标题（`## 按加入顺序（共 16 题 · 80 分）`），M2 前靠用户改标题或前端换中性文案
- 部分导入题题干自带源试卷题号（`**9.**（5 分）12. 设双曲线 …`），导出引擎按试卷顺序重排后不改写题干文本

### 八、visualtest 真实题库复测（导出阶段）

换 `visualtest` 账号（41 道可见题，题篮取 16 道跨三大题）把 T1.9/T1.10 清单再走一遍：

| 验收项 | 结果 |
| --- | --- |
| 三模式内容矩阵 | ✅ 学生练习 `mode: student` 无答案无解析；教师讲义内嵌 8 处 `**答案**`/`**解析**`；标准考卷题末 `## 参考答案` + `## 试题解析` |
| 卷末编号一致性 | ✅ 题干编号 = 参考答案编号 = 解析编号；只有 8 道已录入答案的题进答案区，其余不生成空占位 |
| 两种排法 | ✅ 按题型（一/二/三大题）与按加入顺序（单组）均重排为连续 1–16，与页面题号逐一对应 |
| 单大题导出 | ✅ 「导出范围：一、单选题（3 题）」，仅 3 道、`total_score: 15`、题号从 1 起 |
| zip 打包 | ✅ `application/zip` 50186 B：`exam.md` + `images/` 3 张，md 内 `/uploads/` 零残留 |
| 缺图降级 | ✅ 临时移开 `uploads/questions/bb8d4874-….jpg` 后仍 200 出包，`X-Export-Warnings` 一条 `{"field":"image","question_no":1,…}`，md 保留原 `/uploads/…` 引用；测毕按 sha256 校验还原 |
| 中文文件名 | ✅ `attachment; filename="…"; filename*=UTF-8''…` |
| 分值兜底 | ✅ 16 题 80 分（`metadata` 无 `default_score` → 逐题兜底 5 分） |

复测查出一个真实缺陷并已修：

- **学生用卷的 zip 夹带解析配图**。`collect_bundle_images` 无条件扫 `q.analyses` 与问树叶子答案/解析，而 `render_markdown`
  是按 `include_analysis`/`include_answer` 门控渲染的 —— 学生练习 + ZIP 时 markdown 里 0 张图片引用，包里却躺着 3 张仅教师可见的
  解析配图（内容泄漏 + 45 KB 冗余）。改为收集范围跟随渲染门控，补回归单测 `bundle_images_follow_analysis_switch`；
  修复后同一份 16 题请求：学生包 1 条目 0 图，教师包 4 条目 3 图。

本地数据缺口（非缺陷）：`mathset` 开发库里 0 道题标注了知识点/易错标签，「考点清单」「易错警示」两类 callout 只能靠单测覆盖；
打开「思路点拨」后 `> [TIP] 名师点拨` 由解析文本正常生成，callout 渲染链路在真实数据上已验证。

回归：`cargo test --no-fail-fast` 570 passed / 0 failed（16 个测试二进制，`export::*` 单测 93 例，含新增
`bundle_images_follow_analysis_switch`）。另注意到一个与导出无关的抖动用例：`tests/ai_tagging_engine.rs`
的 `test_engine_no_silent_top1_and_max_limit` 单线程连跑 3 次挂了 2 次（`超出上限的知识点不应被静默丢弃: []`），
`--no-fail-fast` 整轮里又通过，属 AI 标注侧的既有不稳定，未在本次范围内处理。

## 2026-09-02 导出引擎 M2（OMML + DOCX）

### 一、T2.1 ⛔ latex2mathml 覆盖率预扫描（决策门）

工具 `src/bin/scan_latex.rs`（只读，不写任何业务字段）：

```
cargo run --bin scan_latex [--limit N] [--samples M]
退出码 0 = 门下通过（语料降级率 ≤5%）；2 = 未通过，须停下评审备选方案
```

取 `split_content` 作为唯一切分口径（与导出引擎同一实现），扫 stem / analysis / options /
correct_answer / structure 五类文本里的全部公式，逐条跑 `latex_to_mathml`。

| 指标 | 实测 |
| --- | --- |
| 题目 / 公式 | 43 道 / 989 条 |
| 失败 | 10 条 → **降级率 1.01%** |
| 受影响题目 | 6 道（14%） |
| 按字段 | structure 5/326、analysis 3/369、stem 2/208、options 0/83、correct_answer 0/3 |

**根因单一**：10 条失败全是 `LatexError::UnknownEnvironment`，环境分布为 `array` 4 / `aligned` 4 / `cases` 2。
crate 支持 `matrix`/`pmatrix`/`bmatrix`/`vmatrix`/`align`，但不支持 `cases`、`aligned`、`array`、`gathered`、
`split`、`gather`、`dcases`、`rcases`、`eqnarray`、`subarray` —— 恰恰是教辅高频的分段函数与方程组写法。

**归一可行性已验证**（同一工具的探针集，52 条）：`matrix` 接受 `&` 对齐列与 `\text` 条件，且改写后必须产出的
定界形式 `\left\{…\right.` 与 `\left.…\right\}` 均可解析 → 把不支持环境改写成 `matrix` 能消除全部环境类降级，
无需引入 KaTeX 预转换或 temml。该改写落在 T2.2 的「输入归一」职责内（与既定的 `\emptyset→\varnothing` 同层）。

**两点口径提醒**：

- 43 道是本地开发库规模，不代表生产语料；上线前应在真实题库上重跑一次取基线（工具已具备，`--limit` 可控）。
- 门指标按「公式条数」算（1.01%），按「题目数」算是 14%——失败集中在少数题的长篇解析里。Word 里一处红字原文
  就足以让教师觉得「这份卷不能用」，故不能只盯 1.01% 这个数字。

**结论：门通过，按计划继续 `latex2mathml`**；`cases`/`aligned`/`array` 改写作为 T2.2 的必做项而非可选项，
非法输入（如 `\frac{1}{`）仍走「红色原文 + 警告」降级路径。

### 二、T2.2 latex2mathml 封装与输入归一

`src/export/math/mod.rs` 两个出口：`normalize(latex) -> Cow<str>`（源串级）与
`to_mathml(latex, display) -> MathOutcome::{Ok, Failed}`（失败只给 reason，调用方降级为「原文 + 警告」，
不参与控制流中断）。

归一做两件事：

- **符号**：与前端 `LatexRender.vue` 的 KaTeX 配置对齐 —— `\emptyset` → `\varnothing`、U+2205 `∅` → `\varnothing`。
  （**这一条在下一节被推翻**：方向恰好与 `latex2mathml` 的支持面相反。）
- **环境改写**：`cases`/`dcases` → `\left\{ \begin{matrix} … \end{matrix} \right.`、`rcases` → 镜像形式、
  `aligned`/`array`/`subarray`/`gathered`/`gather`/`split`/`eqnarray`/`smallmatrix` → 原位 `matrix`；
  `array` 系的列描述符 `{l}` / `{p{2cm}}` 按花括号平衡剥掉；`matrix`/`pmatrix`/`align` 等 crate 本就支持的不动。
  嵌套按深度配对递归处理，`\begin`/`\end` 不配对或名字不一致时**原样返回**，交给下游降级。

一个必须有的守卫：语料里 `\left\{\begin{array}{l}…\right.` 已是常见形态，改写时若再补一层会得到
`\left\{\left\{…\right.\right.` 直接编译失败，因此定界符**只补缺失的一侧**（前文已以 `\left\{` 结尾 /
后文已以 `\right\}` 开头时不补）。

代价：`array{l}` 的左对齐语义退化成 matrix 的居中。取它是为了换 0 条环境类降级，方向上不划算的说法也讲得通——
但红色原文比轻微对不齐糟糕得多，且 M3 的 typst 路径不吃这条归一（mitex 直接支持这些环境），届时 PDF 侧不受影响。

`scan_latex` 改为调用同一管线，实测：语料 989 条 **0 失败（0.00%）**；52 条覆盖矩阵仅「故意喂的 `\frac{1}{`」
按预期降级。T2.1 那张表里的 1.01% 由此成为归一前的历史基线，已写进工具的文件头注释。

测试：`export::math` 新增 14 例（含 cases/array/aligned/rcases/嵌套/未配对/定界符不重复补/T2.1 三条语料锚点）；
`cargo test --lib` 498 passed / 0 failed。

（**这个 0.00% 是虚的** —— 见下一节：`latex2mathml` 对不认的命令不报错，而是把错误文本混进 MathML 里返回 Ok。）

### 三、T2.2 补：命令别名、降级判定与 XML 修复

写 T2.3 的快照用例时，`f(x)=\begin{cases}x^2,&x\ge 0\\-x,&x<0\end{cases}` 的 MathML 里赫然出现
`<mtext>[PARSE ERROR: Undefined("Command(\"ge\")")]</mtext>`。两件事同时暴露：

1. **降级判定不能只看 `Result`**。crate 对不认的写法（`\ge`、`\varnothing`、`\Big`、`\lg`…）不返回 `Err`，
   而是把 `[PARSE ERROR: …]` 当文本节点塞进输出。之前 T2.1/T2.2 只判 `Err`，于是这类公式全部记为「成功」——
   真到 Word 里就是印着 `[PARSE ERROR: Undefined("Command(\"ge\")")]` 的一行。`to_mathml` 现在扫描输出，
   命中即 `Failed`，并把原因翻成人话（`Undefined("Command(\"ge\")")` → `不支持的命令 \ge`，同类去重、最多列 3 项）。
2. **空集方向映射当初做反了**。crate 认 `\emptyset` 和 U+2205 `∅`，**不认** `\varnothing`；前端 KaTeX 恰好相反。
   现在导出侧统一折算成 `\emptyset`，`∅` 原样保留（不再改写）。

据此把 crate 的支持面摸清（200 条命令逐个探针 + 带参数形态复测），补两类归一：

- **命令别名表**（约 50 条，只收录语义不变的折算，按 token 边界匹配所以 `\ge` 不会命中 `\geq`）：
  `\ge\le\ne→\geq\leq\neq`、`\dfrac\tfrac\cfrac→\frac`、`\dots→\ldots`、`\stackrel→\overset`、
  `\overparen\overgroup→\overbrace`、`\lg\lb\gcd\lcm→\operatorname{…}`、`\mathcal\mathsf\mathtt→\mathrm`、
  `\limits\nolimits\displaystyle` 直接丢掉、`\big\Big\bigg\Bigg`（含 l/r 变体）丢掉（crate 只认 `\left…\right`，
  最多退化成普通尺寸括号）、`\prime→'`、`\degree→^{\circ}` 等。会丢信息的写法（`\nleq`、`\cancel` 的斜线）
  **不改写**，宁可降级成红色原文。
- **参数级改写**（花括号平衡读参数）：`\textcolor{red}{x}`/`\boxed{…}`/`\href{}{}`/`\mathrel{…}` 取参数内容，
  `\phantom{…}`/`\hspace{…}`/`\kern` 整段删除，`\substack{a\\b}` → `\begin{matrix}a\\b\end{matrix}`。
  AI 生成的题里 `\textcolor`/`\boxed` 不少，crate 全不认。

还有一个必须修的硬伤：crate 把公式里的裸 `<` 原样写进文本节点 —— `x<0` 产出 `<mo><</mo>`，
**整串不是良构 XML**。`x<0` 在国内教辅是海量写法，而官方 XSLT 与 T2.4 要用的 roxmltree 都要良构输入。
`escape_stray_markup` 按 MathML 标签白名单判别，非标签开头的 `<` / `&` 一律转义（实体引用不二次转义）。

复测（同一工具、同一口径）：

| 指标 | 归一后 |
| --- | --- |
| 语料公式 | 989 条 / **0 失败（0.00%）** |
| 覆盖矩阵 | 52 条，非预期降级 1 条 |
| 受影响题目 | 0 道 |

剩下的 1 条是 `{1\over 2}`（TeX 原始 `\over`，本地语料 0 次），记为已知边界：真遇到会走「红色原文 + 警告」。
另有一条 crate 的硬限制：`\text{a & b}` 里的 `&` 会被当列分隔符报 `Undefined("RBrace")` —— 语料未见，同样按降级处理。

测试：`export::math` 20 例（新增别名表 token 边界、参数级改写、PARSE ERROR 判定与原因、转义幂等）；
`cargo test --lib` 504 passed / 0 failed。

### 四、T2.3 MML2OMML 黄金快照基建（R2）

本机 `C:\Program Files\Microsoft Office\root\Office16\MML2OMML.XSL` 存在，python 3.11.9 + lxml 6.1.2 可用，
无 xsltproc → 走 lxml 执行官方 XSL。

- `tests/snapshots/cases/*.mathml`：26 个 Presentation MathML 用例（22 个由真实管线跑出的 LaTeX 落盘 +
  4 个手写补 `mfenced`/`menclose`/`linethickness="0"`/`mathvariant` 家族/crate 与转换器都不认的构造），
  首行 `<!-- latex: … -->` 记录来源。
- `scripts/gen_omml_snapshots.py`：XSL 查找顺序 `--xsl` → `MML2OMML_XSL` → `assets/xsl/MML2OMML.XSL` → Office 目录；
  生成 `tests/snapshots/<名>.omml`；`--check` 只比对不写（CI 用），缺固件或不一致时列清单并退出码 1。
  输出剥掉 XML 声明、统一 `\n`、UTF-8，重复生成结果逐字节稳定（已 `--check` 验证）。
- `.gitignore` `/assets/xsl/*` + `!/assets/xsl/README.md`；`assets/xsl/README.md` 写清拷贝来源、用途、再生成命令，
  并强调**运行时绝不执行 XSL**（服务端不引 libxslt）。

快照给出的 OMML 事实（T2.4 的实现基准，与 §5.3 有几处出入，以固件为准）：

- `m:r` 里**没有** `w:rPr/rFonts="Cambria Math"`，只有裸 `m:t` —— 计划 §5.3 那句 run 属性写在 MathML→OMML
  转换器之外（Word 在 `m:oMath` 内自动用数学字体），否则黄金快照对不上。
- `munderover(∑)` → `m:nary` + `naryPr{chr,limLoc=undOvr,grow=1,subHide,supHide}`，且**被加和式落在 `m:e` 之外**
  （MathML 里 `<mi>i</mi>` 本就是 munderover 的兄弟），`m:e` 留空。
- `mspace` 被整个丢弃；`mtext`/`merror` → `m:r` + `m:rPr/m:nor`；`mathvariant` → `m:sty`/`m:scr`；
  `mfenced` → `m:d`+`sepChr`；`menclose[box]` → `m:borderBox`，`updiagonalstrike` → 四边 hide + `strikeBLTR`；
  `mmultiscripts` → `m:sPre`（官方支持，我们的「未知节点递归子节点」兜底留给更偏的构造）。
- 矩阵会生成完整 `m:mPr{baseJc,plcHide,mcs/mc{count,mcJc}}`，而 `pmatrix` / `cases` 的定界符落在 `<m:oMath>`
  的**首尾两个普通 `m:r`**（`(` `{`），官方实现也不给 `m:d` 包裹 —— 后果是括号不随矩阵高度伸缩。
  与 Word 自己粘贴 MathML 的结果一致，按固件为准，转换器不自作主张改形。

### 五、T2.4 MathML→OMML 转换器（黄金快照全绿）

`src/export/math/omml.rs`（roxmltree 读 + quick-xml 写，R3）。**55 个固件全部逐节点一致**，
且 `gen_omml_snapshots.py --check` 退出码 0 —— 即「Rust 产物 ≡ 官方 XSL 产物」这条链的两端都验过。

照抄 XSL 时最容易静默写错、写错了也不报错的几处（改本模块前先跑快照）：

1. **run 合流的相容判定有两个 `$sFontCur='normal'` 分支**（XSL 670–689）。后一个的条件是「该节点没有任何
   字体类属性」，正是它让相邻 `mtext` / `ms` 合成同一个 `m:t`；只抄第一个，「$-1$ 元」这类中英混排会被
   拆成一串单字 run，Word 里字距与字体全乱。
2. **XPath 的 `position()` 从 1 起算且把 base 算进节点集**：`cndSuperScript` 末尾那个 `- 1` 不是笔误；
   `SplitScripts`（XSL 2578）的 `m:sub` 收第 1、3、5… 个 = 0 基偶数下标。两处任一写反都会把上标印进下标位。
3. **`ancestor-or-self::mml:mstyle[@scriptlevel][1]` 是逆文档序** —— `[1]` 是**最近**的那个 mstyle，
   不是最外层。写成「循环里覆盖变量」就变成取最外层（`scriptlevel.omml` 断言 `m:scrLvl m:val="2"`）。
4. **`NaryHandleMrowMstyle` 会把 n-ary 紧跟的兄弟 `mrow`/`mstyle` 吞进 `m:e`**，同时 `mrow` 自己的模板靠
   `FIsNaryArgument` 短路不再输出。所以 `mrow` 的主体（线性分数 / `m:func` / 普通合流）必须抽成 `row_body`
   给两处共用，否则 `\int_0^1 f(x)dx` 的 `f(x)` 会掉到 `m:nary` 外面。
5. **roxmltree 的 `prev_siblings()` / `next_siblings()` 包含自身**（它的 `prev_sibling_element()` 内部就是
   `.skip(1)`）。`mpadded` 的「无元素兄弟」判定若写成轴迭代 + `any(is_element)`，永远为真。
6. **quick-xml 0.42 的 writer 什么都不转义**：文本要 `escape::partial_escape`、属性值要 `escape::escape`，
   否则公式里的 `<` 直接产出非法 XML（T2.2 补的 `escape_stray_markup` 是同一个坑的另一半）。
7. `mfrac` 的 `skw`（`bevelled`）、`menclose` 的 `actuarial` / `longdiv`（XSL 里整条分支什么也不输出）这类
   「看着像漏写」的分支一律照抄，不替 Word 补语义 —— 补了就对不上固件。

三处与 XSL 的有据差异（`mglyph` 分支不可达、不处理 `maligngroup`/`malignmark`、`@foo` 与 `@mml:foo` 不区分）
写在模块头注释里，对本管线的可达输入没有影响。

警告口径：面向教师的降级警告仍只在 `to_omml` 返回 `Failed` 时给（`MathOutcome` 与 `to_mathml` 同一套约定）；
未认出的节点走 XSL 的 catch-all（只递归元素子节点）+ `tracing::warn`，内容保住了就不打扰教师。

固件补了 6 个手写用例，覆盖原先没有断言的分支：`menclose`（top bottom / updiagonalstrike / actuarial /
circle right）、`table_labeled`（`mlabeledtr` + 变列数 → `m:m` 的 `mcs/count`，标签列被丢弃）、
`multiscripts` 与 `multiscripts_none`（前后脚本同时存在、`none` 占位吃掉另一侧）、`frac_types`
（bar / noBar / skw）、`scriptlevel`（嵌套 mstyle 取最近值）。

测试：`export::math` 26 例（黄金对比 1 例遍历 55 个用例 + run 合流切分 / `OutputText` 控制字符 /
n-ary 吸收后续 mrow / 未知节点不 panic 各 1 例 + T2.2 的 20 例）；`cargo test --lib` 510 passed / 0 failed；
`cargo clippy --lib --tests` 与 `cargo doc --no-deps` 本模块无告警。

### 六、T2.5 选项估宽与栅格决策（R7 口径）

`src/typeset/blocks/choice_grid.rs`（新增 `src/typeset/mod.rs` + `blocks/mod.rs`，`lib.rs` 注册）。
放在 `typeset::` 而不是 `export::` 下，是因为这份判定要被 docx `w:tbl`（T2.7）与 typst `grid()`（M3）
共用 —— 同一道卷子在两种格式里列数不同，比宽度估得粗更糟。纯函数、零 typst 依赖、不内置版面常量：
调用方把可用栏宽换算成 em 传进 `decide(options, available_em)`，返回 `ChoiceGrid { columns, rows }`。
换算基准统一按 1em = 10.5pt ≈ 3.7mm ≈ 14px（docx 默认样式正文字号），图片 px→em 与 `\hspace{}`
的 pt/mm/cm/px 都走这一条。

R7 的「按渲染字形数折算」实现成一个小扫描器（`Measurer`），几处不显然的取舍：

1. **「剥命令名」不等于「命令名都不占宽」**。`\frac` 的名字是宏、不印字，`\log` 的名字就是要排出来的
   三个字母 —— 所以有一张 `FUNCTION_WORDS` 表按 `名长 × 0.55em` 计，其余命令名一律计 0。
   另外 `\begin{array}{cc}` 的列描述符、`\\[6pt]` 的行距参数、`\displaystyle` 一类纯排版指令都占 0。
2. **竖排结构取 max 不取 sum**。分式/`binom`/`overset` 的两侧参数、`cases`/`matrix` 的多行，
   渲染宽度都由较宽的一侧决定。旧口径把这些全部相加，才是「12 字符估 6.6em」的根源。
3. **`\\` 只在自己那一层分行**。`1+\frac{\begin{cases}a\\b\end{cases}}{2}` 里内部分行只让内层取
   `max(a,b)`，外层仍是一行；但 `multiline` 标志要冒泡上来，让整题选项退到单列。若分行是全局的，
   这行的宽度会被算成 `a` 或 `b` 与 `2` 的组合，判错列数。
4. **单列的三个触发条件**：宽度比 > 50%、选项含 `LineBreak`/块级公式/表格/图组、公式内部多行。
   第三条哪怕很窄也单列 —— 一行里出现两个高矮不齐的公式块，比多占三行更难看。
5. **未知命令按 1em 估**，即宁可少排一列也不把溢出留在纸上；这条与「`COLUMN_SPEC_ENVS` 两处各持一份」
   是同一类保守选择：`export::math` 那份为了改写 matrix 要剥掉它，这份为了「这组花括号不占宽」要认出它，
   语义相反，合并反而会把两边绑住。

列数不超过选项数（3 个短选项 → 1×3，5 个 → 4 列 2 行），`available_em <= 0` 视为单列，
非法/未闭合 LaTeX 只估宽不 panic（用例里塞了空串、`\`、`}{`、`\text{未闭合` 等）。

顺带修掉 `export/math/mod.rs` 头注释里指向私有函数的 rustdoc 告警（`escape_stray_markup` 改普通代码标记）。

测试：`typeset::blocks::choice_grid` 16 例（em 口径、分式/上下标/根号/`\text`/函数名/定界符折算、
多行取最宽行、四档决策表 1×4 / 2×2 / 4×1 / 含公式、列数上限、长度单位、不 panic）；
`cargo test --lib` 526 passed / 0 failed；新模块 clippy 与 rustdoc 无告警。
真实题库的选项宽度分布留到 T2.8 手工验收时随卷核对（阈值按「最宽选项 ÷ 栏宽」单一指标，
未按总宽均摊，真卷若出现「三个短选项 + 一个长选项」会整体降一列，这是决策表的既定行为）。

### 七、T2.6 DOCX 打包器骨架（OPC 容器与静态部件）

`src/export/docx/mod.rs` 只管**容器与静态部件**：`[Content_Types].xml`、`_rels/.rels`、
`word/_rels/document.xml.rels`、`styles.xml`、`settings.xml`、`docProps/*`，以及 `document.xml`
的外壳（根元素命名空间 + 末尾 `sectPr`）。往里灌内容的是 T2.7 的 `docx/writer.rs`，接口就是
`Package { title, body, sect_pr, extra_rels, media }` + `build()`。

**单测不断言 XML 字符串，而是把产物重新解压校三条不变量**：①每个 XML 部件良构；
②`[Content_Types].xml` 的 Default(扩展名) / Override(部件路径)覆盖包里**每一个**部件；
③每条 `Relationship/@Target` 解析到包里真实存在的部件（基准目录取 `_rels` 的上一级 ——
`word/_rels/document.xml.rels` 的目标相对 `word/`，按字符串前缀算是错的）。
理由：这三条坏掉的表现是「Word 说文件已损坏」，不是少显示一块内容，比对字符串更贴近故障形态。

但**结构合法不等于 Word 认**，所以补了真机探针：`#[ignore]` 用例把最小包写到 `target/t26_probe.docx`，
`scripts/check_docx_opens.ps1` 用 COM 分别以 `Word.Application` 与 `KWPS.Application` 只读打开
（`AddToRecentFiles=false`，关闭不保存）。本机实测**两端都 PASS**：`pages=1`、`omaths=1`、正文 21 字。
`OMaths.Count == 1` 是这一节最硬的证据 —— 它说明公式是被识别成可编辑的 Office Math 对象，而不是图片。
探针标 `#[ignore]` 是因为 CI 没装 Office，跑它只会假失败；脚本对未注册的 ProgID 打 `SKIP` 并以退出码 2
收场，把「没装」与「通过」区分开。

首次编译暴露的坑（这模块写完一直没进过编译器，攒了一批）：

1. **`concat!` 只接受字面量，不接受 `const` 路径** —— 8 处 `XML_DECL` 与 6 处 xmlns 拼接全报错。
   改法是把这些拼接挪到 `format!` 的命名参数上（`{d}` / `{w}` / `{m}`）。命名空间声明走
   `ns_decl(prefix, uri)` 运行时拼，而不是把 URI 抄第二份进字面量：抄了就有两份真相，
   而单测判定文档根元素用的正是 `NS_*` 常量。
2. **`r#"…"#` 里 `"#` 就是终止符** —— `r#"<w:styles "##` 在 `"#` 处结束，剩下一个裸 `#` 成语法错误。
3. **raw string 不认行尾 `\` 续行** —— 测试里两处多行 XML 用了 `\` 续行，反斜杠与缩进会原样进
   `document.xml`，Word 里表现为正文多出一个 `\`。改成把整段拆成 `concat!` 的多个字面量分段。
4. `roxmltree::Namespace::uri` 是私有字段，要调 `n.uri()`。
5. 媒体部件是二进制，不能当 XML 解析 —— `parse()` 里加了 `is_xml_part` 前置断言，
   含图的用例（T2.7 会真加图）不会再被「良构」循环误伤。

静态部件的属性与子元素顺序按 Word 自身产出照抄：`w:pPr` 是 `keepNext → pBdr → shd → spacing → ind → jc`，
`w:settings` 是 `zoom → defaultTabStop → characterSpacingControl → m:mathPr → themeFontLang → clrSchemeMapping`。
Word 对顺序宽容，WPS 与严格校验器不容 —— 而 DoD 要求两端都能打开，所以按严格的来。
`m:mathPr`（`mathFont` = Cambria Math）落 `settings.xml`；`brkBinSub m:val="--"` 那两个连字符是 Word
自己写出的字面量，不是没填完的占位符。

测试：`export::docx` 9 例（部件齐全且入口最先、每个 XML 部件良构、两条不变量、文字与真实管线产出的
`m:oMath`、样式与 `m:mathPr` 缺省值、`pStyle` 引用全部有定义、媒体与关系同包同进同出、标题转义、A4 页面），
另 1 例 `#[ignore]` 探针；`cargo test --lib` 534 passed / 0 failed；本模块 clippy 与 rustdoc 无告警，
`rustfmt --check` 退出码 0。页脚部件与 `m:oMathPara` 都留给 T2.7，`extra_rels` 已为其留出接口
（自定义 rId 需从 `rId3` 起，`rId1/rId2` 被 styles/settings 占了）。

### 八、T2.7 docx writer 与 ⛔R5 决策门（keepNext 实测）

`src/export/docx/writer.rs`：`generate_docx(bundle, options, upload_dir) -> DocxResult { bytes, issues }`。
IR → OOXML 字符串全在这里，`mod.rs` 只当容器。图片抓取必须先做、渲染必须是同步的 —— 题目树递归
里没法 `await`，所以 `prefetch()` 先按 URL 去重把字节读进 `HashMap`（**不重试**，读不到就是缺图），
再同步渲染。产物 `issues` 与 Markdown 侧同一套 `Issue` 口径，坏公式只降级不失败整卷。

#### ⛔ R5：`w:keepNext` + 紧随其后的 `w:tbl`，Word / WPS 到底分不分页

这条决定选项栅格留 `w:tbl` 还是退回 `w:tabs`，**只能实测**。判据设计成**对照实验**而不是单文件读数：
`scripts/strip_keepnext.py` 从 writer 产物里剥掉全部 `w:keepNext` 得到同结构的负对照，
`scripts/check_keepnext.ps1` 用 COM 只读打开两份，逐对「题号段 → 选项表」量
`violation = 题号段页码 != 首行起始页码`。2026-09-02 本机实测（24 道题、A4、10.5pt、17 页）：

| 编辑器 | 探针（带 keepNext） | 负对照（剥掉） |
| --- | --- | --- |
| Word 2016 (`Word.Application`) | **0 / 24** 违例 | **8 / 24** 违例 |
| WPS (`KWPS.Application`) | **0 / 24** 违例 | **8 / 24** 违例 |

对照组里被孤立的题号段停在页尾（`stemY≈717`，可用底部 771），它的首行选项搬到下一页 —— 失效形态
真实存在且可测。同一份内容加上 `keepNext`，两端 24 对全部不分离，且总页数与字符数逐字不变
（`pages=17 chars=9122` 两端一致）。**结论：keepNext 在 `w:tbl` 之前确实生效且两端一致 → 选项栅格
保留 `w:tbl`，不退回 `w:tabs`。**

探针标量之外，脚本还要求「对照组必须违例」：单看探针 0 违例是可以作假的 —— 第一版夹具每题一张 40 行的
表，每道题独占一页，16 对里 15 对根本没到过分页边界，`violations=0` 是白给的。改成「1 行题号 + 22 行选项」
让一页装两道，边界才落在题目中间。**夹具没压力时判 INCONCLUSIVE 而不是 PASS**，这条已经写进脚本的判据。

#### 几处不显然的实现口径

- **`m:oMathPara` 由 writer 负责**（R2 的分工）：`to_omml` 永远只返回根 `m:oMath`，块级公式要单独成段时
  才由 writer 套 `m:oMathPara`。用例断言「两个 `m:oMath` 里恰好一个被包进 `m:oMathPara`」。
- **只嵌 png/jpg/jpeg/gif**：`[Content_Types].xml` 的 Default 就只有这四个扩展名，塞一个 webp 部件进去会让
  不变量②（每个部件都要有 content type）失效 —— 表现不是「图不显示」而是**整个文件被判损坏**。其余格式
  与读不到的图一样降级成 `[图片缺失 …]` 占位段 + Image 警告。尺寸从 PNG/GIF/JPEG 头部自己解析（不引 image
  crate 到这条路径），按 96dpi 折 EMU，宽 14cm / 高 24cm 双上限。
- **单位换算只有一套**：1em = 10.5pt = 210 twips，A4 可用宽 = 11906 − 1418×2 = **9070 twips**，选项可用宽
  再减 420 缩进 → `GRID_EM=(9070-420)/210≈41.19`，正好对上 T2.5 的 25% / 50% 阈值。`choice_grid` 里的
  `PX_PER_EM` 是私有的，writer 自己持一份常量而不是把它挪成 pub —— 两边换算口径若漂了，用例（1×4/2×2/4×1
  三档）会先炸。
- **页脚三件套**：`[Content_Types].xml` 的 Override + `word/_rels/document.xml.rels` 里
  `Type="…/relationships/footer"` 的关系 + `sectPr` 里 `footerReference`（**必须在 `pgSz` 之前**，schema 顺序），
  rId 从 `rId3` 起接。`PAGE` 域写成 `<w:instrText xml:space="preserve"> PAGE </w:instrText>`，少那对空格
  Word 会把域名连成一串解析不了。
- **答案/解析的开关门控与 `markdown.rs` 逐字节一致**：只按 `options.answer_at_end` 决定就地还是卷末，
  模式过滤在上游 `assembler.rs`。图片收集直接复用 `markdown::collect_bundle_images`（为此改 `pub(crate)`）——
  学生用卷的包里不能夹带只有教师该看到的解析配图，两种格式打包出的图片集合必须同一份真相。
  顺带记一笔：`ExportOptions.answer_space` 两个生成器都没用到，是计划里挂着、实现里没有的字段。

#### 踩过的坑

1. **相邻两张 `w:tbl` 会被 Word 合成一张表** —— 表后紧跟表必须在中间垫一个 `w:p`（`SPACER`），卷末与
   单元格里也是；同理每个 `w:tc` 的最后一个块级元素必须是 `w:p`（ECMA-376 §17.4.5.7），否则严格校验器与
   WPS 会丢整格 —— 富文本用例遍历每个 `w:tc`，断言它的最后一个元素就是 `w:p`。
2. **`cargo check --lib` 看不见 `#[cfg(test)]` 里的错**。T2.6 抽出来的 `test_support` 一直没进过编译器，
   `part()` 返回 `Vec<u8>` 而调用处要 `&[u8]`，`check` 全绿而 `cargo test --lib` 才报 E0308。之后一律用
   `--no-run` / `test` 而不是 `check` 来判「测试代码编不编得过」。
3. **roxmltree 0.21 没有 `prev_element_sibling`** —— 判「选项表前一个块是题号段」只能用 `prev_sibling()`
   + `is_element()` 自己走。
4. **PowerShell 5.1 按 GBK 读无 BOM 脚本**：注释里的中文字节可能被解成多余的引号/花括号，整个脚本
   报「Try statement is missing its Catch」这种莫名其妙的位置错误。两个 `*.ps1` 现在都带 UTF-8 BOM 存，
   脚本头部注明这条约束（改完文件要确认 BOM 还在）。控制台输出同理一律 ASCII，中文只在注释里。
5. **`-not $comObject` 判不出「还没找到」**：Word COM 对象的布尔转换不随引用变化，反向遍历段落找题号段
   的 `while (-not $stem)` 一路退到文档第 1 段，于是 16 对全被「题号以数字开头」的过滤器丢掉，
   脚本安静地报 `pairs=0` 还顺带给了个 PASS。改成 `$null -eq $stem`，并且 pairs=0 一律判 INCONCLUSIVE。
6. **几何推算压力这条路是死的**：Word 对 auto 行高返回 `Rows.Item(1).Height = 9999999`，行尾标记的
   `Range.End-1` 被报回**该行起始页**的纵坐标，据此算出的「受压对」恒为 0。改用对照实验后同一份夹具
   立刻暴露出 8 处真实分离 —— 排版行为要问排版器本身，别问它的度量 API。

测试：`export::docx` 19 例（三档栅格、表跟随题号段、`oMathPara` 唯一、坏公式降级不失败整卷、图片只嵌
可读格式且字节完好、px↔EMU 双上限、三种图片头解析、答案/解析两开关、卷头信息与分值表、页脚域与关系、
富文本卷的 `w:tc`/样式不变量），另 2 例 `#[ignore]` 探针（T2.6 最小包 + R5 夹具）；
`cargo test --lib` 545 passed / 0 failed / 3 ignored；`cargo clippy --lib --tests` 本模块仅测试代码里
`field_reassign_with_default` 一类与既有 `markdown.rs` 同形的告警；`cargo doc --no-deps` 3 条告警全在
`ai/ocr` 与 `models` 的旧文件里，新模块为 0；`rustfmt --edition 2024 --check` 对新增的 `docx/mod.rs` 与
`docx/writer.rs` 退出码 0。`markdown.rs` **刻意保持原排版**：对它跑 rustfmt 会顺手改到 200 行已提交代码
（import 顺序、let 链、结构体字面量折行），把两个可见性改动埋进格式噪声里 —— 本仓库整体不是
rustfmt-clean，格式统一该单独一刀，不混进功能提交。

### 九、T2.8 `/export/docx` 端到端与 M2 收尾

**端点**。`handlers/export.rs::export_docx` 与 `export_markdown` 同骨架：装配 → 生成 → 合并警告 → 文件响应。
两条链路的尾巴此前各写一份，这回收成 `file_response(content_type, ext, title, body, issues)` —— RFC 5987
文件名编码、`X-Export-Warnings` 的 URL 编码与 8000 字符截断（超限补 `truncated:true` 哨兵）现在只有一处真相。
理由不是省事：两种格式的警告语义若漂了，前端表现为「Markdown 有警告、Word 静默」，是最难查的那类差异。
docx **没有 `?bundle=` 开关** —— 图片一律内嵌进 OPC 包，包本身就是 bundle。路由挂 `src/lib.rs` 的 `/export/docx`。

**测试**（`tests/export_docx_api.rs`，4 例 oneshot 打真库）：无 token → 401；正常卷 → 200 + OOXML content type
+ `filename*=UTF-8''%E9%9B%86…` + `PK\x03\x04` + 7 个必备部件 + `m:oMath==4` 且 `m:oMathPara==1` + 无警告头；
含 `$\frac{1}{$` 的卷 → 仍 200、`m:oMath==3`、原文留在纸上、警告 `{field:"stem",question_no:1,latex:"\frac{1}{"}`、
无截断哨兵；题目 ID 不存在 → 跳过该题并记 `field:"other"` 警告，`参考答案` 段照常出。三处口径值得单独记：

- **文字断言一律走 `text_of()`**：把全文档的 `w:t` / `m:t` 拼平再比。OMML 转换器会按结构切 run，
  对着原始 XML 找子串会因为 run 边界假失败（同一公式两次转换的 run 数不保证相同）。
- **计数走 roxmltree `has_tag_name((NS_M,"oMath"))`**，不数 `<m:oMath>` 字符串 —— 带属性的开始标签
  （`<m:oMath …>`）字符串数法会漏。roxmltree 0.21 的 `ExpandedName` 没有 `local_name`，只能这么判。
- **`E0502` 的所有权坑**：`ZipArchive::by_name` 要 `&mut`，而 match 的 scrutinee 已经借住 `archive`
  并跨所有分支，`unwrap_or_else` 里再捕获 `archive` 同样编不过。修法是先收集 **owned** `Vec<String>`
  文件名（`Vec<&str>` 仍持借用）再进 match，把报错文本里的包内清单留着 —— 缺部件时只说「缺」没用，
  要说「包里有什么」。

**前端**：`ExportDialog.vue` 的 Word 芯片从 `enabled:false` 打开（标签「Word」、提示「公式可编辑」），
`runExport` 按 `format` 分派 `exportApi.docx` / `exportApi.markdown`，扩展名兜底三档 `docx / zip / md`。

**真机验收**（浏览器登录 `visualtest` → 试题篮组卷 → 导出 Word；`POST /api/v1/export/docx` 200、33,181 字节，
落 `target/t28_browser_export.docx`）：包内 11 部件（含 `word/media/image1.jpg`、`image2.jpg`、`footer1.xml`），
`document.xml` 实测 `oMath=34 / oMathPara=0 / w:tbl=4 / w:drawing=2 / 红色降级 run=1`；
`scripts/check_docx_opens.ps1` 两端 **PASS** 且逐字相同：`Word.Application pages=3 omaths=34 chars=1032`、
`KWPS.Application` 同。`OMaths.Count` 与从 XML 里数出的 `m:oMath` 相等，就是「双击公式进编辑器」的机器判据 ——
任一编辑器把它当图片，这个数会直接掉到 0。`oMathPara=0` 与切分规则自洽：只有 `$$…$$` / `\[…\]` 才
`display:true`（`content.rs:92`），这份真实卷的公式全是 `$…$` 行内，所以 writer 不该套 `m:oMathPara`；
该分支另有单测 `display_math_gets_one_mathpara` 盯着，不是丢了。

**验收暴露的两处**：

1. **降级原因串是英文原句**。`latex2mathml` 对 `\frac{1}{` 报 `The token "RBrace" is expected, but the
   token "EOF" is found.`，直接进教师看的警告里不合适。修在源头 `math/mod.rs`：
   `Failed(format!("公式无法解析：{}", …trim_matches('"')))` —— 裁掉 crate 错误串两端的裸引号，前面补中文语境；
   单测改判前缀且断言不以引号收尾。API 层只判「非空」。剩下的已知边界：中文前缀 + 英文核心句，够教师定位
   「哪一题的哪一段坏了」，不够他知道「怎么改」。
2. **奇数个 `$` 会让降级扩散**。第 3 题的干里 `$` 不成对，切分器按 `$…$` 贪婪配对，把「，比较」「 与 」
   两段中文吃成了公式对象（实测含中文的 `m:t` 恰好这 2 枚，它们算在 34 里），同时纸上留了一个游离 `$`
   （`g(2)$ 的大小。`）。畸形输入下的既有行为，先记账：要不要在「配对片段里中文标点占比过高」时退回字面量，
   等教师反馈再定，不提前加启发式。

**M2 收尾**。全量 `cargo test` 退出码 0（`--lib` 545 passed / 0 failed / 3 ignored，集成 13 个 target 共
90 passed，含新增的 4 例）；`python scripts/gen_omml_snapshots.py --check` 55 例固件 exit 0；
`npm run build` ✓ 27.57s；本阶段新增与改动的文件（`handlers/export.rs`、`export/docx/*`、
`export/math/mod.rs`、`tests/export_docx_api.rs`）clippy 0 告警，`cargo doc --no-deps` 干净。
`handlers/export.rs` 里既有的 5 处 rustfmt 差异（第 10/20/24/149/346 行）与 `markdown.rs` 同一口径处理：
只保证新写代码 clean，不动已提交排版。

M2 到此交付完：`LaTeX →(规范化 + latex2mathml)→ Presentation MathML →(MML2OMML 等价转换器)→ OMML → OOXML`，
Word 与 WPS 里公式可编辑，一枚坏公式只降级不失败整卷。M3 是排版系统与 PDF 委托。

## 2026-09-02 导出引擎 M3（排版内核与 PDF 基础版）

### 一、T3.1 typst 全家桶依赖引入与构建耗时基线（P4）

版本按实施时 crates.io 稳定版锁：`typst` / `typst-pdf` / `typst-svg` / `typst-assets` = **0.15.1**，
`mitex` = **0.2.4**（计划 §依赖清单写的 `0.3` 当时尚未发布，不是笔误），`comemo` 0.5.1，`ecow` 0.2。

**ecow 必须跟 typst 的传递依赖同主版本**。一开始照最新钉 0.3.0，`Cargo.lock` 里同时编进 0.2.6 与 0.3.0 两份，
实现 `World` 时报的是「两个不同的类型相遇」—— 因为 typst 0.15 的签名用的是 0.2 的 `EcoString` / `Sm`。
最坏的是报错点在使用处而不是依赖声明处，回看要绕一圈。现在回钉 0.2 并在 `Cargo.toml` 注释里写明这条约束。

构建耗时（P4 要求实测，不测就不知道代价）：`Cargo.lock` 361 → 545 个包（+184）；dev 首次含新依赖全量构建
**10m54s** 退出码 0；release 首次全量 **19.9 分钟** 编完依赖树，修复后 release 增量重建 **2m55s**。
两个 profile 都不需要额外的 Windows 原生工具链 —— 新增依赖里唯一的 `-sys` 是 `linux-raw-sys`
（target-gated，Windows 根本不参与编译），typst 的字体/图形栈（`rustybuzz` / `ttf-parser` /
`resvg` / `pdf-writer` / `zlib-rs`）全是纯 Rust。副作用记一笔：typst 的 feature 统一把 `image` 重新编了一遍
（多解了几种格式），这是构建时长与产物体积的常态成本，不是配置错了。

`World` trait 摸底（供 T3.5）：0.15.1 是 **7 个方法**（`library` / `book` / `main` / `source` / `file` /
`font` / `today`）；PDF 出口 `typst_pdf::pdf(&PagedDocument, &PdfOptions)`；`mitex::convert_math` 返回
`Result<String, String>`，与本项目 `MathOutcome` 的 Ok/Failed 口径天然对齐。

### 二、T3.2 `LayoutSpec` 九字段与四内置预设

`src/typeset/spec.rs`：§6.1 字段定齐（`paper` / `columns` / `margins` / `binding` / `header_footer` /
`profile` / `fonts` / `answer_blank` / `color`）+ 4 预设（A4 讲义单栏、A4 练习双栏、A3 对折双栏考卷、
A3 三栏考卷）+ `for_profile()` 的 mode→默认 spec 映射（讲义 A4 单栏、学生 A4 双栏、考卷 A3 对折双栏）。
**依赖方向守死：typeset 不 import export** —— `ExportMode → OutputProfile` 的翻译留给适配器（T3.3），
spec.rs 只交出合并规则，一个 export 符号都不碰。

B5 留白合并落成 `resolve_blank(options_height_cm)`，口径是**开关在 options 手里**：`None` 就不留白
（spec 的兜底高度不许自己生效，否则「没勾留白」的卷会被 spec 悄悄加一块石板），`Some(h)` 才留白；
`h` 非正数视为「没填高度」退回 spec 的 6cm；样式恒取 spec。三档都有断言。

版心宽只有一处算法：`column_width_mm() = (纸宽 − 左右边距 − (n−1)×栏距) / n`，A4 单栏 174 / A4 双栏 86 /
A3 三栏 124 各一条断言；`column_gutter_mm()` 在单栏时归零，免得母版写出没有意义的 `column-gutter`。
覆盖语义是字段级的：所有嵌套结构都 `#[serde(default)]`，只给 `{"binding":{"position":"center_fold"}}`
也能反序列化，未知键一律忽略 —— 前端会带未来版本的键，400 不是正确表现。

ts-rs 导出 `frontend/src/api/types/layout.ts`（14 个类型，B6），`ExamRequest.spec` 从「透传 JSON 占位」
改成 `Option<LayoutSpec>` + `#[ts(optional)]`（前端是可缺省键而不是 `LayoutSpec | undefined`）。
测试 12 例 + 预设序列化固件 `tests/snapshots/layout_presets.json`；固件是**编译期 `include_str!`** 读的，
改预设会先编不过，所以重生成走一枚 `#[ignore]` 的写盘用例。

偏离计划一处：§6.1 纸张注里的「双面标记」没做成字段 —— M3 无人消费它，奇偶外侧对齐属 T4.7，
届时随逻辑页码一起加，避免空字段先进前端类型。

### 三、T3.3 `LayoutDoc` IR 与两域之间唯一的桥

`src/typeset/ir.rs`（排版域）三条不变式写在模块头：**① 没有裸文本**，一切文字到 IR 已是 `InlineNode`
（公式在导出域就归一过了，typeset 不再解析 stem）；**② 每个块自带 `BlockMeta{breakable, keep_with_next}`**，
母版不需要回头查上下文，四档语义构造子 `flow()` / `attach()` / `glued()` / `solid()`；**③ 线性块序列**，
分页、跨栏、藏答案由下游 `typst_gen`（T3.6）按 spec 决定，IR 不掺和版式细节。

`src/export/pdf.rs` 是 `ExamBundle → LayoutDoc` 的**唯一桥**，反向依赖为零：typeset 只借用 `export::model`
里的纯数据类型（`InlineNode` / `ExamOption` / `Callout`），不碰 assembler / generator / handler。这条方向在
两个模块头都写死了 —— 不写下来的话，迟早有人图省事从 typeset 里 import 装配器，那时两个域就焊死了。

B5 在这座桥上落地：留白的**开关与高度**在 `options.answer_space`（题级 override 优先于卷级），
**样式**恒取 `spec.answer_blank`；两者冲突时 options 赢，并且只补**一枚卷级 `Info`** 而不是每题一枚 ——
警告要进 `X-Export-Warnings` 头，B3 的 8KB 截断线经不起逐题刷。教师讲义（`profile == Teacher`）整卷不留白，
答案直接折进解析块。答案两条路各自独立：`answer_at_end` → `doc.answer_key`（卷末），否则题干后紧跟一块
`LayoutBlock::Answer`；`include_answer` 只管答案、不管解析，这条在 markdown 里已经踩过，IR 层用断言钉住。

选项栅格与 docx **共用同一个决策**：`choice_grid::decide` 的可用宽从 `spec.column_width_mm()` 换算，新增
`MM_PER_EM = 3.7` 与 `em_from_mm()`（1em = 10.5pt ≈ 3.7mm），再减掉 hanging 缩进 2em（与 docx 的
`INDENT_TWIPS = 420` 对齐）。同一份卷在 `a4_lecture` 排 4 列、在 `a3_tri_exam` 排 2 列 —— 两个出口各自
判一次列数是迟早会漂的那种重复。测试 16 例（块形状 5 / spec 3 / 留白 4 / 答案 3 / 退化输入 1），
`cargo test --lib --offline` = 587 passed / 0 failed / 4 ignored。

偏离计划一处：`ExamRequest.spec` 的合并语义从「字段级覆盖」改成**整体替换**。字段级合并要把九个嵌套结构
都写一遍 merge，而 M4 之前唯一的消费者是前端预览，它总是带整套 spec 过来；换成整体替换后 `resolve_spec`
只有三行，代价是前端只能带完整对象 —— 反正所有结构都 `#[serde(default)]`，缺键即取默认值，不会 400。
`model.rs` 字段注释与 `exam.ts` 同步更新（ts-rs 会把 Rust 文档注释原样抄进 `.ts`，改注释就得重跑绑定测试）。

### 四、T3.4 mitex 管线：把「一枚坏公式炸掉整卷」挡住

`src/typeset/math.rs`。公开面只有三样：`to_typst(latex, display) -> Result<String, String>`、`degraded(latex)`、
`MITEX_PREAMBLE`（随生成源码注入一次的定义块）。**typst 里一个解析不出来的标识符是 `unknown variable`，
那是整卷编译失败**，比少排一个公式严重一个量级 —— 所以这一节真正的产出不是转换器，是那道降级闸门。

mitex 0.2.4 的实际契约（读 `mitex-0.2.4/src/`，不是读 README）：

- 公开 API 只有 `convert_text` / `convert_math` / `convert_math_no_macro(input, Option<CommandSpec>)`。
- **传进去的 `spec` 只作用于 parse 阶段**，convert 阶段仍取 `mitex_spec_gen::DEFAULT_SPEC`（`converter.rs` 尾部）。
  所以「给 mitex 一张自定义命令表」这条路是无效的；扩展覆盖只能事后补 —— 输出是纯文本，缺什么名字就在
  preamble 里定义什么名字。
- `convert_math` 交出的是**裸数学体**，不含 `$…$`。typst 的块级/行级判据是「开引号后紧跟空白」，所以
  `display` 走 `$ … $`、行内走 `$x$`，包装留给我们做。
- 对不配平括号是**宽容**的（`\frac{1}{` 实测 `Ok`）。所以语料守的不是「非法输入报错」，而是「任意输入只能
  Ok 或 Err，不许 panic」—— panic 会打断整卷，这才是真风险。
- 两处会静默改变语义的输出：`\%` 原样写成裸 `%`（typst 里是行注释，方程尾巴整段没了）→ 出口 `escape_percent`；
  `\text{}` 转成 `#textmath[…]` → 这个名字必须我们自己定义。

**守卫 `unresolved_name` 只看四种「名字位置」**，其余字母串在数学模式里只是普通文本（`$abc$` 合法）：
`#name`（查全局作用域）、裸 `name(`、裸 `name.`（查数学作用域）、以及第 4 条 —— 裸 `name` 但它出自 mitex
词表。第 4 条是 75 枚语料逼出来的：`A\cap B` 转成 `$A sect  B$`，`sect` 后面既没 `(` 也没 `.`，前三条整条
放过，typst 却报 unknown variable。**typst 0.15 把 `\cap` 的符号名从 `sect` 改成了 `inter`，mitex 0.2.4 还在
写旧名**，preamble 里补 `#let sect = math.inter`。

三张名单一律不手抄，全部运行时现读：mitex 会吐哪些词 ← `DEFAULT_SPEC` 的 `alias.unwrap_or(key)` 取根段；
typst 认哪些名字 ← `Library::default()` 的 `math.scope()` / `global.scope()`；我们定义了哪些 ← 从
`MITEX_PREAMBLE` 文本里扫 `#let`。理由很实在：**改一边忘一边就是整卷编译失败**，而 typst/mitex 升级会同时
改名与新增。代价是 `mitex-spec-gen` 得从 dev-dependencies 提到 dependencies —— 它本就是 mitex 的非 dev 依赖，
已经在二进制里，不增加体积。`preamble_names()` 与模板同源之后，T3.6 只要保证把这块原样输出一次即可。

两个方向相反的坑：**`zws`（零宽空格）与 `space.nobreak`（LaTeX 的 `~`）都是 typst 原生数学符号**，preamble
里原先各写了一行 `#let`，实测等于把原生行为盖掉。于是加了一枚 `preamble_never_shadows_a_typst_definition`
断言 —— 现在 preamble 只允许定义「typst 两个作用域里都查不到」的名字。同类判例：间距命令里 `\,` `\;` `\:`
正好落在原生 `thin` / `med` / `thick` 上，而 `\!` 是 mitex 自造的 `negthinspace`，typst 0.15 无此名，只能降级。
`math.root` 的 index 是**可选位置参数**，`root(x)` 报 `missing argument: radicand`（实测），所以 `mitexsqrt`
必须按参数个数分流到 `math.sqrt` / `math.root`。数学函数（`mat` / `frac` / `display` …）只活在数学作用域，
顶层 `#let` 的函数体按代码作用域解析看不见它们，必须写 `math.mat(..)`。

测试：math.rs 16 例（4 枚 `#[ignore]` 探针负责把 mitex 词表、未解析名字、typst 符号修饰符打出来核对）。
语料常量 `CORPUS`（75 条，覆盖分式/根式/嵌套上下标/集合逻辑/向量几何/矩阵数组分段/中文与百分号/大括号标注）
与 `UNSUPPORTED`（5 条已知「mitex 转得动但 typst 不认」）**同时**喂给 T3.5 的编译测试 —— 守卫说能过的必须真
编译得动，两边交叉验证，否则守卫会悄悄变严（无谓降级）或变松（整卷失败）。

一处小坑记在这：`MITEX_PREAMBLE` 是普通字符串字面量，**里面的 `//` 注释也受转义规则管**，写 `\cap` 会让
rustc 报 `unknown character escape`，得写 `\\cap`。

### 五、T3.5 typst 编译器：手写 World、进程级字体池、两个出口

`src/typeset/compiler.rs`。公开面：`CompileRequest` / `compile_pdf` / `compile_svg_pages` / `Compiled<T>` /
`CompileError` / `CJK_FAMILIES` / `missing_cjk_families`。typst 0.15 的 `World` 是 7 个方法，手写即可，
不需要 `typst-eval` 之类的内部 crate。几处只在编译时才浮出来的事实：`PagedDocument` / `Page` 定义在
`typst-layout` crate，`typst` 门面没有把它导成 `typst::layout`；`Library::default()` 要把 `LibraryExt` 引进
作用域才看得见；`ecow` 必须跟 typst 的传递依赖同主版本（钉 0.3 会编进两份 ecow，`Sm` 变成两个不同类型，
`World` 实现直接对不上）。

字体走 R6：**运行时读目录，禁 `include_bytes!`**。`typst_assets::fonts()` 要 `fonts` feature —— 它不在
typst-assets 的 default 里，依赖树里也没人打开，漏开就拿到空迭代器、任何文档一个字体都没有（一枚断言盯着）。
整个进程按「目录集」记忆化字体池：83MB 的思源 OTF 逐请求解析，单卷 500ms 的目标当场破产。同一份字体可能在
两个目录各一份，用「长度 + 首尾 64 字节」指纹粗去重。

**缺中文字体 typst 一声不吭**：`#set text(font: "unknown family")` 连告警都不发，直接回退成豆腐块。所以
只能主动查 `FontBook::contains_family` —— 而它的族名索引是**小写键**，传 `"Source Han Serif SC"` 原样去查
恒为「缺」（实测，第一版就被这个坑到 `cjk_fonts_actually_load_from_assets_dir` 红）。查询侧统一 `to_lowercase()`。

素材两条路径分开：`/uploads/**` 映射 `config.upload_dir`，经 `VirtualPath::realize` 逐段校验拒绝任何能逃出
root 的写法（Windows 的 `\` 与盘符也算）—— 文件名来自库里存的 URL，不能当可信路径直接 join。外链图片由调用方
抓成字节后按 `/ext/<n>.<ext>` **序号**注入，不用 URL 哈希名：typst 的 `FileId` 走全局 interner，上限 65535 且
**永不回收**，哈希名会让它随请求单调增长。`today()` 一律 `None`（源码不写 `datetime`，卷面日期由 Rust 侧
格式化后注入）。`Compiled<T>` 手写 `Debug`：derive 出来的一行日志就是几十万字节的 PDF 原文。

诊断口径：typst 遇错即中止求值，一次编译通常只有一条诊断，`CompileError::summary()` 仍把总数带上（多条是我
们自己追加的情况）。`flatten_one` 只取消息 + 首条提示，**不带行列** —— 源码是生成的，行号对教师无意义。
「一枚坏公式不失败整卷」在这层的落点是：预防靠 T3.4 的守卫（编译后补救来不及），编译层的 `diagnostics`
只用于记 Issue 与日志。

测试：compiler.rs 10 例（1 枚探针）。hello-world 出 `%PDF` 开头且 >800 字节的非空产物；中文 + 注入图片 +
mitex 公式混合源编译成功且无字体告警；75 条语料整批编译（失败时逐条定位并打印 `summary()`）；诊断可枚举；
字体池的注册/去重/复用/`fonts` feature 四枚断言；SVG 逐页。**整个 typeset 测试集 63 例 1.03s 跑完**，
typst 编译的耗时余量比 M3 预估更宽松。全量 `cargo test --lib --offline` = 608 passed / 0 failed / 9 ignored。

顺手修一处被新依赖暴露的 M2 断裂：`export/math/mod.rs` 里 `hex.strip_prefix(['x','X'].as_ref())` 现在推不出
类型（typst 依赖闭包带进来的 `palette` 给 `[T; N]` 补了多个 `AsRef` 实现 → E0283），改成
`strip_prefix("x").or_else(|| strip_prefix("X"))`。语义等价，但这类「加依赖把老代码编坏了」的账值得记下来。

### 六、T3.6 `typst_gen`：把 spec 翻译成一份编得过的源码

`src/typeset/typst_gen.rs`。公开面只有 `generate(doc, images) -> Generated { source, issues }`：`LayoutDoc`
+ `LayoutSpec` 进，一段完整 typst markup 出，编译归 T3.5。这一层的契约是**生成的源码必须编得过**，所以它也是
所有实测坑的收容所：

- **多栏走页级 `columns`，不走 `#columns(2)[…]` 包壳**。后者是布局容器，容器里 `#pagebreak` 直接报
  "pagebreaks are not allowed inside of containers"（实测）—— 卷末答案就没法另起一页，T4 的脚注与行号会同源失效。
  typst 在 `pages/run.rs` 里同时读 `PageElem::columns` 与 `ColumnsElem::gutter`，所以栏距单独写一条
  `#set columns(gutter:)` 就够，与栏数同处一页级口径。
- **页码只能在 `context` 里求值**：裸 `counter(page).display()` 报 "can only be used when context is known"（实测）。
- **数学字体在 typst 0.15 里没有旋钮**：`math` 是模块不是元素，`#set math(font:)` 报 "expected function, found
  module"，而 `EquationElem` 自带的 show_set 又把方程字体硬设成 New Computer Modern Math。`spec.fonts.math`
  于是和 docx 侧一样没有落点（OMML 的字体由 Word 自己挑）—— 字段按 §6.1 定齐、注释写明现状、`layout.ts` 的文档
  注释跟着重生成。等真能落地再生效，比悄悄收着一个不起作用的字段诚实。
- **图片宽度在 Rust 侧算**：typst 的 `min()` 不许 ratio 与 length 比较（实测），所以 `px → mm`
  （`PX_MM = 25.4 / 96.0`）后 `clamp(1.0, spec.column_width_mm())`，写进源码的是绝对值。
- 素材表是**三态**的：`Some(path)` 渲染 / `None` 静默跳过 / 缺键才记一条通用 Issue。形状照抄 docx 的
  `HashMap<String, Option<ImagePart>>`。不去 string-match 上游的警告文本再决定报不报 —— 那样一张坏图会同时刷出
  两条 `Image` 警告，而 `X-Export-Warnings` 头顶着 B3 的 8KB 截断线。
- `MITEX_PREAMBLE` 原样输出一次即可：T3.4 的「我们定义了哪些名字」就是从这段文本扫 `#let` 来的，两边天然同源。

**验收口径**：PDF 字节里存的是矢量轮廓，**没有可读明文**，所以「中文有没有豆腐块」只能靠 frame-tree oracle
`compiler::rendered_runs`。`simulated_paper_has_no_tofu_and_keeps_its_text` 用 20 题仿真卷（四大题型 + 单栏/双栏
/A3 三套 spec 各跑真编译）断言：页数 >1、必现文案都在 `glue(runs)` 里、并且**每一个 CJK run 的族名都以
`Source Han` 开头**。13 例（3 例真编译），`#image` / `#callout-box` / 选项栅格这些结构断言负责在没有文本 oracle
的情况下盯住模板本身。

### 七、T3.7 `/export/pdf`：PDF 出口唯一化

`src/export/pdf.rs` 从 T3.7 起兼任**渲染出口**（`generate_pdf(doc, upload_dir)`），入口只有
`handlers/export.rs::export_pdf`。**R1：不建 `/typeset/render`** —— 路由表里只加 `/export/pdf` 与
`/typeset/profiles`（后者只返回四枚预设，给 T3.8 的表单用），并留一枚 404 断言
`there_is_no_typeset_render_route` 把这个决定钉住，免得 M4 有人手滑加一条平行通道。

预取侧两条新事实：

- **typst 按路径扩展名分派图片解码器**。注入名 `/ext/<n>.<ext>` 里的 `ext` 一律用嗅探出的真实格式（`infer`），
  不用 URL 里那一段 —— 一张 `.jpg` 结尾的 PNG 若按原名注入，选错解码器就是**整卷编译失败**。
- 新增 `RENDERABLE` 白名单（png/jpg/jpeg/gif/webp/svg）：`tif/bmp/ico/heic` 都能被 `infer` 认出来，但 typst 0.15
  解不动。缺这道门等于把「一张怪图」升级成「一次 500」，白名单外只丢图 + 记警告。

失败口径只有一种：`compile_pdf` 返回 `Err` → 500 `ERR_TYPESET_COMPILE_FAILED`。公式降级、图片丢弃都只进
`X-Export-Warnings`，与 docx / markdown 共用 `file_response`（顺手把 `content_type` 提成 `&'static str` 参数）。

§13.4 的「缺字体回退 + 记警告」到这才算真落地：新增 `compiler::missing_cjk_fonts(dirs)`（走记忆化池），每次编译
后查一遍，缺哪几族就写进警告。以前 `missing_cjk_families` 只被测试用着，线上缺字体是静默的豆腐块。

**两个出口对「坏公式」的判定不一致**，集成测试的降级用例只能各用各的输入：docx 侧那枚 `$\frac{1}{$`
（latex2mathml 报错）在 PDF 侧完全不成问题 —— mitex 对不配平括号宽容（T3.4 已记），`to_typst` 返回 Ok、typst 也
编得过。PDF 侧的降级锚点是 `\argmax_x f`：mitex 转得动、typst 不认，被 `unresolved_name` 拦下来。

测试：pdf.rs T3.7 新增 5 例（序号注入与按 URL 去重、读不到的图带上真实原因、非白名单格式被拦在包外、
`collect_images` 覆盖到每一种可带图的块、真编译出嵌图的 PDF），`tests/export_pdf_api.rs` 7 例（401、RFC 5987
文件名、降级不中断整卷、profiles 预设 round-trip 成 `spec` 后仍能出 PDF、隐藏题存活、四枚预设清单、
`/typeset/render` 不存在）。全量 `cargo test --lib --offline` = **627 passed / 0 failed / 9 ignored**，
新增文件 clippy 零告警。

### 八、踩坑记录：`cargo fmt -- <file>` 在这个仓库会把全仓库格式化了

想只格式化新增的两个文件，跑的是：

```bash
cargo fmt -- src/export/pdf.rs src/typeset/ir.rs src/typeset/mod.rs
```

结果 `git diff` 变成 **91 个文件 / +3894 / −2531**。`cargo fmt -- <args>` 是把 args 透传给 rustfmt 而不是
限定范围，而本仓库**有意不是 rustfmt-clean 的**（已提交排版里有大量手工折行，`markdown.rs` 与
`handlers/export.rs` 各有若干处 rustfmt 差异是既成口径），所以 rustfmt 一上来就把全仓库按默认风格重排了。

先确认噪声是纯 reflow（抽查 `git diff src/config.rs` —— 只有换行没有语义变化），再
`git checkout -- $(git diff --name-only | grep -v -E '^(src/export/mod\.rs|src/typeset/mod\.rs|src/typeset/blocks/choice_grid\.rs)$')`
收回来，只留 3 处有意改动 + 2 个未跟踪新文件。**这个仓库里格式化单个文件请用**
`rustfmt --edition 2024 <files>`（`--check` 用于验证），不要碰 `cargo fmt`。

**同一条坑在 T3.7 复发了一次，而且换了个马甲**：这次用的是上面那条「正确口径」——

```bash
rustfmt --edition 2024 src/typeset/typst_gen.rs ... src/lib.rs tests/export_pdf_api.rs
```

`git status` 当场变成 **83 个文件**。rustfmt 处理一个文件时会**顺着 `mod` 声明递归**，`src/lib.rs`
一进参数就等于把整个 crate 重排（`src/ai/**`、`src/handlers/questions.rs`、`src/workers/**` 全中）。
所以那条口径得再收紧一句：**只给叶子文件跑 rustfmt，绝不把 `lib.rs` / `mod.rs` 这类聚合入口写进
参数**（`src/typeset/mod.rs` 与 `src/handlers/mod.rs` 同样是递归入口，任何一个进参数都等于整棵模块树
重排）；入口文件的改动一律手写、照周边风格排版（本次 `lib.rs` 最终 diff = 4 增 1 删）。反过来，
本来就 rustfmt-clean 的文件（如 `typeset/mod.rs`、`handlers/mod.rs`）在列表里不会留下噪声 —— 别把
「这次没事」当成「可以放进去」，没被重排只是因为它们本来就没得重排。

回收流程同上：先把 9 个有意改动的文件拷到 `$LOCALAPPDATA/Temp/fmtbak`，
`git diff --name-only | grep -v <保留清单> | xargs git checkout --`，再逐个 `git diff` 复核剩下文件里
**纯 reflow 的 hunk** 手工还原（本次 `handlers/export.rs` 三处：import 排序、`sanitize_filename` 的超宽
`filter` 闭包、一枚单行 `assert!`）。还原后重跑 `cargo test --lib` 与两个端点集成套件确认没改坏。

### 九、T3.8 导出面板的 PDF 版面区：四个下拉与一条只读密封线

`ExportDialog.vue` 里 PDF 从 M1 起的占位（灰、写着「M3 交付」）转成正常格式项，选到 PDF 才展开
「版面」区块：预设 / 纸张 / 栏数 / 留白样式四个 `AppSelect`，字段全部来自 ts-rs 生成的
`api/types/layout.ts`（前端不另手写一份 spec 形状）。取值口径照 T3.3 定下的**整体替换**：
选预设 = 深拷贝整份 `spec` 覆盖当前值（`JSON.parse(JSON.stringify())` 断开引用，否则微调会写花
预设本体），之后四个下拉只改自己那一个字段。

三处值得记的决定：

- **预设清单懒加载 + 可失败**：第一次点 PDF 才 `GET /typeset/profiles`；拉不到只 toast，
  `layout` 留 null → 请求体不带 `spec` → 后端按 mode 取默认预设。版面下拉空着不该拦住一次导出。
- **密封线只做展示，不做开关**：`grep binding src/typeset/typst_gen.rs` 是空的 —— 排版器今天
  根本不读 `spec.binding`。放一个能点但什么都不发生的开关，比没有更糟，所以渲染成一行只读说明
  「居中折叠（M4 起排版）/ 不装订」，值由 `layout.binding` 推出来，M4 接上后这行自动变准。
- **`spec` 只在 PDF 分支带上**：`buildRequest()` 里 `spec: isPdf ? layout : undefined`。
  markdown / docx 看见 `spec` 会一脸茫然，把它们一起发出去等于给另两个出口埋无谓的分支。

**「spec 到底生效没有」第一次有了机器判据**：PDF 里的中文是矢量轮廓（搜关键词恒为 false），
但 typst 把页面字典**明文**写进 PDF —— 第一个 `/MediaBox [0 0 W H]` 直接读得到。于是
`tests/export_pdf_api.rs` 加了一枚 `media_box(bytes)` helper，同一份卷导两次：不带 `spec` 时
A4 纸宽 595.28pt，回传 `a3_tri_exam` 后是 1190.55pt（高度与 A4 相同）。纸宽这一个数就足够钉住
「前端微调回传」这条链路，不必肉眼开文件。

浏览器验收（`npm --prefix frontend run dev` + `cargo run --bin mathset`，探针账号 3 题）四次导出
全部 200 + `%PDF-1.7`：teacher 默认 → 面板显示「A4 讲义 · 单栏」、请求体 `spec={paper:"a4",
columns:1}`、MediaBox 595.28；预设换 A3 三栏 → 纸张与栏数两个下拉**同步跟着变**、MediaBox 1190.55；
再把栏数手动调回双栏、留白样式调成纯空白 → 两个字段都进了请求体，体积 36.4KB→39.7KB（6cm 留白
真被画出来了）。换预设时留白样式被重置回横线，正是整体替换该有的样子。

## 2026-09-02 导出引擎 M4 批次①（题型注册表与防跨页）

### 一、T4.1 `typeset/blocks`：把出块逻辑从桥上搬进排版侧

`export/pdf.rs` 里那套「题干 → 小问 → 留白」的出块代码搬到 `src/typeset/blocks/mod.rs`，拆成
`BlockBuilder`（一个题型模板）+ `Registry`（注册表）+ 五个 builder（choice / multiple / fill /
solution / composite）。搬家的理由是分页策略：哪块能跨页、哪块要粘住下一块，是**题型模板**该说的
话，不是适配器该猜的话 —— T4.5 之前适配器只能靠 `q.kind` 现编 if-else，新题型进来就得改桥。

方向不变式仍然是硬的：`export → typeset` 单向。切分行内内容是 `export` 的能力（`split_content`
认 markdown 与公式），`blocks` 不许 `use` 它，于是它以参数的形态注入：

```rust
pub type Splitter = dyn Fn(&str) -> Vec<InlineNode>;
fn build(&self, q: &ExamQuestion, ctx: &BlockCtx, split: &Splitter) -> Vec<LayoutBlock>;
```

三条设计值得记：

- **`builder(kind)` 用 `.rev()` 扫描**，所以后注册者覆盖先注册者。这是「仅需注册即可接管一个题型」
  的机制本体，不是顺手写的。
- **未命中的 kind 落到 `FALLBACK`（`WRITTEN` 策略）**。兜底不是摆设：注册表为空 = 「新题型刚进枚举、
  还没注册」，此时仍要出题干 + 小问 + 留白，而不是静默出一张空白卷。用例就照这个口径写。
- **`Policy` 三轴**：`expands_parts`（要不要展开问树）/ `wants_blank`（给不给作答区）/
  `compact_stem`（题干能不能整块不跨页）。trait 的默认实现就是这三轴的乘积，五个内置 builder
  **全都只声明 `kinds()` 与 `policy()`**，一处 `build` 也没覆写 —— 题型差异能被这三轴表达完，
  说明抽象选对了。表达不完的仍然可以覆写 `build`，`blocks/mod.rs` 里那枚假想 `ProveBuilder`
  （题干整块不跨页 + 一块大留白、不排小问）走的就是这条路。填空题不给整块留白（B2：作答位在行内
  下划线上）由 `wants_blank=false` 表达，不再是一枚特判。

扩展性验收做在**两层**：`blocks/mod.rs` 里假想 `ProveBuilder` 接管 `Solution` 的注册表单测，加上
`export/pdf.rs` 里假想 `FillBlankBuilder` 接管 `Fill` 后 `#blank-lines(` 出现在 typst 源码里的端到端
单测。只测注册表会漏掉「IR 出了块但排版器不认」这一类断链 —— 而 T4.1 承诺的恰恰是「一行不改地接到
版面上」。

### 二、⛔ typst 0.15 里没有 keep-with-next 这个原语

任务分解写的是「小问标题 `keep with next`」，先去 typst 源码找对应物，结果是没有：

```
grep -rn "keep.with.next\|KeepWithNext\|keep_lines_together" \
  typst-library-0.15.1/src typst-layout-0.15.1/src   # 空
```

`par` 也没有 `keep-lines-together`。所以 0.15 上做粘连只有一条路：**把一串块折进一枚
`block(breakable: false)` 壳**。落法即 `FUNCTION_LIBRARY` 里的 `#keep-together(body)` +
Rust 侧 `plan_groups` 决定哪些块进同一个壳。docx 侧走的是真 `w:keepNext`（⛔R5 已实测它对表格
有效），两边共用同一份 IR 语义，各自的实现细节留在自己那侧。

### 三、为什么必须自己算高度：超过一页的 `breakable: false` 会溢出而不是自动断开

`typst-layout-0.15.1/src/flow/distribute.rs::single()`（整块不可断的分支）：先在当前区里量，

```rust
if !self.regions.size.y.fits(frame.height()) && self.regions.may_progress() {
    return Err(Stop::Finish(false));   // 换一页再来
}
```

而 `regions.rs::may_progress()` = `!backlog.is_empty() || last.is_some_and(|h| self.size.y != h)`。
换页后重试时尺寸已变，`may_progress()` 转 false → **照排**。净效果是一枚比版心高的整块被塞进新页，
溢出部分裁掉，不是「退回可断」。所以预算只能在 Rust 侧算：

- `budget_mm` = 版心高 × 0.75（A4 学生版 `(297 − 22 − 22) × 0.75 = 189.75mm`）。留的 1/4 是给估高
  误差和首页大卷头的余量。
- 估得准才算数：`Blank` 用它的 `height_mm`（确定值），题干/小问按「(文字宽 + 悬挂缩进) ÷ 栏宽 = 行数
  × 8mm + 2mm」估，选择题再按**最宽那条选项** × 行数加成（宁可估高少粘一块，不许估低做出超页整块）。
- 估不准的一律不粘：图片（只知道像素宽，等比缩放后的毫米高无从得知）、表格、显式换行、块级公式、
  `Callout`、内嵌 `Answer`。判据复用 `choice_grid::requires_single_column`，不另写一套。
- 链**从尾往回吞**，不是从头往后吞：最要命的孤立场景就在链尾 ——「最后一个小问的标号留在页脚，一整块
  作答区跑到下一页去」。
- 终结块估不准时**退一格**：丢的只是「与终结块那一环」的粘连，链内题面块照旧粘住。

### 四、自己刚写的用例抓到一个真 bug：退让路径会把块整个丢掉

`plan_groups` 第一版收尾写的是 `i = term + 1`。可退让时壳只覆盖到 `end`（`end < term`），`end+1 ..= term`
这些没进壳的块就被游标跨过去了 —— 用例表现为 `planned()` 返回 `[0..2]`，而序列有三块，第三块（那枚
240mm 留白）**整个从版面上消失**。改成 `i = range.end`（没粘成才跳回链尾）。

顺手把这层不变式做成所有 T4.5 用例的公共前置：`planned()` 包一层断言「分段不重不漏覆盖整条序列」+
「壳里不许出现模板没要求的粘连」（非末位块必须本来就 `keep_with_next`）。这类通用不变式写成 helper
比在每个用例里重复期望值更耐改。

### 五、粘连只在一题之内：收尾规则住在适配器，不住在模板

模板给题干块（乃至最后一个小问）置 `keep_with_next` 是「粘住它**自己的**小问 / 留白」的意思，这个位
一路传到序列末尾就会把下一道短题焊进同一枚壳 —— 恰好踩进第三节那条溢出路径。于是桥上加一条：

```rust
if let Some(last) = out.last_mut() { last.meta_mut().keep_with_next = false; }
```

`LayoutBlock::meta_mut()` 是为此加的可写访问器（`meta()` 一直是只读的）。

代价要说清楚：`single_choice_becomes_one_glued_question_block` 从 T3.3 起断言
`assert!(q.meta.keep_with_next)`，现在**必须反过来**。单选题只出一块，那块就是链尾，它的 keep 没有
意义 —— 粘不动也不该粘。断言改成 `!keep_with_next`，并补一枚 `keep_chain_is_closed_inside_its_own_question`：
两解答题带小问与卷面留白，keep 序列应是 `[true, true, false, true, false]`。

### 六、帧树回读第一次有了「页」这一维，边界卷差点成为空断言

T3.5 的 `rendered_runs` 是全卷一根明文；防跨页的断言口径是「这两段字在**同一页**」，所以需要逐页。
新增 `rendered_pages(doc) -> Vec<Vec<RenderedRun>>`，`rendered_runs` 退化成它的 flatten。一个 `TextItem`
不会跨页 —— typst 只在行间断页，所以按页切分不会把一段字劈成两半记两次。

两个坑：

- **单行题干永远不会被页界劈开**，于是「对照组确实有腰斩」这条非空断言一开始就是失败的（30 题全落
  页内）。改 `stem_text(i)` 让题干 1~4 句随题号变化，边界卷才真造出来。用例留了守卫：
  `assert!(straddling(&loose, n) > 0, "边界卷没造出来…本用例就成了空断言")` —— 没有这条，
  「glued 后 0 题腰斩」可以只是因为压根没题腰斩。
- **页脚逐页画一遍**：`第 N 页 / 共 M 页` 的明文会插在跨页题干的中间，把「粘连只改分页、不改内容」
  那根 `flat(glued) == flat(loose)` 断言带偏（第一次失败就是这个原因）。该 fixture 关掉
  `spec.header_footer.page_number`，页码不归 T4.5 管。

最后落在 6 枚用例上：短链折成一枚壳（含留白）、跨题不许粘、估不准的终结块只丢最后一环、超预算留白
留在壳外、长题干腰斩但小问与它的作答区同页、带图的题整题不粘；再加一枚编译级边界卷 ——
`loose` 30 题里有题腰斩、`glued` 0 题、两边正文明文逐字相等。

### 七、`gen` 是 edition 2024 的保留关键字

`let gen = generate(&doc, …)` 直接编不过：`expected identifier, found reserved keyword 'gen'`。
改名 `rendered`。这个 crate 里想给「生成结果」起短名的下次注意。

### 八、本批次已知边界

- 选项栅格的可用宽仍按 `HANGING_EM = 2.0` 决策（`export::pdf` 与 typst_gen 用例的 `available_em`），
  而 typst 实际画的是 `HANG_EM = 2.6em` 缩进 —— T2.5 遗留的口径差，M4 后续批次统一，本批次不动它以免
  把栅格决策的用例全洗一遍。
- `rendered_pages` 的粒度是**物理页**：A3 折叠/三栏下「一栏」不是一页。T4.5 只需判断同页，够用；
  T4.7 要断言逻辑页与栏序时，得从帧树里把 `pos` 一起读出来。

## 2026-09-02 导出引擎 M4 批次②（选项栅格与左文右图）

### 一、T4.2：列数只有 Rust 一个来源，typst 那侧只许往下降

`QuestionBlock.grid` 自 T3.3 就躺在 IR 里，排版器却始终把选项竖排一列 —— 字段是死的。T4.2 做的
就是把 `#choices(列数, [A. …], [B. …], …)` 接上，列数直接取 `q.grid.columns`，不再在渲染侧重新判
一遍。与 docx 的同源在于**判定函数只有一个**（`choice_grid::decide`，`export/docx/writer.rs:468`
与 `blocks/mod.rs::question_block` 各调一次），可用宽则各侧自己传：docx 传它的 `GRID_EM`，typst
侧传 `ctx.available_em`。同一份估宽算法 + 各自的栏宽，不会再出现「Word 里两列、PDF 里一列」这种
各排各的。

Rust 判的是**先验**（估宽），只有 typst 能拿到渲染后的真实宽度，所以 `cols > 1` 时包一层
`layout(size => …)` + `measure(cell).width`，装不下就 4 → 2 → 1 地降。跳过 3 是故意的：四枚选项
排成 3+1 比排成 2+2 难看，而「降一档」的代价本来就只是多占一行。

`size.width` 取的是**当前实际栏宽**而不是 spec 里的常数，这一点让 T4.3 白捡了一个便宜：题干浮动
成左文右图之后，选项的可用宽自动变小，兜底会把它降下来 —— 不需要为「图 + 栅格」的组合另写判定。

### 二、⛔ `..rest` 不是数组，以及 typst 的位置参数两条规矩

写模板时踩到三处，全都是「看起来该能用、实际报错」型：

- `#let choices(cols, ..cells)` 里的 `cells` 是 **arguments**，直接 `for cell in cells` 报
  `cannot loop over arguments`。要 `cells.pos()` 取位置参数来循环，而下面 `grid(..cells)` 的展开
  传参照旧可用 —— 同一个变量两种用法，只有一种能循环。
- 位置参数**不能具名**传：`#figure-float(label, figure: […])` 报
  `the argument 'figure' is positional`。
- 更阴的是行尾的 `[content]`：它填的是**下一个未填的位置参数**。所以模板里
  `item(label: lbl, …)[#body]` 会把 `#body` 塞进 `label`，然后报 `missing argument: body`。
  规矩是「凡后面要跟 `[content]` 的调用，前面的位置参数一律别写成具名」。

顺带记一笔 typst 没有整数除法（`..` 在 Rust 里是区间、在 typst 里根本没有），栅格算每列宽是
`(size.width - (c - 1) * gut) / c` 的浮点式。

### 三、验收不靠眼睛：把「几列几行」变成帧树坐标的聚类

任务分解的验收口径是「短选项 1×4、中 2×2、长 4×1」，肉眼比 PDF 不可复用。批次①把落点坐标读出来
之后，这条变成了三段断言：`option_positions()` 把 A/B/C/D 四枚标签文字的 `(x_mm, y_mm)` 取出来，
`lanes()` 按容差把 x 聚成「道」，`measured_grid()` 返回 `(行, 列)`：

```rust
assert_eq!(measured_grid(&option_positions(&choice_doc(4, [SHORT; 4]))), (1, 4));
```

2×2 那条额外断言了**行优先**填充（A B 一行、C D 一行）与行的上下次序 —— typst grid 默认就是行
优先，但「教师按 A B / C D 念答案」是卷面语义，不该靠默认行为不写断言。兜底另有两条：手工把列数
写成 4 而选项实际很宽 ⇒ 版面必须是 2 列（再狠一点是 1 列）；降列前后卷面明文逐字相等
（`dropping_columns_never_loses_option_text`）—— 兜底只改版面，不许改内容。

### 四、四处漏包 `#(…)` 的智能引号：这一条改的是真实 PDF 输出

`math::typst_str()` 返回的是**带引号的字符串字面量**（`"A. "`）。把它裸嵌进 markup 里
（`["A. "]`），typst 的 smartquote 会把那个 `"` 当成引号开始去配对，于是卷面上画出来的是
`“A. `。HEAD 上这种写法有四处同罪：选项前缀 `[{}{}]`、表格单元格 `[{lit}]`、图注 `[{}]`、
作答标签 `push_str(&typst_str(..))`。全部补成 `#("…")` 即可 —— 表达式模式里的字符串字面量不走
markup 改写。

要紧的是：**这不是源码层的洁癖，而是真实 PDF 上的错字** —— HEAD 上这四类文本画出来前面都多一枚
`“`（实测：选项标签上图成 `“A. `）。源码断言看不见它，是批次①建起来的帧树回读第一次把明文照出来
才被发现。所以这次修复之后重出的 PDF 与 M3 的旧样卷会在选项、表格、图注这几处不同 —— 那是修 bug，
不是排版回归。规则也钉成了断言：编译一页试卷后扫全页帧树，出现 `“` 或 `”` 即红
（`markup_triggers_stay_literal_text`）。

### 五、T4.3 左文右图：`35%` 轨道为什么恒不失宽

模板只有一句关键代码：

```typst
#grid(columns: (1fr, 35%), gutter: 6pt,
  item(label, indent: indent)[#body],   // 位置参数：见上节第二条
  align(right + top)[#figure])
```

「图列宽度恒定不失宽」不是靠估宽估得准，而是 typst 的轨道算法保证的：
`typst-layout-0.15.1/src/grid/layouter.rs::measure_columns` 先把所有**相对**轨道按父容器宽折算
（`Sizing::Rel` 走 `relative_to(regions.base().x)`），剩下的余额才由 `grow_fractional_columns`
分给 `fr`。所以 `35%` 恒等于栏宽 × 35%，跟左栏文字长短毫无关系。

那个 `35%` 与 Rust 侧的 `FIGURE_SHARE = 0.35` 是双写，防漂的针是**算出来的**而不是硬编码：

```rust
assert!(s.contains(&format!("columns: (1fr, {}%)", FIGURE_SHARE * 100.0)));
```

悬挂缩进只出现在左格（`item` 自己就是一个块），图格在它的 `inset` 之外、享整栏宽 —— 于是判定吃的
宽度是 `spec.column_width_mm()`，与选项栅格那套 `available_em`（扣了一个 indent）差一档。
`blocks/mod.rs` 里那条接线用例就是拿这个口径差当证据的：`LayoutSpec::default()` 单栏 174mm ⇒
图列 60.9mm，200px ≈ 52.9mm 放行；换成 `available_em`（30em ≈ 111mm）只得 38.9mm，同一张图会被
判不浮动。**「这一张浮起了」本身就是口径正确的证据。**

### 六、浮动判定只放行「作者声明过宽度」的尾部单图

`figure_float::plan` 是纯函数、零 typst 依赖，五条放行条件各对应一种惊喜：只浮动尾部那一枚（图后
还有文字时那段会被挤进 65% 的左栏另起一段，读起来像漏了一段）；必须单张（图组在 30mm 右栏里必然
折行）；必须显式 `width`（没有 px 就无从估宽，而 typst 里的图片不会自己缩小，装不下就是栏外溢出）；
`align` 必须没写过（`{align: center}` 的本意是「栏内居中」，浮动后居中对象换成了那 30mm 的右栏，
作者意图被悄悄改写）；左栏不许有表格与块级公式（两者都按整栏宽设计，压到 65% 会在自己内部溢出）。
估宽还留一成余量。

「必须显式 width」这条值得单独记：只有作者写了 `![alt](url){width:300}` 时 `InlineNode::Image`
才带宽度（`export/content.rs` 的属性解析），所以后台题库里绝大多数配图**永远不会浮动**，照旧独占
整行。这是设计而非缺陷 —— 不浮动不是降级，是这道题不适合。

`Split` 存的是两个下标而不是节点副本，题干一字不改。这让「浮动只是版面行为」有了一条廉价到不该省
的断言：同一份 doc 浮动与不浮动，编译后卷面明文逐字相等。

### 七、栅格几何只能从帧树读，而 `FrameItem::Image` 只认栅格图

`compiler.rs` 的帧树遍历升级成一次走查同时收文字与图片（`placed_images()` 给出
`x/y/w/h` 毫米），于是「图不失宽」变成了可比对的数：同一份文档只差左栏文字长度（短题干对 12 倍
长干题干），两次编译出来的图片 `w_mm` 与 `x_mm` 必须在 0.2mm 内相同，且额外断言浮动那份的左栏**
占的行更多** —— 否则「宽度相等」有可能是两份压根都没浮动造成的空成功。

一个坑：`FrameItem::Image` **只在栅格图时存在**，SVG 进 typst 会展开成矢量 `Group`，帧树里根本没有
`Image` 节点。所以这条用例得喂一张真 PNG —— 测试里现合成一枚 1×1 的 `dot_png()`，不依赖仓库里的
任何资产。

### 八、R7 兜底的成本，以及它为什么不必和粘连打架

`#[ignore]` 探针 `cost_of_the_measure_fallback`（20 题模拟卷，a4_practice，先编译一次付掉进程级
字体池，再取三次最好成绩，对照组把所有题判成单列 ⇒ 一个单元格都不量）：

```
兜底开：116ms / 3 页；全单列（不量）：122ms / 3 页；差 -6ms
```

差值在噪声以内（甚至为负），预算是 500ms/卷 —— 兜底不是性能问题，不需要为它设计开关。

原先担心 T4.3 的浮动图与 T4.5 的 `keep-together` 壳会合流出「壳里套一枚不许跨页的图」这种溢出裁切，
查下来不成立：`block_height_mm` 对含 `Image` / `ImgRow` 的题干恒返回 `None`，不可估高的块本来就进
不了粘连链，浮动题因此**自动**落在壳外。没有为此特判一行代码。

### 九、本批次已知边界

- 批次①记的 `HANGING_EM = 2.0`（决策侧）vs `HANG_EM = 2.6em`（typst 实际画的）口径差**仍然没关**，
  本批次靠 R7 兜住它而不是关掉它 —— 关掉要把 `choice_grid` 全部用例的期望列数洗一遍，收益只是
  少一次运行时降列。
- 只浮动尾部一枚：图组、显式对齐的图、未声明宽度的图一律通栏。真需要「多张竖排配图并排」时，
  扩展点在 `figure_float::plan` 的放行条件，不在模板。
- docx 侧不读 `figure` 字段（`figure_float` 只被排版侧引用），这类题在 Word 里仍是通栏图。两个
  出口版面可以不同，内容仍一字不差 —— 与 M2 定下的「docx 不追求像素级一致」同口径。
- 图列宽只在**两种栏宽**上断过言：`a4_practice`（86mm 双栏 ⇒ 图列 30.1mm，渲染侧用例）与
  `LayoutSpec::default()`（单栏 174mm ⇒ 图列 60.9mm，接线侧用例）。`a4_lecture` / `a3_fold_exam` /
  `a3_tri_exam` 三档预设的图列宽没有各自的断言，公式 `栏宽 × FIGURE_SHARE` 是共用的，真出问题会是
  「某个预设下浮动判定与模板轨道不吻合」，T4.9 的四预设版面矩阵一并补。
- 浮动题干**跨页**时左右两格各自怎么断没有专门用例 —— 现有覆盖都是「单页内装得下」的情形。Rust 侧
  只把 `q.meta.breakable` 透传给外壳，图格本身没有独立的不许断声明。

## 2026-09-02 导出引擎 M4 批次③（答题留白与三模式）

### 一、留白在真实卷子上一次都没出现过：三档优先级缺了第三档

`blocks/mod.rs::blank_block` 在 HEAD 上只有两档：

```rust
let space = q.answer_space.or(ctx.options.answer_space)?;   // None = 不留白
```

`?` 就是那个洞。wire 侧 `AnswerSpace` 是**整块 `Option`**（`#[ts(optional)]`），前端只在导出面板里
写 `spec.answer_blank.style`，从不填 `options.answer_space` —— 于是「学生卷 = 题干 + 选项 + 留白」
这条 T4.6 的默认口径在版面上根本立不起来，面板里那个留白下拉是个死控件。抽出来的
`blocks/blank.rs::plan` 补上第三档，优先级在这一处定死：

1. 逐题 `q.answer_space`（试题篮里单独设过）
2. 全卷 `options.answer_space`（请求级开关，B5 说的「开关在 options 手里」）
3. 版面 `spec.answer_blank`（样式 + 默认高度）

**这是一次行为改动，不是重构**：改完之后「谁都没表态」的学生卷会开始长出留白。定它是对的，因为
§6.2 与 T4.6 的 DoD 都写着学生模式 = 题干 + 选项 + 留白，而旧行为让这条永远不成立。仍然保留唯一
的「明确关掉」出口：`spec.answer_blank.height_cm <= 0`。样式冲突按 B5 以 options 为准，卷级冲突由
`pdf.rs::blank_conflicts` 记**一条** info（`question_no: None`），不逐题刷屏。

### 二、教师（讲义）模式一律不留白 —— §6.2 那句话在排版侧只需要「不出」

`plan` 的第一行就是 `ctx.profile == OutputProfile::Teacher → None`。讲义上的作答区不是给学生写的，
它在教师侧的正确形态是解析：四类 Callout 由 `assembler::derive_callouts` 按 `options.callouts`
挂到题块上，答案与全解全析按 `answer_at_end` 决定内嵌题末还是走卷末答案区。所以「教师版折叠为解析
Callout」在排版侧只是**不出留白** —— 再补一块等于把解析印两遍。

### 三、`place(dy:)` 而不是段落流：行距与点阵都得由 Rust 说了算

留白的行数与行距是 Rust 除出来的（`blank_rows`：按目标间距 round 行数，再用 `高度 / 行数` 精确铺满
块高，块底就不剩空档）。模板里每一行用 `place(top + left, dy: step * i, line(...))` 钉住，**不走
段落流**：走流的话行距就成了「字号行高 + par leading」，与 Rust 那个数无关，而块是固定高度 + `clip`
的 —— 多出来的一两条会被静默裁掉，画出的行数与行距都不再是卷面上说好的那一份。

点阵不是「很多小圆」，是一根虚线：`stroke: (cap: "round", dash: ("dot", gap))`，且 `gap` 与纵向
`step` 同值才是二维散点，否则就是一排排虚线。断言读的是帧树里的 `PlacedLine`（本批次新增
`placed_lines()`，只收 `Geometry::Line`）：行数、行距、点距（`dash[0] + dash[1]`）、点径、通栏宽
五项全等，纯空白则一根线都不许有 —— 但它的 `height_mm` 得真的占住地方，用「换留白高度 ⇒ 后一题
落点差 == 高度差」反证它不是假样式。

### 四、⚠ 块边界上没有 leading：整份 PDF 的题块都在压字

出三模式样卷时目视发现教师讲义上「第 1 题的答案」印在「第 2 题的题干」上。量下来：

| 情形 | 实测行距 |
| --- | --- |
| 同段两行（`par` 内） | 5.31mm（字框 7.65pt + leading 0.7em 7.35pt） |
| 相邻两个 `#item`（`above: 1pt`） | **3.07mm** |

原因不在批次③的代码里，是 T3.6 模板带出来的：**单行 `block` 的高度就是字框**，`par` 的 leading
只加在同一段相邻两行之间，永远不管块与块。而 typst 对相邻块的间距取的是 `max(前块 below, 后块
above)` 而不是两者之和（最小实验实测：`below: 0.7em` 配 `above: 1pt` 与配 `above: 0pt` 得到同一个
5.31mm）。CJK 字形占满 em 方框（10.5pt ≈ 3.7mm），3.07mm 的行距就是上下两行互相压进对方的字身。

修在模板，一处根参数：

```typst
#let item(label, indent: 2.6em, above: 3pt, lead: 0.7em, breakable: true, body) = block(
  above: above + lead, ...)
```

`lead` 用 em 而不是 pt，跟着正文字号走。四个「自己是流级块」的入口各自补同一口气：粘连壳
（`keep-together`，壳内首块的 `above` 在容器边界被吞掉，所以这份间距必须由壳来带）、`figure-float`
及其通栏的选项 `rest`、留白块（否则第一条横线贴着题干下沿）、以及选项栅格的**行** gutter
（`2pt → 0.7em`：栅格每一行也只有字框高，leading 同样管不到）。`figure-float` 里那枚左格是网格
单元、要与配图顶对齐，那里显式 `lead: 0pt`。

回归口径刻意不写绝对毫米数：同一份编译产物里「同段两行」就是免费的行距基准，块间距只许比它多
0～1.6mm（`stacked_blocks_keep_the_paragraph_pitch`，裸块与粘连壳两种形态各量一遍）。负控验过：
把 `lead` 改回 `0pt`，用例立刻红并打出 `3.05mm vs 5.29mm`。

顺带把 T4.5 估高里的 `PAR_SPACING_MM` 从 2.0 改成 2.6 —— 它原先注释成「`par.spacing: 0.55em`」，
而那个属性从来管不到块边界，现在块间下限就是一个 leading。

### 五、T4.6 的验收：差异要在纸上有账，不在 IR 里

任务分解写的是「三模式各出一份样卷，模式间内容差异符合 §四 options 开关语义」。样卷是 `#[ignore]`
的手工用例（`writes_one_sample_pdf_per_mode`，产出在 `<temp>/mathset-t46-samples`），CI 跑不到，
所以差异矩阵单独做成常驻用例 `the_three_modes_differ_exactly_as_the_switches_say`，读的是**编译
产物**而不是 IR：

```rust
struct Paper { text: String, rules: usize, blanks: usize, key: usize }
```

`text` = 全卷帧树明文拼接（公式是 mitex 画出的形状，别指望 `$a_1$` 那样的原文能搜到），`rules` =
通宽横线数（目前全卷唯一会画它们的就是留白，卷头注意事项框自带上下两条边，所以矩阵用例把注意事项
清空了），再加 IR 里的 `Blank` 块数与 `answer_key` 长度。三卷各断一组：

- 学生：`blanks=1, rules>0, key=0`，答案/解析/四类 Callout 标题一律**不许出现**；
- 讲义：`blanks=0, rules=0, key=0`，四色 Callout 标题与正文齐备，答案与解析内嵌题末，
  「参考答案与解析」这个卷末标题不许出现；
- 考卷：`blanks=1, key=2`，`rules` 与学生卷**逐条相等**（换成考卷模式不多画一根线），卷末汇总带
  全解全析与小问答案，Callout 标题不出现。

Callout 只在讲义那份喂进题目：`assembler::derive_callouts` 的门控那边已有单测，矩阵测的是模式开关
的净效果，不重复测派生本身。`callout()` 在模板里的实际名字是 `callout-box(bar, bg, title, body)`，
配色由 Rust 侧给（`callout_colors`）—— 印前纯黑时它只能拿到黑与白。

### 六、5–9s 不是首次导出耗时：冷启动的账要写清楚

`#[ignore]` 探针跑三模式样卷时打印的是每卷 8.6–10.2s，而 `cost_of_the_measure_fallback` 量到
的逐请求成本只有 131ms。差在哪：**同一进程里前两编各 5–9s**，把 measure 兜底整个关掉仍是 6.2s
⇒ 不是兜底，是进程级初始化（字体池解析 + 首次走通布局代码路径）。暖身后 190–350ms/卷。
这个数字关系 M5 的「20 题卷预览 ≤1s」预算怎么花，所以写进了两处 doc 注释，别让 131ms 被读成
首次导出耗时。

### 七、本批次已知边界

- 留白三档只作用于 **PDF 出口**：docx writer 不读 `answer_space`（Word 里作答区本来就该是空白页
  或稿纸，不是画出来的横线），markdown 只把字段编译进 `BlankStyle` 占位。两个出口的版面可以不同，
  内容仍一字不差 —— 与 M2 定下的口径一致。
- `above` 小于一个 leading 的题块（小问 1pt、题块 3pt）在 typst 的 max 语义下已被 leading 吃掉，
  只剩「相对次序」有意义。真要拉开题间距，加的量应该写在 `lead` 之上而不是替换它。
- 9.5pt 的 Callout / 解析行距由同一条 em 规则跟着字号走，没单独量过；`analysis` 与答案同段续排
  （软换行），所以卷面上是「1. A；C 先画出数轴再比较端点」这样一行，不是另起一行。
- 三模式样卷不进 CI：一次 24s，且要落盘 PDF。常驻的是上面那条读帧树的差异矩阵。
- 卷头仍是简化版（题名 + 副题 + 元信息 + 注意事项），学校/班级/姓名栏与密封线在 T4.9/T4.8。



