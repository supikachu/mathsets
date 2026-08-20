-- =============================================================================
-- 恢复 math_method_* 章节解题方法结构，移除数学思想方法，导数专题挂入第三章
-- =============================================================================
-- 背景：
--   20260819000002 曾将非 deriv.* 节点全部 deprecated，导致题型专题 Tab 只剩导数专题根。
--   用户要求：
--     1. 恢复按章节组织的解题方法/题型内容
--     2. 永久移除「数学思想方法」及其子节点（通用方法改走 tags.category=method）
--     3. 「导数专题」作为第三章「导数及其应用」的子节点，而非独立根节点
-- =============================================================================

DO $$
DECLARE
    v_tree_id UUID;
    ch3_id UUID;
    ch3_path ltree;
BEGIN
    SELECT kt.id INTO v_tree_id
    FROM knowledge_trees kt
    WHERE kt.code = 'math_method_high' AND kt.space_id IS NULL
    LIMIT 1;

    IF v_tree_id IS NULL THEN
        RAISE NOTICE 'math_method_high 不存在，跳过';
        RETURN;
    END IF;

    -- 1. 删除「数学思想方法」整棵子树（含 question_knowledge_nodes 级联）
    WITH RECURSIVE thought_roots AS (
        SELECT kn.id
        FROM knowledge_nodes kn
        WHERE kn.tree_id = v_tree_id
          AND kn.name = '数学思想方法'
    ),
    thought_subtree AS (
        SELECT id FROM thought_roots
        UNION ALL
        SELECT kn.id
        FROM knowledge_nodes kn
        JOIN thought_subtree ts ON kn.parent_id = ts.id
    )
    DELETE FROM knowledge_nodes
    WHERE id IN (SELECT id FROM thought_subtree);

    -- 2. 恢复其余被 deprecated 的节点（章节 + 解题方法叶子）
    UPDATE knowledge_nodes kn
    SET is_active = TRUE,
        status = 'active',
        updated_at = NOW()
    WHERE kn.tree_id = v_tree_id
      AND kn.is_active = FALSE
      AND kn.status = 'deprecated';

    -- 3. 将导数专题挂入「第三章 导数及其应用」
    SELECT kn.id, kn.path
    INTO ch3_id, ch3_path
    FROM knowledge_nodes kn
    WHERE kn.tree_id = v_tree_id
      AND kn.depth = 0
      AND kn.name LIKE '第三章%导数%'
    LIMIT 1;

    IF ch3_id IS NULL THEN
        RAISE NOTICE '未找到第三章导数节点，导数专题保持原位置';
        RETURN;
    END IF;

    -- 3a. 根节点：导数专题
    UPDATE knowledge_nodes kn
    SET parent_id = ch3_id,
        depth = 1,
        path = ch3_path || 'deriv'::ltree,
        sort_order = GREATEST(kn.sort_order, 90),
        updated_at = NOW()
    WHERE kn.tree_id = v_tree_id
      AND kn.code = 'deriv'
      AND kn.path::text = 'deriv';

    -- 3b. 子节点：depth +1，path 前缀替换
    UPDATE knowledge_nodes kn
    SET depth = kn.depth + 1,
        path = ch3_path || kn.path,
        updated_at = NOW()
    WHERE kn.tree_id = v_tree_id
      AND kn.path::text LIKE 'deriv.%';
END $$;

-- 初中树：同样删除数学思想方法；若有 deprecated 非思想节点则恢复
DO $$
DECLARE
    v_tree_id UUID;
BEGIN
    SELECT kt.id INTO v_tree_id
    FROM knowledge_trees kt
    WHERE kt.code = 'math_method_junior' AND kt.space_id IS NULL
    LIMIT 1;

    IF v_tree_id IS NULL THEN
        RETURN;
    END IF;

    WITH RECURSIVE thought_roots AS (
        SELECT kn.id
        FROM knowledge_nodes kn
        WHERE kn.tree_id = v_tree_id
          AND kn.name = '数学思想方法'
    ),
    thought_subtree AS (
        SELECT id FROM thought_roots
        UNION ALL
        SELECT kn.id
        FROM knowledge_nodes kn
        JOIN thought_subtree ts ON kn.parent_id = ts.id
    )
    DELETE FROM knowledge_nodes
    WHERE id IN (SELECT id FROM thought_subtree);

    UPDATE knowledge_nodes kn
    SET is_active = TRUE,
        status = 'active',
        updated_at = NOW()
    WHERE kn.tree_id = v_tree_id
      AND kn.is_active = FALSE
      AND kn.status = 'deprecated';
END $$;
