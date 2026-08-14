pub mod ai_setting;
pub mod ai_task;
pub mod collection;
pub mod document;
pub mod notification;
pub mod paper;
pub mod question;
pub mod space;
pub mod tag_governance;
pub mod user;

use serde::Serialize;

/// 通用分页响应结构
#[derive(Debug, Serialize)]
pub struct PageResult<T: Serialize> {
    pub items: Vec<T>,
    pub total: i64,
    pub page: u32,
    pub page_size: u32,
}
