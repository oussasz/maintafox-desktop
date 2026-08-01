import { z, ZodError } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  AcknowledgeBudgetAlertInput,
  BudgetActual,
  BudgetActualFilter,
  BudgetAlertConfig,
  BudgetAlertConfigFilter,
  BudgetAlertEvaluationResult,
  BudgetAlertEvent,
  BudgetAlertEventFilter,
  BudgetCommitment,
  BudgetCommitmentFilter,
  BudgetDashboardFilter,
  BudgetDashboardRow,
  BudgetDrilldownRow,
  BudgetForecast,
  BudgetForecastFilter,
  BudgetForecastGenerationResult,
  BudgetLine,
  BudgetLineFilter,
  BudgetReportPack,
  BudgetReportPackExport,
  BudgetReportPackFilter,
  BudgetVersion,
  BudgetVersionFilter,
  CostCenter,
  CostCenterFilter,
  CreateBudgetActualInput,
  CreateBudgetAlertConfigInput,
  CreateBudgetCommitmentInput,
  CreateBudgetLineInput,
  CreateBudgetSuccessorInput,
  CreateBudgetVarianceReviewInput,
  CreateBudgetVersionInput,
  CreateCostCenterInput,
  ErpApprovedReforecastExportItem,
  ErpExportBatchResult,
  ErpMasterImportResult,
  ErpPostedActualExportItem,
  EvaluateBudgetAlertsInput,
  ExportBudgetReportPackInput,
  ForecastRun,
  GenerateBudgetForecastInput,
  ImportErpCostCenterMasterInput,
  IntegrationException,
  IntegrationExceptionFilter,
  PostBudgetActualInput,
  PostedExportBatch,
  PostedExportBatchFilter,
  RecordErpExportBatchInput,
  ReverseBudgetActualInput,
  TransitionBudgetVarianceReviewInput,
  TransitionBudgetVersionLifecycleInput,
  UpdateBudgetAlertConfigInput,
  UpdateBudgetLineInput,
  UpdateBudgetVersionInput,
  UpdateCostCenterInput,
  UpdateIntegrationExceptionInput,
  BudgetVarianceReview,
  BudgetVarianceReviewFilter,
} from "@shared/ipc-types";

const CostCenterSchema = z.object({
  id: z.number(),
  code: z.string(),
  name: z.string(),
  entity_id: z.number().nullable(),
  entity_name: z.string().nullable(),
  parent_cost_center_id: z.number().nullable(),
  parent_cost_center_code: z.string().nullable(),
  budget_owner_id: z.number().nullable(),
  erp_external_id: z.string().nullable(),
  is_active: z.number(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const BudgetVersionSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  fiscal_year: z.number(),
  scenario_type: z.string(),
  version_no: z.number(),
  status: z.string(),
  currency_code: z.string(),
  title: z.string().nullable(),
  planning_basis: z.string().nullable(),
  source_basis_mix_json: z.string().nullable(),
  labor_assumptions_json: z.string().nullable(),
  baseline_reference: z.string().nullable(),
  erp_external_ref: z.string().nullable(),
  successor_of_version_id: z.number().nullable(),
  created_by_id: z.number().nullable(),
  approved_at: z.string().nullable(),
  approved_by_id: z.number().nullable(),
  frozen_at: z.string().nullable(),
  frozen_by_id: z.number().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const BudgetLineSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  budget_version_id: z.number(),
  cost_center_id: z.number(),
  cost_center_code: z.string(),
  cost_center_name: z.string(),
  period_month: z.number().nullable(),
  budget_bucket: z.string(),
  planned_amount: z.number(),
  source_basis: z.string().nullable(),
  justification_note: z.string().nullable(),
  asset_family: z.string().nullable(),
  work_category: z.string().nullable(),
  shutdown_package_ref: z.string().nullable(),
  team_id: z.number().nullable(),
  skill_pool_id: z.number().nullable(),
  labor_lane: z.string().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const BudgetActualSchema = z.object({
  id: z.number(),
  budget_version_id: z.number(),
  cost_center_id: z.number(),
  cost_center_code: z.string(),
  cost_center_name: z.string(),
  period_month: z.number().nullable(),
  budget_bucket: z.string(),
  amount_source: z.number(),
  source_currency: z.string(),
  amount_base: z.number(),
  base_currency: z.string(),
  source_type: z.string(),
  source_id: z.string(),
  work_order_id: z.number().nullable(),
  equipment_id: z.number().nullable(),
  posting_status: z.string(),
  provisional_reason: z.string().nullable(),
  posted_at: z.string().nullable(),
  posted_by_id: z.number().nullable(),
  reversal_of_actual_id: z.number().nullable(),
  reversal_reason: z.string().nullable(),
  personnel_id: z.number().nullable(),
  team_id: z.number().nullable(),
  rate_card_lane: z.string().nullable(),
  event_at: z.string(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const BudgetCommitmentSchema = z.object({
  id: z.number(),
  budget_version_id: z.number(),
  cost_center_id: z.number(),
  cost_center_code: z.string(),
  cost_center_name: z.string(),
  period_month: z.number().nullable(),
  budget_bucket: z.string(),
  commitment_type: z.string(),
  source_type: z.string(),
  source_id: z.string(),
  obligation_amount: z.number(),
  source_currency: z.string(),
  base_amount: z.number(),
  base_currency: z.string(),
  commitment_status: z.string(),
  work_order_id: z.number().nullable(),
  contract_id: z.number().nullable(),
  purchase_order_id: z.number().nullable(),
  planning_commitment_ref: z.string().nullable(),
  due_at: z.string().nullable(),
  explainability_note: z.string().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const ForecastRunSchema = z.object({
  id: z.number(),
  budget_version_id: z.number(),
  generated_by_id: z.number().nullable(),
  idempotency_key: z.string(),
  scope_signature: z.string(),
  method_mix_json: z.string().nullable(),
  confidence_policy_json: z.string().nullable(),
  generated_at: z.string(),
});

const BudgetForecastSchema = z.object({
  id: z.number(),
  forecast_run_id: z.number(),
  budget_version_id: z.number(),
  cost_center_id: z.number(),
  cost_center_code: z.string(),
  cost_center_name: z.string(),
  period_month: z.number().nullable(),
  budget_bucket: z.string(),
  forecast_amount: z.number(),
  forecast_method: z.string(),
  confidence_level: z.string(),
  driver_type: z.string().nullable(),
  driver_reference: z.string().nullable(),
  explainability_json: z.string().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const BudgetForecastGenerationResultSchema = z.object({
  run: ForecastRunSchema,
  forecasts: z.array(BudgetForecastSchema),
  reused_existing_run: z.boolean(),
});

const BudgetVarianceReviewSchema = z.object({
  id: z.number(),
  budget_version_id: z.number(),
  cost_center_id: z.number(),
  cost_center_code: z.string(),
  cost_center_name: z.string(),
  period_month: z.number().nullable(),
  budget_bucket: z.string(),
  variance_amount: z.number(),
  variance_pct: z.number(),
  driver_code: z.string(),
  action_owner_id: z.number(),
  review_status: z.string(),
  review_commentary: z.string(),
  snapshot_context_json: z.string(),
  opened_at: z.string(),
  reviewed_at: z.string().nullable(),
  closed_at: z.string().nullable(),
  reopened_from_review_id: z.number().nullable(),
  reopen_reason: z.string().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const BudgetDashboardRowSchema = z.object({
  budget_version_id: z.number(),
  cost_center_id: z.number(),
  cost_center_code: z.string(),
  cost_center_name: z.string(),
  period_month: z.number().nullable(),
  budget_bucket: z.string(),
  spend_mix: z.string(),
  team_id: z.number().nullable(),
  assignee_id: z.number().nullable(),
  labor_lane: z.string().nullable(),
  planned_amount: z.number(),
  committed_amount: z.number(),
  actual_amount: z.number(),
  forecast_amount: z.number(),
  variance_to_plan: z.number(),
  variance_to_forecast: z.number(),
  currency_code: z.string(),
  source_links_json: z.string(),
});

const BudgetDrilldownRowSchema = z.object({
  layer_type: z.string(),
  record_id: z.number(),
  budget_version_id: z.number(),
  cost_center_id: z.number(),
  cost_center_code: z.string(),
  period_month: z.number().nullable(),
  budget_bucket: z.string(),
  amount: z.number(),
  currency_code: z.string(),
  source_type: z.string().nullable(),
  source_id: z.string().nullable(),
  work_order_id: z.number().nullable(),
  pm_occurrence_ref: z.string().nullable(),
  inspection_ref: z.string().nullable(),
  shutdown_package_ref: z.string().nullable(),
  team_id: z.number().nullable(),
  assignee_id: z.number().nullable(),
  labor_lane: z.string().nullable(),
  hours_overrun_rate: z.number().nullable(),
  first_pass_effect: z.number().nullable(),
  repeat_work_penalty: z.number().nullable(),
  schedule_discipline_impact: z.number().nullable(),
});

const ErpMasterImportResultSchema = z.object({
  imported_count: z.number(),
  linked_count: z.number(),
  inactive_count: z.number(),
});

const ErpPostedActualExportItemSchema = z.object({
  actual_id: z.number(),
  budget_version_id: z.number(),
  fiscal_year: z.number(),
  scenario_type: z.string(),
  external_cost_center_code: z.string().nullable(),
  local_cost_center_code: z.string(),
  budget_bucket: z.string(),
  amount_source: z.number(),
  source_currency: z.string(),
  amount_base: z.number(),
  base_currency: z.string(),
  source_type: z.string(),
  source_id: z.string(),
  posted_at: z.string().nullable(),
  reconciliation_flags: z.array(z.string()),
});

const ErpApprovedReforecastExportItemSchema = z.object({
  forecast_id: z.number(),
  forecast_run_id: z.number(),
  budget_version_id: z.number(),
  fiscal_year: z.number(),
  scenario_type: z.string(),
  version_status: z.string(),
  external_cost_center_code: z.string().nullable(),
  local_cost_center_code: z.string(),
  period_month: z.number().nullable(),
  budget_bucket: z.string(),
  forecast_amount: z.number(),
  base_currency: z.string(),
  forecast_method: z.string(),
  confidence_level: z.string(),
  reconciliation_flags: z.array(z.string()),
});

const PostedExportBatchSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  batch_uuid: z.string(),
  export_kind: z.string(),
  tenant_id: z.string().nullable(),
  relay_payload_json: z.string(),
  total_posted: z.number(),
  line_count: z.number(),
  status: z.string(),
  erp_ack_at: z.string().nullable(),
  erp_http_code: z.number().nullable(),
  rejection_code: z.string().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const IntegrationExceptionSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  posted_export_batch_id: z.number(),
  source_record_kind: z.string(),
  source_record_id: z.number(),
  maintafox_value_snapshot: z.string(),
  external_value_snapshot: z.string().nullable(),
  resolution_status: z.string(),
  rejection_code: z.string().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const ErpExportBatchResultSchema = z.object({
  batch: PostedExportBatchSchema,
  jsonl: z.string(),
  integration_exceptions: z.array(IntegrationExceptionSchema),
});

const BudgetAlertConfigSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  budget_version_id: z.number().nullable(),
  cost_center_id: z.number().nullable(),
  budget_bucket: z.string().nullable(),
  alert_type: z.string(),
  threshold_pct: z.number().nullable(),
  threshold_amount: z.number().nullable(),
  recipient_user_id: z.number().nullable(),
  recipient_role_id: z.number().nullable(),
  labor_template: z.string().nullable(),
  dedupe_window_minutes: z.number(),
  requires_ack: z.boolean(),
  is_active: z.boolean(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const BudgetAlertEventSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  alert_config_id: z.number().nullable(),
  budget_version_id: z.number(),
  cost_center_id: z.number(),
  cost_center_code: z.string(),
  cost_center_name: z.string(),
  period_month: z.number().nullable(),
  budget_bucket: z.string(),
  alert_type: z.string(),
  severity: z.string(),
  title: z.string(),
  message: z.string(),
  dedupe_key: z.string(),
  current_value: z.number(),
  threshold_value: z.number().nullable(),
  variance_amount: z.number().nullable(),
  currency_code: z.string(),
  payload_json: z.string().nullable(),
  notification_event_id: z.number().nullable(),
  notification_id: z.number().nullable(),
  acknowledged_at: z.string().nullable(),
  acknowledged_by_id: z.number().nullable(),
  acknowledgement_note: z.string().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const BudgetAlertEvaluationResultSchema = z.object({
  evaluated_at: z.string(),
  emitted_count: z.number(),
  deduped_count: z.number(),
  considered_rows: z.number(),
  events: z.array(BudgetAlertEventSchema),
});

const BudgetReportPackTotalsSchema = z.object({
  baseline_amount: z.number(),
  commitment_amount: z.number(),
  posted_actual_amount: z.number(),
  forecast_amount: z.number(),
  variance_amount: z.number(),
  variance_pct: z.number(),
});

const BudgetReportPackSchema = z.object({
  generated_at: z.string(),
  budget_version_id: z.number(),
  fiscal_year: z.number(),
  scenario_type: z.string(),
  version_status: z.string(),
  currency_code: z.string(),
  posting_status_filter: z.string(),
  forecast_method_mix_json: z.string(),
  totals: BudgetReportPackTotalsSchema,
  spend_mix_json: z.string(),
  top_work_orders_json: z.string(),
  top_assets_json: z.string(),
  workforce_efficiency_json: z.string(),
  explainability_json: z.string(),
  multi_currency_flags: z.array(z.string()),
});

const BudgetReportPackExportSchema = z.object({
  format: z.string(),
  file_name: z.string(),
  mime_type: z.string(),
  content: z.string(),
  report: BudgetReportPackSchema,
});

interface IpcErrorShape {
  code: string;
  message: string;
}

function isIpcError(err: unknown): err is IpcErrorShape {
  return typeof err === "object" && err !== null && "code" in err && "message" in err;
}

export class BudgetIpcError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "BudgetIpcError";
    this.code = code;
  }
}

function mapInvokeError(err: unknown): never {
  if (isIpcError(err)) throw new BudgetIpcError(err.code, err.message);
  if (err instanceof Error) throw err;
  throw new Error(String(err));
}

async function invokeParsed<T>(
  command: string,
  args: Record<string, unknown> | undefined,
  schema: z.ZodType<T>,
): Promise<T> {
  try {
    const raw = await invoke<unknown>(command, args);
    return schema.parse(raw);
  } catch (err) {
    if (err instanceof ZodError) throw new BudgetIpcError("VALIDATION_FAILED", err.message);
    mapInvokeError(err);
  }
}

export function listCostCenters(filter: CostCenterFilter = {}): Promise<CostCenter[]> {
  return invokeParsed("list_cost_centers", { filter }, z.array(CostCenterSchema));
}

export function createCostCenter(input: CreateCostCenterInput): Promise<CostCenter> {
  return invokeParsed("create_cost_center", { input }, CostCenterSchema);
}

export function updateCostCenter(
  costCenterId: number,
  expectedRowVersion: number,
  input: UpdateCostCenterInput,
): Promise<CostCenter> {
  return invokeParsed(
    "update_cost_center",
    { costCenterId, expectedRowVersion, input },
    CostCenterSchema,
  );
}

export function listBudgetVersions(filter: BudgetVersionFilter = {}): Promise<BudgetVersion[]> {
  return invokeParsed("list_budget_versions", { filter }, z.array(BudgetVersionSchema));
}

export function createBudgetVersion(input: CreateBudgetVersionInput): Promise<BudgetVersion> {
  return invokeParsed("create_budget_version", { input }, BudgetVersionSchema);
}

export function createBudgetSuccessorVersion(
  input: CreateBudgetSuccessorInput,
): Promise<BudgetVersion> {
  return invokeParsed("create_budget_successor_version", { input }, BudgetVersionSchema);
}

export function updateBudgetVersion(
  versionId: number,
  expectedRowVersion: number,
  input: UpdateBudgetVersionInput,
): Promise<BudgetVersion> {
  return invokeParsed(
    "update_budget_version",
    { versionId, expectedRowVersion, input },
    BudgetVersionSchema,
  );
}

export function transitionBudgetVersionLifecycle(
  input: TransitionBudgetVersionLifecycleInput,
): Promise<BudgetVersion> {
  return invokeParsed("transition_budget_version_lifecycle", { input }, BudgetVersionSchema);
}

export function listBudgetLines(filter: BudgetLineFilter = {}): Promise<BudgetLine[]> {
  return invokeParsed("list_budget_lines", { filter }, z.array(BudgetLineSchema));
}

export function createBudgetLine(input: CreateBudgetLineInput): Promise<BudgetLine> {
  return invokeParsed("create_budget_line", { input }, BudgetLineSchema);
}

export function updateBudgetLine(
  lineId: number,
  expectedRowVersion: number,
  input: UpdateBudgetLineInput,
): Promise<BudgetLine> {
  return invokeParsed(
    "update_budget_line",
    { lineId, expectedRowVersion, input },
    BudgetLineSchema,
  );
}

export function listBudgetActuals(filter: BudgetActualFilter = {}): Promise<BudgetActual[]> {
  return invokeParsed("list_budget_actuals", { filter }, z.array(BudgetActualSchema));
}

export function createBudgetActual(input: CreateBudgetActualInput): Promise<BudgetActual> {
  return invokeParsed("create_budget_actual", { input }, BudgetActualSchema);
}

export function postBudgetActual(input: PostBudgetActualInput): Promise<BudgetActual> {
  return invokeParsed("post_budget_actual", { input }, BudgetActualSchema);
}

export function reverseBudgetActual(input: ReverseBudgetActualInput): Promise<BudgetActual> {
  return invokeParsed("reverse_budget_actual", { input }, BudgetActualSchema);
}

export function listBudgetCommitments(
  filter: BudgetCommitmentFilter = {},
): Promise<BudgetCommitment[]> {
  return invokeParsed("list_budget_commitments", { filter }, z.array(BudgetCommitmentSchema));
}

export function createBudgetCommitment(
  input: CreateBudgetCommitmentInput,
): Promise<BudgetCommitment> {
  return invokeParsed("create_budget_commitment", { input }, BudgetCommitmentSchema);
}

export function listForecastRuns(budgetVersionId?: number): Promise<ForecastRun[]> {
  return invokeParsed(
    "list_forecast_runs",
    { budgetVersionId: budgetVersionId ?? null },
    z.array(ForecastRunSchema),
  );
}

export function listBudgetForecasts(filter: BudgetForecastFilter = {}): Promise<BudgetForecast[]> {
  return invokeParsed("list_budget_forecasts", { filter }, z.array(BudgetForecastSchema));
}

export function generateBudgetForecasts(
  input: GenerateBudgetForecastInput,
): Promise<BudgetForecastGenerationResult> {
  return invokeParsed("generate_budget_forecasts", { input }, BudgetForecastGenerationResultSchema);
}

export function listBudgetVarianceReviews(
  filter: BudgetVarianceReviewFilter = {},
): Promise<BudgetVarianceReview[]> {
  return invokeParsed(
    "list_budget_variance_reviews",
    { filter },
    z.array(BudgetVarianceReviewSchema),
  );
}

export function createBudgetVarianceReview(
  input: CreateBudgetVarianceReviewInput,
): Promise<BudgetVarianceReview> {
  return invokeParsed("create_budget_variance_review", { input }, BudgetVarianceReviewSchema);
}

export function transitionBudgetVarianceReview(
  input: TransitionBudgetVarianceReviewInput,
): Promise<BudgetVarianceReview> {
  return invokeParsed("transition_budget_variance_review", { input }, BudgetVarianceReviewSchema);
}

export function listBudgetDashboardRows(
  filter: BudgetDashboardFilter = {},
): Promise<BudgetDashboardRow[]> {
  return invokeParsed("list_budget_dashboard_rows", { filter }, z.array(BudgetDashboardRowSchema));
}

export function listBudgetDashboardDrilldown(
  filter: BudgetDashboardFilter = {},
): Promise<BudgetDrilldownRow[]> {
  return invokeParsed(
    "list_budget_dashboard_drilldown",
    { filter },
    z.array(BudgetDrilldownRowSchema),
  );
}

export function importErpCostCenterMaster(
  input: ImportErpCostCenterMasterInput,
): Promise<ErpMasterImportResult> {
  return invokeParsed("import_erp_cost_center_master", { input }, ErpMasterImportResultSchema);
}

export function exportPostedActualsForErp(): Promise<ErpPostedActualExportItem[]> {
  return invokeParsed(
    "export_posted_actuals_for_erp",
    undefined,
    z.array(ErpPostedActualExportItemSchema),
  );
}

export function exportApprovedReforecastsForErp(): Promise<ErpApprovedReforecastExportItem[]> {
  return invokeParsed(
    "export_approved_reforecasts_for_erp",
    undefined,
    z.array(ErpApprovedReforecastExportItemSchema),
  );
}

export function recordErpExportBatch(
  input: RecordErpExportBatchInput,
): Promise<ErpExportBatchResult> {
  return invokeParsed("record_erp_export_batch", { input }, ErpExportBatchResultSchema);
}

export function listPostedExportBatches(
  filter: PostedExportBatchFilter = {},
): Promise<PostedExportBatch[]> {
  return invokeParsed("list_posted_export_batches", { filter }, z.array(PostedExportBatchSchema));
}

export function listIntegrationExceptions(
  filter: IntegrationExceptionFilter = {},
): Promise<IntegrationException[]> {
  return invokeParsed(
    "list_integration_exceptions",
    { filter },
    z.array(IntegrationExceptionSchema),
  );
}

export function updateIntegrationException(
  exceptionId: number,
  expectedRowVersion: number,
  input: UpdateIntegrationExceptionInput,
): Promise<IntegrationException> {
  return invokeParsed(
    "update_integration_exception",
    { exceptionId, expectedRowVersion, input },
    IntegrationExceptionSchema,
  );
}

export function listBudgetAlertConfigs(
  filter: BudgetAlertConfigFilter = {},
): Promise<BudgetAlertConfig[]> {
  return invokeParsed("list_budget_alert_configs", { filter }, z.array(BudgetAlertConfigSchema));
}

export function createBudgetAlertConfig(
  input: CreateBudgetAlertConfigInput,
): Promise<BudgetAlertConfig> {
  return invokeParsed("create_budget_alert_config", { input }, BudgetAlertConfigSchema);
}

export function updateBudgetAlertConfig(
  configId: number,
  expectedRowVersion: number,
  input: UpdateBudgetAlertConfigInput,
): Promise<BudgetAlertConfig> {
  return invokeParsed(
    "update_budget_alert_config",
    { configId, expectedRowVersion, input },
    BudgetAlertConfigSchema,
  );
}

export function listBudgetAlertEvents(
  filter: BudgetAlertEventFilter = {},
): Promise<BudgetAlertEvent[]> {
  return invokeParsed("list_budget_alert_events", { filter }, z.array(BudgetAlertEventSchema));
}

export function evaluateBudgetAlerts(
  input: EvaluateBudgetAlertsInput,
): Promise<BudgetAlertEvaluationResult> {
  return invokeParsed("evaluate_budget_alerts", { input }, BudgetAlertEvaluationResultSchema);
}

export function acknowledgeBudgetAlert(
  input: AcknowledgeBudgetAlertInput,
): Promise<BudgetAlertEvent> {
  return invokeParsed("acknowledge_budget_alert", { input }, BudgetAlertEventSchema);
}

export function buildBudgetReportPack(filter: BudgetReportPackFilter): Promise<BudgetReportPack> {
  return invokeParsed("build_budget_report_pack", { filter }, BudgetReportPackSchema);
}

export function exportBudgetReportPack(
  input: ExportBudgetReportPackInput,
): Promise<BudgetReportPackExport> {
  return invokeParsed("export_budget_report_pack", { input }, BudgetReportPackExportSchema);
}
