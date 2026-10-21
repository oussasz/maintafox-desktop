/**
 * Units of measure in Données de référence (MEASURE.UNITS) and the shared unit picker:
 * protected standard units, plant units with their base unit and factor, the "to review" filter
 * for units created by migration 201.
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  MeasureUnitSelect,
  buildPlantUnitMetadata,
  parseUnitMeta,
  unitOptionText,
} from "@/components/lookups/MeasureUnitPicker";
import { ReferenceValueEditor } from "@/components/lookups/ReferenceValueEditor";
import enReference from "@/i18n/locale-data/en/reference.json";
import frReference from "@/i18n/locale-data/fr/reference.json";
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
const DOMAIN_ID = 41;

const unitsDomain: ReferenceDomain = {
  id: DOMAIN_ID,
  code: "MEASURE.UNITS",
  name: "Unités de mesure",
  structure_type: "unit_set",
  governance_level: "tenant_managed",
  governance_category: "controlled_catalog",
  audience: "company",
  is_extendable: true,
  validation_rules_json: null,
  created_at: TS,
  updated_at: TS,
};

const draft: ReferenceSet = {
  id: 51,
  domain_id: DOMAIN_ID,
  version_no: 2,
  status: "draft",
  effective_from: null,
  created_by_id: 1,
  created_at: TS,
  published_at: null,
};

function unit(
  id: number,
  code: string,
  label: string,
  meta: Record<string, unknown>,
  extra: Partial<ReferenceValue> = {},
): ReferenceValue {
  return {
    id,
    set_id: draft.id,
    parent_id: null,
    code,
    label,
    description: null,
    sort_order: id,
    color_hex: null,
    icon_name: null,
    semantic_tag: null,
    external_code: null,
    is_active: true,
    metadata_json: JSON.stringify(meta),
    ...extra,
  };
}

const kg = unit(
  1,
  "KG",
  "Kilogramme",
  { origin: "system", symbol: "kg", label_en: "Kilogram" },
  { semantic_tag: "mass", external_code: "KGM" },
);
const rpm = unit(
  2,
  "RPM",
  "Tour par minute",
  { origin: "system", symbol: "tr/min", label_en: "Revolution per minute" },
  { semantic_tag: "frequency", external_code: "M46" },
);
const sac = unit(
  3,
  "SAC",
  "Sac",
  { symbol: "sac", label_en: "Bag", base_factor: 25 },
  { parent_id: kg.id, semantic_tag: "mass" },
);
const tr = unit(4, "TR", "tr", { origin: "migrated", symbol: "tr", review_required: true });

const caps: ReferenceGovernanceCapabilities = {
  category: "controlled_catalog",
  enforcement_phase: "target",
  can_create_value: true,
  can_update_value: true,
  can_deactivate_value: true,
  can_create_draft_set: true,
  can_discard_draft_set: true,
  can_publish: true,
  can_operational_create: false,
  is_read_only: false,
  requires_analytical_protection: false,
  set_status: "draft",
  system_values_locked: true,
  audience: "company",
};

function serve(overrides: Record<string, (args: Record<string, unknown>) => unknown> = {}) {
  mockInvoke.mockImplementation(async (cmd: string, args?: unknown) => {
    const a = (args ?? {}) as Record<string, unknown>;
    const override = overrides[cmd];
    if (override) return override(a);
    switch (cmd) {
      case "get_reference_governance_capabilities":
        return caps;
      case "list_reference_values":
        return [kg, rpm, sac, tr];
      case "list_reference_sets":
        return [draft];
      case "list_reference_domains":
        return [unitsDomain];
      case "compute_ref_publish_readiness":
        return {
          set_id: draft.id,
          domain_id: DOMAIN_ID,
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

beforeEach(() => {
  mockInvoke.mockReset();
  useReferenceGovernanceStore.setState({
    values: [],
    valuesError: null,
    error: null,
    readiness: null,
    impactSummary: null,
    editingValueId: null,
    newValueDraft: null,
  });
  useReferenceManagerStore.setState({
    domains: [unitsDomain],
    setsMap: { [DOMAIN_ID]: [draft] },
    setsLoading: {},
    selectedDomainId: DOMAIN_ID,
    selectedSetId: draft.id,
  });
});

describe("unit helpers", () => {
  it("reads and writes the unit metadata, dropping the review flag", () => {
    const meta = parseUnitMeta(tr.metadata_json);
    expect(meta.reviewRequired).toBe(true);
    expect(meta.origin).toBe("migrated");
    const written = JSON.parse(
      buildPlantUnitMetadata(tr.metadata_json, { symbol: " tr ", labelEn: "Turn", baseFactor: 1 }),
    ) as Record<string, unknown>;
    expect(written).toEqual({ origin: "migrated", symbol: "tr", label_en: "Turn", base_factor: 1 });
    expect(unitOptionText(kg, "fr")).toBe("Kilogramme (kg)");
    expect(unitOptionText(kg, "en")).toBe("Kilogram (kg)");
  });

  it("keeps a retired unit visible in the picker", () => {
    const onChange = vi.fn();
    render(
      <MeasureUnitSelect
        ariaLabel="Unit"
        value={99}
        units={[kg]}
        onChange={onChange}
        currentText="gal"
        allowEmpty
      />,
    );
    const select = screen.getByRole("combobox", { name: "Unit" });
    expect(within(select).getByRole("option", { name: "No unit" })).toBeInTheDocument();
    expect(
      within(select).getByRole("option", { name: "gal (retired from reference data)" }),
    ).toBeInTheDocument();
    fireEvent.change(select, { target: { value: String(kg.id) } });
    expect(onChange).toHaveBeenCalledWith(kg.id);
  });

  it("has the same unit strings in French and English", () => {
    expect(Object.keys(frReference.units).sort()).toEqual(Object.keys(enReference.units).sort());
  });
});

describe("ReferenceValueEditor — units of measure", () => {
  it("shows symbols, base factors, protected standard units and filters the units to review", async () => {
    serve();
    render(<ReferenceValueEditor setId={draft.id} domainId={DOMAIN_ID} />);

    const sacRow = (await screen.findByText("Sac")).closest("tr") as HTMLElement;
    expect(within(sacRow).getByText("1 sac = 25 kg")).toBeInTheDocument();
    const kgRow = screen.getByText("Kilogramme").closest("tr") as HTMLElement;
    expect(within(kgRow).getByText("Standard unit (protected)")).toBeInTheDocument();
    expect(within(kgRow).getByText("KGM")).toBeInTheDocument();
    expect(within(kgRow).queryByRole("button", { name: 'Edit "Kilogramme"' })).toBeNull();
    const trRow = screen.getByText("tr", { selector: "span.min-w-0" }).closest("tr") as HTMLElement;
    expect(within(trRow).getByText("To review")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "To review (1)" }));
    await waitFor(() => expect(screen.queryByText("Sac")).toBeNull());
    expect(screen.getByRole("button", { name: "To review (1)" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("creates a plant unit with its base standard unit and factor", async () => {
    const created = unit(
      5,
      "FUT",
      "Fût",
      { symbol: "fût", label_en: "Drum", base_factor: 200 },
      { parent_id: kg.id },
    );
    serve({ create_reference_value: () => created });
    render(<ReferenceValueEditor setId={draft.id} domainId={DOMAIN_ID} />);
    await screen.findByText("Sac");

    fireEvent.click(screen.getByRole("button", { name: /Add value/ }));
    const row = screen.getByText("New").closest("tr") as HTMLElement;
    fireEvent.change(within(row).getByLabelText("Code"), { target: { value: "FUT" } });
    fireEvent.change(within(row).getByLabelText("Label"), { target: { value: "Fût" } });
    fireEvent.change(within(row).getByLabelText("Symbol"), { target: { value: "fût" } });
    fireEvent.change(within(row).getByLabelText("English label"), { target: { value: "Drum" } });
    const save = within(row).getByRole("button", { name: "Save" });
    expect(save).toBeDisabled();
    fireEvent.change(within(row).getByLabelText("Factor"), { target: { value: "200" } });
    // Only active standard units are offered as a base.
    const base = within(row).getByLabelText("Base standard unit");
    expect(within(base).queryByRole("option", { name: "Bag (sac)" })).toBeNull();
    fireEvent.change(base, { target: { value: String(kg.id) } });
    expect(save).toBeEnabled();
    fireEvent.click(save);

    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("create_reference_value", {
        payload: {
          set_id: draft.id,
          parent_id: kg.id,
          code: "FUT",
          label: "Fût",
          description: null,
          metadata_json: JSON.stringify({ label_en: "Drum", symbol: "fût", base_factor: 200 }),
        },
      }),
    );
  });

  it("completes a unit to review: base first, then the factor, and the flag goes", async () => {
    serve({
      move_reference_value_parent: () => ({ ...tr, parent_id: rpm.id }),
      update_reference_value: () => ({ ...tr, parent_id: rpm.id, semantic_tag: "frequency" }),
    });
    render(<ReferenceValueEditor setId={draft.id} domainId={DOMAIN_ID} />);
    const trRow = (await screen.findByText("tr", { selector: "span.min-w-0" })).closest(
      "tr",
    ) as HTMLElement;
    fireEvent.click(within(trRow).getByRole("button", { name: 'Edit "tr"' }));

    const editing = screen.getByLabelText("Factor").closest("tr") as HTMLElement;
    fireEvent.change(within(editing).getByLabelText("Base standard unit"), {
      target: { value: String(rpm.id) },
    });
    fireEvent.change(within(editing).getByLabelText("Factor"), { target: { value: "1" } });
    fireEvent.click(within(editing).getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("update_reference_value", {
        valueId: tr.id,
        payload: {
          label: "tr",
          description: null,
          metadata_json: JSON.stringify({
            origin: "migrated",
            label_en: "",
            symbol: "tr",
            base_factor: 1,
          }),
        },
      }),
    );
    const calls = mockInvoke.mock.calls.map((c) => c[0]);
    expect(calls.indexOf("move_reference_value_parent")).toBeLessThan(
      calls.indexOf("update_reference_value"),
    );
    expect(mockInvoke).toHaveBeenCalledWith("move_reference_value_parent", {
      valueId: tr.id,
      newParentId: rpm.id,
    });
  });
});
