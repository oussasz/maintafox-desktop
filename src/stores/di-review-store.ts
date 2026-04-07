/**
 * di-review-store.ts
 *
 * Zustand store for DI review queue and triage actions.
 * Phase 2 – Sub-phase 04 – File 02 – Sprint S3.
 */

import { create } from "zustand";

import {
  screenDi,
  returnDi,
  rejectDi,
  approveDi,
  deferDi,
  reactivateDi,
  getDiReviewEvents,
} from "@/services/di-review-service";
import { listDis, getDi } from "@/services/di-service";
import { toErrorMessage } from "@/utils/errors";
import type {
  InterventionRequest,
  DiScreenInput,
  DiReturnInput,
  DiRejectInput,
  DiApproveInput,
  DiDeferInput,
  DiReactivateInput,
  DiReviewEvent,
  DiSummaryRow,
} from "@shared/ipc-types";

interface DiReviewStoreState {
  // Review queue
  reviewQueue: InterventionRequest[];
  // Active review context
  activeReviewDi: InterventionRequest | null;
  reviewEvents: DiReviewEvent[];
  similarDis: DiSummaryRow[];
  // Flags
  saving: boolean;
  error: string | null;

  loadReviewQueue: () => Promise<void>;
  openForReview: (id: number) => Promise<void>;
  screen: (input: DiScreenInput) => Promise<void>;
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
  saving: false,
  error: null,

  loadReviewQueue: async () => {
    set({ error: null });
    try {
      const page = await listDis({
        status: ["pending_review", "returned_for_clarification"],
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

  screen: async (input) => {
    set({ saving: true, error: null });
    try {
      const updated = await screenDi(input);
      set({ activeReviewDi: updated });
      await get().loadReviewQueue();
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
      set({ activeReviewDi: updated });
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
