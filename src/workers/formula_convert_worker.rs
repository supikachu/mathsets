//! 后台轮询：将 pending/stale 的题目 MathType 资产跑完转换

use std::time::Duration;

use tracing::{info, warn};

use crate::mathtype::convert::MathTypeConvertConfig;
use crate::mathtype::sync::sync_question_math_assets;
use crate::AppState;

pub async fn start_worker(state: AppState) {
    let cfg = MathTypeConvertConfig::from_env();
    if !cfg.is_enabled() {
        info!("MathType Formula Worker 未启用（未配置 MATHTYPE_CONVERT_URL / MATHTYPE_OLE_CLI）");
        return;
    }
    info!(
        "MathType Formula Worker 已启动 convert_url={:?} ole_cli={:?}",
        cfg.convert_url,
        cfg.ole_cli.as_ref().map(|p| p.display().to_string())
    );

    let mut interval = tokio::time::interval(Duration::from_secs(15));
    loop {
        interval.tick().await;
        if let Err(e) = poll_once(&state, &cfg).await {
            warn!("MathType Formula Worker 轮询失败: {e}");
        }
    }
}

async fn poll_once(state: &AppState, cfg: &MathTypeConvertConfig) -> Result<(), String> {
    let ids: Vec<uuid::Uuid> = sqlx::query_scalar(
        r#"
        SELECT DISTINCT question_id
        FROM question_math_assets
        WHERE status IN ('pending', 'stale')
        ORDER BY question_id
        LIMIT 8
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| e.to_string())?;

    for id in ids {
        match sync_question_math_assets(&state.pool, cfg, id).await {
            Ok(r) => {
                if r.total > 0 {
                    info!(
                        "MathType 同步 question={} total={} ready={} failed={} skipped={}",
                        id, r.total, r.ready, r.failed, r.skipped
                    );
                }
            }
            Err(e) => warn!("MathType 同步失败 question={id}: {e}"),
        }
    }
    Ok(())
}
