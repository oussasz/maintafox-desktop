//! Composite equipment health score (lifecycle + meters + open work).

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;

use crate::errors::{AppError, AppResult};

/// IPC payload aligned with `shared/ipc-types.ts` `AssetHealthScore`.
#[derive(Debug, Clone, Serialize)]
pub struct AssetHealthScore {
    pub asset_id: i64,
    pub score: Option<i64>,
    pub label: String,
}

fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    let t = s.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(t) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(n) = chrono::NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S") {
        return Some(DateTime::from_naive_utc_and_offset(n, Utc));
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(t, "%Y-%m-%d") {
        let n = d.and_hms_opt(0, 0, 0)?;
        return Some(DateTime::from_naive_utc_and_offset(n, Utc));
    }
    None
}

async fn count_i64(db: &DatabaseConnection, sql: &str, asset_id: i64) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, [asset_id.into()]))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("expected COUNT row")))?;
    Ok(row.try_get::<i64>("", "cnt").unwrap_or(0))
}

/// Combines recency of lifecycle/meter activity with open DI/WO pressure.
pub async fn get_asset_health_score(db: &DatabaseConnection, asset_id: i64) -> AppResult<AssetHealthScore> {
    let lifecycle_n = count_i64(
        db,
        "SELECT COUNT(*) AS cnt FROM equipment_lifecycle_events WHERE equipment_id = ?",
        asset_id,
    )
    .await?;

    if lifecycle_n == 0 {
        return Ok(AssetHealthScore {
            asset_id,
            score: None,
            label: "no_data".to_string(),
        });
    }

    let last_lifecycle_row = db
        .query_one(
            Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT MAX(occurred_at) AS t FROM equipment_lifecycle_events WHERE equipment_id = ?",
                [asset_id.into()],
            ),
        )
        .await?;
    let last_lifecycle: Option<String> = last_lifecycle_row
        .as_ref()
        .and_then(|r| r.try_get::<Option<String>>("", "t").ok())
        .flatten();

    let meter_row = db
        .query_one(
            Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c, MAX(last_read_at) AS m \
                 FROM equipment_meters WHERE equipment_id = ? AND is_active = 1",
                [asset_id.into()],
            ),
        )
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("meter aggregate missing")))?;

    let meter_count: i64 = meter_row.try_get("", "c").unwrap_or(0);
    let last_meter: Option<String> = meter_row
        .try_get::<Option<String>>("", "m")
        .ok()
        .flatten();

    let mut candidates: Vec<DateTime<Utc>> = Vec::new();
    if let Some(ref s) = last_lifecycle {
        if let Some(dt) = parse_ts(s) {
            candidates.push(dt);
        }
    }
    if meter_count > 0 {
        if let Some(ref s) = last_meter {
            if let Some(dt) = parse_ts(s) {
                candidates.push(dt);
            }
        }
    }

    let Some(most_recent) = candidates.into_iter().max() else {
        return Ok(AssetHealthScore {
            asset_id,
            score: None,
            label: "no_data".to_string(),
        });
    };

    let now = Utc::now();
    let days = (now - most_recent).num_days().max(0);

    let recency: i64 = if days <= 7 {
        100
    } else if days <= 30 {
        85
    } else if days <= 90 {
        68
    } else {
        50
    };

    let meter_points: i64 = if meter_count == 0 {
        72
    } else if let Some(ref s) = last_meter {
        match parse_ts(s) {
            Some(t) if (now - t).num_days() <= 30 => 92,
            Some(_) => 55,
            None => 62,
        }
    } else {
        62
    };

    let open_di = count_i64(
        db,
        "SELECT COUNT(*) AS cnt FROM intervention_requests \
         WHERE asset_id = ? AND status NOT IN (\
           'rejected','converted_to_work_order','closed_as_non_executable','archived'\
         )",
        asset_id,
    )
    .await?;

    let open_wo = count_i64(
        db,
        "SELECT COUNT(*) AS cnt FROM work_orders wo \
         JOIN work_order_statuses s ON s.id = wo.status_id \
         WHERE wo.equipment_id = ? AND s.code NOT IN ('closed','cancelled')",
        asset_id,
    )
    .await?;

    let penalty = (open_di * 12 + open_wo * 10).min(75);
    let workload = 100 - penalty;

    let score = (recency * 45 + workload * 35 + meter_points * 20) / 100;
    let score = score.clamp(0, 100);

    let label = if score >= 80 {
        "good"
    } else if score >= 50 {
        "fair"
    } else {
        "poor"
    };

    Ok(AssetHealthScore {
        asset_id,
        score: Some(score),
        label: label.to_string(),
    })
}
