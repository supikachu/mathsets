-- ============================================================
-- 推库申请表：解耦空间内部审核与公共题库终审
-- ============================================================

-- 推库申请状态
CREATE TYPE submission_status AS ENUM ('pending', 'approved', 'rejected');

-- 推库申请表
CREATE TABLE public_library_submissions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    question_id UUID NOT NULL REFERENCES questions(id) ON DELETE CASCADE,
    source_space_id UUID NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    submitted_by UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status submission_status NOT NULL DEFAULT 'pending',
    review_comment TEXT,
    reviewed_by UUID REFERENCES users(id),
    reviewed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    -- 防止同一题目存在多个 pending 申请（DEFERRABLE 允许事务内短暂冲突）
    -- 被拒绝后可重新申请（旧记录为 rejected，新记录为 pending，不冲突）
    UNIQUE (question_id) DEFERRABLE INITIALLY DEFERRED
);

-- 管理员按状态查询
CREATE INDEX idx_pls_status ON public_library_submissions(status);
-- 按来源空间查询
CREATE INDEX idx_pls_source_space ON public_library_submissions(source_space_id);
