/**
 * personnel-store.ts — Zustand store for Personnel workspace (PRD §6.6).
 */

import { create } from "zustand";

import { appErrorText } from "@/lib/app-error";
import {
  createPersonnel,
  deactivatePersonnel,
  getPersonnel,
  listPersonnel,
  updatePersonnel,
} from "@/services/personnel-service";
import { toErrorMessage } from "@/utils/errors";
import type {
  Personnel,
  PersonnelCreateInput,
  PersonnelDetailPayload,
  PersonnelListFilter,
  PersonnelUpdateInput,
} from "@shared/ipc-types";

const DEFAULT_FILTER: PersonnelListFilter = { limit: 50, offset: 0 };

interface PersonnelStoreState {
  items: Personnel[];
  total: number;
  activePersonnel: PersonnelDetailPayload | null;
  filter: PersonnelListFilter;
  loading: boolean;
  saving: boolean;
  error: string | null;
  /** Failure of the last list load (raw, for ErrorState): never shown as an empty directory. */
  loadError: unknown;
  /** Failure of the last detail load (raw, for ErrorState in the detail pane). */
  detailError: unknown;
  /** A next page is being appended. */
  loadingMore: boolean;
  showCreateForm: boolean;
  editingPersonnel: Personnel | null;

  setFilter: (partial: Partial<PersonnelListFilter>) => void;
  loadPersonnel: () => Promise<void>;
  /** Appends the next page of the current filter (the list states how many of `total` it shows). */
  loadMore: () => Promise<void>;
  openPersonnel: (id: number) => Promise<void>;
  closePersonnel: () => void;
  submitNewPersonnel: (input: PersonnelCreateInput) => Promise<Personnel>;
  updateExisting: (input: PersonnelUpdateInput) => Promise<void>;
  deactivate: (id: number, rowVersion: number) => Promise<void>;
  openCreateForm: (personnel?: Personnel) => void;
  closeCreateForm: () => void;
}

export const usePersonnelStore = create<PersonnelStoreState>()((set, get) => ({
  items: [],
  total: 0,
  activePersonnel: null,
  filter: { ...DEFAULT_FILTER },
  loading: false,
  saving: false,
  error: null,
  loadError: null,
  detailError: null,
  loadingMore: false,
  showCreateForm: false,
  editingPersonnel: null,

  setFilter: (partial) => {
    set((s) => ({
      filter: { ...s.filter, ...partial, offset: 0 },
    }));
  },

  loadPersonnel: async () => {
    set({ loading: true, error: null, loadError: null });
    try {
      const page = await listPersonnel({ ...get().filter, offset: 0 });
      set({ items: page.items, total: page.total });
    } catch (err) {
      set({ error: toErrorMessage(err), loadError: err });
    } finally {
      set({ loading: false });
    }
  },

  loadMore: async () => {
    const { items, total, loadingMore } = get();
    if (loadingMore || items.length >= total) return;
    set({ loadingMore: true, loadError: null });
    try {
      const page = await listPersonnel({ ...get().filter, offset: items.length });
      set((s) => {
        const seen = new Set(s.items.map((p) => p.id));
        return {
          items: [...s.items, ...page.items.filter((p) => !seen.has(p.id))],
          total: page.total,
        };
      });
    } catch (err) {
      set({ loadError: err });
    } finally {
      set({ loadingMore: false });
    }
  },

  openPersonnel: async (id) => {
    set({ error: null, detailError: null });
    try {
      const detail = await getPersonnel(id);
      set({ activePersonnel: detail });
    } catch (err) {
      set({ error: toErrorMessage(err), detailError: err });
    }
  },

  closePersonnel: () => {
    set({ activePersonnel: null });
  },

  submitNewPersonnel: async (input) => {
    set({ saving: true, error: null });
    try {
      const created = await createPersonnel(input);
      await get().loadPersonnel();
      set({ showCreateForm: false, editingPersonnel: null });
      return created;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  updateExisting: async (input) => {
    set({ saving: true, error: null });
    try {
      await updatePersonnel(input);
      const active = get().activePersonnel;
      if (active?.personnel.id === input.id) {
        const detail = await getPersonnel(input.id);
        set({ activePersonnel: detail });
      }
      await get().loadPersonnel();
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  deactivate: async (id, rowVersion) => {
    set({ saving: true, error: null });
    try {
      await deactivatePersonnel(id, rowVersion);
      await get().loadPersonnel();
      const active = get().activePersonnel;
      if (active?.personnel.id === id) {
        const detail = await getPersonnel(id);
        set({ activePersonnel: detail });
      }
    } catch (err) {
      // A refusal (open work orders) is coded: shown in the user's language.
      set({ error: appErrorText(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  openCreateForm: (personnel) => {
    set({ showCreateForm: true, editingPersonnel: personnel ?? null });
  },

  closeCreateForm: () => {
    set({ showCreateForm: false, editingPersonnel: null });
  },
}));
