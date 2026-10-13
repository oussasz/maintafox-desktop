/**
 * asset-import-store — behaviour tests for the asset import wizard.
 *
 * Upload (hash + bytes), validate (+ auto preview), preview, apply policy,
 * batch history, setters and resetFlow.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  applyAssetImportBatch,
  createAssetImportBatch,
  getAssetImportPreview,
  listAssetImportBatches,
  validateAssetImportBatch,
} from "@/services/asset-import-service";
import type {
  ApplyResult,
  ImportBatchSummary,
  ImportPreview,
  ImportPreviewRow,
} from "@shared/ipc-types";

import { useAssetImportStore } from "../asset-import-store";

vi.mock("@/services/asset-import-service", () => ({
  applyAssetImportBatch: vi.fn(),
  createAssetImportBatch: vi.fn(),
  getAssetImportPreview: vi.fn(),
  listAssetImportBatches: vi.fn(),
  validateAssetImportBatch: vi.fn(),
}));

const mockApply = vi.mocked(applyAssetImportBatch);
const mockCreate = vi.mocked(createAssetImportBatch);
const mockPreview = vi.mocked(getAssetImportPreview);
const mockList = vi.mocked(listAssetImportBatches);
const mockValidate = vi.mocked(validateAssetImportBatch);

// SHA-256("abc")
const ABC_SHA256 = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

// ── Fixtures ──────────────────────────────────────────────────────────────────

function makeBatch(overrides: Partial<ImportBatchSummary> = {}): ImportBatchSummary {
  return {
    id: 11,
    source_filename: "assets.csv",
    source_sha256: ABC_SHA256,
    initiated_by_id: 1,
    status: "uploaded",
    total_rows: 3,
    valid_rows: 0,
    warning_rows: 0,
    error_rows: 0,
    created_at: "2026-10-01T10:00:00Z",
    updated_at: "2026-10-01T10:00:00Z",
    ...overrides,
  };
}

function makeRow(overrides: Partial<ImportPreviewRow> = {}): ImportPreviewRow {
  return {
    id: 1,
    row_no: 1,
    normalized_asset_code: "P-101",
    normalized_external_key: null,
    validation_status: "valid",
    validation_messages: [],
    proposed_action: "create",
    raw_json: '{"code":"P-101"}',
    ...overrides,
  };
}

function makePreview(overrides: Partial<ImportPreview> = {}): ImportPreview {
  return {
    batch: makeBatch({ status: "validated", valid_rows: 2, warning_rows: 1 }),
    rows: [
      makeRow(),
      makeRow({
        id: 2,
        row_no: 2,
        validation_status: "warning",
        validation_messages: [{ category: "field", severity: "warning", message: "missing x" }],
      }),
    ],
    ...overrides,
  };
}

function makeApplyResult(overrides: Partial<ApplyResult> = {}): ApplyResult {
  return {
    batch: makeBatch({ status: "applied" }),
    created: 2,
    updated: 0,
    skipped: 1,
    errored: 0,
    ...overrides,
  };
}

function deferred<T>() {
  let resolve: (value: T) => void = () => undefined;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

function makeFile(content = "abc", name = "assets.csv"): File {
  const bytes = new TextEncoder().encode(content);
  // jsdom's File may not implement arrayBuffer(); provide a deterministic one.
  const file = new File([bytes], name, { type: "text/csv" });
  Object.defineProperty(file, "arrayBuffer", {
    value: () => Promise.resolve(bytes.slice().buffer),
  });
  return file;
}

const initial = useAssetImportStore.getState();

beforeEach(() => {
  vi.clearAllMocks();
  mockApply.mockReset();
  mockCreate.mockReset();
  mockPreview.mockReset();
  mockList.mockReset();
  mockValidate.mockReset();
  useAssetImportStore.setState(initial, true);
});

// ── Tests ─────────────────────────────────────────────────────────────────────

describe("asset-import-store", () => {
  it("starts at the upload step with default policy", () => {
    const s = useAssetImportStore.getState();
    expect(s.step).toBe("upload");
    expect(s.batchId).toBeNull();
    expect(s.batch).toBeNull();
    expect(s.previewRows).toEqual([]);
    expect(s.includeWarnings).toBe(false);
    expect(s.externalSystemCode).toBe("import");
    expect(s.applyResult).toBeNull();
    expect(s.batches).toEqual([]);
    expect(s.loading).toBe(false);
    expect(s.saving).toBe(false);
    expect(s.error).toBeNull();
  });

  describe("uploadAndCreateBatch", () => {
    it("hashes the file, sends name/sha256/bytes and advances to validate", async () => {
      const batch = makeBatch();
      mockCreate.mockResolvedValueOnce(batch);

      await useAssetImportStore.getState().uploadAndCreateBatch(makeFile("abc", "assets.csv"));

      expect(mockCreate).toHaveBeenCalledWith("assets.csv", ABC_SHA256, [97, 98, 99]);
      const s = useAssetImportStore.getState();
      expect(s.selectedFileName).toBe("assets.csv");
      expect(s.selectedFileSha256).toBe(ABC_SHA256);
      expect(s.batchId).toBe(11);
      expect(s.batch).toEqual(batch);
      expect(s.step).toBe("validate");
      expect(s.loading).toBe(false);
      expect(s.error).toBeNull();
    });

    it("sets loading during the upload", async () => {
      const d = deferred<ImportBatchSummary>();
      mockCreate.mockReturnValueOnce(d.promise);
      useAssetImportStore.setState({ error: "old" });

      const p = useAssetImportStore.getState().uploadAndCreateBatch(makeFile());
      expect(useAssetImportStore.getState().loading).toBe(true);
      expect(useAssetImportStore.getState().error).toBeNull();

      await vi.waitFor(() => {
        expect(mockCreate).toHaveBeenCalled();
      });
      d.resolve(makeBatch());
      await p;
      expect(useAssetImportStore.getState().loading).toBe(false);
    });

    it("on failure sets error, stays on upload and records nothing", async () => {
      mockCreate.mockRejectedValueOnce({ code: "VALIDATION", message: "duplicate file" });

      await expect(
        useAssetImportStore.getState().uploadAndCreateBatch(makeFile()),
      ).resolves.toBeUndefined();

      const s = useAssetImportStore.getState();
      expect(s.error).toBe("duplicate file");
      expect(s.step).toBe("upload");
      expect(s.batchId).toBeNull();
      expect(s.selectedFileName).toBeNull();
      expect(s.loading).toBe(false);
    });
  });

  describe("validateBatch", () => {
    it("is a no-op when there is no batch", async () => {
      await useAssetImportStore.getState().validateBatch();
      expect(mockValidate).not.toHaveBeenCalled();
      expect(useAssetImportStore.getState().loading).toBe(false);
    });

    it("validates, moves to preview and auto-loads the preview rows", async () => {
      useAssetImportStore.setState({ batchId: 11, step: "validate" });
      mockValidate.mockResolvedValueOnce(makeBatch({ status: "validated" }));
      const preview = makePreview();
      mockPreview.mockResolvedValueOnce(preview);

      await useAssetImportStore.getState().validateBatch();

      expect(mockValidate).toHaveBeenCalledWith(11);
      expect(mockPreview).toHaveBeenCalledWith(11);
      const s = useAssetImportStore.getState();
      expect(s.step).toBe("preview");
      expect(s.previewRows).toEqual(preview.rows);
      expect(s.batch).toEqual(preview.batch);
      expect(s.loading).toBe(false);
    });

    it("on validation failure sets error and does not load the preview", async () => {
      useAssetImportStore.setState({ batchId: 11, step: "validate" });
      mockValidate.mockRejectedValueOnce(new Error("bad csv"));

      await useAssetImportStore.getState().validateBatch();

      const s = useAssetImportStore.getState();
      expect(s.error).toBe("bad csv");
      expect(s.step).toBe("validate");
      expect(mockPreview).not.toHaveBeenCalled();
      expect(s.loading).toBe(false);
    });

    it("if the auto preview fails the step is already preview and error is set", async () => {
      useAssetImportStore.setState({ batchId: 11, step: "validate" });
      const validated = makeBatch({ status: "validated" });
      mockValidate.mockResolvedValueOnce(validated);
      mockPreview.mockRejectedValueOnce({ message: "preview failed" });

      await useAssetImportStore.getState().validateBatch();

      const s = useAssetImportStore.getState();
      expect(s.step).toBe("preview");
      expect(s.batch).toEqual(validated);
      expect(s.previewRows).toEqual([]);
      expect(s.error).toBe("preview failed");
    });
  });

  describe("loadPreview", () => {
    it("is a no-op without a batch", async () => {
      await useAssetImportStore.getState().loadPreview();
      expect(mockPreview).not.toHaveBeenCalled();
    });

    it("loads preview rows and batch", async () => {
      useAssetImportStore.setState({ batchId: 11 });
      const preview = makePreview();
      const d = deferred<ImportPreview>();
      mockPreview.mockReturnValueOnce(d.promise);

      const p = useAssetImportStore.getState().loadPreview();
      expect(useAssetImportStore.getState().loading).toBe(true);
      d.resolve(preview);
      await p;

      expect(mockPreview).toHaveBeenCalledWith(11);
      expect(useAssetImportStore.getState().previewRows).toEqual(preview.rows);
      expect(useAssetImportStore.getState().batch).toEqual(preview.batch);
      expect(useAssetImportStore.getState().loading).toBe(false);
    });

    it("sets error on failure", async () => {
      useAssetImportStore.setState({ batchId: 11 });
      mockPreview.mockRejectedValueOnce("gone");
      await useAssetImportStore.getState().loadPreview();
      expect(useAssetImportStore.getState().error).toBe("gone");
      expect(useAssetImportStore.getState().loading).toBe(false);
    });
  });

  describe("applyBatch", () => {
    it("is a no-op without a batch", async () => {
      await useAssetImportStore.getState().applyBatch();
      expect(mockApply).not.toHaveBeenCalled();
    });

    it("sends the policy from state, stores the result and moves to done", async () => {
      useAssetImportStore.setState({ batchId: 11, step: "preview" });
      useAssetImportStore.getState().setIncludeWarnings(true);
      useAssetImportStore.getState().setExternalSystemCode("SAP");
      const result = makeApplyResult();
      mockApply.mockResolvedValueOnce(result);

      await useAssetImportStore.getState().applyBatch();

      expect(mockApply).toHaveBeenCalledWith(11, {
        include_warnings: true,
        external_system_code: "SAP",
      });
      const s = useAssetImportStore.getState();
      expect(s.applyResult).toEqual(result);
      expect(s.batch).toEqual(result.batch);
      expect(s.step).toBe("done");
      expect(s.saving).toBe(false);
    });

    it("sends null external_system_code when it is empty", async () => {
      useAssetImportStore.setState({ batchId: 11 });
      useAssetImportStore.getState().setExternalSystemCode("");
      mockApply.mockResolvedValueOnce(makeApplyResult());

      await useAssetImportStore.getState().applyBatch();

      expect(mockApply).toHaveBeenCalledWith(11, {
        include_warnings: false,
        external_system_code: null,
      });
    });

    it("uses saving (not loading) while in flight", async () => {
      useAssetImportStore.setState({ batchId: 11 });
      const d = deferred<ApplyResult>();
      mockApply.mockReturnValueOnce(d.promise);

      const p = useAssetImportStore.getState().applyBatch();
      expect(useAssetImportStore.getState().saving).toBe(true);
      expect(useAssetImportStore.getState().loading).toBe(false);
      d.resolve(makeApplyResult());
      await p;
      expect(useAssetImportStore.getState().saving).toBe(false);
    });

    it("on failure sets error, stays on the current step and does not rethrow", async () => {
      useAssetImportStore.setState({ batchId: 11, step: "preview" });
      mockApply.mockRejectedValueOnce({ code: "CONFLICT", message: "batch already applied" });

      await expect(useAssetImportStore.getState().applyBatch()).resolves.toBeUndefined();

      const s = useAssetImportStore.getState();
      expect(s.error).toBe("batch already applied");
      expect(s.step).toBe("preview");
      expect(s.applyResult).toBeNull();
      expect(s.saving).toBe(false);
    });
  });

  describe("loadBatches", () => {
    it("loads batch history with no filters", async () => {
      const rows = [makeBatch(), makeBatch({ id: 12 })];
      mockList.mockResolvedValueOnce(rows);

      await useAssetImportStore.getState().loadBatches();

      expect(mockList).toHaveBeenCalledWith();
      expect(useAssetImportStore.getState().batches).toEqual(rows);
      expect(useAssetImportStore.getState().loading).toBe(false);
    });

    it("sets error on failure and keeps prior history", async () => {
      const previous = [makeBatch()];
      useAssetImportStore.setState({ batches: previous });
      mockList.mockRejectedValueOnce(new Error("list failed"));

      await useAssetImportStore.getState().loadBatches();

      expect(useAssetImportStore.getState().error).toBe("list failed");
      expect(useAssetImportStore.getState().batches).toBe(previous);
      expect(useAssetImportStore.getState().loading).toBe(false);
    });
  });

  describe("setters and resetFlow", () => {
    it("setIncludeWarnings and setExternalSystemCode update policy", () => {
      useAssetImportStore.getState().setIncludeWarnings(true);
      useAssetImportStore.getState().setExternalSystemCode("ERP");
      expect(useAssetImportStore.getState().includeWarnings).toBe(true);
      expect(useAssetImportStore.getState().externalSystemCode).toBe("ERP");
    });

    it("resetFlow restores every field to its initial value (including history)", () => {
      useAssetImportStore.setState({
        step: "done",
        selectedFileName: "x.csv",
        selectedFileSha256: ABC_SHA256,
        batchId: 11,
        batch: makeBatch(),
        previewRows: [makeRow()],
        includeWarnings: true,
        externalSystemCode: "SAP",
        applyResult: makeApplyResult(),
        batches: [makeBatch()],
        loading: true,
        saving: true,
        error: "e",
      });

      useAssetImportStore.getState().resetFlow();

      const s = useAssetImportStore.getState();
      expect(s.step).toBe("upload");
      expect(s.selectedFileName).toBeNull();
      expect(s.selectedFileSha256).toBeNull();
      expect(s.batchId).toBeNull();
      expect(s.batch).toBeNull();
      expect(s.previewRows).toEqual([]);
      expect(s.includeWarnings).toBe(false);
      expect(s.externalSystemCode).toBe("import");
      expect(s.applyResult).toBeNull();
      expect(s.batches).toEqual([]);
      expect(s.loading).toBe(false);
      expect(s.saving).toBe(false);
      expect(s.error).toBeNull();
      expect(typeof s.uploadAndCreateBatch).toBe("function");
    });
  });
});
