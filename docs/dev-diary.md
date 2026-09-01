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
- 验收用的 `export_ui_probe` 账号与其 4 道样题仍留在本地开发库 `mathset`，可随时删除
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
