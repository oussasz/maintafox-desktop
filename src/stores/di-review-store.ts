/**
 * di-review-store.ts
 *
 * Zustand store for DI review queue and triage actions.
 * Phase 2 – Sub-phase 04 – File 02 – Sprint S3.
 */

import { create } from "zustand";

import {
  archiveDi,
  approveDi,
  cancelOwnDi,
  closeDi,
  closeDiAsNonExecutable,
  deferDi,
  getDiReviewEvents,
  reactivateDi,
  returnDi,
  screenDi,
} from "@/services/di-review-service";
import { getDi, listDis } from "@/services/di-service";
import { toErrorMessage } from "@/utils/errors";
import type {
  DiApproveInput,
  DiCancelOwnInput,
  DiCloseInput,
  DiDeferInput,
  DiReactivateInput,
  DiRejectInput,
  DiReturnInput,
  DiReviewEvent,
  DiScreenInput,
  DiSummaryRow,
  InterventionRequest,
} from "@shared/ipc-types";

export interface ApproveResult {
  approved: boolean;
  converted: boolean;
  woId: number | null;
  woCode: string | null;
  conversionError: string | null;
  approvedRowVersion: number;
}

interface DiReviewStoreState {
  // Review queue
  reviewQueue: InterventionRequest[];
  // Active review context
  activeReviewDi: InterventionRequest | null;
  reviewEvents: DiReviewEvent[];
  similarDis: DiSummaryRow[];
  // Dialog state
  approvalDi: InterventionRequest | null;
  rejectionDi: InterventionRequest | null;
  returnDi_: InterventionRequest | null;
  // Flags
  saving: boolean;
  error: string | null;

  loadReviewQueue: () => Promise<void>;
  openForReview: (id: number) => Promise<void>;
  openApproval: (di: InterventionRequest) => void;
  closeApproval: () => void;
  openRejection: (di: InterventionRequest) => void;
  closeRejection: () => void;
  openReturn: (di: InterventionRequest) => void;
  closeReturn: () => void;
  screen: (input: DiScreenInput) => Promise<InterventionRequest>;
  returnForClarification: (input: DiReturnInput) => Promise<void>;
  reject: (input: DiRejectInput) => Promise<void>;
  closeWithDisposition: (input: DiCloseInput) => Promise<void>;
  cancelOwn: (input: DiCancelOwnInput) => Promise<void>;
  approve: (input: DiApproveInput) => Promise<ApproveResult>;
  defer: (input: DiDeferInput) => Promise<void>;
  reactivate: (input: DiReactivateInput) => Promise<void>;
  closeAsNonExecutable: (
    diId: number,
    expectedRowVersion: number,
    notes?: string | null,
  ) => Promise<void>;
  archive: (diId: number, expectedRowVersion: number, notes?: string | null) => Promise<void>;
}

export const useDiReviewStore = create<DiReviewStoreState>()((set, get) => ({
  reviewQueue: [],
  activeReviewDi: null,
  reviewEvents: [],
  similarDis: [],
  approvalDi: null,
  rejectionDi: null,
  returnDi_: null,
  saving: false,
  error: null,

  loadReviewQueue: async () => {
    set({ error: null });
    try {
      const page = await listDis({
        status: ["in_review", "returned_for_clarification", "awaiting_approval"],
        limit: 200,
        offset: 0,
      });
      set({ reviewQueue: page.items });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    }
  },

  openForReview: async (id) => {
    set({ error: null, activeReviewDi: null, reviewEvents: [], similarDis: [] });
    try {
      const [detail, events] = await Promise.all([getDi(id), getDiReviewEvents(id)]);
      set({
        activeReviewDi: detail.di,
        reviewEvents: events,
        similarDis: detail.similar,
      });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    }
  },

  openApproval: (di) => set({ approvalDi: di }),
  closeApproval: () => set({ approvalDi: null }),
  openRejection: (di) => set({ rejectionDi: di }),
  closeRejection: () => set({ rejectionDi: null }),
  openReturn: (di) => set({ returnDi_: di }),
  closeReturn: () => set({ returnDi_: null }),

  screen: async (input) => {
    set({ saving: true, error: null });
    try {
      const updated = await screenDi(input);
      set({ activeReviewDi: updated });
      await get().loadReviewQueue();
      return updated;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  returnForClarification: async (input) => {
    set({ saving: true, error: null });
    try {
      const updated = await returnDi(input);
      set({ activeReviewDi: updated });
      await get().loadReviewQueue();
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  reject: async (input) => {
    const notes = [input.reason_code.trim(), input.notes?.trim()].filter(Boolean).join(": ");
    await get().closeWithDisposition({
      di_id: input.di_id,
      actor_id: input.actor_id,
      expected_row_version: input.expected_row_version,
      disposition_code: "rejected_invalid",
      notes: notes || null,
      related_di_id: null,
    });
  },

  closeWithDisposition: async (input) => {
    set({ saving: true, error: null });
    try {
      const updated = await closeDi(input);
      set({ activeReviewDi: updated });
      await get().loadReviewQueue();
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  cancelOwn: async (input) => {
    set({ saving: true, error: null });
    try {
      const updated = await cancelOwnDi(input);
      set({ activeReviewDi: updated });
      await get().loadReviewQueue();
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  approve: async (input) => {
    set({ saving: true, error: null });
    try {
      const updated = await approveDi(input);
      set((s) => ({
        activeReviewDi: updated,
        approvalDi: s.approvalDi?.id === updated.id ? updated : s.approvalDi,
      }));
      await get().loadReviewQueue();
      return {
        approved: true,
        converted: false,
        woId: null,
        woCode: null,
        conversionError: null,
        approvedRowVersion: updated.row_version,
      };
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  defer: async (input) => {
    set({ saving: true, error: null });
    try {
      const updated = await deferDi(input);
      set({ activeReviewDi: updated });
      await get().loadReviewQueue();
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  reactivate: async (input) => {
    set({ saving: true, error: null });
    try {
      const updated = await reactivateDi(input);
      set({ activeReviewDi: updated });
      await get().loadReviewQueue();
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  closeAsNonExecutable: async (diId, expectedRowVersion, notes) => {
    set({ saving: true, error: null });
    try {
      const updated = await closeDiAsNonExecutable({
        di_id: diId,
        actor_id: 0,
        expected_row_version: expectedRowVersion,
        notes: notes ?? null,
      });
      set({ activeReviewDi: updated });
      await get().loadReviewQueue();
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  archive: async (diId, expectedRowVersion, notes) => {
    set({ saving: true, error: null });
    try {
      const updated = await archiveDi({
        di_id: diId,
        actor_id: 0,
        expected_row_version: expectedRowVersion,
        notes: notes ?? null,
      });
      set({ activeReviewDi: updated });
      await get().loadReviewQueue();
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },
}));
