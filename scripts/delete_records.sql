-- =============================================================================
-- Generic Record Deletion Script with Cascade Support
--
-- Features:
--   1. Delete specific IDs from specific tables with automatic foreign-key cascading
--   2. Supports 'questions', 'papers', 'users', 'knowledge_trees', 'knowledge_nodes',
--      'documents', 'ai_parse_tasks', 'question_collections', 'spaces', and any generic table
--   3. Safe transaction handling with DryRun mode
--
-- Usage via psql:
--   psql -U postgres -d mathset -f scripts/delete_records.sql
--
-- Usage via function:
--   SELECT * FROM mathset_delete_records('questions', ARRAY['c70d4482-df30-4a22-9aa5-482c9dd1cc39'::uuid], false); -- Execute
--   SELECT * FROM mathset_delete_records('questions', ARRAY['c70d4482-df30-4a22-9aa5-482c9dd1cc39'::uuid], true);  -- DryRun (preview)
-- =============================================================================

SET client_encoding = 'UTF8';
SET lc_messages = 'C';

CREATE OR REPLACE FUNCTION mathset_delete_records(
    p_table TEXT,
    p_ids UUID[],
    p_dry_run BOOLEAN DEFAULT false
) RETURNS TABLE (
    step_name TEXT,
    affected_table TEXT,
    deleted_count BIGINT
) AS $$
DECLARE
    v_table TEXT := lower(trim(p_table));
    v_count BIGINT;
BEGIN
    IF p_ids IS NULL OR array_length(p_ids, 1) = 0 THEN
        RAISE NOTICE 'No IDs provided. Nothing to delete.';
        RETURN;
    END IF;

    RAISE NOTICE '============================================================';
    RAISE NOTICE 'Target Table: % | IDs Count: % | Mode: %', 
        v_table, array_length(p_ids, 1), CASE WHEN p_dry_run THEN 'DRY-RUN (Preview)' ELSE 'EXECUTE' END;
    RAISE NOTICE '============================================================';

    -- =========================================================================
    -- 1. Table: questions
    -- =========================================================================
    IF v_table = 'questions' THEN
        -- ai_tagging_suggestions
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM ai_tagging_suggestions WHERE question_id = ANY(p_ids);
        ELSE
            WITH d AS (DELETE FROM ai_tagging_suggestions WHERE question_id = ANY(p_ids) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Tagging Suggestions'; affected_table := 'ai_tagging_suggestions'; deleted_count := v_count; RETURN NEXT;

        -- ai_tagging_tasks
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM ai_tagging_tasks WHERE question_id = ANY(p_ids);
        ELSE
            WITH d AS (DELETE FROM ai_tagging_tasks WHERE question_id = ANY(p_ids) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Tagging Tasks'; affected_table := 'ai_tagging_tasks'; deleted_count := v_count; RETURN NEXT;

        -- tag_candidates
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM tag_candidates WHERE source_question_id = ANY(p_ids);
        ELSE
            WITH d AS (DELETE FROM tag_candidates WHERE source_question_id = ANY(p_ids) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Tag Candidates'; affected_table := 'tag_candidates'; deleted_count := v_count; RETURN NEXT;

        -- public_library_submissions
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM public_library_submissions WHERE question_id = ANY(p_ids);
        ELSE
            WITH d AS (DELETE FROM public_library_submissions WHERE question_id = ANY(p_ids) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Public Submissions'; affected_table := 'public_library_submissions'; deleted_count := v_count; RETURN NEXT;

        -- review_records
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM review_records WHERE question_id = ANY(p_ids);
        ELSE
            WITH d AS (DELETE FROM review_records WHERE question_id = ANY(p_ids) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Review Records'; affected_table := 'review_records'; deleted_count := v_count; RETURN NEXT;

        -- question_reviewers
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM question_reviewers WHERE question_id = ANY(p_ids);
        ELSE
            WITH d AS (DELETE FROM question_reviewers WHERE question_id = ANY(p_ids) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Question Reviewers'; affected_table := 'question_reviewers'; deleted_count := v_count; RETURN NEXT;

        -- question_knowledge_nodes
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM question_knowledge_nodes WHERE question_id = ANY(p_ids);
        ELSE
            WITH d AS (DELETE FROM question_knowledge_nodes WHERE question_id = ANY(p_ids) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Knowledge Node Relations'; affected_table := 'question_knowledge_nodes'; deleted_count := v_count; RETURN NEXT;

        -- question_knowledge_points_deprecated
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM question_knowledge_points_deprecated WHERE question_id = ANY(p_ids);
        ELSE
            WITH d AS (DELETE FROM question_knowledge_points_deprecated WHERE question_id = ANY(p_ids) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Deprecated KP Relations'; affected_table := 'question_knowledge_points_deprecated'; deleted_count := v_count; RETURN NEXT;

        -- question_tags_relation
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM question_tags_relation WHERE question_id = ANY(p_ids);
        ELSE
            WITH d AS (DELETE FROM question_tags_relation WHERE question_id = ANY(p_ids) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Tag Relations'; affected_table := 'question_tags_relation'; deleted_count := v_count; RETURN NEXT;

        -- question_versions
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM question_versions WHERE question_id = ANY(p_ids);
        ELSE
            WITH d AS (DELETE FROM question_versions WHERE question_id = ANY(p_ids) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Question Versions'; affected_table := 'question_versions'; deleted_count := v_count; RETURN NEXT;

        -- collection_questions
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM collection_questions WHERE question_id = ANY(p_ids);
        ELSE
            WITH d AS (DELETE FROM collection_questions WHERE question_id = ANY(p_ids) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Collection Mappings'; affected_table := 'collection_questions'; deleted_count := v_count; RETURN NEXT;

        -- paper_questions
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM paper_questions WHERE question_id = ANY(p_ids);
        ELSE
            WITH d AS (DELETE FROM paper_questions WHERE question_id = ANY(p_ids) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Paper Mappings'; affected_table := 'paper_questions'; deleted_count := v_count; RETURN NEXT;

        -- ai_parse_tasks disassociation
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM ai_parse_tasks WHERE question_id = ANY(p_ids);
        ELSE
            WITH u AS (UPDATE ai_parse_tasks SET question_id = NULL WHERE question_id = ANY(p_ids) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM u;
        END IF;
        step_name := 'Disassociate AI Parse Tasks'; affected_table := 'ai_parse_tasks'; deleted_count := v_count; RETURN NEXT;

        -- self-referencing disassociation
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM questions WHERE parent_id = ANY(p_ids) OR origin_question_id = ANY(p_ids);
        ELSE
            WITH u AS (
                UPDATE questions 
                SET parent_id = CASE WHEN parent_id = ANY(p_ids) THEN NULL ELSE parent_id END,
                    origin_question_id = CASE WHEN origin_question_id = ANY(p_ids) THEN NULL ELSE origin_question_id END
                WHERE parent_id = ANY(p_ids) OR origin_question_id = ANY(p_ids)
                RETURNING *
            )
            SELECT COUNT(*) INTO v_count FROM u;
        END IF;
        step_name := 'Disassociate Parent/Origin Questions'; affected_table := 'questions'; deleted_count := v_count; RETURN NEXT;

        -- main questions
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM questions WHERE id = ANY(p_ids);
        ELSE
            WITH d AS (DELETE FROM questions WHERE id = ANY(p_ids) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Main Questions'; affected_table := 'questions'; deleted_count := v_count; RETURN NEXT;

    -- =========================================================================
    -- 2. Table: papers
    -- =========================================================================
    ELSIF v_table = 'papers' THEN
        -- paper_questions
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM paper_questions WHERE paper_id = ANY(p_ids);
        ELSE
            WITH d AS (DELETE FROM paper_questions WHERE paper_id = ANY(p_ids) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Paper Question Mappings'; affected_table := 'paper_questions'; deleted_count := v_count; RETURN NEXT;

        -- main papers
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM papers WHERE id = ANY(p_ids);
        ELSE
            WITH d AS (DELETE FROM papers WHERE id = ANY(p_ids) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Main Papers'; affected_table := 'papers'; deleted_count := v_count; RETURN NEXT;

    -- =========================================================================
    -- 3. Table: knowledge_trees
    -- =========================================================================
    ELSIF v_table = 'knowledge_trees' THEN
        -- question_knowledge_nodes
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM question_knowledge_nodes 
            WHERE node_id IN (SELECT id FROM knowledge_nodes WHERE tree_id = ANY(p_ids));
        ELSE
            WITH d AS (
                DELETE FROM question_knowledge_nodes 
                WHERE node_id IN (SELECT id FROM knowledge_nodes WHERE tree_id = ANY(p_ids))
                RETURNING *
            )
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Question Knowledge Node Relations'; affected_table := 'question_knowledge_nodes'; deleted_count := v_count; RETURN NEXT;

        -- knowledge_node_paths
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM knowledge_node_paths 
            WHERE ancestor_id IN (SELECT id FROM knowledge_nodes WHERE tree_id = ANY(p_ids))
               OR descendant_id IN (SELECT id FROM knowledge_nodes WHERE tree_id = ANY(p_ids));
        ELSE
            WITH d AS (
                DELETE FROM knowledge_node_paths 
                WHERE ancestor_id IN (SELECT id FROM knowledge_nodes WHERE tree_id = ANY(p_ids))
                   OR descendant_id IN (SELECT id FROM knowledge_nodes WHERE tree_id = ANY(p_ids))
                RETURNING *
            )
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Knowledge Node Paths'; affected_table := 'knowledge_node_paths'; deleted_count := v_count; RETURN NEXT;

        -- knowledge_nodes
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM knowledge_nodes WHERE tree_id = ANY(p_ids);
        ELSE
            WITH d AS (DELETE FROM knowledge_nodes WHERE tree_id = ANY(p_ids) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Knowledge Nodes'; affected_table := 'knowledge_nodes'; deleted_count := v_count; RETURN NEXT;

        -- main knowledge_trees
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM knowledge_trees WHERE id = ANY(p_ids);
        ELSE
            WITH d AS (DELETE FROM knowledge_trees WHERE id = ANY(p_ids) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Main Knowledge Trees'; affected_table := 'knowledge_trees'; deleted_count := v_count; RETURN NEXT;

    -- =========================================================================
    -- 4. Table: knowledge_nodes
    -- =========================================================================
    ELSIF v_table = 'knowledge_nodes' THEN
        -- Find all descendants recursively
        CREATE TEMP TABLE temp_node_descendants ON COMMIT DROP AS
        WITH RECURSIVE d_tree AS (
            SELECT id FROM knowledge_nodes WHERE id = ANY(p_ids)
            UNION ALL
            SELECT k.id FROM knowledge_nodes k JOIN d_tree dt ON k.parent_id = dt.id
        )
        SELECT DISTINCT id FROM d_tree;

        -- question_knowledge_nodes
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM question_knowledge_nodes 
            WHERE node_id IN (SELECT id FROM temp_node_descendants);
        ELSE
            WITH d AS (
                DELETE FROM question_knowledge_nodes 
                WHERE node_id IN (SELECT id FROM temp_node_descendants)
                RETURNING *
            )
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Question Knowledge Node Relations'; affected_table := 'question_knowledge_nodes'; deleted_count := v_count; RETURN NEXT;

        -- knowledge_node_paths
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM knowledge_node_paths 
            WHERE ancestor_id IN (SELECT id FROM temp_node_descendants)
               OR descendant_id IN (SELECT id FROM temp_node_descendants);
        ELSE
            WITH d AS (
                DELETE FROM knowledge_node_paths 
                WHERE ancestor_id IN (SELECT id FROM temp_node_descendants)
                   OR descendant_id IN (SELECT id FROM temp_node_descendants)
                RETURNING *
            )
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Knowledge Node Paths'; affected_table := 'knowledge_node_paths'; deleted_count := v_count; RETURN NEXT;

        -- knowledge_nodes
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM knowledge_nodes WHERE id IN (SELECT id FROM temp_node_descendants);
        ELSE
            WITH d AS (DELETE FROM knowledge_nodes WHERE id IN (SELECT id FROM temp_node_descendants) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Knowledge Nodes (including descendants)'; affected_table := 'knowledge_nodes'; deleted_count := v_count; RETURN NEXT;

    -- =========================================================================
    -- 5. Table: ai_parse_tasks
    -- =========================================================================
    ELSIF v_table = 'ai_parse_tasks' THEN
        -- tag_candidates
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM tag_candidates WHERE source_task_id = ANY(p_ids);
        ELSE
            WITH d AS (DELETE FROM tag_candidates WHERE source_task_id = ANY(p_ids) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Tag Candidates'; affected_table := 'tag_candidates'; deleted_count := v_count; RETURN NEXT;

        -- main ai_parse_tasks
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM ai_parse_tasks WHERE id = ANY(p_ids);
        ELSE
            WITH d AS (DELETE FROM ai_parse_tasks WHERE id = ANY(p_ids) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete AI Parse Tasks'; affected_table := 'ai_parse_tasks'; deleted_count := v_count; RETURN NEXT;

    -- =========================================================================
    -- 6. Table: question_collections
    -- =========================================================================
    ELSIF v_table = 'question_collections' THEN
        -- collection_questions
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM collection_questions WHERE collection_id = ANY(p_ids);
        ELSE
            WITH d AS (DELETE FROM collection_questions WHERE collection_id = ANY(p_ids) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Collection Question Mappings'; affected_table := 'collection_questions'; deleted_count := v_count; RETURN NEXT;

        -- main question_collections
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM question_collections WHERE id = ANY(p_ids);
        ELSE
            WITH d AS (DELETE FROM question_collections WHERE id = ANY(p_ids) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Question Collections'; affected_table := 'question_collections'; deleted_count := v_count; RETURN NEXT;

    -- =========================================================================
    -- 7. Table: documents
    -- =========================================================================
    ELSIF v_table = 'documents' THEN
        -- disassociate ai_parse_tasks
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM ai_parse_tasks WHERE document_id = ANY(p_ids);
        ELSE
            WITH u AS (UPDATE ai_parse_tasks SET document_id = NULL WHERE document_id = ANY(p_ids) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM u;
        END IF;
        step_name := 'Disassociate Tasks from Document'; affected_table := 'ai_parse_tasks'; deleted_count := v_count; RETURN NEXT;

        -- main documents
        IF p_dry_run THEN
            SELECT COUNT(*) INTO v_count FROM documents WHERE id = ANY(p_ids);
        ELSE
            WITH d AS (DELETE FROM documents WHERE id = ANY(p_ids) RETURNING *)
            SELECT COUNT(*) INTO v_count FROM d;
        END IF;
        step_name := 'Delete Documents'; affected_table := 'documents'; deleted_count := v_count; RETURN NEXT;

    -- =========================================================================
    -- 8. Generic Table Fallback
    -- =========================================================================
    ELSE
        -- Execute direct deletion on the given table
        IF p_dry_run THEN
            EXECUTE format('SELECT COUNT(*) FROM %I WHERE id = ANY($1)', v_table)
            INTO v_count
            USING p_ids;
        ELSE
            EXECUTE format('WITH d AS (DELETE FROM %I WHERE id = ANY($1) RETURNING *) SELECT COUNT(*) FROM d', v_table)
            INTO v_count
            USING p_ids;
        END IF;
        step_name := 'Delete from ' || v_table; affected_table := v_table; deleted_count := v_count; RETURN NEXT;
    END IF;

    RAISE NOTICE 'Finished processing table: %', v_table;
END;
$$ LANGUAGE plpgsql;

DO $$
BEGIN
    RAISE NOTICE 'mathset_delete_records function registered successfully.';
END $$;
