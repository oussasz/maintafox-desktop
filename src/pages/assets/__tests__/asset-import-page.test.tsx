/**
 * Supervisor Verification — Phase 2 - SP02 - F04 - Sprint S3
 *
 * V1 — Apply button disabled when batch is not validated.
 * V2 — Conflict table shows category and message for each row issue.
 * V3 — Outcome summary displays created/updated/skipped/errored counts.
 */

import { render, screen, waitFor } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";

// ── i18n mock ─────────────────────────────────────────────────────────────────

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, vars?: Record<string, unknown>) => {
      const map: Record<string, string> = {
        "import.page.title": "Asset Import",
        "import.steps.upload": "Upload",
        "import.steps.validate": "Validate",
        "import.steps.preview": "Preview",
        "import.steps.apply": "Apply",
        "import.steps.done": "Done",
        "import.status.valid": "Valid",
        "import.status.warning": "Warning",
        "import.status.error": "Error",
        "import.upload.title": "Upload CSV",
        "import.upload.description": "Select a CSV file to import.",
        "import.upload.hint": "Choose a .csv file",
        "import.upload.selectFile": "Select file",
        "import.validate.title": "Validate",
        "import.validate.file": "File",
        "import.validate.rows": "Rows",
        "import.validate.run": "Run validation",
        "import.preview.title": "Preview & Inspect",
        "import.preview.description": "Review rows before applying.",
        "import.preview.total": "Total",
        "import.preview.columns.code": "Code",
        "import.preview.columns.externalKey": "External Key",
        "import.preview.columns.status": "Status",
        "import.preview.columns.action": "Action",
        "import.preview.columns.messages": "Messages",
        "import.preview.includeWarnings": "Include warnings",
        "import.apply.run": "Apply Import",
        "import.done.title": "Import Complete",
        "import.done.description": "The import has been applied.",
        "import.done.created": "Created",
        "import.done.updated": "Updated",
        "import.done.skipped": "Skipped",
        "import.done.errored": "Errored",
        "import.done.newImport": "New Import",
      };
      if (key === "import.validate.description") {
        return `Validate ${vars?.["filename"] ?? ""} (${vars?.["rows"] ?? 0} rows)`;
      }
      return map[key] ?? key;
    },
    i18n: { language: "en" },
  }),
}));

// Async import after mock is set up
const { AssetImportPage } = await import("@/pages/assets/AssetImportPage");
const { useAssetImportStore } = await import("@/stores/asset-import-store");

// ── Fixtures ──────────────────────────────────────────────────────────────────

const validatedBatch = {
  id: 1,
  source_filename: "test.csv",
  source_sha256: "abc",
  initiated_by_id: 1,
  status: "validated",
  total_rows: 10,
  valid_rows: 7,
  warning_rows: 2,
  error_rows: 1,
  created_at: "2026-04-06T00:00:00Z",
  updated_at: "2026-04-06T00:00:00Z",
};

const uploadedBatch = { ...validatedBatch, status: "uploaded" };

const previewRows = [
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
        message: "Unknown class code.",
      },
    ],
    proposed_action: null,
    raw_json: "{}",
  },
  {
    id: 3,
    row_no: 3,
    normalized_asset_code: "PMP-003",
    normalized_external_key: "EXT-3",
    validation_status: "warning",
    validation_messages: [
      {
        category: "Reclassification",
        severity: "warning",
        message: "Asset will be re-classified.",
      },
    ],
    proposed_action: "update",
    raw_json: "{}",
  },
];

const applyResult = {
  batch: { ...validatedBatch, status: "applied" },
  created: 5,
  updated: 2,
  skipped: 2,
  errored: 1,
};

// ── Tests ─────────────────────────────────────────────────────────────────────

describe("AssetImportPage — S3 Supervisor Verification", () => {
  beforeEach(() => {
    // Reset store to initial state
    useAssetImportStore.setState({
      step: "upload",
      selectedFileName: null,
      selectedFileSha256: null,
      batchId: null,
      batch: null,
      previewRows: [],
      includeWarnings: false,
      externalSystemCode: "import",
      applyResult: null,
      batches: [],
      loading: false,
      saving: false,
      error: null,
    });
  });

  // ─── V1 — Apply button disabled when validation not complete ─────────────

  describe("V1 — Apply button disabled when not validated", () => {
    it("disables apply button when batch status is uploaded", async () => {
      useAssetImportStore.setState({
        step: "preview",
        batch: uploadedBatch as never,
        previewRows: previewRows as never,
      });

      render(<AssetImportPage />);

      await waitFor(() => {
        expect(screen.getByTestId("apply-button")).toBeInTheDocument();
      });

      expect(screen.getByTestId("apply-button")).toBeDisabled();
    });

    it("enables apply button when batch status is validated", async () => {
      useAssetImportStore.setState({
        step: "preview",
        batch: validatedBatch as never,
        previewRows: previewRows as never,
      });

      render(<AssetImportPage />);

      await waitFor(() => {
        expect(screen.getByTestId("apply-button")).toBeInTheDocument();
      });

      expect(screen.getByTestId("apply-button")).not.toBeDisabled();
    });

    it("disables apply button when saving is in progress", async () => {
      useAssetImportStore.setState({
        step: "preview",
        batch: validatedBatch as never,
        previewRows: previewRows as never,
        saving: true,
      });

      render(<AssetImportPage />);

      await waitFor(() => {
        expect(screen.getByTestId("apply-button")).toBeInTheDocument();
      });

      expect(screen.getByTestId("apply-button")).toBeDisabled();
    });
  });

  // ─── V2 — Conflict table shows category and message ─────────────────────

  describe("V2 — Conflict table rendering", () => {
    it("renders conflict rows with category and message", async () => {
      useAssetImportStore.setState({
        step: "preview",
        batch: validatedBatch as never,
        previewRows: previewRows as never,
      });

      render(<AssetImportPage />);

      await waitFor(() => {
        expect(screen.getByText("Preview & Inspect")).toBeInTheDocument();
      });

      // Conflict messages should show [category] message
      const messages = screen.getAllByTestId("conflict-message");
      expect(messages).toHaveLength(2);
      expect(messages[0]?.textContent).toContain("[UnknownClassCode]");
      expect(messages[0]?.textContent).toContain("Unknown class code.");
      expect(messages[1]?.textContent).toContain("[Reclassification]");
      expect(messages[1]?.textContent).toContain("Asset will be re-classified.");
    });

    it("renders summary badges with counts", async () => {
      useAssetImportStore.setState({
        step: "preview",
        batch: validatedBatch as never,
        previewRows: previewRows as never,
      });

      render(<AssetImportPage />);

      await waitFor(() => {
        expect(screen.getByTestId("summary-badges")).toBeInTheDocument();
      });

      const badges = screen.getByTestId("summary-badges");
      expect(badges.textContent).toContain("10"); // total
      expect(badges.textContent).toContain("7"); // valid
      expect(badges.textContent).toContain("2"); // warning
      expect(badges.textContent).toContain("1"); // error
    });
  });

  // ─── V3 — Outcome summary ───────────────────────────────────────────────

  describe("V3 — Outcome summary rendering", () => {
    it("displays created/updated/skipped/errored counts", async () => {
      useAssetImportStore.setState({
        step: "done",
        applyResult: applyResult as never,
      });

      render(<AssetImportPage />);

      await waitFor(() => {
        expect(screen.getByTestId("outcome-summary")).toBeInTheDocument();
      });

      expect(screen.getByText("Import Complete")).toBeInTheDocument();

      const summary = screen.getByTestId("outcome-summary");
      expect(summary.textContent).toContain("5"); // created
      expect(summary.textContent).toContain("2"); // updated + skipped
      expect(summary.textContent).toContain("1"); // errored

      // Also check the labels
      expect(screen.getByText("Created")).toBeInTheDocument();
      expect(screen.getByText("Updated")).toBeInTheDocument();
      expect(screen.getByText("Skipped")).toBeInTheDocument();
      expect(screen.getByText("Errored")).toBeInTheDocument();
    });

    it("shows a New Import button in done step", async () => {
      useAssetImportStore.setState({
        step: "done",
        applyResult: applyResult as never,
      });

      render(<AssetImportPage />);

      await waitFor(() => {
        expect(screen.getByText("New Import")).toBeInTheDocument();
      });
    });
  });
});
