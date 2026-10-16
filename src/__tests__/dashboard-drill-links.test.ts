/**
 * Every home figure opens a real screen, and every query parameter it passes is one the target
 * page reads on arrival.
 * (Audit 2026-10: six tiles pointed at /analytics/rams/mttr, /inventory/stock, … which did not exist.)
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { matchRoutes } from "react-router-dom";
import { describe, expect, it } from "vitest";

import { allHomeLinks } from "@/components/dashboard/home/home-links";
import { appRouteTree } from "@/router";
import { OVERVIEW_KPI_REGISTRY } from "@shared/overview-kpi-registry";

// RouteObject vs AgnosticRouteObject differ only under exactOptionalPropertyTypes.
const tree = appRouteTree as Parameters<typeof matchRoutes>[0];

function resolvesToScreen(path: string): boolean {
  const pathname = path.split(/[?#]/)[0] ?? path;
  const matches = matchRoutes(tree, pathname);
  if (!matches || matches.length === 0) return false;
  const leaf = matches[matches.length - 1]?.route;
  return leaf != null && leaf.path !== "*";
}

/** Source of the page behind each route the home page links to (where its params are read). */
const PAGE_SOURCE: Record<string, string> = {
  "/requests": "src/pages/RequestsPage.tsx",
  "/work-orders": "src/pages/WorkOrdersPage.tsx",
  "/inspections": "src/pages/InspectionsPage.tsx",
  "/pm": "src/pages/PmPage.tsx",
  "/inventory": "src/pages/InventoryPage.tsx",
  "/permits": "src/pages/PermitsPage.tsx",
  "/equipment": "src/pages/assets/AssetRegistryPage.tsx",
};

/** How a page names a query parameter it reads (literal, or the constant that holds it). */
const PARAM_SPELLING: Record<string, string[]> = {
  queue: ['"queue"'],
  permit: ["PERMIT_QUERY_SELECT"],
};

function readsParam(path: string, param: string): boolean {
  const file = PAGE_SOURCE[path];
  if (!file) return false;
  const src = readFileSync(resolve(__dirname, "../..", file), "utf8");
  const spellings = PARAM_SPELLING[param] ?? [`"${param}"`];
  return spellings.some((s) => src.includes(s)) && src.includes("searchParams");
}

describe("dashboard KPI drill links", () => {
  it.each(OVERVIEW_KPI_REGISTRY.map((e) => [e.semanticKey, e.drillPath] as const))(
    "%s opens an existing screen (%s)",
    (_key, drillPath) => {
      expect(resolvesToScreen(drillPath)).toBe(true);
    },
  );

  it.each(allHomeLinks().map((l) => [l] as const))(
    "home link %s opens an existing screen",
    (link) => {
      expect(resolvesToScreen(link)).toBe(true);
    },
  );

  it.each(
    allHomeLinks()
      .filter((l) => l.includes("?"))
      .map((l) => [l] as const),
  )("home link %s passes a parameter its page reads", (link) => {
    const [path = "", query = ""] = link.split("?");
    for (const param of new URLSearchParams(query).keys()) {
      expect(readsParam(path, param), `${path} reads "${param}"`).toBe(true);
    }
  });

  it("an unknown address lands on the not-found screen, not a raw router error", () => {
    expect(resolvesToScreen("/analytics/rams/mttr-that-does-not-exist")).toBe(false);
    const matches = matchRoutes(tree, "/no/such/screen");
    expect(matches?.[matches.length - 1]?.route.path).toBe("*");
  });
});
