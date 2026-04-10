/**
 * di-review-store.ts
 *
 * Zustand store for DI review queue and triage actions.
 * Phase 2 – Sub-phase 04 – File 02 – Sprint S3.
 */

import { create } from "zustand";

import { convertDiToWo } from "@/services/di-conversion-service";
import {
  approveDi,
  deferDi,
  getDiReviewEvents,
  reactivateDi,
  rejectDi,
  returnDi,
  screenDi,
} from "@/services/di-review-service";
import { getDi, listDis } from "@/services/di-service";
import { toErrorMessage } from "@/utils/errors";
import type {
  DiApproveInput,
  DiDeferInput,
  DiReactivateInput,
  DiRejectInput,
  DiReturnInput,
  DiReviewEvent,
  DiScreenInput,
  DiSummaryRow,
  InterventionRequest,
} from "@shared/ipc-types";

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
  approve: (input: DiApproveInput) => Promise<void>;
  defer: (input: DiDeferInput) => Promise<void>;
  reactivate: (input: DiReactivateInput) => Promise<void>;
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
        status: ["pending_review", "returned_for_clarification", "awaiting_approval"],
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
    set({ saving: true, error: null });
    try {
      const updated = await rejectDi(input);
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
      // Chain conversion: create WO from the approved DI
      try {
        await convertDiToWo({
          diId: input.di_id,
          expectedRowVersion: updated.row_version,
          conversionNotes: input.notes ?? "",
        });
        // Re-fetch the DI to get the post-conversion state
        try {
          const fresh = await getDi(input.di_id);
          set({ activeReviewDi: fresh.di });
        } catch {
          set({ activeReviewDi: updated });
        }
      } catch {
        // Conversion may fail (missing permission, etc.); approval still succeeded
        set({ activeReviewDi: updated });
      }
      await get().loadReviewQueue();
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
}));
