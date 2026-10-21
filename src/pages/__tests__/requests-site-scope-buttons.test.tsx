/**
 * Site-scoped roles on the requests screen buttons: they follow what the backend checks.
 * - Bulk triage needs di.screen / di.review at the site of EVERY selected request.
 * - The SLA rules panel is a plant-wide setting: it needs di.admin plant-wide (rights at no node),
 *   not di.admin held on one site.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

// "Held anywhere": everything, so a button that read this would show for a site-only user.
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({
    can: () => true,
    canAny: () => true,
    canAll: () => true,
    permissions: [],
    isLoading: false,
  }),
}));

// Rights per org node, as `get_my_access_at` answers them (node 0 = plant-wide only).
const rights = vi.hoisted(() => ({ byNode: new Map<number, string[]>() }));
vi.mock("@/hooks/use-access-at", () => ({
  useAccessAt: (node: number | null | undefined) => {
    const granted = node == null ? [] : (rights.byNode.get(node) ?? []);
    return { loading: false, can: (p: string) => granted.includes(p) };
  },
  useAccessAtNodes: (nodes: readonly number[]) => ({
    loading: false,
    canAllAny: (perms: readonly string[]) =>
      nodes.every((n) => perms.some((p) => (rights.byNode.get(n) ?? []).includes(p))),
  }),
}));

vi.mock("@/services/di-service", async (importOriginal: () => Promise<Record<string, unknown>>) => {
  const actual = await importOriginal();
  return {
    ...actual,
    listDis: vi.fn(),
    getDiQueueCounts: vi.fn().mockResolvedValue(null),
  };
});

import { RequestsPage } from "@/pages/RequestsPage";
import * as diService from "@/services/di-service";
import { useDiStore } from "@/stores/di-store";
import type { InterventionRequest } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

function request(id: number, node: number): InterventionRequest {
  return {
    id,
    code: `DI-000${id}`,
    org_node_id: node,
    status: "submitted",
    submitter_id: 42,
    converted_to_wo_id: null,
    archived_at: null,
    row_version: 1,
    title: `Request ${id}`,
  } as unknown as InterventionRequest;
}

function renderToScreen() {
  return render(
    <MemoryRouter initialEntries={["/requests?queue=to_screen"]}>
      <RequestsPage />
    </MemoryRouter>,
  );
}

describe("requests screen buttons follow the backend's site checks", () => {
  beforeEach(() => {
    rights.byNode.clear();
    useDiStore.getState().setFilter({ open_only: null, waiting_days: null });
    vi.mocked(diService.listDis).mockResolvedValue({
      items: [request(1, 10), request(2, 20)],
      total: 2,
      hidden_outside_scope: 0,
      hidden_without_site: 0,
    });
  });

  it("enables bulk triage only when the right is held at every selected request's site", async () => {
    rights.byNode.set(10, [P.DI_SCREEN]);
    renderToScreen();
    const boxes = await screen.findAllByRole("checkbox", { name: /bulk.selectOne/ });
    expect(boxes).toHaveLength(2);

    fireEvent.click(boxes[0] as HTMLElement);
    const triage = await screen.findByRole("button", { name: /bulk\.triage|Admit|Admettre/ });
    expect(triage).toBeEnabled();

    // The second request is on a site where the user holds nothing: the whole action is refused.
    fireEvent.click(boxes[1] as HTMLElement);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /bulk\.triage|Admit|Admettre/ })).toBeDisabled(),
    );
    expect(
      screen.getByText(/bulk\.outsideYourSites|outside your sites|hors de vos sites/),
    ).toBeTruthy();

    // Holding di.review there is enough (the backend accepts screen OR review).
    rights.byNode.set(20, [P.DI_REVIEW]);
    fireEvent.click(boxes[1] as HTMLElement);
    fireEvent.click(boxes[1] as HTMLElement);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /bulk\.triage|Admit|Admettre/ })).toBeEnabled(),
    );
  });

  it("shows the SLA rules button only for di.admin held plant-wide", async () => {
    // di.admin on a site only: the backend refuses the plant-wide SLA rules.
    rights.byNode.set(10, [P.DI_ADMIN]);
    const { unmount } = renderToScreen();
    await screen.findAllByRole("checkbox", { name: /bulk.selectOne/ });
    expect(screen.queryByRole("button", { name: /sla\.title|SLA/ })).toBeNull();
    unmount();

    rights.byNode.set(0, [P.DI_ADMIN]);
    renderToScreen();
    expect(await screen.findByRole("button", { name: /sla\.title|SLA/ })).toBeInTheDocument();
  });
});
