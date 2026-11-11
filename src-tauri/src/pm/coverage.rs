//! Nested preventive plans (L1-15): a plan can be covered by another plan of the same asset — the
//! annual overhaul includes the weekly check. While the covering plan (or one above it) has an
//! occurrence on the asset inside the window of the covered plan's occurrence, the covered plan
//! does not generate that occurrence, so the asset never gets two work orders for the same week.
//!
//! The link is `pm_plans.covered_by_plan_id` (migration 292); it travels with the plan.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmPlanRef {
    pub id: i64,
    pub code: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmPlanCoverage {
    pub plan_id: i64,
    pub covered_by: Option<PmPlanRef>,
    /// The plans this plan covers directly.
    pub covers: Vec<PmPlanRef>,
}

fn refused(code: &str, text: &str) -> AppError {
    AppError::ValidationFailed(vec![format!("{code}: {text}")])
}

fn plan_ref(row: &sea_orm::QueryResult) -> AppResult<PmPlanRef> {
    Ok(PmPlanRef {
        id: row.try_get("", "id")?,
        code: row.try_get("", "code")?,
        title: row.try_get("", "title")?,
    })
}

/// The plans above `plan_id` in its coverage chain (nearest first is not guaranteed).
pub async fn covering_chain(db: &impl ConnectionTrait, plan_id: i64) -> AppResult<Vec<i64>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "WITH RECURSIVE anc(id) AS (
                 SELECT covered_by_plan_id FROM pm_plans WHERE id = ? AND covered_by_plan_id IS NOT NULL
                 UNION
                 SELECT p.covered_by_plan_id FROM pm_plans p JOIN anc ON p.id = anc.id
                  WHERE p.covered_by_plan_id IS NOT NULL
             )
             SELECT id FROM anc",
            [plan_id.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| r.try_get::<i64>("", "id").map_err(AppError::from))
        .collect()
}

/// True when a plan above `plan_id` has a live occurrence on `equipment_id` due inside
/// `[window_start, window_end]` (RFC 3339 instants).
pub async fn is_covered_in_window(
    db: &impl ConnectionTrait,
    plan_id: i64,
    equipment_id: i64,
    window_start: &str,
    window_end: &str,
) -> AppResult<bool> {
    let chain = covering_chain(db, plan_id).await?;
    if chain.is_empty() {
        return Ok(false);
    }
    let marks = vec!["?"; chain.len()].join(",");
    let mut binds: Vec<sea_orm::Value> = vec![
        equipment_id.into(),
        window_start.to_string().into(),
        window_end.to_string().into(),
    ];
    binds.extend(chain.into_iter().map(Into::into));
    let hit = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT 1 AS ok FROM pm_occurrences o
                  WHERE o.equipment_id = ? AND o.due_at >= ? AND o.due_at <= ?
                    AND o.status NOT IN ('cancelled', 'missed')
                    AND o.pm_plan_id IN ({marks})
                  LIMIT 1"
            ),
            binds,
        ))
        .await?;
    Ok(hit.is_some())
}

/// Number of links between a plan and the top of its chain (0: not covered); used to generate the
/// covering plans before the plans they cover.
pub fn depth_of(plan_id: i64, covered_by: &std::collections::HashMap<i64, i64>) -> usize {
    let mut depth = 0;
    let mut current = plan_id;
    while let Some(next) = covered_by.get(&current) {
        depth += 1;
        current = *next;
        if depth > covered_by.len() {
            break; // a cycle is refused on write; never loop on bad data
        }
    }
    depth
}

pub async fn get_coverage(db: &DatabaseConnection, plan_id: i64) -> AppResult<PmPlanCoverage> {
    let covered_by = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT c.id, c.code, c.title FROM pm_plans p JOIN pm_plans c ON c.id = p.covered_by_plan_id WHERE p.id = ?",
            [plan_id.into()],
        ))
        .await?
        .map(|r| plan_ref(&r))
        .transpose()?;
    let covers = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code, title FROM pm_plans WHERE covered_by_plan_id = ? ORDER BY code",
            [plan_id.into()],
        ))
        .await?
        .iter()
        .map(plan_ref)
        .collect::<AppResult<Vec<_>>>()?;
    Ok(PmPlanCoverage {
        plan_id,
        covered_by,
        covers,
    })
}

/// Sets (or clears, with `None`) the plan that covers `plan_id`. Both plans must target the same
/// asset, and the chain may not loop back.
pub async fn set_covered_by(
    db: &DatabaseConnection,
    plan_id: i64,
    covered_by_plan_id: Option<i64>,
) -> AppResult<PmPlanCoverage> {
    if let Some(cover) = covered_by_plan_id {
        if cover == plan_id {
            return Err(refused("GATE_PM_COVERAGE_SELF", "A plan cannot cover itself."));
        }
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, asset_scope_type, asset_scope_id FROM pm_plans WHERE id IN (?, ?)",
                [plan_id.into(), cover.into()],
            ))
            .await?;
        let scope_of = |id: i64| -> AppResult<(String, Option<i64>)> {
            for r in &rows {
                let rid: i64 = r.try_get("", "id")?;
                if rid == id {
                    return Ok((r.try_get("", "asset_scope_type")?, r.try_get("", "asset_scope_id")?));
                }
            }
            Err(AppError::NotFound {
                entity: "pm_plan".to_string(),
                id: id.to_string(),
            })
        };
        let own = scope_of(plan_id)?;
        let upper = scope_of(cover)?;
        if own.0 != "equipment" || own != upper {
            return Err(refused(
                "GATE_PM_COVERAGE_SCOPE",
                "Only plans of the same single asset can cover one another.",
            ));
        }
        if covering_chain(db, cover).await?.contains(&plan_id) {
            return Err(refused(
                "GATE_PM_COVERAGE_CYCLE",
                "The covering plan is itself covered by this plan.",
            ));
        }
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE pm_plans SET covered_by_plan_id = ?, row_version = row_version + 1,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?",
        [covered_by_plan_id.into(), plan_id.into()],
    ))
    .await?;
    get_coverage(db, plan_id).await
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::depth_of;

    #[test]
    fn depth_counts_links_to_the_top_and_survives_a_loop() {
        // weekly(1) -> monthly(2) -> annual(3)
        let chain: HashMap<i64, i64> = HashMap::from([(1, 2), (2, 3)]);
        assert_eq!(depth_of(3, &chain), 0);
        assert_eq!(depth_of(2, &chain), 1);
        assert_eq!(depth_of(1, &chain), 2);
        // bad data (never written: refused on write) must not loop forever
        let looped: HashMap<i64, i64> = HashMap::from([(1, 2), (2, 1)]);
        assert!(depth_of(1, &looped) <= 3);
    }
}
