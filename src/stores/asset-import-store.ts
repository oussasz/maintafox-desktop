/**
 * asset-import-store.ts
 *
 * Zustand store for the SP02-F04 asset import wizard workflow.
 * Manages file selection, batch lifecycle, validation preview,
 * apply policy, and outcome summary.
 */

import { create } from "zustand";

import {
  applyAssetImportBatch,
  createAssetImportBatch,
  getAssetImportPreview,
  listAssetImportBatches,
  validateAssetImportBatch,
} from "@/services/asset-import-service";
import { toErrorMessage } from "@/utils/errors";
import type { ApplyResult, ImportBatchSummary, ImportPreviewRow } from "@shared/ipc-types";

// ── Types ─────────────────────────────────────────────────────────────────────

export type ImportStep = "upload" | "validate" | "preview" | "apply" | "done";

interface AssetImportState {
  // ── Step tracking ───────────────────────────────────────────────────────
  step: ImportStep;

  // ── File metadata ───────────────────────────────────────────────────────
  selectedFileName: string | null;
  selectedFileSha256: string | null;

  // ── Batch ───────────────────────────────────────────────────────────────
  batchId: number | null;
  batch: ImportBatchSummary | null;

  // ── Validation preview ──────────────────────────────────────────────────
  previewRows: ImportPreviewRow[];

  // ── Apply policy ────────────────────────────────────────────────────────
  includeWarnings: boolean;
  externalSystemCode: string;

  // ── Result ──────────────────────────────────────────────────────────────
  applyResult: ApplyResult | null;

  // ── Batch history ───────────────────────────────────────────────────────
  batches: ImportBatchSummary[];

  // ── Loading / error ─────────────────────────────────────────────────────
  loading: boolean;
  saving: boolean;
  error: string | null;

  // ── Actions ─────────────────────────────────────────────────────────────
  uploadAndCreateBatch: (file: File) => Promise<void>;
  validateBatch: () => Promise<void>;
  loadPreview: () => Promise<void>;
  applyBatch: () => Promise<void>;
  loadBatches: () => Promise<void>;
  setIncludeWarnings: (value: boolean) => void;
  setExternalSystemCode: (value: string) => void;
  resetFlow: () => void;
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async function computeSha256(data: ArrayBuffer): Promise<string> {
  const hashBuffer = await crypto.subtle.digest("SHA-256", data);
  const hashArray = Array.from(new Uint8Array(hashBuffer));
  return hashArray.map((b) => b.toString(16).padStart(2, "0")).join("");
}

// ── Store ─────────────────────────────────────────────────────────────────────

const initialState = {
  step: "upload" as ImportStep,
  selectedFileName: null as string | null,
  selectedFileSha256: null as string | null,
  batchId: null as number | null,
  batch: null as ImportBatchSummary | null,
  previewRows: [] as ImportPreviewRow[],
  includeWarnings: false,
  externalSystemCode: "import",
  applyResult: null as ApplyResult | null,
  batches: [] as ImportBatchSummary[],
  loading: false,
  saving: false,
  error: null as string | null,
};

export const useAssetImportStore = create<AssetImportState>()((set, get) => ({
  ...initialState,

  uploadAndCreateBatch: async (file: File) => {
    set({ loading: true, error: null });
    try {
      const buffer = await file.arrayBuffer();
      const sha256 = await computeSha256(buffer);
      const bytes = Array.from(new Uint8Array(buffer));

      const batch = await createAssetImportBatch(file.name, sha256, bytes);

      set({
        selectedFileName: file.name,
        selectedFileSha256: sha256,
        batchId: batch.id,
        batch,
        step: "validate",
      });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  validateBatch: async () => {
    const { batchId } = get();
    if (batchId == null) return;

    set({ loading: true, error: null });
    try {
      const batch = await validateAssetImportBatch(batchId);
      set({ batch, step: "preview" });

      // Auto-load preview after validation
      const preview = await getAssetImportPreview(batchId);
      set({ previewRows: preview.rows, batch: preview.batch });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  loadPreview: async () => {
    const { batchId } = get();
    if (batchId == null) return;

    set({ loading: true, error: null });
    try {
      const preview = await getAssetImportPreview(batchId);
      set({ previewRows: preview.rows, batch: preview.batch });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  applyBatch: async () => {
    const { batchId, includeWarnings, externalSystemCode } = get();
    if (batchId == null) return;

    set({ saving: true, error: null });
    try {
      const result = await applyAssetImportBatch(batchId, {
        include_warnings: includeWarnings,
        external_system_code: externalSystemCode || null,
      });
      set({ applyResult: result, batch: result.batch, step: "done" });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ saving: false });
    }
  },

  loadBatches: async () => {
    set({ loading: true, error: null });
    try {
      const batches = await listAssetImportBatches();
      set({ batches });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  setIncludeWarnings: (value: boolean) => set({ includeWarnings: value }),
  setExternalSystemCode: (value: string) => set({ externalSystemCode: value }),

  resetFlow: () => set({ ...initialState }),
}));
