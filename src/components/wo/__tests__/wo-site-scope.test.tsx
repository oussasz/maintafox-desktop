/**
 * Site-scoped roles on the work-order screens: the list says how many work orders it leaves out
 * and why (FR and EN), a work order recorded without a site is flagged "to review", and the row
 * menu offers the actions the user holds at the work order's site — not a right held on another
 * site. A work order without a site is answered with the plant-wide rights only (node 0).
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enOt from "@/i18n/locale-data/en/ot.json";
import frOt from "@/i18n/locale-data/fr/ot.json";
import type { WorkOrder } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

const lang = vi.hoisted(() => ({ current: "en" as "en" | "fr" }));

function lookup(catalog: unknown, key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      catalog,
    );
  return typeof value === "string" ? value : undefined;
}

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) => {
      const catalog = lang.current === "fr" ? frOt : enOt;
      const count = typeof opts?.["count"] === "number" ? opts["count"] : undefined;
      const plural =
        count === undefined
          ? undefined
          : lookup(
              catalog,
              `${key}_${count === 1 || (lang.current === "fr" && count === 0) ? "one" : "other"}`,
            );
      const value = plural ?? lookup(catalog, key);
      if (value == null) throw new Error(`missing ${lang.current} ot i18n key: ${key}`);
      return value.replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts?.[n] ?? ""));
    },
    i18n: { language: lang.current },
  }),
}));

// Rights per org node, as `get_my_access_at` answers them (node 0: the plant-wide rights).
const rightsAt = vi.hoisted(() => ({
  byNode: new Map<number, string[]>(),
  asked: [] as (number | null | undefined)[],
}));
vi.mock("@/hooks/use-access-at", () => ({
  useAccessAt: (node: number | null | undefined) => {
    rightsAt.asked.push(node);
    const granted = node == null ? [] : (rightsAt.byNode.get(node) ?? []);
    return { loading: false, can: (p: string) => granted.includes(p) };
  },
}));
// The "held anywhere" set must not be what the row menu reads.
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => {
    throw new Error("the row menu must use the rights at the work order's site");
  },
}));

import { WoContextMenu } from "../WoContextMenu";
import { WoHiddenNotice, WoNoSiteBadge, woAccessNode, woWithoutSite } from "../WoSiteScope";

function workOrder(overrides: Partial<WorkOrder> = {}): WorkOrder {
  return {
    id: 1,
    code: "OT-0001",
    entity_id: 5,
    status_code: "draft",
    ...overrides,
  } as unknown as WorkOrder;
}

describe("hidden work orders notice", () => {
  beforeEach(() => {
    lang.current = "en";
  });

  it("says how many work orders are left out and why, in English", () => {
    render(<WoHiddenNotice outsideScope={3} withoutSite={1} />);
    const notice = screen.getByRole("status");
    expect(notice).toHaveTextContent("3 work orders are not shown: outside your sites.");
    expect(notice).toHaveTextContent("1 work order is not shown: it has no site.");
    expect(notice).toHaveTextContent(
      "These counts cover all work orders, whatever the filters or search.",
    );
    expect(notice).toHaveTextContent("Ask a user with a plant-wide role to see them.");
  });

  it("says it in French", () => {
    lang.current = "fr";
    render(<WoHiddenNotice outsideScope={3} withoutSite={2} />);
    const notice = screen.getByRole("status");
    expect(notice).toHaveTextContent("3 OT ne sont pas affichés : hors de vos sites.");
    expect(notice).toHaveTextContent("2 OT ne sont pas affichés : ils n’ont pas de site.");
    expect(notice).toHaveTextContent(
      "Ces nombres portent sur tous les OT, quels que soient les filtres ou la recherche.",
    );
  });

  it("shows only the reason that applies, and nothing for a plant-wide user", () => {
    const { rerender } = render(<WoHiddenNotice outsideScope={0} withoutSite={4} />);
    expect(screen.getByRole("status")).not.toHaveTextContent("outside your sites");
    rerender(<WoHiddenNotice outsideScope={0} withoutSite={0} />);
    expect(screen.queryByRole("status")).toBeNull();
  });
});

describe("work order without a site", () => {
  beforeEach(() => {
    lang.current = "en";
  });

  it("is flagged to review; a work order with a site is not", () => {
    expect(woWithoutSite({ entity_id: null })).toBe(true);
    expect(woWithoutSite({ entity_id: 0 })).toBe(true);
    expect(woWithoutSite({ entity_id: 9 })).toBe(false);
    const { rerender } = render(<WoNoSiteBadge wo={{ entity_id: null }} />);
    expect(screen.getByText("To review: no site")).toBeInTheDocument();
    rerender(<WoNoSiteBadge wo={{ entity_id: 9 }} />);
    expect(screen.queryByText("To review: no site")).toBeNull();
  });

  it("has a French badge", () => {
    lang.current = "fr";
    render(<WoNoSiteBadge wo={{ entity_id: null }} />);
    expect(screen.getByText("À revoir : sans site")).toBeInTheDocument();
  });

  it("is checked against the plant-wide rights only (node 0)", () => {
    expect(woAccessNode({ entity_id: 9 })).toBe(9);
    expect(woAccessNode({ entity_id: null })).toBe(0);
    expect(woAccessNode(null)).toBeNull();
  });
});

describe("row menu actions", () => {
  beforeEach(() => {
    lang.current = "en";
    rightsAt.asked = [];
    rightsAt.byNode = new Map([
      [5, [P.OT_VIEW, P.OT_EDIT, P.OT_CLOSE]],
      [6, [P.OT_VIEW, P.OT_CLOSE]],
      [7, [P.OT_VIEW, P.OT_EDIT]],
      [9, [P.OT_VIEW]],
      [0, [P.OT_VIEW]],
    ]);
  });

  function openMenu(wo: WorkOrder) {
    render(
      <WoContextMenu wo={wo} onViewDetail={vi.fn()} onEdit={vi.fn()} onCancel={vi.fn()}>
        <button type="button">actions</button>
      </WoContextMenu>,
    );
    // No rights lookup per row: only when the menu opens.
    expect(rightsAt.asked).toEqual([]);
    fireEvent.keyDown(screen.getByRole("button", { name: "actions" }), { key: "Enter" });
  }

  it("offers edit and cancel where the user edits, from the rights at the work order's site", async () => {
    openMenu(workOrder({ entity_id: 5 }));
    expect(await screen.findByText(enOt.contextMenu.edit)).toBeInTheDocument();
    expect(screen.getByText(enOt.contextMenu.cancel)).toBeInTheDocument();
    expect(rightsAt.asked).toContain(5);
  });

  it("does not offer them on a work order of a site where the user only views", async () => {
    openMenu(workOrder({ entity_id: 9 }));
    expect(await screen.findByText(enOt.contextMenu.viewDetail)).toBeInTheDocument();
    expect(screen.queryByText(enOt.contextMenu.edit)).toBeNull();
    expect(screen.queryByText(enOt.contextMenu.cancel)).toBeNull();
    expect(rightsAt.asked).toContain(9);
  });

  it("offers cancel on the edit right (the backend's check), not on the close right", async () => {
    openMenu(workOrder({ entity_id: 7 }));
    expect(await screen.findByText(enOt.contextMenu.cancel)).toBeInTheDocument();
  });

  it("does not offer cancel to a user who may close but not edit", async () => {
    openMenu(workOrder({ entity_id: 6 }));
    expect(await screen.findByText(enOt.contextMenu.viewDetail)).toBeInTheDocument();
    expect(screen.queryByText(enOt.contextMenu.cancel)).toBeNull();
  });

  it("answers a work order without a site with the plant-wide rights", async () => {
    openMenu(workOrder({ entity_id: null }));
    expect(await screen.findByText(enOt.contextMenu.viewDetail)).toBeInTheDocument();
    expect(screen.queryByText(enOt.contextMenu.edit)).toBeNull();
    expect(rightsAt.asked).toContain(0);
    expect(rightsAt.asked).not.toContain(null);
  });
});
