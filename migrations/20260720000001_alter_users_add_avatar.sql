-- =============================================================================
-- 用户表扩展：添加头像 URL 字段
-- 用于用户中心模块（upload_avatar / update_my_profile）的头像持久化
-- =============================================================================

-- 添加 avatar_url 字段
-- 使用 IF NOT EXISTS 保证幂等：
--   - 对于此前已通过修改版迁移（直接在 20260719000001 中加字段）应用过的数据库，
--     此处不会重复添加列，避免 "column already exists" 错误
--   - 对于全新数据库（没有修改过历史迁移），此处会正常添加列
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS avatar_url VARCHAR(255);

-- 为 avatar_url 建立索引（可选，便于后续按头像 URL 反查用户）
-- 当前用户量级不大，暂不强制建立索引，避免无谓的索引维护开销
-- CREATE INDEX IF NOT EXISTS idx_users_avatar_url ON users(avatar_url) WHERE avatar_url IS NOT NULL;

-- 更新已迁移数据说明（仅作注释，不执行）
-- 历史用户记录 avatar_url 将保持 NULL，前端会展示 display_name 首字母作为头像回退
