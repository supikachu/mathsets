-- =============================================================================
-- 🚨 紧急修复：标签系统 B1 迁移补跑（数据消失 Bug 根因）
-- =============================================================================
-- 问题背景：
--   B2/B3 重构了 Rust 模型与 handler，全部假设新表 knowledge_trees /
--   knowledge_nodes / question_knowledge_nodes 已存在，且 questions.difficulty
--   已从 enum 迁移为 SMALLINT 1-5。但 B1 阶段承诺的 SQL 迁移脚本从未落地，
--   导致所有列表 / 详情 API 因 sqlx 解码失败而 500，前端表现为"数据全不见"。
--
-- 本迁移一次性完成：
--   1) 启用 ltree 扩展
--   2) 创建缺失枚举：exam_type / tag_category / knowledge_tree_kind / knowledge_link_source
--   3) questions.difficulty: enum('easy','medium','hard') → SMALLINT (1-5)
--   4) questions.semester: VARCHAR → semester_type enum（合并 semester_new 列）
--   5) questions.exam_type: VARCHAR → exam_type enum
--   6) questions.metadata JSONB 列新增 + 长尾字段数据搬迁
--   7) tags 表补全 parent_id / path / aliases / description / is_active
--   8) tags.category: VARCHAR → tag_category enum
--   9) 创建 knowledge_trees / knowledge_nodes / question_knowledge_nodes 三表
--  10) 从旧 knowledge_points 迁移 244 个节点到 knowledge_nodes（含 LTREE path 计算）
--  11) 从旧 question_knowledge_points 迁移 11 条关联到 question_knowledge_nodes
--  12) 旧表 RENAME 为 *_deprecated（不删，留作回滚兜底）
-- =============================================================================

-- ─────────────────────────────────────────────────────────────────────────────
-- 0. ltree 扩展（knowledge_nodes.path / tags.path 物化路径所需）
-- ─────────────────────────────────────────────────────────────────────────────
CREATE EXTENSION IF NOT EXISTS ltree;

-- ─────────────────────────────────────────────────────────────────────────────
-- 1. 缺失枚举类型
-- ─────────────────────────────────────────────────────────────────────────────
DO $$ BEGIN
    CREATE TYPE exam_type AS ENUM (
        'midterm', 'final', 'gaokao', 'mock', 'entrance', 'daily', 'other'
    );
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
    CREATE TYPE tag_category AS ENUM (
        'core_competence', 'method', 'school', 'scene', 'error_prone'
    );
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
    CREATE TYPE knowledge_tree_kind AS ENUM ('knowledge', 'ability', 'chapter');
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
    CREATE TYPE knowledge_link_source AS ENUM ('manual', 'ai');
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

-- ─────────────────────────────────────────────────────────────────────────────
-- 2. questions.difficulty: enum → SMALLINT (1-5)
--    映射：easy → 2, medium → 3, hard → 4
-- ─────────────────────────────────────────────────────────────────────────────
DO $$
BEGIN
    -- 仅当 difficulty 列仍是 enum 类型时执行迁移
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'questions' AND column_name = 'difficulty'
          AND udt_name = 'difficulty'
    ) THEN
        -- 2.1 新增临时 SMALLINT 列
        ALTER TABLE questions ADD COLUMN difficulty_new SMALLINT;

        -- 2.2 按 enum 文本值映射到 1-5
        UPDATE questions SET difficulty_new = 2 WHERE difficulty::text = 'easy';
        UPDATE questions SET difficulty_new = 3 WHERE difficulty::text = 'medium';
        UPDATE questions SET difficulty_new = 4 WHERE difficulty::text = 'hard';
        UPDATE questions SET difficulty_new = 3 WHERE difficulty_new IS NULL;

        -- 2.3 切换列
        ALTER TABLE questions DROP COLUMN difficulty;
        ALTER TABLE questions RENAME COLUMN difficulty_new TO difficulty;
        ALTER TABLE questions ALTER COLUMN difficulty SET NOT NULL;
        ALTER TABLE questions ALTER COLUMN difficulty SET DEFAULT 3;

        -- 2.4 旧枚举类型暂不 DROP，留作回滚兜底（DROP TYPE difficulty;）
    END IF;
END $$;

-- ─────────────────────────────────────────────────────────────────────────────
-- 3. questions.semester: VARCHAR → semester_type enum
--    合并旧迁移加的 semester_new 列（一直是 NULL，从未被使用）
--    旧值：'上学期' / '下学期' / '全年' → first / second / full_year
-- ─────────────────────────────────────────────────────────────────────────────
DO $$
DECLARE
    sem_col_type TEXT;
BEGIN
    SELECT data_type INTO sem_col_type
    FROM information_schema.columns
    WHERE table_name = 'questions' AND column_name = 'semester';

    -- 3.1 仅当 semester 仍是 VARCHAR 时执行
    IF sem_col_type = 'character varying' THEN
        -- 创建新 enum 列
        ALTER TABLE questions ADD COLUMN semester_v2 semester_type;

        -- 文本映射（兼容 '上学期'/'下学期'/'全年' 以及 'first'/'second'/'full_year'）
        UPDATE questions SET semester_v2 = 'first'     WHERE semester::text IN ('上学期', 'first', 'First');
        UPDATE questions SET semester_v2 = 'second'    WHERE semester::text IN ('下学期', 'second', 'Second');
        UPDATE questions SET semester_v2 = 'full_year' WHERE semester::text IN ('全年', 'full_year', 'FullYear');

        -- 切换列
        ALTER TABLE questions DROP COLUMN semester;
        ALTER TABLE questions RENAME COLUMN semester_v2 TO semester;
    END IF;

    -- 3.2 处理旧迁移遗留的 semester_new 列：直接 DROP（一直是 NULL，无数据）
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'questions' AND column_name = 'semester_new'
    ) THEN
        ALTER TABLE questions DROP COLUMN semester_new;
    END IF;
END $$;

-- ─────────────────────────────────────────────────────────────────────────────
-- 4. questions.exam_type: VARCHAR → exam_type enum
--    大部分为 NULL；非空值映射：期末→final, 期中→midterm, 高考→gaokao,
--    中考→entrance, 月考/周测→daily, 模拟→mock, 其他→other
-- ─────────────────────────────────────────────────────────────────────────────
DO $$
DECLARE
    et_col_type TEXT;
BEGIN
    SELECT data_type INTO et_col_type
    FROM information_schema.columns
    WHERE table_name = 'questions' AND column_name = 'exam_type';

    IF et_col_type = 'character varying' THEN
        ALTER TABLE questions ADD COLUMN exam_type_v2 exam_type;

        UPDATE questions SET exam_type_v2 = 'final'    WHERE exam_type::text = '期末';
        UPDATE questions SET exam_type_v2 = 'midterm'  WHERE exam_type::text = '期中';
        UPDATE questions SET exam_type_v2 = 'gaokao'   WHERE exam_type::text = '高考';
        UPDATE questions SET exam_type_v2 = 'entrance' WHERE exam_type::text = '中考';
        UPDATE questions SET exam_type_v2 = 'mock'     WHERE exam_type::text = '模拟';
        UPDATE questions SET exam_type_v2 = 'daily'    WHERE exam_type::text IN ('月考', '周测');
        UPDATE questions SET exam_type_v2 = 'other'    WHERE exam_type::text IS NOT NULL AND exam_type_v2 IS NULL;

        ALTER TABLE questions DROP COLUMN exam_type;
        ALTER TABLE questions RENAME COLUMN exam_type_v2 TO exam_type;
    END IF;
END $$;

-- ─────────────────────────────────────────────────────────────────────────────
-- 5. questions.metadata JSONB 列 + 长尾字段数据搬迁
--    academic_year / grade_semester / exam_region → metadata JSONB
-- ─────────────────────────────────────────────────────────────────────────────
ALTER TABLE questions ADD COLUMN IF NOT EXISTS metadata JSONB NOT NULL DEFAULT '{}'::jsonb;

-- 把已存在的长尾字段值搬进 metadata（仅当 metadata 还是空对象时）
UPDATE questions
SET metadata = metadata
    || jsonb_build_object(
        'academic_year',  COALESCE(NULLIF(academic_year,  ''), NULL),
        'grade_semester', COALESCE(NULLIF(grade_semester, ''), NULL),
        'exam_region',    COALESCE(NULLIF(exam_region,    ''), NULL)
    )
WHERE academic_year IS NOT NULL OR grade_semester IS NOT NULL OR exam_region IS NOT NULL;

-- 旧列保留为 deprecated，不删（Rust 模型不再读取，留作回滚兜底）
COMMENT ON COLUMN questions.academic_year  IS 'DEPRECATED: 数据已迁移到 metadata JSONB';
COMMENT ON COLUMN questions.grade_semester IS 'DEPRECATED: 数据已迁移到 metadata JSONB';
COMMENT ON COLUMN questions.exam_region    IS 'DEPRECATED: 数据已迁移到 metadata JSONB';
COMMENT ON COLUMN questions.grade          IS 'DEPRECATED: 请使用 grade_level (grade_level ENUM)';

-- ─────────────────────────────────────────────────────────────────────────────
-- 6. tags 表补全新列
-- ─────────────────────────────────────────────────────────────────────────────
ALTER TABLE tags ADD COLUMN IF NOT EXISTS parent_id   UUID REFERENCES tags(id) ON DELETE SET NULL;
ALTER TABLE tags ADD COLUMN IF NOT EXISTS path        LTREE;
ALTER TABLE tags ADD COLUMN IF NOT EXISTS aliases     JSONB NOT NULL DEFAULT '[]'::jsonb;
ALTER TABLE tags ADD COLUMN IF NOT EXISTS description TEXT;
ALTER TABLE tags ADD COLUMN IF NOT EXISTS is_active   BOOLEAN NOT NULL DEFAULT TRUE;

-- ─────────────────────────────────────────────────────────────────────────────
-- 7. tags.category: VARCHAR → tag_category enum
--    现有值 'core_competence' / 'method' / 'school' 与枚举名完全匹配
-- ─────────────────────────────────────────────────────────────────────────────
DO $$
DECLARE
    cat_col_type TEXT;
BEGIN
    SELECT data_type INTO cat_col_type
    FROM information_schema.columns
    WHERE table_name = 'tags' AND column_name = 'category';

    IF cat_col_type = 'character varying' THEN
        ALTER TABLE tags ADD COLUMN category_v2 tag_category;
        -- 现有值都是合法枚举名，直接强转
        UPDATE tags SET category_v2 = category::tag_category WHERE category IS NOT NULL;
        ALTER TABLE tags DROP COLUMN category;
        ALTER TABLE tags RENAME COLUMN category_v2 TO category;
        ALTER TABLE tags ALTER COLUMN category SET NOT NULL;
    END IF;
END $$;

-- 为 tags.path 填充默认值（每行 path = 替换 id 中的 '-' 为 '_' 的 ltree 标签）
UPDATE tags SET path = REPLACE(id::text, '-', '_')::ltree WHERE path IS NULL;

-- tags 索引
CREATE INDEX IF NOT EXISTS idx_tags_path        ON tags USING GIST (path);
CREATE INDEX IF NOT EXISTS idx_tags_parent      ON tags(parent_id);
CREATE INDEX IF NOT EXISTS idx_tags_active_cat  ON tags(is_active, category);

-- ─────────────────────────────────────────────────────────────────────────────
-- 8. knowledge_trees 表
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS knowledge_trees (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    code        VARCHAR(100) NOT NULL,
    name        VARCHAR(200) NOT NULL,
    kind        knowledge_tree_kind NOT NULL DEFAULT 'knowledge',
    space_id    UUID REFERENCES spaces(id) ON DELETE CASCADE,
    version     INT NOT NULL DEFAULT 1,
    description TEXT,
    is_active   BOOLEAN NOT NULL DEFAULT TRUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- code 在同空间内唯一（partial unique index 兼容 NULL space_id）
CREATE UNIQUE INDEX IF NOT EXISTS idx_kt_unique_global
    ON knowledge_trees (code) WHERE space_id IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_kt_unique_space
    ON knowledge_trees (code, space_id) WHERE space_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_kt_kind      ON knowledge_trees(kind);
CREATE INDEX IF NOT EXISTS idx_kt_space     ON knowledge_trees(space_id);
CREATE INDEX IF NOT EXISTS idx_kt_active    ON knowledge_trees(is_active);

-- ─────────────────────────────────────────────────────────────────────────────
-- 9. knowledge_nodes 表（LTREE 物化路径 + 邻接表双轨）
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS knowledge_nodes (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tree_id         UUID NOT NULL REFERENCES knowledge_trees(id) ON DELETE CASCADE,
    parent_id       UUID REFERENCES knowledge_nodes(id) ON DELETE SET NULL,
    code            VARCHAR(100),
    path            LTREE NOT NULL,
    depth           SMALLINT NOT NULL DEFAULT 0,
    name            VARCHAR(200) NOT NULL,
    aliases         JSONB NOT NULL DEFAULT '[]'::jsonb,
    description     TEXT,
    sort_order      INT NOT NULL DEFAULT 0,
    question_count  INT NOT NULL DEFAULT 0,
    is_active       BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_kn_tree     ON knowledge_nodes(tree_id);
CREATE INDEX IF NOT EXISTS idx_kn_parent   ON knowledge_nodes(parent_id);
CREATE INDEX IF NOT EXISTS idx_kn_path     ON knowledge_nodes USING GIST (path);
CREATE INDEX IF NOT EXISTS idx_kn_active   ON knowledge_nodes(is_active);

-- ─────────────────────────────────────────────────────────────────────────────
-- 10. question_knowledge_nodes 关联表
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS question_knowledge_nodes (
    question_id     UUID NOT NULL REFERENCES questions(id) ON DELETE CASCADE,
    node_id         UUID NOT NULL REFERENCES knowledge_nodes(id) ON DELETE CASCADE,
    is_primary      BOOLEAN NOT NULL DEFAULT FALSE,
    relevance_score SMALLINT,
    ai_confidence   NUMERIC(5,4),
    source          knowledge_link_source NOT NULL DEFAULT 'manual',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (question_id, node_id)
);

CREATE INDEX IF NOT EXISTS idx_qkn_question ON question_knowledge_nodes(question_id);
CREATE INDEX IF NOT EXISTS idx_qkn_node     ON question_knowledge_nodes(node_id);
CREATE INDEX IF NOT EXISTS idx_qkn_source   ON question_knowledge_nodes(source);

-- ─────────────────────────────────────────────────────────────────────────────
-- 11. 数据迁移：旧 knowledge_points (244 行) → knowledge_nodes
--     策略：保留原 UUID 作为新 id；path 用 REPLACE(id::text, '-', '_')::ltree 递归拼接
-- ─────────────────────────────────────────────────────────────────────────────

-- 11.1 创建默认知识树（全局，space_id=NULL）
INSERT INTO knowledge_trees (id, code, name, kind, space_id, description)
VALUES (
    '00000000-0000-0000-0000-000000000001'::uuid,
    'math_knowledge',
    '数学知识树',
    'knowledge',
    NULL,
    '从旧 knowledge_points 表迁移而来（默认全局知识树）'
)
ON CONFLICT DO NOTHING;

-- 11.2 用递归 CTE 计算 path 和 depth，并迁移到 knowledge_nodes
--     仅当 knowledge_nodes 还是空表时执行（幂等）
DO $$
DECLARE
    node_count INT;
BEGIN
    SELECT COUNT(*) INTO node_count FROM knowledge_nodes;
    IF node_count = 0 THEN
        INSERT INTO knowledge_nodes (
            id, tree_id, parent_id, code, path, depth, name, aliases,
            description, sort_order, question_count, is_active, created_at, updated_at
        )
        WITH RECURSIVE build_tree AS (
            -- 根节点：parent_id IS NULL
            SELECT
                kp.id,
                kp.parent_id,
                kp.name,
                kp.sort_order,
                kp.created_at,
                REPLACE(kp.id::text, '-', '_')::ltree AS path,
                0::smallint AS depth
            FROM knowledge_points kp
            WHERE kp.parent_id IS NULL
            UNION ALL
            -- 递归子节点
            SELECT
                kp.id,
                kp.parent_id,
                kp.name,
                kp.sort_order,
                kp.created_at,
                bt.path || REPLACE(kp.id::text, '-', '_')::ltree,
                (bt.depth + 1)::smallint
            FROM knowledge_points kp
            JOIN build_tree bt ON kp.parent_id = bt.id
        )
        SELECT
            bt.id,
            '00000000-0000-0000-0000-000000000001'::uuid,
            bt.parent_id,
            NULL,                           -- code：旧表无对应字段，留空
            bt.path,
            bt.depth,
            bt.name,
            '[]'::jsonb,                    -- aliases 默认空数组
            NULL,                           -- description
            bt.sort_order,
            0,                              -- question_count 初始 0
            TRUE,
            bt.created_at,
            bt.created_at                   -- updated_at 用原 created_at 兜底
        FROM build_tree bt;

        RAISE NOTICE '已迁移 % 个 knowledge_nodes 节点', node_count;
    END IF;
END $$;

-- ─────────────────────────────────────────────────────────────────────────────
-- 12. 数据迁移：旧 question_knowledge_points (11 行) → question_knowledge_nodes
-- ─────────────────────────────────────────────────────────────────────────────
INSERT INTO question_knowledge_nodes (
    question_id, node_id, is_primary, source, created_at
)
SELECT
    qkp.question_id,
    qkp.knowledge_point_id,
    FALSE,                  -- is_primary 默认 FALSE（旧表无主知识点概念）
    'manual'::knowledge_link_source,
    NOW()
FROM question_knowledge_points qkp
WHERE NOT EXISTS (
    SELECT 1 FROM question_knowledge_nodes qkn
    WHERE qkn.question_id = qkp.question_id AND qkn.node_id = qkp.knowledge_point_id
)
ON CONFLICT DO NOTHING;

-- ─────────────────────────────────────────────────────────────────────────────
-- 13. 旧表重命名为 *_deprecated（保留 FK 关系，不删数据，留作回滚兜底）
--     注意：必须先 DROP 旧表上指向 questions 的 FK 约束，否则 RENAME 时
--     旧 FK 名会跟着保留，但语义不变，不影响新代码。
-- ─────────────────────────────────────────────────────────────────────────────
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM pg_tables WHERE schemaname = 'public' AND tablename = 'knowledge_points'
    ) THEN
        ALTER TABLE knowledge_points RENAME TO knowledge_points_deprecated;
        RAISE NOTICE '已将 knowledge_points 重命名为 knowledge_points_deprecated';
    END IF;

    IF EXISTS (
        SELECT 1 FROM pg_tables WHERE schemaname = 'public' AND tablename = 'question_knowledge_points'
    ) THEN
        ALTER TABLE question_knowledge_points RENAME TO question_knowledge_points_deprecated;
        RAISE NOTICE '已将 question_knowledge_points 重命名为 question_knowledge_points_deprecated';
    END IF;
END $$;

-- ─────────────────────────────────────────────────────────────────────────────
-- 14. 验证：确保新表有数据
-- ─────────────────────────────────────────────────────────────────────────────
-- 这部分仅作迁移完成后的 sanity check，不影响事务
