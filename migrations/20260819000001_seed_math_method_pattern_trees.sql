-- =============================================================================
-- 题型专题树：将 math_method_*（kind=ability）定义为「专题技法」词表
-- =============================================================================
-- 语义约定（与 tags.category=method 拆分）：
--   * knowledge_trees.kind = ability + code = math_method_{high|junior}
--     → 题型专题 / 专题技法（如 凹凸反转、隐零点、极值点偏移）
--   * tags.category = method
--     → 通用解题方法 / 数学思想（如 数形结合、分类讨论、放缩法）
--
-- 存量注意：
--   若库中已有 math_method_* 且节点是旧「通用方法」内容，本迁移不会删除，
--   仅在树下追加「导数专题」分支（按 code 幂等）。请管理员事后审计旧节点，
--   merge 到 tags 或 status=deprecated，避免与专题叶子混用。
-- =============================================================================

-- 1. 确保全局树存在（已存在则只更新展示名 / 描述，不改 id）
INSERT INTO knowledge_trees (id, code, name, kind, space_id, description, is_active, created_at, updated_at)
SELECT
    'a0800000-0000-4000-8000-000000000001'::uuid,
    'math_method_high',
    '高中数学·题型专题',
    'ability',
    NULL,
    '高中数学题型专题 / 专题技法树（人教 A 版配套）。叶子节点用于精确筛选，如凹凸反转、隐零点。',
    TRUE,
    NOW(),
    NOW()
WHERE NOT EXISTS (
    SELECT 1 FROM knowledge_trees WHERE code = 'math_method_high' AND space_id IS NULL
);

UPDATE knowledge_trees
SET name = '高中数学·题型专题',
    description = '高中数学题型专题 / 专题技法树（人教 A 版配套）。叶子节点用于精确筛选，如凹凸反转、隐零点。',
    kind = 'ability',
    updated_at = NOW()
WHERE code = 'math_method_high' AND space_id IS NULL;

INSERT INTO knowledge_trees (id, code, name, kind, space_id, description, is_active, created_at, updated_at)
SELECT
    'a0800000-0000-4000-8000-000000000002'::uuid,
    'math_method_junior',
    '初中数学·题型专题',
    'ability',
    NULL,
    '初中数学题型专题 / 专题技法树（浙教版配套）。节点可后续补充。',
    TRUE,
    NOW(),
    NOW()
WHERE NOT EXISTS (
    SELECT 1 FROM knowledge_trees WHERE code = 'math_method_junior' AND space_id IS NULL
);

UPDATE knowledge_trees
SET name = '初中数学·题型专题',
    description = '初中数学题型专题 / 专题技法树（浙教版配套）。节点可后续补充。',
    kind = 'ability',
    updated_at = NOW()
WHERE code = 'math_method_junior' AND space_id IS NULL;

-- 2. 幂等写入高中导数专题节点（按 tree_id + code 去重）
DO $$
DECLARE
    tree_high UUID;
    d INT;
BEGIN
    SELECT id INTO tree_high
    FROM knowledge_trees
    WHERE code = 'math_method_high' AND space_id IS NULL
    LIMIT 1;

    IF tree_high IS NULL THEN
        RAISE EXCEPTION 'math_method_high 树未创建';
    END IF;

    -- helper: 仅当 (tree_id, code) 不存在时插入
    CREATE TEMP TABLE IF NOT EXISTS _pattern_seed (
        id UUID PRIMARY KEY,
        parent_code TEXT,
        code TEXT NOT NULL,
        path TEXT NOT NULL,
        depth SMALLINT NOT NULL,
        name TEXT NOT NULL,
        aliases JSONB NOT NULL,
        sort_order INT NOT NULL
    ) ON COMMIT DROP;

    TRUNCATE _pattern_seed;
    INSERT INTO _pattern_seed (id, parent_code, code, path, depth, name, aliases, sort_order) VALUES
    -- 根
    ('a0810000-0000-4000-8000-000000000001', NULL, 'deriv', 'deriv', 0,
     '导数专题', '[]'::jsonb, 10),

    -- 零点与方程根
    ('a0810000-0000-4000-8000-000000000010', 'deriv', 'zero', 'deriv.zero', 1,
     '零点与方程根', '[]'::jsonb, 10),
    ('a0810000-0000-4000-8000-000000000011', 'zero', 'hidden_zero', 'deriv.zero.hidden_zero', 2,
     '隐零点的应用', '[{"alias":"隐零点","locale":"zh"},{"alias":"隐零点应用","locale":"zh"}]'::jsonb, 10),
    ('a0810000-0000-4000-8000-000000000012', 'zero', 'exp_ln', 'deriv.zero.exp_ln', 2,
     'e^x 与 ln x 组合函数问题',
     '[{"alias":"对数单身狗指数找基友","locale":"zh"},{"alias":"指数对数组合函数","locale":"zh"}]'::jsonb, 20),
    ('a0810000-0000-4000-8000-000000000013', 'zero', 'zero_count', 'deriv.zero.zero_count', 2,
     '讨论函数零点或方程根的个数', '[{"alias":"零点个数","locale":"zh"}]'::jsonb, 30),
    ('a0810000-0000-4000-8000-000000000014', 'zero', 'zero_param', 'deriv.zero.zero_param', 2,
     '由零点个数求参数范围', '[{"alias":"零点个数求参","locale":"zh"}]'::jsonb, 40),

    -- 不等式证明（单变量）
    ('a0810000-0000-4000-8000-000000000020', 'deriv', 'ineq1', 'deriv.ineq1', 1,
     '不等式证明（单变量）', '[]'::jsonb, 20),
    ('a0810000-0000-4000-8000-000000000021', 'ineq1', 'virtual_zero', 'deriv.ineq1.virtual_zero', 2,
     '虚设零点', '[{"alias":"虚设零点法","locale":"zh"}]'::jsonb, 10),
    ('a0810000-0000-4000-8000-000000000022', 'ineq1', 'convex_flip', 'deriv.ineq1.convex_flip', 2,
     '凹凸反转', '[{"alias":"凹凸反转法","locale":"zh"}]'::jsonb, 20),
    ('a0810000-0000-4000-8000-000000000023', 'ineq1', 'tangent_scale', 'deriv.ineq1.tangent_scale', 2,
     '切线放缩', '[{"alias":"切线放缩法","locale":"zh"}]'::jsonb, 30),
    ('a0810000-0000-4000-8000-000000000024', 'ineq1', 'elim_param', 'deriv.ineq1.elim_param', 2,
     '合理消参', '[{"alias":"消参","locale":"zh"}]'::jsonb, 40),
    ('a0810000-0000-4000-8000-000000000025', 'ineq1', 'subst_ineq', 'deriv.ineq1.subst_ineq', 2,
     '不等式证明·换元法', '[{"alias":"换元证明不等式","locale":"zh"}]'::jsonb, 50),

    -- 不等式证明（双变量）
    ('a0810000-0000-4000-8000-000000000030', 'deriv', 'ineq2', 'deriv.ineq2', 1,
     '不等式证明（双变量）', '[]'::jsonb, 30),
    ('a0810000-0000-4000-8000-000000000031', 'ineq2', 'reduce_var', 'deriv.ineq2.reduce_var', 2,
     '消参减元法', '[{"alias":"消参减元","locale":"zh"}]'::jsonb, 10),

    -- 极值点偏移
    ('a0810000-0000-4000-8000-000000000040', 'deriv', 'shift', 'deriv.shift', 1,
     '极值点偏移', '[{"alias":"极值点偏移问题","locale":"zh"}]'::jsonb, 40),
    ('a0810000-0000-4000-8000-000000000041', 'shift', 'sum_type', 'deriv.shift.sum_type', 2,
     '极值点偏移·x1+x2 型', '[{"alias":"x1+x2型不等式","locale":"zh"}]'::jsonb, 10),
    ('a0810000-0000-4000-8000-000000000042', 'shift', 'prod_type', 'deriv.shift.prod_type', 2,
     '极值点偏移·x1·x2 型', '[{"alias":"x1x2型不等式","locale":"zh"},{"alias":"乘积型偏移","locale":"zh"}]'::jsonb, 20),

    -- 恒成立 / 参变分离
    ('a0810000-0000-4000-8000-000000000050', 'deriv', 'always', 'deriv.always', 1,
     '恒成立问题', '[]'::jsonb, 50),
    ('a0810000-0000-4000-8000-000000000051', 'always', 'sep_solvable', 'deriv.always.sep_solvable', 2,
     '参变分离（零点可求）', '[{"alias":"参变分离零点可求","locale":"zh"}]'::jsonb, 10),
    ('a0810000-0000-4000-8000-000000000052', 'always', 'sep_unsolvable', 'deriv.always.sep_unsolvable', 2,
     '参变分离（零点不可求）', '[{"alias":"参变分离零点不可求","locale":"zh"}]'::jsonb, 20),
    ('a0810000-0000-4000-8000-000000000053', 'always', 'sep_iso', 'deriv.always.sep_iso', 2,
     '同构或放缩后参变分离', '[{"alias":"同构后参变分离","locale":"zh"}]'::jsonb, 30),
    ('a0810000-0000-4000-8000-000000000054', 'always', 'minmax', 'deriv.always.minmax', 2,
     '最值分析法', '[{"alias":"最值分析","locale":"zh"}]'::jsonb, 40),
    ('a0810000-0000-4000-8000-000000000055', 'always', 'endpoint', 'deriv.always.endpoint', 2,
     '端点效应（非单调）', '[{"alias":"端点效应非单调","locale":"zh"}]'::jsonb, 50);

    -- 按 depth 分层插入，保证父节点先于子节点
    FOR d IN 0..2 LOOP
        INSERT INTO knowledge_nodes (
            id, tree_id, parent_id, code, path, depth, name, aliases,
            description, sort_order, question_count, is_active, status, source, created_at, updated_at
        )
        SELECT
            s.id,
            tree_high,
            p.id,
            s.code,
            s.path::ltree,
            s.depth,
            s.name,
            s.aliases,
            NULL,
            s.sort_order,
            0,
            TRUE,
            'active',
            'system',
            NOW(),
            NOW()
        FROM _pattern_seed s
        LEFT JOIN knowledge_nodes p
          ON p.tree_id = tree_high AND p.code = s.parent_code
        WHERE s.depth = d
          AND NOT EXISTS (
              SELECT 1 FROM knowledge_nodes e
              WHERE e.tree_id = tree_high AND e.code = s.code
          )
          AND NOT EXISTS (
              SELECT 1 FROM knowledge_nodes e2 WHERE e2.id = s.id
          )
          AND (s.parent_code IS NULL OR p.id IS NOT NULL);
    END LOOP;
END $$;
