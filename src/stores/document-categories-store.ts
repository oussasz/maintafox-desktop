/**
 * The plant's document categories (Données de référence, `DOCUMENT.CATEGORIES`), shared by the
 * document library and the asset documents tab. Read again at once when a list changes in
 * Données de référence in this app, and when a screen opens on a copy older than
 * `REFERENCE_LIST_MAX_AGE_MS` (changes received by sync).
 */
import { create } from "zustand";

import { REFERENCE_LIST_MAX_AGE_MS, onReferenceDataChanged } from "@/lib/reference-data-events";
import { listDocumentCategories } from "@/services/documentation-service";
import type { DocumentCategoryOption } from "@shared/ipc-types";

interface DocumentCategoriesState {
  categories: DocumentCategoryOption[] | null;
  /** When `categories` was read (ms since epoch). */
  loadedAt: number;
  loading: boolean;
  error: unknown;
  /** Reads the categories unless a recent copy is kept (or `force`). */
  load: (force?: boolean) => Promise<void>;
}

/** Set when a forced read is asked while another read is under way. */
let readAgain = false;

export const useDocumentCategoriesStore = create<DocumentCategoriesState>((set, get) => ({
  categories: null,
  loadedAt: 0,
  loading: false,
  error: null,
  load: async (force = false) => {
    const { loading, categories, loadedAt } = get();
    if (loading) {
      // A change while a read is under way: read again once it ends (it may predate the change).
      if (force) readAgain = true;
      return;
    }
    if (categories && !force && Date.now() - loadedAt < REFERENCE_LIST_MAX_AGE_MS) return;
    readAgain = false;
    set({ loading: true, error: null });
    try {
      set({ categories: await listDocumentCategories(), loadedAt: Date.now(), loading: false });
    } catch (err) {
      set({ error: err, loading: false });
    }
    if (readAgain) void get().load(true);
  },
}));

// A list edited or published in Données de référence: read the kept copy again at once.
onReferenceDataChanged(() => {
  const state = useDocumentCategoriesStore.getState();
  if (state.categories) void state.load(true);
});
