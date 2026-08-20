-- 待审核候选按「归一化名 + 维度」全局去重：同一词不再随每道题各插一条。
-- 先清掉已有 pending 重复（保留最新），再加部分唯一索引。

DELETE FROM tag_candidates a
USING tag_candidates b
WHERE a.status = 'pending'
  AND b.status = 'pending'
  AND a.normalized_name = b.normalized_name
  AND a.kind = b.kind
  AND a.id < b.id;

CREATE UNIQUE INDEX IF NOT EXISTS idx_tag_candidates_pending_name_kind
    ON tag_candidates (normalized_name, kind)
    WHERE status = 'pending';
