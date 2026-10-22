/**
 * A new PC joins its plant before any first administrator (CMMS Advisor decision 2026-10-21): the
 * first-start screen asks the sync worker for a pull, shows "joining" while it runs, sends to
 * sign-in when accounts arrive, asks for a connection (with a retry) when the pull cannot complete,
 * and shows the creation form only once the backend reports that the server confirmed an empty
 * organisation (`get_activation_bootstrap_state`, `plant_join`).
 */
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useSyncOrchestratorStore } from "@/stores/sync-orchestrator-store";
import type { SyncWorkerStatus } from "@shared/ipc-types";

import { SetupInitialAdminPage } from "../SetupInitialAdminPage";

const ipc = vi.hoisted(() => ({
  plantJoin: "joining" as string | null,
  joinRefused: null as unknown,
  calls: [] as string[],
}));

// The IPC boundary: the page and the product-licence service's schemas run for real.
vi.mock("@/lib/ipc-invoke", () => ({
  invoke: vi.fn((cmd: string) => {
    ipc.calls.push(cmd);
    switch (cmd) {
      case "get_activation_bootstrap_state":
        return Promise.resolve({
          tenant_id: "tenant-plant",
          company_display_name: null,
          has_tenant_admin: false,
          plant_join: ipc.plantJoin,
        });
      case "request_plant_join":
        return ipc.joinRefused ? Promise.reject(ipc.joinRefused) : Promise.resolve(null);
      default:
        return Promise.resolve(null);
    }
  }),
}));

vi.mock("@/hooks/use-session", () => ({
  useSession: () => ({ isLoading: false, error: null, errorCode: null, refresh: vi.fn() }),
}));

// A real store with the shape the page reads: a test pushes worker statuses into it.
const sync = vi.hoisted(() => ({ initialize: vi.fn(), shutdown: vi.fn() }));
vi.mock("@/stores/sync-orchestrator-store", async () => {
  const { create } = await import("zustand");
  return {
    useSyncOrchestratorStore: create(() => ({
      worker: null as unknown,
      initialize: sync.initialize,
      shutdown: sync.shutdown,
    })),
  };
});

function workerStatus(state: SyncWorkerStatus["state"]): SyncWorkerStatus {
  return {
    state,
    blocker: null,
    paused_until: null,
    last_success_at: null,
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
  };
}

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "fr", changeLanguage: vi.fn() },
  }),
}));

function renderSetup() {
  return render(
    <MemoryRouter initialEntries={["/admin-setup"]}>
      <Routes>
        <Route path="/admin-setup" element={<SetupInitialAdminPage />} />
        <Route path="/login" element={<p>sign-in-screen</p>} />
      </Routes>
    </MemoryRouter>,
  );
}

describe("SetupInitialAdminPage: joining the plant first", () => {
  beforeEach(() => {
    ipc.calls.length = 0;
    ipc.joinRefused = null;
    sync.initialize.mockClear();
    sync.shutdown.mockClear();
    useSyncOrchestratorStore.setState({ worker: null });
  });

  it("asks for a pull and shows no creation form while joining", async () => {
    ipc.plantJoin = "joining";
    renderSetup();
    expect(await screen.findByText("setupAdmin.join.title")).toBeInTheDocument();
    expect(screen.getByText("setupAdmin.join.progress")).toBeInTheDocument();
    expect(screen.queryByLabelText("setupAdmin.username.label")).not.toBeInTheDocument();
    await waitFor(() => expect(ipc.calls).toContain("request_plant_join"));
    expect(sync.initialize).toHaveBeenCalled();
  });

  it("sends to sign-in when the plant's accounts arrived", async () => {
    ipc.plantJoin = "sign_in";
    renderSetup();
    expect(await screen.findByText("sign-in-screen")).toBeInTheDocument();
  });

  it("asks for a connection and retries the pull, never offering the form", async () => {
    ipc.plantJoin = "connection_needed";
    renderSetup();
    const retry = await screen.findByRole("button", { name: /setupAdmin\.join\.retry/ });
    expect(screen.getByText("setupAdmin.join.connectionTitle")).toBeInTheDocument();
    expect(screen.queryByLabelText("setupAdmin.username.label")).not.toBeInTheDocument();
    await waitFor(() => expect(ipc.calls).toContain("get_activation_bootstrap_state"));

    // Retry asks the worker, then shows what the backend answers (never "joining" by itself).
    ipc.calls.length = 0;
    ipc.plantJoin = "joining";
    fireEvent.click(retry);
    await waitFor(() =>
      expect(ipc.calls).toEqual(["request_plant_join", "get_activation_bootstrap_state"]),
    );
    expect(await screen.findByText("setupAdmin.join.title")).toBeInTheDocument();
  });

  it("reads the state again on every worker status", async () => {
    ipc.plantJoin = "joining";
    renderSetup();
    expect(await screen.findByText("setupAdmin.join.title")).toBeInTheDocument();
    await waitFor(() => expect(ipc.calls).toContain("get_activation_bootstrap_state"));

    // The pull completed on an empty organisation: the worker goes idle.
    ipc.plantJoin = "create_first_admin";
    ipc.calls.length = 0;
    act(() => useSyncOrchestratorStore.setState({ worker: workerStatus("idle") }));
    expect(await screen.findByLabelText("setupAdmin.username.label")).toBeInTheDocument();
    expect(ipc.calls).toContain("get_activation_bootstrap_state");
  });

  it("shows the creation form once the server confirmed an empty organisation", async () => {
    ipc.plantJoin = "create_first_admin";
    renderSetup();
    expect(await screen.findByText("setupAdmin.title")).toBeInTheDocument();
    expect(screen.getByLabelText("setupAdmin.username.label")).toBeInTheDocument();
    expect(screen.queryByTestId("plant-join")).not.toBeInTheDocument();
  });
});
