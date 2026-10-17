/**
 * Dashboard links open the work-order and request lists on the same rule as the figure they
 * came from: the link's filter reaches the list query (backend rule), not a frontend look-alike.
 */
import { render, waitFor } from "@testing-library/react";
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

import { RequestsPage } from "@/pages/RequestsPage";
import { WorkOrdersPage } from "@/pages/WorkOrdersPage";
import { useDiStore } from "@/stores/di-store";
import { useWoStore } from "@/stores/wo-store";

function renderAt(path: string, page: JSX.Element) {
  return render(<MemoryRouter initialEntries={[path]}>{page}</MemoryRouter>);
}

describe("work-order list links", () => {
  beforeEach(() => {
    useWoStore.getState().setFilter({ open_only: null, overdue_only: null });
  });

  it("scope=overdue asks the list for overdue work only", async () => {
    renderAt("/work-orders?scope=overdue", <WorkOrdersPage />);
    await waitFor(() => expect(useWoStore.getState().filter.overdue_only).toBe(true));
    expect(useWoStore.getState().filter.open_only ?? null).toBeNull();
  });

  it("scope=open asks the list for open work by the count's rule", async () => {
    renderAt("/work-orders?scope=open", <WorkOrdersPage />);
    await waitFor(() => expect(useWoStore.getState().filter.open_only).toBe(true));
    expect(useWoStore.getState().filter.lifecycle_states ?? null).toBeNull();
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
