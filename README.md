# MathSet 📐

> 面向教师的 **AI 驱动数学题库系统** —— 智能录入、协同编辑、公式完美渲染

[![Rust](https://img.shields.io/badge/Rust-1.85+-orange)](https://www.rust-lang.org/)
[![Vue](https://img.shields.io/badge/Vue-3.5+-42b883)](https://vuejs.org/)
[![TypeScript](https://img.shields.io/badge/TypeScript-5.7+-3178C6)](https://www.typescriptlang.org/)
[![PostgreSQL](https://img.shields.io/badge/PostgreSQL-17+-336791)](https://www.postgresql.org/)
[![License](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

---

## ✨ 项目亮点

- **🤖 AI 智能录入** — 拍照或粘贴题目文本，大模型自动识别题型、知识点、难度，秒级完成结构化标注
- **📐 公式完美渲染** — 基于 KaTeX 的 LaTeX 公式渲染引擎，支持空集符号、多行公式对齐等国内教材特化适配
- **👥 协同题库** — 个人 / 团队 / 公共三种空间模式，支持题目审核流转、版本快照、多人协作
- **🎨 Apple 级 UI** — 遵循 Apple HIG 设计规范，胶囊分段控件、微阴影层级、流畅动效，告别传统后台管理系统的干瘪感
- **🔐 安全可靠** — JWT 认证 + RBAC 权限控制，AI API Key AES-256-GCM 加密存储
- **🌓 深色模式** — 原生支持明暗主题切换，保护长时间用眼

## 🏗️ 技术栈

| 层级 | 技术 | 说明 |
|------|------|------|
| **后端** | Rust + Axum 0.8 | 高性能异步 Web 框架 |
| **数据库** | PostgreSQL 17 + SQLx | 类型安全的 SQL 访问，支持 LTREE 层级查询 |
| **前端** | Vue 3.5 + TypeScript 5.7 | Composition API + `<script setup>` |
| **样式** | Tailwind CSS 3.4 + Design Tokens | Apple 风格设计系统 |
| **状态** | Pinia 3 | 全局状态管理 |
| **路由** | Vue Router 4.5 | SPA 路由 |
| **公式** | KaTeX 0.16 | LaTeX 数学公式渲染 |
| **AI** | Deepseek / 通义千问 / OpenAI | 多提供商可切换，支持视觉模型 |

## 📁 项目结构

```
mathset/
├── src/                    # Rust 后端
│   ├── handlers/           # API 处理器（题目、空间、审核、AI 等）
│   ├── models/             # 数据模型与 DTO
│   ├── auth/               # JWT 认证与权限
│   └── config.rs           # 应用配置（环境变量）
├── frontend/               # Vue 前端
│   ├── src/
│   │   ├── views/          # 页面组件（QuestionList、QuestionEdit 等）
│   │   ├── components/     # 通用组件（AppLayout、AppIcon 等）
│   │   ├── composables/    # 组合式函数（useToast、useTheme 等）
│   │   ├── stores/         # Pinia 状态仓库
│   │   ├── api/            # API 客户端封装
│   │   └── styles/         # 设计 Token 与全局样式
├── migrations/             # 数据库迁移脚本
├── docs/                   # 项目文档
├── .env.example            # 环境变量模板
└── Cargo.toml              # Rust 依赖
```

## 🚀 快速开始

### 环境要求

- **Rust** ≥ 1.85
- **Node.js** ≥ 20
- **PostgreSQL** ≥ 17
- **pnpm**（推荐）或 npm

### 1. 克隆与安装

```bash
git clone https://github.com/supikachu/mathsets.git
cd mathsets

# 前端依赖
cd frontend
pnpm install
cd ..
```

### 2. 配置环境变量

```bash
cp .env.example .env
```

编辑 `.env` 文件，填入必要的配置：

```bash
# 数据库连接（必填）
DATABASE_URL=postgres://user:password@localhost:5432/mathset

# JWT 密钥（生产环境必须修改）
JWT_SECRET=your_secure_secret_key_here

# AI 提供商配置（至少配置一个）
DEEPSEEK_API_KEY=your_api_key_here
```

### 3. 数据库迁移

```bash
# 安装 sqlx-cli
cargo install sqlx-cli --no-default-features --features postgres

# 执行迁移
sqlx migrate run
```

### 4. 启动后端

```bash
cargo run
# 服务启动于 http://127.0.0.1:3000
```

### 5. 启动前端（开发模式）

```bash
cd frontend
pnpm dev
# 前端启动于 http://127.0.0.1:5173
# 自动代理 /api 请求到后端
```

### 6. 构建生产版本

```bash
# 前端构建
cd frontend
pnpm build

# 后端构建
cd ..
cargo build --release
```

## 📖 文档

| 文档 | 说明 |
|------|------|
| [文档中心](docs/README.md) | 所有文档的索引 |
| [设计系统规范](docs/design-system-rules.md) | Apple HIG 风格指南 |
| [开发日志](docs/dev-diary.md) | 迭代记录与技术决策 |
| [AI 解析接口](docs/api/ai_parse.md) | AI 智能录入 API |

## 🗺️ Roadmap

- [x] 基础 CRUD：题目创建、编辑、删除、列表
- [x] 多维筛选：题型、难度、知识点、来源、年份等
- [x] AI 智能录入：文本/图片 → 结构化题目
- [x] 协同空间：个人 / 团队 / 公共题库
- [x] 审核流程：提交 → 审核 → 通过/驳回
- [x] 版本快照：题目修改历史与回滚
- [x] 知识点树：章节 → 知识点 → 解题方法三级体系
- [ ] 🚧 试卷生成：智能组卷、难度配比
- [ ] 🚧 批量导入：Word/PDF 题库解析
- [ ] 🚧 数据大屏：班级正确率、知识点覆盖分析
- [ ] 🚧 移动端适配：响应式布局优化

## 🤝 贡献

欢迎贡献代码！请阅读 [CONTRIBUTING.md](CONTRIBUTING.md) 了解详细流程。

1. Fork 本仓库
2. 创建特性分支 (`git checkout -b feature/amazing-feature`)
3. 提交改动 (`git commit -m 'feat: add amazing feature'`)
4. 推送分支 (`git push origin feature/amazing-feature`)
5. 创建 Pull Request

## 📄 许可证

本项目基于 [MIT License](LICENSE) 开源。

---

<p align="center">
  Made with ❤️ for math teachers everywhere
</p>
