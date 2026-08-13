-- AI 用量日志表扩展（v1.1，两阶段流水线监控）
-- 为 ai_usage_log 追加 OCR 引擎审计字段，支持 M2/M4 多引擎切换与降级追踪。
-- M1 仅写入 endpoint，其余字段为预留（nullable / default），不影响现有 INSERT。

ALTER TABLE ai_usage_log
    ADD COLUMN IF NOT EXISTS ocr_engine    TEXT,      -- Stage 1 OCR 引擎标识：qwen_vl / doc2x / mineru_local / mineru_api
    ADD COLUMN IF NOT EXISTS latency_ms    INTEGER,   -- 端到端耗时（毫秒）
    ADD COLUMN IF NOT EXISTS stage         TEXT,      -- 流水线阶段标记：stage1_ocr / stage2_parse / full
    ADD COLUMN IF NOT EXISTS truncated     BOOLEAN DEFAULT FALSE,  -- Stage 2 输出是否被 max_tokens 截断
    ADD COLUMN IF NOT EXISTS fallback_from TEXT,      -- 降级前引擎（如 doc2x → qwen_vl 时记录 doc2x）
    ADD COLUMN IF NOT EXISTS fallback_to   TEXT;      -- 降级后引擎（如 qwen_vl）
