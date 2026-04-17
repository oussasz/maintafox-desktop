import { create } from "zustand";

import {
  createCapacityRule,
  createPlanningWindow,
  createScheduleBreakIn,
  createScheduleCommitment,
  exportPlanningGanttPdf,
  freezeSchedulePeriod,
  getPlanningGanttSnapshot,
  getScheduleBacklogSnapshot,
  listCapacityRules,
  listPlanningWindows,
  listScheduleBreakIns,
  listScheduleChangeLog,
  listScheduleCommitments,
  listTeamCapacityLoad,
  notifyScheduleTeams,
  refreshScheduleCandidates,
  rescheduleScheduleCommitment,
} from "@/services/planning-service";
import { toErrorMessage } from "@/utils/errors";
import type {
  CapacityRule,
  CapacityRuleFilter,
  CreateCapacityRuleInput,
  CreatePlanningWindowInput,
  CreateScheduleBreakInInput,
  CreateScheduleCommitmentInput,
  FreezeSchedulePeriodInput,
  NotifyTeamsInput,
  NotifyTeamsResult,
  PlanningGanttFilter,
  PlanningGanttSnapshot,
  ScheduleBreakIn,
  ScheduleBreakInFilter,
  PlanningWindow,
  PlanningWindowFilter,
  RescheduleCommitmentInput,
  ScheduleBacklogSnapshot,
  ScheduleChangeLogEntry,
  ScheduleCommitment,
  ScheduleCommitmentFilter,
  TeamCapacityLoad,
} from "@shared/ipc-types";

interface PlanningStoreState {
  backlog: ScheduleBacklogSnapshot | null;
  commitments: ScheduleCommitment[];
  capacityRules: CapacityRule[];
  planningWindows: PlanningWindow[];
  breakIns: ScheduleBreakIn[];
  capacityLoad: TeamCapacityLoad[];
  ganttSnapshot: PlanningGanttSnapshot | null;
  changeLog: ScheduleChangeLogEntry[];
  loading: boolean;
  saving: boolean;
  error: string | null;
  loadBacklog: () => Promise<void>;
  refreshBacklog: () => Promise<void>;
  loadCommitments: (filter: ScheduleCommitmentFilter) => Promise<void>;
  loadCapacityRules: (filter?: CapacityRuleFilter) => Promise<void>;
  loadPlanningWindows: (filter?: PlanningWindowFilter) => Promise<void>;
  loadCapacityLoad: (periodStart: string, periodEnd: string, teamId?: number | null) => Promise<void>;
  loadGantt: (filter: PlanningGanttFilter) => Promise<void>;
  loadChangeLog: (commitmentId?: number | null) => Promise<void>;
  loadBreakIns: (filter?: ScheduleBreakInFilter) => Promise<void>;
  createCapacityRule: (input: CreateCapacityRuleInput) => Promise<CapacityRule>;
  createPlanningWindow: (input: CreatePlanningWindowInput) => Promise<PlanningWindow>;
  createCommitment: (input: CreateScheduleCommitmentInput) => Promise<ScheduleCommitment>;
  rescheduleCommitment: (input: RescheduleCommitmentInput) => Promise<ScheduleCommitment>;
  createBreakIn: (input: CreateScheduleBreakInInput) => Promise<ScheduleBreakIn>;
  freezeSchedule: (input: FreezeSchedulePeriodInput) => Promise<number>;
  notifyTeams: (input: NotifyTeamsInput) => Promise<NotifyTeamsResult>;
  exportGanttPdf: (periodStart: string, periodEnd: string, teamId?: number | null) => Promise<void>;
}

function downloadBinary(fileName: string, mimeType: string, bytes: number[]) {
  const blob = new Blob([new Uint8Array(bytes)], { type: mimeType });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = fileName;
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
  URL.revokeObjectURL(url);
}

export const usePlanningStore = create<PlanningStoreState>()((set, get) => ({
  backlog: null,
  commitments: [],
  capacityRules: [],
  planningWindows: [],
  breakIns: [],
  capacityLoad: [],
  ganttSnapshot: null,
  changeLog: [],
  loading: false,
  saving: false,
  error: null,

  loadBacklog: async () => {
    set({ loading: true, error: null });
    try {
      const backlog = await getScheduleBacklogSnapshot({ limit: 300 });
      set({ backlog });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  refreshBacklog: async () => {
    set({ saving: true, error: null });
    try {
      await refreshScheduleCandidates({
        include_work_orders: true,
        include_pm_occurrences: true,
        include_approved_di: true,
        limit_per_source: 300,
      });
      await get().loadBacklog();
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  loadCommitments: async (filter) => {
    set({ loading: true, error: null });
    try {
      const commitments = await listScheduleCommitments(filter);
      set({ commitments });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  loadCapacityRules: async (filter = {}) => {
    set({ loading: true, error: null });
    try {
      const capacityRules = await listCapacityRules(filter);
      set({ capacityRules });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  loadPlanningWindows: async (filter = {}) => {
    set({ loading: true, error: null });
    try {
      const planningWindows = await listPlanningWindows(filter);
      set({ planningWindows });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  loadCapacityLoad: async (periodStart, periodEnd, teamId) => {
    set({ loading: true, error: null });
    try {
      const capacityLoad = await listTeamCapacityLoad(periodStart, periodEnd, teamId);
      set({ capacityLoad });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  loadGantt: async (filter) => {
    set({ loading: true, error: null });
    try {
      const ganttSnapshot = await getPlanningGanttSnapshot(filter);
      set({ ganttSnapshot, commitments: ganttSnapshot.commitments, capacityLoad: ganttSnapshot.capacity });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  loadChangeLog: async (commitmentId) => {
    set({ loading: true, error: null });
    try {
      const changeLog = await listScheduleChangeLog(commitmentId);
      set({ changeLog });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  loadBreakIns: async (filter = {}) => {
    set({ loading: true, error: null });
    try {
      const breakIns = await listScheduleBreakIns(filter);
      set({ breakIns });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  createCapacityRule: async (input) => {
    set({ saving: true, error: null });
    try {
      const created = await createCapacityRule(input);
      await get().loadCapacityRules({});
      return created;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  createPlanningWindow: async (input) => {
    set({ saving: true, error: null });
    try {
      const created = await createPlanningWindow(input);
      await get().loadPlanningWindows({});
      return created;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  createCommitment: async (input) => {
    set({ saving: true, error: null });
    try {
      const created = await createScheduleCommitment(input);
      return created;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  rescheduleCommitment: async (input) => {
    set({ saving: true, error: null });
    try {
      const updated = await rescheduleScheduleCommitment(input);
      return updated;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  createBreakIn: async (input) => {
    set({ saving: true, error: null });
    try {
      const created = await createScheduleBreakIn(input);
      return created;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  freezeSchedule: async (input) => {
    set({ saving: true, error: null });
    try {
      const count = await freezeSchedulePeriod(input);
      return count;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  notifyTeams: async (input) => {
    set({ saving: true, error: null });
    try {
      return await notifyScheduleTeams(input);
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  exportGanttPdf: async (periodStart, periodEnd, teamId) => {
    set({ saving: true, error: null });
    try {
      const doc = await exportPlanningGanttPdf({
        period_start: periodStart,
        period_end: periodEnd,
        team_id: teamId ?? null,
        paper_size: "A3",
      });
      downloadBinary(doc.file_name, doc.mime_type, doc.bytes);
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },
}));

