/**
 * Qualification gate, workforce capacity / holidays / skills and personnel labour services:
 * each calls its IPC command with the expected arguments and validates the response shape.
 */
import { beforeEach, describe, expect, it } from "vitest";

import { mockInvoke } from "@/test/mocks/tauri";

import {
  checkPersonForPermitRole,
  checkPersonForWorkOrder,
  getQualificationMatrix,
  getWoQualificationAssessment,
  listQualificationOverrides,
  listQualificationRequirements,
  recordQualificationOverride,
  upsertQualificationRequirement,
} from "../qualification-gate-service";
import { addPersonnelLabor, listWoLaborPeople } from "../wo-execution-service";
import {
  getPersonCapacity,
  getTeamCapacity,
  listPersonnelSkills,
  listWorkCalendarHolidays,
  removePersonnelSkill,
  updateExternalCompany,
  upsertPersonnelSkill,
  upsertWorkCalendarHoliday,
} from "../workforce-service";

const requirement = {
  id: 1,
  entity_sync_id: "r-1",
  target_kind: "work_order_type",
  work_order_id: null,
  work_order_code: null,
  target_code: "CORRECTIVE",
  target_label: "Corrective",
  certification_type_id: 3,
  certification_code: "HAB-B1V",
  certification_name: "Habilitation B1V",
  skill_reference_value_id: null,
  skill_code: null,
  skill_label: null,
  min_proficiency_level: null,
  coverage: "each",
  notes: null,
  is_active: true,
  row_version: 1,
  created_at: "2026-10-14T08:00:00Z",
  updated_at: "2026-10-14T08:00:00Z",
};

const check = {
  personnel_id: 7,
  full_name: "Samir",
  employee_code: "EXT-1",
  roles: ["candidate"],
  qualified: false,
  gaps: [
    {
      requirement_key: "cert:3",
      kind: "certification",
      code: "HAB-B1V",
      label: "Habilitation B1V",
      status: "missing",
      expires_at: null,
      held_level: null,
      required_level: null,
      source_kind: "work_order",
      source_code: null,
    },
  ],
  warnings: [],
  override_info: null,
};

const skillRow = {
  id: 1,
  personnel_id: 7,
  reference_value_id: 9,
  skill_code: "HYDRAULICS",
  skill_label: "Hydraulique",
  proficiency_level: 3,
  valid_from: "2026-10-14",
  valid_to: null,
  source_type: "manual",
  is_primary: false,
  is_validated: true,
  verified_by_name: "Chef d'équipe",
  verified_at: "2026-10-14T08:00:00Z",
};

beforeEach(() => {
  mockInvoke.mockReset();
});

describe("qualification gate service", () => {
  it("lists and saves job requirements", async () => {
    mockInvoke.mockResolvedValueOnce([requirement]);
    expect(await listQualificationRequirements({ target_kind: "work_order_type" })).toHaveLength(1);
    expect(mockInvoke).toHaveBeenLastCalledWith("list_qualification_requirements", {
      filter: { target_kind: "work_order_type" },
    });
    mockInvoke.mockResolvedValueOnce(requirement);
    const saved = await upsertQualificationRequirement({
      target_kind: "work_order_type",
      target_code: "CORRECTIVE",
      certification_type_id: 3,
    });
    expect(saved.certification_name).toBe("Habilitation B1V");
  });

  it("checks people against a work order and a LOTO role", async () => {
    mockInvoke.mockResolvedValueOnce(check);
    expect((await checkPersonForWorkOrder(7, 4)).gaps[0]?.status).toBe("missing");
    expect(mockInvoke).toHaveBeenLastCalledWith("check_person_for_work_order", {
      personnelId: 7,
      woId: 4,
    });
    mockInvoke.mockResolvedValueOnce(check);
    await checkPersonForPermitRole(7, 2, "isolation_apply");
    expect(mockInvoke).toHaveBeenLastCalledWith("check_person_for_permit_role", {
      personnelId: 7,
      permitId: 2,
      role: "isolation_apply",
    });
    mockInvoke.mockResolvedValueOnce({
      work_order_id: 4,
      work_day: "2026-10-14",
      requirements: [],
      people: [check],
      uncovered: [],
      assessed_team_id: null,
      ok: false,
    });
    expect((await getWoQualificationAssessment(4)).ok).toBe(false);
  });

  it("records an override and lists the log", async () => {
    mockInvoke.mockResolvedValueOnce({ ...check, qualified: true });
    await recordQualificationOverride(
      7,
      { context_kind: "work_order", work_order_id: 4 },
      "Encadré sur place",
    );
    expect(mockInvoke).toHaveBeenLastCalledWith("record_qualification_override", {
      input: {
        personnel_id: 7,
        reason: "Encadré sur place",
        context_kind: "work_order",
        work_order_id: 4,
      },
    });
    mockInvoke.mockResolvedValueOnce([]);
    expect(await listQualificationOverrides({ work_order_id: 4 })).toEqual([]);
  });

  it("rejects a malformed matrix", async () => {
    mockInvoke.mockResolvedValueOnce({ columns: [], rows: [], generated_on: "2026-10-14" });
    expect((await getQualificationMatrix()).rows).toEqual([]);
    mockInvoke.mockResolvedValueOnce({ columns: "nope" });
    await expect(getQualificationMatrix()).rejects.toThrow();
  });
});

describe("workforce service", () => {
  it("reads team and person capacity", async () => {
    mockInvoke.mockResolvedValueOnce([
      {
        team_id: 1,
        day: "2026-10-12",
        available_hours: 12,
        scheduled_hours: 16,
        absent_hours: 4,
        people: 2,
        people_scheduled: 2,
        people_absent: 0,
        basis: "roster",
        rule_available_hours: null,
        rule_overtime_hours: null,
        members: [],
      },
    ]);
    expect((await getTeamCapacity(1, "2026-10-12", "2026-10-12"))[0]?.basis).toBe("roster");
    expect(mockInvoke).toHaveBeenLastCalledWith("get_team_capacity", {
      teamId: 1,
      dateFrom: "2026-10-12",
      dateTo: "2026-10-12",
    });
    mockInvoke.mockResolvedValueOnce([]);
    expect(await getPersonCapacity(7, "2026-10-12", "2026-10-25")).toEqual([]);
  });

  it("lists and saves holidays", async () => {
    const holiday = {
      id: 1,
      entity_sync_id: "h-1",
      holiday_date: "2026-11-01",
      label: "Fête de la Révolution",
      entity_id: null,
      entity_name: null,
      is_active: true,
      row_version: 1,
    };
    mockInvoke.mockResolvedValueOnce([holiday]);
    expect(await listWorkCalendarHolidays({ date_from: "2026-01-01" })).toHaveLength(1);
    mockInvoke.mockResolvedValueOnce(holiday);
    expect(
      (await upsertWorkCalendarHoliday({ holiday_date: "2026-11-01", label: "Fête" })).id,
    ).toBe(1);
  });

  it("records, lists and removes skills", async () => {
    mockInvoke.mockResolvedValueOnce([skillRow]);
    expect(await listPersonnelSkills(7)).toHaveLength(1);
    mockInvoke.mockResolvedValueOnce([skillRow]);
    await upsertPersonnelSkill({
      personnel_id: 7,
      reference_value_id: 9,
      proficiency_level: 3,
      validate: true,
    });
    expect(mockInvoke).toHaveBeenLastCalledWith("upsert_personnel_skill", {
      input: { personnel_id: 7, reference_value_id: 9, proficiency_level: 3, validate: true },
    });
    mockInvoke.mockResolvedValueOnce([]);
    expect(await removePersonnelSkill(7, 9)).toEqual([]);
    expect(mockInvoke).toHaveBeenLastCalledWith("remove_personnel_skill", {
      personnelId: 7,
      referenceValueId: 9,
    });
  });

  it("updates a contractor company", async () => {
    mockInvoke.mockResolvedValueOnce({
      id: 3,
      name: "Elec Services",
      service_domain: null,
      contract_start: null,
      contract_end: null,
      onboarding_status: "active",
      insurance_status: "valid",
      notes: null,
      is_active: 1,
      created_at: "2026-10-14T08:00:00Z",
      updated_at: "2026-10-14T08:00:00Z",
    });
    const company = await updateExternalCompany({
      id: 3,
      name: "Elec Services",
      onboarding_status: "active",
      insurance_status: "valid",
      is_active: true,
    });
    expect(company.onboarding_status).toBe("active");
  });
});

describe("personnel labour", () => {
  it("books labour by person and lists everyone bookable", async () => {
    mockInvoke.mockResolvedValueOnce({ id: 1 });
    await addPersonnelLabor({ wo_id: 4, personnel_id: 7, is_overtime: true });
    expect(mockInvoke).toHaveBeenLastCalledWith("add_personnel_labor", {
      input: { wo_id: 4, personnel_id: 7, is_overtime: true },
    });
    mockInvoke.mockResolvedValueOnce([]);
    expect(await listWoLaborPeople()).toEqual([]);
    expect(mockInvoke).toHaveBeenLastCalledWith("list_wo_labor_people", undefined);
  });
});
