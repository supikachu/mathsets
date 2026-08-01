-- 将 space_members.role 的 DEFAULT 从 'editor' 改为 'member'
-- Phase 6 角色对齐后，Rust SpaceRole 枚举只有 Owner/Member/Viewer
-- 'editor' 虽然仍是合法的 space_role ENUM 值，但无法反序列化为 Rust 枚举
ALTER TABLE space_members ALTER COLUMN role SET DEFAULT 'member'::space_role;
