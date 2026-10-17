//! SYSTEM / COMPANY audience of reference domains (migration 196, `reference::audience`).
//!
//! - backfill: every domain gets its audience, only the Maintafox lists are SYSTEM;
//! - guard: every user-facing write on a SYSTEM domain is refused with
//!   `GATE_REF_SYSTEM_DOMAIN_READ_ONLY`, before the A/B/C rules;
//! - a COMPANY domain stays editable, the seeders still write SYSTEM domains, and the read paths the
//!   pickers use are unchanged.

use std::collections::BTreeSet;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::AppError;
use crate::reference::audience::{
    assert_module_catalog_editable, module_catalog_audience, DomainAudience, GATE_REF_SYSTEM_DOMAIN_READ_ONLY,
    MODULE_CATALOGS,
};
use crate::reference::{
    aliases, domains, governance, imports, migrations as ref_migrations, publish, schedule_patterns, search, sets,
    values,
};

const SYSTEM_CODES: &[&str] = &["DI.DISPOSITION", "EQUIPMENT.CRITICALITY", "EQUIPMENT.STATUS"];
/// SYSTEM lists added by migration 198 (shared urgency scale, OT types and their natures).
const SYSTEM_CODES_198: &[&str] = &["WORK.MAINTENANCE_NATURES", "WORK.ORDER_TYPES", "WORK.URGENCY"];
/// SYSTEM list added by migration 201 (physical dimensions of the units of measure).
const SYSTEM_CODES_201: &[&str] = &["MEASURE.DIMENSIONS"];
/// Planning window types (migration 211).
const SYSTEM_CODES_211: &[&str] = &["PLANNING.WINDOW_TYPES"];
/// LOTO energy sources (migration 225).
const SYSTEM_CODES_225: &[&str] = &["PERMIT.ENERGY_TYPES"];

fn assert_gate<T: std::fmt::Debug>(result: Result<T, AppError>, what: &str) {
    match result {
        Err(AppError::ValidationFailed(msgs)) => assert!(
            msgs.iter().any(|m| m.starts_with(GATE_REF_SYSTEM_DOMAIN_READ_ONLY)),
            "{what}: expected {GATE_REF_SYSTEM_DOMAIN_READ_ONLY}, got {msgs:?}"
        ),
        other => panic!("{what}: expected {GATE_REF_SYSTEM_DOMAIN_READ_ONLY}, got {other:?}"),
    }
}

async fn scalar_i64(db: &DatabaseConnection, sql: &str) -> i64 {
    let row = db
        .query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("query")
        .expect("row");
    row.try_get_by_index::<i64>(0).expect("i64")
}

async fn exec(db: &DatabaseConnection, sql: &str) {
    db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("exec");
}

async fn audience_by_code(db: &DatabaseConnection) -> Vec<(String, String)> {
    db.query_all(Statement::from_string(
        DbBackend::Sqlite,
        "SELECT code, audience FROM reference_domains ORDER BY code".to_string(),
    ))
    .await
    .expect("domains")
    .iter()
    .map(|r| (r.try_get("", "code").unwrap(), r.try_get("", "audience").unwrap()))
    .collect()
}

async fn published_value_id(db: &DatabaseConnection, domain_code: &str) -> i64 {
    scalar_i64(
        db,
        &format!(
            "SELECT v.id FROM reference_values v JOIN reference_sets s ON s.id = v.set_id \
               JOIN reference_domains d ON d.id = s.domain_id \
              WHERE d.code = '{domain_code}' AND s.status = 'published' AND v.is_active = 1 \
              ORDER BY v.sort_order, v.id LIMIT 1"
        ),
    )
    .await
}

fn new_value(set_id: i64, code: &str, label: &str) -> values::CreateReferenceValuePayload {
    values::CreateReferenceValuePayload {
        set_id,
        parent_id: None,
        code: code.into(),
        label: label.into(),
        description: None,
        sort_order: None,
        color_hex: None,
        icon_name: None,
        semantic_tag: None,
        external_code: None,
        metadata_json: None,
    }
}

fn label_update(label: &str) -> values::UpdateReferenceValuePayload {
    values::UpdateReferenceValuePayload {
        label: Some(label.into()),
        description: None,
        sort_order: None,
        color_hex: None,
        icon_name: None,
        semantic_tag: None,
        external_code: None,
        metadata_json: None,
    }
}

// ─── Backfill (migration 196) ─────────────────────────────────────────────────

#[tokio::test]
async fn backfill_marks_only_the_maintafox_lists_system() {
    let db = crate::db::test_db::migrated().await;
    let rows = audience_by_code(&db).await;
    assert!(!rows.is_empty(), "migrations seed reference domains");

    let system: BTreeSet<&str> = rows
        .iter()
        .filter(|(_, a)| a == "system")
        .map(|(c, _)| c.as_str())
        .collect();
    assert_eq!(
        system,
        SYSTEM_CODES
            .iter()
            .chain(SYSTEM_CODES_198)
            .chain(SYSTEM_CODES_201)
            .chain(SYSTEM_CODES_211)
            .chain(SYSTEM_CODES_225)
            .copied()
            .collect::<BTreeSet<_>>()
    );
    for (code, audience) in &rows {
        assert!(
            audience == "system" || audience == "company",
            "{code} has audience {audience}"
        );
    }
    for company in [
        "EQUIPMENT.CLASS",
        "EQUIPMENT.FAMILY",
        "ORG.SCHEDULE_CLASS",
        "WORK.SYMPTOMS",
        "WORK.FAILURE_MODES",
    ] {
        assert!(
            rows.iter().any(|(c, a)| c == company && a == "company"),
            "{company} must be a company list"
        );
    }
}

#[tokio::test]
async fn audience_column_rejects_other_values() {
    let db = crate::db::test_db::migrated().await;
    let err = db
        .execute(Statement::from_string(
            DbBackend::Sqlite,
            "UPDATE reference_domains SET audience = 'tenant' WHERE code = 'WORK.SYMPTOMS'".to_string(),
        ))
        .await
        .expect_err("CHECK constraint");
    assert!(err.to_string().contains("CHECK"), "{err}");
}

#[tokio::test]
async fn domain_reads_carry_the_audience() {
    let db = crate::db::test_db::seeded().await;
    let all = domains::list_reference_domains(&db).await.expect("list");
    let status = all.iter().find(|d| d.code == "EQUIPMENT.STATUS").expect("status");
    assert_eq!(status.audience, DomainAudience::System);
    let class = domains::get_reference_domain_by_code(&db, "EQUIPMENT.CLASS")
        .await
        .expect("class");
    assert_eq!(class.audience, DomainAudience::Company);
    let json = serde_json::to_value(status).expect("json");
    assert_eq!(json["audience"], "system");
}

// ─── Guard on SYSTEM domains ──────────────────────────────────────────────────

/// A company list built through the normal commands (so every step proves a COMPANY domain is
/// editable), then reclassified SYSTEM the way a release would, and every write tried again.
#[tokio::test]
async fn every_user_write_on_a_system_domain_is_refused() {
    let db = crate::db::test_db::seeded().await;

    // ── COMPANY: everything below is accepted ──
    let domain = domains::create_reference_domain(
        &db,
        domains::CreateReferenceDomainPayload {
            code: "TEST.AUDIENCE".into(),
            name: "Audience test".into(),
            structure_type: "hierarchical".into(),
            governance_level: "tenant_managed".into(),
            governance_category: Some("controlled_catalog".into()),
            is_extendable: Some(true),
            validation_rules_json: None,
        },
        1,
    )
    .await
    .expect("create domain");
    assert_eq!(domain.audience, DomainAudience::Company, "user domains are COMPANY");

    let draft = sets::create_draft_set(&db, domain.id, 1).await.expect("draft");
    let v1 = values::create_value(&db, new_value(draft.id, "V1", "Value 1"), 1)
        .await
        .expect("v1");
    let v2 = values::create_value(&db, new_value(draft.id, "V2", "Value 2"), 1)
        .await
        .expect("v2");
    let v3 = values::create_value(&db, new_value(draft.id, "V3", "Value 3"), 1)
        .await
        .expect("v3");
    values::deactivate_value(&db, v3.id, 1).await.expect("deactivate v3");
    values::update_value(&db, v1.id, label_update("Value one"), 1)
        .await
        .expect("update v1");
    let alias = aliases::create_alias(
        &db,
        aliases::CreateReferenceAliasPayload {
            reference_value_id: v1.id,
            alias_label: "Valeur 1".into(),
            locale: "fr".into(),
            alias_type: "legacy".into(),
            is_preferred: Some(false),
        },
        1,
    )
    .await
    .expect("alias");
    let batch = imports::create_import_batch(&db, domain.id, "values.csv", "abc", Some(1))
        .await
        .expect("import batch");
    let caps = governance::capabilities_for(&domain, Some(&draft));
    assert!(caps.can_create_value && !caps.is_read_only);
    assert_eq!(caps.audience, "company");

    // ── Reclassified SYSTEM by the product ──
    exec(
        &db,
        &format!(
            "UPDATE reference_domains SET audience = 'system' WHERE id = {}",
            domain.id
        ),
    )
    .await;
    let domain = domains::get_reference_domain(&db, domain.id).await.expect("reload");
    let caps = governance::capabilities_for(&domain, Some(&draft));
    assert!(caps.is_read_only);
    assert!(!caps.can_create_value && !caps.can_update_value && !caps.can_deactivate_value);
    assert!(!caps.can_create_draft_set && !caps.can_discard_draft_set);
    assert!(!caps.can_publish && !caps.can_operational_create);
    assert_eq!(caps.audience, "system");

    // Domain
    assert_gate(
        domains::update_reference_domain(
            &db,
            domain.id,
            domains::UpdateReferenceDomainPayload {
                name: Some("Renamed".into()),
                structure_type: None,
                governance_level: None,
                governance_category: None,
                is_extendable: None,
                validation_rules_json: None,
            },
            1,
        )
        .await,
        "update domain",
    );

    // Sets
    assert_gate(sets::create_draft_set(&db, domain.id, 1).await, "create draft set");
    assert_gate(sets::discard_draft_set(&db, draft.id).await, "discard draft set");
    assert_gate(sets::validate_set(&db, draft.id, 1).await, "validate set");
    assert_gate(sets::publish_set(&db, draft.id, 1).await, "publish set");
    assert_gate(
        publish::publish_reference_set(&db, draft.id, 1).await,
        "governed publish",
    );

    // Values
    assert_gate(
        values::create_value(&db, new_value(draft.id, "V4", "Value 4"), 1).await,
        "create value",
    );
    assert_gate(
        values::create_operational_value(
            &db,
            values::CreateOperationalReferenceValuePayload {
                domain_code: "TEST.AUDIENCE".into(),
                label: "Quick".into(),
                description: None,
                parent_id: None,
                code: None,
            },
            1,
        )
        .await,
        "operational create",
    );
    assert_gate(
        values::update_value(&db, v1.id, label_update("Edited"), 1).await,
        "update value",
    );
    assert_gate(values::deactivate_value(&db, v2.id, 1).await, "deactivate value");
    assert_gate(values::reactivate_value(&db, v3.id, 1).await, "reactivate value");
    assert_gate(
        values::move_value_parent(&db, v2.id, Some(v1.id), 1).await,
        "move value",
    );

    // Aliases
    assert_gate(
        aliases::create_alias(
            &db,
            aliases::CreateReferenceAliasPayload {
                reference_value_id: v2.id,
                alias_label: "Valeur 2".into(),
                locale: "fr".into(),
                alias_type: "legacy".into(),
                is_preferred: Some(false),
            },
            1,
        )
        .await,
        "create alias",
    );
    assert_gate(
        aliases::update_alias(
            &db,
            alias.id,
            aliases::UpdateReferenceAliasPayload {
                alias_label: Some("Valeur un".into()),
                locale: None,
                alias_type: None,
                is_preferred: None,
            },
            1,
        )
        .await,
        "update alias",
    );
    assert_gate(aliases::delete_alias(&db, alias.id, 1).await, "delete alias");

    // Imports
    assert_gate(
        imports::create_import_batch(&db, domain.id, "more.csv", "def", Some(1)).await,
        "create import batch",
    );
    assert_gate(
        imports::stage_import_rows(&db, batch.id, Vec::new()).await,
        "stage import rows",
    );
    assert_gate(
        imports::validate_import_batch(&db, batch.id, Some(1)).await,
        "validate import batch",
    );
    assert_gate(
        imports::apply_import_batch(
            &db,
            batch.id,
            imports::RefImportApplyPolicy {
                include_warnings: false,
                target_set_id: draft.id,
            },
            1,
        )
        .await,
        "apply import batch",
    );

    // Merge / migrate (retire a value into another)
    assert_gate(
        ref_migrations::merge_reference_values(&db, domain.id, v2.id, v1.id, 1).await,
        "merge values",
    );
    assert_gate(
        ref_migrations::migrate_reference_usage(&db, domain.id, v2.id, v1.id, 1).await,
        "migrate usage",
    );

    // Nothing changed.
    let reloaded = values::get_value(&db, v1.id).await.expect("v1");
    assert_eq!(reloaded.label, "Value one");
    assert!(values::get_value(&db, v2.id).await.expect("v2").is_active);
    assert_eq!(
        aliases::get_alias(&db, alias.id).await.expect("alias").alias_label,
        "Valeur 1"
    );
}

/// The seeded SYSTEM lists are refused even where their A/B/C category alone would allow the write
/// (`DI.DISPOSITION` is a B working dictionary: live edits on the published set).
#[tokio::test]
async fn seeded_system_lists_are_read_only_for_users() {
    let db = crate::db::test_db::seeded().await;
    let disposition = domains::get_reference_domain_by_code(&db, "DI.DISPOSITION")
        .await
        .expect("DI.DISPOSITION");
    assert_eq!(disposition.audience, DomainAudience::System);
    assert_eq!(
        governance::category_of(&disposition),
        governance::GovernanceCategory::OperationalDictionary
    );
    let published = sets::list_sets_for_domain(&db, disposition.id)
        .await
        .expect("sets")
        .into_iter()
        .find(|s| s.status == "published")
        .expect("published set");
    assert_gate(
        values::create_value(&db, new_value(published.id, "plant_specific", "Plant specific"), 1).await,
        "create value in DI.DISPOSITION",
    );
    assert_gate(
        values::create_operational_value(
            &db,
            values::CreateOperationalReferenceValuePayload {
                domain_code: "DI.DISPOSITION".into(),
                label: "Plant specific".into(),
                description: None,
                parent_id: None,
                code: None,
            },
            1,
        )
        .await,
        "operational create in DI.DISPOSITION",
    );
    let value_id = published_value_id(&db, "DI.DISPOSITION").await;
    assert_gate(
        values::update_value(&db, value_id, label_update("Renamed"), 1).await,
        "update DI.DISPOSITION value",
    );

    for code in ["EQUIPMENT.STATUS", "EQUIPMENT.CRITICALITY"] {
        let domain = domains::get_reference_domain_by_code(&db, code).await.expect(code);
        assert_gate(sets::create_draft_set(&db, domain.id, 1).await, code);
        let caps = governance::capabilities_for(&domain, None);
        assert!(caps.is_read_only && !caps.can_operational_create, "{code}");
        assert_eq!(caps.audience, "system");
    }

    // The schedule pattern editor writes through the same guard.
    let schedule_value = published_value_id(&db, "ORG.SCHEDULE_CLASS").await;
    exec(
        &db,
        "UPDATE reference_domains SET audience = 'system' WHERE code = 'ORG.SCHEDULE_CLASS'",
    )
    .await;
    assert_gate(
        schedule_patterns::upsert_schedule_pattern(
            &db,
            schedule_patterns::UpsertSchedulePatternPayload {
                reference_value_id: schedule_value,
                shift_pattern_code: None,
                is_continuous: Some(true),
                nominal_hours_per_day: None,
                details: None,
            },
            1,
        )
        .await,
        "schedule pattern",
    );
}

#[tokio::test]
async fn company_working_dictionary_stays_editable() {
    let db = crate::db::test_db::seeded().await;
    let symptom = domains::get_reference_domain_by_code(&db, "DI.ORIGIN")
        .await
        .expect("DI.ORIGIN");
    assert_eq!(symptom.audience, DomainAudience::Company);
    let created = values::create_operational_value(
        &db,
        values::CreateOperationalReferenceValuePayload {
            domain_code: "DI.ORIGIN".into(),
            label: "Sous-traitant".into(),
            description: None,
            parent_id: None,
            code: None,
        },
        1,
    )
    .await
    .expect("plant symptom");
    values::update_value(&db, created.id, label_update("Sous-traitant de maintenance"), 1)
        .await
        .expect("edit plant origin");
    let caps = governance::capabilities_for(&symptom, None);
    assert!(caps.can_operational_create && !caps.is_read_only);
    assert_eq!(caps.audience, "company");
}

// ─── Pickers and seeders are unaffected ──────────────────────────────────────

#[tokio::test]
async fn pickers_still_read_system_lists() {
    let db = crate::db::test_db::seeded().await;
    let hits = search::search_reference_values(&db, "DI.DISPOSITION", "dupl", "fr", 10)
        .await
        .expect("search");
    assert!(hits.iter().any(|h| h.code == "duplicate"), "{hits:?}");

    let status = domains::get_reference_domain_by_code(&db, "EQUIPMENT.STATUS")
        .await
        .expect("status");
    let published = sets::list_sets_for_domain(&db, status.id)
        .await
        .expect("sets")
        .into_iter()
        .find(|s| s.status == "published")
        .expect("published");
    let listed = values::list_values(&db, published.id).await.expect("values");
    assert!(listed.iter().any(|v| v.code == "DECOMMISSIONED"));
    assert!(
        domains::list_reference_domains(&db)
            .await
            .expect("list")
            .iter()
            .any(|d| d.code == "EQUIPMENT.STATUS"),
        "the engine keeps listing SYSTEM domains (stable ids for pickers and links)"
    );
}

#[tokio::test]
async fn integrity_pass_still_writes_system_lists_and_restores_their_audience() {
    let db = crate::db::test_db::seeded().await;
    // Drift: a seeded status value disappears and audiences were changed on a replica.
    exec(
        &db,
        "DELETE FROM reference_values WHERE code = 'SPARE' AND set_id IN \
           (SELECT s.id FROM reference_sets s JOIN reference_domains d ON d.id = s.domain_id \
             WHERE d.code = 'EQUIPMENT.STATUS')",
    )
    .await;
    exec(
        &db,
        "UPDATE reference_domains SET audience = CASE code \
            WHEN 'EQUIPMENT.STATUS' THEN 'company' WHEN 'DI.ORIGIN' THEN 'system' \
            WHEN 'WORK.SYMPTOMS' THEN 'system' WHEN 'WORK.URGENCY' THEN 'company' \
            WHEN 'WORK.ORDER_TYPES' THEN 'company' ELSE audience END",
    )
    .await;

    crate::reference::system_catalog_integrity::ensure_system_reference_catalog_integrity(&db)
        .await
        .expect("integrity pass");

    let spare = scalar_i64(
        &db,
        "SELECT COUNT(*) FROM reference_values v JOIN reference_sets s ON s.id = v.set_id \
           JOIN reference_domains d ON d.id = s.domain_id \
          WHERE d.code = 'EQUIPMENT.STATUS' AND v.code = 'SPARE'",
    )
    .await;
    assert_eq!(spare, 1, "the seeder writes the SYSTEM list");
    let rows = audience_by_code(&db).await;
    let of = |code: &str| rows.iter().find(|(c, _)| c == code).map(|(_, a)| a.clone());
    assert_eq!(of("EQUIPMENT.STATUS").as_deref(), Some("system"));
    assert_eq!(of("EQUIPMENT.CRITICALITY").as_deref(), Some("system"));
    assert_eq!(of("DI.ORIGIN").as_deref(), Some("company"));
    assert_eq!(of("WORK.SYMPTOMS").as_deref(), Some("company"));
    assert_eq!(of("WORK.URGENCY").as_deref(), Some("system"));
    assert_eq!(of("WORK.ORDER_TYPES").as_deref(), Some("system"));
}

// ─── Module catalogs (OT types, statuses, article families…) ─────────────────

#[test]
fn module_catalog_audiences() {
    assert_eq!(
        module_catalog_audience("WO.WORK_ORDER_STATUSES"),
        Some(DomainAudience::System)
    );
    // The plant's view of the shared urgency scale (labels and target times only).
    assert_eq!(
        module_catalog_audience("WORK.URGENCY_SCALE"),
        Some(DomainAudience::Company)
    );
    assert_eq!(
        module_catalog_audience("INVENTORY.ARTICLE_FAMILY"),
        Some(DomainAudience::Company)
    );
    assert_eq!(
        module_catalog_audience("INVENTORY.TAX_CATEGORY"),
        Some(DomainAudience::Company)
    );
    // OT types are a SYSTEM reference domain since migration 198, no longer a module table.
    assert_eq!(module_catalog_audience("WO.WORK_ORDER_TYPES"), None);
    // Notification categories are the program's; their routing and escalation the plant's.
    assert_eq!(
        module_catalog_audience("NOTIFICATION.CATEGORIES"),
        Some(DomainAudience::System)
    );
    assert_eq!(
        module_catalog_audience("NOTIFICATION.RULES"),
        Some(DomainAudience::Company)
    );
    assert_eq!(MODULE_CATALOGS.len(), 6);

    assert_gate(assert_module_catalog_editable("WO.WORK_ORDER_STATUSES"), "OT statuses");
    assert!(assert_module_catalog_editable("INVENTORY.ARTICLE_FAMILY").is_ok());
    assert!(assert_module_catalog_editable("WORK.URGENCY_SCALE").is_ok());
    assert!(matches!(
        assert_module_catalog_editable("WO.UNKNOWN"),
        Err(AppError::Internal(_))
    ));

    let json = serde_json::to_value(MODULE_CATALOGS[3]).expect("json");
    assert_eq!(json["code"], "WO.WORK_ORDER_STATUSES");
    assert_eq!(json["audience"], "system");
}
