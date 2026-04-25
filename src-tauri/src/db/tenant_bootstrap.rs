use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TryGetable};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::errors::AppResult;
use crate::settings;

const TENANT_BOOTSTRAP_KEY: &str = "tenant.initial_bootstrap";
const TENANT_BOOTSTRAP_SCOPE: &str = "device";
const TENANT_BOOTSTRAP_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TenantBootstrapState {
    version: u32,
    tenant_id: String,
    root_org_node_id: i64,
    demo_seeded: bool,
    bootstrapped_at: String,
}

pub async fn bootstrap_from_activation_claim(db: &DatabaseConnection, changed_by_id: i32) -> AppResult<()> {
    let Some(claim) = crate::commands::product_license::get_activation_claim_record(db).await? else {
        return Ok(());
    };
    // Tenant-global guard from VPS: if already initialized elsewhere, skip all local bootstrap seeding.
    if claim.is_initialized.unwrap_or(false) {
        return Ok(());
    }
    let tenant_id = claim.tenant_id.trim();
    if tenant_id.is_empty() {
        return Ok(());
    }

    let now = Utc::now().to_rfc3339();
    let root_name = normalize_root_name(claim.tenant_display_name.as_deref());
    let root_id = ensure_root_organization(db, tenant_id, &root_name, &now).await?;
    let want_demo_data = claim.has_demo_data.unwrap_or(false);

    let previous_state = load_bootstrap_state(db).await?;
    let already_demo_seeded = previous_state
        .as_ref()
        .map(|s| s.tenant_id == tenant_id && s.demo_seeded)
        .unwrap_or(false);

    if want_demo_data && !already_demo_seeded {
        seed_generic_demo_sandbox(db, tenant_id, root_id, &now).await?;
    }

    let state = TenantBootstrapState {
        version: TENANT_BOOTSTRAP_VERSION,
        tenant_id: tenant_id.to_string(),
        root_org_node_id: root_id,
        demo_seeded: want_demo_data || already_demo_seeded,
        bootstrapped_at: now,
    };
    settings::set_setting(
        db,
        TENANT_BOOTSTRAP_KEY,
        TENANT_BOOTSTRAP_SCOPE,
        &serde_json::to_string(&state)?,
        changed_by_id,
        "tenant activation bootstrap updated",
    )
    .await?;

    Ok(())
}

async fn load_bootstrap_state(db: &DatabaseConnection) -> AppResult<Option<TenantBootstrapState>> {
    let setting = settings::get_setting(db, TENANT_BOOTSTRAP_KEY, TENANT_BOOTSTRAP_SCOPE).await?;
    let Some(setting) = setting else {
        return Ok(None);
    };
    let parsed = serde_json::from_str::<TenantBootstrapState>(&setting.setting_value_json).ok();
    Ok(parsed)
}

async fn ensure_root_organization(
    db: &DatabaseConnection,
    tenant_id: &str,
    tenant_name: &str,
    now: &str,
) -> AppResult<i64> {
    let existing_root = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM org_nodes WHERE parent_id IS NULL AND deleted_at IS NULL ORDER BY id LIMIT 1".to_string(),
        ))
        .await?;
    if let Some(row) = existing_root {
        let id = try_get_i64(&row, "id")?;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE org_nodes SET name = ?, updated_at = ? WHERE id = ?",
            [tenant_name.into(), now.into(), id.into()],
        ))
        .await?;
        return Ok(id);
    }

    let model_id = ensure_active_org_model(db, now).await?;
    let root_type_id = ensure_root_node_type(db, model_id, now).await?;
    let root_code = format!("TENANT-ROOT-{}", code_fragment(tenant_id));

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO org_nodes
            (sync_id, code, name, node_type_id, parent_id, ancestor_path, depth, status, created_at, updated_at, row_version)
          VALUES (?, ?, ?, ?, NULL, '/', 0, 'active', ?, ?, 1)",
        [
            Uuid::new_v4().to_string().into(),
            root_code.into(),
            tenant_name.into(),
            root_type_id.into(),
            now.into(),
            now.into(),
        ],
    ))
    .await?;
    let root_id = last_insert_id(db).await?;
    let full_path = format!("/{root_id}/");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE org_nodes SET ancestor_path = ?, updated_at = ? WHERE id = ?",
        [full_path.into(), now.into(), root_id.into()],
    ))
    .await?;
    Ok(root_id)
}

async fn ensure_active_org_model(db: &DatabaseConnection, now: &str) -> AppResult<i64> {
    let existing = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM org_structure_models WHERE status = 'active' ORDER BY version_number DESC, id DESC LIMIT 1"
                .to_string(),
        ))
        .await?;
    if let Some(row) = existing {
        return Ok(try_get_i64(&row, "id")?);
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO org_structure_models
            (sync_id, version_number, status, description, activated_at, created_at, updated_at)
          VALUES (?, 1, 'active', ?, ?, ?, ?)",
        [
            Uuid::new_v4().to_string().into(),
            "Tenant activation bootstrap".into(),
            now.into(),
            now.into(),
            now.into(),
        ],
    ))
    .await?;
    last_insert_id(db).await
}

async fn ensure_root_node_type(db: &DatabaseConnection, model_id: i64, now: &str) -> AppResult<i64> {
    let existing = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM org_node_types WHERE structure_model_id = ? AND is_root_type = 1 AND is_active = 1 LIMIT 1",
            [model_id.into()],
        ))
        .await?;
    if let Some(row) = existing {
        return Ok(try_get_i64(&row, "id")?);
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO org_node_types
            (sync_id, structure_model_id, code, label, can_host_assets, can_own_work,
             can_carry_cost_center, can_aggregate_kpis, can_receive_permits,
             is_root_type, is_active, created_at, updated_at)
          VALUES (?, ?, 'ROOT', 'Root Organization', 1, 1, 1, 1, 1, 1, 1, ?, ?)",
        [Uuid::new_v4().to_string().into(), model_id.into(), now.into(), now.into()],
    ))
    .await?;
    last_insert_id(db).await
}

async fn seed_generic_demo_sandbox(
    db: &DatabaseConnection,
    tenant_id: &str,
    root_id: i64,
    now: &str,
) -> AppResult<()> {
    let model_id = ensure_active_org_model(db, now).await?;
    let zone_type_id = ensure_node_type(
        db,
        model_id,
        "ZONE",
        "Zone",
        false,
        true,
        false,
        now,
    )
    .await?;
    let line_type_id = ensure_node_type(
        db,
        model_id,
        "LINE",
        "Line",
        true,
        true,
        false,
        now,
    )
    .await?;

    let suffix = code_fragment(tenant_id);
    let zone_code = format!("GEN-ZONE-1-{suffix}");
    let line_code = format!("GEN-LINE-A-{suffix}");
    let zone_id = ensure_org_node(db, &zone_code, "Zone 1", zone_type_id, Some(root_id), 1, now).await?;
    let line_id = ensure_org_node(db, &line_code, "Line A", line_type_id, Some(zone_id), 2, now).await?;

    let class_pump = ensure_equipment_class(db, "GEN-CLASS-PUMP", "Pump", None, "class", now).await?;
    let class_motor = ensure_equipment_class(db, "GEN-CLASS-MOTOR", "Motor", None, "class", now).await?;

    let pump_id = ensure_equipment(
        db,
        &format!("GEN-PUMP-001-{suffix}"),
        "Standard Pump",
        Some(class_pump),
        line_id,
        "Generic",
        now,
    )
    .await?;
    let motor_id = ensure_equipment(
        db,
        &format!("GEN-MOTOR-001-{suffix}"),
        "General Motor",
        Some(class_motor),
        line_id,
        "Generic",
        now,
    )
    .await?;

    let submitter_id = resolve_submitter_id(db).await?;
    ensure_di(
        db,
        &format!("GEN-DI-001-{suffix}"),
        "Template vibration check",
        "Investigate unusual vibration on Standard Pump.",
        pump_id,
        line_id,
        submitter_id,
        "medium",
        now,
    )
    .await?;
    ensure_di(
        db,
        &format!("GEN-DI-002-{suffix}"),
        "Template overheating check",
        "Verify temperature trend on General Motor.",
        motor_id,
        line_id,
        submitter_id,
        "high",
        now,
    )
    .await?;

    let draft_status_id = resolve_work_order_status_id(db, "draft").await?;
    let corrective_type_id = resolve_work_order_type_id(db, "corrective").await?;
    let preventive_type_id = resolve_work_order_type_id(db, "preventive").await?;
    let urgency_medium_id = resolve_urgency_id(db, 3).await?;
    let urgency_high_id = resolve_urgency_id(db, 4).await?;

    let wo_1 = ensure_work_order(
        db,
        &format!("GEN-WO-001-{suffix}"),
        corrective_type_id,
        draft_status_id,
        Some(pump_id),
        root_id,
        submitter_id,
        urgency_high_id,
        "Template corrective task",
        "Inspect coupling alignment and bearing condition for Standard Pump.",
        now,
    )
    .await?;
    let wo_2 = ensure_work_order(
        db,
        &format!("GEN-WO-002-{suffix}"),
        preventive_type_id,
        draft_status_id,
        Some(motor_id),
        root_id,
        submitter_id,
        urgency_medium_id,
        "Template preventive task",
        "Run routine checks and lubrication for General Motor.",
        now,
    )
    .await?;

    ensure_wo_transition(db, wo_1, submitter_id, now).await?;
    ensure_wo_transition(db, wo_2, submitter_id, now).await?;
    Ok(())
}

async fn ensure_node_type(
    db: &DatabaseConnection,
    model_id: i64,
    code: &str,
    label: &str,
    can_host_assets: bool,
    can_own_work: bool,
    is_root_type: bool,
    now: &str,
) -> AppResult<i64> {
    let existing = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM org_node_types WHERE structure_model_id = ? AND code = ? LIMIT 1",
            [model_id.into(), code.into()],
        ))
        .await?;
    if let Some(row) = existing {
        return Ok(try_get_i64(&row, "id")?);
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO org_node_types
            (sync_id, structure_model_id, code, label, can_host_assets, can_own_work,
             can_carry_cost_center, can_aggregate_kpis, can_receive_permits,
             is_root_type, is_active, created_at, updated_at)
          VALUES (?, ?, ?, ?, ?, ?, 0, 1, 0, ?, 1, ?, ?)",
        [
            Uuid::new_v4().to_string().into(),
            model_id.into(),
            code.into(),
            label.into(),
            bool_to_i32(can_host_assets).into(),
            bool_to_i32(can_own_work).into(),
            bool_to_i32(is_root_type).into(),
            now.into(),
            now.into(),
        ],
    ))
    .await?;
    last_insert_id(db).await
}

async fn ensure_org_node(
    db: &DatabaseConnection,
    code: &str,
    name: &str,
    node_type_id: i64,
    parent_id: Option<i64>,
    depth: i32,
    now: &str,
) -> AppResult<i64> {
    let existing = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM org_nodes WHERE code = ? AND deleted_at IS NULL LIMIT 1",
            [code.into()],
        ))
        .await?;
    if let Some(row) = existing {
        return Ok(try_get_i64(&row, "id")?);
    }

    let ancestor_path = if let Some(pid) = parent_id {
        format!("/{pid}/")
    } else {
        "/".to_string()
    };
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO org_nodes
            (sync_id, code, name, node_type_id, parent_id, ancestor_path, depth, status, created_at, updated_at, row_version)
          VALUES (?, ?, ?, ?, ?, ?, ?, 'active', ?, ?, 1)",
        [
            Uuid::new_v4().to_string().into(),
            code.into(),
            name.into(),
            node_type_id.into(),
            parent_id.map_or(sea_orm::Value::String(None), |v| v.into()),
            ancestor_path.into(),
            depth.into(),
            now.into(),
            now.into(),
        ],
    ))
    .await?;
    let id = last_insert_id(db).await?;
    let full_path = if let Some(pid) = parent_id {
        format!("/{pid}/{id}/")
    } else {
        format!("/{id}/")
    };
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE org_nodes SET ancestor_path = ?, updated_at = ? WHERE id = ?",
        [full_path.into(), now.into(), id.into()],
    ))
    .await?;
    Ok(id)
}

async fn ensure_equipment_class(
    db: &DatabaseConnection,
    code: &str,
    name: &str,
    parent_id: Option<i64>,
    level: &str,
    now: &str,
) -> AppResult<i64> {
    let existing = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM equipment_classes WHERE code = ? LIMIT 1",
            [code.into()],
        ))
        .await?;
    if let Some(row) = existing {
        return Ok(try_get_i64(&row, "id")?);
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO equipment_classes
            (sync_id, code, name, parent_id, level, is_active, created_at, updated_at)
          VALUES (?, ?, ?, ?, ?, 1, ?, ?)",
        [
            Uuid::new_v4().to_string().into(),
            code.into(),
            name.into(),
            parent_id.map_or(sea_orm::Value::String(None), |v| v.into()),
            level.into(),
            now.into(),
            now.into(),
        ],
    ))
    .await?;
    last_insert_id(db).await
}

async fn ensure_equipment(
    db: &DatabaseConnection,
    asset_code: &str,
    name: &str,
    class_id: Option<i64>,
    node_id: i64,
    manufacturer: &str,
    now: &str,
) -> AppResult<i64> {
    let existing = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM equipment WHERE asset_id_code = ? LIMIT 1",
            [asset_code.into()],
        ))
        .await?;
    if let Some(row) = existing {
        return Ok(try_get_i64(&row, "id")?);
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO equipment
            (sync_id, asset_id_code, name, class_id, lifecycle_status, installed_at_node_id,
             manufacturer, created_at, updated_at, row_version)
          VALUES (?, ?, ?, ?, 'active_in_service', ?, ?, ?, ?, 1)",
        [
            Uuid::new_v4().to_string().into(),
            asset_code.into(),
            name.into(),
            class_id.map_or(sea_orm::Value::String(None), |v| v.into()),
            node_id.into(),
            manufacturer.into(),
            now.into(),
            now.into(),
        ],
    ))
    .await?;
    last_insert_id(db).await
}

async fn ensure_di(
    db: &DatabaseConnection,
    code: &str,
    title: &str,
    description: &str,
    asset_id: i64,
    org_node_id: i64,
    submitter_id: i64,
    urgency: &str,
    now: &str,
) -> AppResult<()> {
    let exists = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM intervention_requests WHERE code = ? LIMIT 1",
            [code.into()],
        ))
        .await?;
    if exists.is_some() {
        return Ok(());
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO intervention_requests
            (code, asset_id, org_node_id, status, title, description, origin_type,
             impact_level, production_impact, safety_flag, environmental_flag, quality_flag,
             reported_urgency, submitted_at, submitter_id, row_version, created_at, updated_at)
          VALUES (?, ?, ?, 'submitted', ?, ?, 'operator', 'unknown', 0, 0, 0, 0, ?, ?, ?, 1, ?, ?)",
        [
            code.into(),
            asset_id.into(),
            org_node_id.into(),
            title.into(),
            description.into(),
            urgency.into(),
            now.into(),
            submitter_id.into(),
            now.into(),
            now.into(),
        ],
    ))
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn ensure_work_order(
    db: &DatabaseConnection,
    code: &str,
    type_id: i64,
    status_id: i64,
    equipment_id: Option<i64>,
    entity_id: i64,
    requester_id: i64,
    urgency_id: Option<i64>,
    title: &str,
    description: &str,
    now: &str,
) -> AppResult<i64> {
    let existing = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM work_orders WHERE code = ? LIMIT 1",
            [code.into()],
        ))
        .await?;
    if let Some(row) = existing {
        return Ok(try_get_i64(&row, "id")?);
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO work_orders (
            code, type_id, status_id, equipment_id, entity_id,
            requester_id, planner_id, urgency_id, title, description,
            row_version, created_at, updated_at
          ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
        [
            code.into(),
            type_id.into(),
            status_id.into(),
            equipment_id.map_or(sea_orm::Value::String(None), |v| v.into()),
            entity_id.into(),
            requester_id.into(),
            requester_id.into(),
            urgency_id.map_or(sea_orm::Value::String(None), |v| v.into()),
            title.into(),
            description.into(),
            now.into(),
            now.into(),
        ],
    ))
    .await?;
    last_insert_id(db).await
}

async fn ensure_wo_transition(db: &DatabaseConnection, wo_id: i64, actor_id: i64, now: &str) -> AppResult<()> {
    let exists = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM wo_state_transition_log WHERE wo_id = ? AND action = 'create' LIMIT 1",
            [wo_id.into()],
        ))
        .await?;
    if exists.is_some() {
        return Ok(());
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO wo_state_transition_log (wo_id, from_status, to_status, action, actor_id, acted_at) VALUES (?, 'none', 'draft', 'create', ?, ?)",
        [wo_id.into(), actor_id.into(), now.into()],
    ))
    .await?;
    Ok(())
}

async fn resolve_submitter_id(db: &DatabaseConnection) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE deleted_at IS NULL ORDER BY CASE WHEN LOWER(username) = 'admin' THEN 0 ELSE 1 END, id LIMIT 1"
                .to_string(),
        ))
        .await?;
    Ok(row
        .and_then(|r| i64::try_get(&r, "", "id").ok())
        .unwrap_or(1))
}

async fn resolve_work_order_status_id(db: &DatabaseConnection, code: &str) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM work_order_statuses WHERE code = ? LIMIT 1",
            [code.into()],
        ))
        .await?;
    Ok(row.and_then(|r| i64::try_get(&r, "", "id").ok()).unwrap_or(1))
}

async fn resolve_work_order_type_id(db: &DatabaseConnection, code: &str) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM work_order_types WHERE code = ? LIMIT 1",
            [code.into()],
        ))
        .await?;
    Ok(row.and_then(|r| i64::try_get(&r, "", "id").ok()).unwrap_or(1))
}

async fn resolve_urgency_id(db: &DatabaseConnection, level: i64) -> AppResult<Option<i64>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM urgency_levels WHERE level = ? LIMIT 1",
            [level.into()],
        ))
        .await?;
    Ok(row.and_then(|r| i64::try_get(&r, "", "id").ok()))
}

async fn last_insert_id(db: &DatabaseConnection) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| anyhow::anyhow!("missing last_insert_rowid() row"))?;
    let id = try_get_i64(&row, "id")?;
    Ok(id)
}

fn normalize_root_name(raw: Option<&str>) -> String {
    let value = raw.unwrap_or("").trim();
    if value.is_empty() {
        "Parent Company".to_string()
    } else {
        value.to_string()
    }
}

fn code_fragment(value: &str) -> String {
    let mut out = String::new();
    let mut previous_dash = false;
    for ch in value.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_uppercase()
        } else {
            '-'
        };
        if mapped == '-' {
            if previous_dash {
                continue;
            }
            previous_dash = true;
            out.push('-');
        } else {
            previous_dash = false;
            out.push(mapped);
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "TENANT".to_string()
    } else {
        trimmed.to_string()
    }
}

fn bool_to_i32(value: bool) -> i32 {
    if value {
        1
    } else {
        0
    }
}

fn try_get_i64(row: &sea_orm::QueryResult, column: &str) -> AppResult<i64> {
    i64::try_get(row, "", column).map_err(|e| anyhow::anyhow!("{e:?}").into())
}
