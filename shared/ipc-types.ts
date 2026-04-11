// IPC contract types shared between src/ (frontend) and the Tauri command layer.
// Types defined here must be kept in sync with Rust structs in src-tauri/src/.

export interface HealthCheckResponse {
  status: "ok" | "degraded";
  version: string;
  db_connected: boolean;
  locale: string;
}

// ─── Startup Events ────────────────────────────────────────────────────────

export type StartupStage =
  | "db_ready"
  | "migrations_complete"
  | "entitlement_cache_loaded"
  | "ready"
  | "failed";

export interface StartupEvent {
  stage: StartupStage;
  /** Present only for stage = "migrations_complete" */
  applied?: number;
  /** Present only for stage = "failed" */
  reason?: string;
}

// ─── App Info ──────────────────────────────────────────────────────────────

export interface AppInfoResponse {
  version: string;
  build_mode: "debug" | "release";
  os: string;
  arch: string;
  app_name: string;
  default_locale: string;
}

// ─── Task Status ───────────────────────────────────────────────────────────

export type TaskStatusKind = "running" | "cancelled" | "finished";

export interface TaskStatusEntry {
  id: string;
  status: TaskStatusKind;
}

// ─── Shutdown ──────────────────────────────────────────────────────────────

// shutdown_app — no response type; the Rust command calls app.exit(0).

// ─── Auth & Session ────────────────────────────────────────────────────────

export interface SessionInfo {
  is_authenticated: boolean;
  is_locked: boolean;
  user_id: number | null;
  username: string | null;
  display_name: string | null;
  is_admin: boolean | null;
  force_password_change: boolean | null;
  expires_at: string | null;
  last_activity_at: string | null;
  password_expires_in_days: number | null;
  pin_configured: boolean | null;
}

export interface LoginRequest {
  username: string;
  password: string;
}

export interface LoginResponse {
  session_info: SessionInfo;
}

// ─── RBAC ──────────────────────────────────────────────────────────────────

export interface PermissionRecord {
  name: string;
  description: string;
  category: string;
  is_dangerous: boolean;
  requires_step_up: boolean;
}

export interface StepUpRequest {
  password: string;
}

export interface StepUpResponse {
  success: boolean;
  expires_at: string;
}

// ─── Device Trust ──────────────────────────────────────────────────────────

export interface DeviceTrustStatus {
  device_fingerprint: string;
  is_trusted: boolean;
  is_revoked: boolean;
  offline_allowed: boolean;
  offline_hours_remaining: number | null;
  device_label: string | null;
  trusted_at: string | null;
  status?: string;
}

// ─── Locale ────────────────────────────────────────────────────────────────

export interface LocalePreference {
  active_locale: string;
  user_locale: string | null;
  tenant_locale: string | null;
  os_locale: string | null;
  supported_locales: string[];
}

// ─── Settings ──────────────────────────────────────────────────────────────

export interface AppSetting {
  id: number;
  setting_key: string;
  setting_scope: string;
  setting_value_json: string;
  category: string;
  setting_risk: "low" | "high";
  validation_status: "valid" | "draft" | "error" | "untested";
  secret_ref_id: number | null;
  last_modified_by_id: number | null;
  last_modified_at: string;
}

export interface PolicySnapshot {
  id: number;
  policy_domain: string;
  version_no: number;
  snapshot_json: string;
  is_active: boolean;
  activated_at: string | null;
  activated_by_id: number | null;
}

export interface PolicyTestResult {
  rule_name: string;
  severity: "pass" | "warn" | "fail";
  message: string;
}

export interface SavePolicyDraftPayload {
  domain: string;
  snapshot_json: string;
}

export interface ActivatePolicyPayload {
  domain: string;
  snapshot_id: number;
}

export interface SessionPolicy {
  idle_timeout_minutes: number;
  absolute_session_minutes: number;
  offline_grace_hours: number;
  step_up_window_minutes: number;
  max_failed_attempts: number;
  lockout_minutes: number;
}

export interface SettingsChangeEvent {
  id: number;
  setting_key_or_domain: string;
  change_summary: string;
  old_value_hash: string | null;
  new_value_hash: string | null;
  changed_by_id: number | null;
  changed_at: string;
  required_step_up: boolean;
  apply_result: string;
}

// ─── Updater ───────────────────────────────────────────────────────────────

export interface UpdateCheckResult {
  available: boolean;
  version: string | null;
  notes: string | null;
  pub_date: string | null;
}

// ─── Diagnostics ───────────────────────────────────────────────────────────

export interface IntegrityIssue {
  code: string;
  description: string;
  is_auto_repairable: boolean;
  subject: string;
}

export interface IntegrityReport {
  is_healthy: boolean;
  is_recoverable: boolean;
  issues: IntegrityIssue[];
  seed_schema_version: number | null;
  domain_count: number;
  value_count: number;
}

export interface DiagnosticsAppInfo {
  app_version: string;
  os_name: string;
  os_version: string;
  arch: string;
  db_schema_version: number;
  active_locale: string;
  sync_status: string;
  uptime_seconds: number;
}

export interface SupportBundle {
  generated_at: string;
  app_info: DiagnosticsAppInfo;
  log_lines: string[];
  collection_warnings: string[];
}

// ─── Backup & Restore ──────────────────────────────────────────────────────

export interface BackupRunRecord {
  id: number;
  trigger: string;
  status: string;
  output_path: string;
  file_size_bytes: number | null;
  sha256_checksum: string | null;
  encryption_mode: string;
  db_schema_version: number | null;
  started_at: string;
  completed_at: string | null;
  error_message: string | null;
  initiated_by_id: number | null;
}

export interface BackupRunResult {
  run_id: number;
  output_path: string;
  file_size_bytes: number;
  sha256_checksum: string;
  status: string;
}

export interface RestoreTestResult {
  backup_path: string;
  integrity_ok: boolean;
  stored_checksum: string | null;
  computed_checksum: string;
  checksum_match: boolean;
  integrity_check_output: string;
  warnings: string[];
}

// ─── Organization — Structure Model & Config ───────────────────────────────

export interface OrgStructureModel {
  id: number;
  sync_id: string;
  version_number: number;
  status: string;
  description: string | null;
  activated_at: string | null;
  activated_by_id: number | null;
  superseded_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface OrgNodeType {
  id: number;
  sync_id: string;
  structure_model_id: number;
  code: string;
  label: string;
  icon_key: string | null;
  color: string | null;
  depth_hint: number | null;
  can_host_assets: boolean;
  can_own_work: boolean;
  can_carry_cost_center: boolean;
  can_aggregate_kpis: boolean;
  can_receive_permits: boolean;
  is_root_type: boolean;
  is_active: boolean;
  created_at: string;
  updated_at: string;
}

export interface OrgRelationshipRule {
  id: number;
  structure_model_id: number;
  parent_type_id: number;
  child_type_id: number;
  min_children: number | null;
  max_children: number | null;
  created_at: string;
  parent_type_label?: string | null;
  child_type_label?: string | null;
}

export interface CreateStructureModelPayload {
  description?: string | null;
}

export interface CreateOrgNodeTypePayload {
  structure_model_id: number;
  code: string;
  label: string;
  icon_key?: string;
  color?: string;
  depth_hint?: number;
  can_host_assets: boolean;
  can_own_work: boolean;
  can_carry_cost_center: boolean;
  can_aggregate_kpis: boolean;
  can_receive_permits: boolean;
  is_root_type: boolean;
}

export interface UpdateOrgNodeTypePayload {
  id: number;
  label?: string;
  icon_key?: string | null;
  color?: string | null;
  depth_hint?: number | null;
  can_host_assets?: boolean;
  can_own_work?: boolean;
  can_carry_cost_center?: boolean;
  can_aggregate_kpis?: boolean;
  can_receive_permits?: boolean;
}

export interface CreateRelationshipRulePayload {
  structure_model_id: number;
  parent_type_id: number;
  child_type_id: number;
  min_children?: number | null;
  max_children?: number | null;
}

// ─── Organization — Equipment Assignment ───────────────────────────────────

export interface OrgNodeEquipmentRow {
  id: number;
  asset_id_code: string;
  name: string;
  lifecycle_status: string;
  installed_at_node_id: number | null;
  current_node_name: string | null;
}

export interface AssignEquipmentPayload {
  equipment_id: number;
  node_id: number;
}

// ─── Organization — Nodes ──────────────────────────────────────────────────

export interface OrgNode {
  id: number;
  sync_id: string;
  code: string;
  name: string;
  node_type_id: number;
  parent_id: number | null;
  ancestor_path: string;
  depth: number;
  description: string | null;
  cost_center_code: string | null;
  external_reference: string | null;
  status: string;
  effective_from: string | null;
  effective_to: string | null;
  erp_reference: string | null;
  notes: string | null;
  created_at: string;
  updated_at: string;
  deleted_at: string | null;
  row_version: number;
  origin_machine_id: string | null;
  last_synced_checkpoint: string | null;
}

export interface OrgTreeRow {
  node: OrgNode;
  node_type_code: string;
  node_type_label: string;
  can_host_assets: boolean;
  can_own_work: boolean;
  can_carry_cost_center: boolean;
  can_aggregate_kpis: boolean;
  can_receive_permits: boolean;
  child_count: number;
}

export interface OrgNodeResponsibility {
  id: number;
  node_id: number;
  responsibility_type: string;
  person_id: number | null;
  team_id: number | null;
  valid_from: string | null;
  valid_to: string | null;
  created_at: string;
  updated_at: string;
}

export interface OrgEntityBinding {
  id: number;
  node_id: number;
  binding_type: string;
  external_system: string;
  external_id: string;
  is_primary: boolean;
  valid_from: string | null;
  valid_to: string | null;
  created_at: string;
}

export interface CreateOrgNodePayload {
  code: string;
  name: string;
  node_type_id: number;
  parent_id?: number | null;
  description?: string | null;
  cost_center_code?: string | null;
  external_reference?: string | null;
  effective_from?: string | null;
  erp_reference?: string | null;
  notes?: string | null;
}

export interface UpdateOrgNodeMetadataPayload {
  node_id: number;
  name?: string;
  description?: string | null;
  cost_center_code?: string | null;
  external_reference?: string | null;
  erp_reference?: string | null;
  notes?: string | null;
  status?: string;
  expected_row_version: number;
}

export interface MoveOrgNodePayload {
  node_id: number;
  new_parent_id?: number | null;
  expected_row_version: number;
  effective_from?: string | null;
}

export interface AssignResponsibilityPayload {
  node_id: number;
  responsibility_type: string;
  person_id?: number | null;
  team_id?: number | null;
  valid_from?: string | null;
  valid_to?: string | null;
}

export interface UpsertOrgEntityBindingPayload {
  node_id: number;
  binding_type: string;
  external_system: string;
  external_id: string;
  is_primary: boolean;
  valid_from?: string | null;
  valid_to?: string | null;
}

// ─── Organization — Designer ───────────────────────────────────────────────

export interface OrgDesignerNodeRow {
  node_id: number;
  parent_id: number | null;
  ancestor_path: string;
  depth: number;
  code: string;
  name: string;
  status: string;
  row_version: number;
  node_type_id: number;
  node_type_code: string;
  node_type_label: string;
  can_host_assets: boolean;
  can_own_work: boolean;
  can_carry_cost_center: boolean;
  can_aggregate_kpis: boolean;
  can_receive_permits: boolean;
  child_count: number;
  active_responsibility_count: number;
  active_binding_count: number;
}

export interface OrgDesignerSnapshot {
  active_model_id: number | null;
  active_model_version: number | null;
  draft_model_id: number | null;
  nodes: OrgDesignerNodeRow[];
}

export interface OrgImpactDependencySummary {
  domain: string;
  status: string;
  count: number | null;
  note: string | null;
}

export interface OrgImpactPreview {
  action: "MoveNode" | "DeactivateNode" | "ReassignResponsibility";
  subject_node_id: number;
  affected_node_count: number;
  descendant_count: number;
  active_responsibility_count: number;
  active_binding_count: number;
  blockers: string[];
  warnings: string[];
  dependencies: OrgImpactDependencySummary[];
}

export interface PreviewOrgChangePayload {
  action: string;
  node_id: number;
  new_parent_id?: number | null;
  responsibility_type?: string | null;
  replacement_person_id?: number | null;
  replacement_team_id?: number | null;
}

// ─── Organization — Governance ─────────────────────────────────────────────

export interface OrgValidationIssue {
  code: string;
  severity: string;
  message: string;
  related_id: number | null;
}

export interface OrgPublishValidationResult {
  model_id: number;
  can_publish: boolean;
  issue_count: number;
  blocking_count: number;
  issues: OrgValidationIssue[];
  remap_count: number;
}

export interface OrgChangeEvent {
  id: number;
  entity_kind: string;
  entity_id: number | null;
  change_type: string;
  before_json: string | null;
  after_json: string | null;
  preview_summary_json: string | null;
  changed_by_id: number | null;
  changed_at: string;
  requires_step_up: boolean;
  apply_result: string;
}

// ─── Reference Data — Core ─────────────────────────────────────────────────

export interface ReferenceDomain {
  id: number;
  code: string;
  name: string;
  structure_type: string;
  governance_level: string;
  is_extendable: boolean;
  validation_rules_json: string | null;
  created_at: string;
  updated_at: string;
}

export interface CreateReferenceDomainPayload {
  code: string;
  name: string;
  structure_type: string;
  governance_level: string;
  is_extendable?: boolean;
  validation_rules_json?: string | null;
}

export interface UpdateReferenceDomainPayload {
  name?: string;
  structure_type?: string;
  governance_level?: string;
  is_extendable?: boolean;
  validation_rules_json?: string | null;
}

export interface ReferenceSet {
  id: number;
  domain_id: number;
  version_no: number;
  status: string;
  effective_from: string | null;
  created_by_id: number | null;
  created_at: string;
  published_at: string | null;
}

export interface ReferenceValue {
  id: number;
  set_id: number;
  parent_id: number | null;
  code: string;
  label: string;
  description: string | null;
  sort_order: number | null;
  color_hex: string | null;
  icon_name: string | null;
  semantic_tag: string | null;
  external_code: string | null;
  is_active: boolean;
  metadata_json: string | null;
}

export interface CreateReferenceValuePayload {
  set_id: number;
  parent_id?: number | null;
  code: string;
  label: string;
  description?: string | null;
  sort_order?: number | null;
  color_hex?: string | null;
  icon_name?: string | null;
  semantic_tag?: string | null;
  external_code?: string | null;
  metadata_json?: string | null;
}

export interface UpdateReferenceValuePayload {
  label?: string;
  description?: string | null;
  sort_order?: number | null;
  color_hex?: string | null;
  icon_name?: string | null;
  semantic_tag?: string | null;
  external_code?: string | null;
  metadata_json?: string | null;
}

export interface ReferenceValueMigration {
  id: number;
  domain_id: number;
  from_value_id: number;
  to_value_id: number;
  reason_code: string | null;
  migrated_by_id: number | null;
  migrated_at: string;
}

export interface ReferenceUsageMigrationResult {
  migration: ReferenceValueMigration;
  source_value: ReferenceValue;
  target_value: ReferenceValue;
  remapped_references: number;
  source_deactivated: boolean;
}

// ─── Reference Data — Aliases ──────────────────────────────────────────────

export interface ReferenceAlias {
  id: number;
  reference_value_id: number;
  alias_label: string;
  locale: string;
  alias_type: string;
  is_preferred: boolean;
  created_at: string;
}

export interface CreateReferenceAliasPayload {
  reference_value_id: number;
  alias_label: string;
  locale: string;
  alias_type: string;
  is_preferred?: boolean;
}

export interface UpdateReferenceAliasPayload {
  alias_label?: string;
  locale?: string;
  alias_type?: string;
  is_preferred?: boolean;
}

// ─── Reference Data — Import / Export ──────────────────────────────────────

export interface ImportRowMessage {
  category: string;
  severity: string;
  message: string;
}

export interface RefImportBatchSummary {
  id: number;
  domain_id: number;
  source_filename: string;
  source_sha256: string;
  status: string;
  total_rows: number;
  valid_rows: number;
  warning_rows: number;
  error_rows: number;
  initiated_by_id: number | null;
  created_at: string;
  updated_at: string;
}

export interface RefImportRow {
  id: number;
  batch_id: number;
  row_no: number;
  raw_json: string;
  normalized_code: string | null;
  validation_status: string;
  messages: ImportRowMessage[];
  proposed_action: string | null;
}

export interface RefImportPreview {
  batch: RefImportBatchSummary;
  rows: RefImportRow[];
}

export interface ImportRowInput {
  code?: string | null;
  label?: string | null;
  description?: string | null;
  parent_code?: string | null;
  sort_order?: number | null;
  color_hex?: string | null;
  icon_name?: string | null;
  semantic_tag?: string | null;
  external_code?: string | null;
  metadata_json?: string | null;
}

export interface RefImportApplyPolicy {
  include_warnings: boolean;
  target_set_id: number;
}

export interface RefImportApplyResult {
  batch: RefImportBatchSummary;
  created: number;
  updated: number;
  skipped: number;
  errored: number;
}

export interface RefExportRow {
  value: ReferenceValue;
  aliases: ReferenceAlias[];
}

export interface RefExportResult {
  domain: ReferenceDomain;
  set: ReferenceSet;
  rows: RefExportRow[];
}

// ─── Reference Data — Search ───────────────────────────────────────────────

export interface ReferenceSearchHit {
  value_id: number;
  code: string;
  label: string;
  matched_text: string;
  match_source: string;
  alias_type: string | null;
  rank: number;
}

// ─── Reference Data — Publish Governance ───────────────────────────────────

export interface ReferencePublishIssue {
  check: string;
  message: string;
  severity: string;
}

export interface ReferencePublishReadiness {
  set_id: number;
  domain_id: number;
  is_ready: boolean;
  is_protected: boolean;
  issues: ReferencePublishIssue[];
  impact_preview_required: boolean;
  impact_preview_available: boolean;
}

export interface ModuleImpact {
  module: string;
  status: string;
  affected_count: number;
  details: string | null;
}

export interface ReferenceImpactSummary {
  set_id: number;
  domain_id: number;
  domain_code: string;
  total_affected: number;
  dimensions: ModuleImpact[];
  computed_at: string;
}

export interface ReferencePublishResult {
  set: ReferenceSet;
  superseded_set_id: number | null;
  readiness: ReferencePublishReadiness;
}

// ─── Asset — Identity & Hierarchy ──────────────────────────────────────────

export interface Asset {
  id: number;
  sync_id: string;
  asset_code: string;
  asset_name: string;
  class_id: number | null;
  class_code: string | null;
  class_name: string | null;
  family_code: string | null;
  family_name: string | null;
  criticality_value_id: number | null;
  criticality_code: string | null;
  status_code: string;
  manufacturer: string | null;
  model: string | null;
  serial_number: string | null;
  maintainable_boundary: boolean;
  org_node_id: number | null;
  org_node_name: string | null;
  commissioned_at: string | null;
  decommissioned_at: string | null;
  created_at: string;
  updated_at: string;
  deleted_at: string | null;
  row_version: number;
}

export interface AssetHierarchyRow {
  relation_id: number;
  parent_asset_id: number;
  child_asset_id: number;
  relation_type: string;
  effective_from: string | null;
  effective_to: string | null;
}

export interface CreateAssetPayload {
  asset_code: string;
  asset_name: string;
  class_code: string;
  family_code?: string | null;
  criticality_code: string;
  status_code: string;
  manufacturer?: string | null;
  model?: string | null;
  serial_number?: string | null;
  maintainable_boundary: boolean;
  org_node_id: number;
  commissioned_at?: string | null;
}

export interface UpdateAssetIdentityPayload {
  asset_name?: string;
  class_code?: string;
  family_code?: string | null;
  criticality_code?: string;
  status_code?: string;
  manufacturer?: string | null;
  model?: string | null;
  serial_number?: string | null;
  maintainable_boundary?: boolean;
  commissioned_at?: string | null;
  decommissioned_at?: string | null;
}

export interface LinkAssetPayload {
  parent_asset_id: number;
  child_asset_id: number;
  relation_type: string;
  effective_from?: string | null;
}

// ─── Asset — Lifecycle ─────────────────────────────────────────────────────

export interface AssetLifecycleEvent {
  id: number;
  sync_id: string;
  asset_id: number;
  event_type: string;
  event_at: string;
  from_org_node_id: number | null;
  to_org_node_id: number | null;
  from_status_code: string | null;
  to_status_code: string | null;
  from_class_code: string | null;
  to_class_code: string | null;
  related_asset_id: number | null;
  reason_code: string | null;
  notes: string | null;
  approved_by_id: number | null;
  created_by_id: number | null;
  created_at: string;
}

export interface RecordLifecycleEventPayload {
  asset_id: number;
  event_type: string;
  event_at?: string | null;
  from_org_node_id?: number | null;
  to_org_node_id?: number | null;
  from_status_code?: string | null;
  to_status_code?: string | null;
  from_class_code?: string | null;
  to_class_code?: string | null;
  related_asset_id?: number | null;
  reason_code?: string | null;
  notes?: string | null;
  approved_by_id?: number | null;
}

export interface AssetMeter {
  id: number;
  sync_id: string;
  asset_id: number;
  name: string;
  meter_code: string | null;
  meter_type: string;
  unit: string | null;
  current_reading: number;
  last_read_at: string | null;
  expected_rate_per_day: number | null;
  rollover_value: number | null;
  is_primary: boolean;
  is_active: boolean;
  created_at: string;
  updated_at: string;
}

export interface CreateAssetMeterPayload {
  asset_id: number;
  name: string;
  meter_code?: string | null;
  meter_type: string;
  unit?: string | null;
  initial_reading?: number | null;
  expected_rate_per_day?: number | null;
  rollover_value?: number | null;
  is_primary?: boolean;
}

export interface MeterReading {
  id: number;
  meter_id: number;
  reading_value: number;
  reading_at: string;
  source_type: string;
  source_reference: string | null;
  quality_flag: string;
  created_by_id: number | null;
  created_at: string;
}

export interface RecordMeterReadingPayload {
  meter_id: number;
  reading_value: number;
  reading_at?: string | null;
  source_type: string;
  source_reference?: string | null;
  quality_flag?: string;
}

export interface AssetDocumentLink {
  id: number;
  asset_id: number;
  document_ref: string;
  link_purpose: string;
  is_primary: boolean;
  valid_from: string | null;
  valid_to: string | null;
  created_by_id: number | null;
  created_at: string;
}

export interface UpsertDocumentLinkPayload {
  asset_id: number;
  document_ref: string;
  link_purpose: string;
  is_primary?: boolean;
  valid_from?: string | null;
}

export interface DomainBindingEntry {
  status: "available" | "not_implemented";
  count: number | null;
}

export interface AssetBindingSummary {
  asset_id: number;
  linked_di_count: DomainBindingEntry;
  linked_wo_count: DomainBindingEntry;
  linked_pm_plan_count: DomainBindingEntry;
  linked_failure_event_count: DomainBindingEntry;
  linked_document_count: DomainBindingEntry;
  linked_iot_signal_count: DomainBindingEntry;
  linked_erp_mapping_count: DomainBindingEntry;
}

// ─── Asset — Search ────────────────────────────────────────────────────────

export interface AssetSearchFilters {
  query?: string | null;
  classCodes?: string[] | null;
  familyCodes?: string[] | null;
  statusCodes?: string[] | null;
  orgNodeIds?: number[] | null;
  includeDecommissioned?: boolean;
  limit?: number | null;
}

export interface AssetSearchResult {
  id: number;
  sync_id: string;
  asset_code: string;
  asset_name: string;
  class_code: string | null;
  class_name: string | null;
  family_code: string | null;
  family_name: string | null;
  criticality_code: string | null;
  status_code: string;
  org_node_id: number | null;
  org_node_name: string | null;
  parent_asset_id: number | null;
  parent_asset_code: string | null;
  parent_asset_name: string | null;
  primary_meter_name: string | null;
  primary_meter_reading: number | null;
  primary_meter_unit: string | null;
  primary_meter_last_read_at: string | null;
  external_id_count: number;
  row_version: number;
}

export interface AssetSuggestion {
  id: number;
  asset_code: string;
  asset_name: string;
  status_code: string;
}

// ─── Asset — Import ────────────────────────────────────────────────────────

export interface ValidationMessage {
  category: string;
  severity: string;
  message: string;
}

export interface ImportBatchSummary {
  id: number;
  source_filename: string;
  source_sha256: string;
  initiated_by_id: number | null;
  status: string;
  total_rows: number;
  valid_rows: number;
  warning_rows: number;
  error_rows: number;
  created_at: string;
  updated_at: string;
}

export interface ImportPreviewRow {
  id: number;
  row_no: number;
  normalized_asset_code: string | null;
  normalized_external_key: string | null;
  validation_status: string;
  validation_messages: ValidationMessage[];
  proposed_action: string | null;
  raw_json: string;
}

export interface ImportPreview {
  batch: ImportBatchSummary;
  rows: ImportPreviewRow[];
}

export interface ApplyPolicy {
  include_warnings: boolean;
  external_system_code?: string | null;
}

export interface ApplyResult {
  batch: ImportBatchSummary;
  created: number;
  updated: number;
  skipped: number;
  errored: number;
}

// ─── Intervention Requests (DI) ────────────────────────────────────────────

export type DiStatus =
  | "submitted"
  | "pending_review"
  | "returned_for_clarification"
  | "rejected"
  | "screened"
  | "awaiting_approval"
  | "approved_for_planning"
  | "deferred"
  | "converted_to_work_order"
  | "closed_as_non_executable"
  | "archived";

export type DiOriginType =
  | "operator"
  | "technician"
  | "inspection"
  | "pm"
  | "iot"
  | "quality"
  | "hse"
  | "production"
  | "external";

export type DiUrgency = "low" | "medium" | "high" | "critical";

export type DiImpactLevel = "unknown" | "none" | "minor" | "major" | "critical";

export interface InterventionRequest {
  id: number;
  code: string;
  asset_id: number;
  sub_asset_ref: string | null;
  org_node_id: number;
  status: DiStatus;
  title: string;
  description: string;
  origin_type: DiOriginType;
  symptom_code_id: number | null;
  impact_level: DiImpactLevel;
  production_impact: boolean;
  safety_flag: boolean;
  environmental_flag: boolean;
  quality_flag: boolean;
  reported_urgency: DiUrgency;
  validated_urgency: DiUrgency | null;
  observed_at: string | null;
  submitted_at: string;
  review_team_id: number | null;
  reviewer_id: number | null;
  screened_at: string | null;
  approved_at: string | null;
  deferred_until: string | null;
  declined_at: string | null;
  closed_at: string | null;
  archived_at: string | null;
  converted_to_wo_id: number | null;
  converted_at: string | null;
  reviewer_note: string | null;
  classification_code_id: number | null;
  is_recurrence_flag: boolean;
  recurrence_di_id: number | null;
  is_modified: boolean;
  row_version: number;
  submitter_id: number;
  created_at: string;
  updated_at: string;
}

export interface DiListFilter {
  status?: string[] | null;
  asset_id?: number | null;
  org_node_id?: number | null;
  submitter_id?: number | null;
  reviewer_id?: number | null;
  origin_type?: string | null;
  urgency?: string | null;
  search?: string | null;
  limit: number;
  offset: number;
}

export interface DiListPage {
  items: InterventionRequest[];
  total: number;
}

export interface DiTransitionRow {
  id: number;
  from_status: string;
  to_status: string;
  action: string;
  actor_id: number | null;
  reason_code: string | null;
  notes: string | null;
  acted_at: string;
}

export interface DiSummaryRow {
  id: number;
  code: string;
  title: string;
  status: string;
  submitted_at: string;
}

export interface DiCreateInput {
  asset_id: number;
  org_node_id: number;
  title: string;
  description: string;
  origin_type: string;
  symptom_code_id?: number | null;
  impact_level: string;
  production_impact: boolean;
  safety_flag: boolean;
  environmental_flag: boolean;
  quality_flag: boolean;
  reported_urgency: string;
  observed_at?: string | null;
  submitter_id: number;
}

export interface DiDraftUpdateInput {
  id: number;
  expected_row_version: number;
  title?: string | null;
  description?: string | null;
  symptom_code_id?: number | null;
  impact_level?: string | null;
  production_impact?: boolean | null;
  safety_flag?: boolean | null;
  environmental_flag?: boolean | null;
  quality_flag?: boolean | null;
  reported_urgency?: string | null;
  observed_at?: string | null;
}

export interface DiGetResponse {
  di: InterventionRequest;
  transitions: DiTransitionRow[];
  similar: DiSummaryRow[];
}

// ── DI Review / Triage (File 02) ───────────────────────────────────────────

export interface DiScreenInput {
  di_id: number;
  actor_id: number;
  expected_row_version: number;
  validated_urgency: string;
  review_team_id?: number | null;
  classification_code_id?: number | null;
  reviewer_note?: string | null;
}

export interface DiReturnInput {
  di_id: number;
  actor_id: number;
  expected_row_version: number;
  reviewer_note: string;
}

export interface DiRejectInput {
  di_id: number;
  actor_id: number;
  expected_row_version: number;
  reason_code: string;
  notes?: string | null;
}

export interface DiApproveInput {
  di_id: number;
  actor_id: number;
  expected_row_version: number;
  notes?: string | null;
}

export interface DiDeferInput {
  di_id: number;
  actor_id: number;
  expected_row_version: number;
  deferred_until: string;
  reason_code: string;
  notes?: string | null;
}

export interface DiReactivateInput {
  di_id: number;
  actor_id: number;
  expected_row_version: number;
  notes?: string | null;
}

export interface DiReviewEvent {
  id: number;
  di_id: number;
  event_type: string;
  actor_id: number | null;
  acted_at: string;
  from_status: string;
  to_status: string;
  reason_code: string | null;
  notes: string | null;
  sla_target_hours: number | null;
  sla_deadline: string | null;
  step_up_used: boolean;
}

// ─── Lookup Service ────────────────────────────────────────────────────────

export interface LookupValueOption {
  id: number;
  code: string;
  label: string;
  fr_label: string | null;
  en_label: string | null;
  color: string | null;
  is_active: number;
}

export interface LookupValueRecord extends LookupValueOption {
  sync_id: string;
  domain_id: number;
  description: string | null;
  sort_order: number;
  is_system: number;
  parent_value_id: number | null;
}

export interface LookupDomainSummary {
  id: number;
  code: string;
  name: string;
  value_count: number;
}

export interface LookupDomainFilter {
  search?: string | null;
}

export interface PageRequest {
  page: number;
  per_page: number;
}

export interface Page<T> {
  items: T[];
  total: number;
  page: number;
  per_page: number;
}

// ─── DI Attachments (File 03) ──────────────────────────────────────────────

export type DiAttachmentType = "photo" | "sensor_snapshot" | "pdf" | "other";

export interface DiAttachment {
  id: number;
  di_id: number;
  file_name: string;
  relative_path: string;
  mime_type: string;
  size_bytes: number;
  attachment_type: string;
  uploaded_by_id: number | null;
  uploaded_at: string;
  notes: string | null;
}

export interface DiAttachmentUploadInput {
  diId: number;
  fileName: string;
  fileBytes: number[];
  mimeType: string;
  attachmentType: DiAttachmentType;
  notes?: string;
}

// ─── DI SLA (File 03) ─────────────────────────────────────────────────────

export interface DiSlaRule {
  id: number;
  name: string;
  urgency_level: string;
  origin_type: string | null;
  asset_criticality_class: string | null;
  target_response_hours: number;
  target_resolution_hours: number;
  escalation_threshold_hours: number;
  is_active: boolean;
}

export interface DiSlaStatus {
  rule_id: number | null;
  target_response_hours: number | null;
  target_resolution_hours: number | null;
  sla_deadline: string | null;
  response_elapsed_hours: number | null;
  resolution_elapsed_hours: number | null;
  is_response_breached: boolean;
  is_resolution_breached: boolean;
}

export interface SlaRuleUpdateInput {
  id: number;
  name: string;
  urgency_level: string;
  origin_type?: string | null;
  asset_criticality_class?: string | null;
  target_response_hours: number;
  target_resolution_hours: number;
  escalation_threshold_hours: number;
  is_active: boolean;
}

// ─── DI WO Conversion (File 03) ───────────────────────────────────────────

export interface WoConversionInput {
  di_id: number;
  actor_id: number;
  expected_row_version: number;
  conversion_notes?: string | null;
}

export interface WoConversionResult {
  di: InterventionRequest;
  wo_id: number;
  wo_code: string;
}

// Frontend invokes via: invoke("shutdown_app")

// ─── Asset — Health Score ───────────────────────────────────────────────────

export interface AssetHealthScore {
  asset_id: number;
  score: number | null;
  label: "good" | "fair" | "poor" | "no_data";
}

// ─── Asset — Photos ────────────────────────────────────────────────────────

export interface AssetPhoto {
  id: number;
  asset_id: number;
  file_name: string;
  file_path: string;
  mime_type: string;
  file_size_bytes: number;
  caption: string | null;
  created_by_id: number | null;
  created_at: string;
}

export interface UploadAssetPhotoPayload {
  asset_id: number;
  source_path: string;
  caption: string | null;
}

// ─── Asset — Decommission ──────────────────────────────────────────────────

export interface DecommissionAssetPayload {
  asset_id: number;
  target_status: "RETIRED" | "SCRAPPED" | "TRANSFERRED";
  reason: string;
  notes: string | null;
}

// ─── Dashboard ─────────────────────────────────────────────────────────────

export interface KpiValue {
  key: string;
  value: number;
  previous_value: number;
  available: boolean;
}

export interface DashboardKpis {
  open_dis: KpiValue;
  open_wos: KpiValue;
  total_assets: KpiValue;
  overdue_items: KpiValue;
}

export interface WorkloadDay {
  date: string;
  di_created: number;
  wo_completed: number;
  pm_due: number;
}

export interface DashboardWorkloadChart {
  days: WorkloadDay[];
  period_days: number;
}

// ─── DI Statistics (File 04) ───────────────────────────────────────────────

export interface DiStatsFilter {
  date_from?: string | null;
  date_to?: string | null;
  entity_id?: number | null;
}

export interface DiStatusCount {
  status: string;
  count: number;
}

export interface DiPriorityCount {
  priority: string;
  count: number;
}

export interface DiTypeCount {
  origin_type: string;
  count: number;
}

export interface DiTrendPoint {
  period: string;
  created: number;
  closed: number;
}

export interface DiEquipmentCount {
  asset_id: number;
  asset_label: string;
  count: number;
  percentage: number;
}

export interface DiOverdueDi {
  id: number;
  code: string;
  title: string;
  priority: string;
  days_overdue: number;
}

export interface DiStatsPayload {
  total: number;
  pending: number;
  in_progress: number;
  closed: number;
  closed_this_month: number;
  overdue: number;
  sla_met_count: number;
  sla_total: number;
  safety_issues: number;
  status_distribution: DiStatusCount[];
  priority_distribution: DiPriorityCount[];
  type_distribution: DiTypeCount[];
  monthly_trend: DiTrendPoint[];
  available_years: number[];
  avg_age_days: number;
  max_age_days: number;
  top_equipment: DiEquipmentCount[];
  overdue_dis: DiOverdueDi[];
}

// ─── Work Orders (OT) ─────────────────────────────────────────────────────

export type WoStatus =
  | "draft"
  | "awaiting_approval"
  | "planned"
  | "ready_to_schedule"
  | "assigned"
  | "waiting_for_prerequisite"
  | "in_progress"
  | "paused"
  | "mechanically_complete"
  | "technically_verified"
  | "closed"
  | "cancelled";

export type WoMacroState = "open" | "executing" | "completed" | "closed" | "cancelled";

export interface WorkOrder {
  id: number;
  code: string;
  // Classification
  type_id: number;
  status_id: number;
  // Asset context
  equipment_id: number | null;
  component_id: number | null;
  location_id: number | null;
  // People
  requester_id: number | null;
  source_di_id: number | null;
  entity_id: number | null;
  planner_id: number | null;
  approver_id: number | null;
  assigned_group_id: number | null;
  primary_responsible_id: number | null;
  // Urgency
  urgency_id: number | null;
  // Core description
  title: string;
  description: string | null;
  // Timing
  planned_start: string | null;
  planned_end: string | null;
  shift: WoShift | null;
  scheduled_at: string | null;
  actual_start: string | null;
  actual_end: string | null;
  mechanically_completed_at: string | null;
  technically_verified_at: string | null;
  closed_at: string | null;
  cancelled_at: string | null;
  // Duration accumulators
  expected_duration_hours: number | null;
  actual_duration_hours: number | null;
  active_labor_hours: number | null;
  total_waiting_hours: number | null;
  downtime_hours: number | null;
  // Cost accumulators
  labor_cost: number | null;
  parts_cost: number | null;
  service_cost: number | null;
  total_cost: number | null;
  // Close-out evidence
  recurrence_risk_level: string | null;
  production_impact_id: number | null;
  root_cause_summary: string | null;
  corrective_action_summary: string | null;
  verification_method: string | null;
  // Metadata
  notes: string | null;
  cancel_reason: string | null;
  row_version: number;
  created_at: string;
  updated_at: string;
  // Join fields (populated by queries)
  status_code?: string | null;
  status_label?: string | null;
  status_color?: string | null;
  type_code?: string | null;
  type_label?: string | null;
  urgency_level?: number | null;
  urgency_label?: string | null;
  urgency_color?: string | null;
  asset_code?: string | null;
  asset_label?: string | null;
  planner_username?: string | null;
  responsible_username?: string | null;
}

export interface WoTransitionRow {
  id: number;
  wo_id: number;
  from_status: string;
  to_status: string;
  action: string;
  actor_id: number | null;
  reason_code: string | null;
  notes: string | null;
  acted_at: string;
}

export interface WoCreateInput {
  type_id: number;
  equipment_id?: number | null;
  location_id?: number | null;
  source_di_id?: number | null;
  entity_id?: number | null;
  planner_id?: number | null;
  urgency_id?: number | null;
  title: string;
  description?: string | null;
  notes?: string | null;
  planned_start?: string | null;
  planned_end?: string | null;
  shift?: WoShift | null;
  expected_duration_hours?: number | null;
  creator_id: number;
}

export interface WoDraftUpdateInput {
  id: number;
  expected_row_version: number;
  title?: string | null;
  type_id?: number | null;
  equipment_id?: number | null;
  location_id?: number | null;
  description?: string | null;
  planned_start?: string | null;
  planned_end?: string | null;
  shift?: WoShift | null;
  expected_duration_hours?: number | null;
  notes?: string | null;
  urgency_id?: number | null;
}

export interface WoListFilter {
  status_codes?: string[] | null;
  type_codes?: string[] | null;
  equipment_id?: number | null;
  entity_id?: number | null;
  planner_id?: number | null;
  primary_responsible_id?: number | null;
  urgency_level?: number | null;
  source_di_id?: number | null;
  search?: string | null;
  date_from?: string | null;
  date_to?: string | null;
  limit: number;
  offset: number;
}

export interface WoListPage {
  items: WorkOrder[];
  total: number;
}

export interface WoGetResponse {
  wo: WorkOrder;
  transitions: WoTransitionRow[];
}

export interface WoCancelInput {
  id: number;
  expected_row_version: number;
  actor_id: number;
  cancel_reason: string;
}

// ── WO File 02 — Planning, Labor, Completion types ──────────────────────────

export type WoShift = "morning" | "afternoon" | "night" | "full_day";

export interface WoPlanInput {
  id: number;
  expected_row_version: number;
  planned_start?: string | null;
  planned_end?: string | null;
  expected_duration_hours?: number | null;
  shift?: WoShift | null;
  assigned_to_id?: number | null;
  team_id?: number | null;
}

export interface WoAssignInput {
  id: number;
  expected_row_version: number;
  assigned_to_id: number;
  team_id?: number | null;
}

export interface WoStartInput {
  id: number;
  expected_row_version: number;
}

export interface WoPauseInput {
  id: number;
  expected_row_version: number;
}

export interface WoResumeInput {
  id: number;
  expected_row_version: number;
}

export interface WoMechCompleteInput {
  wo_id: number;
  actor_id: number;
  expected_row_version: number;
  actual_end?: string | null;
  actual_duration_hours?: number | null;
  conclusion?: string | null;
}

export interface WoMechCompleteResponse {
  wo: WorkOrder;
  errors: WoPreflightError[];
}

export interface WoPreflightError {
  code: string;
  message: string;
}

export interface WoCloseInput {
  wo_id: number;
  actor_id: number;
  expected_row_version: number;
}

export interface WoLaborEntry {
  id: number;
  work_order_id: number;
  intervener_id: number;
  intervener_name: string | null;
  skill: string | null;
  started_at: string | null;
  ended_at: string | null;
  hours_worked: number | null;
  hourly_rate: number | null;
  notes: string | null;
  created_at: string;
}

export interface WoAddLaborInput {
  work_order_id: number;
  intervener_id: number;
  started_at?: string | null;
  skill?: string | null;
  hourly_rate?: number | null;
  notes?: string | null;
}

export interface WoCloseLaborInput {
  id: number;
  ended_at: string;
  hours_worked?: number | null;
}

export interface WoTask {
  id: number;
  work_order_id: number;
  sequence: number;
  description: string;
  is_mandatory: boolean;
  is_completed: boolean;
  completed_at: string | null;
  completed_by_id: number | null;
}

export interface WoPartUsage {
  id: number;
  work_order_id: number;
  part_id: number | null;
  part_label: string | null;
  quantity_planned: number | null;
  quantity_actual: number | null;
  unit_cost: number | null;
  notes: string | null;
}

export interface WoCostSummary {
  labor_cost: number;
  parts_cost: number;
  service_cost: number;
  total_cost: number;
  expected_duration_hours: number | null;
  actual_duration_hours: number | null;
}

// ── WO Execution sub-entity types (from wo-execution-service) ────────────────

export type TaskResultCode = "ok" | "nok" | "na" | "deferred";

export type DowntimeType = "full" | "partial" | "standby" | "quality_loss";

/** Task row as returned by execution commands (raw FK columns). */
export interface WoExecTask {
  id: number;
  work_order_id: number;
  task_description: string;
  sequence_order: number;
  estimated_minutes: number | null;
  is_mandatory: boolean;
  is_completed: boolean;
  completed_by_id: number | null;
  completed_at: string | null;
  result_code: TaskResultCode | null;
  notes: string | null;
}

/** Labor/intervener row as returned by execution commands (raw FK columns). */
export interface WoIntervener {
  id: number;
  work_order_id: number;
  intervener_id: number;
  skill_id: number | null;
  started_at: string | null;
  ended_at: string | null;
  hours_worked: number | null;
  hourly_rate: number | null;
  notes: string | null;
}

/** Part row as returned by execution commands (raw FK columns). */
export interface WoExecPart {
  id: number;
  work_order_id: number;
  article_id: number | null;
  article_ref: string | null;
  quantity_planned: number;
  quantity_used: number | null;
  unit_cost: number | null;
  stock_location_id: number | null;
  notes: string | null;
}

export interface WoDelaySegment {
  id: number;
  work_order_id: number;
  started_at: string;
  ended_at: string | null;
  delay_reason_id: number | null;
  comment: string | null;
  entered_by_id: number | null;
}

export interface WoDowntimeSegment {
  id: number;
  work_order_id: number;
  started_at: string;
  ended_at: string | null;
  downtime_type: DowntimeType;
  comment: string | null;
}

export interface WoStatsPayload {
  total: number;
  in_progress: number;
  completed: number;
  overdue: number;
  by_status: { status: string; count: number }[];
  by_urgency: { urgency: string; count: number }[];
  daily_completed: { date: string; count: number }[];
  by_entity: { entity: string; count: number }[];
}

// ─── Admin & Governance ────────────────────────────────────────────────────

export interface AdminStatsPayload {
  active_users: number;
  inactive_users: number;
  total_roles: number;
  system_roles: number;
  custom_roles: number;
  active_sessions: number;
  unassigned_users: number;
  emergency_grants_active: number;
}

export interface UserProfile {
  id: number;
  username: string;
  display_name: string | null;
  email: string | null;
  phone: string | null;
  language: string | null;
  identity_mode: string;
  created_at: string;
  password_changed_at: string | null;
  pin_configured: boolean;
  role_name: string | null;
}

export interface UpdateProfileInput {
  display_name?: string | null;
  email?: string | null;
  phone?: string | null;
  language?: string | null;
}

export interface ChangePasswordInput {
  current_password: string;
  new_password: string;
}

export interface SessionHistoryEntry {
  id: number | string;
  device_label: string | null;
  started_at: string;
  ended_at: string | null;
  duration_seconds: number | null;
  status: string;
}

export interface UserPresence {
  user_id: number;
  status: "active" | "idle" | "offline";
  last_activity_at: string | null;
}

export interface CreateUserInput {
  username: string;
  identity_mode: string;
  personnel_id?: number | null;
  initial_password?: string | null;
  force_password_change?: boolean;
}

export interface SetPinInput {
  current_password: string;
  new_pin: string;
}

export interface ClearPinInput {
  current_password: string;
}

export interface PinUnlockInput {
  pin: string;
}

export interface PasswordPolicySettings {
  max_age_days: number;
  warn_days_before_expiry: number;
  min_length: number;
  require_uppercase: boolean;
  require_lowercase: boolean;
  require_digit: boolean;
  require_special: boolean;
}

export interface RbacSettingEntry {
  key: string;
  value: string;
  description: string | null;
}
