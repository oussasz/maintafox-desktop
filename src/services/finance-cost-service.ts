/**
 * Maintenance cost flow: OT cost (estimated / planned / committed / actual), settlement of
 * closed OTs into the budget, budget cockpit with drill-down, budget periods and asset cost
 * history. Backed by `src-tauri/src/finance/{wo_costing,settlement,cockpit,periods,asset_costs}.rs`.
 */
import { z, ZodError } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import { BudgetIpcError } from "@/services/budget-service";

async function invokeParsed<T>(
  command: string,
  args: Record<string, unknown> | undefined,
  schema: z.ZodType<T>,
): Promise<T> {
  try {
    return schema.parse(await invoke<unknown>(command, args));
  } catch (err) {
    if (err instanceof ZodError) throw new BudgetIpcError("VALIDATION_FAILED", err.message);
    throw err;
  }
}

// ── OT cost ─────────────────────────────────────────────────────────────────────────────

const FiguresSchema = z.object({
  estimated: z.number().nullable(),
  planned: z.number().nullable(),
  committed: z.number(),
  actual: z.number(),
});
export type WoCostFigures = z.infer<typeof FiguresSchema>;

const LaborLineSchema = z.object({
  intervener_entry_id: z.number(),
  personnel_id: z.number().nullable(),
  person_name: z.string().nullable(),
  skill_label: z.string().nullable(),
  hours: z.number(),
  hourly_rate: z.number().nullable(),
  is_overtime: z.boolean(),
  amount: z.number(),
});

const PartsLineSchema = z.object({
  article_id: z.number().nullable(),
  stock_backed: z.boolean(),
  article_code: z.string(),
  article_name: z.string(),
  issued_qty: z.number(),
  returned_qty: z.number(),
  net_qty: z.number(),
  amount: z.number(),
  unvalued_qty: z.number(),
});

const PurchaseLineSchema = z.object({
  purchase_order_id: z.number(),
  po_number: z.string(),
  po_status: z.string(),
  po_line_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  supplier_name: z.string().nullable(),
  demand_source_type: z.string(),
  ordered_qty: z.number(),
  received_qty: z.number(),
  unit_price_tenant: z.number().nullable(),
  ordered_amount: z.number().nullable(),
  received_amount: z.number(),
  open_commitment: z.number(),
});
export type WoPurchaseCostLine = z.infer<typeof PurchaseLineSchema>;

const EstimateSchema = z.object({
  id: z.number(),
  work_order_id: z.number(),
  cost_center_id: z.number().nullable(),
  estimated_labor: z.number(),
  estimated_parts: z.number(),
  estimated_services: z.number(),
  estimated_other: z.number(),
  currency_code: z.string(),
  notes: z.string().nullable(),
  row_version: z.number(),
  updated_at: z.string(),
});
export type WoCostEstimate = z.infer<typeof EstimateSchema>;

const ResolvedCostCenterSchema = z.object({
  cost_center_id: z.number(),
  code: z.string(),
  name: z.string(),
  source: z.string(),
});

const BreakdownSchema = z.object({
  work_order_id: z.number(),
  work_order_code: z.string(),
  status_code: z.string(),
  equipment_id: z.number().nullable(),
  currency_code: z.string(),
  labor: FiguresSchema,
  parts: FiguresSchema,
  services: FiguresSchema,
  other: FiguresSchema,
  total: FiguresSchema,
  labor_overtime_amount: z.number(),
  overtime_captured: z.boolean(),
  labor_hours: z.number(),
  unpriced_labor_hours: z.number(),
  unvalued_parts_qty: z.number(),
  planned_labor_gap: z.string().nullable(),
  labor_lines: z.array(LaborLineSchema),
  parts_lines: z.array(PartsLineSchema),
  purchase_lines: z.array(PurchaseLineSchema),
  estimate: EstimateSchema.nullable(),
  cost_center: ResolvedCostCenterSchema.nullable(),
});
export type WoCostBreakdown = z.infer<typeof BreakdownSchema>;

const SettlementSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  work_order_id: z.number(),
  work_order_code: z.string(),
  work_order_title: z.string().nullable(),
  equipment_id: z.number().nullable(),
  equipment_code: z.string().nullable(),
  settlement_cycle: z.number(),
  status: z.string(),
  pending_reason: z.string().nullable(),
  cost_center_id: z.number().nullable(),
  cost_center_code: z.string().nullable(),
  cost_center_name: z.string().nullable(),
  budget_version_id: z.number().nullable(),
  budget_version_title: z.string().nullable(),
  fiscal_year: z.number().nullable(),
  period_month: z.number().nullable(),
  currency_code: z.string(),
  labor_amount: z.number(),
  labor_overtime_amount: z.number(),
  parts_amount: z.number(),
  services_amount: z.number(),
  other_amount: z.number(),
  total_amount: z.number(),
  overtime_captured: z.boolean(),
  unpriced_labor_hours: z.number(),
  unvalued_parts_qty: z.number(),
  closed_at: z.string().nullable(),
  posted_at: z.string().nullable(),
  reversed_at: z.string().nullable(),
  reversal_reason: z.string().nullable(),
  row_version: z.number(),
});
export type WoCostSettlement = z.infer<typeof SettlementSchema>;

const WoCostViewSchema = z.object({
  breakdown: BreakdownSchema,
  settlements: z.array(SettlementSchema),
});
export type WoCostView = z.infer<typeof WoCostViewSchema>;

export interface UpsertWoCostEstimateInput {
  work_order_id: number;
  cost_center_id: number | null;
  estimated_labor: number;
  estimated_parts: number;
  estimated_services: number;
  estimated_other: number;
  notes: string | null;
  expected_row_version: number | null;
}

export interface WoSettlementFilter {
  status?: "posted" | "pending" | "reversed" | null;
  work_order_id?: number | null;
  cost_center_id?: number | null;
  fiscal_year?: number | null;
  limit?: number | null;
}

export function getWoCostView(workOrderId: number): Promise<WoCostView> {
  return invokeParsed("get_wo_cost_view", { workOrderId }, WoCostViewSchema);
}

export function upsertWoCostEstimate(input: UpsertWoCostEstimateInput): Promise<WoCostEstimate> {
  return invokeParsed("upsert_wo_cost_estimate", { input }, EstimateSchema);
}

export function listWoSettlements(filter: WoSettlementFilter = {}): Promise<WoCostSettlement[]> {
  return invokeParsed("list_wo_settlements", { filter }, z.array(SettlementSchema));
}

export function retryWoSettlement(settlementId: number): Promise<WoCostSettlement> {
  return invokeParsed("retry_wo_settlement", { settlementId }, SettlementSchema);
}

// ── Budget cockpit ──────────────────────────────────────────────────────────────────────

const CockpitFiguresSchema = z.object({
  budget: z.number(),
  committed: z.number(),
  actual: z.number(),
  forecast: z.number(),
  variance: z.number(),
});
export type CockpitFigures = z.infer<typeof CockpitFiguresSchema>;

const CockpitRowSchema = CockpitFiguresSchema.extend({
  cost_center_id: z.number(),
  cost_center_code: z.string(),
  cost_center_name: z.string(),
  bucket: z.string(),
  consumed_pct: z.number().nullable(),
  open_reviews: z.number(),
  latest_driver_code: z.string().nullable(),
});
export type BudgetCockpitRow = z.infer<typeof CockpitRowSchema>;

const CockpitMonthSchema = z.object({
  period_month: z.number(),
  budget: z.number(),
  actual: z.number(),
  cumulative_budget: z.number(),
  cumulative_actual: z.number(),
});
export type BudgetCockpitMonth = z.infer<typeof CockpitMonthSchema>;

const CockpitSchema = z.object({
  fiscal_year: z.number(),
  version: z
    .object({
      id: z.number(),
      title: z.string().nullable(),
      status: z.string(),
      scenario_type: z.string(),
      version_no: z.number(),
      currency_code: z.string(),
    })
    .nullable(),
  currency_code: z.string().nullable(),
  as_of_month: z.number().nullable(),
  totals: CockpitFiguresSchema,
  rows: z.array(CockpitRowSchema),
  months: z.array(CockpitMonthSchema),
  pending_settlements: z.number(),
  pending_settlement_amount: z.number(),
  unassigned_commitments: z.number(),
  setup: z.object({
    cost_centers: z.number(),
    versions_for_year: z.number(),
    budget_lines: z.number(),
  }),
});
export type BudgetCockpit = z.infer<typeof CockpitSchema>;

const DrilldownActualSchema = z.object({
  actual_id: z.number(),
  period_month: z.number().nullable(),
  bucket: z.string(),
  amount: z.number(),
  posting_status: z.string(),
  source_type: z.string(),
  source_id: z.string(),
  rate_card_lane: z.string().nullable(),
  work_order_id: z.number().nullable(),
  work_order_code: z.string().nullable(),
  work_order_title: z.string().nullable(),
  equipment_code: z.string().nullable(),
  event_at: z.string().nullable(),
  is_reversal: z.boolean(),
});
export type DrilldownActual = z.infer<typeof DrilldownActualSchema>;

const DrilldownCommitmentSchema = z.object({
  kind: z.string(),
  reference: z.string(),
  document_id: z.number().nullable(),
  supplier_name: z.string().nullable(),
  bucket: z.string(),
  work_order_id: z.number().nullable(),
  work_order_code: z.string().nullable(),
  amount: z.number(),
  due_at: z.string().nullable(),
});
export type DrilldownCommitment = z.infer<typeof DrilldownCommitmentSchema>;

const DrilldownSchema = z.object({
  actuals: z.array(DrilldownActualSchema),
  commitments: z.array(DrilldownCommitmentSchema),
});
export type BudgetCockpitDrilldown = z.infer<typeof DrilldownSchema>;

export interface BudgetCockpitFilter {
  fiscal_year: number;
  budget_version_id?: number | null;
  cost_center_id?: number | null;
}

export interface BudgetCockpitDrilldownFilter {
  fiscal_year: number;
  budget_version_id?: number | null;
  cost_center_id: number;
  bucket?: string | null;
  period_month?: number | null;
}

export function getBudgetCockpit(filter: BudgetCockpitFilter): Promise<BudgetCockpit> {
  return invokeParsed("get_budget_cockpit", { filter }, CockpitSchema);
}

export function getBudgetCockpitDrilldown(
  filter: BudgetCockpitDrilldownFilter,
): Promise<BudgetCockpitDrilldown> {
  return invokeParsed("get_budget_cockpit_drilldown", { filter }, DrilldownSchema);
}

// ── Budget periods ──────────────────────────────────────────────────────────────────────

const PeriodSchema = z.object({
  fiscal_year: z.number(),
  period_month: z.number(),
  status: z.string(),
  closed_at: z.string().nullable(),
  closed_by_name: z.string().nullable(),
  reopened_at: z.string().nullable(),
  notes: z.string().nullable(),
  actual_amount: z.number(),
  pending_settlements: z.number(),
  is_future: z.boolean(),
  row_version: z.number().nullable(),
});
export type BudgetPeriod = z.infer<typeof PeriodSchema>;

export function listBudgetPeriods(fiscalYear: number): Promise<BudgetPeriod[]> {
  return invokeParsed("list_budget_periods", { fiscalYear }, z.array(PeriodSchema));
}

export function closeBudgetPeriod(input: {
  fiscal_year: number;
  period_month: number;
  notes: string | null;
}): Promise<BudgetPeriod[]> {
  return invokeParsed("close_budget_period", { input }, z.array(PeriodSchema));
}

export function reopenBudgetPeriod(input: {
  fiscal_year: number;
  period_month: number;
  reason: string;
}): Promise<BudgetPeriod[]> {
  return invokeParsed("reopen_budget_period", { input }, z.array(PeriodSchema));
}

// ── Asset cost history ──────────────────────────────────────────────────────────────────

const AssetCostRowSchema = z.object({
  equipment_id: z.number(),
  equipment_code: z.string(),
  equipment_name: z.string(),
  cost_center_code: z.string().nullable(),
  replacement_value: z.number().nullable(),
  lifetime_cost: z.number(),
  lifetime_labor: z.number(),
  lifetime_parts: z.number(),
  lifetime_services: z.number(),
  cost_12m: z.number(),
  closed_work_orders: z.number(),
  corrective_work_orders: z.number(),
  corrective_cost: z.number(),
  preventive_cost: z.number(),
  avg_corrective_cost: z.number().nullable(),
  avg_preventive_cost: z.number().nullable(),
  cost_12m_to_rav_pct: z.number().nullable(),
  lifetime_to_rav_pct: z.number().nullable(),
  last_closed_at: z.string().nullable(),
  by_year: z.array(z.object({ year: z.number(), amount: z.number() })),
});
export type AssetCostRow = z.infer<typeof AssetCostRowSchema>;

const AssetCostHistorySchema = z.object({
  currency_code: z.string().nullable(),
  rows: z.array(AssetCostRowSchema),
  fleet_replacement_value: z.number(),
  fleet_cost_12m: z.number(),
  fleet_cost_to_rav_pct: z.number().nullable(),
  assets_without_rav: z.number(),
});
export type AssetCostHistory = z.infer<typeof AssetCostHistorySchema>;

export interface AssetCostFilter {
  search?: string | null;
  cost_center_code?: string | null;
  only_with_cost?: boolean | null;
  limit?: number | null;
}

export function listAssetCostHistory(filter: AssetCostFilter = {}): Promise<AssetCostHistory> {
  return invokeParsed("list_asset_cost_history", { filter }, AssetCostHistorySchema);
}
