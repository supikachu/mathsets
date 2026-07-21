-- =============================================================================
-- 清理 knowledge_nodes 脏数据（AI 测试阶段残留）
-- =============================================================================
-- 纯净标准 SQL，无 psql 专属语法，sqlx migrate!() 可直接执行。
-- 幂等设计：DELETE 语句天然幂等，重复执行不会报错。
--
-- 安全特性：
--   1. 全程包裹在 BEGIN/COMMIT 事务中，出错即 ROLLBACK
--   2. ON DELETE CASCADE 自动清理 question_knowledge_nodes 关联
--   3. 递归 CTE 处理子树，避免父节点删除后子节点变孤儿
-- =============================================================================

BEGIN;

-- -----------------------------------------------------------------------------
-- Step 1: 递归删除所有"测试"节点及其子孙
--   起始条件：name ILIKE '%测试%'
--   递归条件：parent_id 指向已标记为脏数据的节点
-- -----------------------------------------------------------------------------

WITH RECURSIVE dirty_nodes AS (
    SELECT id FROM knowledge_nodes WHERE name ILIKE '%测试%'
    UNION ALL
    SELECT kn.id
    FROM knowledge_nodes kn
    JOIN dirty_nodes dn ON kn.parent_id = dn.id
)
DELETE FROM knowledge_nodes
WHERE id IN (SELECT id FROM dirty_nodes);

-- -----------------------------------------------------------------------------
-- Step 2: 删除同树 + 同父下同名重复节点（保留 created_at 最早的）
--   PARTITION BY tree_id, parent_id, name → 同一棵树、同一个父节点下同名
--   ORDER BY created_at ASC → 保留最早创建的（rn = 1），删除其余
-- -----------------------------------------------------------------------------

WITH duplicates AS (
    SELECT id,
           ROW_NUMBER() OVER (
               PARTITION BY tree_id, parent_id, name
               ORDER BY created_at ASC
           ) AS rn
    FROM knowledge_nodes
)
DELETE FROM knowledge_nodes
WHERE id IN (SELECT id FROM duplicates WHERE rn > 1);

COMMIT;
