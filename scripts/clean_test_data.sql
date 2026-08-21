-- =============================================================================
-- 清理自动化测试残留数据（tests/* 直接使用 dev 库时产生）
--
-- 匹配规则：测试用户名格式 = 前缀 + 8 位 hex UUID 片段
--   test_ / doc_ / pb_ / pt_ / src_ / tg_ / wkr_ / leader_ / tgtsk_
-- 严格正则校验，防止误删真实账号。知识树按测试 code 前缀匹配（见 §7）。
--
-- 执行：psql "$DATABASE_URL" -f scripts/clean_test_data.sql
-- 幂等：重复执行不会误删真实数据（正则保证）。
-- =============================================================================

SET client_encoding = 'UTF8';
SET lc_messages = 'C';

BEGIN;

-- 测试用户集合（CTE 复用）
-- 依赖顺序删除：子表 → 关联表 → 容器 → 用户

-- 1. 标签候选（引用测试任务/测试题）
DELETE FROM tag_candidates
WHERE source_task_id IN (
    SELECT id FROM ai_parse_tasks WHERE creator_id IN (
        SELECT id FROM users WHERE username ~ '^(test_|doc_|pb_|pt_|src_|tg_|wkr_|leader_)[0-9a-f]{8}$')
)
   OR source_question_id IN (
    SELECT id FROM questions WHERE creator_id IN (
        SELECT id FROM users WHERE username ~ '^(test_|doc_|pb_|pt_|src_|tg_|wkr_|leader_)[0-9a-f]{8}$')
);

-- 2. 题目关联
DELETE FROM question_knowledge_nodes WHERE question_id IN (
    SELECT id FROM questions WHERE creator_id IN (
        SELECT id FROM users WHERE username ~ '^(test_|doc_|pb_|pt_|src_|tg_|wkr_|leader_)[0-9a-f]{8}$')
);
DELETE FROM question_tags_relation WHERE question_id IN (
    SELECT id FROM questions WHERE creator_id IN (
        SELECT id FROM users WHERE username ~ '^(test_|doc_|pb_|pt_|src_|tg_|wkr_|leader_)[0-9a-f]{8}$')
);
DELETE FROM question_versions WHERE question_id IN (
    SELECT id FROM questions WHERE creator_id IN (
        SELECT id FROM users WHERE username ~ '^(test_|doc_|pb_|pt_|src_|tg_|wkr_|leader_)[0-9a-f]{8}$')
);
DELETE FROM review_records WHERE question_id IN (
    SELECT id FROM questions WHERE creator_id IN (
        SELECT id FROM users WHERE username ~ '^(test_|doc_|pb_|pt_|src_|tg_|wkr_|leader_)[0-9a-f]{8}$')
);

-- 3. 容器关联
DELETE FROM collection_questions WHERE collection_id IN (
    SELECT id FROM question_collections WHERE creator_id IN (
        SELECT id FROM users WHERE username ~ '^(test_|doc_|pb_|pt_|src_|tg_|wkr_|leader_)[0-9a-f]{8}$')
);
DELETE FROM paper_questions WHERE paper_id IN (
    SELECT id FROM papers WHERE creator_id IN (
        SELECT id FROM users WHERE username ~ '^(test_|doc_|pb_|pt_|src_|tg_|wkr_|leader_)[0-9a-f]{8}$')
);

-- 4. 任务 / 题目 / 试卷 / 集合 / 文档
DELETE FROM ai_parse_tasks WHERE creator_id IN (
    SELECT id FROM users WHERE username ~ '^(test_|doc_|pb_|pt_|src_|tg_|wkr_|leader_)[0-9a-f]{8}$');
DELETE FROM questions WHERE creator_id IN (
    SELECT id FROM users WHERE username ~ '^(test_|doc_|pb_|pt_|src_|tg_|wkr_|leader_)[0-9a-f]{8}$');
DELETE FROM papers WHERE creator_id IN (
    SELECT id FROM users WHERE username ~ '^(test_|doc_|pb_|pt_|src_|tg_|wkr_|leader_)[0-9a-f]{8}$');
DELETE FROM question_collections WHERE creator_id IN (
    SELECT id FROM users WHERE username ~ '^(test_|doc_|pb_|pt_|src_|tg_|wkr_|leader_)[0-9a-f]{8}$');
DELETE FROM documents WHERE creator_id IN (
    SELECT id FROM users WHERE username ~ '^(test_|doc_|pb_|pt_|src_|tg_|wkr_|leader_)[0-9a-f]{8}$');

-- 5. 用户关联数据
DELETE FROM notifications WHERE user_id IN (
    SELECT id FROM users WHERE username ~ '^(test_|doc_|pb_|pt_|src_|tg_|wkr_|leader_)[0-9a-f]{8}$');
DELETE FROM ai_usage_log WHERE user_id IN (
    SELECT id FROM users WHERE username ~ '^(test_|doc_|pb_|pt_|src_|tg_|wkr_|leader_)[0-9a-f]{8}$');
DELETE FROM user_ai_settings WHERE user_id IN (
    SELECT id FROM users WHERE username ~ '^(test_|doc_|pb_|pt_|src_|tg_|wkr_|leader_)[0-9a-f]{8}$');
DELETE FROM public_library_submissions WHERE submitted_by IN (
    SELECT id FROM users WHERE username ~ '^(test_|doc_|pb_|pt_|src_|tg_|wkr_|leader_)[0-9a-f]{8}$')
   OR source_space_id IN (
    SELECT id FROM spaces WHERE owner_user_id IN (
        SELECT id FROM users WHERE username ~ '^(test_|doc_|pb_|pt_|src_|tg_|wkr_|leader_)[0-9a-f]{8}$')
);

-- 6. 空间（测试用户个人空间 + 成员关系）
DELETE FROM space_members WHERE user_id IN (
    SELECT id FROM users WHERE username ~ '^(test_|doc_|pb_|pt_|src_|tg_|wkr_|leader_)[0-9a-f]{8}$')
   OR space_id IN (
    SELECT id FROM spaces WHERE owner_user_id IN (
        SELECT id FROM users WHERE username ~ '^(test_|doc_|pb_|pt_|src_|tg_|wkr_|leader_)[0-9a-f]{8}$')
);
DELETE FROM spaces WHERE owner_user_id IN (
    SELECT id FROM users WHERE username ~ '^(test_|doc_|pb_|pt_|src_|tg_|wkr_|leader_)[0-9a-f]{8}$');

-- 7. 测试知识树（集成测试 code 前缀，级联删除节点与节点关联）
--    tk_  test_knowledge_points_crud（名称「测试知识树」）
--    lt_  test_question_full_lifecycle（名称「生命周期测试树」）
--    tp_  test_register_preserves_ability_tree_kind
--    tg_tree% / tg_tree2% / tg_{kind}_  tag_governance_api
--    e7_% / tgapi_%  ai_tagging_engine / ai_tagging_api
DELETE FROM knowledge_trees WHERE
    code LIKE 'tk_%'
    OR code LIKE 'lt_%'
    OR code LIKE 'tp_%'
    OR code LIKE 'tg_tree%'
    OR code LIKE 'tg_tree2%'
    OR code LIKE 'e7_%'
    OR code LIKE 'tgapi_%'
    OR code ~ '^tg_(knowledge|chapter|ability)_'
    OR code = 'tg_test_tree';

-- 8. 测试用户
DELETE FROM users WHERE username ~ '^(test_|doc_|pb_|pt_|src_|tg_|wkr_|leader_)[0-9a-f]{8}$';

COMMIT;
