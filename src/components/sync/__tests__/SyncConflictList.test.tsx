/**
 * Sync conflict review list — record named by its number (not uuid), confirmation before a
 * version is replaced, error with retry when the inbox cannot be loaded.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && "record" in opts ? `${key}::${String(opts["record"])}` : key,
    i18n: { language: "fr", changeLanguage: vi.fn() },
  }),
}));

const mockRefresh = vi.fn();
const mockResolve = vi.fn();
let inbox: unknown[] = [];

vi.mock("@/stores/sync-orchestrator-store", () => ({
  useSyncOrchestratorStore: (sel: (s: unknown) => unknown) =>
    sel({
      conflictInbox: inbox,
      refreshInbox: mockRefresh,
      resolveConflictAction: mockResolve,
    }),
}));

import { SyncConflictList } from "../SyncConflictList";

const conflict = {
  id: 3,
  conflict_key: "k",
  source_scope: "tenant",
  source_batch_id: null,
  linked_outbox_id: null,
  linked_inbox_id: null,
  entity_type: "work_orders",
  entity_sync_id: "6f1c2a8e-uuid",
  operation: "update",
  conflict_type: "CONCURRENT_EDIT",
  local_payload_json: JSON.stringify({ code: "OT-2026-0144", title: "Pump repair" }),
  inbound_payload_json: JSON.stringify({ code: "OT-2026-0144", title: "Pump repair (B)" }),
  authority_side: "server",
  checkpoint_token: null,
  auto_resolution_policy: "manual",
  requires_operator_review: true,
  recommended_action: "accept_remote",
  status: "new",
  resolution_action: null,
  resolution_note: null,
  resolved_by_id: null,
  resolved_at: null,
  row_version: 2,
  local_row_version: 5,
  inbound_row_version: 6,
  created_at: "2026-10-16T08:00:00Z",
  updated_at: "2026-10-16T08:00:00Z",
};

beforeEach(() => {
  vi.clearAllMocks();
  inbox = [];
});

describe("SyncConflictList", () => {
  it("names the record by its number and asks before taking the server's version", async () => {
    inbox = [conflict];
    mockRefresh.mockResolvedValue(undefined);
    mockResolve.mockResolvedValue(undefined);

    render(<SyncConflictList />);

    expect(await screen.findByText("OT-2026-0144")).toBeInTheDocument();
    expect(screen.getByText("sync.entity.work_orders")).toBeInTheDocument();
    // The uuid is only in the collapsed technical details, never in the record line.
    expect(screen.getByText(/6f1c2a8e-uuid/).closest("details")).not.toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /sync\.panel\.action\.accept_remote/ }));
    expect(await screen.findByText("sync.confirm.accept_remote::OT-2026-0144")).toBeInTheDocument();
    expect(mockResolve).not.toHaveBeenCalled();

    const confirmButtons = screen.getAllByRole("button", {
      name: "sync.panel.action.accept_remote",
    });
    fireEvent.click(confirmButtons[confirmButtons.length - 1] as HTMLElement);
    await waitFor(() =>
      expect(mockResolve).toHaveBeenCalledWith({
        conflict_id: 3,
        expected_row_version: 2,
        action: "accept_remote",
        resolution_note: null,
      }),
    );
  });

  it("shows an inbox load failure with retry instead of 'nothing to review'", async () => {
    mockRefresh.mockRejectedValueOnce({ code: "DB", message: "down" });

    render(<SyncConflictList />);

    expect(await screen.findByText("sync.inboxLoadError")).toBeInTheDocument();
    expect(screen.queryByText("sync.noConflicts")).not.toBeInTheDocument();

    mockRefresh.mockResolvedValueOnce(undefined);
    fireEvent.click(screen.getByRole("button", { name: "ds.state.retry" }));
    expect(await screen.findByText("sync.noConflicts")).toBeInTheDocument();
  });

  it("runs reversible triage actions without a confirmation", async () => {
    inbox = [conflict];
    mockRefresh.mockResolvedValue(undefined);
    mockResolve.mockResolvedValue(undefined);

    render(<SyncConflictList showTriageActions withResolutionNote />);

    fireEvent.click(await screen.findByRole("button", { name: "sync.panel.action.retry_later" }));
    await waitFor(() =>
      expect(mockResolve).toHaveBeenCalledWith(
        expect.objectContaining({
          action: "retry_later",
          resolution_note: "sync.panel.resolutionNote",
        }),
      ),
    );
    expect(screen.queryByText("sync.confirm.title")).not.toBeInTheDocument();
  });
});
