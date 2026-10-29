import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import { mockInvoke } from "@/test/mocks/tauri";

import { ReportFaultScreen } from "../screens/ReportFaultScreen";

import { ipcAnswering, renderMobile } from "./helpers";

// Only the IPC boundary is stubbed: the engine's answers for the lists the form reads. The
// service wrappers, their schemas and the form are the real ones.

const DOMAINS: Record<string, number> = {
  "DI.ORIGIN": 1,
  "DI.REQUEST_TYPE": 2,
  "WORK.SYMPTOMS": 3,
  "DI.IMPACT_LEVEL": 4,
};

function domain(code: string, id: number) {
  return {
    id,
    code,
    name: code,
    structure_type: "flat",
    governance_level: "plant",
    governance_category: "B",
    audience: "company",
    is_extendable: true,
    validation_rules_json: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  };
}

function refValue(setId: number, id: number, code: string, label: string) {
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
    semantic_tag: null,
    external_code: null,
    is_active: true,
    metadata_json: null,
  };
}

const VALUES: Record<number, ReturnType<typeof refValue>[]> = {
  // set id = domain id * 10
  10: [refValue(10, 101, "operator", "Opérateur")],
  20: [refValue(20, 201, "corrective", "Dépannage")],
  30: [refValue(30, 301, "LEAK", "Fuite")],
  40: [refValue(40, 401, "major", "Majeur")],
};

const ASSET = {
  id: 77,
  entity_sync_id: "ast-77",
  asset_code: "PMP-001",
  asset_name: "Pompe principale",
  class_code: null,
  class_name: null,
  family_code: null,
  family_name: null,
  subfamily_code: null,
  subfamily_name: null,
  criticality_code: null,
  criticality_to_review: false,
  status_code: "ACTIVE",
  org_node_id: 5,
  org_node_name: "Atelier 1",
  org_path: null,
  parent_asset_id: null,
  parent_asset_code: null,
  parent_asset_name: null,
  primary_meter_name: null,
  primary_meter_reading: null,
  primary_meter_unit: null,
  primary_meter_last_read_at: null,
  external_id_count: 0,
  row_version: 1,
};

const URGENCY = {
  id: 9,
  code: "urgent",
  rank: 2,
  label_fr: "Urgent",
  label_en: "Urgent (en)",
  description: null,
  color_hex: null,
  response_target_hours: null,
  completion_target_hours: null,
  escalation_threshold_hours: null,
  targets_source: "starter",
};

function createdDi(input: Record<string, unknown>) {
  const nullables = [
    "sub_asset_ref",
    "validated_urgency",
    "observed_at",
    "reviewer_id",
    "screened_at",
    "approved_at",
    "deferred_until",
    "declined_at",
    "closed_at",
    "archived_at",
    "converted_to_wo_id",
    "converted_at",
    "reviewer_note",
    "classification_code_id",
    "recurrence_di_id",
  ];
  return {
    ...Object.fromEntries(nullables.map((k) => [k, null])),
    ...input,
    id: 1,
    code: "DI-0042",
    status: "submitted",
    submitted_at: "2026-10-29T08:00:00Z",
    is_recurrence_flag: false,
    row_version: 1,
    created_at: "2026-10-29T08:00:00Z",
    updated_at: "2026-10-29T08:00:00Z",
  };
}

function handlers(onCreate: (input: Record<string, unknown>) => void) {
  return ipcAnswering({
    list_reference_domains: () => Object.entries(DOMAINS).map(([code, id]) => domain(code, id)),
    list_reference_sets: (args) => [
      {
        id: Number(args?.["domainId"]) * 10,
        domain_id: args?.["domainId"],
        version_no: 1,
        status: "published",
        effective_from: null,
        created_by_id: null,
        created_at: "2026-01-01T00:00:00Z",
        published_at: "2026-01-01T00:00:00Z",
      },
    ],
    list_reference_values: (args) => VALUES[Number(args?.["setId"])] ?? [],
    list_urgency_levels: () => [URGENCY],
    search_assets: () => [ASSET],
    create_di: (args) => {
      const input = (args as { input: Record<string, unknown> }).input;
      onCreate(input);
      return createdDi(input);
    },
  });
}

/** Opens a picker row (its name starts with its label) and taps one option of its sheet. */
async function pick(label: string, option: string) {
  fireEvent.click(await screen.findByRole("button", { name: new RegExp(`^${label}:`) }));
  fireEvent.click(await screen.findByRole("radio", { name: option }));
}

/** Opens the equipment sheet and taps the engine's match. */
async function pickEquipment(code: RegExp) {
  fireEvent.click(await screen.findByRole("button", { name: "Choisir l'équipement" }));
  fireEvent.click(await screen.findByRole("button", { name: code }));
}

describe("report a fault (DI form)", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("offers the engine's lists and submits the chosen values through create_di", async () => {
    let created: Record<string, unknown> | null = null;
    mockInvoke.mockImplementation(
      handlers((input) => {
        created = input;
      }),
    );
    renderMobile(<ReportFaultScreen userId={12} />);

    // Equipment comes from the engine's search.
    await pickEquipment(/PMP-001/);
    fireEvent.change(screen.getByLabelText("Objet"), { target: { value: "  Fuite pompe  " } });
    fireEvent.change(screen.getByLabelText("Description"), {
      target: { value: "Fuite d'huile au palier" },
    });

    // Each list shows the engine's labels (nothing declared in the shell).
    fireEvent.click(await screen.findByRole("button", { name: /^Origine:/ }));
    expect(screen.getAllByRole("radio").map((r) => r.textContent)).toEqual(["Opérateur"]);
    fireEvent.click(screen.getByRole("radio", { name: "Opérateur" }));
    await pick("Type de demande", "Dépannage");
    await pick("Symptôme", "Fuite");
    await pick("Urgence", "Urgent");
    await pick("Niveau d'impact", "Majeur");
    fireEvent.click(screen.getByLabelText("Sécurité des personnes"));

    fireEvent.click(screen.getByRole("button", { name: "Envoyer la demande" }));

    expect(await screen.findByText("Demande DI-0042 enregistrée.")).toBeTruthy();
    expect(created).toEqual({
      equipment_id: 77,
      org_node_id: 5,
      title: "Fuite pompe",
      description: "Fuite d'huile au palier",
      origin_type: "operator",
      request_type: "corrective",
      symptom_code_id: 301,
      impact_level: "major",
      production_impact: false,
      safety_flag: true,
      environmental_flag: false,
      quality_flag: false,
      reported_urgency: "urgent",
      observed_at: null,
      source_inspection_anomaly_id: null,
      submitter_id: 12,
    });
  });

  it("does not call create_di while required fields are missing", async () => {
    let calls = 0;
    mockInvoke.mockImplementation(
      handlers(() => {
        calls += 1;
      }),
    );
    renderMobile(<ReportFaultScreen userId={12} />);
    await screen.findByRole("button", { name: /^Origine:/ });

    fireEvent.click(screen.getByRole("button", { name: "Envoyer la demande" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Champs obligatoires à remplir");
    expect(alert).toHaveTextContent("Équipement");
    expect(alert).toHaveTextContent("Objet");
    expect(calls).toBe(0);
  });

  it("shows a refusal of create_di in plain words, never the engine's text", async () => {
    mockInvoke.mockImplementation(async (cmd: string, args?: unknown) => {
      if (cmd === "create_di") {
        throw { code: "PERMISSION_DENIED", message: "Permission denied: di.create" };
      }
      return ipcAnswering({
        list_reference_domains: () => Object.entries(DOMAINS).map(([c, id]) => domain(c, id)),
        list_reference_sets: (a) => [
          {
            id: Number(a?.["domainId"]) * 10,
            domain_id: a?.["domainId"],
            version_no: 1,
            status: "published",
            effective_from: null,
            created_by_id: null,
            created_at: "x",
            published_at: "x",
          },
        ],
        list_reference_values: (a) => VALUES[Number(a?.["setId"])] ?? [],
        list_urgency_levels: () => [URGENCY],
        search_assets: () => [ASSET],
      })(cmd, args);
    });
    renderMobile(<ReportFaultScreen userId={12} />);

    await pickEquipment(/PMP-001/);
    fireEvent.change(screen.getByLabelText("Objet"), { target: { value: "Fuite" } });
    fireEvent.change(screen.getByLabelText("Description"), { target: { value: "Fuite" } });
    await pick("Origine", "Opérateur");
    await pick("Type de demande", "Dépannage");
    await pick("Symptôme", "Fuite");
    await pick("Urgence", "Urgent");
    await pick("Niveau d'impact", "Majeur");
    fireEvent.click(screen.getByRole("button", { name: "Envoyer la demande" }));

    // The refusal in the user's words; the engine's technical text stays in the log.
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent("Action non autorisée"),
    );
    expect(screen.queryByText(/Permission denied/)).toBeNull();
  });
});
