//! Master data of the demo company, written through the services the screens use: governed
//! reference values, organisation, cost centres, assets, people and logins, stock, suppliers,
//! budgets, preventive plans and inspection routes.

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use serde::de::DeserializeOwned;
use serde_json::json;

use super::catalog::{self, Duty};
use super::{column_i64, lookup_value_id, ref_value_id, scalar_i64, scalar_text, Ctx, GenerationOptions};
use crate::auth::session_manager::AuthenticatedUser;
use crate::errors::{AppError, AppResult};
use crate::reference::governance::{self, GovernanceCategory};
use crate::state::AppState;

/// Algeria is UTC+1 all year (no daylight saving time).
pub const UTC_OFFSET_HOURS: i64 = 1;

/// A local (Algerian) time of `day` as UTC.
pub fn local(day: NaiveDate, hour: u32, minute: u32) -> DateTime<Utc> {
    day.and_hms_opt(hour, minute, 0).expect("valid time").and_utc() - Duration::hours(UTC_OFFSET_HOURS)
}

/// Local calendar day of a UTC moment.
pub fn local_day(at: DateTime<Utc>) -> NaiveDate {
    (at + Duration::hours(UTC_OFFSET_HOURS)).date_naive()
}

/// Algerian working week: Sunday to Thursday.
pub fn is_working_day(day: NaiveDate) -> bool {
    !matches!(day.weekday(), chrono::Weekday::Fri | chrono::Weekday::Sat)
}

pub(crate) fn de<T: DeserializeOwned>(value: serde_json::Value) -> AppResult<T> {
    serde_json::from_value(value).map_err(|e| AppError::Internal(anyhow::anyhow!("demo payload: {e}")))
}

pub fn rfc(at: DateTime<Utc>) -> String {
    at.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[derive(Debug, Clone)]
pub struct AssetInfo {
    pub id: i64,
    pub code: &'static str,
    pub class: &'static str,
    pub criticality: &'static str,
    pub site: &'static str,
    pub node_id: i64,
    pub failures_per_year: f64,
    pub hours_per_day: f64,
    pub loto: bool,
    pub is_component: bool,
}

#[derive(Debug, Clone)]
pub struct PersonInfo {
    pub personnel_id: i64,
    pub user_id: Option<i64>,
    pub site: &'static str,
    pub duty: Duty,
    pub team_id: i64,
    pub team: &'static str,
    pub name: &'static str,
    pub skill: &'static str,
}

#[derive(Debug, Clone)]
pub struct ArticleInfo {
    pub id: i64,
    pub code: &'static str,
    pub unit_cost: f64,
    pub reorder: f64,
    pub max: f64,
    pub used_by: &'static [&'static str],
    pub critical: bool,
}

#[derive(Debug, Clone)]
pub struct PmPlanInfo {
    pub plan_id: i64,
    pub asset_code: &'static str,
    pub hours: f64,
}

#[derive(Debug, Clone)]
pub struct InspectionRoute {
    pub template_id: i64,
    pub site: &'static str,
    /// (checkpoint id, kind, asset id) in sequence order.
    pub checkpoints: Vec<(i64, CheckKind, Option<i64>)>,
}

#[derive(Debug, Clone, Copy)]
pub enum CheckKind {
    /// Air network pressure (bar): normal 6.5–7.5, warning 6.0–8.0.
    Pressure,
    /// Temperature (°C): normal 40–75, warning 30–85.
    Temperature,
    /// Yes/no check (fail on false).
    Boolean,
}

/// Everything the history needs from the master data.
pub struct Company {
    pub admin_id: i64,
    pub nodes: BTreeMap<String, i64>,
    pub assets: Vec<AssetInfo>,
    pub people: Vec<PersonInfo>,
    pub teams: BTreeMap<&'static str, i64>,
    pub articles: Vec<ArticleInfo>,
    /// Site -> (warehouse id, stock location id).
    pub stores: BTreeMap<&'static str, (i64, i64)>,
    pub suppliers: Vec<i64>,
    pub pm_plans: Vec<PmPlanInfo>,
    pub routes: Vec<InspectionRoute>,
    pub refs: Refs,
}

/// Reference value ids the history uses.
#[derive(Debug, Clone, Default)]
pub struct Refs {
    pub symptoms: BTreeMap<String, i64>,
    pub modes: BTreeMap<String, i64>,
    pub causes: BTreeMap<String, i64>,
    pub delay_waiting_part: i64,
    pub loto_permit_type: i64,
    pub energy_electrical: i64,
    pub energy_hydraulic: i64,
    pub method_breaker: i64,
    pub method_valve: i64,
}

impl Company {
    pub fn person(&self, site: &str, duty: Duty) -> &PersonInfo {
        self.people
            .iter()
            .find(|p| p.site == site && p.duty == duty)
            .expect("catalog has every duty on every site")
    }

    pub fn people_with(&self, site: &str, duties: &[Duty]) -> Vec<&PersonInfo> {
        self.people
            .iter()
            .filter(|p| p.site == site && duties.contains(&p.duty))
            .collect()
    }

    pub fn user(&self, site: &str, duty: Duty) -> i64 {
        self.person(site, duty).user_id.expect("duty with a login")
    }
}

pub async fn build(ctx: &mut Ctx<'_>, _opts: &GenerationOptions, start_day: NaiveDate) -> AppResult<Company> {
    let (admin_id, admin) = onboarding_admin(ctx).await?;
    let refs = reference_data(ctx, admin_id).await?;
    let schedule_id = working_schedule(ctx, admin_id).await?;
    let nodes = organisation(ctx, admin_id as i32).await?;
    cost_centers(ctx, &nodes).await?;
    let assets = assets(ctx, &nodes, admin_id as i32, start_day).await?;
    let (people, teams) = people(ctx, &nodes, schedule_id, admin_id, &admin, start_day).await?;
    let (stores, articles) = stock(ctx, admin_id, start_day).await?;
    let suppliers = suppliers(ctx).await?;
    budgets(ctx, admin_id, start_day).await?;
    let mut company = Company {
        admin_id,
        nodes,
        assets,
        people,
        teams,
        articles,
        stores,
        suppliers,
        pm_plans: Vec::new(),
        routes: Vec::new(),
        refs,
    };
    company.pm_plans = pm_plans(ctx, &company, start_day).await?;
    company.routes = inspection_routes(ctx, &company).await?;
    Ok(company)
}

// ── Onboarding administrator ─────────────────────────────────────────────────────────────────

async fn onboarding_admin(ctx: &mut Ctx<'_>) -> AppResult<(i64, AuthenticatedUser)> {
    let id = scalar_i64(
        ctx.db,
        "SELECT ua.id FROM user_accounts ua \
         JOIN user_scope_assignments usa ON usa.user_id = ua.id AND usa.deleted_at IS NULL \
         JOIN roles r ON r.id = usa.role_id \
         WHERE r.name = 'Administrator' AND ua.is_active = 1 AND ua.deleted_at IS NULL \
         ORDER BY ua.id LIMIT 1",
        vec![],
    )
    .await?
    .ok_or_else(|| AppError::ValidationFailed(vec!["Aucun administrateur actif dans la base.".into()]))?;
    let username = scalar_text(
        ctx.db,
        "SELECT username FROM user_accounts WHERE id = ?",
        vec![id.into()],
    )
    .await?
    .unwrap_or_default();
    let caller = AuthenticatedUser {
        user_id: i32::try_from(id).map_err(|e| AppError::Internal(anyhow::anyhow!("{e}")))?,
        username,
        display_name: None,
        is_admin: true,
        force_password_change: false,
        tenant_id: String::new(),
        token_tenant_id: String::new(),
    };
    Ok((id, caller))
}

// ── Reference data (Données de référence) ────────────────────────────────────────────────────

/// Adds the missing `values` (code, label, parent id) to a governed list, the way the reference
/// screen does for that list's governance: straight into the published set (operational lists)
/// or through a draft that is validated and published (controlled lists).
async fn ensure_values(
    ctx: &mut Ctx<'_>,
    domain_code: &str,
    values: &[(&str, &str, Option<i64>)],
    actor: i64,
) -> AppResult<()> {
    let mut missing = Vec::new();
    for v in values {
        if ref_value_id(ctx.db, domain_code, v.0).await.is_err() {
            missing.push(*v);
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    let domain = crate::reference::domains::get_reference_domain_by_code(ctx.db, domain_code).await?;
    match governance::category_of(&domain) {
        GovernanceCategory::OperationalDictionary => {
            for (code, label, parent) in missing {
                ctx.tick()?;
                crate::reference::values::create_operational_value(
                    ctx.db,
                    crate::reference::values::CreateOperationalReferenceValuePayload {
                        domain_code: domain_code.to_string(),
                        label: label.to_string(),
                        description: None,
                        parent_id: parent,
                        code: Some(code.to_string()),
                    },
                    actor,
                )
                .await?;
                ctx.count("valeurs de référence", 1);
            }
        }
        GovernanceCategory::ControlledCatalog => {
            ctx.tick()?;
            let draft = crate::reference::sets::create_draft_set(ctx.db, domain.id, actor).await?;
            for (code, label, parent) in missing {
                ctx.tick()?;
                crate::reference::values::create_value(
                    ctx.db,
                    crate::reference::values::CreateReferenceValuePayload {
                        set_id: draft.id,
                        parent_id: parent,
                        code: code.to_string(),
                        label: label.to_string(),
                        description: None,
                        sort_order: None,
                        color_hex: None,
                        icon_name: None,
                        semantic_tag: None,
                        external_code: None,
                        metadata_json: None,
                    },
                    actor,
                )
                .await?;
                ctx.count("valeurs de référence", 1);
            }
            ctx.tick()?;
            crate::reference::sets::validate_set(ctx.db, draft.id, actor).await?;
            ctx.tick()?;
            crate::reference::sets::publish_set(ctx.db, draft.id, actor).await?;
        }
        GovernanceCategory::SystemCatalog => {
            return Err(AppError::ValidationFailed(vec![format!(
                "La liste {domain_code} est une liste système: le générateur ne peut pas y ajouter de valeur."
            )]));
        }
    }
    Ok(())
}

async fn reference_data(ctx: &mut Ctx<'_>, admin: i64) -> AppResult<Refs> {
    let classes: Vec<(&str, &str, Option<i64>)> = catalog::EXTRA_CLASSES.iter().map(|(c, l)| (*c, *l, None)).collect();
    ensure_values(ctx, "EQUIPMENT.CLASS", &classes, admin).await?;
    crate::assets::taxonomy_reference::sync_equipment_classes_from_published_reference(ctx.db).await?;

    ensure_values(
        ctx,
        "WORK.DELAY_REASONS",
        &[
            ("ATTENTE_PIECE", "Attente pièce de rechange", None),
            ("ATTENTE_LIBERATION", "Machine non libérée par la production", None),
        ],
        admin,
    )
    .await?;
    ensure_values(
        ctx,
        "DI.RETURN_REASON",
        &[(
            "INFO_INSUFFISANTE",
            "Description insuffisante, préciser le symptôme",
            None,
        )],
        admin,
    )
    .await?;
    ensure_values(
        ctx,
        "DI.DEFERRAL_REASON",
        &[("ARRET_PROGRAMME", "Reporté à l'arrêt programmé", None)],
        admin,
    )
    .await?;
    ensure_values(
        ctx,
        "PERMIT.HANDOVER_ROLES",
        &[
            ("EXECUTANT", "Exécutant des travaux", None),
            ("EMETTEUR", "Émetteur du permis", None),
            ("PRODUCTION", "Responsable de production", None),
        ],
        admin,
    )
    .await?;

    let mut refs = Refs::default();
    for story in catalog::FAILURE_STORIES {
        refs.symptoms.insert(
            story.symptom.to_string(),
            ref_value_id(ctx.db, "WORK.SYMPTOMS", story.symptom).await?,
        );
        refs.modes.insert(
            story.mode.to_string(),
            ref_value_id(ctx.db, "WORK.FAILURE_MODES", story.mode).await?,
        );
        refs.causes.insert(
            story.cause.to_string(),
            ref_value_id(ctx.db, "WORK.FAILURE_CAUSES", story.cause).await?,
        );
    }
    refs.delay_waiting_part = ref_value_id(ctx.db, "WORK.DELAY_REASONS", "ATTENTE_PIECE").await?;
    refs.loto_permit_type = scalar_i64(ctx.db, "SELECT id FROM permit_types WHERE code = 'loto'", vec![])
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "permit_types".into(),
            id: "loto".into(),
        })?;
    refs.energy_electrical = ref_value_id(ctx.db, "PERMIT.ENERGY_TYPES", "ELECTRICAL").await?;
    refs.energy_hydraulic = ref_value_id(ctx.db, "PERMIT.ENERGY_TYPES", "HYDRAULIC").await?;
    refs.method_breaker = ref_value_id(ctx.db, "PERMIT.ISOLATION_METHODS", "BREAKER_LOCKOUT").await?;
    refs.method_valve = ref_value_id(ctx.db, "PERMIT.ISOLATION_METHODS", "VALVE_LOCKOUT").await?;
    Ok(refs)
}

/// The plant's working time: Sunday to Thursday, 08:00–16:30 (Friday and Saturday off).
async fn working_schedule(ctx: &mut Ctx<'_>, admin: i64) -> AppResult<i64> {
    ensure_values(
        ctx,
        "ORG.SCHEDULE_CLASS",
        &[("JOURNEE_DZ", "Journée dimanche–jeudi 08:00–16:30", None)],
        admin,
    )
    .await?;
    let id = ref_value_id(ctx.db, "ORG.SCHEDULE_CLASS", "JOURNEE_DZ").await?;
    // day_of_week: 1 = Monday … 7 = Sunday.
    let details = (1..=7)
        .map(|day| crate::reference::schedule_patterns::ScheduleDayPattern {
            day_of_week: day,
            shift_start: "08:00".into(),
            shift_end: "16:30".into(),
            is_rest_day: day == 5 || day == 6,
        })
        .collect();
    ctx.tick()?;
    crate::reference::schedule_patterns::upsert_schedule_pattern(
        ctx.db,
        crate::reference::schedule_patterns::UpsertSchedulePatternPayload {
            reference_value_id: id,
            shift_pattern_code: None,
            is_continuous: Some(false),
            nominal_hours_per_day: Some(8.5),
            details: Some(details),
        },
        admin,
    )
    .await?;

    // The planning calendar: Friday and Saturday are the weekly rest days of every site, and the
    // Classe horaire version that capacity reads (day 1 of the rotation is a Monday).
    let today = local_day(ctx.last);
    ctx.tick()?;
    crate::personnel::calendar::set_site_work_week(
        ctx.db,
        crate::personnel::calendar::SiteWorkWeekInput {
            entity_id: None,
            effective_from: today.format("%Y-%m-%d").to_string(),
            weekend_days: vec![5, 6],
        },
        admin,
    )
    .await?;
    let monday = today - Duration::days(i64::from(today.weekday().num_days_from_monday()));
    ctx.tick()?;
    crate::personnel::timetable::upsert_class_version(
        ctx.db,
        de(json!({
            "id": null,
            "expected_row_version": null,
            "reference_value_id": id,
            "effective_from": today.format("%Y-%m-%d").to_string(),
            "cycle_length_days": 7,
            "rotation_start_date": monday.format("%Y-%m-%d").to_string(),
            "days": (1..=7).map(|d| json!({
                "cycle_day": d,
                "shift_start": "08:00",
                "shift_end": "16:30",
                "break_minutes": 30,
                "is_rest_day": d == 5 || d == 6,
            })).collect::<Vec<_>>(),
            "is_continuous": false,
            "utilisation_percent": 100.0,
            "note": "Horaire de journée des équipes de maintenance",
        }))?,
        admin,
    )
    .await?;
    Ok(id)
}

// ── Organisation ─────────────────────────────────────────────────────────────────────────────

async fn organisation(ctx: &mut Ctx<'_>, admin: i32) -> AppResult<BTreeMap<String, i64>> {
    use crate::org::{node_types, relationship_rules, structure_model, validation};

    let has_draft = scalar_i64(
        ctx.db,
        "SELECT COUNT(*) FROM org_structure_models WHERE status = 'draft'",
        vec![],
    )
    .await?
    .unwrap_or(0)
        > 0;
    if has_draft {
        return Err(AppError::ValidationFailed(vec![
            "Un modèle d'organisation en brouillon existe déjà: publiez-le ou abandonnez-le d'abord.".into(),
        ]));
    }
    ctx.tick()?;
    let payload = structure_model::CreateStructureModelPayload {
        description: Some(format!("Organisation {}", catalog::COMPANY_NAME)),
    };
    // A licensed company already has the root written at activation: extend its published model.
    let active = structure_model::get_active_model(ctx.db).await?;
    let model = match active {
        Some(_) => structure_model::fork_draft_from_published(ctx.db, &payload, admin).await?,
        None => structure_model::create_model(ctx.db, payload, admin).await?,
    };
    let model_id = model.id;

    let root_type = scalar_i64(
        ctx.db,
        "SELECT id FROM org_node_types WHERE structure_model_id = ? AND is_root_type = 1 LIMIT 1",
        vec![model_id.into()],
    )
    .await?;
    let mut type_ids: BTreeMap<&str, i64> = BTreeMap::new();
    let specs: [(&str, &str, bool, bool, bool, i32); 4] = [
        ("ENTREPRISE", "Entreprise", false, false, true, 0),
        ("SITE", "Site", false, true, true, 1),
        ("ATELIER", "Atelier", false, true, false, 2),
        ("LIGNE", "Ligne de production", true, true, false, 3),
    ];
    for (code, label, hosts_assets, owns_work, carries_cc, depth) in specs {
        if code == "ENTREPRISE" {
            if let Some(id) = root_type {
                type_ids.insert(code, id);
                continue;
            }
        }
        ctx.tick()?;
        let t = node_types::create_node_type(
            ctx.db,
            node_types::CreateNodeTypePayload {
                structure_model_id: model_id,
                code: code.to_string(),
                label: label.to_string(),
                icon_key: None,
                color: None,
                depth_hint: Some(depth),
                can_host_assets: hosts_assets,
                can_own_work: owns_work,
                can_carry_cost_center: carries_cc,
                can_aggregate_kpis: true,
                can_receive_permits: depth >= 1,
                is_root_type: code == "ENTREPRISE",
            },
        )
        .await?;
        type_ids.insert(code, i64::from(t.id));
    }
    for (parent, child) in [("ENTREPRISE", "SITE"), ("SITE", "ATELIER"), ("ATELIER", "LIGNE")] {
        ctx.tick()?;
        relationship_rules::create_rule(
            ctx.db,
            relationship_rules::CreateRelationshipRulePayload {
                structure_model_id: model_id,
                parent_type_id: type_ids[parent] as i32,
                child_type_id: type_ids[child] as i32,
                min_children: None,
                max_children: None,
            },
        )
        .await?;
    }

    let existing_root = scalar_i64(
        ctx.db,
        "SELECT id FROM org_nodes WHERE structure_model_id = ? AND parent_id IS NULL AND deleted_at IS NULL \
         ORDER BY id LIMIT 1",
        vec![model_id.into()],
    )
    .await?;
    let model_id = i64::from(model_id);
    let root_id = match existing_root {
        Some(id) => id,
        None => {
            ctx.tick()?;
            create_node(
                ctx,
                model_id,
                type_ids["ENTREPRISE"],
                None,
                catalog::COMPANY_CODE,
                catalog::COMPANY_NAME,
                None,
            )
            .await?
        }
    };
    let mut ids: BTreeMap<String, i64> = BTreeMap::new();
    for site in catalog::SITES {
        let id = create_node(
            ctx,
            model_id,
            type_ids["SITE"],
            Some(root_id),
            site.code,
            site.name,
            Some(site.cost_center),
        )
        .await?;
        ids.insert(site.code.to_string(), id);
    }
    for area in catalog::AREAS {
        let parent = ids[area.site];
        let id = create_node(
            ctx,
            model_id,
            type_ids["ATELIER"],
            Some(parent),
            area.code,
            area.name,
            None,
        )
        .await?;
        ids.insert(area.code.to_string(), id);
    }
    for line in catalog::LINES {
        let parent = ids[line.area];
        let id = create_node(
            ctx,
            model_id,
            type_ids["LIGNE"],
            Some(parent),
            line.code,
            line.name,
            None,
        )
        .await?;
        ids.insert(line.code.to_string(), id);
    }
    ctx.count("nœuds d'organisation", ids.len() as i64);

    ctx.tick()?;
    let check = validation::validate_draft_model_for_publish(ctx.db, model_id).await?;
    if !check.can_publish {
        return Err(AppError::ValidationFailed(vec![format!(
            "Organisation refusée à la publication: {:?}",
            check.issues
        )]));
    }
    ctx.tick()?;
    validation::publish_model_with_remap(ctx.db, model_id, admin).await?;

    // Node ids as published (read back by code from the active model).
    let mut published = BTreeMap::new();
    for code in ids.keys() {
        let id = scalar_i64(
            ctx.db,
            "SELECT n.id FROM org_nodes n JOIN org_structure_models m ON m.id = n.structure_model_id \
             WHERE m.status = 'active' AND n.code = ? AND n.deleted_at IS NULL LIMIT 1",
            vec![code.clone().into()],
        )
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "org_nodes".into(),
            id: code.clone(),
        })?;
        published.insert(code.clone(), id);
    }
    Ok(published)
}

async fn create_node(
    ctx: &mut Ctx<'_>,
    model_id: i64,
    type_id: i64,
    parent: Option<i64>,
    code: &str,
    name: &str,
    cost_center: Option<&str>,
) -> AppResult<i64> {
    ctx.tick()?;
    let node = crate::org::nodes::create_org_node(
        ctx.db,
        crate::org::nodes::CreateOrgNodePayload {
            code: code.to_string(),
            name: name.to_string(),
            node_type_id: type_id,
            parent_id: parent,
            description: None,
            cost_center_code: cost_center.map(str::to_string),
            external_reference: None,
            effective_from: None,
            erp_reference: None,
            notes: None,
            structure_model_id: model_id,
        },
        0,
    )
    .await?;
    Ok(node.id)
}

async fn cost_centers(ctx: &mut Ctx<'_>, nodes: &BTreeMap<String, i64>) -> AppResult<()> {
    for site in catalog::SITES {
        ctx.tick()?;
        crate::finance::queries::create_cost_center(
            ctx.db,
            de(json!({
                "code": site.cost_center,
                "name": site.cost_center_name,
                "entity_id": nodes[site.code],
                "is_active": true,
            }))?,
        )
        .await?;
        ctx.count("centres de coût", 1);
    }
    Ok(())
}

// ── Assets ───────────────────────────────────────────────────────────────────────────────────

async fn assets(
    ctx: &mut Ctx<'_>,
    nodes: &BTreeMap<String, i64>,
    admin: i32,
    start_day: NaiveDate,
) -> AppResult<Vec<AssetInfo>> {
    let mut out: Vec<AssetInfo> = Vec::new();
    let site_of_line = |line: &str| {
        let area = catalog::LINES.iter().find(|l| l.code == line).expect("line").area;
        catalog::AREAS.iter().find(|a| a.code == area).expect("area").site
    };
    for (i, m) in catalog::MACHINES.iter().enumerate() {
        let site = site_of_line(m.line);
        let cc = catalog::SITES
            .iter()
            .find(|s| s.code == site)
            .expect("site")
            .cost_center;
        // Commissioned 3 to 14 years before the history (fixed by position, not random).
        let years = 3 + (i as i64 * 7) % 12;
        let commissioned = start_day - Duration::days(365 * years + (i as i64 * 37) % 300);
        ctx.tick()?;
        let asset = crate::assets::identity::create_asset(
            ctx.db,
            de(json!({
                "asset_code": m.code,
                "asset_name": m.name,
                "class_code": m.class,
                "family_code": null,
                "subfamily_code": null,
                "criticality_code": m.criticality,
                "status_code": "ACTIVE_IN_SERVICE",
                "manufacturer": m.manufacturer,
                "model": m.model,
                "serial_number": format!("{}-{:05}", m.model, 10_000 + i * 173),
                "maintainable_boundary": true,
                "rams_schedule_reference_value_id": null,
                "rams_utilization_factor": null,
                "org_node_id": nodes[m.line],
                "commissioned_at": commissioned.format("%Y-%m-%d").to_string(),
                "commercial": {
                    "purchase_date": commissioned.format("%Y-%m-%d").to_string(),
                    "warranty_expiry_date": (commissioned + Duration::days(730)).format("%Y-%m-%d").to_string(),
                    "replacement_value": m.replacement_value,
                    "cost_center_code": cc,
                    "notes": null,
                },
            }))?,
            admin,
        )
        .await?;
        if let Some(parent_code) = m.parent {
            let parent = out.iter().find(|a| a.code == parent_code).expect("parent listed first");
            ctx.tick()?;
            crate::assets::hierarchy::link_asset_hierarchy(
                ctx.db,
                crate::assets::hierarchy::LinkAssetPayload {
                    parent_asset_id: parent.id,
                    child_asset_id: asset.id,
                    relation_type: "PARENT_CHILD".into(),
                    effective_from: Some(commissioned.format("%Y-%m-%d").to_string()),
                },
                admin,
            )
            .await?;
            ctx.count("liens composant / machine", 1);
        }
        out.push(AssetInfo {
            id: asset.id,
            code: m.code,
            class: m.class,
            criticality: m.criticality,
            site,
            node_id: nodes[m.line],
            failures_per_year: m.failures_per_year,
            hours_per_day: m.hours_per_day,
            loto: m.loto,
            is_component: m.parent.is_some(),
        });
    }
    ctx.count("équipements", out.len() as i64);
    Ok(out)
}

// ── People, teams and logins ─────────────────────────────────────────────────────────────────

async fn people(
    ctx: &mut Ctx<'_>,
    nodes: &BTreeMap<String, i64>,
    schedule_id: i64,
    admin: i64,
    caller: &AuthenticatedUser,
    start_day: NaiveDate,
) -> AppResult<(Vec<PersonInfo>, BTreeMap<&'static str, i64>)> {
    let mut teams = BTreeMap::new();
    for t in catalog::TEAMS {
        ctx.tick()?;
        let team = crate::repository::team_repository::create_team(
            ctx.db,
            crate::repository::team_repository::TeamCreateInput {
                code: t.code.to_string(),
                name: t.name.to_string(),
                team_type: t.team_type.to_string(),
                primary_node_id: nodes[t.site],
                description: None,
                status: Some("active".into()),
                icon_key: None,
                accent_color: None,
            },
        )
        .await?;
        teams.insert(t.code, team.id);
    }
    ctx.count("équipes", teams.len() as i64);

    let mut positions = BTreeMap::new();
    for (code, name, category) in catalog::POSITIONS {
        ctx.tick()?;
        let p = crate::personnel::queries::create_position(
            ctx.db,
            code.to_string(),
            name.to_string(),
            category.to_string(),
        )
        .await?;
        positions.insert(*code, p.id);
    }

    let state = AppState::new(ctx.db.clone());
    let mut out = Vec::new();
    for (i, person) in catalog::PEOPLE.iter().enumerate() {
        let skill_id = ref_value_id(ctx.db, "PERSONNEL.SKILLS", person.skill).await?;
        let hire = start_day - Duration::days(400 + (i as i64 * 211) % 3000);
        ctx.tick()?;
        let created = crate::personnel::queries::create_personnel(
            ctx.db,
            de(json!({
                "full_name": person.full_name,
                "employee_code": person.code,
                "employment_type": "employee",
                "employment_origin": "internal",
                "employment_status": "active",
                "position_id": positions[person.position],
                "primary_entity_id": nodes[person.site],
                "primary_team_id": teams[person.team],
                "home_schedule_reference_value_id": schedule_id,
                "hire_date": hire.format("%Y-%m-%d").to_string(),
                "skills": [{
                    "reference_value_id": skill_id,
                    "proficiency_level": if matches!(person.duty, Duty::Technician) { 3 } else { 4 },
                    "source_type": "manager_assessment",
                    "valid_to": null,
                    "is_primary": true,
                }],
            }))?,
            admin,
        )
        .await?;
        ctx.tick()?;
        crate::personnel::queries::create_rate_card(
            ctx.db,
            created.id,
            person.rate,
            person.rate * 1.5,
            None,
            "contract".into(),
            admin,
        )
        .await?;

        let mut user_id = None;
        if let Some(username) = person.username {
            let role_id = scalar_i64(ctx.db, "SELECT id FROM roles WHERE name = ?", vec![person.role.into()])
                .await?
                .ok_or_else(|| AppError::NotFound {
                    entity: "roles".into(),
                    id: person.role.into(),
                })?;
            ctx.tick()?;
            // A "hybrid" login without password: it cannot sign in until the administrator issues a
            // password reset from the users screen. No password is generated here.
            let id = crate::commands::admin_users::create_user_impl(
                &state,
                caller,
                crate::commands::admin_users::CreateUserInput {
                    username: username.to_string(),
                    identity_mode: "hybrid".into(),
                    role_id,
                    // Plant-wide: the people of the company work across both sites' records
                    // (shared stores, planning and approvals), as a small plant often does.
                    scope_node_id: None,
                    personnel_id: Some(created.id),
                    initial_password: None,
                    force_password_change: Some(true),
                },
            )
            .await?
            .id;
            ctx.tick()?;
            crate::commands::admin_users::update_user_impl(
                &state,
                caller,
                crate::commands::admin_users::UpdateUserInput {
                    user_id: id,
                    username: None,
                    display_name: Some(person.full_name.to_string()),
                    email: None,
                    phone: None,
                    personnel_id: None,
                    force_password_change: None,
                    is_active: None,
                },
            )
            .await?;
            ctx.count("comptes utilisateurs", 1);
            user_id = Some(id);
        }
        out.push(PersonInfo {
            personnel_id: created.id,
            user_id,
            site: person.site,
            duty: person.duty,
            team_id: teams[person.team],
            team: person.team,
            name: person.full_name,
            skill: person.skill,
        });
    }
    ctx.count("personnel", out.len() as i64);
    Ok((out, teams))
}

// ── Stock and suppliers ──────────────────────────────────────────────────────────────────────

#[allow(clippy::type_complexity)]
async fn stock(
    ctx: &mut Ctx<'_>,
    admin: i64,
    start_day: NaiveDate,
) -> AppResult<(BTreeMap<&'static str, (i64, i64)>, Vec<ArticleInfo>)> {
    let mut stores = BTreeMap::new();
    for site in catalog::SITES {
        ctx.tick()?;
        let wh = crate::inventory::queries::create_warehouse(
            ctx.db,
            de(json!({ "code": format!("MAG-{}", site.code), "name": format!("Magasin pièces de rechange {}", site.name) }))?,
        )
        .await?;
        ctx.tick()?;
        let loc = crate::inventory::queries::create_stock_location(
            ctx.db,
            de(json!({ "warehouse_id": wh.id, "code": "RAYON-A", "name": "Rayonnage A", "is_default": true }))?,
        )
        .await?;
        stores.insert(site.code, (wh.id, loc.id));
    }
    ctx.count("magasins", stores.len() as i64);

    let stocked = lookup_value_id(ctx.db, "inventory.stocking_type", "STOCKED").await?;
    let tax = lookup_value_id(ctx.db, "inventory.tax_category", "TVA_20").await?;
    let buy = lookup_value_id(ctx.db, "inventory.procurement_category", "BUY").await?;
    let mut articles = Vec::new();
    for a in catalog::ARTICLES {
        let unit_id = crate::reference::units::unit_id_by_code(ctx.db, a.unit).await?;
        ctx.tick()?;
        let created = crate::inventory::queries::create_article(
            ctx.db,
            de(json!({
                "article_code": a.code,
                "article_name": a.name,
                "unit_value_id": unit_id,
                "stocking_type_value_id": stocked,
                "tax_category_value_id": tax,
                "procurement_category_value_id": buy,
                "min_stock": a.min,
                "reorder_point": a.reorder,
                "safety_stock": a.min,
                "max_stock": a.max,
                "replenishment_policy_code": "MIN_MAX",
                "lead_time_days": 10,
                "is_critical_spare": a.critical,
                "requires_expiration": false,
                "requires_batch_tracking": false,
                "requires_serial_tracking": false,
                "is_active": true,
            }))?,
        )
        .await?;
        articles.push(ArticleInfo {
            id: created.id,
            code: a.code,
            unit_cost: a.unit_cost,
            reorder: a.reorder,
            max: a.max,
            used_by: a.used_by,
            critical: a.critical,
        });
    }
    ctx.count("articles", articles.len() as i64);

    // Opening stock counted when Maintafox is set up, valued at the purchase price.
    let _ = start_day;
    let posting = local_day(ctx.last).format("%Y-%m-%d").to_string();
    for (site, (_, loc)) in &stores {
        for a in &articles {
            let qty = ((a.reorder + a.max) / 2.0).round();
            ctx.tick()?;
            crate::inventory::queries::adjust_stock_full(
                ctx.db,
                de(json!({
                    "article_id": a.id,
                    "location_id": loc,
                    "delta_qty": qty,
                    "reason_code": "INVENTAIRE",
                    "notes": format!("Stock initial {site} à la mise en service de Maintafox"),
                    "source_ref": null,
                }))?,
                crate::inventory::queries::AdjustStockOptions {
                    unit_cost: Some(a.unit_cost),
                    posting_date: Some(posting.clone()),
                    actor_id: Some(admin),
                    ..Default::default()
                },
            )
            .await?;
            ctx.count("mouvements de stock (stock initial)", 1);
        }
    }
    Ok((stores, articles))
}

async fn suppliers(ctx: &mut Ctx<'_>) -> AppResult<Vec<i64>> {
    let mut out = Vec::new();
    for (code, name, status, terms, lead) in catalog::SUPPLIERS {
        ctx.tick()?;
        let s = crate::inventory::suppliers::upsert_supplier(
            ctx.db,
            None,
            None,
            de(json!({
                "code": code,
                "name": name,
                "status_code": status,
                "payment_terms_code": terms,
                "default_lead_time_days": lead,
                "is_active": true,
            }))?,
        )
        .await?;
        out.push(s.id);
    }
    ctx.count("fournisseurs", out.len() as i64);
    Ok(out)
}

// ── Budgets ──────────────────────────────────────────────────────────────────────────────────

async fn budgets(ctx: &mut Ctx<'_>, admin: i64, start_day: NaiveDate) -> AppResult<()> {
    let currency = crate::inventory::fx::tenant_currency(ctx.db).await?;
    let start_month = crate::finance::fiscal::fiscal_start_month(ctx.db).await?;
    let fiscal_year = |d: NaiveDate| {
        if i64::from(d.month()) >= start_month {
            i64::from(d.year())
        } else {
            i64::from(d.year()) - 1
        }
    };
    let today = local_day(ctx.limit);
    let years: Vec<i64> = (fiscal_year(start_day)..=fiscal_year(today)).collect();
    let centers = column_i64(ctx.db, "SELECT id FROM cost_centers ORDER BY code", vec![]).await?;
    for year in years {
        ctx.tick()?;
        let version = crate::finance::queries::create_budget_version(
            ctx.db,
            admin,
            de(json!({
                "fiscal_year": year,
                "scenario_type": "baseline",
                "currency_code": currency,
                "title": format!("Budget maintenance {year}"),
                "planning_basis": "historique et plans préventifs",
            }))?,
        )
        .await?;
        for cc in &centers {
            for (bucket, annual) in catalog::BUDGET_PER_SITE {
                for month in 1..=12_i64 {
                    ctx.tick()?;
                    crate::finance::queries::create_budget_line(
                        ctx.db,
                        de(json!({
                            "budget_version_id": version.id,
                            "cost_center_id": cc,
                            "period_month": month,
                            "budget_bucket": bucket,
                            "planned_amount": (annual / 12.0).round(),
                            "source_basis": "historique",
                        }))?,
                    )
                    .await?;
                }
            }
        }
        for next in ["submitted", "approved"] {
            ctx.tick()?;
            let rv = super::row_version(ctx.db, "budget_versions", version.id).await?;
            crate::finance::queries::transition_budget_version_lifecycle(
                ctx.db,
                admin,
                de(json!({ "version_id": version.id, "expected_row_version": rv, "next_status": next }))?,
            )
            .await?;
        }
        ctx.count("versions de budget approuvées", 1);
    }
    Ok(())
}

// ── Preventive plans ─────────────────────────────────────────────────────────────────────────

/// (interval months, estimated hours, title prefix, steps)
fn pm_programme(asset: &AssetInfo) -> Vec<(i64, f64, &'static str, &'static [&'static str])> {
    const MONTHLY: &[&str] = &[
        "Consignation et contrôle visuel général",
        "Graissage des points de lubrification",
        "Contrôle des niveaux et appoints",
        "Contrôle des sécurités et arrêts d'urgence",
        "Essai de fonctionnement et restitution",
    ];
    const QUARTERLY: &[&str] = &[
        "Mesure vibratoire des paliers",
        "Contrôle des serrages et des accouplements",
        "Remplacement des filtres",
        "Contrôle thermographique des armoires",
        "Essai de fonctionnement et restitution",
    ];
    match (asset.criticality, asset.class) {
        ("A", _) => vec![
            (1, 4.0, "Préventif mensuel", MONTHLY),
            (3, 8.0, "Révision trimestrielle", QUARTERLY),
        ],
        ("B", "CRANE" | "COMPRESSOR") => vec![(1, 3.0, "Préventif mensuel", MONTHLY)],
        ("B", _) => vec![(2, 3.0, "Préventif bimestriel", MONTHLY)],
        _ => vec![(3, 2.0, "Préventif trimestriel", QUARTERLY)],
    }
}

async fn pm_plans(ctx: &mut Ctx<'_>, company: &Company, start_day: NaiveDate) -> AppResult<Vec<PmPlanInfo>> {
    let mut out = Vec::new();
    for (i, asset) in company.assets.iter().filter(|a| !a.is_component).enumerate() {
        let team = match asset.site {
            "SET" => company.teams["SET-MECA"],
            _ => company.teams["BBA-MAINT"],
        };
        for (k, (months, hours, prefix, steps)) in pm_programme(asset).into_iter().enumerate() {
            let code = format!(
                "PM-{}-{}",
                asset.code,
                if months == 1 {
                    "M"
                } else if months == 2 {
                    "B"
                } else {
                    "T"
                }
            );
            // Staggered first due dates so the workload is spread over the month.
            let anchor = start_day + Duration::days(((i * 3 + k * 11) % 28) as i64 + 3);
            let mut anchor = anchor;
            while !is_working_day(anchor) {
                anchor += Duration::days(1);
            }
            ctx.tick()?;
            let plan = crate::pm::queries::create_pm_plan(
                ctx.db,
                de(json!({
                    "code": code,
                    "title": format!("{prefix} — {}", catalog::MACHINES.iter().find(|m| m.code == asset.code).map(|m| m.name).unwrap_or(asset.code)),
                    "description": "Plan préventif constructeur adapté au retour d'expérience.",
                    "asset_scope_type": "equipment",
                    "asset_scope_id": asset.id,
                    "strategy_type": "fixed",
                    "assigned_team_id": team,
                    "requires_shutdown": true,
                    "requires_permit": false,
                    "is_active": true,
                }))?,
            )
            .await?;
            ctx.tick()?;
            let version = crate::pm::queries::create_pm_plan_version(
                ctx.db,
                plan.id,
                de(json!({
                    "effective_from": rfc(local(anchor, 8, 0)),
                    "trigger_definition_json": json!({
                        "interval_unit": "month",
                        "interval_value": months,
                        "tolerance": { "unit": "days", "early": 3, "late": 3 },
                    }).to_string(),
                    "task_package_json": json!({ "steps": steps }).to_string(),
                    "estimated_duration_hours": hours,
                    "planned_crew_size": if hours > 3.0 { 2 } else { 1 },
                    "change_reason": "Création du plan",
                }))?,
            )
            .await?;
            for next in ["proposed", "approved"] {
                ctx.tick()?;
                let rv = super::row_version(ctx.db, "pm_plans", plan.id).await?;
                crate::pm::queries::transition_pm_plan_lifecycle(
                    ctx.db,
                    de(json!({ "plan_id": plan.id, "expected_row_version": rv, "next_status": next }))?,
                )
                .await?;
            }
            ctx.tick()?;
            let vrv = super::row_version(ctx.db, "pm_plan_versions", version.id).await?;
            crate::pm::queries::publish_pm_plan_version(
                ctx.db,
                de(json!({ "version_id": version.id, "expected_row_version": vrv }))?,
            )
            .await?;
            out.push(PmPlanInfo {
                plan_id: plan.id,
                asset_code: asset.code,
                hours,
            });
        }
    }
    ctx.count("plans préventifs", out.len() as i64);
    Ok(out)
}

// ── Inspection routes ────────────────────────────────────────────────────────────────────────

async fn inspection_routes(ctx: &mut Ctx<'_>, company: &Company) -> AppResult<Vec<InspectionRoute>> {
    let bar = Some(crate::reference::units::unit_id_by_code(ctx.db, "BAR").await?);
    let degc = Some(crate::reference::units::unit_id_by_code(ctx.db, "DEGC").await?);
    let mut out = Vec::new();
    for site in catalog::SITES {
        let utilities: Vec<&AssetInfo> = company
            .assets
            .iter()
            .filter(|a| {
                a.site == site.code
                    && matches!(a.class, "COMPRESSOR" | "TRANSFORMER" | "PUMP" | "HEAT_EXCHANGER")
                    && !a.is_component
            })
            .collect();
        let mut drafts = Vec::new();
        let mut kinds = Vec::new();
        let mut seq = 1;
        for a in &utilities {
            let (kind, code, desc, unit, nmin, nmax, wmin, wmax, check_type) = match a.class {
                "COMPRESSOR" => (
                    CheckKind::Pressure,
                    "PRESSION",
                    "Pression réseau en sortie",
                    bar,
                    6.5,
                    7.5,
                    6.0,
                    8.0,
                    "numeric",
                ),
                "TRANSFORMER" | "HEAT_EXCHANGER" => (
                    CheckKind::Temperature,
                    "TEMP",
                    "Température de fonctionnement",
                    degc,
                    40.0,
                    75.0,
                    30.0,
                    85.0,
                    "numeric",
                ),
                _ => (
                    CheckKind::Boolean,
                    "ETANCHE",
                    "Absence de fuite et de bruit anormal",
                    None,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    "boolean",
                ),
            };
            let numeric = check_type == "numeric";
            drafts.push(json!({
                "sequence_order": seq,
                "asset_id": a.id,
                "component_id": null,
                "checkpoint_code": format!("{}-{}", a.code, code),
                "description": format!("{desc} — {}", a.code),
                "check_type": check_type,
                "measurement_unit_id": unit,
                "normal_min": if numeric { Some(nmin) } else { None },
                "normal_max": if numeric { Some(nmax) } else { None },
                "warning_min": if numeric { Some(wmin) } else { None },
                "warning_max": if numeric { Some(wmax) } else { None },
                "requires_photo": false,
                "requires_comment_on_exception": true,
            }));
            kinds.push((kind, Some(a.id)));
            seq += 1;
        }
        ctx.tick()?;
        let (template, _version, checkpoints) = crate::inspection::queries::create_inspection_template(
            ctx.db,
            de(json!({
                "code": format!("RONDE-UTL-{}", site.code),
                "name": format!("Ronde hebdomadaire utilités — {}", site.name),
                "org_scope_id": company.nodes[site.code],
                "route_scope": "Utilités",
                "estimated_duration_minutes": 45,
                "is_active": true,
                "checkpoints": drafts,
            }))?,
        )
        .await?;
        let mut cps: Vec<(i64, i64)> = checkpoints.iter().map(|c| (c.sequence_order, c.id)).collect();
        cps.sort();
        out.push(InspectionRoute {
            template_id: template.id,
            site: site.code,
            checkpoints: cps
                .into_iter()
                .zip(kinds)
                .map(|((_, id), (kind, asset))| (id, kind, asset))
                .collect(),
        });
    }
    ctx.count("gammes d'inspection", out.len() as i64);
    Ok(out)
}
