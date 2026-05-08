-- 暂时允许 creator_id 为 NULL（接入 JWT 认证前）
ALTER TABLE questions ALTER COLUMN creator_id DROP NOT NULL;
ALTER TABLE questions ALTER COLUMN updated_by DROP NOT NULL;

-- 审核记录也临时放宽
ALTER TABLE review_records ALTER COLUMN reviewer_id DROP NOT NULL;
