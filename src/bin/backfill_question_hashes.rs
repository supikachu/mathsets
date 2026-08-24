//! V2.1.1 题目去重 hash 离线回填 Job（计划书 §八）
//!
//! 用途：为历史题目批量计算 content_hash / normalized_content_hash。
//! 规范化算法与运行时完全一致（src/util/normalize.rs，Rust 单点实现，
//! SQL 不做第二套），保证新旧数据同一套去重语义。
//!
//! 安全特性：
//!   1. 幂等：只回填 hash 为 NULL 的行；重复执行不重复计算、不覆盖已有值
//!   2. 分批处理（默认每批 500 行），可安全中断续跑
//!   3. 单行失败仅记录日志，不中断整批
//!   4. 不修改任何业务字段，只写两个 hash 列
//!
//! 运行方式：
//!   cargo run --bin backfill_question_hashes [batch_size]
//!
//! 例：cargo run --bin backfill_question_hashes 1000

use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

use mathset::config::AppConfig;
use mathset::db;
use mathset::util::normalize::{
    compute_content_hash, compute_normalized_content_hash, normalize_text,
};

/// 每批处理行数
const DEFAULT_BATCH_SIZE: i64 = 500;

#[derive(Debug, sqlx::FromRow)]
struct QuestionRow {
    id: uuid::Uuid,
    stem: String,
    options: Option<Value>,
    correct_answer: Value,
    analysis: Option<String>,
}

#[tokio::main]
async fn main() {
    // 加载 .env（DATABASE_URL 等）
    let _ = dotenvy::dotenv();
    let config = AppConfig::from_env();
    let pool = db::create_pool(&config.database_url, 10).await;
    db::run_migrations(&pool).await;

    let batch_size: i64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_BATCH_SIZE);

    println!("开始回填题目 hash（batch_size={batch_size}）…");

    let mut filled = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    let failed_ids: AtomicUsize = AtomicUsize::new(0);

    loop {
        let rows: Vec<QuestionRow> = sqlx::query_as(
            r#"
            SELECT id, stem, options, correct_answer, analysis,
                   content_hash, normalized_content_hash
            FROM questions
            WHERE content_hash IS NULL OR normalized_content_hash IS NULL
            ORDER BY created_at
            LIMIT $1
            "#,
        )
        .bind(batch_size)
        .fetch_all(&pool)
        .await
        .expect("查询待回填题目失败");

        if rows.is_empty() {
            break;
        }

        for row in rows {
            let content = compute_content_hash(
                &row.stem,
                row.options.as_ref(),
                &row.correct_answer,
                row.analysis.as_deref(),
            );
            let normalized = compute_normalized_content_hash(
                &row.stem,
                row.options.as_ref(),
                &row.correct_answer,
            );

            let result = sqlx::query(
                r#"
                UPDATE questions
                SET content_hash = COALESCE($2, content_hash),
                    normalized_content_hash = COALESCE($3, normalized_content_hash)
                WHERE id = $1
                "#,
            )
            .bind(row.id)
            .bind(&content)
            .bind(&normalized)
            .execute(&pool)
            .await;

            match result {
                Ok(r) if r.rows_affected() > 0 => {
                    filled += 1;
                    if filled % 500 == 0 {
                        println!("已回填 {filled} 行…");
                    }
                }
                Ok(_) => skipped += 1,
                Err(e) => {
                    failed += 1;
                    failed_ids.fetch_add(1, Ordering::Relaxed);
                    println!("行 {} 回填失败: {e}", row.id);
                }
            }
        }
    }

    println!(
        "回填完成：filled={filled} skipped={skipped} failed={failed}（示例 stem：{}）",
        normalize_text("回填校验")
    );
}
