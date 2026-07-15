-- Tag 系统重建：标签字典表 + 题目标签关联 + 结构化元数据列

-- 1. questions 表新增结构化元数据列（枚举型强控制字段，不走 tags 表）
ALTER TABLE questions ADD COLUMN IF NOT EXISTS academic_year VARCHAR(20);
ALTER TABLE questions ADD COLUMN IF NOT EXISTS grade_semester VARCHAR(20);
ALTER TABLE questions ADD COLUMN IF NOT EXISTS exam_type VARCHAR(20);
ALTER TABLE questions ADD COLUMN IF NOT EXISTS exam_region VARCHAR(50);

-- 2. 标签字典表（松散描述性标签：核心素养、解题方法、学校）
CREATE TABLE IF NOT EXISTS tags (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        VARCHAR(100) NOT NULL,
    category    VARCHAR(50) NOT NULL,  -- 'core_competence' | 'method' | 'school'
    space_id    UUID REFERENCES spaces(id) ON DELETE CASCADE,  -- NULL = 全局预置
    use_count   INTEGER NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 查询索引：按类别+名称检索
CREATE INDEX IF NOT EXISTS idx_tags_lookup ON tags(category, name);
-- 空间过滤索引
CREATE INDEX IF NOT EXISTS idx_tags_space ON tags(space_id);
-- 唯一约束：同空间内同类别标签名不重复（COALESCE 处理 NULL space_id）
-- PostgreSQL 不允许在 UNIQUE 约束中使用表达式，改用 partial unique index
CREATE UNIQUE INDEX IF NOT EXISTS idx_tags_unique_global
    ON tags (name, category) WHERE space_id IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_tags_unique_space
    ON tags (name, category, space_id) WHERE space_id IS NOT NULL;

-- 3. 题目 ↔ 标签多对多关联
CREATE TABLE IF NOT EXISTS question_tags_relation (
    question_id UUID NOT NULL REFERENCES questions(id) ON DELETE CASCADE,
    tag_id      UUID NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (question_id, tag_id)
);

CREATE INDEX IF NOT EXISTS idx_qtr_question ON question_tags_relation(question_id);
CREATE INDEX IF NOT EXISTS idx_qtr_tag ON question_tags_relation(tag_id);

-- 4. Seed: 核心素养（全局预置，space_id = NULL）
INSERT INTO tags (name, category, space_id) VALUES
    ('数学抽象', 'core_competence', NULL),
    ('逻辑推理', 'core_competence', NULL),
    ('数学建模', 'core_competence', NULL),
    ('直观想象', 'core_competence', NULL),
    ('数学运算', 'core_competence', NULL),
    ('数据分析', 'core_competence', NULL)
ON CONFLICT DO NOTHING;

-- 5. Seed: 解题方法（全局预置）
INSERT INTO tags (name, category, space_id) VALUES
    ('反证法', 'method', NULL),
    ('数学归纳法', 'method', NULL),
    ('枚举法', 'method', NULL),
    ('构造法', 'method', NULL),
    ('换元法', 'method', NULL),
    ('配方法', 'method', NULL),
    ('待定系数法', 'method', NULL),
    ('面积法', 'method', NULL),
    ('定义法', 'method', NULL),
    ('综合法', 'method', NULL),
    ('分析法', 'method', NULL)
ON CONFLICT DO NOTHING;

-- 6. Seed: 数学思想（全局预置，category 统一为 method）
INSERT INTO tags (name, category, space_id) VALUES
    ('数形结合', 'method', NULL),
    ('分类讨论', 'method', NULL),
    ('化归与转化', 'method', NULL),
    ('函数与方程', 'method', NULL),
    ('整体思想', 'method', NULL),
    ('极限思想', 'method', NULL),
    ('模型思想', 'method', NULL),
    ('统计思想', 'method', NULL),
    ('极值点偏移', 'method', NULL),
    ('隐零点', 'method', NULL),
    ('零点分段', 'method', NULL),
    ('放缩法', 'method', NULL),
    ('参变分离', 'method', NULL),
    ('齐次化', 'method', NULL),
    ('设而不求', 'method', NULL),
    ('韦达定理', 'method', NULL),
    ('判别式法', 'method', NULL),
    ('单调性分析', 'method', NULL),
    ('逆向分析', 'method', NULL),
    ('正向推导', 'method', NULL),
    ('穷举法', 'method', NULL),
    ('图形分析', 'method', NULL),
    ('代数变形', 'method', NULL),
    ('三角代换', 'method', NULL),
    ('向量法', 'method', NULL),
    ('坐标法', 'method', NULL)
ON CONFLICT DO NOTHING;
