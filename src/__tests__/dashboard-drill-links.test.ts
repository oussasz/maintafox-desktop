/**
 * Every home-dashboard KPI tile opens a real screen.
 * (Audit 2026-10: six tiles pointed at /analytics/rams/mttr, /inventory/stock, … which did not exist.)
 */
import { matchRoutes } from "react-router-dom";
import { describe, expect, it } from "vitest";

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

describe("dashboard KPI drill links", () => {
  it.each(OVERVIEW_KPI_REGISTRY.map((e) => [e.semanticKey, e.drillPath] as const))(
    "%s opens an existing screen (%s)",
    (_key, drillPath) => {
      expect(resolvesToScreen(drillPath)).toBe(true);
    },
  );

  it("an unknown address lands on the not-found screen, not a raw router error", () => {
    expect(resolvesToScreen("/analytics/rams/mttr-that-does-not-exist")).toBe(false);
    const matches = matchRoutes(tree, "/no/such/screen");
    expect(matches?.[matches.length - 1]?.route.path).toBe("*");
  });
});
