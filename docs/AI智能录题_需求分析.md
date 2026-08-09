# AI 智能录题 — 重构需求分析（v1.1）

> 本文档基于《AI智能录题_审计报告》的现状盘点，结合 3 项架构调整建议，形成最终需求规格。
> 配套文档：《AI智能录题_任务计划书.md》
>
> **v1.1 变更（2026-08-08）**：补充 4 项关键细节 — ① Stage 2 配图提取（`image_urls`，解决几何题丢图）；② Doc2X PDF 原生异步解析（不再前端切片）；③ OCR 引擎自动降级（首选失败→qwen_vl 兜底 + `fallback_notice`）；④ 大文本多题截断防护（`max_tokens≥4096` + cleaner 容错）。

---

## 0. 文档定位

| 项 | 说明 |
|---|---|
| 现状基线 | 详见 `docs/AI智能录题_审计报告.md` |
| 本文档作用 | 定义「重构后」的目标架构、功能/非功能需求、数据与接口契约 |
| 不在本文档范围 | 实施步骤、排期、任务分解（见任务计划书） |
| 约束 | 本次仅产出文档，**不改动项目代码** |

---

## 1. 背景与目标

### 1.1 现状一句话总结（引自审计报告）

> 现有系统是**真实调通第三方 LLM 的生产级实现（非 Mock）**，已具备图片/PDF 批量 OCR + 结构化拆题 + 知识点匹配 + 表单回填的完整闭环；但存在 4 处断层：`.env` 泄露真实 Key、Markdown 模式前端自解析绕过后端、无专用公式 OCR、图片解析同步阻塞。

### 1.2 重构核心目标

1. **解耦「OCR 识别」与「LLM 结构化」**：从单阶段（Qwen-VL 直出 JSON）演进为两阶段流水线（OCR 引擎出 Markdown → 文本 LLM 结构化为 JSON），提升公式准确率与可替换性。
2. **引擎可插拔**：抽象 `OcrProvider` trait，支持 Doc2X / MinerU / Qwen-VL 三种引擎按用户配置切换。
3. **配置下沉**：OCR 引擎选择与个人 Key 复用现有 AES 加密体系，统一在 `ai_settings` 表管理。
4. **前端可感知**：识别弹窗与设置页暴露引擎选择能力，让用户根据场景（高精度公式 / 私有化部署 / 通用）自主抉择。
5. **安全止血**：清理 `.env` 泄露，吊销并轮换 Key。

### 1.3 非目标（本次不做）

- ❌ 不重写已有题目 AI 打标（`handlers/ai_tagging.rs`）逻辑
- ❌ 不替换现有 DeepSeek 文本 LLM 集成（Stage 2 复用）
- ❌ 不改动虚拟滚动列表 / 题目详情页 UI
- ❌ 不引入新的前端 UI 框架

---

## 2. 架构调整方案

### 2.1 调整一：OcrProvider 抽象层 + Two-Stage Pipeline

#### 2.1.1 现状对比

| 维度 | 现状（单阶段） | 调整后（两阶段） |
|---|---|---|
| 模块 | `src/ai/provider.rs` 单 trait `AiProvider` | 新增 `src/ai/ocr/` 模块，抽象 `OcrProvider` trait |
| 流程 | Qwen-VL 一次输出结构化 JSON | OCR 引擎出 Markdown（含 LaTeX）→ DeepSeek-V3 结构化为 JSON |
| 引擎 | 仅 DeepSeek/Qwen（OpenAI 兼容） | OCR 层：Doc2X / MinerU(local/api) / Qwen-VL；LLM 层：DeepSeek-V3 |
| 公式准确率 | 依赖 qwen-vl-plus 单次能力 | 专用公式 OCR（Doc2X/MinerU）显著提升复杂公式识别 |
| 可替换性 | LLM 不可替换 OCR 能力 | OCR 与 LLM 独立演进、独立配额、独立限流 |

#### 2.1.2 两阶段流水线设计

```
输入 (图片/PDF)
   │
   ▼
┌─────────────────────────────────────────────┐
│  Step 1: OcrProvider                        │
│  ├─ Doc2XProvider    (外部 API, 高精公式)    │
│  ├─ MineruLocalProvider (私有化部署, 本地)    │
│  ├─ MineruApiProvider (MinerU Cloud)         │
│  └─ QwenVlOcrProvider (兜底, 等价现状)        │
│  输出: 纯净 Markdown (含 $...$ / $$...$$)    │
└─────────────────────────────────────────────┘
   │  markdown: String
   ▼
┌─────────────────────────────────────────────┐
│  Step 2: AiProvider (复用现有)              │
│  ├─ DeepSeekProvider::parse_text_with_prompt│
│  │   (model=deepseek-chat, 复用 TEXT_PARSE) │
│  └─ 后处理: cleaner + kp_matcher (现有)     │
│  输出: ParsedQuestion / Vec<ParsedQuestion> │
└─────────────────────────────────────────────┘
```

#### 2.1.3 OcrProvider trait 契约（目标定义）

```rust
// src/ai/ocr/mod.rs（新增）
#[async_trait]
pub trait OcrProvider: Send + Sync {
    /// 引擎标识，用于日志与配额记账
    fn id(&self) -> &'static str;        // "doc2x" | "mineru_local" | "mineru_api" | "qwen_vl"

    /// 单图 OCR → Markdown（含 LaTeX）
    async fn ocr_image(&self, image_b64: &str) -> Result<String, OcrError>;

    /// 是否支持 PDF 直传（Doc2X/MinerU 原生支持，Qwen-VL 不支持）
    fn supports_pdf(&self) -> bool { false }

    /// PDF OCR → 每页 Markdown（默认实现：前端已渲染为图片，逐页调 ocr_image）
    async fn ocr_pdf(&self, _pdf_bytes: &[u8]) -> Result<Vec<String>, OcrError> {
        Err(OcrError::UnsupportedPdf)
    }
}

pub fn create_ocr_provider(cfg: &OcrConfig) -> Box<dyn OcrProvider>;
```

#### 2.1.4 引擎能力矩阵

| 引擎 | 部署形态 | 公式精度 | PDF 原生 | 速率 | 成本 | 适用场景 |
|---|---|---|---|---|---|---|
| Doc2X | 外部 API | ⭐⭐⭐⭐⭐ | ✅ | 中 | 按量付费 | 含复杂公式的试卷、论文 |
| MinerU (local) | 私有化 | ⭐⭐⭐⭐ | ✅ | 慢 | 0（自建） | 数据敏感、离线环境 |
| MinerU (api) | MinerU Cloud | ⭐⭐⭐⭐ | ✅ | 中 | 按量付费 | 不愿自建但需高精度 |
| Qwen-VL（兜底） | 阿里云 | ⭐⭐⭐ | ❌ | 快 | 低 | 简单题、兜底默认 |

### 2.2 调整二：配置与密钥存储扩充

#### 2.2.1 数据库表变更

扩展现有 `ai_settings` 表（已存 `provider`/`api_key_enc`/`model_text`/`model_vision`），新增 OCR 配置列：

| 列名 | 类型 | 说明 | 加密 |
|---|---|---|---|
| `ocr_provider` | `TEXT` | `doc2x` \| `mineru_local` \| `mineru_api` \| `qwen_vl` \| `auto` | 否 |
| `doc2x_api_key_enc` | `BYTEA` | 用户个人 Doc2X Key | ✅ AES |
| `mineru_api_endpoint` | `TEXT` | 私有 MinerU 服务地址（如 `http://10.0.0.5:8000`） | 否 |
| `mineru_api_key_enc` | `BYTEA` | MinerU Cloud Key（若用 Cloud 形态） | ✅ AES |

#### 2.2.2 解析优先级

```
resolve_ocr_config(user):
  1. 用户个人配置 ocr_provider 非 'auto' → 用用户 Key/Endpoint
  2. 用户配置为 'auto' 或未设 → 回落平台默认（环境变量 OCR_DEFAULT_PROVIDER）
  3. 平台默认未配 → 回落 'qwen_vl'（等价现状，兜底）
```

#### 2.2.3 环境变量扩充（`.env.example`）

```bash
# ===== OCR 引擎平台默认配置 =====
OCR_DEFAULT_PROVIDER=auto              # auto | doc2x | mineru_local | mineru_api | qwen_vl
DOC2X_API_KEY=                          # 平台默认 Doc2X Key（用户未配时兜底）
DOC2X_BASE_URL=https://api.doc2x.noedgex.com/v1
MINERU_DEFAULT_ENDPOINT=               # 平台默认私有 MinerU 地址
MINERU_API_KEY=                         # 平台默认 MinerU Cloud Key
```

> 复用现有 `AI_KEY_ENCRYPTION_KEY` 主密钥对用户个人 Key 做 AES 加密。

### 2.3 调整三：前端引擎选择器

#### 2.3.1 设置页（`AiSettings.vue` 或对应设置组件）

新增「OCR 模型设置」板块：

```
┌─ OCR 模型设置 ─────────────────────────────────┐
│                                                │
│  OCR 引擎   [ 默认自动 ▾ ]                     │
│             ├ 默认自动（智能识别）              │
│             ├ Doc2X 极高精公式引擎（推荐）      │
│             ├ MinerU 高精度解析（私有/自建）    │
│             └ Qwen-VL 通用视觉（兜底）          │
│                                                │
│  ─ 选择 Doc2X 时显示 ──                         │
│  Doc2X API Key  [________________]  [测试连接] │
│                                                │
│  ─ 选择 MinerU 时显示 ──                        │
│  MinerU Endpoint [http://__________:____]      │
│  MinerU Cloud Key [________________] (可选)     │
│                                                │
│              [ 保存设置 ]                      │
└────────────────────────────────────────────────┘
```

- 「测试连接」按钮：调用后端 `POST /api/v1/ai/ocr/test-connection`，返回引擎可用性 + 延迟。

#### 2.3.2 识别弹窗（`AiRecognizeDialog.vue`）

在图片/PDF 上传区顶部增加轻量下拉：

```
┌─ 图片/PDF 识别 ──────────────────────────────┐
│                                              │
│  识别引擎  [ 默认 ▾ ]   ← 轻量下拉           │
│             ├ 默认（跟随设置页）              │
│             ├ Doc2X（推荐，高精公式）         │
│             ├ MinerU（私有/自建）             │
│             └ Qwen-VL（兜底，最快）           │
│                                              │
│  ┌──────────────────────────────────────┐     │
│  │   点击或拖拽上传图片/PDF             │     │
│  └──────────────────────────────────────┘     │
└──────────────────────────────────────────────┘
```

- 弹窗内选择为「本次覆盖」，不持久化（持久化在设置页）。
- 默认值 = 用户在设置页的配置；未配置则 = `auto`。

---

### 2.4 关键细节增强（v1.1 新增）

> 本节集中定义 v1.1 补充的 4 项关键细节，是对前述两阶段流水线的强化与防护。

#### 2.4.1 配图提取逻辑（解决几何题丢图）

**问题**：现状审计（#20）发现 Prompt 虽输出 `image_placeholders[]`，但前端无占位符→原图绑定逻辑，几何题配图丢失。

**方案**：当 Stage 1（Doc2X / MinerU）输出的 Markdown 中包含图片链接（`![图](url)` 或 `![图](IMAGE_PLACEHOLDER_N)`）时，Stage 2（DeepSeek-V3）需：
1. 在结构化 JSON 中保留题干/解析内联位置的 Markdown 图片标记；
2. 将所有图片链接提取并去重，存入 `ParsedQuestion.image_urls: string[]` 数组；
3. 前端拿到 `image_urls` 后，对应内联位置渲染图片；未提供 URL 的占位符保留为待补图状态。

**Prompt 规则补充**（写入 `CORE_PARSE_RULES`）：
> 若输入 Markdown 含 `![...](url)` 图片标记，必须在 `image_urls` 数组中收集所有 URL（去重），并在 stem/analysis 对应位置保留该标记。不允许丢弃或改写为纯文本。

#### 2.4.2 Doc2X PDF 原生异步解析（避免前端切片）

**问题**：现状 PDF 路径在前端 `pdfToImages` 逐页渲染为图片，对每页分别调 `parse-image`，N 页触发 N 次后端调用，大 PDF 同步超时风险高、配额消耗 N 倍。

**方案**：对 PDF 输入，**不再前端切片**，直接调用 Doc2X 原生异步 PDF 解析 API：

```
前端上传 PDF（整体）
   ↓ POST /api/v1/ai/parse-pdf (multipart, ocr_provider=doc2x)
后端：
   ① Doc2XProvider::ocr_pdf_async(pdf_bytes)
      → POST Doc2X /v1/pdf 获取 task_id
      → 后端轮询 Doc2X 任务状态（间隔 3s，上限 120s）
      → 拿到全文 Markdown（含 LaTeX + 图片链接）
   ② Stage 2: DeepSeek-V3 结构化为 Vec<ParsedQuestion>
   ③ 返回 {data: [...], fallback_notice?: "..."}
```

- 仅 Doc2X / MinerU 走此路径（`supports_pdf()=true`）；Qwen-VL 不支持 PDF 原生，仍走前端逐页图片兜底。
- `OcrProvider` trait 扩展方法 `ocr_pdf_async(&self, pdf_bytes) -> Future<Output=Result<String, OcrError>>`（返回全文 Markdown，非分页数组）。

#### 2.4.3 OCR 引擎自动降级（Fallback）

**触发条件**：当用户指定的首选引擎发生以下错误时，自动降级到 `QwenVlOcrProvider` 兜底：
- HTTP 429（额度不足 / 限流）
- 401/403（Key 无效 / 无权限）
- 超时（`OcrError::Timeout`）
- 网络错误（`OcrError::Upstream`）

**实现**：在 `parse_image_v2` / `parse_pdf` handler 中包裹降级逻辑：

```rust
match primary_provider.ocr_image(&img).await {
    Ok(md) => md,
    Err(e) if should_fallback(&e) => {
        tracing::warn!("主引擎 {} 失败 {:?}，降级 qwen_vl", primary_id, e);
        fallback_notice = format!("{} 识别失败，已自动切换 Qwen-VL 兜底", primary_id);
        qwen_vl_provider.ocr_image(&img).await?   // 兜底
    }
    Err(e) => return Err(map_ocr_error(e)),
}
```

**响应增强**：`ParseImageResponse` / `ParsePdfResponse` 新增可选字段：

```json
{
  "data": [...],
  "fallback_notice": "doc2x 识别失败（429），已自动切换 Qwen-VL 兜底"  // 仅降级时存在
}
```

- 前端检测到 `fallback_notice` 时 `toast.warning` 提示用户。
- `auto` 模式下首选即 qwen_vl，不触发降级（已是兜底）。
- 用户在设置页可关闭「自动降级」开关（默认开）。

#### 2.4.4 大文本多题截断防护

**问题**：整卷多题 Markdown 体积大，Stage 2 LLM 输出可能被 `max_tokens` 截断，导致 JSON 不完整、`cleaner.rs` 解析失败。

**方案**：
1. **Stage 2 LLM 调用强制 `max_tokens >= 4096`**（DeepSeek-V3 上限内尽量大，建议 8192）。
2. **`cleaner.rs` 强化截断容错**：
   - 现有 `extract_json_by_bracket_count`（花括号栈匹配）作为主路径；
   - 新增**截断修复**：当检测到 JSON 末尾不完整（栈未归零 / 数组 `[` 未闭合），尝试：
     - 补全缺失的 `}` / `]` 闭合符；
     - 丢弃最后一个不完整的对象（如截断在 `,` 或 `{` 中间）；
     - 返回已成功解析的前 N-1 题，并在 `warnings` 标注 `"因长度截断，已丢弃第 N 题"`。
   - 对截断响应记录 `ai_usage_log.truncated=true` 用于监控。
3. **题数上限预警**：单次 Stage 2 输出 > 20 题时，前端提示用户分批上传。

---

## 3. 功能性需求

### 3.1 OCR 引擎抽象与实现

| 需求 ID | 描述 | 优先级 |
|---|---|---|
| FR-OCR-01 | 新增 `src/ai/ocr/` 模块，定义 `OcrProvider` trait | P0 |
| FR-OCR-02 | 实现 `QwenVlOcrProvider`（包装现有 `parse_image_with_prompt`，仅返回 Markdown） | P0 |
| FR-OCR-03 | 实现 `Doc2XProvider`（外部 API 调用，支持图片与 PDF 直传） | P1 |
| FR-OCR-04 | 实现 `MineruLocalProvider`（HTTP 调私有部署服务） | P2 |
| FR-OCR-05 | 实现 `MineruApiProvider`（MinerU Cloud） | P2 |
| FR-OCR-06 | `create_ocr_provider(cfg)` 工厂函数按配置返回 Box<dyn OcrProvider> | P0 |
| FR-OCR-07 | OCR 引擎错误类型 `OcrError`（UnsupportedPdf / Upstream / Timeout / NoApiKey） | P0 |
| FR-OCR-08 | `OcrProvider` trait 扩展 `ocr_pdf_async` 方法（Doc2X/MinerU 原生异步 PDF→全文 Markdown） | P1 |
| FR-OCR-09 | OCR 自动降级：`should_fallback(e)` 判定 + 首选失败降级到 QwenVlOcrProvider | P0 |

### 3.2 两阶段解析流水线

| 需求 ID | 描述 | 优先级 |
|---|---|---|
| FR-PIPE-01 | 新增 `parse_image_v2` handler：OcrProvider 出 Markdown → AiProvider 结构化 | P0 |
| FR-PIPE-02 | 复用现有 `cleaner.rs` / `kp_matcher.rs` 后处理 | P0 |
| FR-PIPE-03 | 保留旧 `parse_image` 端点作为兼容（灰度切换） | P0 |
| FR-PIPE-04 | 新增 `POST /api/v1/ai/ocr/test-connection` 测试引擎可用性 | P1 |
| FR-PIPE-05 | 批量多题切分：Doc2X/MinerU 输出多题 Markdown 时，由 Stage 2 切分为 `Vec<ParsedQuestion>` | P1 |
| FR-PIPE-06 | Stage 2 配图提取：Markdown 中 `![...](url)` 收集到 `ParsedQuestion.image_urls` 数组（v1.1） | P0 |
| FR-PIPE-07 | PDF 原生异步解析：`POST /api/v1/ai/parse-pdf` 调 Doc2X 异步 API + 后端轮询，不再前端切片（v1.1） | P1 |
| FR-PIPE-08 | 自动降级：首选引擎 429/401/403/超时/网络错误 → qwen_vl 兜底，响应带 `fallback_notice`（v1.1） | P0 |
| FR-PIPE-09 | Stage 2 `max_tokens≥4096` + `cleaner.rs` 截断容错（补全闭合符/丢弃末题/`truncated` 标记）（v1.1） | P0 |

### 3.3 用户配置管理

| 需求 ID | 描述 | 优先级 |
|---|---|---|
| FR-CFG-01 | `ai_settings` 表迁移：新增 `ocr_provider` / `doc2x_api_key_enc` / `mineru_api_endpoint` / `mineru_api_key_enc` | P0 |
| FR-CFG-02 | `resolve_ocr_config` 函数：用户优先 → 平台默认 → qwen_vl 兜底 | P0 |
| FR-CFG-03 | `GET/PUT /api/v1/ai/settings` 扩展返回/接收 OCR 字段 | P0 |
| FR-CFG-04 | OCR Key 复用 `AI_KEY_ENCRYPTION_KEY` AES 加密 | P0 |

### 3.4 前端引擎选择器

| 需求 ID | 描述 | 优先级 |
|---|---|---|
| FR-UI-01 | 设置页新增「OCR 模型设置」板块（引擎下拉 + 动态 Key/Endpoint 输入 + 测试连接） | P1 |
| FR-UI-02 | 识别弹窗顶部新增轻量引擎下拉（本次覆盖，不持久化） | P1 |
| FR-UI-03 | `aiApi.updateSettings` 扩展 OCR 字段 | P0 |
| FR-UI-04 | `aiApi.parseImage` 增加 `ocr_provider` 可选参数 | P0 |

### 3.5 既有能力保留（不破坏）

- ✅ Markdown 粘贴模式：**不强制改造**（保留前端 `parseMarkdownToQuestion`），但作为后续可选优化项
- ✅ 异步任务队列 `/ai/parse`：保留文本异步路径
- ✅ 知识点匹配、表单回填、断点续传、多题工作台：全部保留

---

## 4. 非功能性需求

| 类别 | 需求 |
|---|---|
| **安全** | NFR-SEC-01：吊销并轮换 `.env` 中泄露的 DeepSeek/Qwen Key；NFR-SEC-02：`.env` 加入 `.gitignore`，仅保留 `.env.example`；NFR-SEC-03：用户 OCR Key 一律 AES 加密入库，日志脱敏 |
| **性能** | NFR-PERF-01：OCR 调用超时 60s（可配置）；NFR-PERF-02：Stage 2 LLM 调用超时 120s（复用现有）且 `max_tokens≥4096`（v1.1）；NFR-PERF-03：PDF 单文件 ≤ 30 页（前端已限，后端校验）；NFR-PERF-04：Doc2X PDF 异步轮询间隔 3s、上限 120s（v1.1） |
| **可靠** | NFR-REL-01：OCR 引擎失败可降级到 qwen_vl 兜底（`auto` 模式）；NFR-REL-02：OCR Key 缺失时返回 `NoApiKey` 而非 panic；NFR-REL-03：首选引擎 429/401/403/超时/网络错误自动降级，响应带 `fallback_notice`（v1.1）；NFR-REL-04：LLM 输出截断时 cleaner 容错返回已解析前 N-1 题，不整体失败（v1.1） |
| **可观测** | NFR-OBS-01：每次 OCR 调用记录 `engine` / `latency_ms` / `success` 到 `ai_usage_log`；NFR-OBS-02：tracing 日志含引擎 ID 与 stage 标识；NFR-OBS-03：截断事件记录 `ai_usage_log.truncated=true` + 降级事件记录 `fallback_from`/`fallback_to`（v1.1） |
| **兼容** | NFR-COMP-01：旧 `parse_image` 端点保留至少 1 个版本周期；NFR-COMP-02：未配置 OCR 的用户走 qwen_vl，行为等价现状 |

---

## 5. 数据模型变更

### 5.1 `ai_settings` 表（扩展）

```sql
ALTER TABLE ai_settings
  ADD COLUMN ocr_provider         TEXT      DEFAULT 'auto',
  ADD COLUMN doc2x_api_key_enc    BYTEA,
  ADD COLUMN mineru_api_endpoint  TEXT,
  ADD COLUMN mineru_api_key_enc   BYTEA;
```

### 5.2 `ai_usage_log` 表（扩展，用于引擎记账）

```sql
ALTER TABLE ai_usage_log
  ADD COLUMN ocr_engine   TEXT,   -- 'doc2x' | 'mineru_local' | 'mineru_api' | 'qwen_vl'
  ADD COLUMN latency_ms   INTEGER,
  ADD COLUMN stage        TEXT,   -- 'ocr' | 'llm'
  ADD COLUMN truncated    BOOLEAN DEFAULT FALSE,           -- v1.1: Stage 2 输出是否截断
  ADD COLUMN fallback_from TEXT,                            -- v1.1: 降级前引擎（如 'doc2x'）
  ADD COLUMN fallback_to   TEXT;                           -- v1.1: 降级后引擎（如 'qwen_vl'）
```

### 5.3 `ParsedQuestion` 结构扩展（v1.1）

`ai/types.rs` 中的 `ParsedQuestion` 新增字段：

```rust
pub struct ParsedQuestion {
    // ... 现有字段（question_type / stem / options / correct_answer / analysis / ...）
    /// v1.1: 从 Markdown 中提取的所有图片 URL（去重）
    pub image_urls: Vec<String>,
}
```

- 前端 `client.ts` 的 `ParsedQuestion` 类型同步新增 `image_urls?: string[]`。
- 现有 `image_placeholders` 字段保留向后兼容；新逻辑优先用 `image_urls`。

---

## 6. 接口契约变更

### 6.1 扩展端点

| 方法 | 路径 | 变更 |
|---|---|---|
| GET | `/api/v1/ai/settings` | 响应新增 `ocr_provider` / `has_doc2x_key` / `mineru_endpoint` / `has_mineru_key` / `auto_fallback`（v1.1） |
| PUT | `/api/v1/ai/settings` | 请求体新增 `ocr_provider` / `doc2x_api_key` / `mineru_api_endpoint` / `mineru_api_key` / `auto_fallback`（明文，后端加密入库） |
| POST | `/api/v1/ai/parse-image` | 请求新增可选字段 `ocr_provider`（query 或 multipart field），覆盖用户默认；**响应新增可选 `fallback_notice`**（v1.1，降级时存在） |
| POST | `/api/v1/ai/parse-pdf` | **新增（v1.1）**，multipart 上传 PDF 整体 + `ocr_provider`，后端调 Doc2X 异步 API + 轮询 → 全文 Markdown → Stage 2 结构化 → `{data: [...], fallback_notice?}` |
| POST | `/api/v1/ai/ocr/test-connection` | **新增**，body: `{provider, api_key?, endpoint?}` → `{ok, latency_ms, message}` |

### 6.2 保留端点（不动）

- `POST /api/v1/ai/parse-text`
- `POST /api/v1/ai/parse` / `GET /api/v1/ai/parse/{id}`
- `POST /api/v1/questions/ai-tagging`

---

## 7. 前端 UI 变更清单

| 文件 | 变更 |
|---|---|
| `views/QuestionEdit.vue` | 无（仍持有弹窗） |
| `views/edit/components/AiRecognizeDialog.vue` | 上传区顶部加引擎下拉；`aiApi.parseImage` 调用透传 `ocr_provider` |
| 设置页（`AiSettings.vue` 或待新建） | 新增「OCR 模型设置」板块 + 测试连接按钮 |
| `api/client.ts` | `AiSettings` / `parseImage` 类型扩展 `ocr_provider`；新增 `testOcrConnection` 方法 |

---

## 8. 约束与边界

1. **不引入新的后端框架**（继续 Axum + tokio + reqwest + sqlx）。
2. **不引入新的前端框架**（继续 Vue 3 + scoped CSS + CSS 变量）。
3. **OCR 引擎接入优先级**：Doc2X > MinerU(local) > MinerU(api) > Qwen-VL（兜底）。
4. **本次仅做图片/PDF 路径的两阶段化**，文本路径（`parse_text`）保持现状。
5. **灰度策略**：新 `parse_image_v2` 与旧 `parse_image` 并存，前端通过 feature flag 切换，验证稳定后再下线旧端点。

---

## 9. 验收标准（摘要）

| 编号 | 验收项 |
|---|---|
| AC-01 | `.env` 已从 Git 历史清除，Key 已轮换，CI 校验 `.env` 不在版本库 |
| AC-02 | `src/ai/ocr/` 模块存在，`OcrProvider` trait + 4 个实现（含 QwenVlOcrProvider 兜底） |
| AC-03 | `POST /api/v1/ai/parse-image` 支持 `ocr_provider` 参数，两阶段流水线跑通 |
| AC-04 | 设置页可保存 OCR 配置，用户 Key 经 AES 加密入库（数据库查不到明文） |
| AC-05 | 识别弹窗引擎下拉可切换，选择 Doc2X 时调用 Doc2X 引擎 |
| AC-06 | `POST /api/v1/ai/ocr/test-connection` 返回引擎可用性与延迟 |
| AC-07 | 未配置 OCR 的用户走 qwen_vl，行为等价重构前（回归无差异） |
| AC-08 | `ai_usage_log` 记录引擎与延迟，可用于配额与监控 |
| AC-09 | Stage 2 提取 `image_urls`，几何题配图不丢失（v1.1） |
| AC-10 | PDF 走 `POST /ai/parse-pdf`，Doc2X 异步轮询，前端不切片（v1.1） |
| AC-11 | 首选引擎 429/401/403/超时自动降级 qwen_vl，响应带 `fallback_notice`，前端 toast 提示（v1.1） |
| AC-12 | Stage 2 `max_tokens≥4096`，截断时 cleaner 返回前 N-1 题 + `warnings`，不整体失败（v1.1） |
