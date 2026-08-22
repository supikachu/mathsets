# 长 PDF 识别与打标改造 — 执行计划

> 生成日期：2026-08-21
> 状态：P0/P1 已落地（OCR 落库、按题号切块、两路 Stage2、异步打标）
> 关联文档：[智能打标签统一改造](智能打标签统一改造.md) | [AI智能录题_需求分析](AI智能录题_需求分析.md) | [AI录题与标签管理_V2.1.1_开发计划书_V1.1](AI录题与标签管理_V2.1.1_开发计划书_V1.1.md)

---

## 一、背景

长卷（尤其高考**解析卷**）走 PDF 直传快路径时，瓶颈不在 OCR，而在：

1. **Stage2 按约 6000 字切块**，一块里多道题 + 超长「法一～法六」，GLM 输出顶满 `max_tokens=8192` → JSON 截断 / 180s 超时。
2. **OCR Markdown 只在内存**，任务被僵尸回收或 Stage2 重跑会整本再 OCR。
3. **每题同步 `run_tagging`**，且把全文解析送进提取模型，22 题再乘几十次 LLM，堵住预览。

OCR 引擎（MinerU / Doc2X）适合整本；不要按页拆 OCR，也不要为打标再 OCR。

## 二、目标架构

```mermaid
flowchart LR
  pdf[整本PDF] --> ocr[整本OCR一次]
  ocr --> store[progress.ocr_markdown]
  store --> qsplit[按题号切块]
  qsplit --> s2["Stage2 每块1到2题 两路并发"]
  s2 --> preview[预览可编辑]
  preview --> tagq[排队异步打标]
  tagq --> suggest[回写 staged_questions]
```

约束（与打标统一改造一致）：建议可在解析后生成；**题目和候选仍只在用户确认保存后入库**。

## 三、阶段任务

### 阶段一：OCR 只做一次（P0）

| 编号 | 任务 | 文件 | 改动 |
|------|------|------|------|
| T1-1 | OCR 成功写入 `progress.ocr_markdown` / `ocr_engine` / `ocr_chars` | `src/workers/ai_parse_worker.rs` | jsonb 顶层合并，不新增表 |
| T1-2 | 重入跳过 OCR | 同上 `run_pdf_fast_path` | 已有非空 `ocr_markdown` 则直接 Stage2 |

验收：杀进程后同一任务重跑，日志出现「复用已落库 OCR」，不再打引擎上传。

### 阶段二：按题号切块（P0）

| 编号 | 任务 | 文件 | 改动 |
|------|------|------|------|
| T2-1 | `split_markdown_by_question_no` | `ai_parse_worker.rs` | 行首 `16.` / `16、` / `16．` / `第16题`；不切 `（1）` 小问 |
| T2-2 | 解析卷每块 1 题，普通卷最多 2 题 | 同上 | 标题含「解析」或 `【解析】`≥3 |
| T2-3 | 切不出题号则回退 6000 字 | 现有 `split_markdown_chunks` | 单块过长也回退硬切 |
| T2-4 | Prompt：本块只含所列题 | `src/ai/prompt.rs` | 不要补块外题号 |

验收：高考解析卷按题号成块；无题号 OCR 仍能切块；单测覆盖。

### 阶段三：Stage2 限流与瘦身（P1）

| 编号 | 任务 | 文件 | 改动 |
|------|------|------|------|
| T3-1 | 两路并发解析，串行暂存 | `run_pdf_fast_path` | 避免 jsonb 追加竞态 |
| T3-2 | 解析卷 slim：保题干/答案，analysis 可缩短 | `prompt.rs` | 预览不因超长解析整块失败 |
| T3-3 | 单块超时仍重试 1 次 | 现有逻辑 | 402 等 fatal 停止后续块 |

不提高整卷 `max_tokens`。前端 30 页限制只作用于逐页切图回退，本阶段不改。

### 阶段四：打标裁剪 + 异步（P1）

| 编号 | 任务 | 文件 | 改动 |
|------|------|------|------|
| T4-1 | 打标输入：题干+选项+答案+解析前 500 字 | `src/ai/tagging/engine.rs` | `tagging_content_from_parsed` |
| T4-2 | 解析 Worker 入队 `ai_tagging_tasks`，不同步 `run_tagging` | `stage_question` | 不另扣打标配额（已计入解析） |
| T4-3 | 任务行记录 `parse_task_id` + `source_index` | 新 migration | 打标完成后回写暂存 |
| T4-4 | 暂存 `tagging_status`: pending/done/failed | staged JSON | 预览可先出题 |
| T4-5 | 前端终态后继续短轮询，合并标签 | `useAiParsePolling.ts` / `AiRecognizeDialog.vue` | 不重置用户已改题干 |

### 阶段五：观测（P2）

日志：OCR 跳过、切块题数、每块耗时、截断挽救、打标排队。截断文案保持「输出过长被截断」，不引导裁图。

## 四、明确不做

- 按页拆 OCR
- 为打标再 OCR 一遍
- 整卷一次丢给 LLM 出 JSON
- 改日额度数值

## 五、风险与回滚

| 风险 | 处理 |
|------|------|
| OCR 题号丢失，切成 1 块 | 超长回退 6000 字 |
| `（1）` 被当成大题 | 只匹配 `N.` / `第N题` |
| 打标未完成用户就保存 | 编辑页仍可再打标；候选仍在确认保存时写 |
| jsonb 并发丢题 | 解析可并行，暂存必须串行 |

回滚：切块函数改回只走 `split_markdown_chunks`；打标改回 `stage_question` 内同步 `run_tagging`。
