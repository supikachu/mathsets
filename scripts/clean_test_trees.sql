-- 清理测试库里累积的知识树（仅用于 DATABASE_URL_TEST 指向的测试库）
--
-- tests/api.rs 的 test_register_preserves_ability_tree_kind 每跑一次就插入一棵全局
-- ability 树且从不清理，而注册流程会把所有全局树复制进新空间，于是树数量指数级膨胀
-- （实测一轮 api 测试后达到 12101 棵）。GET /api/v1/knowledge-trees 的响应随之超过
-- 测试辅助函数 to_bytes 的 1 MiB 上限，报 LengthLimitError。
--
-- knowledge_nodes.tree_id 外键为 ON DELETE CASCADE，删树会连带删节点。

BEGIN;

-- 注册时复制的空间副本：每个测试用户一份，全是残留
DELETE FROM knowledge_trees WHERE space_id IS NOT NULL;

-- 测试用例自己造的全局树
DELETE FROM knowledge_trees
 WHERE space_id IS NULL
   AND (name LIKE '%测试%' OR name LIKE '树-%'
        OR code ~ '^(tp|tg|tgapi|e7|tree|test)_');

SELECT (SELECT count(*) FROM knowledge_trees) AS trees_left,
       (SELECT count(*) FROM knowledge_trees WHERE space_id IS NULL) AS global_left,
       (SELECT count(*) FROM knowledge_nodes) AS nodes_left;

COMMIT;
