-- no-transaction
-- GIN 索引：加速 metadata->'system_flags' 的 @> 包含查询（「待补全」筛选）
-- 查询必须用 @> 包含操作符命中此索引（->> 退化为 Seq Scan）
-- CONCURRENTLY 不锁表，可在生产环境直接执行；no-transaction 指令避免 sqlx 包入事务块
-- （no-transaction 迁移仅允许单条语句，故本文件只含此 CREATE INDEX）
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_questions_system_flags
  ON questions USING GIN ((metadata->'system_flags'));
