# AI 智能录题 — 任务计划书（v1.1）

> 配套文档：《AI智能录题_需求分析.md》《AI智能录题_审计报告.md》
> 本文档定义实施步骤、里程碑、任务分解与验收。**当前阶段仅产出文档，不动代码。**
>
> **v1.1 变更（2026-08-08）**：新增 4 项任务覆盖需求 v1.1 增强细节 — T1.11/T1.12（配图提取 + 截断容错）、T2.3 修订（Doc2X 异步 PDF）、T2.9/T2.10（自动降级 + parse-pdf 端点）。任务总数 32 → 36 项。

---

## 0. 计划概述

| 项 | 说明 |
|---|---|
| 总目标 | 落地两阶段 OCR+LLM 流水线、引擎可插拔、配置下沉、前端引擎选择器、配图不丢失、引擎降级、截断容错 |
| 里程碑数 | 5 个（M0–M4） |
| 任务总数 | 36 项（见 §3 WBS，v1.1 +4） |
| 关键路径 | M0 → M1 → M2 → M3 → M4 |
| 风险等级 | 中（涉及外部 API 接入 + DB 迁移 + 灰度切换 + 降级/截断容错） |

---

## 1. 里程碑划分

| 里程碑 | 名称 | 目标 | 依赖 | 验收要点 |
|---|---|---|---|---|
| **M0** | 安全止血 | 清理 `.env` 泄露，轮换 Key | 无 | AC-01 |
| **M1** | OCR 抽象层 | `src/ai/ocr/` 模块 + QwenVlOcrProvider 兜底 + 两阶段管线跑通 | M0 | AC-02, AC-03（qwen_vl 路径）, AC-07 |
| **M2** | Doc2X 引擎接入 | Doc2XProvider 实现 + 平台默认配置 + 测试连接端点 | M1 | AC-03（doc2x 路径）, AC-06 |
| **M3** | 配置下沉 + 设置页 | `ai_settings` 表扩展 + 设置页 UI + 引擎选择器 | M1, M2 | AC-04, AC-05 |
| **M4** | MinerU + 异步化 + 评测 | MinerU(local/api) 接入 + 图片异步任务化 + 评测集 | M1, M2 | AC-08, 评测报告 |

> **关键决策**：M2（Doc2X）与 M3（配置/设置页）顺序可调换。若优先交付用户可见能力，先 M3；若优先补齐引擎能力，先 M2。**建议 M2 → M3**（设置页需要引擎已接入才有测试目标）。

---

## 2. 阶段总览图

```
M0 安全止血 ─────●  (P0, 阻塞一切)
                  │
M1 OCR 抽象层 ────●  QwenVlOcrProvider 兜底先跑通
                  │  ┌──────────────┐
M2 Doc2X 接入 ────●──┤ 新引擎接入   │
                  │  └──────────────┘
M3 配置+设置页 ───●  ai_settings 扩展 + 前端引擎选择器
                  │
M4 MinerU+异步+评测●  长尾引擎 + 工程化收尾
                  │
                  ▼
               全量上线
```

---

## 3. 任务分解（WBS）

### M0 — 安全止血（P0，阻塞）

| 任务 ID | 任务 | 涉及文件 | 依赖 |
|---|---|---|---|
| T0.1 | 吊销并轮换 DeepSeek API Key | （DeepSeek 控制台） | — |
| T0.2 | 吊销并轮换 Qwen API Key | （阿里云控制台） | — |
| T0.3 | `.env` 加入 `.gitignore`（若未加） | `.gitignore` | — |
| T0.4 | 用 `git filter-repo` 清除 `.env` 历史 | git 历史 | T0.1, T0.2 |
| T0.5 | 更新 `.env.example` 补全 OCR 配置占位（见需求 §2.2.3） | `.env.example` | — |
| T0.6 | CI 加规则：禁止 `.env` 进入版本库 | `.github/workflows/*` | T0.3 |

**M0 验收**：`git log --all -- .env` 无命中；DeepSeek/Qwen 控制台旧 Key 已禁用；CI 阻断 `.env` 提交。

---

### M1 — OCR 抽象层 + 两阶段管线（P0）

| 任务 ID | 任务 | 涉及文件 | 依赖 |
|---|---|---|---|
| T1.1 | 新增 `src/ai/ocr/mod.rs` 定义 `OcrProvider` trait + `OcrError` | `src/ai/ocr/mod.rs` | M0 |
| T1.2 | 实现 `QwenVlOcrProvider`（包装现有 vision 调用，返回 Markdown） | `src/ai/ocr/qwen_vl.rs` | T1.1 |
| T1.3 | 实现 `create_ocr_provider(cfg)` 工厂 | `src/ai/ocr/mod.rs` | T1.2 |
| T1.4 | 新增 `OcrConfig` 类型与 `resolve_ocr_config`（仅支持 auto/qwen_vl） | `src/handlers/ai.rs` | T1.3 |
| T1.5 | 新增 `parse_image_v2` handler：Stage1 OCR → Stage2 LLM 结构化 | `src/handlers/ai.rs` | T1.4 |
| T1.6 | 注册路由 `POST /api/v1/ai/parse-image-v2`（与旧端点并存） | `src/lib.rs` | T1.5 |
| T1.7 | `ai_usage_log` 扩展迁移（加 `ocr_engine`/`latency_ms`/`stage` 列） | `migrations/` | — |
| T1.8 | Stage2 复用 `cleaner.rs` + `kp_matcher.rs` 后处理 | `src/handlers/ai.rs` | T1.5 |
| T1.9 | 后端单元测试：qwen_vl 路径两阶段跑通 | `src/ai/ocr/` tests | T1.5 |
| T1.10 | 前端 `aiApi.parseImage` 增加 `ocr_provider` 可选参数透传 | `frontend/src/api/client.ts` | T1.6 |
| T1.11 | **v1.1**：`ParsedQuestion` 新增 `image_urls` 字段 + Stage 2 Prompt 规则（提取 `![...](url)` 到数组）+ 前端类型同步 | `src/ai/types.rs`, `src/ai/prompt.rs`, `frontend/src/api/client.ts` | T1.5 |
| T1.12 | **v1.1**：`cleaner.rs` 截断容错强化（补全闭合符/丢弃末题/`truncated` 标记）+ Stage 2 `max_tokens≥4096` 配置 | `src/ai/cleaner.rs`, `src/ai/deepseek.rs` | T1.5 |

**M1 验收**：
- AC-02：`src/ai/ocr/` 模块存在，trait + QwenVlOcrProvider 实现
- AC-03（qwen_vl 路径）：`parse-image-v2` 返回与旧 `parse_image` 等价结果
- AC-07：未配 OCR 走 qwen_vl，行为等价重构前
- AC-09（v1.1）：Stage 2 提取 `image_urls`，含图 Markdown 不丢图
- AC-12（v1.1）：Stage 2 `max_tokens≥4096`，截断输入时 cleaner 返回前 N-1 题 + `warnings`

---

### M2 — Doc2X 引擎接入（P1）

| 任务 ID | 任务 | 涉及文件 | 依赖 |
|---|---|---|---|
| T2.1 | 调研 Doc2X API：鉴权、图片接口、PDF 异步接口（submit→poll）、限流 | （外部文档） | M1 |
| T2.2 | 实现 `Doc2XProvider`（图片 OCR → Markdown） | `src/ai/ocr/doc2x.rs` | T2.1 |
| T2.3 | **v1.1 修订**：实现 `Doc2XProvider::ocr_pdf_async`（POST 提交→3s 间隔轮询→全文 Markdown，上限 120s）；不再走前端逐页切片 | `src/ai/ocr/doc2x.rs` | T2.2 |
| T2.4 | `OcrConfig` 扩展支持 `doc2x` + `doc2x_api_key` + `doc2x_base_url` | `src/handlers/ai.rs` | T2.2 |
| T2.5 | `resolve_ocr_config` 扩展：支持 doc2x 选项与用户/平台优先级 | `src/handlers/ai.rs` | T2.4 |
| T2.6 | 新增 `POST /api/v1/ai/ocr/test-connection` 端点 | `src/handlers/ai.rs`, `src/lib.rs` | T2.2 |
| T2.7 | Doc2X 错误处理与重试（429/5xx 有限重试） | `src/ai/ocr/doc2x.rs` | T2.2 |
| T2.8 | 后端集成测试：Doc2X 图片 + PDF 异步路径 | tests | T2.3, T2.6 |
| T2.9 | **v1.1**：OCR 自动降级 `should_fallback(e)` + 首选失败降级 QwenVlOcrProvider + 响应 `fallback_notice` 字段 + `ai_usage_log` 记 `fallback_from/to` | `src/handlers/ai.rs`, `src/ai/ocr/mod.rs` | T2.2, T1.2 |
| T2.10 | **v1.1**：新增 `POST /api/v1/ai/parse-pdf` 端点（multipart PDF 整体 → Doc2X 异步 → Stage 2 结构化） + 前端弹窗 PDF 模式改调此端点（不再 `pdfToImages` 切片） | `src/handlers/ai.rs`, `src/lib.rs`, `AiRecognizeDialog.vue` | T2.3, T2.9 |

**M2 验收**：
- AC-03（doc2x 路径）：`parse-image-v2?ocr_provider=doc2x` 返回结构化题
- AC-06：`/ocr/test-connection` 返回 `{ok, latency_ms, message}`
- AC-10（v1.1）：PDF 走 `/ai/parse-pdf`，Doc2X 异步轮询，前端不切片
- AC-11（v1.1）：Doc2X 429/超时自动降级 qwen_vl，响应带 `fallback_notice`

---

### M3 — 配置下沉 + 设置页 + 引擎选择器（P1）

| 任务 ID | 任务 | 涉及文件 | 依赖 |
|---|---|---|---|
| T3.1 | DB 迁移：`ai_settings` 加 `ocr_provider`/`doc2x_api_key_enc`/`mineru_api_endpoint`/`mineru_api_key_enc` | `migrations/` | M1 |
| T3.2 | `models/ai_setting.rs` 扩展：新增字段 + AES 加解密复用 | `src/models/ai_setting.rs` | T3.1 |
| T3.3 | `GET/PUT /api/v1/ai/settings` 扩展请求/响应结构 | `src/handlers/ai.rs` | T3.2 |
| T3.4 | 前端 `AiSettings` 类型扩展 + `aiApi.updateSettings` | `frontend/src/api/client.ts` | T3.3 |
| T3.5 | 设置页新增「OCR 模型设置」板块（引擎下拉 + 动态 Key/Endpoint + 测试连接） | 设置页组件 | T3.4, T2.6 |
| T3.6 | 设置页「测试连接」按钮调 `/ocr/test-connection` | 设置页组件 | T3.5 |
| T3.7 | `AiRecognizeDialog.vue` 顶部加引擎轻量下拉（本次覆盖） | `frontend/src/views/edit/components/AiRecognizeDialog.vue` | T3.4 |
| T3.8 | `AiRecognizeDialog.vue` 调用 `parseImage` 透传 `ocr_provider` | 同上 | T3.7 |
| T3.9 | 前端校验：Key 输入框掩码显示，不发控制台日志 | 设置页组件 | T3.5 |

**M3 验收**：
- AC-04：设置页保存 OCR 配置，DB 中 Key 为 BYTEA 密文（查不到明文）
- AC-05：弹窗引擎下拉切换后，识别调用对应引擎

---

### M4 — MinerU + 异步化 + 评测体系（P2）

| 任务 ID | 任务 | 涉及文件 | 依赖 |
|---|---|---|---|
| T4.1 | 调研 MinerU local 部署形态（Docker / API 形态） | （外部文档） | M1 |
| T4.2 | 实现 `MineruLocalProvider`（HTTP 调私有 endpoint） | `src/ai/ocr/mineru_local.rs` | T4.1 |
| T4.3 | 实现 `MineruApiProvider`（MinerU Cloud） | `src/ai/ocr/mineru_api.rs` | T4.1 |
| T4.4 | `resolve_ocr_config` 扩展支持 mineru_local/mineru_api | `src/handlers/ai.rs` | T4.2, T4.3 |
| T4.5 | 图片解析异步化：`ai_parse_tasks` 表支持 image 类型 | `src/handlers/ai_tasks.rs`, `src/workers/ai_parse_worker.rs` | M1 |
| T4.6 | 前端弹窗图片模式改异步轮询（复用 `useAiParsePolling`） | `AiRecognizeDialog.vue`, `useAiParsePolling.ts` | T4.5 |
| T4.7 | 建立评测集：50–100 道题 golden dataset（文本/单图/多图/PDF） | `tests/eval/` | — |
| T4.8 | 评测脚本：字段级 F1 报告（stem/options/answer/analysis） | `tests/eval/` | T4.7 |
| T4.9 | Prompt 版本化：`prompt.rs` 加版本号 + 识别结果关联 | `src/ai/prompt.rs`, `ai_usage_log` | — |
| T4.10 | 额度可配置化：50/日硬编码改 `ai_quota_config` 表 | `migrations/`, `src/handlers/ai.rs` | — |
| T4.11 | 旧 `parse_image` 端点下线（灰度稳定后） | `src/lib.rs`, `src/handlers/ai.rs` | T4.6 |

**M4 验收**：
- AC-08：`ai_usage_log` 含 `ocr_engine`/`latency_ms`/`stage`，可统计各引擎用量
- 评测集 F1 报告产出，作为重构前后对比基线

---

## 4. 任务依赖关系图

```
M0 ──────────────────────────────────────────────────────────────►
 │
 ▼
M1 (T1.1→T1.2→T1.3→T1.4→T1.5→T1.6→T1.8→T1.9)
 │   +T1.11 (image_urls) / T1.12 (截断容错)   │
 │   T1.7 (DB 迁移, 独立)                     │
 │                                            ▼
 ▼                                          M3 (T3.1→T3.2→T3.3→T3.4→T3.5→T3.6)
M2 (T2.1→T2.2→T2.3→T2.4→T2.5→T2.6→T2.7→T2.8)
     +T2.9 (降级) / T2.10 (parse-pdf 端点)      │
 │                                            │
 │                                            ▼
 ▼                                          T3.7→T3.8→T3.9
M4 (T4.1→T4.2→T4.3→T4.4)
     T4.5→T4.6 (异步化, 依赖 M1)
     T4.7→T4.8 (评测, 独立)
     T4.9, T4.10, T4.11 (收尾)
```

---

## 5. 验收标准（与需求文档对齐）

| 编号 | 验收项 | 对应里程碑 |
|---|---|---|
| AC-01 | `.env` 已从 Git 历史清除，Key 已轮换，CI 校验 | M0 |
| AC-02 | `src/ai/ocr/` 模块存在，`OcrProvider` trait + 兜底实现 | M1 |
| AC-03 | `parse-image-v2` 支持 `ocr_provider` 参数，两阶段跑通 | M1(qwen_vl) + M2(doc2x) + M4(mineru) |
| AC-04 | 设置页保存 OCR 配置，DB 中 Key 为密文 | M3 |
| AC-05 | 识别弹窗引擎下拉可切换并生效 | M3 |
| AC-06 | `/ocr/test-connection` 返回引擎可用性与延迟 | M2 |
| AC-07 | 未配 OCR 走 qwen_vl，行为等价重构前 | M1 |
| AC-08 | `ai_usage_log` 记录引擎与延迟 | M1(T1.7) + M4 |
| AC-09 | Stage 2 提取 `image_urls`，几何题配图不丢失（v1.1） | M1(T1.11) |
| AC-10 | PDF 走 `/ai/parse-pdf`，Doc2X 异步轮询，前端不切片（v1.1） | M2(T2.3, T2.10) |
| AC-11 | 首选引擎 429/401/403/超时自动降级 qwen_vl，响应带 `fallback_notice`（v1.1） | M2(T2.9) |
| AC-12 | Stage 2 `max_tokens≥4096`，截断时 cleaner 返回前 N-1 题 + `warnings`（v1.1） | M1(T1.12) |

---

## 6. 风险与对策

| 风险 | 影响 | 概率 | 对策 |
|---|---|---|---|
| Doc2X API 限流/付费门槛 | M2 阻塞 | 中 | 平台默认配额池 + 用户自带 Key 双轨；兜底 qwen_vl |
| MinerU 私有部署依赖用户环境 | M4 推进慢 | 高 | 标为 P2，不阻塞主线；提供 Docker compose 样例 |
| 两阶段管线延迟增加（OCR+LLM 串行） | 用户体验下降 | 中 | 异步化（T4.5/T4.6）+ 进度反馈；OCR 与 LLM 超时分离配置 |
| LLM 输出被 max_tokens 截断（v1.1） | JSON 解析失败 / 丢题 | 中 | `max_tokens≥4096`（T1.12）+ cleaner 截断容错返回前 N-1 题 + `truncated` 监控；>20 题前端提示分批 |
| 自动降级被滥用 / 用户不知情（v1.1） | 误以为高精度引擎生效 | 中 | 响应带 `fallback_notice` + 前端 toast.warning；设置页可关「自动降级」；`ai_usage_log` 记 `fallback_from/to` |
| DB 迁移在生产事故 | 数据丢失 | 低 | 迁移幂等（`IF NOT EXISTS`）+ 回滚脚本 + 预发环境验证 |
| 旧端点下线影响未升级客户端 | 兼容性 | 中 | 旧 `parse_image` 保留 ≥ 1 版本周期，灰度切换 |
| Prompt 改动影响识别准确率 | 回归 | 中 | 评测集（T4.7）守门；Prompt 版本化（T4.9） |

---

## 7. 不在本次范围

- ❌ Markdown 粘贴模式改造为调后端（保留前端 `parseMarkdownToQuestion`，后续可选优化）
- ❌ 图片占位符二次绑定闭环（需求审计中 #20，单列后续迭代）
- ❌ 知识点低置信度人工确认 UI（需求审计中 #12，单列后续迭代）
- ❌ 后端 LLM 重试/熔断（需求审计 P3-11，本次仅 OCR 层做重试）
- ❌ 推倒现有 DeepSeek 文本 LLM 集成

---

## 8. 交付物清单

| 文档/代码 | 状态 |
|---|---|
| `docs/AI智能录题_审计报告.md` | ✅ 已产出 |
| `docs/AI智能录题_需求分析.md` | ✅ 已产出（本文档配套） |
| `docs/AI智能录题_任务计划书.md` | ✅ 本文档 |
| `src/ai/ocr/` 模块 | ⏳ 待 M1 启动 |
| `migrations/*_ai_settings_ocr.sql` | ⏳ 待 M3 |
| `migrations/*_ai_usage_log_ocr.sql` | ⏳ 待 M1 |
| 评测集 `tests/eval/` | ⏳ 待 M4 |
| 设置页 OCR 板块 | ⏳ 待 M3 |
| 识别弹窗引擎下拉 | ⏳ 待 M3 |

---

## 9. 启动建议

> **当前阶段**：3 份文档已就绪，**未改动任何代码**。
>
> **下一步建议**：
> 1. 评审 3 份文档（审计 / 需求 / 计划），确认 M0–M4 里程碑与优先级
> 2. 确认 Doc2X 是否采购（影响 M2 排期）与 MinerU 是否私有部署（影响 M4 排期）
> 3. 启动 M0 安全止血（T0.1–T0.6）— 此阶段不依赖任何外部决策，可立即开始
> 4. M0 完成后启动 M1（T1.1–T1.10）— 此阶段仅依赖现有 qwen_vl，无外部依赖
