-- ============================================================================
-- 削顶：移除知识树包装根节点（情况 A：真实顶级 Node）
-- ----------------------------------------------------------------------------
-- 背景：6 棵正式树（math_knowledge_* / physics_knowledge_* / math_method_* /
--       physics_method_*）存在"包装根"——每棵树唯一顶层节点且名字与树名
--       重复或为"XX综合库"（如"初中数学综合库"、"高中物理方法维度库"）。
-- 操作：
--   1. 备份整表（幂等：已存在则跳过）
--   2. 定位包装根（正式树 code 前缀 + 唯一顶层 + 有子节点）
--   3. 整棵子树（除根）提升一层：path 截掉根段、depth - 1、
--      直接子节点 parent_id 置 NULL
--   4. 删除包装根（FK parent_id 为 ON DELETE SET NULL，双保险；题目关联
--      不受影响——包装根无 question_knowledge_nodes 引用）
-- 安全性：单事务执行（sqlx 迁移默认事务）；章节树（多根）/测试树（单节点）
--         不受影响；脚本幂等可重跑。
-- ============================================================================

-- 1. 备份（幂等）
CREATE TABLE IF NOT EXISTS knowledge_nodes_bak_20260802 AS SELECT * FROM knowledge_nodes;

-- 2+3. 定位包装根并提升子树
-- 保险：仅全局树（space_id IS NULL）+ 名字特征（与树名相同或含"综合库"），
--       避免误伤管理员自建的同前缀空间树
WITH wrapper AS (
  SELECT kn.id, kn.tree_id, kn.path
  FROM knowledge_nodes kn
  JOIN knowledge_trees t ON t.id = kn.tree_id
  WHERE kn.parent_id IS NULL
    AND t.space_id IS NULL
    AND (kn.name = t.name OR kn.name LIKE '%综合库%')
    AND (t.code LIKE 'math_knowledge_%' OR t.code LIKE 'physics_knowledge_%'
      OR t.code LIKE 'math_method_%' OR t.code LIKE 'physics_method_%')
    AND (SELECT COUNT(*) FROM knowledge_nodes r WHERE r.tree_id = kn.tree_id AND r.parent_id IS NULL) = 1
    AND (SELECT COUNT(*) FROM knowledge_nodes c WHERE c.parent_id = kn.id) > 0
)
UPDATE knowledge_nodes d
SET parent_id = CASE WHEN d.parent_id = w.id THEN NULL ELSE d.parent_id END,
    path = subpath(d.path, 1, nlevel(d.path) - 1),
    depth = d.depth - 1
FROM wrapper w
WHERE d.id <> w.id AND d.path <@ w.path;

-- 4. 删除包装根：用备份表（削顶前快照）定位 id，避免"提升后树不再是
--    单根"导致条件失效；删除前断言 question_knowledge_nodes 零引用
--    （FK 为 ON DELETE CASCADE，防止未来环境有引用时静默级联删关联）；
--    parent_id FK 为 ON DELETE SET NULL 双保险；题目本身永不受影响。
-- 幂等说明：重跑时备份表保留削顶前快照 → UPDATE 0 行、DELETE 0 行（id 已删）。
DELETE FROM knowledge_nodes kn
WHERE kn.id IN (
  SELECT b.id FROM knowledge_nodes_bak_20260802 b
  JOIN knowledge_trees t ON t.id = b.tree_id
  WHERE b.parent_id IS NULL
    AND t.space_id IS NULL
    AND (b.name = t.name OR b.name LIKE '%综合库%')
    AND (t.code LIKE 'math_knowledge_%' OR t.code LIKE 'physics_knowledge_%'
      OR t.code LIKE 'math_method_%' OR t.code LIKE 'physics_method_%')
    AND (SELECT COUNT(*) FROM knowledge_nodes_bak_20260802 r
         WHERE r.tree_id = b.tree_id AND r.parent_id IS NULL) = 1
    AND (SELECT COUNT(*) FROM knowledge_nodes_bak_20260802 c
         WHERE c.parent_id = b.id) > 0
)
AND NOT EXISTS (SELECT 1 FROM question_knowledge_nodes q WHERE q.node_id = kn.id);
