# AI 智能录题 / AI 智能识别 — 全栈代码审计与完成度盘点报告

## 一句话总结

> **这是一个真实调通第三方 LLM 的生产级实现（非 Mock），已具备图片/PDF 批量 OCR + 结构化拆题 + 知识点匹配 + 表单回填的完整闭环；主要断层在于：(1) `.env` 已泄露真实 DeepSeek/Qwen API Key；(2) Markdown 模式前端自解析，未复用后端 `/ai/parse-text`；(3) 无专用公式 OCR（Doc2X 等），完全依赖视觉大模型一次性输出 LaTeX；(4) 图片解析为同步阻塞，无异步任务化。**

---

## 一、前端与后端涉及代码路径清单

### 1.1 前端（`frontend/src/`）

| 文件 | 角色 | 关键行 |
|---|---|---|
| `views/QuestionEdit.vue` | 入口按钮 `AI 智能识别` + 弹窗宿主 + 多题工作台 | L13 按钮、L272 弹窗、L1548 `batch-parsed` 接收 |
| `views/edit/components/AiRecognizeDialog.vue` | **核心弹窗**：双模式 Tab、上传/拖拽/粘贴、Loading 进度条、预览、回填、快照续传 | L85 `doAiParse`、L237 `doImageParse`、L262 `doPdfParse`、L318 `handleBatchResults`、L110 `doApplyAiResult` |
| `views/edit/components/AttributeSidePanel.vue` | 次入口（侧栏 AI 按钮） | — |
| `api/client.ts` | AI API 类型化封装 | L770-959 `aiApi` / `aiTaggingApi` / `aiTaskApi` |
| `composables/useAiParsePolling.ts` | 异步任务轮询 Composable | — |
| `utils/parseMarkdown.ts` | **纯前端** Markdown→ParsedQuestion 解析 + `RECOMMENDED_PROMPT` | 被 dialog L95 调用 |
| `utils/imageCompressor.ts` | 上传前图片压缩 | — |
| `utils/pdfToImages.ts` | PDF→图片逐页渲染 | — |
| `utils/concurrency.ts` | `runWithConcurrency` / `withBackoffRetry` | — |
| `utils/batchSnapshot.ts` | IndexedDB 批量录入断点续传 | — |
| `components/LatexRender.vue` | KaTeX 渲染（识别结果展示） | — |

### 1.2 后端（`src/`，Rust + Axum）

| 文件 | 角色 | 关键行 |
|---|---|---|
| `lib.rs` | 路由注册 | L216-225 `parse-text`/`parse-image`/`parse`/`parse/{id}` |
| `main.rs` | worker 启动 | L75 `ai_parse_worker::start_worker` |
| `handlers/ai.rs` | 同步解析 handler | L379 `parse_text`、L416 `parse_image`、L537 `resolve_ai_config` |
| `handlers/ai_tasks.rs` | 异步任务队列 | L82 `submit_parse_task`、L120 `get_task_status` |
| `handlers/ai_tagging.rs` | 已有题目 AI 打标 | L258 `AI_EXTRACT_KEYS_PROMPT`、L298 `AI_CONVERGE_PROMPT` |
| `ai/provider.rs` | `AiProvider` trait + `create_provider` 工厂 | L18/L26/L34/L48 |
| `ai/deepseek.rs` | **真实 HTTP 调用**（OpenAI 兼容协议） | L77 `parse_text_with_prompt`、L136 `parse_image_with_prompt` |
| `ai/prompt.rs` | Prompt 工程（核心规则 + 3 特化） | L28 `CORE_PARSE_RULES`、L200 `BATCH_IMAGE_OCR_FULL_PROMPT` |
| `ai/cleaner.rs` | LLM 输出清洗（剥 fenced code / 花括号计数 / 去尾逗号） | L9 `clean_llm_json`、L39 `extract_json_by_bracket_count` |
| `ai/kp_matcher.rs` | 知识点语义匹配 | — |
| `ai/types.rs` | `ParsedQuestion` 结构体 | — |
| `workers/ai_parse_worker.rs` | 后台 worker（轮询 ai_parse_tasks） | L168 `.parse_text()` |
| `models/ai_setting.rs` | 用户 AI Key（AES 加密存库） | `encrypt_api_key`/`decrypt_api_key` |

### 1.3 配置

| 文件 | 内容 |
|---|---|
| `frontend/vite.config.ts` | L24-34 proxy `/api` + `/uploads` → `127.0.0.1:3000`（AI 路由 `/api/v1/ai/*` 已被覆盖） |
| `.env.example` | L30-50 `AI_DEFAULT_PROVIDER`、`AI_KEY_ENCRYPTION_KEY`、`AI_DEFAULT_MODEL_TEXT/VISION`、`DEEPSEEK/QWEN/OPENAI_API_KEY`、`OPENAI_BASE_URL` |
| `.env` | ⚠️ **已提交真实 Key**：`DEEPSEEK_API_KEY=sk-****`（已脱敏）、`QWEN_API_KEY=sk-****`（已脱敏） |

---

## 二、数据流转与结构化能力（Pipeline）

### 2.1 图片/PDF 解析主链路

```
用户拖拽图片/PDF
  ↓ (前端)
pdfToImages → 逐页渲染 (PDF.js)
  ↓
compressImage (WebP 压缩)
  ↓
runWithConcurrency (并发 3) + withBackoffRetry (指数退避)
  ↓ aiApi.parseImage(file)  →  POST /api/v1/ai/parse-image
  ↓ (后端)
① 额度熔断：INSERT INTO ai_usage_log ... WHERE COUNT < 50 RETURNING (原子防 TOCTOU)
② Multipart 流式分块读取 + infer Magic Number 零信任校验 (JPEG/PNG/WebP)
③ base64 编码 → drop 原始 bytes (防双驻留内存峰值)
④ resolve_ai_config：用户个人 Key (AES 解密) 优先 → 平台默认 Key
⑤ create_provider → DeepSeekProvider::parse_image_with_prompt
   → POST {base_url}/v1/chat/completions  (model=qwen-vl-plus, vision 多模态)
   → system: BATCH_IMAGE_OCR_FULL_PROMPT (CORE_PARSE_RULES + 批量切分规则)
   → user: [{type:image_url, image_url:{url:data:image/png;base64,...}}]
⑥ 后处理 post_process_batch:
   - clean_and_parse → 剥 ```json / 花括号计数 / 去尾逗号 → serde 反序列化
   - 校验 question_type 合法性
   - 补全 analysis 默认结构
   - match_knowledge_nodes: LLM 提取的 knowledge_points → DB knowledge_nodes 语义匹配
⑦ 返回 {data: ParsedQuestion[]}
  ↓ (前端)
handleBatchResults → emit('batch-parsed', questions)
  ↓
父组件 QuestionEdit 进入"多题工作台" → 逐题 Tab 切换 → 人工校对 → 入库
```

### 2.2 文本解析（两条分裂路径 ⚠️）

**路径 A（弹窗 Markdown 模式，前端自解析）：**
```
用户粘贴 AI 输出的 Markdown
  ↓ parseMarkdownToQuestion(aiText)  [纯前端 regex 解析，不调后端]
  ↓ aiResult 预览 → applyAiResult 回填表单
```

**路径 B（异步任务队列，后端 LLM）：**
```
POST /api/v1/ai/parse {raw_text}
  → INSERT INTO ai_parse_tasks (status=pending)
  → 返回 {task_id}
后台 worker 轮询 → parse_text(raw_text, model)
  → DeepSeekProvider::parse_text_with_prompt (model=deepseek-chat)
  → post_process_single → 入库 question
前端 useAiParsePolling 轮询 GET /ai/parse/{id} → 获取结果
```

### 2.3 公式（LaTeX）处理

- **无独立公式 OCR 模块**，完全依赖视觉大模型按 Prompt 规范直接输出 LaTeX：
  - Prompt 明确要求：`公式必须转为 LaTeX，不要保留图片中的像素字符`、`不要把公式转义为 Unicode（如 x² 应写 $x^2$）`、`行内 $...$，块级 $$...$$`
- 几何图形/函数图象/表格 → Prompt 指示插入占位符 `![配图](IMAGE_PLACEHOLDER_N)` 进 `image_placeholders` 数组（但前端目前未见对占位符的二次图片上传/绑定逻辑 — **潜在缺口**）
- 前端 LatexRender.vue (KaTeX) 负责最终渲染

### 2.4 额度与安全

- 图片解析：硬编码 50 次/日/用户（`ai.rs` L427 SQL `WHERE COUNT < 50`）
- 仅 `entry_method='ocr'` 触发配额扣减（`questions.rs` L819）；`manual`/`ai_parse` 跳过
- 用户 API Key：AES 加密入库（`AI_KEY_ENCRYPTION_KEY` 主密钥）
- 图片零信任：Magic Number 校验，不信前端 Content-Type
- 内存防护：流式分块 + 显式 `drop(image_bytes)`

---

## 三、功能完成度对照表

| # | 功能点 | 前端状态 | 后端状态 | 阻碍点 / 未完成原因 |
|---|---|---|---|---|
| 1 | AI 智能识别入口按钮 | ✅ `QuestionEdit.vue:13` | — | — |
| 2 | 弹窗双模式 Tab（Markdown / 图片） | ✅ `AiRecognizeDialog.vue:372` | — | — |
| 3 | 图片拖拽/点击上传 | ✅ `:416` | ✅ Multipart 流式读取 `:457` | — |
| 4 | 图片压缩预处理 | ✅ `imageCompressor.ts` | — | — |
| 5 | PDF 逐页渲染 + 批量识别 | ✅ `pdfToImages` + `runWithConcurrency` | ⚠️ 后端无 PDF 专用接口（每页当单图调 parse-image） | 大 PDF 触发 N 次后端调用，无服务端 PDF 一次性处理 |
| 6 | 单图 OCR → 结构化题 | ✅ `aiApi.parseImage` | ✅ `parse_image` → qwen-vl-plus 真实调用 `:516` | — |
| 7 | 批量多题切分（一图多题） | ✅ 接收 `ParsedQuestion[]` | ✅ Prompt `BATCH_IMAGE_OCR_FULL_PROMPT` 要求输出 `questions[]` | — |
| 8 | Markdown 粘贴解析 | ⚠️ **纯前端 `parseMarkdownToQuestion`**（不调后端） | ✅ 后端 `/ai/parse-text` 已实现但**未被此模式复用** | **路径分裂**：前端要求用户先用外部 AI 生成 Markdown 再粘贴，后端 LLM 能力闲置 |
| 9 | 异步任务队列（文本） | ⚠️ `useAiParsePolling` 存在但未在主弹窗接入 | ✅ `/ai/parse` + worker | 异步路径仅在独立流程使用，弹窗未统一 |
| 10 | 解析结果预览 UI | ✅ `:466` 题干/选项/答案/解析分块预览 | — | 单题预览完整；批量预览移交父组件工作台 |
| 11 | 表单字段回填 | ✅ `doApplyAiResult` `:110` 覆盖 stem/options/blanks/sub_answers/solutions/difficulty | — | — |
| 12 | 知识点 AI 匹配 | ✅ 高置信度(≥0.95)自动勾选 `:166` | ✅ `kp_matcher.rs` 语义匹配 | 低置信度无人工确认 UI |
| 13 | 已有题目 AI 打标 | ✅ `aiTaggingApi.tag` | ✅ `handlers/ai_tagging.rs` (extract+converge 双 prompt) | — |
| 14 | 用户自定义 AI Key（设置页） | ✅ `aiApi.getSettings/updateSettings` | ✅ AES 加密入库 + 用户优先级 | — |
| 15 | 额度管控（图片 50/日） | ✅ `ocr_quota_*` 字段 | ✅ 原子 INSERT 熔断 `:423` | 额度硬编码 50，无管理员可配置 |
| 16 | 退避重试 | ✅ `withBackoffRetry` | — | 后端无重试（依赖前端） |
| 17 | 断点续传（批量录入） | ✅ IndexedDB `batchSnapshot` | — | — |
| 18 | 多题工作台（批量逐题校对） | ✅ 父组件 Tab 切换 | — | — |
| 19 | 专用公式 OCR（Doc2X/TextIn） | ❌ 未实现 | ❌ 未实现 | 完全依赖 LLM 视觉一次性输出，复杂公式/手写体准确率依赖 qwen-vl-plus |
| 20 | 图片占位符二次绑定 | ⚠️ Prompt 输出 `image_placeholders[]` | ⚠️ 后端透传 | **前端未见占位符→原图裁剪/上传绑定逻辑**，配图丢失 |
| 21 | 图片解析异步任务化 | ❌ 同步阻塞（前端 await） | ❌ 仅文本有异步队列 | 大图/多图体验差，超时 120s |
| 22 | 识别准确率/评测 | ❌ 无评测集 | ❌ 无回归测试 | 无 ground truth 数据集，准确率不可量化 |
| 23 | .env 安全 | — | ❌ **真实 API Key 已提交仓库** | `.env:12` `DEEPSEEK_API_KEY=sk-d289498...`、`.env:15` `QWEN_API_KEY=sk-ws-...` |

---

## 四、针对重构该功能的技术路线建议

### 🔴 P0 — 安全止血（必须先做）

1. **立即吊销并轮换** `.env` 中泄露的 `DEEPSEEK_API_KEY` 与 `QWEN_API_KEY`（已进入 Git 历史，仅删除文件不够，需 `git filter-repo` 或重置历史）
2. 确认 `.env` 已在 `.gitignore`，仅保留 `.env.example` 模板

### 🟡 P1 — 统一解析路径（消除架构分裂）

3. **统一文本解析入口**：弹窗 Markdown 模式改为调用后端 `/ai/parse-text`（或异步 `/ai/parse`），废弃纯前端 `parseMarkdownToQuestion`。理由：后端有完整 Prompt 工程 + 清洗 + 知识点匹配，前端自解析绕过了所有这些能力。
4. **图片解析异步化**：复用 `ai_parse_tasks` 表扩展支持 image 类型，前端改为提交任务 → 轮询，消除 120s 同步阻塞超时风险。

### 🟢 P2 — 识别能力增强

5. **引入专用公式 OCR 兜底**：对 LLM 视觉置信度低（`confidence < 0.7`）或含 `image_placeholders` 的题，可选接入 **Doc2X**（数学公式 OCR 行业标杆）做二次校准，再交 LLM 结构化。形成「LLM 视觉主路径 + Doc2X 公式增强」双引擎。
6. **图片占位符闭环**：前端识别到 `image_placeholders[]` 后，弹出原图让用户框选裁剪对应区域并上传，绑定到题干对应位置（目前占位符是死数据）。
7. **知识点匹配人工确认 UI**：低置信度（0.5–0.95）匹配结果展示候选列表供用户勾选，而非静默跳过。

### 🔵 P3 — 工程化与可观测

8. **准确率评测集**：建立 50–100 道题的 golden dataset（含纯文本/单图/多图/PDF），跑回归报告字段级 F1，作为重构前后对比基线。
9. **额度可配置化**：50/日 硬编码改为 `ai_quota_config` 表，支持管理员按角色/空间调整。
10. **Prompt 版本化**：`prompt.rs` 当前是 const + LazyLock，建议加版本号落库，识别结果关联 prompt 版本，便于 A/B 与回归。
11. **后端重试与限流**：`deepseek.rs` 当前无重试（仅前端 `withBackoffRetry`），建议后端对 429/5xx 加有限重试 + 熔断器。

### 重构优先级建议

> **建议保留现有架构骨架（已相当成熟），重点做 P0 安全止血 + P1 路径统一 + P2 公式增强**，而非推倒重来。真正缺失的是「专用公式 OCR + 占位符闭环 + 评测体系」三块，其余皆为已完成且可用。

---

## 附录：状态图例

- ✅ **已完成且可用**
- ⚠️ **已写 UI 但接口是 Mock/未调通**（或路径分裂/未复用）
- 🚧 **已通接口但识别准确率/结构化极差**
- ❌ **完全未实现**
