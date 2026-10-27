/**
 * The KPI registry is the only place a "stable" band and a trend sense are written; the catalog
 * records them for readers. This keeps the two from drifting apart.
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

import {
  DEFAULT_KPI_STABLE_BELOW,
  OVERVIEW_KPI_REGISTRY,
  SMRP_KPI_REGISTRY,
} from "@shared/overview-kpi-registry";

const catalog = readFileSync(
  resolve(process.cwd(), "docs/engineering/KPI_CATALOG.md"),
  "utf8",
).replace(/\r\n/g, "\n");

function section(id: string): string {
  const start = catalog.indexOf(`\n# ${id}\n`);
  expect(start, `${id} is missing from KPI_CATALOG.md`).toBeGreaterThan(-1);
  const next = catalog.indexOf("\n# KPI-", start + 5);
  return catalog.slice(start, next === -1 ? undefined : next);
}

const field = (sec: string, name: string) =>
  new RegExp(`\n${name}:\n([^\n]+)`).exec(sec)?.[1] ?? "";

describe("KPI catalog matches the registry", () => {
  const entries = [
    ...OVERVIEW_KPI_REGISTRY.map((e) => ({
      id: e.catalogId,
      band: e.stableBelow ?? DEFAULT_KPI_STABLE_BELOW,
      sense: e.trendSense,
    })),
    ...SMRP_KPI_REGISTRY.map((e) => ({
      id: e.catalogId,
      band: e.stableBelow,
      sense: e.trendSense,
    })),
  ];

  it.each(entries)("$id: stable band and trend sense", ({ id, band, sense }) => {
    const sec = section(id);
    expect(field(sec, "Stable band")).toContain(`±${band} `);
    // Some catalog entries (the Analytics hub ones) have no "Trend sense" field; when present it
    // must match the registry.
    const written = field(sec, "Trend sense");
    if (written) expect(written.startsWith(sense)).toBe(true);
  });
});
