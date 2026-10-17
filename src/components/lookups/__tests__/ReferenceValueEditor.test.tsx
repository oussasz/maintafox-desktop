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
  const find = (k: string) =>
    k
      .split(".")
      .reduce<unknown>(
        (node, part) => (node as Record<string, unknown> | undefined)?.[part],
        enReference,
      );
  let value = find(key);
  if (typeof value !== "string" && typeof opts?.["count"] === "number") {
    value = find(`${key}_${opts["count"] === 1 ? "one" : "other"}`);
  }
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

const TS = "2026-10-01T10:00:00Z";
const DOMAIN_ID = 7;

const classDomain: ReferenceDomain = {
  id: DOMAIN_ID,
  code: "EQUIPMENT.CLASS",
  name: "Classes d'équipement",
  structure_type: "flat",
  governance_level: "system_seeded",
  governance_category: "controlled_catalog",
  audience: "company",
  is_extendable: false,
  validation_rules_json: null,
  created_at: TS,
  updated_at: TS,
};

function set(id: number, version_no: number, status: string): ReferenceSet {
  return {
    id,
    domain_id: DOMAIN_ID,
    version_no,
    status,
    effective_from: null,
    created_by_id: 1,
    created_at: TS,
    published_at: status === "draft" ? null : TS,
  };
}

function value(
  id: number,
  setId: number,
  code: string,
  label: string,
  system: boolean,
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
    semantic_tag: "equipment_class",
    external_code: null,
    is_active: true,
    metadata_json: system ? '{"origin":"system"}' : null,
  };
}

function caps(status: string): ReferenceGovernanceCapabilities {
  const draft = status === "draft";
  return {
    category: "controlled_catalog",
    enforcement_phase: "target",
    can_create_value: draft,
    can_update_value: draft,
    can_deactivate_value: draft,
    can_create_draft_set: true,
    can_discard_draft_set: draft,
    can_publish: true,
    can_operational_create: false,
    is_read_only: false,
    requires_analytical_protection: false,
    set_status: status,
    system_values_locked: true,
    audience: "company",
  };
}

const published = set(10, 1, "published");
const draft = set(11, 2, "draft");

function serve(sets: ReferenceSet[], overrides: Record<string, () => unknown> = {}) {
  mockInvoke.mockImplementation(async (cmd: string, args?: unknown) => {
    const override = overrides[cmd];
    if (override) return override();
    const a = (args ?? {}) as Record<string, unknown>;
    switch (cmd) {
      case "get_reference_governance_capabilities": {
        const s = sets.find((x) => x.id === a["setId"]);
        return caps(s?.status ?? "published");
      }
      case "list_reference_values":
        return a["setId"] === draft.id
          ? [
              value(21, draft.id, "PUMP", "Pompe", true),
              value(22, draft.id, "PRESS", "Presse", false),
            ]
          : [value(1, published.id, "PUMP", "Pompe", true)];
      case "list_reference_sets":
        return sets;
      case "compute_ref_publish_readiness":
        return {
          set_id: a["setId"],
          domain_id: DOMAIN_ID,
          is_ready: false,
          is_protected: false,
          issues: [{ check: "set_status", message: "draft", severity: "blocker" }],
          impact_preview_required: false,
          impact_preview_available: false,
        };
      default:
        return null;
    }
  });
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
    domains: [classDomain],
    setsMap: { [DOMAIN_ID]: [published] },
    setsLoading: {},
    selectedDomainId: DOMAIN_ID,
    selectedSetId: published.id,
  });
});

describe("ReferenceValueEditor — equipment classes (controlled catalogue)", () => {
  it("shows the real version and status, and creates a draft from the published version", async () => {
    serve([published], {
      create_draft_reference_set: () => draft,
    });
    render(<ReferenceValueEditor setId={published.id} domainId={DOMAIN_ID} />);

    expect(await screen.findByText("Pompe")).toBeInTheDocument();
    expect(screen.getAllByText("Version 1").length).toBeGreaterThan(0);
    expect(screen.getByText("Published")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Add value/ })).not.toBeInTheDocument();

    const create = await screen.findByRole("button", { name: "Create draft" });
    serve([published, draft], { create_draft_reference_set: () => draft });
    fireEvent.click(create);

    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("create_draft_reference_set", {
        domainId: DOMAIN_ID,
      }),
    );
    await waitFor(() => expect(useReferenceManagerStore.getState().selectedSetId).toBe(draft.id));
  });

  it("does not swallow a failed draft creation", async () => {
    serve([published], {
      create_draft_reference_set: () => {
        throw { code: "VALIDATION_FAILED", message: "A draft already exists for this domain." };
      },
    });
    render(<ReferenceValueEditor setId={published.id} domainId={DOMAIN_ID} />);

    fireEvent.click(await screen.findByRole("button", { name: "Create draft" }));

    expect(await screen.findByRole("alert")).toBeInTheDocument();
  });

  it("points to the draft in progress instead of offering a second one", async () => {
    useReferenceManagerStore.setState({ setsMap: { [DOMAIN_ID]: [draft, published] } });
    serve([draft, published]);
    render(<ReferenceValueEditor setId={published.id} domainId={DOMAIN_ID} />);

    const open = await screen.findByRole("button", { name: "Open version v2" });
    expect(screen.queryByRole("button", { name: "Create draft" })).not.toBeInTheDocument();
    fireEvent.click(open);
    await waitFor(() => expect(useReferenceManagerStore.getState().selectedSetId).toBe(draft.id));
  });

  it("in a draft, lets the plant edit its own classes while the product classes stay locked", async () => {
    useReferenceManagerStore.setState({
      setsMap: { [DOMAIN_ID]: [draft, published] },
      selectedSetId: draft.id,
    });
    serve([draft, published]);
    render(<ReferenceValueEditor setId={draft.id} domainId={DOMAIN_ID} />);

    const pressRow = (await screen.findByText("Presse")).closest("tr");
    const pumpRow = screen.getByText("Pompe").closest("tr");
    expect(pressRow).not.toBeNull();
    expect(pumpRow).not.toBeNull();
    expect(
      within(pressRow as HTMLElement).getByRole("button", { name: 'Edit "Presse"' }),
    ).toBeInTheDocument();
    expect(
      within(pumpRow as HTMLElement).queryByRole("button", { name: 'Edit "Pompe"' }),
    ).not.toBeInTheDocument();
    expect(
      within(pumpRow as HTMLElement).getByLabelText("Value provided by the product"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Add value/ })).toBeEnabled();
    expect(await screen.findByRole("button", { name: "Validate and publish" })).toBeInTheDocument();
    expect(screen.getByText("Draft")).toBeInTheDocument();
  });
});
