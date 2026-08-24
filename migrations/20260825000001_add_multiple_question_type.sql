-- B2：补齐多选题枚举值（Rust QuestionType::Multiple / 前端 'multiple' 已存在，库侧此前缺失）
ALTER TYPE question_type ADD VALUE IF NOT EXISTS 'multiple';
