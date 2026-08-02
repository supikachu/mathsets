-- =============================================================================
-- 启用 pg_trgm 扩展（AI 智能打标三级模糊匹配依赖）
-- =============================================================================
-- 背景：
--   handlers::ai_tagging::match_knowledge_nodes 使用 pg_trgm 提供的：
--     1. similarity(text, text) 函数 — 计算 trigram 相似度（0.0-1.0）
--     2. % 操作符 — 相似度预过滤（基于 pg_trgm.similarity_threshold 会话参数）
--   用于 AI 返回知识点名称与 knowledge_nodes.name 的 fuzzy 匹配（第三级兜底）。
--
-- 注意：
--   - CREATE EXTENSION 通常需要数据库超级用户权限；若应用账号无权限，
--     需 DBA 手动执行本脚本一次。
--   - IF NOT EXISTS 保证幂等，可安全重复执行。
--   - pg_trgm 是可信扩展（trusted extension），安装后还会自动创建 GIN 索引支持。
-- =============================================================================

CREATE EXTENSION IF NOT EXISTS pg_trgm;
