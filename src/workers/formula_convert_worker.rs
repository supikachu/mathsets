//! 后台轮询：将 pending/stale 的题目 MathType 资产跑完转换（优先级队列 + 多实例并行）

use std::time::Duration;

use futures::future::join_all;
use tracing::{info, warn};

use crate::mathtype::convert::MathTypeConvertConfig;
use crate::mathtype::sync::sync_question_math_assets;
use crate::AppState;

pub async fn start_worker(state: AppState) {
    let cfg = MathTypeConvertConfig::from_env();
    if !cfg.is_enabled() {
        info!("MathType Formula Worker 未启用（未配置 MATHTYPE_CONVERT_URL(S) / MATHTYPE_OLE_CLI）");
        return;
    }
    info!(
        "MathType Formula Worker 已启动 convert_urls={:?} parallel={} ole_cli={:?}",
        cfg.convert_urls,
        cfg.worker_parallel,
        cfg.ole_cli.as_ref().map(|p| p.display().to_string())
    );

    loop {
        match poll_once(&state, &cfg).await {
            Ok(n) if n > 0 => {
                // 有活干：立刻再取一批
            }
            Ok(_) => {
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
            Err(e) => {
                warn!("MathType Formula Worker 轮询失败: {e}");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

/// 返回本轮处理的题目数
async fn poll_once(state: &AppState, cfg: &MathTypeConvertConfig) -> Result<usize, String> {
    let limit = cfg.worker_parallel.max(1) as i64;
    let ids: Vec<uuid::Uuid> = sqlx::query_scalar(
        r#"
        SELECT question_id
        FROM question_math_assets
        WHERE status IN ('pending', 'stale')
        GROUP BY question_id
        ORDER BY MAX(priority) DESC, MIN(updated_at) ASC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| e.to_string())?;

    if ids.is_empty() {
        return Ok(0);
    }

    let futs = ids.into_iter().map(|id| {
        let pool = state.pool.clone();
        let cfg = cfg.clone();
        async move {
            match sync_question_math_assets(&pool, &cfg, id).await {
                Ok(r) => {
                    if r.total > 0 {
                        info!(
                            "MathType 同步 question={} total={} ready={} failed={} skipped={}",
                            id, r.total, r.ready, r.failed, r.skipped
                        );
                    }
                    Ok(())
                }
                Err(e) => {
                    warn!("MathType 同步失败 question={id}: {e}");
                    Err(e)
                }
            }
        }
    });

    let results = join_all(futs).await;
    Ok(results.len())
}
