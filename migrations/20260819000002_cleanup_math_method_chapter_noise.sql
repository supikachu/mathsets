-- =============================================================================
-- 清理 math_method_* 树中的章节/通用方法污染节点
-- =============================================================================
-- 背景：
--   math_method_* 现语义为「题型专题 / 专题技法」（kind=ability），
--   不应出现「第一章 集合」「数学思想方法」等章节树或通法内容。
--   通用方法应走 tags.category=method；章节应走 math_chapter_*。
--
-- 策略（幂等、不物理删除）：
--   1. 保留 path 以 deriv 开头的正式专题分支（导数专题及子节点）
--   2. 将其余 math_method_high / math_method_junior 节点标记 deprecated + is_active=false
--   3. 已关联题目的 question_knowledge_nodes 保留（便于事后审计/重标）
-- =============================================================================

UPDATE knowledge_nodes kn
SET
    is_active = FALSE,
    status = 'deprecated',
    updated_at = NOW()
FROM knowledge_trees kt
WHERE kn.tree_id = kt.id
  AND kt.code IN ('math_method_high', 'math_method_junior')
  AND kn.is_active = TRUE
  AND NOT (
      kn.path::text = 'deriv'
      OR kn.path::text LIKE 'deriv.%'
  );
