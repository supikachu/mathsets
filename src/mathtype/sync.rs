//! 题目公式资产：标脏、同步转换、加载 ready 资产

use std::collections::HashMap;

use sqlx::PgPool;
use uuid::Uuid;

use crate::export::math::{MathOutcome, to_mathml};
use crate::mathtype::convert::{
    convert_batch, mathml_to_ole_bin, MathTypeConvertConfig, WmfResult,
};
use crate::mathtype::extract::{extract_slots, FormulaSlot};
use crate::models::question::Question;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct MathTypeAssetKey {
    pub question_id: Uuid,
    pub field: String,
    pub ordinal: i32,
}

#[derive(Debug, Clone)]
pub struct ReadyMathAsset {
    pub latex: String,
    pub display: bool,
    pub ole_bin: Vec<u8>,
    pub wmf: Vec<u8>,
    pub baseline_offset_pt: f64,
    pub width_pt: f64,
    pub height_pt: f64,
}

#[derive(Debug, sqlx::FromRow)]
struct AssetRow {
    field: String,
    ordinal: i32,
    latex: String,
    display: bool,
    ole_bin: Option<Vec<u8>>,
    wmf: Option<Vec<u8>>,
    baseline_offset_pt: Option<f64>,
    width_pt: Option<f64>,
    height_pt: Option<f64>,
    status: String,
    priority: i32,
}

/// 题目内容变更后：对齐槽位，未变且 ready 的保留，其余 pending/stale
pub async fn mark_slots_from_question(pool: &PgPool, q: &Question) -> Result<usize, String> {
    mark_slots_from_question_with_priority(pool, q, PRIORITY_NORMAL).await
}

/// AI 延后转换：只登记槽位，低优先级
pub async fn mark_slots_ai_deferred(pool: &PgPool, q: &Question) -> Result<usize, String> {
    mark_slots_from_question_with_priority(pool, q, PRIORITY_AI_DEFER).await
}

pub const PRIORITY_AI_DEFER: i32 = 0;
pub const PRIORITY_NORMAL: i32 = 10;
pub const PRIORITY_EXPORT: i32 = 100;

pub async fn mark_slots_from_question_with_priority(
    pool: &PgPool,
    q: &Question,
    priority: i32,
) -> Result<usize, String> {
    let slots = extract_slots(
        &q.stem,
        q.options.as_ref(),
        q.correct_answer.as_ref(),
        q.analysis.as_deref(),
        q.structure.as_ref(),
    );
    replace_slots(pool, q.id, &slots, priority).await
}

/// 将题目未 ready 资产的优先级至少提升到 `min_priority`（不降低已有更高优先级）
pub async fn bump_question_priority(
    pool: &PgPool,
    question_ids: &[Uuid],
    min_priority: i32,
) -> Result<u64, String> {
    if question_ids.is_empty() {
        return Ok(0);
    }
    let r = sqlx::query(
        r#"
        UPDATE question_math_assets
        SET priority = GREATEST(priority, $2), updated_at = $3
        WHERE question_id = ANY($1)
          AND status IN ('pending', 'stale', 'failed')
        "#,
    )
    .bind(question_ids)
    .bind(min_priority)
    .bind(chrono::Utc::now())
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(r.rows_affected())
}

async fn replace_slots(
    pool: &PgPool,
    question_id: Uuid,
    slots: &[FormulaSlot],
    priority: i32,
) -> Result<usize, String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;

    let existing: Vec<AssetRow> = sqlx::query_as(
        r#"
        SELECT field, ordinal, latex, display, ole_bin, wmf,
               baseline_offset_pt, width_pt, height_pt, status, priority
        FROM question_math_assets
        WHERE question_id = $1
        "#,
    )
    .bind(question_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    let mut keep: HashMap<(String, i32), &AssetRow> = HashMap::new();
    for row in &existing {
        keep.insert((row.field.clone(), row.ordinal), row);
    }

    sqlx::query("DELETE FROM question_math_assets WHERE question_id = $1")
        .bind(question_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    let now = chrono::Utc::now();
    for slot in slots {
        let key = (slot.field.to_string(), slot.ordinal);
        let reuse = keep.get(&key).and_then(|old| {
            if old.latex == slot.latex
                && old.display == slot.display
                && old.status == "ready"
                && old.ole_bin.as_ref().is_some_and(|b| !b.is_empty())
                && old.wmf.as_ref().is_some_and(|b| b.len() >= 22)
            {
                Some(*old)
            } else {
                None
            }
        });

        // 同槽位公式未变时保留更高优先级（避免 sync/mark 冲掉导出提权）
        let slot_priority = keep
            .get(&key)
            .filter(|old| old.latex == slot.latex && old.display == slot.display)
            .map(|old| old.priority.max(priority))
            .unwrap_or(priority);

        let id = Uuid::new_v4();
        if let Some(old) = reuse {
            sqlx::query(
                r#"
                INSERT INTO question_math_assets (
                    id, question_id, field, ordinal, latex, display, mathml,
                    ole_bin, wmf, baseline_offset_pt, width_pt, height_pt,
                    status, error, engine, priority, created_at, updated_at
                ) VALUES (
                    $1,$2,$3,$4,$5,$6,NULL,
                    $7,$8,$9,$10,$11,
                    'ready', NULL, 'reuse', $12, $13, $13
                )
                "#,
            )
            .bind(id)
            .bind(question_id)
            .bind(slot.field)
            .bind(slot.ordinal)
            .bind(&slot.latex)
            .bind(slot.display)
            .bind(old.ole_bin.as_ref())
            .bind(old.wmf.as_ref())
            .bind(old.baseline_offset_pt)
            .bind(old.width_pt)
            .bind(old.height_pt)
            .bind(slot_priority)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        } else {
            sqlx::query(
                r#"
                INSERT INTO question_math_assets (
                    id, question_id, field, ordinal, latex, display,
                    status, priority, created_at, updated_at
                ) VALUES ($1,$2,$3,$4,$5,$6,'pending',$7,$8,$8)
                "#,
            )
            .bind(id)
            .bind(question_id)
            .bind(slot.field)
            .bind(slot.ordinal)
            .bind(&slot.latex)
            .bind(slot.display)
            .bind(slot_priority)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }
    }

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(slots.len())
}

/// 对 pending/stale/failed 槽位跑完整转换；无 Worker 配置时仅写 MathML（若可）并保持 pending
pub async fn sync_question_math_assets(
    pool: &PgPool,
    cfg: &MathTypeConvertConfig,
    question_id: Uuid,
) -> Result<SyncReport, String> {
    let q: Question = sqlx::query_as("SELECT * FROM questions WHERE id = $1")
        .bind(question_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "题目不存在".to_string())?;

    mark_slots_from_question(pool, &q).await?;

    let pending: Vec<PendingRow> = sqlx::query_as(
        r#"
        SELECT id, field, ordinal, latex, display
        FROM question_math_assets
        WHERE question_id = $1 AND status IN ('pending', 'stale', 'failed')
        ORDER BY field, ordinal
        "#,
    )
    .bind(question_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    if pending.is_empty() {
        return Ok(SyncReport {
            total: 0,
            ready: count_ready(pool, question_id).await?,
            failed: 0,
            skipped: 0,
        });
    }

    if !cfg.is_enabled() {
        return Ok(SyncReport {
            total: pending.len(),
            ready: count_ready(pool, question_id).await?,
            failed: 0,
            skipped: pending.len(),
        });
    }

    let mut bins: Vec<(String, Vec<u8>, Uuid, String)> = Vec::new();
    let mut failed = 0usize;
    let mut skipped = 0usize;

    for row in &pending {
        let mathml = match to_mathml(&row.latex, row.display) {
            MathOutcome::Ok(m) if m.contains("<math") => m,
            MathOutcome::Ok(_) => {
                set_failed(pool, row.id, "MathML 输出为空").await?;
                failed += 1;
                continue;
            }
            MathOutcome::Failed(reason) => {
                set_failed(pool, row.id, &reason).await?;
                failed += 1;
                continue;
            }
        };

        sqlx::query(
            r#"UPDATE question_math_assets SET mathml = $2, updated_at = $3 WHERE id = $1"#,
        )
        .bind(row.id)
        .bind(&mathml)
        .bind(chrono::Utc::now())
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

        if cfg.ole_cli.is_none() {
            skipped += 1;
            continue;
        }

        match mathml_to_ole_bin(cfg, &mathml).await {
            Ok(bin) => {
                let batch_id = format!("{}:{}:{}", row.field, row.ordinal, row.id);
                bins.push((batch_id, bin, row.id, row.latex.clone()));
            }
            Err(e) => {
                set_failed(pool, row.id, &e).await?;
                failed += 1;
            }
        }
    }

    if bins.is_empty() || cfg.convert_urls.is_empty() {
        if cfg.convert_urls.is_empty() {
            skipped += bins.len();
            for (_, bin, id, _) in &bins {
                sqlx::query(
                    r#"
                    UPDATE question_math_assets
                    SET ole_bin = $2, status = 'pending', error = '等待 WMF：未配置 MATHTYPE_CONVERT_URL(S)',
                        updated_at = $3
                    WHERE id = $1
                    "#,
                )
                .bind(id)
                .bind(bin)
                .bind(chrono::Utc::now())
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
            }
        }
        return Ok(SyncReport {
            total: pending.len(),
            ready: count_ready(pool, question_id).await?,
            failed,
            skipped,
        });
    }

    let batch_in: Vec<(String, Vec<u8>)> = bins
        .iter()
        .map(|(id, bin, _, _)| (id.clone(), bin.clone()))
        .collect();
    let results = convert_batch(cfg, &batch_in).await?;
    let mut by_id: HashMap<String, Result<WmfResult, String>> = results.into_iter().collect();

    for (batch_id, bin, asset_id, _) in bins {
        match by_id.remove(&batch_id) {
            Some(Ok(wmf)) => {
                sqlx::query(
                    r#"
                    UPDATE question_math_assets SET
                        ole_bin = $2,
                        wmf = $3,
                        baseline_offset_pt = $4,
                        width_pt = $5,
                        height_pt = $6,
                        status = 'ready',
                        error = NULL,
                        engine = $7,
                        updated_at = $8
                    WHERE id = $1
                    "#,
                )
                .bind(asset_id)
                .bind(&bin)
                .bind(&wmf.wmf)
                .bind(wmf.baseline_offset_pt)
                .bind(wmf.width_pt)
                .bind(wmf.height_pt)
                .bind(wmf.method.as_deref().unwrap_or("convert_batch"))
                .bind(chrono::Utc::now())
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
            }
            Some(Err(e)) => {
                sqlx::query(
                    r#"
                    UPDATE question_math_assets SET
                        ole_bin = $2, status = 'failed', error = $3, updated_at = $4
                    WHERE id = $1
                    "#,
                )
                .bind(asset_id)
                .bind(&bin)
                .bind(&e)
                .bind(chrono::Utc::now())
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
                failed += 1;
            }
            None => {
                set_failed(pool, asset_id, "convert_batch 未返回该 id").await?;
                failed += 1;
            }
        }
    }

    Ok(SyncReport {
        total: pending.len(),
        ready: count_ready(pool, question_id).await?,
        failed,
        skipped,
    })
}

#[derive(Debug, Clone)]
pub struct SyncReport {
    pub total: usize,
    pub ready: usize,
    pub failed: usize,
    pub skipped: usize,
}

#[derive(Debug, sqlx::FromRow)]
struct PendingRow {
    id: Uuid,
    field: String,
    ordinal: i32,
    latex: String,
    display: bool,
}

async fn count_ready(pool: &PgPool, question_id: Uuid) -> Result<usize, String> {
    let n: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM question_math_assets WHERE question_id = $1 AND status = 'ready'"#,
    )
    .bind(question_id)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(n as usize)
}

async fn set_failed(pool: &PgPool, id: Uuid, err: &str) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE question_math_assets
        SET status = 'failed', error = $2, updated_at = $3
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(err)
    .bind(chrono::Utc::now())
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 加载题目的 ready 资产，供 docx MathType 导出
pub async fn load_ready_assets(
    pool: &PgPool,
    question_ids: &[Uuid],
) -> Result<HashMap<MathTypeAssetKey, ReadyMathAsset>, String> {
    if question_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<ReadyRow> = sqlx::query_as(
        r#"
        SELECT question_id, field, ordinal, latex, display, ole_bin, wmf,
               baseline_offset_pt, width_pt, height_pt
        FROM question_math_assets
        WHERE question_id = ANY($1) AND status = 'ready'
          AND ole_bin IS NOT NULL AND wmf IS NOT NULL
        "#,
    )
    .bind(question_ids)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut map = HashMap::new();
    for r in rows {
        let Some(ole) = r.ole_bin else { continue };
        let Some(wmf) = r.wmf else { continue };
        map.insert(
            MathTypeAssetKey {
                question_id: r.question_id,
                field: r.field,
                ordinal: r.ordinal,
            },
            ReadyMathAsset {
                latex: r.latex,
                display: r.display,
                ole_bin: ole,
                wmf,
                baseline_offset_pt: r.baseline_offset_pt.unwrap_or(0.0),
                width_pt: r.width_pt.unwrap_or(36.0),
                height_pt: r.height_pt.unwrap_or(18.0),
            },
        );
    }
    Ok(map)
}

#[derive(Debug, sqlx::FromRow)]
struct ReadyRow {
    question_id: Uuid,
    field: String,
    ordinal: i32,
    latex: String,
    display: bool,
    ole_bin: Option<Vec<u8>>,
    wmf: Option<Vec<u8>>,
    baseline_offset_pt: Option<f64>,
    width_pt: Option<f64>,
    height_pt: Option<f64>,
}

/// 提交审核门：若 require_on_submit，则任一非 ready（且题目有公式）则失败
pub async fn submit_gate(
    pool: &PgPool,
    cfg: &MathTypeConvertConfig,
    question_id: Uuid,
) -> Result<(), String> {
    if !cfg.require_on_submit {
        return Ok(());
    }
    let total: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM question_math_assets WHERE question_id = $1"#,
    )
    .bind(question_id)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    if total == 0 {
        return Ok(());
    }
    let not_ready: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM question_math_assets
        WHERE question_id = $1 AND status <> 'ready'
        "#,
    )
    .bind(question_id)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    if not_ready > 0 {
        return Err(format!(
            "还有 {not_ready} 条公式 MathType 资产未就绪，请稍后重试或检查转换 Worker"
        ));
    }
    Ok(())
}
