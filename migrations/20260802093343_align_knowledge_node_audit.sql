BEGIN;

-- ============================================================
-- GAP-3: knowledge_nodes.question_count 维护触发器
-- 通过底层触发器自动维护每个知识点关联的题目数量
-- ============================================================
CREATE OR REPLACE FUNCTION sync_question_count() RETURNS TRIGGER AS $$
BEGIN
  IF (TG_OP = 'INSERT' OR TG_OP = 'DELETE') THEN
    UPDATE knowledge_nodes
    SET question_count = (
      SELECT COUNT(*) FROM question_knowledge_nodes
      WHERE node_id = COALESCE(NEW.node_id, OLD.node_id)
    )
    WHERE id = COALESCE(NEW.node_id, OLD.node_id);
  END IF;
  RETURN NULL;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_qkn_sync_count ON question_knowledge_nodes;
CREATE TRIGGER trg_qkn_sync_count
  AFTER INSERT OR DELETE ON question_knowledge_nodes
  FOR EACH ROW EXECUTE FUNCTION sync_question_count();

-- ============================================================
-- GAP-5: 清理 questions 表残留的 deprecated 列
-- ============================================================
ALTER TABLE questions
  DROP COLUMN IF EXISTS academic_year,
  DROP COLUMN IF EXISTS grade_semester,
  DROP COLUMN IF EXISTS exam_region,
  DROP COLUMN IF EXISTS grade;

COMMIT;
