-- =============================================================================
-- Cleanup Script: Delete questions created by 'visualtest' in the last 7 days,
-- and delete all papers and paper_questions mappings.
--
-- Usage:
--   psql -U postgres -d mathset -f scripts/clean_visualtest_questions_and_all_papers.sql
--   or PowerShell: .\scripts\clean_visualtest_questions_and_all_papers.ps1
-- =============================================================================

SET client_encoding = 'UTF8';
SET lc_messages = 'C';

BEGIN;

-- 1. Create temp table caching target question IDs
CREATE TEMP TABLE temp_target_questions ON COMMIT DROP AS
SELECT q.id
FROM questions q
JOIN users u ON q.creator_id = u.id
WHERE u.username = 'visualtest'
  AND q.created_at >= NOW() - INTERVAL '7 days';

-- 2. Output statistics before deletion
DO $$
DECLARE
    target_q_count INT;
    paper_count INT;
    pq_count INT;
BEGIN
    SELECT COUNT(*) INTO target_q_count FROM temp_target_questions;
    SELECT COUNT(*) INTO paper_count FROM papers;
    SELECT COUNT(*) INTO pq_count FROM paper_questions;
    
    RAISE NOTICE '------------------------------------------------------------';
    RAISE NOTICE 'Target questions created by visualtest (7d): %', target_q_count;
    RAISE NOTICE 'Total papers to delete: %', paper_count;
    RAISE NOTICE 'Total paper_questions mappings to delete: %', pq_count;
    RAISE NOTICE '------------------------------------------------------------';
END $$;

-- 3. Delete all papers and paper-question relations
DELETE FROM paper_questions;
DELETE FROM papers;

-- 4. Delete foreign key relations referencing target questions
DELETE FROM ai_tagging_suggestions
WHERE question_id IN (SELECT id FROM temp_target_questions);

DELETE FROM ai_tagging_tasks
WHERE question_id IN (SELECT id FROM temp_target_questions);

DELETE FROM tag_candidates
WHERE source_question_id IN (SELECT id FROM temp_target_questions);

DELETE FROM public_library_submissions
WHERE question_id IN (SELECT id FROM temp_target_questions);

DELETE FROM review_records
WHERE question_id IN (SELECT id FROM temp_target_questions);

DELETE FROM question_reviewers
WHERE question_id IN (SELECT id FROM temp_target_questions);

DELETE FROM question_knowledge_nodes
WHERE question_id IN (SELECT id FROM temp_target_questions);

DELETE FROM question_knowledge_points_deprecated
WHERE question_id IN (SELECT id FROM temp_target_questions);

DELETE FROM question_tags_relation
WHERE question_id IN (SELECT id FROM temp_target_questions);

DELETE FROM question_versions
WHERE question_id IN (SELECT id FROM temp_target_questions);

DELETE FROM collection_questions
WHERE question_id IN (SELECT id FROM temp_target_questions);

-- Disassociate questions from AI parse tasks
UPDATE ai_parse_tasks
SET question_id = NULL
WHERE question_id IN (SELECT id FROM temp_target_questions);

-- Disassociate self-referencing question relations
UPDATE questions
SET parent_id = NULL
WHERE parent_id IN (SELECT id FROM temp_target_questions);

UPDATE questions
SET origin_question_id = NULL
WHERE origin_question_id IN (SELECT id FROM temp_target_questions);

-- 5. Delete target questions
DELETE FROM questions
WHERE id IN (SELECT id FROM temp_target_questions);

-- 6. Output cleanup result summary
DO $$
DECLARE
    remaining_q INT;
    remaining_p INT;
BEGIN
    SELECT COUNT(*) INTO remaining_q FROM questions;
    SELECT COUNT(*) INTO remaining_p FROM papers;
    
    RAISE NOTICE '------------------------------------------------------------';
    RAISE NOTICE 'Cleanup completed successfully!';
    RAISE NOTICE 'Remaining questions in database: %', remaining_q;
    RAISE NOTICE 'Remaining papers in database: %', remaining_p;
    RAISE NOTICE '------------------------------------------------------------';
END $$;

COMMIT;


