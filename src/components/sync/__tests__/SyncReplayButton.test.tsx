/**
 * "Send again" on one refused sync item (AU-44): shown only with sync.replay, asks first, sends
 * the named conflict / outgoing change with a translated reason, says what was done, and shows a
 * refusal on the item.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { P } from "@shared/rbac/permissions.generated";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && "record" in opts
        ? `${key}::${String(opts["record"])}`
        : opts && "message" in opts
          ? `${key}::${String(opts["message"])}`
          : key,
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

const mocks = vi.hoisted(() => ({ replayItem: vi.fn(), pushAppToast: vi.fn() }));
vi.mock("@/stores/sync-orchestrator-store", () => ({
  useSyncOrchestratorStore: (sel: (s: unknown) => unknown) => sel({ replayItem: mocks.replayItem }),
}));
vi.mock("@/store/app-toast-store", () => ({ pushAppToast: mocks.pushAppToast }));

import { SyncReplayButton } from "../SyncReplayButton";

const result = (requeued: number, transitioned: number) => ({
  run: {},
  requeued_outbox_count: requeued,
  transitioned_conflict_count: transitioned,
  checkpoint_token_after: null,
  guard_applied: false,
});

function confirmButton() {
  const buttons = screen.getAllByRole("button", { name: /sync\.replay\.action/ });
  return buttons[buttons.length - 1] as HTMLElement;
}

beforeEach(() => {
  vi.clearAllMocks();
  granted.clear();
  granted.add(P.SYNC_REPLAY);
});

describe("SyncReplayButton", () => {
  it("is absent without sync.replay", () => {
    granted.clear();
    granted.add(P.SYNC_VIEW);
    granted.add(P.SYNC_MANAGE);
    render(<SyncReplayButton target={{ kind: "outbox", outboxId: 4 }} record="DI-0012" />);
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("asks first, then replays the named conflict with its change and a translated reason", async () => {
    mocks.replayItem.mockResolvedValue(result(1, 1));
    render(
      <SyncReplayButton
        target={{ kind: "conflict", conflictId: 12, outboxId: 501 }}
        record="OT-2026-0144"
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: /sync\.replay\.action/ }));
    expect(
      await screen.findByText("sync.replay.confirmConflict::OT-2026-0144"),
    ).toBeInTheDocument();
    expect(mocks.replayItem).not.toHaveBeenCalled();

    fireEvent.click(confirmButton());
    await waitFor(() =>
      expect(mocks.replayItem).toHaveBeenCalledWith(
        { kind: "conflict", conflictId: 12, outboxId: 501 },
        "sync.replay.reason",
      ),
    );
    await waitFor(() =>
      expect(mocks.pushAppToast).toHaveBeenCalledWith({
        title: "sync.replay.done",
        variant: "success",
      }),
    );
  });

  it("replays a given-up change by its id and says when nothing was left to send", async () => {
    mocks.replayItem.mockResolvedValue(result(0, 0));
    render(<SyncReplayButton target={{ kind: "outbox", outboxId: 4 }} record="DI-0012" />);

    fireEvent.click(screen.getByRole("button", { name: /sync\.replay\.action/ }));
    expect(await screen.findByText("sync.replay.confirmOutbox::DI-0012")).toBeInTheDocument();
    fireEvent.click(confirmButton());
    await waitFor(() =>
      expect(mocks.replayItem).toHaveBeenCalledWith(
        { kind: "outbox", outboxId: 4 },
        "sync.replay.reason",
      ),
    );
    await waitFor(() =>
      expect(mocks.pushAppToast).toHaveBeenCalledWith({
        title: "sync.replay.nothing",
        variant: "success",
      }),
    );
  });

  it("shows a refusal on the item as translated text, not the raw permission code", async () => {
    mocks.replayItem.mockRejectedValue({
      code: "PERMISSION_DENIED",
      message: `Permission requise : ${P.SYNC_REPLAY}`,
    });
    render(<SyncReplayButton target={{ kind: "outbox", outboxId: 4 }} record="DI-0012" />);

    fireEvent.click(screen.getByRole("button", { name: /sync\.replay\.action/ }));
    fireEvent.click(confirmButton());
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toMatch(/^sync\.replay\.failed::/);
    expect(alert.textContent).not.toContain("Permission requise");
    expect(mocks.pushAppToast).not.toHaveBeenCalled();
  });

  it("says in words when the server refuses a change that is not the conflict's own", async () => {
    mocks.replayItem.mockRejectedValue({
      code: "VALIDATION_FAILED",
      message:
        "Validation failed: GATE_RECORD_CHANGED_ELSEWHERE: outbox item 9 is not the change held by sync conflict 12.",
    });
    render(
      <SyncReplayButton target={{ kind: "conflict", conflictId: 12, outboxId: 9 }} record="X" />,
    );

    fireEvent.click(screen.getByRole("button", { name: /sync\.replay\.action/ }));
    fireEvent.click(confirmButton());
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toMatch(/^sync\.replay\.failed::/);
    expect(alert.textContent).not.toContain("GATE_");
    expect(alert.textContent).not.toContain("outbox item");
  });
});
