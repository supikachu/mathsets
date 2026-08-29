# MathSets 文档中心

> 面向教师的 AI 驱动数学题库系统

## 目录结构

```
docs/
├── README.md              ← 本文件：文档索引
├── dev-diary.md           ← 开发日志（含完整迭代记录）
├── design-system-rules.md ← 设计系统规范（Apple HIG 风格指南）
├── requirements.md        ← 产品需求文档
├── ui-ux-design.md        ← UI/UX 设计文档
├── user-guide.md          ← 用户使用指南
├── 智能打标签统一改造.md
├── 长PDF识别与打标改造计划.md
├── 全自动解析质量评估_需求分析.md
├── 全自动解析质量评估_评测规则.md
├── 全自动解析质量评估_裁判沟通词.md
├── 全自动解析质量评估_闭环可行性.md
├── 题目内容模型与解析溯源.md
├── 语义匹配与别名沉淀.md
├── 语义匹配第2阶段_向量召回开发计划.md
├── api/
│   └── ai_parse.md        ← AI 解析接口文档
└── 前后端接口与数据对齐诊断报告.md
```

## 快速导航

| 文档 | 说明 |
|------|------|
| [开发日志](dev-diary.md) | 记录每日开发进展、技术决策与遗留问题 |
| [设计系统规范](design-system-rules.md) | 前端 UI 设计规范：色彩、圆角、阴影、动效 |
| [需求文档](requirements.md) | 产品功能需求与业务规则（题目字段/答案示意已过期，以[题目内容模型](题目内容模型与解析溯源.md)为准） |
| [UI/UX 设计](ui-ux-design.md) | 界面设计稿与交互说明 |
| [用户指南](user-guide.md) | 终端用户操作手册 |
| [智能打标签统一改造](智能打标签统一改造.md) | 五维契约、入库时机与分阶段落地 |
| [长 PDF 识别与打标](长PDF识别与打标改造计划.md) | 整本 OCR 一次、按题号切块、异步打标 |
| [全自动解析质量评估](全自动解析质量评估_需求分析.md) | 同一 OCR 上对照全自动与站外暂存 JSON，分流改进 slice / 提示词 |
| [全自动解析评测规则](全自动解析质量评估_评测规则.md) | 规则分六个桶的语义说明，与 `scripts/bench_eval_quality.py` 对齐 |
| [全自动解析裁判沟通词](全自动解析质量评估_裁判沟通词.md) | 拿 paper.md / full.json / export.json 做保真评测时，给站外模型的可复制提示 |
| [全自动解析闭环可行性](全自动解析质量评估_闭环可行性.md) | 错误样本库、gold、LLM 归因、算法/Prompt 建议草案、100/500/1000 题评测集；含规则分 P0 |
| [题目内容模型与解析溯源](题目内容模型与解析溯源.md) | 解答题 `structure`、答案 `{kind,value}`、不在题目表加解析列；对照外部 report 草案 |
| [语义匹配与别名沉淀](语义匹配与别名沉淀.md) | 关键词→节点收敛、别名回流与向量召回原则 |
| [语义匹配第 2 阶段](语义匹配第2阶段_向量召回开发计划.md) | DashScope embedding + pgvector 召回并集 |

## 相关资源

- 🏠 [项目首页](https://github.com/supikachu/mathsets)
- 🐛 [问题反馈](https://github.com/supikachu/mathsets/issues)
- 💬 [讨论区](https://github.com/supikachu/mathsets/discussions)
