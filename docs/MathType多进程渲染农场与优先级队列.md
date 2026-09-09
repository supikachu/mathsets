# MathType 多进程渲染农场 + 优先级队列（实施方案）

> 目标：在**不改 MT6.dll**、**暂不做全局 hash 去重**的前提下，用「一进程一 DLL + 负载均衡 + 队列优先级」提高 WMF 吞吐，缓解整卷 AI 录入后公式转换排队过久的问题。  
> 关联：`docs/MathType公式资产接入.md`、`docs/（Linux 主系统 + Windows MathType 渲染微服务）实施计划书.md`

---

## 1. 背景与约束

| 现状 | 原因 |
|------|------|
| 单卷公式转换约 5–6 分钟 | 单 `--serve` + Worker 每次只转 1 题（防 MT6 AV） |
| 同机多开未隔离的 serve 易 `access violation @ 0x10` | MT6 / 剪贴板全局状态互抢 |

**硬约束：**

1. **同进程内 MT6 必须串行**（每个 `mt_converter --serve` 内并发上限 = 1）。
2. **扩展靠多进程 / 多机**：一进程一 DLL 实例，互不共享堆。
3. **优先 Direct `MT6.dll` 路径**；多进程下 Clipboard fallback 易互抢，生产应避免依赖。
4. **本期不做**全局 LaTeX/MTEF hash 缓存（后续可加）。

---

## 2. 目标架构

```text
题库 Formula Worker / sync / 导出提权
              │
              ▼
     优先级队列（question_math_assets.priority）
     high(100): 导出 MathType 急需
     mid (10):  手工保存 / 提交审核
     low  (0):  AI 落库仅登记
              │
              ▼
     调度：并行度 ≈ 渲染进程数 N
              │
     ┌────────┼────────┐
     ▼        ▼        ▼
  :8091     :8092     :8093   …  mt_converter --serve
  (MT6#1)   (MT6#2)   (MT6#3)
     \        |        /
      可选 Nginx :8090 轮询
```

OLE（`MathTypeOle.Cli`）仍可在主站侧车执行；本期重点扩展 **WMF `/convert_batch`**。

---

## 3. 阶段 A — 多进程 + 负载均衡

### 3.1 Windows 农场启动

脚本：`tools/mt_converter_portable/scripts/start-farm.ps1`

- 默认端口 `8091..8094`（可用参数改数量）。
- 每个端口一个独立 `mt_converter.exe --serve` 进程。
- 启动前建议结束残留的 `mt_converter`（避免无主控的双 serve）。

### 3.2 环境变量

| 变量 | 说明 |
|------|------|
| `MATHTYPE_CONVERT_URL` | 兼容旧配置：单个 base URL |
| `MATHTYPE_CONVERT_URLS` | 逗号分隔多 URL，例 `http://127.0.0.1:8091,http://127.0.0.1:8092`；**优先于**单 URL |
| `MATHTYPE_WORKER_PARALLEL` | 可选；默认 = URL 个数，Formula Worker 每轮并行题目数上限 |

### 3.3 Rust 客户端

- `MathTypeConvertConfig` 持有 `convert_urls: Vec<String>`。
- `convert_batch`：对 URL 列表 **round-robin**；每个 URL 用 **Semaphore(1)** 保证进程内对该实例串行。
- Formula Worker：每轮 `LIMIT = parallel`，`join` 并行 `sync_question_math_assets`（不同题 → 不同/轮询实例）。

### 3.4 验收

1. `start-farm.ps1` 后四个 `/health` 均 ok。  
2. 配置 `MATHTYPE_CONVERT_URLS` 后重启题库。  
3. 整卷 pending 时日志出现多题交错同步；总墙钟时间相对单进程明显下降（理想接近 ÷N，受 OLE/CPU 限制）。

---

## 4. 阶段 B — 队列与优先级

### 4.1 数据

表 `question_math_assets` 增加：

- `priority INT NOT NULL DEFAULT 0`

拉取顺序：

```sql
ORDER BY MAX(priority) DESC, MIN(updated_at) ASC
```

（按 `question_id` 聚合 pending/stale 行。）

### 4.2 优先级约定

| 场景 | priority |
|------|----------|
| AI 确认落库（仅登记槽位） | `0` |
| 手工 create/update 触发 sync、提交审核 | `10` |
| Word 导出 `docx_math=mathtype` 且存在未 ready | `100`（导出前 bump） |

### 4.3 Worker

- 有 pending 时尽快取最高优先级的最多 N 题并行转。
- 空闲再短 sleep（避免空转打满 CPU）；不再固定「永远 15s 才转 1 题」。

### 4.4 验收

1. AI 大批量落库后，用户点 MathType 导出 → 该卷题目优先被 Worker 消化。  
2. 低优先级长尾不阻塞高优先级。

---

## 5. 阶段 C（后续，本期不做）

- 探针 convert + AV 后从池中摘除 URL。  
- Nginx 统一入口。  
- 全局 hash 去重 / Redis 缓存。  
- 多机 Windows 农场。

---

## 6. 运维备忘

```powershell
# 清残留后启农场
Get-Process mt_converter,MathType,MathTypeLib -ErrorAction SilentlyContinue | Stop-Process -Force
cd tools\mt_converter_portable
.\scripts\start-farm.ps1 -Count 4
```

`.env` 示例：

```env
MATHTYPE_CONVERT_URLS=http://127.0.0.1:8091,http://127.0.0.1:8092,http://127.0.0.1:8093,http://127.0.0.1:8094
MATHTYPE_OLE_CLI=C:/Users/.../MathTypeOle.Cli.exe
```

任务管理器：每个 serve 常见 **父子各一** PyInstaller 进程（正常）；异常是 **两组互不隶属的农场**。

---

## 7. 实现对照

| 阶段 | 状态 |
|------|------|
| 本文档 | 已建立 |
| A：farm 脚本 + 多 URL + Worker 并行 | 已实现 |
| B：priority 列 + 导出/提交提权 | 已实现（migration `20260909000004`） |
| C：熔断 / hash | 未做 |
