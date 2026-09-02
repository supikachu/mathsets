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
