-- =============================================================================
-- V2.1.1 P0：ai_task_status 枚举扩展
--
-- 本迁移只做 ADD VALUE（PG 12+ 允许在事务内执行，但同一事务中不得"使用"新值，
-- 因此本文件不包含任何引用新值的语句；后续迁移与运行时代码才使用）。
--
-- 状态机：pending → processing →（retrying ⇄ pending）→
--         success / partial_success / failed / cancelled
-- 历史 completed 值保留兼容，API 读出时映射为 success。
-- =============================================================================

ALTER TYPE ai_task_status ADD VALUE IF NOT EXISTS 'retrying';
ALTER TYPE ai_task_status ADD VALUE IF NOT EXISTS 'partial_success';
ALTER TYPE ai_task_status ADD VALUE IF NOT EXISTS 'cancelled';
ALTER TYPE ai_task_status ADD VALUE IF NOT EXISTS 'success';
