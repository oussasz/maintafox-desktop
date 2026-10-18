/**
 * Site-scoped roles on the requests screen: the list says how many requests it leaves out and
 * why (FR and EN), a request recorded without a site is flagged "to review", and the row menu
 * offers the decisions the user holds at the request's site — not a right held on another site.
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enDi from "@/i18n/locale-data/en/di.json";
import frDi from "@/i18n/locale-data/fr/di.json";
import type { InterventionRequest } from "@shared/ipc-types";
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
      const catalog = lang.current === "fr" ? frDi : enDi;
      const count = typeof opts?.["count"] === "number" ? opts["count"] : undefined;
      const plural =
        count === undefined
          ? undefined
          : lookup(
              catalog,
              `${key}_${count === 1 || (lang.current === "fr" && count === 0) ? "one" : "other"}`,
            );
      const value = plural ?? lookup(catalog, key);
      if (value == null) throw new Error(`missing ${lang.current} di i18n key: ${key}`);
      return value.replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts?.[n] ?? ""));
    },
    i18n: { language: lang.current },
  }),
}));

// Rights per org node, as `get_my_access_at` answers them.
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
// The plant-wide "held anywhere" set must not be what the row menu reads.
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => {
    throw new Error("the row menu must use the rights at the request's site");
  },
}));
vi.mock("@/hooks/use-session", () => ({
  useSession: () => ({ info: { user_id: 7, display_name: "Amina", username: "amina" } }),
}));

import { DiContextMenu } from "../DiContextMenu";
import { DiHiddenRequestsNotice, DiNoSiteBadge, diWithoutSite } from "../DiSiteScope";

function request(overrides: Partial<InterventionRequest> = {}): InterventionRequest {
  return {
    id: 1,
    code: "DI-0001",
    org_node_id: 5,
    status: "submitted",
    submitter_id: 42,
    converted_to_wo_id: null,
    archived_at: null,
    ...overrides,
  } as unknown as InterventionRequest;
}

describe("hidden requests notice", () => {
  beforeEach(() => {
    lang.current = "en";
  });

  it("says how many requests are left out and why, in English", () => {
    render(<DiHiddenRequestsNotice outsideScope={3} withoutSite={1} />);
    const notice = screen.getByRole("status");
    expect(notice).toHaveTextContent("3 requests are not shown: outside your sites.");
    expect(notice).toHaveTextContent("1 request is not shown: it has no site.");
    expect(notice).toHaveTextContent("Ask a user with a plant-wide role to see them.");
  });

  it("says it in French", () => {
    lang.current = "fr";
    render(<DiHiddenRequestsNotice outsideScope={3} withoutSite={2} />);
    const notice = screen.getByRole("status");
    expect(notice).toHaveTextContent("3 demandes ne sont pas affichées : hors de vos sites.");
    expect(notice).toHaveTextContent("2 demandes ne sont pas affichées : elles n'ont pas de site.");
  });

  it("shows only the reason that applies, and nothing for a plant-wide user", () => {
    const { rerender } = render(<DiHiddenRequestsNotice outsideScope={0} withoutSite={4} />);
    expect(screen.getByRole("status")).not.toHaveTextContent("outside your sites");
    rerender(<DiHiddenRequestsNotice outsideScope={0} withoutSite={0} />);
    expect(screen.queryByRole("status")).toBeNull();
  });
});

describe("request without a site", () => {
  beforeEach(() => {
    lang.current = "en";
  });

  it("is flagged to review; a request with a site is not", () => {
    expect(diWithoutSite({ org_node_id: 0 })).toBe(true);
    expect(diWithoutSite({ org_node_id: 9 })).toBe(false);
    const { rerender } = render(<DiNoSiteBadge di={{ org_node_id: 0 }} />);
    expect(screen.getByText("To review: no site")).toBeInTheDocument();
    rerender(<DiNoSiteBadge di={{ org_node_id: 9 }} />);
    expect(screen.queryByText("To review: no site")).toBeNull();
  });

  it("has a French badge", () => {
    lang.current = "fr";
    render(<DiNoSiteBadge di={{ org_node_id: 0 }} />);
    expect(screen.getByText("À revoir : sans site")).toBeInTheDocument();
  });
});

describe("row menu decisions", () => {
  beforeEach(() => {
    lang.current = "en";
    rightsAt.asked = [];
    rightsAt.byNode = new Map([
      [5, [P.DI_VIEW, P.DI_SCREEN, P.DI_REVIEW]],
      [9, [P.DI_VIEW]],
    ]);
  });

  function openMenu(di: InterventionRequest) {
    render(
      <DiContextMenu
        di={di}
        onViewDetail={vi.fn()}
        onEdit={vi.fn()}
        onTriage={vi.fn()}
        onApprove={vi.fn()}
        onReturn={vi.fn()}
        onClose={vi.fn()}
        onPrint={vi.fn()}
      >
        <button type="button">actions</button>
      </DiContextMenu>,
    );
    expect(rightsAt.asked).toEqual([]);
    fireEvent.keyDown(screen.getByRole("button", { name: "actions" }), { key: "Enter" });
  }

  it("offers triage where the user screens, from the rights at the request's site", async () => {
    openMenu(request({ org_node_id: 5 }));
    expect(await screen.findByText(enDi.triage.acceptForReview)).toBeInTheDocument();
    expect(rightsAt.asked).toContain(5);
  });

  it("does not offer it on a request of a site where the user only views", async () => {
    openMenu(request({ org_node_id: 9 }));
    expect(await screen.findByText(enDi.contextMenu.viewDetail)).toBeInTheDocument();
    expect(screen.queryByText(enDi.triage.acceptForReview)).toBeNull();
    expect(rightsAt.asked).toContain(9);
  });
});
