//! Outbox staging for operational domains (DI, PM, inventory, media).

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

use crate::errors::AppResult;
use crate::sync::domain::{
    ArticleSyncPayload, InterventionRequestSyncPayload, MediaAttachmentSyncPayload,
    PmOccurrenceSyncPayload, PmPlanSyncPayload, PmPlanVersionSyncPayload, StageOutboxItemInput,
    StockLocationSyncPayload, StockMovementSyncPayload, StockReservationSyncPayload,
    WarehouseSyncPayload, SYNC_ENTITY_ARTICLES, SYNC_ENTITY_INTERVENTION_REQUESTS,
    SYNC_ENTITY_MEDIA_ATTACHMENTS, SYNC_ENTITY_PM_OCCURRENCES, SYNC_ENTITY_PM_PLANS,
    SYNC_ENTITY_PM_PLAN_VERSIONS, SYNC_ENTITY_STOCK_LOCATIONS, SYNC_ENTITY_STOCK_MOVEMENTS,
    SYNC_ENTITY_STOCK_RESERVATIONS, SYNC_ENTITY_WAREHOUSES,
};
use crate::sync::queries::stage_outbox_item;

async fn lookup_entity_sync_id(
    db: &impl ConnectionTrait,
    table: &str,
    id: i64,
) -> AppResult<Option<String>> {
    let sql = format!("SELECT entity_sync_id FROM {table} WHERE id = ? LIMIT 1");
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            &sql,
            [id.into()],
        ))
        .await?;
    Ok(row
        .map(|r| r.try_get::<String>("", "entity_sync_id"))
        .transpose()
        .ok()
        .flatten()
        .filter(|s| !s.is_empty()))
}

pub async fn stage_intervention_request_sync(
    db: &DatabaseConnection,
    di_id: i64,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT ir.entity_sync_id, ir.code, ir.title, ir.description, ir.status, ir.origin_type,
                    ir.row_version, ir.updated_at, ir.equipment_id, ir.org_node_id, ir.submitter_id
             FROM intervention_requests ir WHERE ir.id = ?",
            [di_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let code: String = row.try_get("", "code").unwrap_or_default();
    let esid: String = row
        .try_get("", "entity_sync_id")
        .ok()
        .filter(|s: &String| !s.is_empty())
        .unwrap_or_else(|| code.clone());
    let row_version: i64 = row.try_get("", "row_version").unwrap_or(1);
    let equipment_id: i64 = row.try_get("", "equipment_id").unwrap_or(0);
    let org_node_id: i64 = row.try_get("", "org_node_id").unwrap_or(0);
    let submitter_id: i64 = row.try_get("", "submitter_id").unwrap_or(0);
    let payload = InterventionRequestSyncPayload {
        entity_sync_id: esid.clone(),
        row_version,
        code,
        title: row.try_get("", "title").unwrap_or_default(),
        description: row.try_get("", "description").unwrap_or_default(),
        status: row.try_get("", "status").unwrap_or_default(),
        origin_type: row.try_get("", "origin_type").unwrap_or_default(),
        equipment_entity_sync_id: if equipment_id > 0 {
            lookup_entity_sync_id(db, "equipment", equipment_id).await?
        } else {
            None
        },
        org_node_entity_sync_id: if org_node_id > 0 {
            lookup_entity_sync_id(db, "org_nodes", org_node_id).await?
        } else {
            None
        },
        submitter_entity_sync_id: if submitter_id > 0 {
            lookup_entity_sync_id(db, "user_accounts", submitter_id).await?
        } else {
            None
        },
        updated_at: row.try_get("", "updated_at").ok(),
    };
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("intervention_requests:{esid}:v{row_version}"),
            entity_type: SYNC_ENTITY_INTERVENTION_REQUESTS.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            row_version,
            payload_json: serde_json::to_string(&payload)?,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

pub async fn stage_pm_plan_sync(db: &DatabaseConnection, plan_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT p.entity_sync_id, p.code, p.title, p.description, p.strategy_type,
                    p.asset_scope_type, p.lifecycle_status, p.is_active, p.row_version, p.updated_at,
                    p.assigned_team_id
             FROM pm_plans p WHERE p.id = ?",
            [plan_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let code: String = row.try_get("", "code").unwrap_or_default();
    let esid: String = row
        .try_get("", "entity_sync_id")
        .ok()
        .filter(|s: &String| !s.is_empty())
        .unwrap_or_else(|| code.clone());
    let row_version: i64 = row.try_get("", "row_version").unwrap_or(1);
    let is_active: i64 = row.try_get("", "is_active").unwrap_or(1);
    let team_id: Option<i64> = row.try_get("", "assigned_team_id").ok();
    let payload = PmPlanSyncPayload {
        entity_sync_id: esid.clone(),
        row_version,
        code,
        title: row.try_get("", "title").unwrap_or_default(),
        description: row.try_get("", "description").ok(),
        strategy_type: row.try_get("", "strategy_type").unwrap_or_default(),
        asset_scope_type: row.try_get("", "asset_scope_type").unwrap_or_default(),
        assigned_team_entity_sync_id: match team_id {
            Some(id) => lookup_entity_sync_id(db, "teams", id).await?,
            None => None,
        },
        lifecycle_status: row
            .try_get("", "lifecycle_status")
            .unwrap_or_else(|_| "draft".into()),
        is_active: is_active != 0,
        updated_at: row.try_get("", "updated_at").ok(),
    };
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("pm_plans:{esid}:v{row_version}"),
            entity_type: SYNC_ENTITY_PM_PLANS.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            row_version,
            payload_json: serde_json::to_string(&payload)?,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

pub async fn stage_article_sync(db: &DatabaseConnection, article_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT entity_sync_id, article_code, article_name, is_active, unit_value_id,
                    row_version, updated_at
             FROM articles WHERE id = ?",
            [article_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let code: String = row.try_get("", "article_code").unwrap_or_default();
    let esid: String = row
        .try_get("", "entity_sync_id")
        .ok()
        .filter(|s: &String| !s.is_empty())
        .unwrap_or_else(|| code.clone());
    let row_version: i64 = row.try_get("", "row_version").unwrap_or(1);
    let is_active: i64 = row.try_get("", "is_active").unwrap_or(1);
    let payload = ArticleSyncPayload {
        entity_sync_id: esid.clone(),
        row_version,
        article_code: code,
        article_name: row.try_get("", "article_name").unwrap_or_default(),
        is_active: is_active != 0,
        unit_value_id: row.try_get("", "unit_value_id").ok(),
        updated_at: row.try_get("", "updated_at").ok(),
    };
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("articles:{esid}:v{row_version}"),
            entity_type: SYNC_ENTITY_ARTICLES.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            row_version,
            payload_json: serde_json::to_string(&payload)?,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

pub async fn stage_warehouse_sync(db: &DatabaseConnection, warehouse_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT entity_sync_id, code, name, is_active, row_version, updated_at
             FROM warehouses WHERE id = ?",
            [warehouse_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let code: String = row.try_get("", "code").unwrap_or_default();
    let esid: String = row
        .try_get("", "entity_sync_id")
        .ok()
        .filter(|s: &String| !s.is_empty())
        .unwrap_or_else(|| code.clone());
    let row_version: i64 = row.try_get("", "row_version").unwrap_or(1);
    let is_active: i64 = row.try_get("", "is_active").unwrap_or(1);
    let payload = WarehouseSyncPayload {
        entity_sync_id: esid.clone(),
        row_version,
        code,
        name: row.try_get("", "name").unwrap_or_default(),
        is_active: is_active != 0,
        updated_at: row.try_get("", "updated_at").ok(),
    };
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("warehouses:{esid}:v{row_version}"),
            entity_type: SYNC_ENTITY_WAREHOUSES.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            row_version,
            payload_json: serde_json::to_string(&payload)?,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

pub async fn stage_stock_location_sync(db: &DatabaseConnection, location_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT l.entity_sync_id, l.code, l.name, l.is_active, l.row_version, l.updated_at,
                    w.entity_sync_id AS warehouse_entity_sync_id
             FROM stock_locations l
             JOIN warehouses w ON w.id = l.warehouse_id
             WHERE l.id = ?",
            [location_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let code: String = row.try_get("", "code").unwrap_or_default();
    let esid: String = row
        .try_get("", "entity_sync_id")
        .ok()
        .filter(|s: &String| !s.is_empty())
        .unwrap_or_else(|| code.clone());
    let row_version: i64 = row.try_get("", "row_version").unwrap_or(1);
    let is_active: i64 = row.try_get("", "is_active").unwrap_or(1);
    let warehouse_esid: String = row
        .try_get("", "warehouse_entity_sync_id")
        .unwrap_or_default();
    if warehouse_esid.is_empty() {
        return Ok(());
    }
    let payload = StockLocationSyncPayload {
        entity_sync_id: esid.clone(),
        row_version,
        code,
        name: row.try_get("", "name").unwrap_or_default(),
        warehouse_entity_sync_id: warehouse_esid,
        is_active: is_active != 0,
        updated_at: row.try_get("", "updated_at").ok(),
    };
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("stock_locations:{esid}:v{row_version}"),
            entity_type: SYNC_ENTITY_STOCK_LOCATIONS.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            row_version,
            payload_json: serde_json::to_string(&payload)?,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

pub async fn stage_stock_reservation_sync(
    db: &DatabaseConnection,
    reservation_id: i64,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT r.entity_sync_id, r.quantity_reserved, r.status, r.row_version, r.updated_at,
                    r.source_type, r.source_id,
                    a.entity_sync_id AS article_entity_sync_id,
                    w.entity_sync_id AS warehouse_entity_sync_id,
                    l.entity_sync_id AS location_entity_sync_id,
                    wo.entity_sync_id AS work_order_entity_sync_id
             FROM stock_reservations r
             JOIN articles a ON a.id = r.article_id
             JOIN warehouses w ON w.id = r.warehouse_id
             JOIN stock_locations l ON l.id = r.location_id
             LEFT JOIN work_orders wo
               ON wo.id = r.source_id
              AND r.source_type IN ('WORK_ORDER', 'WORK_ORDER_PART')
             WHERE r.id = ?",
            [reservation_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let esid: String = row
        .try_get("", "entity_sync_id")
        .ok()
        .filter(|s: &String| !s.is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let row_version: i64 = row.try_get("", "row_version").unwrap_or(1);
    let article_esid: String = row.try_get("", "article_entity_sync_id").unwrap_or_default();
    let warehouse_esid: String = row
        .try_get("", "warehouse_entity_sync_id")
        .unwrap_or_default();
    let location_esid: String = row.try_get("", "location_entity_sync_id").unwrap_or_default();
    if article_esid.is_empty() || warehouse_esid.is_empty() || location_esid.is_empty() {
        return Ok(());
    }
    let qty: f64 = row.try_get("", "quantity_reserved").unwrap_or(0.0);
    let payload = StockReservationSyncPayload {
        entity_sync_id: esid.clone(),
        row_version,
        article_entity_sync_id: article_esid,
        warehouse_entity_sync_id: warehouse_esid,
        location_entity_sync_id: location_esid,
        work_order_entity_sync_id: row
            .try_get::<String>("", "work_order_entity_sync_id")
            .ok()
            .filter(|s| !s.is_empty()),
        quantity: qty,
        status: row.try_get("", "status").unwrap_or_default(),
        updated_at: row.try_get("", "updated_at").ok(),
    };
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("stock_reservations:{esid}:v{row_version}"),
            entity_type: SYNC_ENTITY_STOCK_RESERVATIONS.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            row_version,
            payload_json: serde_json::to_string(&payload)?,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

pub async fn stage_stock_movement_sync(
    db: &DatabaseConnection,
    transaction_id: i64,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT t.entity_sync_id, t.movement_type, t.quantity, t.source_type, t.reason,
                    t.row_version,
                    a.entity_sync_id AS article_entity_sync_id,
                    w.entity_sync_id AS warehouse_entity_sync_id,
                    l.entity_sync_id AS location_entity_sync_id
             FROM inventory_transactions t
             JOIN articles a ON a.id = t.article_id
             JOIN warehouses w ON w.id = t.warehouse_id
             JOIN stock_locations l ON l.id = t.location_id
             WHERE t.id = ?",
            [transaction_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let esid: String = row
        .try_get("", "entity_sync_id")
        .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());
    let row_version: i64 = row.try_get("", "row_version").unwrap_or(1);
    let article_esid: String = row.try_get("", "article_entity_sync_id").unwrap_or_default();
    let warehouse_esid: String = row
        .try_get("", "warehouse_entity_sync_id")
        .unwrap_or_default();
    let location_esid: String = row.try_get("", "location_entity_sync_id").unwrap_or_default();
    if article_esid.is_empty() || warehouse_esid.is_empty() || location_esid.is_empty() {
        return Ok(());
    }
    let qty: f64 = row.try_get("", "quantity").unwrap_or(0.0);
    let payload = StockMovementSyncPayload {
        entity_sync_id: esid.clone(),
        row_version,
        article_entity_sync_id: article_esid,
        warehouse_entity_sync_id: warehouse_esid,
        location_entity_sync_id: location_esid,
        movement_type: row.try_get("", "movement_type").unwrap_or_default(),
        quantity: qty,
        source_type: row.try_get("", "source_type").unwrap_or_else(|_| "sync".into()),
        notes: row.try_get("", "reason").ok(),
    };
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("inventory_transactions:{esid}:v{row_version}"),
            entity_type: SYNC_ENTITY_STOCK_MOVEMENTS.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            row_version,
            payload_json: serde_json::to_string(&payload)?,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

pub async fn stage_media_attachment_sync(
    db: &DatabaseConnection,
    payload: MediaAttachmentSyncPayload,
) -> AppResult<()> {
    let esid = payload.entity_sync_id.clone();
    let row_version = payload.row_version;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("media_attachments:{esid}:v{row_version}"),
            entity_type: SYNC_ENTITY_MEDIA_ATTACHMENTS.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            row_version,
            payload_json: serde_json::to_string(&payload)?,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

pub async fn stage_pm_plan_version_sync(
    db: &DatabaseConnection,
    version_id: i64,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT v.entity_sync_id, v.version_no, v.status, v.effective_from,
                    v.trigger_definition_json, v.row_version, v.updated_at,
                    p.entity_sync_id AS pm_plan_entity_sync_id
             FROM pm_plan_versions v
             JOIN pm_plans p ON p.id = v.pm_plan_id
             WHERE v.id = ?",
            [version_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let plan_esid: String = row
        .try_get("", "pm_plan_entity_sync_id")
        .unwrap_or_default();
    if plan_esid.is_empty() {
        return Ok(());
    }
    let esid: String = row
        .try_get("", "entity_sync_id")
        .ok()
        .filter(|s: &String| !s.is_empty())
        .unwrap_or_else(|| format!("{plan_esid}:v{}", row.try_get::<i64>("", "version_no").unwrap_or(0)));
    let row_version: i64 = row.try_get("", "row_version").unwrap_or(1);
    let payload = PmPlanVersionSyncPayload {
        entity_sync_id: esid.clone(),
        row_version,
        pm_plan_entity_sync_id: plan_esid,
        version_no: row.try_get("", "version_no").unwrap_or(1),
        status: row.try_get("", "status").unwrap_or_else(|_| "draft".into()),
        effective_from: row.try_get("", "effective_from").ok(),
        trigger_definition_json: row.try_get("", "trigger_definition_json").ok(),
        updated_at: row.try_get("", "updated_at").ok(),
    };
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("pm_plan_versions:{esid}:v{row_version}"),
            entity_type: SYNC_ENTITY_PM_PLAN_VERSIONS.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            row_version,
            payload_json: serde_json::to_string(&payload)?,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

pub async fn stage_pm_occurrence_sync(
    db: &DatabaseConnection,
    occurrence_id: i64,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT o.entity_sync_id, o.due_basis, o.status, o.due_at, o.row_version,
                    p.entity_sync_id AS pm_plan_entity_sync_id,
                    v.entity_sync_id AS pm_plan_version_entity_sync_id,
                    w.entity_sync_id AS linked_work_order_entity_sync_id
             FROM pm_occurrences o
             JOIN pm_plans p ON p.id = o.pm_plan_id
             JOIN pm_plan_versions v ON v.id = o.plan_version_id
             LEFT JOIN work_orders w ON w.id = o.linked_work_order_id
             WHERE o.id = ?",
            [occurrence_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let plan_esid: String = row
        .try_get("", "pm_plan_entity_sync_id")
        .unwrap_or_default();
    let ver_esid: String = row
        .try_get("", "pm_plan_version_entity_sync_id")
        .unwrap_or_default();
    if plan_esid.is_empty() || ver_esid.is_empty() {
        return Ok(());
    }
    let esid: String = row
        .try_get("", "entity_sync_id")
        .ok()
        .filter(|s: &String| !s.is_empty())
        .unwrap_or_else(|| format!("{plan_esid}:occ-{occurrence_id}"));
    let row_version: i64 = row.try_get("", "row_version").unwrap_or(1);
    let payload = PmOccurrenceSyncPayload {
        entity_sync_id: esid.clone(),
        row_version,
        pm_plan_entity_sync_id: plan_esid,
        pm_plan_version_entity_sync_id: ver_esid,
        due_basis: row
            .try_get("", "due_basis")
            .unwrap_or_else(|_| "calendar".into()),
        status: row
            .try_get("", "status")
            .unwrap_or_else(|_| "generated".into()),
        due_at: row.try_get("", "due_at").ok(),
        linked_work_order_entity_sync_id: row.try_get("", "linked_work_order_entity_sync_id").ok(),
    };
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("pm_occurrences:{esid}:v{row_version}"),
            entity_type: SYNC_ENTITY_PM_OCCURRENCES.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            row_version,
            payload_json: serde_json::to_string(&payload)?,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}
