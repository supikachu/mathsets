# AI 异步解析任务 API

> 第四阶段功能：基于 Tokio 后台 Worker 的大模型（LLM）异步题目解析。前端提交 OCR 生肉文本 → 后端入队 → Worker 拾取并调用 LLM → 自动落库为新题目（草稿状态）。

## 概述

由于大模型（LLM）单次解析耗时通常在 5–30 秒以上，直接同步等待会导致 HTTP 超时与连接占用。本模块采用**异步任务队列**模式：

1. 前端 `POST /api/v1/ai/parse` 提交生肉文本，立即返回 `202 Accepted` 与 `task_id`
2. 前端使用 `task_id` 每 2 秒轮询 `GET /api/v1/ai/parse/:id`
3. 当任务状态变为 `completed` 时，响应中携带 `question_id`，前端跳转到题目编辑页继续修订

### 任务状态机

```
┌─────────┐  Worker 拾取  ┌────────────┐  LLM 成功  ┌────────────┐
│ pending │ ───────────▶ │ processing │ ────────▶ │ completed  │
└─────────┘              └────────────┘            └────────────┘
                                │  LLM 失败
                                ▼
                         ┌────────────┐
                         │   failed   │
                         └────────────┘
```

| 状态         | 说明                                       |
| ------------ | ------------------------------------------ |
| `pending`    | 已入队，等待 Worker 拾取                   |
| `processing` | Worker 已锁定任务，正在调用 LLM 解析       |
| `completed`  | 解析成功，`question_id` 字段已填入新题目 ID |
| `failed`     | 解析失败，`error_message` 字段记录错误详情 |

### 鉴权要求

所有接口均需要 JWT Bearer Token，且：

- **提交任务**：任意已登录用户均可提交
- **查询任务**：仅任务创建者本人或管理员（`is_admin`）可查询；其他用户查询返回 `404`（不泄露任务存在性）

---

## 接口列表

### 1. 提交 AI 解析任务

向队列中提交一段 OCR 生肉文本，Worker 会异步调用 LLM 将其解析为一道题目并落库。

#### 请求

```
POST /api/v1/ai/parse
```

##### Headers

| Header          | 必填 | 说明                |
| --------------- | ---- | ------------------- |
| `Authorization` | ✅   | `Bearer <JWT token>` |
| `Content-Type`  | ✅   | `application/json`  |

##### Request Body

| 字段       | 类型   | 必填 | 说明                         |
| ---------- | ------ | ---- | ---------------------------- |
| `raw_text` | string | ✅   | OCR 生肉文本（不可为空白） |

##### 请求示例

```bash
curl -X POST http://localhost:3000/api/v1/ai/parse \
  -H "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..." \
  -H "Content-Type: application/json" \
  -d '{
    "raw_text": "已知集合 A={1, 2, 3}, B={2, 3, 4}，求 A 并 B。解析：两个集合合并去重即可。答案：{1, 2, 3, 4}"
  }'
```

#### 响应

##### 成功响应

- **HTTP 状态码**：`202 Accepted`
- **响应体**：

| 字段         | 类型        | 说明                                          |
| ------------ | ----------- | --------------------------------------------- |
| `task_id`    | UUID string | 任务唯一标识，用于后续轮询                     |
| `status`     | string      | 任务状态，固定为 `"pending"`                   |
| `created_at` | ISO 8601    | 任务创建时间（UTC，如 `2026-07-19T08:30:00Z`）|

##### 响应示例

```json
{
  "task_id": "f3b8a1c2-9d4e-4f2a-8b6c-1e3a5d7f9b21",
  "status": "pending",
  "created_at": "2026-07-19T08:30:00.123456789Z"
}
```

##### 错误响应

| HTTP 状态码 | 错误场景               | 响应体示例                                       |
| ----------- | ---------------------- | ------------------------------------------------ |
| `400`       | `raw_text` 为空字符串  | `{"error": "raw_text 不能为空"}`                 |
| `401`       | 未携带 / 无效 JWT      | `{"error": "未认证"}`                            |
| `500`       | 数据库写入失败         | `{"error": "服务器内部错误，请稍后重试", "code": "ERR_INTERNAL_SERVER"}` |

---

### 2. 轮询查询任务状态

根据 `task_id` 查询任务的当前状态与结果。

#### 请求

```
GET /api/v1/ai/parse/:id
```

##### Headers

| Header          | 必填 | 说明                |
| --------------- | ---- | ------------------- |
| `Authorization` | ✅   | `Bearer <JWT token>` |

##### Path 参数

| 参数  | 类型        | 必填 | 说明                                  |
| ----- | ----------- | ---- | ------------------------------------- |
| `id`  | UUID string | ✅   | 任务 ID（由提交接口返回的 `task_id`） |

##### 请求示例

```bash
curl -X GET http://localhost:3000/api/v1/ai/parse/f3b8a1c2-9d4e-4f2a-8b6c-1e3a5d7f9b21 \
  -H "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..."
```

#### 响应

##### 成功响应

- **HTTP 状态码**：`200 OK`
- **响应体**：

| 字段            | 类型                | 说明                                                         |
| --------------- | ------------------- | ------------------------------------------------------------ |
| `id`            | UUID string         | 任务 ID                                                      |
| `status`        | string enum         | 任务状态：`"pending"` / `"processing"` / `"completed"` / `"failed"` |
| `question_id`   | UUID string \| null | 当 `status = "completed"` 时，填入生成的题目 ID；否则 `null` |
| `error_message` | string \| null      | 当 `status = "failed"` 时，记录失败原因；否则 `null`          |
| `created_at`    | ISO 8601            | 任务创建时间                                                 |
| `updated_at`    | ISO 8601            | 任务最后更新时间（状态流转时更新）                           |

##### 响应示例

**示例 1：任务正在解析中**

```json
{
  "id": "f3b8a1c2-9d4e-4f2a-8b6c-1e3a5d7f9b21",
  "status": "processing",
  "question_id": null,
  "error_message": null,
  "created_at": "2026-07-19T08:30:00.123456789Z",
  "updated_at": "2026-07-19T08:30:05.987654321Z"
}
```

**示例 2：任务已完成**

```json
{
  "id": "f3b8a1c2-9d4e-4f2a-8b6c-1e3a5d7f9b21",
  "status": "completed",
  "question_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "error_message": null,
  "created_at": "2026-07-19T08:30:00.123456789Z",
  "updated_at": "2026-07-19T08:30:18.456789012Z"
}
```

**示例 3：任务失败**

```json
{
  "id": "f3b8a1c2-9d4e-4f2a-8b6c-1e3a5d7f9b21",
  "status": "failed",
  "question_id": null,
  "error_message": "AI 上游错误 (HTTP 429): rate limit exceeded, please retry later...",
  "created_at": "2026-07-19T08:30:00.123456789Z",
  "updated_at": "2026-07-19T08:30:12.345678901Z"
}
```

##### 错误响应

| HTTP 状态码 | 错误场景                                  | 响应体示例                       |
| ----------- | ----------------------------------------- | --------------------------------- |
| `401`       | 未携带 / 无效 JWT                         | `{"error": "未认证"}`             |
| `404`       | 任务不存在，**或当前用户无权查看该任务** | `{"error": "任务不存在"}`         |
| `500`       | 数据库查询失败                            | `{"error": "服务器内部错误，请稍后重试", "code": "ERR_INTERNAL_SERVER"}` |

> **🔒 安全说明**：无权限访问他人任务时统一返回 `404` 而非 `403`，以避免泄露任务存在性（防止枚举攻击）。

---

## 前端集成建议

### 推荐的轮询策略

```typescript
async function pollAiParseTask(
  taskId: string,
  token: string,
  onUpdate: (status: string) => void,
  intervalMs = 2000,
  maxAttempts = 90, // 默认 3 分钟超时
): Promise<{ question_id?: string; error_message?: string }> {
  for (let i = 0; i < maxAttempts; i++) {
    const res = await fetch(`/api/v1/ai/parse/${taskId}`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    if (!res.ok) throw new Error(`查询失败: HTTP ${res.status}`);

    const data = await res.json();
    onUpdate(data.status);

    if (data.status === 'completed') {
      return { question_id: data.question_id };
    }
    if (data.status === 'failed') {
      return { error_message: data.error_message };
    }

    await new Promise((r) => setTimeout(r, intervalMs));
  }
  throw new Error('任务超时');
}
```

### 状态展示建议

| 状态         | UI 提示                       | 进度条建议         |
| ------------ | ----------------------------- | ------------------ |
| `pending`    | "任务已提交，正在排队..."     | 不确定进度（旋转） |
| `processing` | "AI 正在解析，请稍候..."      | 不确定进度（旋转）|
| `completed`  | "✅ 解析完成，跳转编辑页..."  | 100%               |
| `failed`     | "❌ 解析失败：<错误信息>"     | 0%（红色）         |

### 失败重试建议

当任务 `failed` 时，可引导用户：

1. 检查 `error_message` 判断失败原因
2. 常见原因：API Key 未配置、LLM 限流（429）、网络超时、生肉文本格式异常
3. 用户可调整文本后重新 `POST /api/v1/ai/parse` 提交新任务

---

## 字段类型对照表

| 字段            | Rust 类型                       | JSON 类型          | 说明                |
| --------------- | ------------------------------- | ------------------ | ------------------- |
| `task_id`       | `uuid::Uuid`                    | UUID string        | 任务 ID             |
| `status`        | `AiTaskStatus` (enum)           | lowercase string   | 任务状态枚举        |
| `question_id`   | `Option<uuid::Uuid>`            | UUID string \| null| 关联题目 ID         |
| `error_message` | `Option<String>`               | string \| null     | 错误详情            |
| `created_at`    | `chrono::DateTime<Utc>`         | ISO 8601 string    | 创建时间            |
| `updated_at`    | `chrono::DateTime<Utc>`         | ISO 8601 string    | 更新时间            |

## 相关代码位置

| 组件          | 文件路径                                                  |
| ------------- | --------------------------------------------------------- |
| 数据模型      | `src/models/ai_task.rs`                                   |
| Handler       | `src/handlers/ai_tasks.rs`                                |
| 后台 Worker   | `src/workers/ai_parse_worker.rs`                          |
| 数据库迁移    | `migrations/20260719000001_create_ai_parse_tasks.sql`     |
| 端到端测试    | `src/bin/test_ai_flow.rs`                                 |
| 路由注册      | `src/lib.rs`（`/ai/parse`、`/ai/parse/{id}`）             |
