/**
 * Changes given up after their delivery attempts (AU-44): listed by record, error with retry when
 * they cannot be loaded, hidden when there are none, "send again" only with sync.replay and on the
 * item's own id.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { P } from "@shared/rbac/permissions.generated";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && "record" in opts ? `${key}::${String(opts["record"])}` : key,
    i18n: { language: "fr", changeLanguage: vi.fn() },
  }),
}));

const granted = new Set<string>();
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({
    can: (p: string) => granted.has(p),
    canAny: (...ps: string[]) => ps.some((p) => granted.has(p)),
    isLoading: false,
  }),
}));

const mocks = vi.hoisted(() => ({
  listOutboxItems: vi.fn(),
  replayItem: vi.fn(),
  pushAppToast: vi.fn(),
}));
vi.mock("@/services/sync-service", () => ({ listOutboxItems: mocks.listOutboxItems }));
vi.mock("@/stores/sync-orchestrator-store", () => ({
  useSyncOrchestratorStore: (sel: (s: unknown) => unknown) => sel({ replayItem: mocks.replayItem }),
}));
vi.mock("@/store/app-toast-store", () => ({ pushAppToast: mocks.pushAppToast }));

import { SyncDeadLetterList } from "../SyncDeadLetterList";

const item = {
  id: 77,
  idempotency_key: "k-77",
  entity_type: "work_orders",
  entity_sync_id: "6f1c2a8e-uuid",
  operation: "update",
  row_version: 3,
  payload_json: JSON.stringify({ code: "OT-2026-0144", title: "Pump repair" }),
  payload_hash: "h",
  status: "dead_letter",
  acknowledged_at: null,
  rejection_code: null,
  rejection_message: null,
  origin_machine_id: null,
  base_row_version: null,
  owner_user_sync_id: null,
  attempt_count: 5,
  last_attempt_at: null,
  last_error: "HTTP 503: upstream connect error",
  created_at: "2026-10-26T08:00:00Z",
  updated_at: "2026-10-26T08:00:00Z",
};

beforeEach(() => {
  vi.clearAllMocks();
  granted.clear();
  granted.add(P.SYNC_VIEW);
  granted.add(P.SYNC_REPLAY);
});

describe("SyncDeadLetterList", () => {
  it("is hidden and loads nothing when the worker counts none", () => {
    const { container } = render(<SyncDeadLetterList count={0} />);
    expect(container).toBeEmptyDOMElement();
    expect(mocks.listOutboxItems).not.toHaveBeenCalled();
  });

  it("lists the given-up changes by record, the server's text only in the details", async () => {
    mocks.listOutboxItems.mockResolvedValue([item]);
    render(<SyncDeadLetterList count={1} />);

    expect(await screen.findByText("OT-2026-0144")).toBeInTheDocument();
    expect(mocks.listOutboxItems).toHaveBeenCalledWith({ status: "dead_letter", limit: 50 });
    expect(screen.getByText("sync.entity.work_orders")).toBeInTheDocument();
    expect(screen.getByText(/HTTP 503/).closest("details")).not.toBeNull();
  });

  it("shows a load failure with retry, and loads again on retry", async () => {
    mocks.listOutboxItems.mockRejectedValueOnce(new Error("boom")).mockResolvedValueOnce([item]);
    render(<SyncDeadLetterList count={1} />);

    expect(await screen.findByText("sync.replay.deadLetterLoadError")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button"));
    expect(await screen.findByText("OT-2026-0144")).toBeInTheDocument();
    expect(mocks.listOutboxItems).toHaveBeenCalledTimes(2);
  });

  it("offers send again only with sync.replay", async () => {
    granted.delete(P.SYNC_REPLAY);
    mocks.listOutboxItems.mockResolvedValue([item]);
    render(<SyncDeadLetterList count={1} />);

    expect(await screen.findByText("OT-2026-0144")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /sync\.replay\.action/ })).toBeNull();
  });

  it("sends the item's own id after confirmation", async () => {
    mocks.listOutboxItems.mockResolvedValue([item]);
    mocks.replayItem.mockResolvedValue({
      run: {},
      requeued_outbox_count: 1,
      transitioned_conflict_count: 1,
      checkpoint_token_after: null,
      guard_applied: false,
    });
    render(<SyncDeadLetterList count={1} />);

    fireEvent.click(await screen.findByRole("button", { name: /sync\.replay\.action/ }));
    expect(await screen.findByText("sync.replay.confirmOutbox::OT-2026-0144")).toBeInTheDocument();
    const buttons = screen.getAllByRole("button", { name: /sync\.replay\.action/ });
    fireEvent.click(buttons[buttons.length - 1] as HTMLElement);
    await waitFor(() =>
      expect(mocks.replayItem).toHaveBeenCalledWith(
        { kind: "outbox", outboxId: 77 },
        "sync.replay.reason",
      ),
    );
  });
});
