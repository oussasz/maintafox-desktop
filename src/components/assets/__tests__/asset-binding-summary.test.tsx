/**
 * Supervisor Verification — Phase 2 - SP02 - F03 - Sprint S3
 *
 * V1 — Placeholder tolerance: WO, PM, and other not-implemented domains
 *       render cards with clear "Not yet available" state.
 * V2 — Implemented-domain counts: document link count reflects actual
 *       linked rows from the backend response.
 * V3 — Detail panel resilience: rapidly switching asset IDs does not
 *       produce error states; only the last request's data is rendered.
 */

import { render, screen, waitFor, act } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";

import { mockInvoke } from "@/test/mocks/tauri";
import type { AssetBindingSummary as AssetBindingSummaryType } from "@shared/ipc-types";

// ── i18n mock ─────────────────────────────────────────────────────────────────

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => {
      const map: Record<string, string> = {
        "binding.title": "Cross-Module Bindings",
        "binding.notImplemented": "Not yet available",
        "binding.domains.di": "Intervention Requests",
        "binding.domains.wo": "Work Orders",
        "binding.domains.pm": "PM Plans",
        "binding.domains.failure": "Failure Events",
        "binding.domains.document": "Documents",
        "binding.domains.iot": "IoT Signals",
        "binding.domains.erp": "ERP Mappings",
      };
      return map[key] ?? key;
    },
    i18n: { language: "en" },
  }),
}));

// Async import after mock is set up
const { AssetBindingSummary } = await import("@/components/assets/AssetBindingSummary");

// ── Fixtures ──────────────────────────────────────────────────────────────────

function makeSummary(overrides: Partial<AssetBindingSummaryType> = {}): AssetBindingSummaryType {
  return {
    asset_id: 1,
    linked_di_count: { status: "not_implemented", count: null },
    linked_wo_count: { status: "not_implemented", count: null },
    linked_pm_plan_count: { status: "not_implemented", count: null },
    linked_failure_event_count: { status: "not_implemented", count: null },
    linked_document_count: { status: "available", count: 3 },
    linked_iot_signal_count: { status: "not_implemented", count: null },
    linked_erp_mapping_count: { status: "not_implemented", count: null },
    ...overrides,
  };
}

// ── Helpers ────────────────────────────────────────────────────────────────────

function setupMock(summary: AssetBindingSummaryType) {
  mockInvoke.mockImplementation(async (cmd: string) => {
    if (cmd === "get_asset_binding_summary") return summary;
    return null;
  });
}

// ── Tests ──────────────────────────────────────────────────────────────────────

describe("AssetBindingSummary — S3 Supervisor Verification", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // ─── V1 — Placeholder tolerance ──────────────────────────────────────────

  describe("V1 — Placeholder tolerance", () => {
    it("renders all 7 domain cards when most are not_implemented", async () => {
      setupMock(makeSummary());
      render(<AssetBindingSummary assetId={1} />);

      await waitFor(() => {
        expect(screen.getByText("Cross-Module Bindings")).toBeInTheDocument();
      });

      // All 7 domain labels should be present
      expect(screen.getByText("Intervention Requests")).toBeInTheDocument();
      expect(screen.getByText("Work Orders")).toBeInTheDocument();
      expect(screen.getByText("PM Plans")).toBeInTheDocument();
      expect(screen.getByText("Failure Events")).toBeInTheDocument();
      expect(screen.getByText("Documents")).toBeInTheDocument();
      expect(screen.getByText("IoT Signals")).toBeInTheDocument();
      expect(screen.getByText("ERP Mappings")).toBeInTheDocument();
    });

    it("shows 'Not yet available' badges for WO and PM domains", async () => {
      setupMock(makeSummary());
      render(<AssetBindingSummary assetId={1} />);

      await waitFor(() => {
        expect(screen.getByText("Cross-Module Bindings")).toBeInTheDocument();
      });

      // 6 domains are not_implemented → 6 "Not yet available" badges
      const badges = screen.getAllByText("Not yet available");
      expect(badges).toHaveLength(6);
    });

    it("does not show 'Not yet available' for available domains", async () => {
      // Make all domains available
      const allAvailable = makeSummary({
        linked_di_count: { status: "available", count: 2 },
        linked_wo_count: { status: "available", count: 5 },
        linked_pm_plan_count: { status: "available", count: 1 },
        linked_failure_event_count: { status: "available", count: 0 },
        linked_document_count: { status: "available", count: 3 },
        linked_iot_signal_count: { status: "available", count: 0 },
        linked_erp_mapping_count: { status: "available", count: 1 },
      });

      setupMock(allAvailable);
      render(<AssetBindingSummary assetId={1} />);

      await waitFor(() => {
        expect(screen.getByText("Cross-Module Bindings")).toBeInTheDocument();
      });

      expect(screen.queryByText("Not yet available")).not.toBeInTheDocument();
    });
  });

  // ─── V2 — Implemented-domain counts ──────────────────────────────────────

  describe("V2 — Implemented-domain counts", () => {
    it("displays actual document link count from backend", async () => {
      setupMock(makeSummary({ linked_document_count: { status: "available", count: 7 } }));
      render(<AssetBindingSummary assetId={1} />);

      await waitFor(() => {
        expect(screen.getByText("7")).toBeInTheDocument();
      });
    });

    it("displays zero count without error for available domains with 0 links", async () => {
      setupMock(makeSummary({ linked_document_count: { status: "available", count: 0 } }));
      render(<AssetBindingSummary assetId={1} />);

      await waitFor(() => {
        expect(screen.getByText("Cross-Module Bindings")).toBeInTheDocument();
      });

      // Count should render as "0", not "Not yet available"
      expect(screen.getByText("0")).toBeInTheDocument();
    });

    it("handles null count on available domain gracefully (falls back to 0)", async () => {
      // Edge case: available status but null count
      setupMock(
        makeSummary({
          linked_document_count: { status: "available", count: null },
        }),
      );
      render(<AssetBindingSummary assetId={1} />);

      await waitFor(() => {
        expect(screen.getByText("Cross-Module Bindings")).toBeInTheDocument();
      });

      // The component does `entry.count ?? 0`, so should show 0
      expect(screen.getByText("0")).toBeInTheDocument();
    });
  });

  // ─── V3 — Detail panel resilience ────────────────────────────────────────

  describe("V3 — Detail panel resilience", () => {
    it("does not flicker into error state on rapid asset switches", async () => {
      // Each call returns a different summary
      let callCount = 0;
      mockInvoke.mockImplementation(async (cmd: string, args?: unknown) => {
        if (cmd === "get_asset_binding_summary") {
          callCount++;
          const id = (args as { assetId: number }).assetId;
          // Simulate network delay — earlier calls take longer
          await new Promise((r) => setTimeout(r, callCount === 1 ? 50 : 10));
          return makeSummary({
            asset_id: id,
            linked_document_count: { status: "available", count: id * 10 },
          });
        }
        return null;
      });

      const { rerender } = render(<AssetBindingSummary assetId={1} />);

      // Rapidly switch to asset 2, then 3
      rerender(<AssetBindingSummary assetId={2} />);
      rerender(<AssetBindingSummary assetId={3} />);

      // Wait for final render to settle
      await waitFor(() => {
        expect(screen.getByText("Cross-Module Bindings")).toBeInTheDocument();
      });

      // Should never show an error state
      expect(screen.queryByText(/error|failed|échec/i)).not.toBeInTheDocument();

      // The last asset's count (30) should be visible
      await waitFor(() => {
        expect(screen.getByText("30")).toBeInTheDocument();
      });
    });

    it("does not enter error state when invoke is slow", async () => {
      mockInvoke.mockImplementation(async (cmd: string) => {
        if (cmd === "get_asset_binding_summary") {
          await new Promise((r) => setTimeout(r, 100));
          return makeSummary();
        }
        return null;
      });

      render(<AssetBindingSummary assetId={1} />);

      // Initially shows loading (no error)
      expect(screen.queryByText(/error|failed|échec/i)).not.toBeInTheDocument();

      // Wait for data to arrive
      await waitFor(() => {
        expect(screen.getByText("Cross-Module Bindings")).toBeInTheDocument();
      });

      // Still no error
      expect(screen.queryByText(/error|failed|échec/i)).not.toBeInTheDocument();
    });

    it("shows error message if invoke rejects, then recovers on rerender", async () => {
      let shouldFail = true;
      mockInvoke.mockImplementation(async (cmd: string) => {
        if (cmd === "get_asset_binding_summary") {
          if (shouldFail) throw { code: "INTERNAL", message: "db locked" };
          return makeSummary();
        }
        return null;
      });

      const { rerender } = render(<AssetBindingSummary assetId={1} />);

      // First load fails
      await waitFor(() => {
        expect(screen.getByText("db locked")).toBeInTheDocument();
      });

      // Backend recovers
      shouldFail = false;
      await act(async () => {
        rerender(<AssetBindingSummary assetId={2} />);
      });

      // Should recover and show data
      await waitFor(() => {
        expect(screen.getByText("Cross-Module Bindings")).toBeInTheDocument();
      });
      expect(screen.queryByText("db locked")).not.toBeInTheDocument();
    });
  });
});
