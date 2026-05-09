-- 试卷表
CREATE TABLE papers (
    id UUID PRIMARY KEY,
    title VARCHAR(255) NOT NULL,
    description TEXT,
    subject VARCHAR(50) NOT NULL DEFAULT '数学',
    grade VARCHAR(20),
    total_score INTEGER NOT NULL DEFAULT 0,
    duration_minutes INTEGER,
    status VARCHAR(20) NOT NULL DEFAULT 'draft',
    creator_id UUID REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    version INTEGER NOT NULL DEFAULT 1
);

-- 试卷-题目关联表
CREATE TABLE paper_questions (
    id UUID PRIMARY KEY,
    paper_id UUID NOT NULL REFERENCES papers(id) ON DELETE CASCADE,
    question_id UUID NOT NULL REFERENCES questions(id),
    sort_order INTEGER NOT NULL DEFAULT 0,
    score INTEGER NOT NULL DEFAULT 0,
    section VARCHAR(100),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_paper_questions_paper_id ON paper_questions(paper_id);
CREATE INDEX idx_paper_questions_question_id ON paper_questions(question_id);
CREATE INDEX idx_papers_creator_id ON papers(creator_id);
CREATE INDEX idx_papers_status ON papers(status);
