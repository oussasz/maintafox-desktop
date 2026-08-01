/**
 * personnel-store.ts — Zustand store for Personnel workspace (PRD §6.6).
 */

import { create } from "zustand";

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
  showCreateForm: boolean;
  editingPersonnel: Personnel | null;

  setFilter: (partial: Partial<PersonnelListFilter>) => void;
  loadPersonnel: () => Promise<void>;
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
  showCreateForm: false,
  editingPersonnel: null,

  setFilter: (partial) => {
    set((s) => ({
      filter: { ...s.filter, ...partial, offset: 0 },
    }));
  },

  loadPersonnel: async () => {
    set({ loading: true, error: null });
    try {
      const page = await listPersonnel(get().filter);
      set({ items: page.items, total: page.total });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  openPersonnel: async (id) => {
    set({ loading: true, error: null });
    try {
      const detail = await getPersonnel(id);
      set({ activePersonnel: detail });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
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
      set({ error: toErrorMessage(err) });
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
