/**
 * Sync conflict review list — record named by its number (not uuid), a dialog with a required
 * reason before every choice, error with retry when the inbox cannot be loaded.
 */
import { readFileSync } from "node:fs";

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
import { CONFLICT_REASON_MIN } from "../sync-conflict-display";

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
    const confirm = confirmButtons[confirmButtons.length - 1] as HTMLElement;
    // No reason yet: the choice cannot be confirmed.
    expect(confirm).toBeDisabled();
    fireEvent.change(screen.getByRole("textbox", { name: /sync.reason.label/ }), {
      target: { value: "  Server version checked with the planner \n" },
    });
    expect(confirm).toBeEnabled();
    fireEvent.click(confirm);
    await waitFor(() =>
      expect(mockResolve).toHaveBeenCalledWith({
        conflict_id: 3,
        expected_row_version: 2,
        action: "accept_remote",
        resolution_note: "Server version checked with the planner",
      }),
    );
  });

  it("keeps the choice disabled while the reason is shorter than the minimum once trimmed", async () => {
    inbox = [conflict];
    mockRefresh.mockResolvedValue(undefined);

    render(<SyncConflictList />);

    fireEvent.click(await screen.findByRole("button", { name: "sync.panel.action.dismiss" }));
    const confirmButtons = screen.getAllByRole("button", { name: "sync.panel.action.dismiss" });
    const confirm = confirmButtons[confirmButtons.length - 1] as HTMLElement;
    const short = "x".repeat(CONFLICT_REASON_MIN - 1);
    fireEvent.change(screen.getByRole("textbox", { name: /sync.reason.label/ }), {
      target: { value: `   ${short}   ` },
    });
    expect(confirm).toBeDisabled();
    fireEvent.click(confirm);
    expect(mockResolve).not.toHaveBeenCalled();
    fireEvent.change(screen.getByRole("textbox", { name: /sync.reason.label/ }), {
      target: { value: `${short}y` },
    });
    expect(confirm).toBeEnabled();
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

  it("asks the reason for triage actions too", async () => {
    inbox = [conflict];
    mockRefresh.mockResolvedValue(undefined);
    mockResolve.mockResolvedValue(undefined);

    render(<SyncConflictList showTriageActions />);

    fireEvent.click(await screen.findByRole("button", { name: "sync.panel.action.retry_later" }));
    expect(await screen.findByText("sync.confirm.retry_later::OT-2026-0144")).toBeInTheDocument();
    expect(mockResolve).not.toHaveBeenCalled();
    fireEvent.change(screen.getByRole("textbox", { name: /sync.reason.label/ }), {
      target: { value: "Waiting for the other site's answer" },
    });
    const confirmButtons = screen.getAllByRole("button", {
      name: "sync.panel.action.retry_later",
    });
    fireEvent.click(confirmButtons[confirmButtons.length - 1] as HTMLElement);
    await waitFor(() =>
      expect(mockResolve).toHaveBeenCalledWith(
        expect.objectContaining({
          action: "retry_later",
          resolution_note: "Waiting for the other site's answer",
        }),
      ),
    );
  });

  it("explains a username created on two PCs and offers no version to take", async () => {
    inbox = [
      {
        ...conflict,
        entity_type: "user_accounts",
        conflict_type: "USERNAME_CLASH",
        recommended_action: "review",
        local_payload_json: null,
        inbound_payload_json: JSON.stringify({ username: "chef", display_name: "Chef" }),
      },
    ];
    mockRefresh.mockResolvedValue(undefined);

    render(<SyncConflictList />);

    expect(await screen.findByText("sync.conflictKind.USERNAME_CLASH")).toBeInTheDocument();
    expect(screen.getByText("sync.usernameClash")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /sync\.panel\.action\.accept_/ }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "sync.panel.action.dismiss" })).toBeInTheDocument();
  });
});

describe("conflict reason minimum follows the server", () => {
  it("is MIN_REASON_CHARS, which resolve_sync_conflict checks", () => {
    const catalog = readFileSync("src-tauri/src/settings/catalog.rs", "utf8");
    const min = /pub const MIN_REASON_CHARS: usize = ([0-9]+);/.exec(catalog)?.[1];
    expect(CONFLICT_REASON_MIN).toBe(Number(min));
    const queries = readFileSync("src-tauri/src/sync/queries.rs", "utf8");
    const resolve = /pub async fn resolve_sync_conflict[\s\S]*?\r?\n\}\r?\n/.exec(queries)?.[0];
    expect(resolve).toContain("let min_reason = crate::settings::catalog::MIN_REASON_CHARS;");
  });
});
