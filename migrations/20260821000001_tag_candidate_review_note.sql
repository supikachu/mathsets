-- 候选审核备注：通过（含合并）与拒绝的原因落库，供详情回显与审计
ALTER TABLE tag_candidates
    ADD COLUMN IF NOT EXISTS review_note TEXT;
