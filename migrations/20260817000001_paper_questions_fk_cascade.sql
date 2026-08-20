-- paper_questions.question_id 外键改为 ON DELETE CASCADE
--
-- 背景：该外键此前为 NO ACTION，导致 delete_question / gc_abandoned_ai_drafts
-- 删除带试卷关联的题目时触发外键冲突（500 / GC 跳过）。改为 CASCADE 后与
-- collection_questions / question_knowledge_nodes 的删除行为对齐
-- （开发计划书 §五 设计意图：题目删除时集合/试卷关联自动清理）。
ALTER TABLE paper_questions DROP CONSTRAINT paper_questions_question_id_fkey;
ALTER TABLE paper_questions
  ADD CONSTRAINT paper_questions_question_id_fkey
  FOREIGN KEY (question_id) REFERENCES questions(id) ON DELETE CASCADE;
