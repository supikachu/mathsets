-- =============================================================================
-- 消息通知表（协同工作流 + 系统级通知）
-- =============================================================================
-- 用途：
--   1. 工作流通知 (workflow)：题目提交审核、审核通过、审核驳回等业务提醒
--   2. 系统级通知 (system)：全局角色变更、被拉入团队空间等管理员操作通知
--
-- 设计要点：
--   - resource_type + resource_id 支持前端点击跳转闭环（如跳转到题目详情/编辑页）
--   - is_read 标记 + 复合索引优化未读查询性能
--   - ON DELETE CASCADE 确保用户删除时通知自动清理
-- =============================================================================

CREATE TABLE IF NOT EXISTS notifications (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- 通知类型：workflow（工作流）/ system（系统级）
    kind          VARCHAR(20) NOT NULL DEFAULT 'workflow',
    -- 通知标题（前端 Toast / 列表展示）
    title         VARCHAR(200) NOT NULL,
    -- 通知正文（可含驳回理由等）
    body          TEXT,
    -- 关联资源类型 + ID（如 question / question_edit / space）
    resource_type VARCHAR(30),
    resource_id   UUID,
    -- 已读标记
    is_read       BOOLEAN NOT NULL DEFAULT FALSE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 未读通知查询（铃铛红点高频查询）
CREATE INDEX IF NOT EXISTS idx_notifications_user_unread
    ON notifications (user_id, is_read, created_at DESC);

-- 通知列表分页查询
CREATE INDEX IF NOT EXISTS idx_notifications_user_created
    ON notifications (user_id, created_at DESC);
