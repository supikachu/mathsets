# 开发日记

> 项目：协同题库系统 (mathset)
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
- 创建数据库 `mathset`
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
mathset/
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

### 下午待办

#### P0 — 后端补充 🔴
| 任务 | 预估 | 说明 |
|------|------|------|
| 题目统计 API | 1h | 后端加 `GET /api/v1/questions/stats` 返回各状态题数，替代前端多次请求 |
| `creator_name` 返回 | 0.5h | QuestionSummary/Detail 关联 users 表返回创建者显示名 |
| 用户 `me` API | 0.5h | `GET /api/v1/auth/me` 返回当前登录用户完整信息 |

#### P1 — 前端优化 🟡
| 任务 | 预估 | 说明 |
|------|------|------|
| 编辑页加载提示 | 0.5h | 编辑已有题目时显示加载状态 |
| 删除确认优化 | 0.5h | 题目详情页删除后返回列表，统一确认弹窗样式 |
| 404 页面 | 0.5h | 添加 NotFound 页面 |

#### P2 — 新功能 🟢
| 任务 | 预估 | 说明 |
|------|------|------|
| 试卷管理模块（后端） | 3h | papers + paper_questions 表迁移 + API |
| 试卷管理模块（前端） | 3h | 试卷列表/创建/详情页 |
| 用户管理页面 | 2h | 管理员后台查看/禁用用户 |

#### P3 — 技术债 🟢
| 任务 | 预估 | 说明 |
|------|------|------|
| 后端测试补全 | 2h | 教研组 API + 审核权限的测试已经覆盖 |
| 前端组件单元测试 | 2h | 可选 |
| Docker 部署配置 | 1h | docker-compose.yml（PostgreSQL + 后端 + 前端 nginx） |
