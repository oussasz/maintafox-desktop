/**
 * Dashboard links open the work-order and request lists on the same rule as the figure they
 * came from: the link's filter reaches the list query (backend rule), not a frontend look-alike.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({
    can: () => true,
    canAny: () => true,
    canAll: () => true,
    permissions: [],
    isLoading: false,
  }),
}));
// Rights at a request's site (the detail dialog's decisions); not under test here.
vi.mock("@/hooks/use-access-at", () => ({
  useAccessAt: () => ({ can: () => true, loading: false }),
  useAccessAtNodes: () => ({ canAllAny: () => true, loading: false }),
}));

// The integrity view reports the reload signal it was given (its own load is not under test).
const integrity = vi.hoisted(() => ({ signals: [] as number[] }));
vi.mock("@/components/wo/WoIntegrityWorkbench", () => ({
  WoIntegrityWorkbench: ({ reloadSignal = 0 }: { reloadSignal?: number }) => {
    integrity.signals.push(reloadSignal);
    return <p data-testid="integrity-view">{reloadSignal}</p>;
  },
}));

import { RequestsPage } from "@/pages/RequestsPage";
import { WorkOrdersPage } from "@/pages/WorkOrdersPage";
import { useDiStore } from "@/stores/di-store";
import { useWoStore } from "@/stores/wo-store";

function renderAt(path: string, page: JSX.Element) {
  return render(<MemoryRouter initialEntries={[path]}>{page}</MemoryRouter>);
}

describe("work-order list links", () => {
  beforeEach(() => {
    useWoStore.getState().setFilter({ open_only: null, not_closed_only: null, overdue_only: null });
  });

  it("scope=overdue asks the list for overdue work only", async () => {
    renderAt("/work-orders?scope=overdue", <WorkOrdersPage />);
    await waitFor(() => expect(useWoStore.getState().filter.overdue_only).toBe(true));
    expect(useWoStore.getState().filter.open_only ?? null).toBeNull();
  });

  it("scope=not_closed asks the list for work orders not yet closed", async () => {
    renderAt("/work-orders?scope=not_closed", <WorkOrdersPage />);
    await waitFor(() => expect(useWoStore.getState().filter.not_closed_only).toBe(true));
    expect(useWoStore.getState().filter.open_only ?? null).toBeNull();
    expect(useWoStore.getState().filter.overdue_only ?? null).toBeNull();
  });

  it("scope=open asks the list for open work by the count's rule", async () => {
    renderAt("/work-orders?scope=open", <WorkOrdersPage />);
    await waitFor(() => expect(useWoStore.getState().filter.open_only).toBe(true));
    expect(useWoStore.getState().filter.lifecycle_states ?? null).toBeNull();
  });
});

describe("work-order Refresh in the integrity view", () => {
  it("reloads the integrity workbench, not the hidden list", async () => {
    localStorage.setItem("wo-view-mode", "integrity");
    const loadWos = vi.fn(() => Promise.resolve());
    useWoStore.setState({ loadWos } as never);
    try {
      renderAt("/work-orders", <WorkOrdersPage />);
      expect(await screen.findByTestId("integrity-view")).toHaveTextContent("0");
      loadWos.mockClear();
      fireEvent.click(screen.getByRole("button", { name: /refresh|actualiser/i }));
      await waitFor(() => expect(screen.getByTestId("integrity-view")).toHaveTextContent("1"));
      expect(loadWos).not.toHaveBeenCalled();
    } finally {
      localStorage.removeItem("wo-view-mode");
    }
  });
});

describe("request list links", () => {
  beforeEach(() => {
    useDiStore.getState().setFilter({ open_only: null, waiting_days: null });
  });

  it("waiting=7 asks the list for requests waiting at least 7 days", async () => {
    renderAt("/requests?waiting=7", <RequestsPage />);
    await waitFor(() => expect(useDiStore.getState().filter.waiting_days).toBe(7));
  });

  it("scope=open asks the list for open requests by the count's rule", async () => {
    renderAt("/requests?scope=open", <RequestsPage />);
    await waitFor(() => expect(useDiStore.getState().filter.open_only).toBe(true));
    expect(useDiStore.getState().filter.status ?? null).toBeNull();
  });
});
