import { z, ZodError } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  CapacityRule,
  CapacityRuleFilter,
  CreateCapacityRuleInput,
  CreateScheduleBreakInInput,
  CreatePlanningWindowInput,
  CreateScheduleCommitmentInput,
  ExportPlanningGanttPdfInput,
  ExportedBinaryDocument,
  FreezeSchedulePeriodInput,
  NotifyTeamsInput,
  NotifyTeamsResult,
  PlanningGanttFilter,
  PlanningGanttSnapshot,
  PlanningWindow,
  PlanningWindowFilter,
  RefreshScheduleCandidatesInput,
  RefreshScheduleCandidatesResult,
  RescheduleCommitmentInput,
  ScheduleBacklogSnapshot,
  ScheduleBreakIn,
  ScheduleBreakInFilter,
  ScheduleCandidateFilter,
  ScheduleChangeLogEntry,
  ScheduleCommitment,
  ScheduleCommitmentFilter,
  SchedulingConflict,
  TeamCapacityLoad,
  UpdateCapacityRuleInput,
  UpdatePlanningWindowInput,
} from "@shared/ipc-types";

const ScheduleCandidateSchema = z.object({
  id: z.number(),
  source_type: z.string(),
  source_id: z.number(),
  source_di_id: z.number().nullable(),
  readiness_status: z.string(),
  readiness_score: z.number(),
  priority_id: z.number().nullable(),
  required_skill_set_json: z.string().nullable(),
  required_parts_ready: z.number(),
  permit_status: z.string(),
  shutdown_requirement: z.string().nullable(),
  prerequisite_status: z.string(),
  estimated_duration_hours: z.number().nullable(),
  assigned_personnel_id: z.number().nullable(),
  assigned_team_id: z.number().nullable(),
  window_start: z.string().nullable(),
  window_end: z.string().nullable(),
  suggested_assignees_json: z.string().nullable(),
  availability_conflict_count: z.number(),
  skill_match_score: z.number().nullable(),
  estimated_labor_cost_range_json: z.string().nullable(),
  blocking_flags_json: z.string().nullable(),
  open_work_count: z.number().nullable(),
  next_available_window: z.string().nullable(),
  estimated_assignment_risk: z.number().nullable(),
  risk_reason_codes_json: z.string().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const SchedulingConflictSchema = z.object({
  id: z.number(),
  candidate_id: z.number(),
  conflict_type: z.string(),
  reference_type: z.string().nullable(),
  reference_id: z.number().nullable(),
  reason_code: z.string(),
  severity: z.string(),
  details_json: z.string().nullable(),
  resolved_at: z.string().nullable(),
  created_at: z.string(),
});

const CandidateConflictSummarySchema = z.object({
  candidate_id: z.number(),
  blocker_codes: z.array(z.string()),
  blocker_dimensions: z.array(z.string()),
  readiness_status: z.string(),
  readiness_score: z.number(),
});

const ScheduleBacklogSnapshotSchema = z.object({
  as_of: z.string(),
  candidate_count: z.number(),
  ready_count: z.number(),
  blocked_count: z.number(),
  candidates: z.array(ScheduleCandidateSchema),
  conflict_summary: z.array(CandidateConflictSummarySchema),
  derivation_rules: z.array(z.string()),
});

const RefreshScheduleCandidatesResultSchema = z.object({
  inserted_count: z.number(),
  updated_count: z.number(),
  evaluated_count: z.number(),
  ready_count: z.number(),
  blocked_count: z.number(),
});

const CapacityRuleSchema = z.object({
  id: z.number(),
  entity_id: z.number().nullable(),
  team_id: z.number(),
  effective_start: z.string(),
  effective_end: z.string().nullable(),
  available_hours_per_day: z.number(),
  max_overtime_hours_per_day: z.number(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const PlanningWindowSchema = z.object({
  id: z.number(),
  entity_id: z.number().nullable(),
  window_type: z.string(),
  start_datetime: z.string(),
  end_datetime: z.string(),
  is_locked: z.number(),
  lock_reason: z.string().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const ScheduleCommitmentSchema = z.object({
  id: z.number(),
  schedule_candidate_id: z.number(),
  source_type: z.string(),
  source_id: z.number(),
  schedule_period_start: z.string(),
  schedule_period_end: z.string(),
  committed_start: z.string(),
  committed_end: z.string(),
  assigned_team_id: z.number(),
  assigned_personnel_id: z.number().nullable(),
  committed_by_id: z.number().nullable(),
  frozen_at: z.string().nullable(),
  estimated_labor_cost: z.number().nullable(),
  budget_threshold: z.number().nullable(),
  cost_variance_warning: z.number(),
  has_blocking_conflict: z.number(),
  nearest_feasible_window: z.string().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const ScheduleChangeLogEntrySchema = z.object({
  id: z.number(),
  commitment_id: z.number().nullable(),
  action_type: z.string(),
  actor_id: z.number().nullable(),
  field_changed: z.string().nullable(),
  old_value: z.string().nullable(),
  new_value: z.string().nullable(),
  reason_code: z.string().nullable(),
  reason_note: z.string().nullable(),
  reason: z.string().nullable(),
  details_json: z.string().nullable(),
  created_at: z.string(),
});

const ScheduleBreakInSchema = z.object({
  id: z.number(),
  schedule_commitment_id: z.number(),
  break_in_reason: z.string(),
  approved_by_user_id: z.number().nullable(),
  approved_by_personnel_id: z.number().nullable(),
  override_reason: z.string().nullable(),
  old_slot_start: z.string(),
  old_slot_end: z.string(),
  new_slot_start: z.string(),
  new_slot_end: z.string(),
  old_assignee_id: z.number().nullable(),
  new_assignee_id: z.number().nullable(),
  cost_impact_delta: z.number().nullable(),
  notification_dedupe_key: z.string().nullable(),
  row_version: z.number(),
  created_by_id: z.number().nullable(),
  created_at: z.string(),
});

const TeamCapacityLoadSchema = z.object({
  team_id: z.number(),
  work_date: z.string(),
  available_hours: z.number(),
  overtime_hours: z.number(),
  committed_hours: z.number(),
  utilization_ratio: z.number(),
});

const PlanningAssigneeLaneSchema = z.object({
  personnel_id: z.number(),
  full_name: z.string(),
  blocked_intervals_json: z.string(),
  commitments_json: z.string(),
});

const PlanningGanttSnapshotSchema = z.object({
  period_start: z.string(),
  period_end: z.string(),
  commitments: z.array(ScheduleCommitmentSchema),
  locked_windows: z.array(PlanningWindowSchema),
  capacity: z.array(TeamCapacityLoadSchema),
  assignee_lanes: z.array(PlanningAssigneeLaneSchema),
});

const NotifyTeamsResultSchema = z.object({
  emitted_count: z.number(),
  skipped_count: z.number(),
});

const ExportedBinaryDocumentSchema = z.object({
  file_name: z.string(),
  mime_type: z.string(),
  bytes: z.array(z.number().int().min(0).max(255)),
});

interface IpcErrorShape {
  code: string;
  message: string;
}

function isIpcError(err: unknown): err is IpcErrorShape {
  return typeof err === "object" && err !== null && "code" in err && "message" in err;
}

export class PlanningIpcError extends Error {
  readonly code: string;
  constructor(code: string, message: string) {
    super(message);
    this.name = "PlanningIpcError";
    this.code = code;
  }
}

function mapInvokeError(err: unknown): never {
  if (isIpcError(err)) throw new PlanningIpcError(err.code, err.message);
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
    if (err instanceof ZodError) throw new PlanningIpcError("VALIDATION_FAILED", err.message);
    mapInvokeError(err);
  }
}

export function refreshScheduleCandidates(
  input: RefreshScheduleCandidatesInput,
): Promise<RefreshScheduleCandidatesResult> {
  return invokeParsed(
    "refresh_schedule_candidates",
    { input },
    RefreshScheduleCandidatesResultSchema,
  );
}

export function getScheduleBacklogSnapshot(
  filter: ScheduleCandidateFilter,
): Promise<ScheduleBacklogSnapshot> {
  return invokeParsed("get_schedule_backlog_snapshot", { filter }, ScheduleBacklogSnapshotSchema);
}

export function listSchedulingConflicts(
  candidateId?: number | null,
  includeResolved?: boolean | null,
): Promise<SchedulingConflict[]> {
  return invokeParsed(
    "list_scheduling_conflicts",
    { candidateId: candidateId ?? null, includeResolved: includeResolved ?? null },
    z.array(SchedulingConflictSchema),
  );
}

export function listCapacityRules(filter: CapacityRuleFilter): Promise<CapacityRule[]> {
  return invokeParsed("list_capacity_rules", { filter }, z.array(CapacityRuleSchema));
}

export function createCapacityRule(input: CreateCapacityRuleInput): Promise<CapacityRule> {
  return invokeParsed("create_capacity_rule", { input }, CapacityRuleSchema);
}

export function updateCapacityRule(
  ruleId: number,
  expectedRowVersion: number,
  input: UpdateCapacityRuleInput,
): Promise<CapacityRule> {
  return invokeParsed(
    "update_capacity_rule",
    { ruleId, expectedRowVersion, input },
    CapacityRuleSchema,
  );
}

export function listPlanningWindows(filter: PlanningWindowFilter): Promise<PlanningWindow[]> {
  return invokeParsed("list_planning_windows", { filter }, z.array(PlanningWindowSchema));
}

export function createPlanningWindow(input: CreatePlanningWindowInput): Promise<PlanningWindow> {
  return invokeParsed("create_planning_window", { input }, PlanningWindowSchema);
}

export function updatePlanningWindow(
  windowId: number,
  expectedRowVersion: number,
  input: UpdatePlanningWindowInput,
): Promise<PlanningWindow> {
  return invokeParsed(
    "update_planning_window",
    { windowId, expectedRowVersion, input },
    PlanningWindowSchema,
  );
}

export function listScheduleCommitments(
  filter: ScheduleCommitmentFilter,
): Promise<ScheduleCommitment[]> {
  return invokeParsed("list_schedule_commitments", { filter }, z.array(ScheduleCommitmentSchema));
}

export function listScheduleChangeLog(
  commitmentId?: number | null,
): Promise<ScheduleChangeLogEntry[]> {
  return invokeParsed(
    "list_schedule_change_log",
    { commitmentId: commitmentId ?? null },
    z.array(ScheduleChangeLogEntrySchema),
  );
}

export function listScheduleBreakIns(filter: ScheduleBreakInFilter): Promise<ScheduleBreakIn[]> {
  return invokeParsed("list_schedule_break_ins", { filter }, z.array(ScheduleBreakInSchema));
}

export function createScheduleCommitment(
  input: CreateScheduleCommitmentInput,
): Promise<ScheduleCommitment> {
  return invokeParsed("create_schedule_commitment", { input }, ScheduleCommitmentSchema);
}

export function rescheduleScheduleCommitment(
  input: RescheduleCommitmentInput,
): Promise<ScheduleCommitment> {
  return invokeParsed("reschedule_schedule_commitment", { input }, ScheduleCommitmentSchema);
}

export function createScheduleBreakIn(input: CreateScheduleBreakInInput): Promise<ScheduleBreakIn> {
  return invokeParsed("create_schedule_break_in", { input }, ScheduleBreakInSchema);
}

export function freezeSchedulePeriod(input: FreezeSchedulePeriodInput): Promise<number> {
  return invokeParsed("freeze_schedule_period", { input }, z.number());
}

export function getPlanningGanttSnapshot(
  filter: PlanningGanttFilter,
): Promise<PlanningGanttSnapshot> {
  return invokeParsed("get_planning_gantt_snapshot", { filter }, PlanningGanttSnapshotSchema);
}

export function listTeamCapacityLoad(
  periodStart: string,
  periodEnd: string,
  teamId?: number | null,
): Promise<TeamCapacityLoad[]> {
  return invokeParsed(
    "list_team_capacity_load",
    { periodStart, periodEnd, teamId: teamId ?? null },
    z.array(TeamCapacityLoadSchema),
  );
}

export function exportPlanningGanttPdf(
  input: ExportPlanningGanttPdfInput,
): Promise<ExportedBinaryDocument> {
  return invokeParsed("export_planning_gantt_pdf", { input }, ExportedBinaryDocumentSchema);
}

export function notifyScheduleTeams(input: NotifyTeamsInput): Promise<NotifyTeamsResult> {
  return invokeParsed("notify_schedule_teams", { input }, NotifyTeamsResultSchema);
}
