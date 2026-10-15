import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ReferenceValueEditor } from "@/components/lookups/ReferenceValueEditor";
import enReference from "@/i18n/locale-data/en/reference.json";
import { useReferenceGovernanceStore } from "@/stores/reference-governance-store";
import { useReferenceManagerStore } from "@/stores/reference-manager-store";
import { mockInvoke } from "@/test/mocks/tauri";
import type {
  ReferenceDomain,
  ReferenceGovernanceCapabilities,
  ReferenceSet,
  ReferenceValue,
} from "@shared/ipc-types";

/** `reference` keys resolve against the real English catalogue: a missing key fails the test. */
function translateReference(key: string, opts?: Record<string, unknown>): string {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      enReference,
    );
  if (typeof value !== "string") {
    if (typeof opts?.["defaultValue"] === "string") return opts["defaultValue"];
    throw new Error(`missing reference i18n key: ${key}`);
  }
  return value.replace(/\{\{(\w+)\}\}/g, (_, name: string) => String(opts?.[name] ?? ""));
}

vi.mock("react-i18next", () => ({
  useTranslation: (ns?: string) => ({
    t: ns === "reference" ? translateReference : (key: string) => key,
    i18n: { language: "en", resolvedLanguage: "en" },
  }),
}));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => true, canAny: () => true, isLoading: false }),
}));

const TS = "2026-10-15T10:00:00Z";
const REQUEST_TYPES_ID = 40;
const NATURES_ID = 3;

function domain(id: number, code: string, audience: ReferenceDomain["audience"]): ReferenceDomain {
  return {
    id,
    code,
    name: code,
    structure_type: "flat",
    governance_level: "system_seeded",
    governance_category: "controlled_catalog",
    audience,
    is_extendable: audience === "company",
    validation_rules_json: null,
    created_at: TS,
    updated_at: TS,
  };
}

const requestTypes = domain(REQUEST_TYPES_ID, "DI.REQUEST_TYPE", "company");
const naturesDomain = domain(NATURES_ID, "WORK.MAINTENANCE_NATURES", "system");

function set(id: number, domainId: number, status: string): ReferenceSet {
  return {
    id,
    domain_id: domainId,
    version_no: status === "draft" ? 2 : 1,
    status,
    effective_from: null,
    created_by_id: 1,
    created_at: TS,
    published_at: status === "draft" ? null : TS,
  };
}

const draft = set(41, REQUEST_TYPES_ID, "draft");
const naturesSet = set(30, NATURES_ID, "published");

function value(
  id: number,
  setId: number,
  code: string,
  label: string,
  tag: string | null,
  meta: string | null,
): ReferenceValue {
  return {
    id,
    set_id: setId,
    parent_id: null,
    code,
    label,
    description: null,
    sort_order: id,
    color_hex: null,
    icon_name: null,
    semantic_tag: tag,
    external_code: null,
    is_active: true,
    metadata_json: meta,
  };
}

const natureValues = [
  value(
    1,
    30,
    "corrective_deferred",
    "Corrective différée",
    null,
    '{"label_en":"Deferred corrective"}',
  ),
  value(
    2,
    30,
    "preventive_predetermined",
    "Préventive systématique",
    null,
    '{"label_en":"Predetermined preventive"}',
  ),
  value(3, 30, "improvement", "Amélioration", null, '{"label_en":"Improvement / modification"}'),
];

const requestValues = [
  value(51, draft.id, "LEAK", "Fuite", null, null),
  value(52, draft.id, "RETROFIT", "Mise à niveau", "improvement", null),
];

function caps(): ReferenceGovernanceCapabilities {
  return {
    category: "controlled_catalog",
    enforcement_phase: "target",
    can_create_value: true,
    can_update_value: true,
    can_deactivate_value: true,
    can_create_draft_set: false,
    can_discard_draft_set: true,
    can_publish: true,
    can_operational_create: false,
    is_read_only: false,
    requires_analytical_protection: false,
    set_status: "draft",
    system_values_locked: true,
    audience: "company",
  };
}

function serve() {
  mockInvoke.mockImplementation(async (cmd: string, args?: unknown) => {
    const a = (args ?? {}) as Record<string, unknown>;
    switch (cmd) {
      case "get_reference_governance_capabilities":
        return caps();
      case "list_reference_domains":
        return [requestTypes, naturesDomain];
      case "list_reference_sets":
        return a["domainId"] === NATURES_ID ? [naturesSet] : [draft];
      case "list_reference_values":
        return a["setId"] === naturesSet.id ? natureValues : requestValues;
      case "update_reference_value": {
        const payload = a["payload"] as { semantic_tag?: string };
        const current = requestValues.find((v) => v.id === a["valueId"]) as ReferenceValue;
        return { ...current, semantic_tag: payload.semantic_tag || null };
      }
      case "create_reference_value": {
        const payload = a["payload"] as { code: string; label: string; semantic_tag?: string };
        return value(60, draft.id, payload.code, payload.label, payload.semantic_tag ?? null, null);
      }
      case "compute_ref_publish_readiness":
        return {
          set_id: draft.id,
          domain_id: REQUEST_TYPES_ID,
          is_ready: false,
          is_protected: false,
          issues: [],
          impact_preview_required: false,
          impact_preview_available: false,
        };
      default:
        return null;
    }
  });
}

function lastPayload(cmd: string): Record<string, unknown> {
  const call = [...mockInvoke.mock.calls].reverse().find((c) => c[0] === cmd);
  expect(call).toBeDefined();
  return (call?.[1] as { payload: Record<string, unknown> }).payload;
}

beforeEach(() => {
  mockInvoke.mockReset();
  useReferenceGovernanceStore.setState({
    values: [],
    valuesError: null,
    error: null,
    readiness: null,
    impactSummary: null,
  });
  useReferenceManagerStore.setState({
    domains: [requestTypes, naturesDomain],
    setsMap: { [REQUEST_TYPES_ID]: [draft] },
    setsLoading: {},
    selectedDomainId: REQUEST_TYPES_ID,
    selectedSetId: draft.id,
  });
  serve();
});

describe("Request types — nature of the work order (EN 13306)", () => {
  it("shows each type's nature, and the corrective fallback when none is set", async () => {
    render(<ReferenceValueEditor setId={draft.id} domainId={REQUEST_TYPES_ID} />);

    const retrofit = (await screen.findByText("Mise à niveau")).closest("tr") as HTMLElement;
    expect(await within(retrofit).findByText("Improvement / modification")).toBeInTheDocument();
    const leak = screen.getByText("Fuite").closest("tr") as HTMLElement;
    expect(within(leak).getByText(enReference.maintenanceNature.fallback)).toBeInTheDocument();
    expect(
      screen.getByRole("columnheader", { name: enReference.maintenanceNature.column }),
    ).toBeInTheDocument();
  });

  it("offers only the published natures plus the fallback, and saves the chosen nature", async () => {
    render(<ReferenceValueEditor setId={draft.id} domainId={REQUEST_TYPES_ID} />);
    const leak = (await screen.findByText("Fuite")).closest("tr") as HTMLElement;
    fireEvent.click(within(leak).getByRole("button", { name: 'Edit "Fuite"' }));

    const picker = await screen.findByRole("combobox", {
      name: enReference.maintenanceNature.column,
    });
    await waitFor(() =>
      expect(
        within(picker)
          .getAllByRole("option")
          .map((o) => o.textContent),
      ).toEqual([
        enReference.maintenanceNature.fallback,
        "Deferred corrective",
        "Predetermined preventive",
        "Improvement / modification",
      ]),
    );
    fireEvent.change(picker, { target: { value: "preventive_predetermined" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(lastPayload("update_reference_value")["semantic_tag"]).toBe(
        "preventive_predetermined",
      ),
    );
  });

  it("clears the nature back to the corrective fallback", async () => {
    render(<ReferenceValueEditor setId={draft.id} domainId={REQUEST_TYPES_ID} />);
    const retrofit = (await screen.findByText("Mise à niveau")).closest("tr") as HTMLElement;
    fireEvent.click(within(retrofit).getByRole("button", { name: 'Edit "Mise à niveau"' }));

    const picker = await screen.findByRole("combobox", {
      name: enReference.maintenanceNature.column,
    });
    expect(picker).toHaveValue("improvement");
    fireEvent.change(picker, { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    // An empty string clears the stored tag on the server; null would leave it unchanged.
    await waitFor(() => expect(lastPayload("update_reference_value")["semantic_tag"]).toBe(""));
  });

  it("creates a request type with its nature", async () => {
    render(<ReferenceValueEditor setId={draft.id} domainId={REQUEST_TYPES_ID} />);
    await screen.findByText("Fuite");
    fireEvent.click(screen.getByRole("button", { name: /Add value/ }));

    fireEvent.change(screen.getByRole("textbox", { name: "Code" }), {
      target: { value: "CALIB" },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "Label" }), {
      target: { value: "Étalonnage" },
    });
    const picker = screen.getByRole("combobox", { name: enReference.maintenanceNature.column });
    await waitFor(() => expect(within(picker).getAllByRole("option")).toHaveLength(4));
    fireEvent.change(picker, { target: { value: "preventive_predetermined" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(lastPayload("create_reference_value")).toMatchObject({
        code: "CALIB",
        label: "Étalonnage",
        semantic_tag: "preventive_predetermined",
      }),
    );
  });
});
