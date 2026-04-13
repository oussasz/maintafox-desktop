import { render, screen, waitFor, within, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

import { ArchiveExplorer } from "@/components/archive/ArchiveExplorer";
import { PermissionProvider } from "@/contexts/PermissionContext";
import type { ArchiveFilterInput, ArchiveItemSummary } from "@/services/archive-service";

const mockGetMyPermissions = vi.fn();

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(vi.fn())),
}));

vi.mock("@/services/rbac-service", () => ({
  getMyPermissions: (...args: unknown[]) => mockGetMyPermissions(...args),
}));

const archiveMocks = vi.hoisted(() => {
  const fixtureItems: ArchiveItemSummary[] = [
    {
      id: 1,
      source_module: "work_orders",
      source_record_id: "WO-1001",
      archive_class: "WO_CLOSED",
      source_state: "closed",
      archive_reason_code: "lifecycle",
      archived_at: "2024-06-01T12:00:00.000Z",
      archived_by_id: 1,
      retention_policy_id: null,
      restore_policy: "standard",
      restore_until_at: null,
      legal_hold: false,
      checksum_sha256: null,
      search_text: "alpha note",
    },
    {
      id: 2,
      source_module: "work_orders",
      source_record_id: "WO-1002",
      archive_class: "WO_CLOSED",
      source_state: "closed",
      archive_reason_code: "lifecycle",
      archived_at: "2025-01-15T12:00:00.000Z",
      archived_by_id: 1,
      retention_policy_id: null,
      restore_policy: "standard",
      restore_until_at: null,
      legal_hold: false,
      checksum_sha256: null,
      search_text: "beta note",
    },
    {
      id: 3,
      source_module: "di",
      source_record_id: "DI-9",
      archive_class: "DI_RESOLVED",
      source_state: "resolved",
      archive_reason_code: "closed",
      archived_at: "2024-03-01T12:00:00.000Z",
      archived_by_id: 1,
      retention_policy_id: null,
      restore_policy: "standard",
      restore_until_at: null,
      legal_hold: false,
      checksum_sha256: null,
      search_text: "gamma",
    },
  ];

  return {
    fixtureItems,
    listArchiveItems: vi.fn((_filter: ArchiveFilterInput) => Promise.resolve(fixtureItems)),
    getArchiveItem: vi.fn((_id: number) =>
      Promise.reject(new Error("getArchiveItem not stubbed for this test")),
    ),
    exportArchiveItems: vi.fn(),
    purgeArchiveItems: vi.fn(),
    restoreArchiveItem: vi.fn(),
    setLegalHold: vi.fn(),
  };
});

vi.mock("@/services/archive-service", () => ({
  listArchiveItems: (filter: ArchiveFilterInput) => archiveMocks.listArchiveItems(filter),
  getArchiveItem: (id: number) => archiveMocks.getArchiveItem(id),
  exportArchiveItems: (payload: unknown) => archiveMocks.exportArchiveItems(payload),
  purgeArchiveItems: (payload: unknown) => archiveMocks.purgeArchiveItems(payload),
  restoreArchiveItem: (payload: unknown) => archiveMocks.restoreArchiveItem(payload),
  setLegalHold: (payload: unknown) => archiveMocks.setLegalHold(payload),
}));

function renderExplorer() {
  return render(
    <PermissionProvider>
      <ArchiveExplorer />
    </PermissionProvider>,
  );
}

function lastListArchiveFilter(): ArchiveFilterInput | undefined {
  const calls = archiveMocks.listArchiveItems.mock.calls as unknown as [ArchiveFilterInput][];
  return calls[calls.length - 1]?.[0];
}

function makePermissions() {
  return [
    {
      name: "arc.export",
      description: "",
      category: "archive",
      is_dangerous: false,
      requires_step_up: false,
    },
    {
      name: "arc.restore",
      description: "",
      category: "archive",
      is_dangerous: false,
      requires_step_up: false,
    },
    {
      name: "arc.purge",
      description: "",
      category: "archive",
      is_dangerous: true,
      requires_step_up: false,
    },
  ];
}

describe("ArchiveExplorer — folder tree after filter updates (SP07 carry-forward)", () => {
  beforeEach(() => {
    mockGetMyPermissions.mockReset();
    mockGetMyPermissions.mockResolvedValue(makePermissions());
    archiveMocks.listArchiveItems.mockReset();
    archiveMocks.listArchiveItems.mockImplementation(() =>
      Promise.resolve(archiveMocks.fixtureItems),
    );
  });

  it("keeps module / class / year tree clickable after search, legal-hold, and class-chip filters", async () => {
    renderExplorer();

    await waitFor(() => {
      expect(archiveMocks.listArchiveItems).toHaveBeenCalled();
    });
    await waitFor(() => {
      expect(screen.queryByText("Loading archive items...")).not.toBeInTheDocument();
    });

    const tree = screen.getByTestId("archive-folder-tree");
    const moduleWo = within(tree).getByText("work_orders");
    fireEvent.click(moduleWo);
    await waitFor(() => {
      expect(lastListArchiveFilter()?.source_module).toBe("work_orders");
    });

    const search = screen.getByPlaceholderText("Search archived text...");
    fireEvent.change(search, { target: { value: "alpha" } });
    await waitFor(() => {
      expect(lastListArchiveFilter()?.search_text).toBe("alpha");
    });

    const legalCheckbox = screen.getByRole("checkbox", { name: /legal hold only/i });
    fireEvent.click(legalCheckbox);
    await waitFor(() => {
      expect(lastListArchiveFilter()?.legal_hold).toBe(true);
    });

    const woClosedChip = screen.getAllByRole("button", { name: "WO_CLOSED" })[1]!;
    fireEvent.click(woClosedChip);
    await waitFor(() => {
      expect(screen.getByText("WO-1001")).toBeInTheDocument();
    });

    expect(within(tree).getByText("work_orders")).toBeVisible();
    expect(within(tree).getByText("di")).toBeVisible();

    const moduleDi = within(tree).getByText("di");
    fireEvent.click(moduleDi);
    await waitFor(() => {
      expect(lastListArchiveFilter()?.source_module).toBe("di");
    });

    const woClosedInTree = within(tree).getByRole("button", { name: "WO_CLOSED" });
    fireEvent.click(woClosedInTree);

    const year2024UnderWo = within(tree).getAllByRole("button", { name: /^2024 / })[1]!;
    fireEvent.click(year2024UnderWo);
    await waitFor(() => {
      const f = lastListArchiveFilter();
      expect(f?.date_from).toBe("2024-01-01");
      expect(f?.date_to).toContain("2024-12-31");
    });

    expect(within(tree).getByText("di")).toBeVisible();
    fireEvent.click(within(tree).getByText("work_orders"));
    await waitFor(() => {
      expect(lastListArchiveFilter()?.source_module).toBe("work_orders");
    });
  });
});
