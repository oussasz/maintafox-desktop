import { describe, it, expect, beforeEach } from "vitest";

import { mockInvoke } from "@/test/mocks/tauri";

import {
  createAssetImportBatch,
  validateAssetImportBatch,
  getAssetImportPreview,
  applyAssetImportBatch,
  listAssetImportBatches,
} from "../asset-import-service";

// ── Fixtures ──────────────────────────────────────────────────────────────────

const batchFixture = {
  id: 1,
  source_filename: "test.csv",
  source_sha256: "abc123",
  initiated_by_id: 1,
  status: "uploaded",
  total_rows: 5,
  valid_rows: 3,
  warning_rows: 1,
  error_rows: 1,
  created_at: "2026-04-06T00:00:00Z",
  updated_at: "2026-04-06T00:00:00Z",
};

const previewFixture = {
  batch: { ...batchFixture, status: "validated" },
  rows: [
    {
      id: 1,
      row_no: 1,
      normalized_asset_code: "PMP-001",
      normalized_external_key: null,
      validation_status: "valid",
      validation_messages: [],
      proposed_action: "create",
      raw_json: "{}",
    },
    {
      id: 2,
      row_no: 2,
      normalized_asset_code: "PMP-002",
      normalized_external_key: null,
      validation_status: "error",
      validation_messages: [
        {
          category: "UnknownClassCode",
          severity: "error",
          message: "Classe introuvable.",
        },
      ],
      proposed_action: null,
      raw_json: "{}",
    },
  ],
};

const applyResultFixture = {
  batch: { ...batchFixture, status: "applied" },
  created: 3,
  updated: 0,
  skipped: 2,
  errored: 0,
};

// ── Tests ─────────────────────────────────────────────────────────────────────

describe("asset-import-service", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("createAssetImportBatch calls the correct command", async () => {
    mockInvoke.mockResolvedValueOnce(batchFixture);
    const result = await createAssetImportBatch("test.csv", "abc123", [1, 2, 3]);
    expect(mockInvoke).toHaveBeenCalledWith("create_asset_import_batch", {
      filename: "test.csv",
      fileSha256: "abc123",
      csvContent: [1, 2, 3],
    });
    expect(result.id).toBe(1);
    expect(result.status).toBe("uploaded");
  });

  it("validateAssetImportBatch calls the correct command", async () => {
    mockInvoke.mockResolvedValueOnce({ ...batchFixture, status: "validated" });
    const result = await validateAssetImportBatch(1);
    expect(mockInvoke).toHaveBeenCalledWith("validate_asset_import_batch", {
      batchId: 1,
    });
    expect(result.status).toBe("validated");
  });

  it("getAssetImportPreview returns batch and rows", async () => {
    mockInvoke.mockResolvedValueOnce(previewFixture);
    const result = await getAssetImportPreview(1);
    expect(result.batch.status).toBe("validated");
    expect(result.rows).toHaveLength(2);
    expect(result.rows[1]?.validation_messages[0]?.category).toBe("UnknownClassCode");
  });

  it("applyAssetImportBatch calls with policy", async () => {
    mockInvoke.mockResolvedValueOnce(applyResultFixture);
    const result = await applyAssetImportBatch(1, {
      include_warnings: false,
      external_system_code: null,
    });
    expect(mockInvoke).toHaveBeenCalledWith("apply_asset_import_batch", {
      batchId: 1,
      policy: { include_warnings: false, external_system_code: null },
    });
    expect(result.created).toBe(3);
    expect(result.skipped).toBe(2);
  });

  it("listAssetImportBatches sends null defaults", async () => {
    mockInvoke.mockResolvedValueOnce([batchFixture]);
    const result = await listAssetImportBatches();
    expect(mockInvoke).toHaveBeenCalledWith("list_asset_import_batches", {
      statusFilter: null,
      limit: null,
    });
    expect(result).toHaveLength(1);
  });

  it("rejects on malformed response", async () => {
    mockInvoke.mockResolvedValueOnce({ bad: "shape" });
    await expect(createAssetImportBatch("x", "y", [])).rejects.toThrow();
  });
});
