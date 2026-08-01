//! Model-scope helpers for org trees.
//!
//! Production (ops) always uses the **active** structure model's nodes.
//! Designer draft workspace uses the **draft** model's nodes exclusively.

use crate::errors::{AppError, AppResult};
use crate::org::fail::{fail, fail_params};
use sea_orm::{ConnectionTrait, DbBackend, Statement};

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "org model_scope decode failed for column '{column}': {e}"
    ))
}

/// Return the id of the currently active structure model.
pub async fn get_active_model_id(db: &impl ConnectionTrait) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM org_structure_models WHERE status = 'active' LIMIT 1".to_string(),
        ))
        .await?
        .ok_or_else(|| {
            fail(
                "ORG_NO_ACTIVE_MODEL",
                "No active organization structure model exists.",
            )
        })?;
    row.try_get::<i64>("", "id")
        .map_err(|e| decode_err("id", e))
}

/// Optional active model id (None when bootstrapping / no publish yet).
pub async fn try_get_active_model_id(db: &impl ConnectionTrait) -> AppResult<Option<i64>> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM org_structure_models WHERE status = 'active' LIMIT 1".to_string(),
        ))
        .await?;
    match row {
        Some(r) => Ok(Some(
            r.try_get::<i64>("", "id")
                .map_err(|e| decode_err("id", e))?,
        )),
        None => Ok(None),
    }
}

pub async fn get_model_status(db: &impl ConnectionTrait, model_id: i64) -> AppResult<String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT status FROM org_structure_models WHERE id = ?",
            [model_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "org_structure_model".to_string(),
            id: model_id.to_string(),
        })?;
    row.try_get::<String>("", "status")
        .map_err(|e| decode_err("status", e))
}

/// Assert a node belongs to the given structure model and is not deleted.
pub async fn assert_node_in_model(
    db: &impl ConnectionTrait,
    node_id: i64,
    model_id: i64,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT structure_model_id FROM org_nodes WHERE id = ? AND deleted_at IS NULL",
            [node_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "org_node".to_string(),
            id: node_id.to_string(),
        })?;
    let sid: i64 = row
        .try_get("", "structure_model_id")
        .map_err(|e| decode_err("structure_model_id", e))?;
    if sid != model_id {
        return Err(fail(
            "ORG_NODE_WRONG_MODEL",
            "This org node belongs to a different structure model.",
        ));
    }
    Ok(())
}

/// Assert a node is part of the **active** (production) tree.
/// Operational modules must use this before reading/writing node FKs.
pub async fn assert_node_is_active_tree(db: &impl ConnectionTrait, node_id: i64) -> AppResult<()> {
    let active_id = get_active_model_id(db).await?;
    assert_node_in_model(db, node_id, active_id).await
}

/// Assert an org node exists in the **active** tree and has status = `active`.
/// Use from operational modules (assets, DI, personnel, …) before persisting FKs.
pub async fn assert_org_node_active(db: &impl ConnectionTrait, org_node_id: i64) -> AppResult<()> {
    assert_node_is_active_tree(db, org_node_id).await?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT status FROM org_nodes WHERE id = ? AND deleted_at IS NULL",
            [org_node_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "org_node".to_string(),
            id: org_node_id.to_string(),
        })?;
    let status: String = row
        .try_get("", "status")
        .map_err(|e| decode_err("status", e))?;
    if status != "active" {
        return Err(fail_params(
            "ORG_NODE_NOT_ACTIVE",
            format!("This organization node is not active (status: {status})."),
            &[("status", status)],
        ));
    }
    Ok(())
}

/// Reject structural mutations on the **active** tree while a draft model exists.
///
/// When a draft workspace is open, create/move/deactivate must target the draft
/// model only. Metadata-only updates are out of scope for this guard.
pub async fn assert_structural_edit_allowed_for_model(
    db: &impl ConnectionTrait,
    structure_model_id: i64,
) -> AppResult<()> {
    let draft_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM org_structure_models WHERE status = 'draft'".to_string(),
        ))
        .await?
        .expect("COUNT always returns a row");
    let draft_count: i64 = draft_row
        .try_get("", "c")
        .map_err(|e| decode_err("c", e))?;
    if draft_count == 0 {
        return Ok(());
    }

    let Some(active_id) = try_get_active_model_id(db).await? else {
        return Ok(());
    };
    if structure_model_id == active_id {
        return Err(fail(
            "ORG_EDIT_REQUIRES_DRAFT",
            "Structural changes must be made in the draft workspace while a draft exists.",
        ));
    }
    Ok(())
}

/// Resolve an org node id by code from the **active** structure model only.
pub async fn try_resolve_active_org_node_id_by_code(
    db: &impl ConnectionTrait,
    code: &str,
) -> AppResult<Option<i64>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT n.id FROM org_nodes n \
             INNER JOIN org_structure_models m \
               ON m.id = n.structure_model_id AND m.status = 'active' \
             WHERE n.code = ? AND n.deleted_at IS NULL \
             LIMIT 1",
            [code.into()],
        ))
        .await?;
    match row {
        Some(r) => Ok(Some(
            r.try_get::<i64>("", "id")
                .map_err(|e| decode_err("id", e))?,
        )),
        None => Ok(None),
    }
}

/// Resolve structure_model_id for a node (non-deleted).
pub async fn get_node_structure_model_id(
    db: &impl ConnectionTrait,
    node_id: i64,
) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT structure_model_id FROM org_nodes WHERE id = ? AND deleted_at IS NULL",
            [node_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "org_node".to_string(),
            id: node_id.to_string(),
        })?;
    row.try_get::<i64>("", "structure_model_id")
        .map_err(|e| decode_err("structure_model_id", e))
}

/// Assign NULL-scoped live nodes to the given model id. Returns rows updated.
pub async fn heal_null_structure_model_ids(
    db: &impl ConnectionTrait,
    model_id: i64,
) -> AppResult<u64> {
    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE org_nodes \
             SET structure_model_id = ? \
             WHERE structure_model_id IS NULL AND deleted_at IS NULL",
            [model_id.into()],
        ))
        .await?;
    Ok(result.rows_affected())
}

/// Heal onto the active model when one exists. Idempotent startup/bootstrap helper.
pub async fn heal_null_structure_model_ids_onto_active(
    db: &impl ConnectionTrait,
) -> AppResult<u64> {
    let Some(active_id) = try_get_active_model_id(db).await? else {
        return Ok(0);
    };
    heal_null_structure_model_ids(db, active_id).await
}

/// Fail if any live node still has NULL structure_model_id.
pub async fn assert_no_null_structure_model_ids(db: &impl ConnectionTrait) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM org_nodes \
             WHERE structure_model_id IS NULL AND deleted_at IS NULL"
                .to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("count query returned no row")))?;
    let count: i64 = row
        .try_get("", "c")
        .map_err(|e| decode_err("c", e))?;
    if count > 0 {
        return Err(fail_params(
            "ORG_NODES_MISSING_MODEL",
            format!(
                "{count} org node(s) still have no structure model — heal or assign before forking."
            ),
            &[("count", count.to_string())],
        ));
    }
    Ok(())
}
