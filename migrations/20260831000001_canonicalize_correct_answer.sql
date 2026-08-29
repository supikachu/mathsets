-- 存量 correct_answer 收到 {kind,value}。解答题顶层答案清为 JSON null。
-- 已是规范对象（含 kind）的行不改。畸形对象跳过。

UPDATE questions
SET correct_answer = jsonb_build_object(
    'kind', 'choice',
    'value', jsonb_build_object('options', correct_answer)
)
WHERE question_type IN ('choice', 'multiple')
  AND correct_answer IS NOT NULL
  AND jsonb_typeof(correct_answer) = 'array';

UPDATE questions
SET correct_answer = jsonb_build_object(
    'kind', 'choice',
    'value', jsonb_build_object('options', jsonb_build_array(correct_answer #>> '{}'))
)
WHERE question_type IN ('choice', 'multiple')
  AND correct_answer IS NOT NULL
  AND jsonb_typeof(correct_answer) = 'string';

UPDATE questions
SET correct_answer = jsonb_build_object(
    'kind', 'fill',
    'value', jsonb_build_object('blanks', correct_answer)
)
WHERE question_type = 'fill'
  AND correct_answer IS NOT NULL
  AND jsonb_typeof(correct_answer) = 'array';

UPDATE questions
SET correct_answer = 'null'::jsonb
WHERE question_type = 'solution'
  AND correct_answer IS NOT NULL
  AND jsonb_typeof(correct_answer) <> 'null';
