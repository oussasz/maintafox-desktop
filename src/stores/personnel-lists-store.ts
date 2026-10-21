/**
 * The plant's personnel lists (Données de référence): employment statuses and types, absence
 * types (with their behaviour tag) and position categories, shared by every personnel screen.
 * Read again at once when a list changes in Données de référence in this app, and when a screen
 * opens on a copy older than `REFERENCE_LIST_MAX_AGE_MS` (changes received by sync).
 */
import { create } from "zustand";

import { findListOption, listOptionName, listOptionTexts } from "@/lib/list-option-text";
import { REFERENCE_LIST_MAX_AGE_MS, onReferenceDataChanged } from "@/lib/reference-data-events";
import { listPersonnelReferenceOptions } from "@/services/personnel-service";
import type { PersonnelOption, PersonnelReferenceOptions } from "@shared/ipc-types";

export type PersonnelListName = keyof PersonnelReferenceOptions;

interface PersonnelListsState {
  options: PersonnelReferenceOptions | null;
  /** When `options` was read (ms since epoch). */
  loadedAt: number;
  loading: boolean;
  error: unknown;
  /** Reads the lists unless a recent copy is kept (or `force`). */
  load: (force?: boolean) => Promise<void>;
}

/** Set when a forced read is asked while another read is under way. */
let readAgain = false;

export const usePersonnelListsStore = create<PersonnelListsState>((set, get) => ({
  options: null,
  loadedAt: 0,
  loading: false,
  error: null,
  load: async (force = false) => {
    const { loading, options, loadedAt } = get();
    if (loading) {
      // A change while a read is under way: read again once it ends (it may predate the change).
      if (force) readAgain = true;
      return;
    }
    if (options && !force && Date.now() - loadedAt < REFERENCE_LIST_MAX_AGE_MS) return;
    readAgain = false;
    set({ loading: true, error: null });
    try {
      set({ options: await listPersonnelReferenceOptions(), loadedAt: Date.now(), loading: false });
    } catch (err) {
      set({ error: err, loading: false });
    }
    if (readAgain) void get().load(true);
  },
}));

// A list edited or published in Données de référence: read the kept copy again at once.
onReferenceDataChanged(() => {
  if (usePersonnelListsStore.getState().options) void usePersonnelListsStore.getState().load(true);
});

/** Name of a list value in the interface language. */
export const personnelOptionName: (option: PersonnelOption, language: string) => string =
  listOptionName;

/** Display text of each value of a list (name, plus the exact code when names look alike). */
export const personnelOptionTexts: (
  options: readonly PersonnelOption[],
  language: string,
) => Map<string, string> = listOptionTexts;

/** The value with exactly this code (codes are matched exactly, never case-insensitively). */
export const findPersonnelOption: (
  options: readonly PersonnelOption[] | undefined,
  code: string | null | undefined,
) => PersonnelOption | null = findListOption;
