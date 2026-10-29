import { act, fireEvent, renderHook, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { mockInvoke } from "@/test/mocks/tauri";
import type { SyncWorkerStatus } from "@shared/ipc-types";

import { useSyncStatus } from "../hooks/use-sync-status";
import { ReportFaultScreen } from "../screens/ReportFaultScreen";
import { WorkOrdersScreen } from "../screens/WorkOrdersScreen";

import { ipcAnswering, renderMobile } from "./helpers";

// The engine's event channel is the only thing replaced besides the IPC boundary: the hook, the
// screens and the services are the real ones.
const channel = vi.hoisted(() => ({
  handlers: [] as Array<(event: { payload: unknown }) => void>,
  unlisten: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (_name: string, handler: (event: { payload: unknown }) => void) => {
    channel.handlers.push(handler);
    return channel.unlisten;
  }),
}));

function status(overrides: Partial<SyncWorkerStatus> = {}): SyncWorkerStatus {
  return {
    state: "idle",
    blocker: null,
    paused_until: null,
    last_success_at: "2026-10-29T11:55:00Z",
    last_attempt_at: null,
    next_attempt_at: null,
    pending_outbox_count: 0,
    dead_letter_outbox_count: 0,
    unresolved_conflict_count: 0,
    media_pending_count: 0,
    media_failed_count: 0,
    media_download_pending_count: 0,
    media_download_failed_count: 0,
    consecutive_failures: 0,
    last_error: null,
    ...overrides,
  };
}

async function emit(next: SyncWorkerStatus) {
  await act(async () => {
    for (const handler of channel.handlers) handler({ payload: next });
  });
}

const NOW = Date.parse("2026-10-29T12:00:00Z");

beforeEach(() => {
  mockInvoke.mockReset();
  channel.handlers.length = 0;
  channel.unlisten.mockClear();
  vi.useFakeTimers({ toFake: ["setInterval", "clearInterval", "Date"], now: NOW });
});

afterEach(() => {
  vi.useRealTimers();
});

describe("useSyncStatus", () => {
  it("subscribes once, follows the event, and releases the subscription on unmount", async () => {
    mockInvoke.mockImplementation(ipcAnswering({ get_sync_worker_status: () => status() }));
    const { result, unmount } = renderHook(() => useSyncStatus());
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(result.current.status?.last_success_at).toBe("2026-10-29T11:55:00Z");
    expect(channel.handlers).toHaveLength(1);

    await emit(status({ last_success_at: "2026-10-29T11:59:00Z" }));
    expect(result.current.status?.last_success_at).toBe("2026-10-29T11:59:00Z");

    unmount();
    expect(channel.unlisten).toHaveBeenCalledTimes(1);
    expect(channel.handlers).toHaveLength(1);
  });
});

describe("Mes OT", () => {
  function handlers(onList: () => void) {
    return ipcAnswering({
      get_sync_worker_status: () => status(),
      list_wo: () => {
        onList();
        return { items: [], total: 0, hidden_outside_scope: 0, hidden_without_site: 0 };
      },
    });
  }

  it("shows how fresh the data is, and keeps it true every minute", async () => {
    mockInvoke.mockImplementation(handlers(() => {}));
    renderMobile(<WorkOrdersScreen userId={4} name="Karim" />);
    expect(await screen.findByText("Synchronisé il y a 5 min")).toBeTruthy();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(screen.getByText("Synchronisé il y a 6 min")).toBeTruthy();
  });

  it("says so when the phone never synchronised, and adds the state when the last try failed", async () => {
    mockInvoke.mockImplementation(
      ipcAnswering({
        get_sync_worker_status: () => status({ last_success_at: null }),
        list_wo: () => ({ items: [], total: 0, hidden_outside_scope: 0, hidden_without_site: 0 }),
      }),
    );
    renderMobile(<WorkOrdersScreen userId={4} name={null} />);
    expect(await screen.findByText("Jamais synchronisé")).toBeTruthy();

    await emit(status({ state: "offline" }));
    expect(screen.getByText("Synchronisé il y a 5 min · Hors ligne")).toBeTruthy();
  });

  it("reads the list again when a background sync finishes, and not for an unchanged status", async () => {
    let lists = 0;
    mockInvoke.mockImplementation(handlers(() => (lists += 1)));
    renderMobile(<WorkOrdersScreen userId={4} name="Karim" />);
    await screen.findByText("Synchronisé il y a 5 min");
    await waitFor(() => expect(lists).toBe(1));

    // Same last success (only the state moved): nothing to read again.
    await emit(status({ state: "running" }));
    expect(lists).toBe(1);

    await emit(status({ last_success_at: "2026-10-29T11:59:30Z" }));
    await waitFor(() => expect(lists).toBe(2));
    expect(screen.getByText("Synchronisé à l'instant")).toBeTruthy();
  });
});

describe("Signaler", () => {
  function handlers(onLists: () => void) {
    return ipcAnswering({
      get_sync_worker_status: () => status(),
      list_reference_domains: () => [],
      list_urgency_levels: () => {
        onLists();
        return [];
      },
    });
  }

  it("reads its lists again after a background sync without losing what was typed", async () => {
    let reads = 0;
    mockInvoke.mockImplementation(handlers(() => (reads += 1)));
    renderMobile(<ReportFaultScreen userId={12} />);
    expect(await screen.findByText("Synchronisé il y a 5 min")).toBeTruthy();

    const subject = await screen.findByLabelText("Objet");
    fireEvent.change(subject, { target: { value: "Fuite pompe" } });
    fireEvent.click(screen.getByLabelText("Sécurité des personnes"));
    await waitFor(() => expect(reads).toBe(1));

    await emit(status({ last_success_at: "2026-10-29T11:59:30Z" }));
    await waitFor(() => expect(reads).toBe(2));

    // The same field is still on screen (never replaced by a spinner), with its text and toggle.
    expect(screen.getByLabelText("Objet")).toBe(subject);
    expect((subject as HTMLInputElement).value).toBe("Fuite pompe");
    expect((screen.getByLabelText("Sécurité des personnes") as HTMLInputElement).checked).toBe(
      true,
    );
    expect(screen.getByText("Synchronisé à l'instant")).toBeTruthy();
  });
});

async function settle() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
}

const EMPTY_PAGE = { items: [], total: 0, hidden_outside_scope: 0, hidden_without_site: 0 };

describe("failed and interrupted reads", () => {
  it("Mes OT keeps its list when a background read fails", async () => {
    let calls = 0;
    mockInvoke.mockImplementation(
      ipcAnswering({
        get_sync_worker_status: () => status(),
        list_wo: () => {
          calls += 1;
          if (calls > 1) throw new Error("boom");
          return EMPTY_PAGE;
        },
      }),
    );
    renderMobile(<WorkOrdersScreen userId={4} name="Karim" />);
    await screen.findByText("Rien à faire pour l'instant");
    await emit(status({ last_success_at: "2026-10-29T11:59:30Z" }));
    await settle();
    expect(calls).toBe(2);
    expect(screen.getByText("Rien à faire pour l'instant")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Réessayer" })).toBeNull();
  });

  it("Mes OT ignores a read that ends after the screen is gone", async () => {
    vi.spyOn(window, "scrollTo").mockImplementation(() => {});
    const errors = vi.spyOn(console, "error").mockImplementation(() => {});
    let release: (page: unknown) => void = () => {};
    mockInvoke.mockImplementation(
      ipcAnswering({
        get_sync_worker_status: () => status(),
        list_wo: () => new Promise((resolve) => (release = resolve)),
      }),
    );
    const { unmount } = renderMobile(<WorkOrdersScreen userId={4} name="Karim" />);
    await settle();
    unmount();
    release(EMPTY_PAGE);
    await settle();
    expect(errors).not.toHaveBeenCalled();
    errors.mockRestore();
    vi.mocked(window.scrollTo).mockRestore();
  });

  it("Mes OT: the first status is a baseline, a first-ever success afterwards is a change", async () => {
    let lists = 0;
    mockInvoke.mockImplementation(
      ipcAnswering({
        get_sync_worker_status: () => status({ last_success_at: null }),
        list_wo: () => {
          lists += 1;
          return EMPTY_PAGE;
        },
      }),
    );
    renderMobile(<WorkOrdersScreen userId={4} name="Karim" />);
    await screen.findByText("Jamais synchronisé");
    await settle();
    expect(lists).toBe(1);
    await emit(status({ last_success_at: "2026-10-29T11:59:30Z" }));
    await settle();
    expect(lists).toBe(2);
  });

  it("Signaler keeps its form when a background read fails", async () => {
    let reads = 0;
    mockInvoke.mockImplementation(
      ipcAnswering({
        get_sync_worker_status: () => status(),
        list_reference_domains: () => [],
        list_urgency_levels: () => {
          reads += 1;
          if (reads > 1) throw new Error("boom");
          return [];
        },
      }),
    );
    renderMobile(<ReportFaultScreen userId={12} />);
    const subject = await screen.findByLabelText("Objet");
    fireEvent.change(subject, { target: { value: "Fuite pompe" } });
    await emit(status({ last_success_at: "2026-10-29T11:59:30Z" }));
    await settle();
    expect(reads).toBe(2);
    expect(screen.getByLabelText("Objet")).toBe(subject);
    expect((subject as HTMLInputElement).value).toBe("Fuite pompe");
    expect(screen.queryByRole("button", { name: "Réessayer" })).toBeNull();
  });

  it("Signaler shows the error, not an endless spinner, when a sync interrupts the first load and the re-read fails", async () => {
    let reads = 0;
    mockInvoke.mockImplementation(
      ipcAnswering({
        get_sync_worker_status: () => status(),
        list_reference_domains: () => [],
        list_urgency_levels: () => {
          reads += 1;
          if (reads === 1) return new Promise(() => {});
          throw new Error("boom");
        },
      }),
    );
    renderMobile(<ReportFaultScreen userId={12} />);
    await settle();
    expect(reads).toBe(1);
    expect(screen.getByText("Chargement…")).toBeTruthy();

    await emit(status({ last_success_at: "2026-10-29T11:59:30Z" }));
    await settle();
    expect(reads).toBe(2);
    expect(screen.queryByText("Chargement…")).toBeNull();
    expect(screen.getByRole("button", { name: "Réessayer" })).toBeTruthy();
  });
});
