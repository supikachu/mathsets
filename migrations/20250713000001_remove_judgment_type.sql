-- 移除 question_type 枚举中的 judgment 值
-- 注意: 如果数据库中已有 judgment 类型的题目，需先迁移为其他类型

-- 将已有 judgment 题目改为 choice（防止删除枚举值时外键约束失败）
UPDATE questions SET question_type = 'choice' WHERE question_type = 'judgment';

-- 重建枚举类型（PostgreSQL 不支持直接 ALTER TYPE REMOVE VALUE）
ALTER TYPE question_type RENAME TO question_type_old;
CREATE TYPE question_type AS ENUM ('choice', 'fill', 'solution');
ALTER TABLE questions ALTER COLUMN question_type TYPE question_type USING question_type::text::question_type;
DROP TYPE question_type_old;
