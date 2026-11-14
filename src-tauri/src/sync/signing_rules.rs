//! Who must sign a synchronised change (per-user signing, sync-signing-design.md v9 §5).
//!
//! The sync server refuses an item unless every *author field* that the item sets or changes,
//! and every author field one of whose *guarded* columns changes, names a user who signed the
//! item; the PCs re-check the same rule on arrival with the server-attested signers. This file is
//! the single list of those fields, exported to `shared/sync/entity-registry.json`
//! (`author_fields`, `authority`, `server_only_types`) and from there to the control-plane
//! allowlist by `scripts/sync-allowlist-append.mjs`.
//!
//! Every column of a synchronised table that refers to `user_accounts` (declared foreign key,
//! `row_spec` reference, polymorphic reference or JSON reference) is classified here exactly once:
//! either as an author field (the person who did, decided or approved something) or as
//! `not_attribution` (the person the row is about: an assignee, a recipient, an owner). The test
//! `every_user_reference_is_classified` fails on any unclassified or stale column.
//!
//! `authority` is the right a signer must hold plant-wide (judged by the server in its own state)
//! to change a row of that type at all.

use crate::rbac::permissions;

/// One author field: the local column (a reference to `user_accounts`) and the payload keys of
/// the columns of the same row this person vouches for.
#[derive(Debug, Clone, Copy)]
pub struct AuthorField {
    pub column: &'static str,
    /// Payload keys (wire names): changing one of them needs this author's signature.
    pub guards: &'static [&'static str],
    /// Payload keys of content this person approved: changing one of them needs this author's
    /// signature unless the same change clears the author (the approval is withdrawn). Unlike a
    /// guard, it may keep its value when the author is cleared (server rule 4).
    pub vouches: &'static [&'static str],
}

#[derive(Debug, Clone, Copy)]
pub struct SigningRule {
    pub entity_type: &'static str,
    pub author_fields: &'static [AuthorField],
    /// User-reference columns that name the subject of the row, not who made the change
    /// (including polymorphic and JSON columns holding user ids).
    pub not_attribution: &'static [&'static str],
    /// Right the maker must hold plant-wide in the server's state to change this type.
    pub authority: Option<&'static str>,
}

const fn author(column: &'static str, guards: &'static [&'static str]) -> AuthorField {
    AuthorField {
        column,
        guards,
        vouches: &[],
    }
}

const fn author_vouching(
    column: &'static str,
    guards: &'static [&'static str],
    vouches: &'static [&'static str],
) -> AuthorField {
    AuthorField {
        column,
        guards,
        vouches,
    }
}

/// Entity types the server publishes to every PC but no device may push: the PCs never stage
/// them (`scope` is the delivery scope).
pub const SERVER_ONLY_TYPES: &[(&str, &str)] = &[("user_signing_keys", "tenant")];

/// Rule of `entity_type`; `None` when the type names no user and needs no authority.
pub fn signing_rule(entity_type: &str) -> Option<&'static SigningRule> {
    SIGNING_RULES.iter().find(|r| r.entity_type == entity_type)
}

/// Payload key of an author field (`approved_by_id` → `approved_by_sync_id`).
pub fn author_payload_key(field: &AuthorField) -> String {
    crate::sync::row_layout::ref_payload_key(field.column)
}

/// True when items of this type must carry the maker's signature (server rule 3).
pub fn needs_maker_signature(entity_type: &str) -> bool {
    signing_rule(entity_type).is_some_and(|r| !r.author_fields.is_empty() || r.authority.is_some())
}

/// Sorted by entity type.
pub static SIGNING_RULES: &[SigningRule] = &[
    SigningRule {
        entity_type: "activity_events",
        author_fields: &[author("actor_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "admin_change_events",
        author_fields: &[author("actor_id", &[])],
        not_attribution: &["target_user_id"],
        authority: None,
    },
    SigningRule {
        entity_type: "admin_recovery_acknowledgements",
        author_fields: &[author("acknowledged_by_id", &["acknowledged_at"])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "admin_recovery_notices",
        author_fields: &[],
        not_attribution: &["recipient_user_id", "created_user_id"],
        authority: None,
    },
    SigningRule {
        entity_type: "app_settings",
        // The last modifier answers for the value; a background write clears it (no false credit).
        author_fields: &[author_vouching("last_modified_by_id", &[], &["setting_value_json"])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "asset_corrective_actions",
        author_fields: &[
            author("created_by_id", &[]),
            author_vouching(
                "updated_by_id",
                &[],
                &["action_text", "due_date", "status", "completed_at"],
            ),
        ],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "asset_document_links",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "asset_five_why_analyses",
        author_fields: &[
            author("created_by_id", &[]),
            author_vouching(
                "updated_by_id",
                &[],
                &[
                    "problem_statement",
                    "why_1",
                    "why_2",
                    "why_3",
                    "why_4",
                    "why_5",
                    "root_cause",
                ],
            ),
        ],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "asset_meter_readings",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "asset_photos",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "audit_events",
        author_fields: &[author("actor_id", &[])],
        not_attribution: &["target_id", "details_json"],
        authority: None,
    },
    SigningRule {
        entity_type: "budget_actuals",
        author_fields: &[author("posted_by_id", &["posted_at"])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "budget_alert_configs",
        author_fields: &[],
        not_attribution: &["recipient_user_id"],
        authority: None,
    },
    SigningRule {
        entity_type: "budget_alert_events",
        author_fields: &[author(
            "acknowledged_by_id",
            &["acknowledged_at", "acknowledgement_note"],
        )],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "budget_forecast_runs",
        author_fields: &[author("generated_by_id", &["generated_at"])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "budget_periods",
        author_fields: &[
            author("closed_by_id", &["closed_at"]),
            author("reopened_by_id", &["reopened_at"]),
        ],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "budget_variance_reviews",
        author_fields: &[],
        not_attribution: &["action_owner_id"],
        authority: None,
    },
    SigningRule {
        entity_type: "budget_versions",
        author_fields: &[
            author("created_by_id", &[]),
            author("approved_by_id", &["approved_at"]),
            author("frozen_by_id", &["frozen_at"]),
        ],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "cost_centers",
        author_fields: &[],
        not_attribution: &["budget_owner_id"],
        authority: None,
    },
    SigningRule {
        entity_type: "currency_exchange_rates",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "data_integrity_findings",
        author_fields: &[author("waiver_approver_id", &["waiver_reason"])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "data_integrity_repair_actions",
        author_fields: &[author("actor_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "delegated_admin_policies",
        author_fields: &[author(
            "changed_by_id",
            &[
                "admin_role_sync_id",
                "managed_scope_type",
                "managed_scope_reference_sync_id",
                "allowed_domains_json",
                "requires_step_up_for_publish",
            ],
        )],
        not_attribution: &[],
        authority: Some(permissions::ADM_USERS),
    },
    SigningRule {
        entity_type: "di_attachments",
        author_fields: &[author("uploaded_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "di_change_events",
        author_fields: &[author("actor_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "di_review_events",
        author_fields: &[author("actor_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "di_state_transition_log",
        author_fields: &[author("actor_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "equipment_criticality_assessments",
        author_fields: &[author("assessed_by_id", &["assessed_at"])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "equipment_lifecycle_events",
        author_fields: &[author("performed_by_id", &[]), author("approved_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "equipment_spare_parts",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "event_tree_models",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "failure_events",
        author_fields: &[author("recorded_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "fmeca_analyses",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "fta_models",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "goods_receipts",
        author_fields: &[author("received_by_id", &["received_at"])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "inspection_rounds",
        author_fields: &[author("reviewed_by_id", &["reviewed_at", "review_note"])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "intervention_requests",
        author_fields: &[
            author("submitter_id", &[]),
            author("reviewer_id", &["screened_at"]),
            author("closed_by_id", &["closed_at", "disposition_code", "disposition_notes"]),
            author("site_assigned_by_id", &["site_assigned_at", "site_assign_reason"]),
        ],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "inventory_count_lines",
        author_fields: &[
            author("counted_by_id", &["counted_qty"]),
            // `self_approved`: the counter approved their own variance (four-eyes opt-out, m259):
            // only the approver may change it, unless the approval itself is withdrawn (review
            // F6). Vouched, not guarded: the column is NOT NULL and stays 0 when an approval is
            // reset.
            author_vouching("approved_by_id", &["approved_at", "approval_note"], &["self_approved"]),
            author("second_approved_by_id", &["second_approved_at", "second_approval_note"]),
        ],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "inventory_count_sessions",
        author_fields: &[
            author("opened_by_id", &[]),
            author("submitted_by_id", &["submitted_at"]),
            author("posted_by_id", &["posted_at"]),
            author("reversed_by_id", &["reversed_at", "reversal_reason"]),
        ],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "inventory_document_links",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "inventory_lots",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "inventory_mutation_audit_links",
        author_fields: &[author("reviewer_id", &["reviewer_evidence"])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "inventory_reconciliation_runs",
        author_fields: &[author("checked_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "inventory_state_events",
        author_fields: &[author("actor_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "inventory_transactions",
        author_fields: &[author("performed_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "inventory_valuation_periods",
        author_fields: &[author("closed_by_id", &["closed_at"])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "library_documents",
        author_fields: &[author("uploaded_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "lookup_domains",
        author_fields: &[author("published_by_id", &["published_at"])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "loto_card_print_jobs",
        author_fields: &[author("printed_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "markov_models",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "mc_models",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "notification_acknowledgements",
        author_fields: &[author("acknowledged_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "notification_escalation_steps",
        author_fields: &[],
        not_attribution: &["route_to_reference"],
        authority: None,
    },
    SigningRule {
        entity_type: "notification_events",
        author_fields: &[],
        not_attribution: &["payload_json"],
        authority: None,
    },
    SigningRule {
        entity_type: "notification_preferences",
        author_fields: &[],
        not_attribution: &["user_id"],
        authority: None,
    },
    SigningRule {
        entity_type: "notifications",
        author_fields: &[],
        not_attribution: &["recipient_user_id"],
        authority: None,
    },
    SigningRule {
        entity_type: "org_change_events",
        author_fields: &[author("changed_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "org_node_types",
        author_fields: &[],
        not_attribution: &[],
        authority: Some(permissions::ORG_ADMIN),
    },
    // The org tree decides what a site-scoped grant covers (RBAC resolver): moving or changing a
    // node is judged as an organisation-administration act (design §11 implementation check).
    SigningRule {
        entity_type: "org_nodes",
        author_fields: &[],
        not_attribution: &[],
        authority: Some(permissions::ORG_ADMIN),
    },
    SigningRule {
        entity_type: "org_structure_models",
        author_fields: &[author("activated_by_id", &["activated_at"])],
        not_attribution: &[],
        authority: Some(permissions::ORG_ADMIN),
    },
    SigningRule {
        entity_type: "org_type_relationship_rules",
        author_fields: &[],
        not_attribution: &[],
        authority: Some(permissions::ORG_ADMIN),
    },
    SigningRule {
        entity_type: "permit_gas_tests",
        author_fields: &[author("measured_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "permit_handover_logs",
        author_fields: &[author("signed_by_id", &["signed_at"])],
        not_attribution: &["received_by_id"],
        authority: None,
    },
    SigningRule {
        entity_type: "permit_isolations",
        author_fields: &[
            author("applied_by_id", &["applied_at"]),
            author("verified_by_id", &["verified_at"]),
            author("removal_verified_by_id", &["removal_verified_at"]),
            author("cancelled_by_id", &["cancelled_at"]),
            author("removed_by_id", &["removed_at"]),
            author(
                "forced_authorized_by_id",
                &["forced_reason", "forced_contact_attempts_json"],
            ),
            author("informed_by_id", &["informed_at", "informed_method"]),
        ],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "permit_personal_locks",
        author_fields: &[
            author("placed_by_id", &["placed_at"]),
            author("removed_by_id", &["removed_at"]),
            author(
                "forced_authorized_by_id",
                &["forced_reason", "forced_contact_attempts_json"],
            ),
            author("informed_by_id", &["informed_at", "informed_method"]),
        ],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "permit_suspensions",
        author_fields: &[
            author("suspended_by_id", &["suspended_at"]),
            author("reinstated_by_id", &["reinstated_at"]),
        ],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "permit_types",
        author_fields: &[
            author(
                "three_person_changed_by_id",
                &[
                    "requires_three_person_removal",
                    "three_person_changed_at",
                    "three_person_change_reason",
                ],
            ),
            author(
                "isolation_changed_by_id",
                &[
                    "requires_energy_isolation",
                    "isolation_changed_at",
                    "isolation_change_reason",
                ],
            ),
        ],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "permit_workers",
        author_fields: &[
            author("added_by_id", &["added_at"]),
            author("induction_by_id", &["induction_at"]),
            author("withdrawn_by_id", &["withdrawn_at"]),
        ],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "personnel_assignment_history",
        author_fields: &[author("changed_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "personnel_availability_blocks",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "personnel_schedule_exceptions",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "planning_windows",
        author_fields: &[author(
            "site_assigned_by_id",
            &["site_assigned_at", "site_assign_reason"],
        )],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "pm_executions",
        author_fields: &[author("completed_by_id", &[]), author("actor_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "pm_occurrence_transitions",
        author_fields: &[author("actor_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "policy_snapshots",
        author_fields: &[author("activated_by_id", &["activated_at"])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "position_change_events",
        author_fields: &[author("changed_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "procurement_approval_limits",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "procurement_requisitions",
        author_fields: &[author("requested_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "purchase_orders",
        author_fields: &[
            author("ordered_by_id", &["ordered_at"]),
            author("approved_by_id", &["approved_at"]),
        ],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "qualification_overrides",
        author_fields: &[author("approved_by_id", &["approved_at"])],
        not_attribution: &["user_id"],
        authority: None,
    },
    SigningRule {
        entity_type: "qualification_requirements",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "ram_expert_sign_offs",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "rbac_settings",
        author_fields: &[],
        not_attribution: &[],
        authority: Some(permissions::ADM_SETTINGS),
    },
    SigningRule {
        entity_type: "rbd_models",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "rcm_studies",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "reference_sets",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "reference_validation_reports",
        author_fields: &[author("validated_by_id", &["validated_at"])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "reference_value_migrations",
        author_fields: &[author("migrated_by_id", &["migrated_at"])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "reliability_cost_profiles",
        author_fields: &[author_vouching("updated_by_id", &[], &["planned_cost", "failure_cost"])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "reliability_studies",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "reliability_study_decisions",
        author_fields: &[
            author("proposed_by_id", &["proposed_at"]),
            author("decided_by_id", &["decided_at", "decision_note"]),
            author("applied_by_id", &["applied_at"]),
        ],
        not_attribution: &["approver_id"],
        authority: None,
    },
    SigningRule {
        entity_type: "reliability_study_record_events",
        author_fields: &[author("actor_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "repairable_orders",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "report_schedules",
        author_fields: &[],
        not_attribution: &["user_id"],
        authority: None,
    },
    SigningRule {
        entity_type: "role_permissions",
        author_fields: &[author("granted_by_id", &["granted_at"])],
        not_attribution: &[],
        authority: Some(permissions::ADM_ROLES),
    },
    SigningRule {
        entity_type: "role_templates",
        author_fields: &[],
        not_attribution: &[],
        authority: Some(permissions::ADM_ROLES),
    },
    SigningRule {
        entity_type: "roles",
        author_fields: &[],
        not_attribution: &[],
        authority: Some(permissions::ADM_ROLES),
    },
    SigningRule {
        entity_type: "saved_activity_filters",
        author_fields: &[],
        not_attribution: &["user_id", "filter_json"],
        authority: None,
    },
    SigningRule {
        entity_type: "schedule_break_ins",
        author_fields: &[
            author("created_by_id", &[]),
            author("approved_by_user_id", &["approver_signed_at"]),
        ],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "schedule_change_log",
        author_fields: &[author("actor_id", &[])],
        not_attribution: &["details_json"],
        authority: None,
    },
    SigningRule {
        entity_type: "schedule_class_versions",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "schedule_commitments",
        author_fields: &[
            author("committed_by_id", &[]),
            author("status_changed_by_id", &["status_changed_at"]),
        ],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "settings_change_events",
        author_fields: &[author("changed_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "site_setting_events",
        author_fields: &[author("changed_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "site_settings",
        // A per-site safety rule is only ever set by a person (`settings::site`): its value needs
        // that person's signature. The site it applies to is not guarded: an organisation publish
        // moves the rule to the published node and keeps its author (`org::validation`).
        author_fields: &[author("updated_by_id", &["setting_key", "value_json"])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "site_work_weeks",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "stock_reservations",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "team_schedule_assignments",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "unit_conversions",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "user_accounts",
        author_fields: &[
            author(
                "password_changed_by_id",
                &[
                    "password_hash",
                    "password_changed_at",
                    "password_set_by_sync_id",
                    "password_set_at",
                ],
            ),
            author("password_set_by_id", &["password_set_at"]),
            author("status_changed_by_id", &["is_active", "deleted_at"]),
        ],
        not_attribution: &[],
        authority: Some(permissions::ADM_USERS),
    },
    SigningRule {
        entity_type: "user_analytics_dashboard_layouts",
        author_fields: &[],
        not_attribution: &["user_id"],
        authority: None,
    },
    SigningRule {
        entity_type: "user_dashboard_layouts",
        author_fields: &[],
        not_attribution: &["user_id"],
        authority: None,
    },
    SigningRule {
        entity_type: "user_dismissals",
        author_fields: &[],
        not_attribution: &["user_id"],
        authority: None,
    },
    SigningRule {
        entity_type: "user_scope_assignments",
        author_fields: &[author("assigned_by_id", &[])],
        not_attribution: &["user_id"],
        authority: Some(permissions::ADM_USERS),
    },
    SigningRule {
        entity_type: "weekly_schedule_events",
        author_fields: &[author("approver_id", &[])],
        not_attribution: &["actor_id"],
        authority: None,
    },
    SigningRule {
        entity_type: "weekly_schedules",
        author_fields: &[
            author("approved_by_id", &["approved_at"]),
            author("frozen_by_id", &["frozen_at"]),
            author("published_by_id", &["published_at"]),
            author(
                "unfreeze_requested_by_id",
                &["unfreeze_requested_at", "unfreeze_reason"],
            ),
        ],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "weibull_fit_results",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "wo_action_events",
        author_fields: &[author("actor_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "wo_change_events",
        author_fields: &[author("actor_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "wo_cost_estimates",
        author_fields: &[author_vouching(
            "updated_by_id",
            &[],
            &[
                "estimated_labor",
                "estimated_parts",
                "estimated_services",
                "estimated_other",
                "notes",
            ],
        )],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "wo_cost_settlements",
        author_fields: &[
            author("posted_by_id", &["posted_at"]),
            author("reversed_by_id", &["reversed_at", "reversal_reason"]),
        ],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "wo_history_records",
        author_fields: &[author("imported_by_id", &[]), author("coded_by_id", &["coded_at"])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "wo_state_transition_log",
        author_fields: &[author("actor_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "work_calendar_holidays",
        author_fields: &[author("created_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "work_order_attachments",
        author_fields: &[author("uploaded_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "work_order_delay_segments",
        author_fields: &[author("entered_by_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "work_order_execution_events",
        author_fields: &[author("actor_id", &[])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "work_order_interveners",
        author_fields: &[],
        not_attribution: &["intervener_id"],
        authority: None,
    },
    SigningRule {
        entity_type: "work_order_tasks",
        author_fields: &[author("completed_by_id", &["completed_at", "result_code"])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "work_order_verifications",
        author_fields: &[author("verified_by_id", &["verified_at"])],
        not_attribution: &[],
        authority: None,
    },
    SigningRule {
        entity_type: "work_orders",
        author_fields: &[
            author("planning_approved_by_id", &["planning_approved_at"]),
            author(
                "permit_rule_cleared_by_id",
                &["permit_rule_cleared_at", "permit_rule_clear_reason"],
            ),
            author("site_assigned_by_id", &["site_assigned_at", "site_assign_reason"]),
            author("validated_by_id", &["validated_at"]),
            author("fmeca_parts_override_signed_by_id", &["fmeca_parts_override_reason"]),
        ],
        not_attribution: &["requester_id", "planner_id", "primary_responsible_id"],
        authority: None,
    },
    SigningRule {
        entity_type: "work_permits",
        author_fields: &[
            author("created_by_id", &[]),
            author("submitted_by_id", &["submitted_at"]),
            // The self-approval flags (four-eyes opt-out, m244 / m294) are vouched for by their
            // approver: no other signer may set or clear them (review F6).
            author("approved_by_id", &["approved_at", "self_approved"]),
            author("cancelled_by_id", &["cancelled_at"]),
            // The HSE approver vouches for the permit content they approved: a change of it is
            // signed by them or withdraws their approval (`permit::safety_content`).
            author_vouching(
                "hse_approved_by_id",
                &["hse_approved_at", "hse_approval_note", "hse_self_approved"],
                &[
                    "work_description",
                    "risk_assessment",
                    "preventive_measures",
                    "ppe_required",
                ],
            ),
        ],
        not_attribution: &[],
        authority: None,
    },
];
