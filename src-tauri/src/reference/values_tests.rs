//! Supervisor verification tests for Phase 2 SP03 File 01 Sprint S3.
//!
//! V1 — Value code uniqueness within a set
//! V2 — Hierarchy cycle detection and parent validation
//! V3 — Draft-only mutation guard (published sets immutable)

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::auth::rbac::{self, PermissionScope};
    use crate::errors::AppError;
    use crate::reference::domains::{self, CreateReferenceDomainPayload};
    use crate::reference::sets;
    use crate::reference::values::{self, CreateReferenceValuePayload, UpdateReferenceValuePayload};

    /// In-memory SQLite with all migrations + seed data.
    async fn setup() -> sea_orm::DatabaseConnection {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("in-memory SQLite should connect");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA foreign_keys = ON;".to_string(),
        ))
        .await
        .expect("PRAGMA foreign_keys");

        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("migrations should apply cleanly");

        crate::db::seeder::seed_system_data(&db)
            .await
            .expect("seeder should run cleanly");

        db
    }

    /// Creates a hierarchical domain and returns its id.
    async fn setup_domain(db: &sea_orm::DatabaseConnection) -> i64 {
        let payload = CreateReferenceDomainPayload {
            code: "FAILURE_CLASS".to_string(),
            name: "Failure Classes".to_string(),
            structure_type: "hierarchical".to_string(),
            governance_level: "protected_analytical".to_string(),
            is_extendable: Some(false),
            validation_rules_json: None,
        };
        domains::create_reference_domain(db, payload, 1)
            .await
            .expect("create domain")
            .id
    }

    /// Creates a domain + draft set, returns (domain_id, set_id).
    async fn setup_draft_set(db: &sea_orm::DatabaseConnection) -> (i64, i64) {
        let domain_id = setup_domain(db).await;
        let set = sets::create_draft_set(db, domain_id, 1)
            .await
            .expect("create draft set");
        (domain_id, set.id)
    }

    fn value_payload(set_id: i64, code: &str, label: &str) -> CreateReferenceValuePayload {
        CreateReferenceValuePayload {
            set_id,
            parent_id: None,
            code: code.to_string(),
            label: label.to_string(),
            description: None,
            sort_order: None,
            color_hex: None,
            icon_name: None,
            semantic_tag: None,
            external_code: None,
            metadata_json: None,
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V1 — Value code uniqueness within a set
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v1_create_value_succeeds() {
        let db = setup().await;
        let (_dom, set_id) = setup_draft_set(&db).await;

        let v = values::create_value(&db, value_payload(set_id, "MECH", "Mécanique"), 1)
            .await
            .expect("first value");

        assert_eq!(v.code, "MECH");
        assert_eq!(v.label, "Mécanique");
        assert!(v.is_active);
        assert!(v.parent_id.is_none());
    }

    #[tokio::test]
    async fn v1_duplicate_code_in_same_set_rejected() {
        let db = setup().await;
        let (_dom, set_id) = setup_draft_set(&db).await;

        values::create_value(&db, value_payload(set_id, "ELEC", "Électrique"), 1)
            .await
            .expect("first");

        let err = values::create_value(&db, value_payload(set_id, "ELEC", "Électrique 2"), 1)
            .await
            .expect_err("duplicate code");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn v1_same_code_different_set_ok() {
        let db = setup().await;
        let (domain_id, set1_id) = setup_draft_set(&db).await;

        values::create_value(&db, value_payload(set1_id, "MECH", "Mécanique"), 1)
            .await
            .expect("set1");

        // Publish set1 to allow a new draft
        sets::validate_set(&db, set1_id, 1).await.expect("validate");
        sets::publish_set(&db, set1_id, 1).await.expect("publish");

        let set2 = sets::create_draft_set(&db, domain_id, 1)
            .await
            .expect("draft v2");

        let v = values::create_value(&db, value_payload(set2.id, "MECH", "Mécanique v2"), 1)
            .await
            .expect("same code in set2");

        assert_eq!(v.code, "MECH");
    }

    #[tokio::test]
    async fn v1_code_normalized_to_uppercase() {
        let db = setup().await;
        let (_dom, set_id) = setup_draft_set(&db).await;

        let v = values::create_value(&db, value_payload(set_id, "low_case", "Test"), 1)
            .await
            .expect("normalized");

        assert_eq!(v.code, "LOW_CASE");
    }

    #[tokio::test]
    async fn v1_empty_code_rejected() {
        let db = setup().await;
        let (_dom, set_id) = setup_draft_set(&db).await;

        let err = values::create_value(&db, value_payload(set_id, "", "Label"), 1)
            .await
            .expect_err("empty code");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn v1_empty_label_rejected() {
        let db = setup().await;
        let (_dom, set_id) = setup_draft_set(&db).await;

        let err = values::create_value(&db, value_payload(set_id, "GOOD", ""), 1)
            .await
            .expect_err("empty label");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn v1_list_values_ordered() {
        let db = setup().await;
        let (_dom, set_id) = setup_draft_set(&db).await;

        let mut p1 = value_payload(set_id, "ZEBRA", "Z item");
        p1.sort_order = Some(2);
        let mut p2 = value_payload(set_id, "ALPHA", "A item");
        p2.sort_order = Some(1);

        values::create_value(&db, p1, 1).await.expect("z");
        values::create_value(&db, p2, 1).await.expect("a");

        let list = values::list_values(&db, set_id).await.expect("list");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].code, "ALPHA"); // sort_order 1
        assert_eq!(list[1].code, "ZEBRA"); // sort_order 2
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V2 — Hierarchy cycle detection and parent validation
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v2_create_with_parent() {
        let db = setup().await;
        let (_dom, set_id) = setup_draft_set(&db).await;

        let parent = values::create_value(&db, value_payload(set_id, "ROOT", "Root"), 1)
            .await
            .expect("parent");

        let mut child_payload = value_payload(set_id, "CHILD", "Child");
        child_payload.parent_id = Some(parent.id);

        let child = values::create_value(&db, child_payload, 1)
            .await
            .expect("child");

        assert_eq!(child.parent_id, Some(parent.id));
    }

    #[tokio::test]
    async fn v2_parent_must_be_in_same_set() {
        let db = setup().await;
        let (domain_id, set1_id) = setup_draft_set(&db).await;

        let v1 = values::create_value(&db, value_payload(set1_id, "ROOT", "Root"), 1)
            .await
            .expect("in set1");

        // Publish set1, create set2
        sets::validate_set(&db, set1_id, 1).await.expect("validate");
        sets::publish_set(&db, set1_id, 1).await.expect("publish");
        let set2 = sets::create_draft_set(&db, domain_id, 1).await.expect("set2");

        let mut cross_set_payload = value_payload(set2.id, "CROSS", "Cross-set");
        cross_set_payload.parent_id = Some(v1.id);

        let err = values::create_value(&db, cross_set_payload, 1)
            .await
            .expect_err("cross-set parent");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn v2_parent_not_found_rejected() {
        let db = setup().await;
        let (_dom, set_id) = setup_draft_set(&db).await;

        let mut p = value_payload(set_id, "ORPHAN", "Orphan");
        p.parent_id = Some(999_999);

        let err = values::create_value(&db, p, 1)
            .await
            .expect_err("nonexistent parent");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn v2_self_parent_rejected_on_move() {
        let db = setup().await;
        let (_dom, set_id) = setup_draft_set(&db).await;

        let v = values::create_value(&db, value_payload(set_id, "SELF", "Selfloop"), 1)
            .await
            .expect("created");

        let err = values::move_value_parent(&db, v.id, Some(v.id), 1)
            .await
            .expect_err("self-parent");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn v2_cycle_detection_a_b_a() {
        let db = setup().await;
        let (_dom, set_id) = setup_draft_set(&db).await;

        // Create A → B hierarchy
        let a = values::create_value(&db, value_payload(set_id, "A", "Node A"), 1)
            .await
            .expect("A");

        let mut bp = value_payload(set_id, "B", "Node B");
        bp.parent_id = Some(a.id);
        let b = values::create_value(&db, bp, 1).await.expect("B");

        // Try moving A under B → cycle A→B→A
        let err = values::move_value_parent(&db, a.id, Some(b.id), 1)
            .await
            .expect_err("cycle A→B→A");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn v2_cycle_detection_deep_chain() {
        let db = setup().await;
        let (_dom, set_id) = setup_draft_set(&db).await;

        // Create A → B → C chain
        let a = values::create_value(&db, value_payload(set_id, "A", "A"), 1)
            .await
            .expect("A");

        let mut bp = value_payload(set_id, "B", "B");
        bp.parent_id = Some(a.id);
        let b = values::create_value(&db, bp, 1).await.expect("B");

        let mut cp = value_payload(set_id, "C", "C");
        cp.parent_id = Some(b.id);
        let c = values::create_value(&db, cp, 1).await.expect("C");

        // Try moving A under C → cycle A→B→C→A
        let err = values::move_value_parent(&db, a.id, Some(c.id), 1)
            .await
            .expect_err("deep cycle");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn v2_move_to_root_succeeds() {
        let db = setup().await;
        let (_dom, set_id) = setup_draft_set(&db).await;

        let a = values::create_value(&db, value_payload(set_id, "A", "A"), 1)
            .await
            .expect("A");

        let mut bp = value_payload(set_id, "B", "B");
        bp.parent_id = Some(a.id);
        let b = values::create_value(&db, bp, 1).await.expect("B");

        // Move B to root (no parent)
        let moved = values::move_value_parent(&db, b.id, None, 1)
            .await
            .expect("move to root");

        assert!(moved.parent_id.is_none());
    }

    #[tokio::test]
    async fn v2_move_to_sibling_succeeds() {
        let db = setup().await;
        let (_dom, set_id) = setup_draft_set(&db).await;

        let a = values::create_value(&db, value_payload(set_id, "A", "A"), 1)
            .await
            .expect("A");
        let b = values::create_value(&db, value_payload(set_id, "B", "B"), 1)
            .await
            .expect("B");
        let c = values::create_value(&db, value_payload(set_id, "C", "C"), 1)
            .await
            .expect("C");

        // Move C under B (B is not a descendant of C, so OK)
        let moved = values::move_value_parent(&db, c.id, Some(b.id), 1)
            .await
            .expect("move to sibling");

        assert_eq!(moved.parent_id, Some(b.id));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V3 — Draft-only mutation guard (published sets immutable)
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn v3_create_value_in_published_set_rejected() {
        let db = setup().await;
        let (_dom, set_id) = setup_draft_set(&db).await;

        // Add a value, then publish
        values::create_value(&db, value_payload(set_id, "V1", "Val1"), 1)
            .await
            .expect("v1");
        sets::validate_set(&db, set_id, 1).await.expect("validate");
        sets::publish_set(&db, set_id, 1).await.expect("publish");

        // Try to add another value to the now-published set
        let err = values::create_value(&db, value_payload(set_id, "V2", "Val2"), 1)
            .await
            .expect_err("published set");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn v3_update_value_in_published_set_rejected() {
        let db = setup().await;
        let (_dom, set_id) = setup_draft_set(&db).await;

        let v = values::create_value(&db, value_payload(set_id, "V1", "Val1"), 1)
            .await
            .expect("v1");

        sets::validate_set(&db, set_id, 1).await.expect("validate");
        sets::publish_set(&db, set_id, 1).await.expect("publish");

        let update = UpdateReferenceValuePayload {
            label: Some("Changed".into()),
            description: None,
            sort_order: None,
            color_hex: None,
            icon_name: None,
            semantic_tag: None,
            external_code: None,
            metadata_json: None,
        };

        let err = values::update_value(&db, v.id, update, 1)
            .await
            .expect_err("published set update");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn v3_deactivate_in_published_set_rejected() {
        let db = setup().await;
        let (_dom, set_id) = setup_draft_set(&db).await;

        let v = values::create_value(&db, value_payload(set_id, "V1", "Active"), 1)
            .await
            .expect("v1");

        sets::validate_set(&db, set_id, 1).await.expect("validate");
        sets::publish_set(&db, set_id, 1).await.expect("publish");

        let err = values::deactivate_value(&db, v.id, 1)
            .await
            .expect_err("deactivate in published");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn v3_move_value_in_published_set_rejected() {
        let db = setup().await;
        let (_dom, set_id) = setup_draft_set(&db).await;

        let a = values::create_value(&db, value_payload(set_id, "A", "A"), 1)
            .await
            .expect("A");
        let b = values::create_value(&db, value_payload(set_id, "B", "B"), 1)
            .await
            .expect("B");

        sets::validate_set(&db, set_id, 1).await.expect("validate");
        sets::publish_set(&db, set_id, 1).await.expect("publish");

        let err = values::move_value_parent(&db, a.id, Some(b.id), 1)
            .await
            .expect_err("move in published");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Edge cases
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn edge_update_partial_fields() {
        let db = setup().await;
        let (_dom, set_id) = setup_draft_set(&db).await;

        let v = values::create_value(&db, value_payload(set_id, "UPD", "Original"), 1)
            .await
            .expect("created");

        let update = UpdateReferenceValuePayload {
            label: Some("Updated Label".into()),
            description: None,
            sort_order: None,
            color_hex: Some(Some("#FF0000".into())),
            icon_name: None,
            semantic_tag: None,
            external_code: None,
            metadata_json: None,
        };

        let updated = values::update_value(&db, v.id, update, 1)
            .await
            .expect("update");

        assert_eq!(updated.label, "Updated Label");
        assert_eq!(updated.color_hex.as_deref(), Some("#FF0000"));
    }

    #[tokio::test]
    async fn edge_deactivate_already_inactive_rejected() {
        let db = setup().await;
        let (_dom, set_id) = setup_draft_set(&db).await;

        let v = values::create_value(&db, value_payload(set_id, "DEACT", "Deactivatable"), 1)
            .await
            .expect("created");

        values::deactivate_value(&db, v.id, 1)
            .await
            .expect("first deactivation");

        let err = values::deactivate_value(&db, v.id, 1)
            .await
            .expect_err("already inactive");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn edge_get_nonexistent_value() {
        let db = setup().await;

        let err = values::get_value(&db, 999_999)
            .await
            .expect_err("not found");

        assert!(matches!(err, AppError::NotFound { .. }));
    }

    #[tokio::test]
    async fn edge_update_noop_returns_existing() {
        let db = setup().await;
        let (_dom, set_id) = setup_draft_set(&db).await;

        let v = values::create_value(&db, value_payload(set_id, "NOOP", "Noop"), 1)
            .await
            .expect("created");

        let update = UpdateReferenceValuePayload {
            label: None,
            description: None,
            sort_order: None,
            color_hex: None,
            icon_name: None,
            semantic_tag: None,
            external_code: None,
            metadata_json: None,
        };

        let same = values::update_value(&db, v.id, update, 1)
            .await
            .expect("noop update");

        assert_eq!(same.id, v.id);
        assert_eq!(same.label, "Noop");
    }

    #[tokio::test]
    async fn edge_metadata_json_stored() {
        let db = setup().await;
        let (_dom, set_id) = setup_draft_set(&db).await;

        let mut p = value_payload(set_id, "META", "With metadata");
        p.metadata_json = Some(r#"{"key": "value"}"#.to_string());

        let v = values::create_value(&db, p, 1).await.expect("created");
        assert_eq!(v.metadata_json.as_deref(), Some(r#"{"key": "value"}"#));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // V3 — Permission split (ref.view / ref.manage / ref.publish)
    //
    // Follows the V7 pattern from import_tests.rs:
    //   - seed data provides roles & permissions
    //   - manually INSERT user_scope_assignments to bind user → role
    //   - call rbac::check_permission() directly to verify access
    // ═══════════════════════════════════════════════════════════════════════

    /// Assigns a user to a role by name at tenant scope.
    async fn assign_role(db: &sea_orm::DatabaseConnection, user_id: i32, role_name: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO user_scope_assignments \
             (sync_id, user_id, role_id, scope_type, created_at, updated_at) \
             VALUES ('test-assign-' || ?, ?, \
               (SELECT id FROM roles WHERE name = ?), \
               'tenant', ?, ?)",
            [
                user_id.into(),
                user_id.into(),
                role_name.into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await
        .expect("insert user_scope_assignment");
    }

    #[tokio::test]
    async fn v3_unassigned_user_denied_all_ref_permissions() {
        let db = setup().await;
        // User 999 has no scope assignments
        let view = rbac::check_permission(&db, 999, "ref.view", &PermissionScope::Global)
            .await
            .expect("check");
        let manage = rbac::check_permission(&db, 999, "ref.manage", &PermissionScope::Global)
            .await
            .expect("check");
        let publish = rbac::check_permission(&db, 999, "ref.publish", &PermissionScope::Global)
            .await
            .expect("check");

        assert!(!view, "unassigned user must not have ref.view");
        assert!(!manage, "unassigned user must not have ref.manage");
        assert!(!publish, "unassigned user must not have ref.publish");
    }

    #[tokio::test]
    async fn v3_operator_has_view_only() {
        let db = setup().await;
        assign_role(&db, 10, "Operator").await;

        let view = rbac::check_permission(&db, 10, "ref.view", &PermissionScope::Global)
            .await
            .expect("check");
        let manage = rbac::check_permission(&db, 10, "ref.manage", &PermissionScope::Global)
            .await
            .expect("check");
        let publish = rbac::check_permission(&db, 10, "ref.publish", &PermissionScope::Global)
            .await
            .expect("check");

        assert!(view, "Operator must have ref.view");
        assert!(!manage, "Operator must NOT have ref.manage");
        assert!(!publish, "Operator must NOT have ref.publish");
    }

    #[tokio::test]
    async fn v3_administrator_has_all_ref_permissions() {
        let db = setup().await;
        assign_role(&db, 20, "Administrator").await;

        let view = rbac::check_permission(&db, 20, "ref.view", &PermissionScope::Global)
            .await
            .expect("check");
        let manage = rbac::check_permission(&db, 20, "ref.manage", &PermissionScope::Global)
            .await
            .expect("check");
        let publish = rbac::check_permission(&db, 20, "ref.publish", &PermissionScope::Global)
            .await
            .expect("check");

        assert!(view, "Administrator must have ref.view");
        assert!(manage, "Administrator must have ref.manage");
        assert!(publish, "Administrator must have ref.publish");
    }

    #[tokio::test]
    async fn v3_ref_publish_requires_step_up() {
        let db = setup().await;

        // ref.publish is flagged requires_step_up in seeder
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT requires_step_up FROM permissions WHERE name = ?",
                ["ref.publish".into()],
            ))
            .await
            .expect("query")
            .expect("permission must exist");
        let requires: i64 = row.try_get("", "requires_step_up").expect("column");
        assert_eq!(requires, 1, "ref.publish must require step-up authentication");
    }

    #[tokio::test]
    async fn v3_ref_manage_does_not_require_step_up() {
        let db = setup().await;

        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT requires_step_up FROM permissions WHERE name = ?",
                ["ref.manage".into()],
            ))
            .await
            .expect("query")
            .expect("permission must exist");
        let requires: i64 = row.try_get("", "requires_step_up").expect("column");
        assert_eq!(requires, 0, "ref.manage must NOT require step-up");
    }

    #[tokio::test]
    async fn v3_readonly_has_view_only() {
        let db = setup().await;
        assign_role(&db, 30, "Readonly").await;

        let view = rbac::check_permission(&db, 30, "ref.view", &PermissionScope::Global)
            .await
            .expect("check");
        let manage = rbac::check_permission(&db, 30, "ref.manage", &PermissionScope::Global)
            .await
            .expect("check");
        let publish = rbac::check_permission(&db, 30, "ref.publish", &PermissionScope::Global)
            .await
            .expect("check");

        assert!(view, "Readonly must have ref.view");
        assert!(!manage, "Readonly must NOT have ref.manage");
        assert!(!publish, "Readonly must NOT have ref.publish");
    }
}
