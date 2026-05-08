-- 创建用户角色枚举类型
DO $$ BEGIN
    CREATE TYPE user_role AS ENUM ('admin', 'groupleader', 'teacher', 'viewer');
EXCEPTION
    WHEN duplicate_object THEN NULL;
END $$;

-- 用户表
CREATE TABLE IF NOT EXISTS users (
    id              UUID PRIMARY KEY,
    username        VARCHAR(50) NOT NULL UNIQUE,
    email           VARCHAR(255) NOT NULL UNIQUE,
    password_hash   VARCHAR(255) NOT NULL,
    display_name    VARCHAR(100) NOT NULL,
    role            user_role NOT NULL DEFAULT 'teacher',
    is_active       BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 用户名和邮箱快速查找
CREATE INDEX IF NOT EXISTS idx_users_username ON users(username);
CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);
