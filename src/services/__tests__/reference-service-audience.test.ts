/**
 * Reference domain audience over IPC (owner rule: Maintafox lists are hidden from Données de
 * référence). The service keeps returning SYSTEM domains and their values: the pickers of OT, DI,
 * RAMS… read them through the same calls; only the management screen filters them out.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  listPublishedReferenceValuesByDomainCode,
  listReferenceDomains,
  listReferenceModuleCatalogs,
} from "@/services/reference-service";

const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@/lib/ipc-invoke", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  invoke: ipc.invoke,
}));

const TS = "2026-10-15T00:00:00Z";

function domain(id: number, code: string, audience: string): Record<string, unknown> {
  return {
    id,
    code,
    name: code,
    structure_type: "flat",
    governance_level: "system_seeded",
    governance_category: "system_catalog",
    audience,
    is_extendable: false,
    validation_rules_json: null,
    created_at: TS,
    updated_at: TS,
  };
}

const STATUS = domain(3, "EQUIPMENT.STATUS", "system");
const CLASS = domain(2, "EQUIPMENT.CLASS", "company");

function value(id: number, code: string, sort: number): Record<string, unknown> {
  return {
    id,
    set_id: 30,
    parent_id: null,
    code,
    label: code,
    description: null,
    sort_order: sort,
    color_hex: null,
    icon_name: null,
    semantic_tag: null,
    external_code: null,
    is_active: true,
    metadata_json: '{"origin":"system"}',
  };
}

beforeEach(() => {
  ipc.invoke.mockReset();
});

describe("reference-service — domain audience", () => {
  it("returns SYSTEM and COMPANY domains with their audience", async () => {
    ipc.invoke.mockResolvedValueOnce([STATUS, CLASS]);

    const domains = await listReferenceDomains();

    expect(domains.map((d) => [d.code, d.audience])).toEqual([
      ["EQUIPMENT.STATUS", "system"],
      ["EQUIPMENT.CLASS", "company"],
    ]);
  });

  it("rejects a domain without a known audience", async () => {
    ipc.invoke.mockResolvedValueOnce([domain(9, "X.Y", "tenant")]);

    await expect(listReferenceDomains()).rejects.toThrow();
  });

  it("reads the module catalog classification from the backend", async () => {
    ipc.invoke.mockResolvedValueOnce([
      { code: "WO.WORK_ORDER_STATUSES", audience: "system" },
      { code: "INVENTORY.ARTICLE_FAMILY", audience: "company" },
    ]);

    const catalogs = await listReferenceModuleCatalogs();

    expect(ipc.invoke).toHaveBeenCalledWith("list_reference_module_catalogs");
    expect(catalogs).toEqual([
      { code: "WO.WORK_ORDER_STATUSES", audience: "system" },
      { code: "INVENTORY.ARTICLE_FAMILY", audience: "company" },
    ]);
  });

  it("pickers still list the values of a SYSTEM domain", async () => {
    ipc.invoke.mockImplementation((cmd: string) => {
      switch (cmd) {
        case "list_reference_domains":
          return Promise.resolve([STATUS, CLASS]);
        case "list_reference_sets":
          return Promise.resolve([
            {
              id: 30,
              domain_id: 3,
              version_no: 1,
              status: "published",
              effective_from: TS,
              created_by_id: null,
              created_at: TS,
              published_at: TS,
            },
          ]);
        case "list_reference_values":
          return Promise.resolve([value(301, "IN_STOCK", 2), value(300, "ACTIVE_IN_SERVICE", 1)]);
        default:
          return Promise.reject(new Error(`unexpected ${cmd}`));
      }
    });

    const values = await listPublishedReferenceValuesByDomainCode("equipment.status");

    expect(values.map((v) => v.code)).toEqual(["ACTIVE_IN_SERVICE", "IN_STOCK"]);
  });
});
