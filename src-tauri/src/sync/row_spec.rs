//! Row sync rules per entity type (Phase 2.4).
//!
//! The generic capture/apply engine (`sync::capture`, `sync::row_codec`) sends every column of a
//! replicated row. It reads the table shape from SQLite (columns, primary key, declared foreign
//! keys) and this file adds what the schema cannot say:
//!
//! - references that are not declared as foreign keys (`refs`), including references whose target
//!   depends on another column (`RefTarget::Poly`) and references to app catalogs that are not
//!   replicated (`RefTarget::Catalog`, sent by natural key);
//! - integer `*_id` columns that hold values, not references (`value_columns`);
//! - columns that describe this device only and never travel (`device_local`);
//! - a row filter for tables that mix replicated and device rows (`row_filter`).
//!
//! An entity type is synchronised by the generic engine exactly when it has a `RowSpec` here; the
//! registry flags `has_stager` / `has_materializer` must agree (CI test).
//!
//! Local integer ids never travel: a reference is sent as the target's `entity_sync_id` (or its
//! catalog key) and resolved back to a local id on the receiving device. A reference whose target
//! has not arrived yet defers the row; `soft` references (back edges of a dependency cycle, such
//! as a plan's current version) are written as NULL and completed when the target arrives.

/// Where a reference column points.
#[derive(Debug, Clone, Copy)]
pub enum RefTarget {
    /// A row of a replicated entity type, sent by its `entity_sync_id`.
    Entity(&'static str),
    /// A row of a local app catalog (not replicated), sent by the value of `key_column`.
    Catalog {
        table: &'static str,
        key_column: &'static str,
    },
    /// The entity type is named by `type_column`: `(type value, entity type)`. A row whose type
    /// is not listed must leave the reference NULL, otherwise staging fails visibly.
    Poly {
        type_column: &'static str,
        targets: &'static [(&'static str, &'static str)],
    },
    /// A local id that has no replicated meaning yet: it must stay NULL to be synchronised (a
    /// non-NULL value fails staging visibly instead of sending a local id).
    Unmapped,
}

#[derive(Debug, Clone, Copy)]
pub struct RefSpec {
    pub column: &'static str,
    pub target: RefTarget,
    /// Back edge of a dependency cycle: written as NULL until the target arrives.
    pub soft: bool,
    /// Link of a history row (event, log): when the target no longer exists on this device the
    /// event outlives it, and the link is sent as NULL instead of failing.
    pub history: bool,
}

/// Local ids stored inside a JSON text column.
///
/// `path` selects the ids: `$` (the whole value is an id), `[*]` (array of ids), `[*].field`
/// (field of every object of an array), `.field` (field of an object); segments combine.
#[derive(Debug, Clone, Copy)]
pub struct JsonRefSpec {
    pub column: &'static str,
    pub path: &'static str,
    pub entity: &'static str,
    /// History value: a target that no longer exists is sent empty instead of failing.
    pub history: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct RowSpec {
    pub entity_type: &'static str,
    /// References not declared as SQLite foreign keys, and overrides of declared ones.
    pub refs: &'static [RefSpec],
    /// Integer `*_id` columns that are values (external ids, counters), not references.
    pub value_columns: &'static [&'static str],
    /// Columns describing this device only (local file paths, login counters, cached paths).
    /// Never sent; never overwritten by inbound rows.
    pub device_local: &'static [&'static str],
    /// SQL condition on the row (written with the `{row}` alias) limiting which rows replicate.
    pub row_filter: Option<&'static str>,
    /// Columns derived on each device from replicated data (for example a path of local ids):
    /// never sent; written with the given SQL literal when a row is received, then recomputed by
    /// `sync::apply_hooks` in the same transaction.
    pub derived: &'static [(&'static str, &'static str)],
    /// Natural-key table whose rows every device seeds itself: rows that exist when sync of the
    /// type starts are not queued (id-keyed tables tell seeded rows apart by their identity).
    pub skip_baseline: bool,
    /// Local ids inside JSON text columns, sent as sync ids.
    pub json_refs: &'static [JsonRefSpec],
}

const fn r(column: &'static str, entity: &'static str) -> RefSpec {
    RefSpec {
        column,
        target: RefTarget::Entity(entity),
        soft: false,
        history: false,
    }
}

const fn soft(column: &'static str, entity: &'static str) -> RefSpec {
    RefSpec {
        column,
        target: RefTarget::Entity(entity),
        soft: true,
        history: false,
    }
}

/// Reference from a history row (see `RefSpec::history`).
const fn hist(column: &'static str, entity: &'static str) -> RefSpec {
    RefSpec {
        column,
        target: RefTarget::Entity(entity),
        soft: false,
        history: true,
    }
}

const fn poly(
    column: &'static str,
    type_column: &'static str,
    targets: &'static [(&'static str, &'static str)],
) -> RefSpec {
    RefSpec {
        column,
        target: RefTarget::Poly { type_column, targets },
        soft: false,
        history: false,
    }
}

const fn poly_hist(
    column: &'static str,
    type_column: &'static str,
    targets: &'static [(&'static str, &'static str)],
) -> RefSpec {
    RefSpec {
        column,
        target: RefTarget::Poly { type_column, targets },
        soft: false,
        history: true,
    }
}

/// Polymorphic back edge (see `RefSpec::soft`).
const fn poly_soft(
    column: &'static str,
    type_column: &'static str,
    targets: &'static [(&'static str, &'static str)],
) -> RefSpec {
    RefSpec {
        column,
        target: RefTarget::Poly { type_column, targets },
        soft: true,
        history: false,
    }
}

const fn catalog(column: &'static str, table: &'static str, key_column: &'static str) -> RefSpec {
    RefSpec {
        column,
        target: RefTarget::Catalog { table, key_column },
        soft: false,
        history: false,
    }
}

const fn unmapped(column: &'static str) -> RefSpec {
    RefSpec {
        column,
        target: RefTarget::Unmapped,
        soft: false,
        history: false,
    }
}

const fn spec(entity_type: &'static str, refs: &'static [RefSpec]) -> RowSpec {
    RowSpec {
        entity_type,
        refs,
        value_columns: &[],
        device_local: &[],
        row_filter: None,
        derived: &[],
        skip_baseline: false,
        json_refs: &[],
    }
}

const fn json_ref(column: &'static str, path: &'static str, entity: &'static str) -> JsonRefSpec {
    JsonRefSpec {
        column,
        path,
        entity,
        history: false,
    }
}

const fn json_ref_hist(column: &'static str, path: &'static str, entity: &'static str) -> JsonRefSpec {
    JsonRefSpec {
        column,
        path,
        entity,
        history: true,
    }
}

const USER: &str = "user_accounts";

/// Polymorphic target of a type whose value is not a row id (for example a role name): the value
/// is sent unchanged.
pub(crate) const NOT_A_ROW: &str = "";

/// Polymorphic target of a type whose rows stay on each device (local-only tables): the
/// reference is sent empty.
pub(crate) const LOCAL_ROW: &str = "-";

/// RBAC scope references: the org node (or team) a role assignment is limited to, stored as the
/// local id in text form. A tenant-wide assignment stores the licence tenant id (or nothing),
/// which is the same on every device of the plant: it is sent unchanged.
const SCOPE_TARGETS: &[(&str, &str)] = &[
    ("tenant", NOT_A_ROW),
    ("entity", "org_nodes"),
    ("org_node", "org_nodes"),
    ("site", "org_nodes"),
    ("team", "teams"),
];

/// Sources of stock movements and reservations (`source_type` → entity of `source_id`).
const STOCK_SOURCE_TARGETS: &[(&str, &str)] = &[
    ("WO", "work_orders"),
    ("PM_WO", "work_orders"),
    ("PO_GR", "goods_receipts"),
    ("COUNT_SESSION", "inventory_count_sessions"),
    ("REPAIRABLE_ORDER", "repairable_orders"),
];

/// Sources of stock reservations: work orders (corrective or preventive). Manual reservations
/// carry no source id.
const RESERVATION_SOURCE_TARGETS: &[(&str, &str)] = &[("WO", "work_orders"), ("PM_WO", "work_orders")];

/// Work items a schedule candidate or commitment is for (`source_type` → entity of `source_id`).
const SCHEDULE_SOURCE_TARGETS: &[(&str, &str)] = &[
    ("work_order", "work_orders"),
    ("pm_occurrence", "pm_occurrences"),
    ("inspection_follow_up", "intervention_requests"),
];

/// Procurement demand sources (`demand_source_type` → entity of `demand_source_id`). A repairable
/// order can itself point to a purchase order line, so the link is completed once the source
/// arrives (`poly_soft`).
const DEMAND_SOURCE_TARGETS: &[(&str, &str)] = &[
    ("WORK_ORDER", "work_orders"),
    ("WORK_ORDER_PART", "work_orders"),
    ("RESERVATION", "stock_reservations"),
    ("REPAIRABLE", "repairable_orders"),
];

pub const ROW_SPECS: &[RowSpec] = &[
    // ── Identity, access and organisation ───────────────────────────────────────────────────
    spec("roles", &[]),
    spec("role_templates", &[]),
    RowSpec {
        // Every device seeds the baseline grants itself.
        skip_baseline: true,
        ..spec(
            "role_permissions",
            &[
                r("role_id", "roles"),
                catalog("permission_id", "permissions", "name"),
                // Hard: a natural-key table cannot hold soft references. A permission whose author
                // never arrives waits as any row with a missing parent (`sync::grant_check`).
                r("granted_by_id", USER),
            ],
        )
    },
    RowSpec {
        skip_baseline: true,
        ..spec("rbac_settings", &[])
    },
    RowSpec {
        // Sign-in state of this device; the password hash travels (SYNC_STRATEGY §7).
        device_local: &[
            "pin_hash",
            "failed_login_attempts",
            "locked_until",
            "last_login_at",
            "last_seen_at",
            "consecutive_lockouts",
        ],
        // Users exist before the personnel records they are later linked to (and personnel
        // reference org data that references users): the link is completed when it arrives.
        // Who wrote the password and who last enabled or disabled the account (migration 261,
        // checked on arrival by `sync::credential_check`): soft, because a change of one's own
        // password names the row itself, and an author not applied yet never holds the account
        // back as a parent (the check makes the row wait for that author instead).
        ..spec(
            "user_accounts",
            &[
                soft("personnel_id", "personnel"),
                soft("password_changed_by_id", USER),
                soft("password_set_by_id", USER),
                soft("status_changed_by_id", USER),
            ],
        )
    },
    spec(
        "user_scope_assignments",
        &[
            r("user_id", USER),
            r("role_id", "roles"),
            // Soft: an assignment whose author a complete pull did not bring is applied for review
            // (`sync::grant_check`, which makes it wait for the author until then); the link is
            // completed if the author arrives later.
            soft("assigned_by_id", USER),
            poly("scope_reference", "scope_type", SCOPE_TARGETS),
        ],
    ),
    spec(
        "delegated_admin_policies",
        &[
            poly("managed_scope_reference", "managed_scope_type", SCOPE_TARGETS),
            // Who created or last changed the policy (migration 262), checked on arrival by
            // `sync::grant_check`; soft, as `assigned_by_id` above.
            soft("changed_by_id", USER),
        ],
    ),
    spec("org_structure_models", &[r("activated_by_id", USER)]),
    spec("org_node_types", &[r("structure_model_id", "org_structure_models")]),
    spec(
        "org_type_relationship_rules",
        &[
            r("structure_model_id", "org_structure_models"),
            r("parent_type_id", "org_node_types"),
            r("child_type_id", "org_node_types"),
        ],
    ),
    RowSpec {
        device_local: &["last_synced_checkpoint"],
        // Paths of local ids, recomputed by `apply_hooks` from the parent chain.
        derived: &[("ancestor_path", "'/'"), ("depth", "0")],
        ..spec(
            "org_nodes",
            &[r("node_type_id", "org_node_types"), r("parent_id", "org_nodes")],
        )
    },
    spec(
        "org_node_responsibilities",
        &[
            r("node_id", "org_nodes"),
            r("person_id", "personnel"),
            r("team_id", "teams"),
        ],
    ),
    spec("org_entity_bindings", &[r("node_id", "org_nodes")]),
    spec(
        "org_change_events",
        &[
            poly_hist(
                "entity_id",
                "entity_kind",
                &[
                    ("structure_model", "org_structure_models"),
                    ("org_node", "org_nodes"),
                    ("org_node_responsibility", "org_node_responsibilities"),
                    ("org_entity_binding", "org_entity_bindings"),
                ],
            ),
            hist("changed_by_id", USER),
        ],
    ),
    spec("teams", &[r("primary_node_id", "org_nodes")]),
    spec("personnel", &[r("supervisor_id", "personnel")]),
    spec(
        "positions",
        &[r("requirement_profile_id", "qualification_requirement_profiles")],
    ),
    spec("position_change_events", &[hist("changed_by_id", USER)]),
    spec("external_companies", &[]),
    spec("external_company_contacts", &[]),
    // ── Assets ──────────────────────────────────────────────────────────────────────────────
    RowSpec {
        device_local: &["last_synced_checkpoint"],
        ..spec(
            "equipment",
            &[
                r("class_id", "equipment_classes"),
                r("criticality_value_id", "lookup_values"),
                r("installed_at_node_id", "org_nodes"),
                r("functional_position_node_id", "org_nodes"),
            ],
        )
    },
    spec("equipment_classes", &[r("parent_id", "equipment_classes")]),
    spec(
        "equipment_hierarchy",
        &[
            r("parent_equipment_id", "equipment"),
            r("child_equipment_id", "equipment"),
        ],
    ),
    spec("equipment_meters", &[r("equipment_id", "equipment")]),
    spec(
        "asset_meter_readings",
        &[r("meter_id", "equipment_meters"), r("created_by_id", USER)],
    ),
    spec(
        "equipment_lifecycle_events",
        &[
            hist("equipment_id", "equipment"),
            hist("performed_by_id", USER),
            hist("from_node_id", "org_nodes"),
            hist("to_node_id", "org_nodes"),
            hist("related_work_order_id", "work_orders"),
            hist("related_asset_id", "equipment"),
            hist("approved_by_id", USER),
        ],
    ),
    spec(
        "asset_document_links",
        &[r("asset_id", "equipment"), r("created_by_id", USER)],
    ),
    spec("asset_external_ids", &[r("asset_id", "equipment")]),
    spec("asset_photos", &[]),
    spec("equipment_spare_parts", &[]),
    spec(
        "equipment_criticality_assessments",
        &[hist("equipment_id", "equipment"), hist("assessed_by_id", USER)],
    ),
    spec(
        "library_documents",
        &[r("equipment_id", "equipment"), r("uploaded_by_id", USER)],
    ),
    // `relative_path` travels: it names the file's place under the app data directory (unique by a
    // UUID) on every device, the same value the attachment row carries; the bytes travel through
    // object storage (upload by the sync worker, download queued by the apply hook).
    spec("media_attachments", &[]),
    // ── Reference catalogs ──────────────────────────────────────────────────────────────────
    spec("lookup_domains", &[r("published_by_id", USER)]),
    spec(
        "lookup_values",
        &[r("domain_id", "lookup_domains"), r("parent_value_id", "lookup_values")],
    ),
    spec("lookup_value_aliases", &[r("value_id", "lookup_values")]),
    spec("reference_domains", &[]),
    RowSpec {
        // When the publish impact preview ran on this device (cleared when the set's values
        // change): a preview made on one device does not count for another.
        device_local: &["impact_previewed_at"],
        ..spec(
            "reference_sets",
            &[r("domain_id", "reference_domains"), r("created_by_id", USER)],
        )
    },
    spec(
        "reference_values",
        &[r("set_id", "reference_sets"), r("parent_id", "reference_values")],
    ),
    spec("reference_aliases", &[r("reference_value_id", "reference_values")]),
    spec(
        "reference_validation_reports",
        &[r("set_id", "reference_sets"), r("validated_by_id", USER)],
    ),
    spec(
        "reference_value_migrations",
        &[
            hist("domain_id", "reference_domains"),
            hist("from_value_id", "reference_values"),
            hist("to_value_id", "reference_values"),
            hist("migrated_by_id", USER),
        ],
    ),
    // ── Intervention requests (DI) ──────────────────────────────────────────────────────────
    RowSpec {
        // The SLA poller of each device notifies that device's users once and marks it here.
        device_local: &["sla_response_breach_notified_at", "sla_resolution_breach_notified_at"],
        ..spec(
            "intervention_requests",
            &[
                r("equipment_id", "equipment"),
                r("org_node_id", "org_nodes"),
                r("symptom_code_id", "reference_values"),
                r("classification_code_id", "reference_values"),
                r("submitter_id", USER),
                r("reviewer_id", USER),
                r("closed_by_id", USER),
                // Who gave an old request without a site its site (migration 245).
                r("site_assigned_by_id", USER),
                r("sla_rule_id", "di_sla_rules"),
                r("related_di_id", "intervention_requests"),
                r("recurrence_di_id", "intervention_requests"),
                // A request converted to a work order points to it, and the work order points
                // back to its source request.
                soft("converted_to_wo_id", "work_orders"),
            ],
        )
    },
    spec(
        "di_attachments",
        &[r("di_id", "intervention_requests"), r("uploaded_by_id", USER)],
    ),
    RowSpec {
        // The work order a request was converted to (`commands::di`, conversion event).
        json_refs: &[json_ref_hist("details_json", "$.wo_id", "work_orders")],
        ..spec(
            "di_change_events",
            &[hist("di_id", "intervention_requests"), hist("actor_id", USER)],
        )
    },
    spec(
        "di_review_events",
        &[
            r("di_id", "intervention_requests"),
            hist("actor_id", USER),
            r("related_di_id", "intervention_requests"),
        ],
    ),
    spec("di_sla_rules", &[]),
    spec(
        "di_state_transition_log",
        &[hist("di_id", "intervention_requests"), hist("actor_id", USER)],
    ),
    // ── Work orders ─────────────────────────────────────────────────────────────────────────
    spec("work_order_statuses", &[]),
    spec("delay_reason_codes", &[]),
    spec("closeout_validation_policies", &[]),
    spec(
        "work_orders",
        &[
            // OT type and urgency level: values of the SYSTEM lists `WORK.ORDER_TYPES` and
            // `WORK.URGENCY` (migration 198).
            r("type_id", "reference_values"),
            r("status_id", "work_order_statuses"),
            r("equipment_id", "equipment"),
            r("location_id", "org_nodes"),
            r("entity_id", "org_nodes"),
            r("requester_id", USER),
            r("planner_id", USER),
            r("primary_responsible_id", USER),
            r("planning_approved_by_id", USER),
            // Who cleared the permit flag a conversion rule set (migration 233).
            r("permit_rule_cleared_by_id", USER),
            // Who gave an old work order without a site its site (migration 245).
            r("site_assigned_by_id", USER),
            r("assigned_team_id", "teams"),
            r("urgency_id", "reference_values"),
            r("source_di_id", "intervention_requests"),
            // The Classe horaire version the work order is planned on (migration 202).
            r("shift_class_version_id", "schedule_class_versions"),
            // Never written by any command yet: synchronised only while empty.
            unmapped("component_id"),
            unmapped("approver_id"),
            unmapped("production_impact_id"),
        ],
    ),
    spec("work_order_tasks", &[]),
    spec("work_order_interveners", &[r("skill_id", "reference_values")]),
    spec(
        "work_order_parts",
        &[
            r("article_id", "articles"),
            r("stock_location_id", "stock_locations"),
            unmapped("cost_override_by_id"),
        ],
    ),
    spec("work_order_tools", &[]),
    spec("work_order_verifications", &[]),
    spec("work_order_delay_segments", &[]),
    spec("work_order_downtime_segments", &[]),
    spec("work_order_failure_details", &[]),
    spec(
        "work_order_execution_events",
        &[poly_hist(
            "entity_id",
            "entity_kind",
            &[
                ("downtime", "work_order_downtime_segments"),
                ("part", "work_order_parts"),
                ("task", "work_order_tasks"),
                ("tool", "work_order_tools"),
            ],
        )],
    ),
    spec("work_order_attachments", &[]),
    spec("wo_action_events", &[hist("actor_id", USER)]),
    spec(
        "wo_change_events",
        &[hist("wo_id", "work_orders"), hist("actor_id", USER)],
    ),
    spec(
        "wo_state_transition_log",
        &[hist("wo_id", "work_orders"), hist("actor_id", USER)],
    ),
    // ── Preventive maintenance and planning ─────────────────────────────────────────────────
    spec(
        "pm_plans",
        &[
            // A plan targets one equipment, or fans out over an equipment family, an organisation
            // location (subtree) or a criticality group (`pm::generation`).
            poly(
                "asset_scope_id",
                "asset_scope_type",
                &[
                    ("equipment", "equipment"),
                    ("family", "reference_values"),
                    ("location", "org_nodes"),
                    ("criticality_group", "lookup_values"),
                ],
            ),
            r("assigned_team_id", "teams"),
            // A plan points to its current version, which points back to the plan.
            soft("current_version_id", "pm_plan_versions"),
        ],
    ),
    RowSpec {
        // The asset meter a usage-based version counts on (`pm::generation` trigger shapes).
        json_refs: &[
            json_ref("trigger_definition_json", "$.asset_meter_id", "equipment_meters"),
            json_ref("trigger_definition_json", "$.meter.asset_meter_id", "equipment_meters"),
        ],
        ..spec(
            "pm_plan_versions",
            // Revision created from an FMECA item, an RCM decision (Phase 6.5) or a PM finding.
            // Soft: the source also points to the version it created (FMECA/RCM).
            &[poly_soft(
                "source_id",
                "source_type",
                &[
                    ("fmeca_item", "fmeca_items"),
                    ("rcm_decision", "rcm_decisions"),
                    ("pm_finding", "pm_findings"),
                    ("reliability_study_decision", "reliability_study_decisions"),
                ],
            )],
        )
    },
    // Generated by every device from the plans (deterministic identity, `pm::queries`); sent when
    // a person acts on an occurrence.
    spec("pm_occurrences", &[]),
    spec("pm_executions", &[]),
    spec("pm_findings", &[]),
    spec("pm_occurrence_transitions", &[]),
    spec("pm_trigger_events", &[]),
    // Who gave a window without a site its site (migration 245).
    spec("planning_windows", &[r("site_assigned_by_id", USER)]),
    spec("capacity_rules", &[]),
    spec(
        "schedule_candidates",
        &[poly("source_id", "source_type", SCHEDULE_SOURCE_TARGETS)],
    ),
    spec(
        "schedule_commitments",
        &[
            poly("source_id", "source_type", SCHEDULE_SOURCE_TARGETS),
            r("committed_by_id", USER),
        ],
    ),
    spec("schedule_break_ins", &[]),
    RowSpec {
        // Ids inside the change details (`planning::scheduling`).
        json_refs: &[
            json_ref_hist("details_json", "$.personnel_id", "personnel"),
            json_ref_hist("details_json", "$.break_in_id", "schedule_break_ins"),
            json_ref_hist("details_json", "$.approved_by_user_id", USER),
            json_ref_hist("details_json", "$.requested_by_user_id", USER),
            json_ref_hist("details_json", "$.approved_by_personnel_id", "personnel"),
            json_ref_hist("details_json", "$.schedule_candidate_id", "schedule_candidates"),
            json_ref_hist("details_json", "$.team_id", "teams"),
        ],
        ..spec("schedule_change_log", &[hist("actor_id", USER)])
    },
    spec("schedule_details", &[]),
    spec("weekly_schedules", &[]),
    spec("weekly_schedule_events", &[]),
    spec("weekly_schedule_baseline_items", &[]),
    // ── Finance ─────────────────────────────────────────────────────────────────────────────
    spec("cost_centers", &[]),
    spec("budget_versions", &[]),
    spec("budget_lines", &[]),
    spec("budget_actuals", &[r("reversal_of_actual_id", "budget_actuals")]),
    spec(
        "budget_commitments",
        &[
            r("purchase_order_id", "purchase_orders"),
            // No contracts table exists and no command writes it: synchronised only while empty.
            unmapped("contract_id"),
        ],
    ),
    spec("budget_erp_cost_center_master", &[]),
    spec("budget_forecast_runs", &[]),
    spec("budget_forecasts", &[]),
    spec("budget_variance_reviews", &[]),
    spec("budget_alert_configs", &[]),
    // The alert's notification belongs to its recipient and reaches only that person's devices:
    // elsewhere the link stays empty (completed if the notification arrives later).
    spec("budget_alert_events", &[soft("notification_id", "notifications")]),
    spec("posted_export_batches", &[]),
    spec(
        "integration_exceptions",
        &[poly(
            "source_record_id",
            "source_record_kind",
            &[
                ("budget_actual", "budget_actuals"),
                ("budget_forecast", "budget_forecasts"),
            ],
        )],
    ),
    // ── Personnel and qualification ─────────────────────────────────────────────────────────
    spec("personnel_assignment_history", &[hist("changed_by_id", USER)]),
    spec(
        "personnel_authorizations",
        &[r("source_certification_type_id", "certification_types")],
    ),
    spec("personnel_availability_blocks", &[]),
    spec("personnel_rate_cards", &[r("cost_center_id", "cost_centers")]),
    spec("personnel_skills", &[]),
    spec("personnel_team_assignments", &[]),
    spec("skill_categories", &[]),
    spec("skill_definitions", &[r("category_id", "skill_categories")]),
    spec(
        "team_skill_requirements",
        &[r("team_id", "teams"), r("skill_id", "skill_definitions")],
    ),
    spec("certification_types", &[]),
    spec("personnel_certifications", &[]),
    RowSpec {
        json_refs: &[json_ref(
            "required_certification_type_ids_json",
            "[*]",
            "certification_types",
        )],
        ..spec("qualification_requirement_profiles", &[])
    },
    spec("qualification_profile_skills", &[]),
    spec("training_sessions", &[]),
    spec("training_attendance", &[]),
    RowSpec {
        // Required id of a document version, but no document version table exists in this
        // version: it is sent as the value it is (revisit when document versions are stored).
        value_columns: &["document_version_id"],
        ..spec("document_acknowledgements", &[])
    },
    // ── Inspections ─────────────────────────────────────────────────────────────────────────
    spec(
        "inspection_templates",
        // A template points to its current version, which points back to the template.
        &[soft("current_version_id", "inspection_template_versions")],
    ),
    RowSpec {
        // The checkpoint package lists the equipment of each checkpoint by local id.
        // The unit of each measurement checkpoint (MEASURE.UNITS, migration 201) is a history
        // value of the published package: a unit retired since is sent empty.
        json_refs: &[
            json_ref("checkpoint_package_json", "[*].asset_id", "equipment"),
            json_ref_hist("checkpoint_package_json", "[*].measurement_unit_id", "reference_values"),
        ],
        ..spec("inspection_template_versions", &[])
    },
    // No command writes the checkpoint component yet: synchronised only while empty.
    spec("inspection_checkpoints", &[unmapped("component_id")]),
    spec("inspection_rounds", &[]),
    RowSpec {
        // When this device last generated the schedule's rounds: each device generates for
        // itself (generated rounds have deterministic identities), so it never travels.
        device_local: &["last_generated_at"],
        ..spec("inspection_schedules", &[])
    },
    spec("inspection_results", &[]),
    spec("inspection_evidence", &[]),
    spec(
        "inspection_anomalies",
        // Requests and work orders created from an anomaly point back to it.
        &[
            soft("linked_di_id", "intervention_requests"),
            soft("linked_work_order_id", "work_orders"),
        ],
    ),
    // ── Permits ─────────────────────────────────────────────────────────────────────────────
    // The mandatory PPE list has no catalog table yet (seeded empty): sent as the value it is.
    spec("permit_types", &[]),
    spec(
        "work_permits",
        &[
            r("linked_work_order_id", "work_orders"),
            r("permit_type_id", "permit_types"),
            r("asset_id", "equipment"),
            r("entity_id", "org_nodes"),
            // Who wrote, submitted and approved the permit (four-eyes, migration 244).
            r("created_by_id", USER),
            r("submitted_by_id", USER),
            r("approved_by_id", USER),
        ],
    ),
    spec("permit_handover_logs", &[r("permit_id", "work_permits")]),
    spec(
        "permit_isolations",
        &[
            r("permit_id", "work_permits"),
            r("applied_by_id", USER),
            r("verified_by_id", USER),
            r("removal_verified_by_id", USER),
            // Who cancelled a planned line (migration 243).
            r("cancelled_by_id", USER),
            // Who took the lock off, who authorised a forced removal and who recorded that the
            // worker was told (migration 249).
            r("removed_by_id", USER),
            r("forced_authorized_by_id", USER),
            r("informed_by_id", USER),
        ],
    ),
    spec(
        "permit_suspensions",
        &[
            r("permit_id", "work_permits"),
            r("suspended_by_id", USER),
            r("reinstated_by_id", USER),
        ],
    ),
    spec(
        "loto_card_print_jobs",
        &[
            r("permit_id", "work_permits"),
            r("isolation_id", "permit_isolations"),
            r("printed_by_id", USER),
        ],
    ),
    // ── Reliability (RAMS) ──────────────────────────────────────────────────────────────────
    spec("failure_hierarchies", &[]),
    spec("failure_codes", &[]),
    spec(
        "failure_events",
        &[
            poly("source_id", "source_type", &[("work_order", "work_orders")]),
            r("recorded_by_id", USER),
            // No command writes the failed component yet: synchronised only while empty.
            unmapped("component_id"),
        ],
    ),
    spec("runtime_exposure_logs", &[]),
    // Every device seeds the default guardrail flags (seeded identity).
    spec("ram_advanced_guardrails", &[]),
    spec("reliability_beta_benchmarks", &[]),
    spec("reliability_dashboard_policies", &[]),
    // Reliability studies (migration 179). Users are referenced by identity; the cost default of a
    // class has an identity derived from the class (`study::store::upsert_cost_profile`).
    spec("reliability_studies", &[r("created_by_id", USER)]),
    spec("reliability_study_assets", &[]),
    spec("reliability_study_record_events", &[r("actor_id", USER)]),
    RowSpec {
        // The PM trigger the decision will write keeps the meter it counts on.
        json_refs: &[json_ref(
            "proposed_trigger_definition_json",
            "$.asset_meter_id",
            "equipment_meters",
        )],
        ..spec(
            "reliability_study_decisions",
            &[
                r("approver_id", USER),
                r("proposed_by_id", USER),
                r("decided_by_id", USER),
                r("applied_by_id", USER),
            ],
        )
    },
    spec("reliability_cost_profiles", &[r("updated_by_id", USER)]),
    spec("fmeca_analyses", &[r("created_by_id", USER)]),
    spec("fmeca_items", &[unmapped("component_id")]),
    spec("fmeca_item_parts", &[]),
    spec("rcm_studies", &[r("created_by_id", USER)]),
    spec("rcm_decisions", &[]),
    spec("fta_models", &[r("created_by_id", USER)]),
    spec("rbd_models", &[r("created_by_id", USER)]),
    spec("event_tree_models", &[r("created_by_id", USER)]),
    spec("markov_models", &[r("created_by_id", USER)]),
    spec("mc_models", &[r("created_by_id", USER)]),
    spec(
        "weibull_fit_results",
        &[
            r("created_by_id", USER),
            // Optional failure-mode filter of the fit (migration 170, column without FK).
            r("failure_mode_id", "failure_codes"),
        ],
    ),
    spec("ram_expert_sign_offs", &[r("created_by_id", USER)]),
    spec("ram_ishikawa_diagrams", &[]),
    // Per user: issues a person dismissed in the reliability views. `scope_key` embeds the local
    // equipment id: rebuilt on apply (`apply_hooks`), unique placeholder until then.
    RowSpec {
        derived: &[("scope_key", "('pending:' || lower(hex(randomblob(16))))")],
        ..spec("user_dismissals", &[])
    },
    // ── Data integrity and analytics contract ───────────────────────────────────────────────
    spec(
        "data_integrity_findings",
        &[poly(
            "record_id",
            "record_class",
            &[
                ("wo_failure_details", "work_order_failure_details"),
                ("work_order_downtime_segments", "work_order_downtime_segments"),
                ("work_orders", "work_orders"),
            ],
        )],
    ),
    spec("data_integrity_repair_actions", &[]),
    spec("analytics_contract_versions", &[]),
    // ── Settings, policies, retention ───────────────────────────────────────────────────────
    RowSpec {
        // Device settings (licence onboarding, bootstrap state, updater, diagnostics) stay here.
        row_filter: Some("{row}.setting_scope = 'tenant'"),
        // Secrets live in each device's key store: the reference to them is local.
        device_local: &["secret_ref_id"],
        ..spec("app_settings", &[r("last_modified_by_id", USER)])
    },
    spec("policy_snapshots", &[r("activated_by_id", USER)]),
    spec("retention_policies", &[]),
    // ── History logs (append-only) ──────────────────────────────────────────────────────────
    RowSpec {
        // Ids inside the event summary (`activity::emitter` callers: RBAC, planning, PM).
        json_refs: &[
            json_ref_hist("summary_json", "$.role_id", "roles"),
            json_ref_hist("summary_json", "$.assignment_id", "user_scope_assignments"),
            json_ref_hist("summary_json", "$.break_in_id", "schedule_break_ins"),
            json_ref_hist("summary_json", "$.schedule_commitment_id", "schedule_commitments"),
            json_ref_hist("summary_json", "$.impacted_assignee_id", "personnel"),
            json_ref_hist("summary_json", "$.team_id", "teams"),
            json_ref_hist("summary_json", "$.pm_occurrence_id", "pm_occurrences"),
            json_ref_hist("summary_json", "$.pm_plan_id", "pm_plans"),
            json_ref_hist("summary_json", "$.linked_work_order_id", "work_orders"),
        ],
        ..spec(
            "activity_events",
            &[
                hist("entity_scope_id", "org_nodes"),
                hist("actor_id", USER),
                poly_hist(
                    "source_record_id",
                    "source_record_type",
                    &[
                        ("work_order", "work_orders"),
                        ("intervention_request", "intervention_requests"),
                        ("pm_occurrence", "pm_occurrences"),
                        ("personnel", "personnel"),
                        ("schedule_commitment", "schedule_commitments"),
                        ("role_assignment", "user_scope_assignments"),
                        // Archives and import batches are kept on the device that made them.
                        ("archive_item", LOCAL_ROW),
                        ("personnel_import_batch", LOCAL_ROW),
                    ],
                ),
            ],
        )
    },
    RowSpec {
        // Ids inside the details (`audit::emit` wraps the caller's detail under `detail`).
        json_refs: &[
            json_ref_hist("details_json", "$.detail.role_id", "roles"),
            json_ref_hist("details_json", "$.detail.assignment_id", "user_scope_assignments"),
            json_ref_hist("details_json", "$.filter.actor_id", USER),
            // The site before and after a site given to an old record (`org::site_assignment`).
            json_ref_hist("details_json", "$.site_before", "org_nodes"),
            json_ref_hist("details_json", "$.site_after", "org_nodes"),
            // The location before and after an equipment move (`assets::lifecycle` `asset.moved`).
            json_ref_hist("details_json", "$.detail.from_org_node_id", "org_nodes"),
            json_ref_hist("details_json", "$.detail.to_org_node_id", "org_nodes"),
            // The work order of a qualification requirement before and after its change
            // (`qualification::requirements::audit_view`).
            json_ref_hist("details_json", "$.before.work_order_id", "work_orders"),
            json_ref_hist("details_json", "$.after.work_order_id", "work_orders"),
            // The person given a qualification override (`qualification::gate`): its personnel
            // record or its login (the override itself is named by its sync id, sent as it is).
            // Rule: personnel records and logins named here are never hard-deleted (deactivated
            // only): their sync ids are in the sealed hash, and a deleted row would be encoded
            // empty, breaking the chain of the PC that sealed the event.
            json_ref_hist(
                "details_json",
                "$.qualification_override.personnel_id",
                "personnel",
            ),
            json_ref_hist("details_json", "$.qualification_override.user_id", USER),
        ],
        // Audits kept on the device on purpose (never staged, so never a stage error): the
        // acknowledgement of a safety rule notice (`settings::received_notice`, the notice itself
        // never travels), and the audits of permit and permit type changes (`permit::queries`):
        // the change itself travels on its own row (who submitted, approved, changed the setting,
        // when and why), while these details hold local ids that mean nothing on another PC. The
        // choice made on a sync conflict (`sync::queries::resolve_sync_conflict`, its reason) stays
        // on the PC with the conflict, which is local-only; the data it changed travels on its row.
        // The audits of stock to reconcile (`inventory::negative_stock`, `inventory::controls`,
        // `inventory::queries`: target `stock_location_article`) name the article, location, count
        // and movement by local ids: kept too; the stock state they describe travels on its rows.
        // Likewise the audits of a reservation released at work-order close or cancel, or released
        // on two PCs (`inventory::reservations`, target `stock_reservation`): local ids of the
        // reservation, article, location, movement and work order; the release itself travels on
        // the reservation row and its release movement.
        // An event already sealed in its local form (`audit::chain`, hash `L1:` / `L2:`: kept on
        // this device when written, its stub travels) never starts travelling later, even when a
        // newer version can encode it: its hash covers its local form, which another PC cannot
        // recompute. At sealing `chain_hash` is still empty, so a new event is judged by the rest of
        // this filter and sealed in the matching form.
        // The audits of a session of this PC (`auth::session_manager`, target `app_session`: a
        // session closed by the stored-state check, a clock anomaly) stay on it too: the session
        // record is local to the device (`app_sessions`) and its id means nothing on another PC; the
        // actor is recorded in the event, on this PC.
        row_filter: Some(
            "{row}.action_code NOT IN ('notification.safety_notice_acknowledged', 'sync.conflict_resolved') \
             AND COALESCE({row}.target_type, '') NOT IN ('work_permit', 'permit_type', 'stock_location_article', 'stock_reservation', 'app_session') \
             AND substr(COALESCE({row}.chain_hash, ''), 1, 3) NOT IN ('L1:', 'L2:')",
        ),
        ..spec(
            "audit_events",
            &[
                hist("actor_id", USER),
                poly_hist(
                    "target_id",
                    "target_type",
                    &[
                        ("user_account", USER),
                        ("user_scope_assignment", "user_scope_assignments"),
                        ("work_order", "work_orders"),
                        // A site given to an old record without one (`org::site_assignment`).
                        ("intervention_request", "intervention_requests"),
                        ("planning_window", "planning_windows"),
                        // A reliability study decision withdrawn or refused (`reliability::study::decisions`):
                        // the target is already the decision's sync id, the same on every PC, sent as it is.
                        ("reliability_study_decisions", NOT_A_ROW),
                        // A break-in signed by a second planner, and a frozen-week conflict closed
                        // by its break-in (`planning::break_in_approval`, `planning::sync_check`).
                        ("schedule_commitment", "schedule_commitments"),
                        // A reopened-week conflict closed by its decision (`planning::sync_check`).
                        ("weekly_schedule", "weekly_schedules"),
                        // A break-in conflict closed once judged valid, and a crew site change
                        // conflict closed once it crosses no frozen week (`planning::sync_check`).
                        ("schedule_break_in", "schedule_break_ins"),
                        ("team", "teams"),
                        // A work-order type given its maintenance nature (`wo::types`): the target
                        // is already the type's sync id (a WORK.ORDER_TYPES value), sent as it is,
                        // so the audit is unchanged when the value is deleted later (a discarded
                        // draft set, a retired domain).
                        ("work_order_type", NOT_A_ROW),
                        // A qualification requirement created or changed
                        // (`qualification::requirements`; requirements are never deleted).
                        ("qualification_requirement", "qualification_requirements"),
                        // A person let into a permit role without the qualifications it requires
                        // (`qualification::gate`, override in a permit role): the target is the
                        // permit, sent as its sync id; the role is in the details (`$.role`).
                        // Events of this type sealed before this mapping existed were sealed in
                        // their local form (`L1:` / `L2:`): the row filter keeps them on their PC
                        // (only their stub travels) and their hash never depends on this mapping.
                        // Rule: a permit named by a sealed audit is never hard-deleted (no code
                        // deletes `work_permits` rows): its sync id is in the sealed hash, and a
                        // deleted target would be encoded empty, breaking that PC's chain.
                        ("permit_role", "work_permits"),
                        // A block's critical flag changed (`personnel::availability`): the target
                        // is already the block's sync id, the same on every PC, sent as it is, so
                        // the audit is unchanged when the block is deleted later.
                        ("personnel_availability_block", NOT_A_ROW),
                        // Device, licence, activation, export, repair and import records stay on
                        // the device they belong to.
                        ("trusted_device", LOCAL_ROW),
                        ("machine_activation_contract", LOCAL_ROW),
                        ("license_admin_action", LOCAL_ROW),
                        ("licensing_trust_key", LOCAL_ROW),
                        // A phone enrolled, cancelled or revoked from "Téléphones"
                        // (`commands::mobile_enrolment`): the target is a licence-server id, a
                        // person's sync id or a local account id, so it is not sent; the licence
                        // server keeps the enrolment's own audit.
                        ("mobile_phone", LOCAL_ROW),
                        ("sync_repair_actions", LOCAL_ROW),
                        ("sync_conflict", LOCAL_ROW),
                        ("audit_events", LOCAL_ROW),
                        ("personnel_import_batch", LOCAL_ROW),
                        // A refused vendor recovery code: the target is the vendor's code id
                        // (`auth::recovery_code`), sent as it is.
                        ("admin_recovery_code", NOT_A_ROW),
                        // A stock count posted on two PCs (`inventory::count_sync`): the target is
                        // already the row's sync id, the same on every PC, sent as it is.
                        ("inventory_transactions", NOT_A_ROW),
                        ("inventory_mutation_audit_links", NOT_A_ROW),
                        ("inventory_count_sessions", NOT_A_ROW),
                        // An equipment move (D19, `assets::lifecycle` `asset.moved`): the target is
                        // the equipment, sent as its sync id (equipment rows are never hard-deleted).
                        ("equipment", "equipment"),
                    ],
                ),
            ],
        )
    },
    // Hash-only stubs of the audit events kept on their PC (`audit::chain`): no reference, no
    // content.
    spec("audit_chain_stubs", &[]),
    RowSpec {
        json_refs: &[
            json_ref_hist("diff_json", "$.role_id", "roles"),
            json_ref_hist("diff_json", "$.assignment_id", "user_scope_assignments"),
        ],
        ..spec(
            "admin_change_events",
            &[
                hist("actor_id", USER),
                hist("target_user_id", USER),
                hist("target_role_id", "roles"),
                poly_hist("scope_reference", "scope_type", SCOPE_TARGETS),
            ],
        )
    },
    RowSpec {
        // Changes the application makes by itself on each device (seeding at startup) stay local.
        row_filter: Some("{row}.changed_by_id IS NOT NULL"),
        ..spec("settings_change_events", &[hist("changed_by_id", USER)])
    },
    // Per-site rules and their audit trail (`settings::site`).
    spec("site_settings", &[]),
    spec("site_setting_events", &[]),
    // ── Notifications ───────────────────────────────────────────────────────────────────────
    RowSpec {
        // The safety rule notice is local to each PC (`settings::received_notice`): its category
        // and rule rows never travel (each PC has them from migration 246).
        // Same for the notice of a forced lock removal (migration 249).
        row_filter: Some(concat!(
            "COALESCE({row}.code, '') NOT IN ",
            crate::safety_notice_categories_sql!()
        )),
        ..spec("notification_categories", &[])
    },
    spec("notification_escalation_policies", &[]),
    RowSpec {
        row_filter: Some(concat!(
            "COALESCE({row}.category_code, '') NOT IN ",
            crate::safety_notice_categories_sql!()
        )),
        ..spec(
            "notification_rules",
            &[r("escalation_policy_id", "notification_escalation_policies")],
        )
    },
    spec(
        "notification_escalation_steps",
        &[
            r("escalation_policy_id", "notification_escalation_policies"),
            poly(
                "route_to_reference",
                "route_to_type",
                &[
                    ("role", NOT_A_ROW),
                    ("team", "teams"),
                    ("user", USER),
                    // Resolved from the record and the people notified; `permission` holds a code.
                    ("supervisor", NOT_A_ROW),
                    ("planner", NOT_A_ROW),
                    ("team_lead", NOT_A_ROW),
                    ("entity_manager", NOT_A_ROW),
                    ("permission", NOT_A_ROW),
                ],
            ),
        ],
    ),
    RowSpec {
        // Ids the escalation routing reads from the payload (`notifications::router`).
        json_refs: &[
            json_ref_hist("payload_json", "$.team_id", "teams"),
            json_ref_hist("payload_json", "$.assigned_team_id", "teams"),
            json_ref_hist("payload_json", "$.org_node_id", "org_nodes"),
            json_ref_hist("payload_json", "$.entity_id", "org_nodes"),
            json_ref_hist("payload_json", "$.watcher_user_ids[*]", USER),
            // Ids the event texts and links read (`pm::queries`, `planning::scheduling`, finance).
            json_ref_hist("payload_json", "$.pm_occurrence_id", "pm_occurrences"),
            json_ref_hist("payload_json", "$.pm_plan_id", "pm_plans"),
            json_ref_hist("payload_json", "$.di_id", "intervention_requests"),
            json_ref_hist("payload_json", "$.break_in_id", "schedule_break_ins"),
            json_ref_hist("payload_json", "$.schedule_commitment_id", "schedule_commitments"),
            json_ref_hist("payload_json", "$.commitment_id", "schedule_commitments"),
            json_ref_hist("payload_json", "$.impacted_assignee_id", "personnel"),
            json_ref_hist("payload_json", "$.assignee_id", "personnel"),
        ],
        // The de-duplication key is built from local ids (`di-…-<id>`, `planning.notify.<id>…`):
        // received on another device it would suppress an unrelated event there. Each device keeps
        // its own keys; received events carry none. Whether the event's condition cleared is
        // computed by each device from synchronised data (`notifications::conditions`).
        device_local: &["dedupe_key", "resolved_at"],
        // The notice of a safety rule changed on another PC is about the apply on this PC: each PC
        // that applies the change tells its own administrators; it never travels
        // (`settings::received_notice`).
        row_filter: Some(concat!(
            "COALESCE({row}.category_code, '') NOT IN ",
            crate::safety_notice_categories_sql!()
        )),
        ..spec(
            "notification_events",
            &[
                // Org node of the record, for read-time visibility (`notifications::inbox`).
                hist("org_node_id", "org_nodes"),
                poly_hist(
                    "source_record_id",
                    "source_module",
                    &[
                        ("di", "intervention_requests"),
                        ("intervention_requests", "intervention_requests"),
                        ("pm", "pm_occurrences"),
                        ("personnel", "personnel"),
                        ("planning", "schedule_commitments"),
                        ("finance_budget", "budget_versions"),
                        ("work_orders", "work_orders"),
                        ("wo", "work_orders"),
                        ("inspection", "inspection_rounds"),
                        ("inventory", "articles"),
                        ("permit", "work_permits"),
                        // The daily digest names its day (`2026-10-15`); conflicts carry no record.
                        ("notifications", NOT_A_ROW),
                        ("sync", NOT_A_ROW),
                        // The safety rule notice names the received row by its sync id; it stays on
                        // the PC (row filter above).
                        ("settings", NOT_A_ROW),
                    ],
                ),
            ],
        )
    },
    RowSpec {
        // Links to the record the notification is about (`/requests?openDi=12`).
        json_refs: &[
            json_ref_hist("action_url", "?openDi", "intervention_requests"),
            json_ref_hist("action_url", "?di", "intervention_requests"),
            json_ref_hist("action_url", "?occurrence", "pm_occurrences"),
            json_ref_hist("action_url", "?openWo", "work_orders"),
            json_ref_hist("action_url", "?openRound", "inspection_rounds"),
            json_ref_hist("action_url", "?articleId", "articles"),
            json_ref_hist("action_url", "?permit", "work_permits"),
        ],
        // Whether the condition cleared is computed by each device (`notifications::conditions`);
        // whether the condition was already true when this device started watching it, too.
        device_local: &["resolved_at", "already_true"],
        // Reading or archiving the notice of a safety rule changed on another PC stays on this PC:
        // its event never travels, so another PC would wait for it forever.
        row_filter: Some(concat!(
            "NOT EXISTS (SELECT 1 FROM notification_events e WHERE e.id = {row}.notification_event_id \
             AND e.category_code IN ",
            crate::safety_notice_categories_sql!(),
            ")"
        )),
        ..spec(
            "notifications",
            &[
                r("notification_event_id", "notification_events"),
                r("recipient_user_id", USER),
                r("recipient_role_id", "roles"),
            ],
        )
    },
    RowSpec {
        // An acknowledgement of the safety rule notice stays on this PC with its notification.
        row_filter: Some(concat!(
            "NOT EXISTS (SELECT 1 FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
             WHERE n.id = {row}.notification_id AND e.category_code IN ",
            crate::safety_notice_categories_sql!(),
            ")"
        )),
        ..spec(
            "notification_acknowledgements",
            &[r("notification_id", "notifications"), r("acknowledged_by_id", USER)],
        )
    },
    // ── Reports ─────────────────────────────────────────────────────────────────────────────
    spec("report_templates", &[]),
    RowSpec {
        // Each device of the user runs the schedule with its own run times: the next run is
        // computed from the cron expression on apply (`apply_hooks`), this device has not run it.
        derived: &[
            ("next_run_at", "strftime('%Y-%m-%dT%H:%M:%SZ','now')"),
            ("last_run_at", "NULL"),
        ],
        ..spec(
            "report_schedules",
            &[r("user_id", USER), r("template_id", "report_templates")],
        )
    },
    // ── Personal items (per user, only to that user's devices) ─────────────────────────────
    RowSpec {
        // The safety rule notice ignores preferences and its preference rows stay on the PC.
        row_filter: Some(concat!(
            "COALESCE({row}.category_code, '') NOT IN ",
            crate::safety_notice_categories_sql!()
        )),
        ..spec("notification_preferences", &[r("user_id", USER)])
    },
    RowSpec {
        // Ids a saved activity filter narrows on (`commands::activity_feed::ActivityFilter`).
        json_refs: &[
            json_ref_hist("filter_json", "$.entity_scope_id", "org_nodes"),
            json_ref_hist("filter_json", "$.actor_id", USER),
        ],
        ..spec("saved_activity_filters", &[r("user_id", USER)])
    },
    // Keyed by the user: one layout per person (widget ids only, no row ids).
    spec("user_dashboard_layouts", &[r("user_id", USER)]),
    spec("user_analytics_dashboard_layouts", &[r("user_id", USER)]),
    // ── Inventory and procurement ───────────────────────────────────────────────────────────
    spec("article_families", &[]),
    spec("articles", &[]),
    spec("unit_conversions", &[]),
    spec("inventory_lots", &[]),
    spec("currency_exchange_rates", &[]),
    spec("inventory_valuation_periods", &[]),
    spec("inventory_valuation_snapshot_lines", &[]),
    spec("procurement_approval_limits", &[]),
    spec("article_cost_profiles", &[]),
    spec("warehouses", &[]),
    spec("stock_locations", &[]),
    spec(
        "stock_reservations",
        &[
            poly("source_id", "source_type", RESERVATION_SOURCE_TARGETS),
            r("created_by_id", USER),
        ],
    ),
    spec(
        "inventory_transactions",
        &[
            poly("source_id", "source_type", STOCK_SOURCE_TARGETS),
            r("performed_by_id", USER),
        ],
    ),
    spec("inventory_article_equivalents", &[]),
    spec(
        "inventory_count_sessions",
        &[
            r("opened_by_id", USER),
            r("submitted_by_id", USER),
            r("posted_by_id", USER),
            r("reversed_by_id", USER),
        ],
    ),
    RowSpec {
        // The row version at which this PC raised the line's "above the tolerance" flag from a
        // received "within" (`inventory::count_four_eyes::store_judged_critique`): it describes this
        // PC's own judgement, so it never travels.
        device_local: &["critique_raised_version"],
        ..spec(
            "inventory_count_lines",
            &[
                r("approved_by_id", USER),
                r("counted_by_id", USER),
                r("second_approved_by_id", USER),
            ],
        )
    },
    spec(
        "inventory_document_links",
        &[
            poly(
                "entity_id",
                "entity_type",
                &[
                    ("SUPPLIER", "inventory_suppliers"),
                    ("PURCHASE_ORDER", "purchase_orders"),
                    ("REPAIRABLE_ORDER", "repairable_orders"),
                    ("ARTICLE", "articles"),
                ],
            ),
            r("created_by_id", USER),
        ],
    ),
    spec(
        "inventory_mutation_audit_links",
        &[
            poly(
                "source_entity_id",
                "source_entity_type",
                &[("inventory_count_line", "inventory_count_lines")],
            ),
            r("reviewer_id", USER),
        ],
    ),
    spec("inventory_reconciliation_runs", &[r("checked_by_id", USER)]),
    spec("inventory_reconciliation_findings", &[]),
    spec(
        "inventory_state_events",
        &[
            poly_hist(
                "entity_id",
                "entity_type",
                &[
                    ("purchase_order", "purchase_orders"),
                    ("repairable_order", "repairable_orders"),
                    ("requisition", "procurement_requisitions"),
                    // Article creation and edits (`inventory::queries`).
                    ("ARTICLE", "articles"),
                ],
            ),
            hist("actor_id", USER),
        ],
    ),
    spec("inventory_supplier_article_sources", &[]),
    spec("inventory_supplier_contacts", &[]),
    spec(
        "inventory_supplier_prices",
        &[
            r("currency_value_id", "lookup_values"),
            // Price unit: a unit of MEASURE.UNITS (migration 201).
            r("price_unit_value_id", "reference_values"),
        ],
    ),
    spec(
        "inventory_suppliers",
        &[
            r("currency_value_id", "lookup_values"),
            r("default_buyer_person_id", "personnel"),
        ],
    ),
    spec("inventory_valuation_policies", &[]),
    spec(
        "procurement_requisitions",
        &[
            poly_soft("demand_source_id", "demand_source_type", DEMAND_SOURCE_TARGETS),
            r("requested_by_id", USER),
        ],
    ),
    spec("procurement_requisition_lines", &[unmapped("demand_source_line_id")]),
    spec(
        "purchase_orders",
        &[r("ordered_by_id", USER), r("approved_by_id", USER)],
    ),
    spec(
        "purchase_order_lines",
        &[
            poly_soft("demand_source_id", "demand_source_type", DEMAND_SOURCE_TARGETS),
            unmapped("demand_source_line_id"),
        ],
    ),
    spec("goods_receipts", &[r("received_by_id", USER)]),
    spec("goods_receipt_lines", &[]),
    spec("repairable_orders", &[r("created_by_id", USER)]),
    // ── Finance: OT cost flow and budget periods ────────────────────────────────────────────
    spec("wo_cost_estimates", &[]),
    spec("wo_cost_settlements", &[]),
    spec("budget_periods", &[]),
    // Personnel, qualifications and training (migrations 191-197).
    spec("qualification_requirements", &[]),
    spec("qualification_overrides", &[]),
    spec("work_calendar_holidays", &[]),
    spec("site_work_weeks", &[]),
    spec("schedule_class_versions", &[]),
    spec("team_schedule_assignments", &[]),
    spec("personnel_schedule_exceptions", &[]),
    // Vendor administrator recovery notices and their acknowledgements (`auth::recovery_code`):
    // append-only, references by declared foreign keys (recipient, created account, notice, who
    // acknowledged); `code_id` is the vendor's code id (a value: the spent register stays local).
    spec("admin_recovery_notices", &[]),
    spec("admin_recovery_acknowledgements", &[]),
];

/// Rules of an entity type synchronised by the generic engine.
pub fn row_spec(entity_type: &str) -> Option<&'static RowSpec> {
    ROW_SPECS.iter().find(|s| s.entity_type == entity_type)
}
