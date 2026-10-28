import { beforeEach, describe, expect, it, vi } from "vitest";

import { invoke } from "@/lib/ipc-invoke";

import * as svc from "../personnel-service";

vi.mock("@/lib/ipc-invoke", () => ({ invoke: vi.fn() }));

const mockInvoke = vi.mocked(invoke);

/** Each command: how the screen calls it, and what it must send to the backend. */
const COMMANDS: Array<{
  name: string;
  call: () => Promise<unknown>;
  command: string;
  args: Record<string, unknown> | undefined;
  /** The third `invoke` argument, when the command passes one. */
  options?: { notifyOnError: boolean };
}> = [
  {
    name: "getPersonnel",
    call: () => svc.getPersonnel(4),
    command: "get_personnel",
    args: { id: 4 },
  },
  {
    name: "createPersonnel",
    call: () => svc.createPersonnel({ full_name: "A" } as never),
    command: "create_personnel",
    args: { input: { full_name: "A" } },
  },
  {
    name: "updatePersonnel",
    call: () => svc.updatePersonnel({ id: 4 } as never),
    command: "update_personnel",
    args: { input: { id: 4 } },
  },
  {
    name: "deactivatePersonnel",
    call: () => svc.deactivatePersonnel(4, 2),
    command: "deactivate_personnel",
    args: { id: 4, expectedRowVersion: 2 },
  },
  {
    name: "listPositions",
    call: () => svc.listPositions(),
    command: "list_positions",
    args: undefined,
  },
  {
    name: "listPositionsFiltered",
    call: () => svc.listPositionsFiltered({ search: "elec" } as never),
    command: "list_positions",
    args: { filter: { search: "elec" } },
  },
  { name: "getPosition", call: () => svc.getPosition(5), command: "get_position", args: { id: 5 } },
  {
    name: "upsertPosition",
    call: () => svc.upsertPosition({ code: "ELEC" } as never),
    command: "upsert_position",
    args: { input: { code: "ELEC" } },
  },
  {
    name: "archivePosition",
    call: () => svc.archivePosition(5),
    command: "archive_position",
    args: { id: 5 },
  },
  {
    name: "getPositionRequirementSeed",
    call: () => svc.getPositionRequirementSeed(5),
    command: "get_position_requirement_seed",
    args: { positionId: 5 },
  },
  {
    name: "uploadPersonnelPhoto",
    call: () => svc.uploadPersonnelPhoto(4, "C:/photo.png"),
    command: "upload_personnel_photo",
    args: { personnelId: 4, sourcePath: "C:/photo.png" },
  },
  {
    name: "readPersonnelPhotoPreview",
    call: () => svc.readPersonnelPhotoPreview(4),
    command: "read_personnel_photo_preview",
    args: { personnelId: 4 },
  },
  {
    name: "listPersonnelAssignmentHistory",
    call: () => svc.listPersonnelAssignmentHistory(4),
    command: "list_personnel_assignment_history",
    args: { personnelId: 4, limit: 60 },
  },
  {
    name: "getPersonnelAvailabilityState",
    call: () => svc.getPersonnelAvailabilityState(4),
    command: "get_personnel_availability_state",
    args: { personnelId: 4 },
  },
  {
    name: "createPosition",
    call: () => svc.createPosition("ELEC", "Electrician", "technical"),
    command: "create_position",
    args: { code: "ELEC", name: "Electrician", category: "technical" },
  },
  {
    name: "listRateCards",
    call: () => svc.listRateCards(4),
    command: "list_rate_cards",
    args: { personnelId: 4 },
  },
  {
    name: "getActiveRateCard",
    call: () => svc.getActiveRateCard(4),
    command: "get_active_rate_card",
    args: { personnelId: 4 },
  },
  {
    name: "createRateCard",
    call: () => svc.createRateCard(4, 45, 60, null, "manual"),
    command: "create_rate_card",
    args: {
      personnelId: 4,
      laborRate: 45,
      overtimeRate: 60,
      costCenterId: null,
      sourceType: "manual",
    },
  },
  {
    name: "listAuthorizations",
    call: () => svc.listAuthorizations(4),
    command: "list_authorizations",
    args: { personnelId: 4 },
  },
  {
    name: "createAuthorization",
    call: () => svc.createAuthorization(4, "electrical", "2026-01-01", null, 9),
    command: "create_authorization",
    args: {
      personnelId: 4,
      authorizationType: "electrical",
      validFrom: "2026-01-01",
      validTo: null,
      sourceCertificationTypeId: 9,
    },
  },
  {
    name: "listExternalCompanies (no filter)",
    call: () => svc.listExternalCompanies(),
    command: "list_external_companies",
    args: { filter: {} },
  },
  {
    name: "createExternalCompany (optional fields omitted)",
    call: () => svc.createExternalCompany("Acme"),
    command: "create_external_company",
    args: {
      name: "Acme",
      serviceDomain: null,
      contractStart: null,
      contractEnd: null,
      notes: null,
    },
  },
  {
    name: "createExternalCompany (all fields)",
    call: () => svc.createExternalCompany("Acme", "hvac", "2026-01-01", "2026-12-31", "note"),
    command: "create_external_company",
    args: {
      name: "Acme",
      serviceDomain: "hvac",
      contractStart: "2026-01-01",
      contractEnd: "2026-12-31",
      notes: "note",
    },
  },
  {
    name: "listCompanyContacts",
    call: () => svc.listCompanyContacts(3),
    command: "list_company_contacts",
    args: { companyId: 3 },
  },
  {
    name: "listSkillsMatrix",
    call: () => svc.listSkillsMatrix({} as never),
    command: "list_skills_matrix",
    args: { filter: {} },
  },
  {
    name: "listAvailabilityCalendar",
    call: () => svc.listAvailabilityCalendar({} as never),
    command: "list_availability_calendar",
    args: { filter: {} },
  },
  {
    name: "listTeamCapacitySummary",
    call: () => svc.listTeamCapacitySummary({} as never),
    command: "list_team_capacity_summary",
    args: { filter: {} },
  },
  {
    name: "listActiveTeams (no filter)",
    call: () => svc.listActiveTeams(),
    command: "list_active_teams",
    args: { filter: {} },
  },
  { name: "getTeam", call: () => svc.getTeam(3), command: "get_team", args: { teamId: 3 } },
  {
    name: "listTeamMembers",
    call: () => svc.listTeamMembers(3),
    command: "list_team_members",
    args: { teamId: 3 },
  },
  {
    name: "createTeam",
    call: () => svc.createTeam({ code: "MEC" } as never),
    command: "create_team",
    args: { input: { code: "MEC" } },
  },
  {
    name: "updateTeam",
    call: () => svc.updateTeam({ id: 3 } as never),
    command: "update_team",
    args: { input: { id: 3 } },
  },
  {
    name: "setTeamLead",
    call: () => svc.setTeamLead({ team_id: 3 } as never),
    command: "set_team_lead",
    args: { input: { team_id: 3 } },
  },
  {
    name: "createAvailabilityBlock",
    call: () => svc.createAvailabilityBlock({ personnel_id: 4 } as never),
    command: "create_availability_block",
    args: { input: { personnel_id: 4 } },
    options: { notifyOnError: false },
  },
  {
    name: "updateAvailabilityBlock",
    call: () => svc.updateAvailabilityBlock({ id: 8 } as never),
    command: "update_availability_block",
    args: { input: { id: 8 } },
    options: { notifyOnError: false },
  },
  {
    name: "endAvailabilityBlock",
    call: () => svc.endAvailabilityBlock(8, 3),
    command: "end_availability_block",
    args: { id: 8, expectedRowVersion: 3 },
  },
  {
    name: "listPersonnelTeamAssignments",
    call: () => svc.listPersonnelTeamAssignments(4),
    command: "list_personnel_team_assignments",
    args: { personnelId: 4 },
  },
  {
    name: "listPersonnelAvailabilityBlocks",
    call: () => svc.listPersonnelAvailabilityBlocks(4),
    command: "list_personnel_availability_blocks",
    args: { personnelId: 4, limit: 50 },
  },
  {
    name: "listPersonnelWorkHistory",
    call: () => svc.listPersonnelWorkHistory(4),
    command: "list_personnel_work_history",
    args: { personnelId: 4, limit: 60 },
  },
  {
    name: "getPersonnelWorkloadSummary",
    call: () => svc.getPersonnelWorkloadSummary(4),
    command: "get_personnel_workload_summary",
    args: { personnelId: 4 },
  },
  {
    name: "scanSuccessionRisk (no scope)",
    call: () => svc.scanSuccessionRisk(),
    command: "scan_succession_risk",
    args: { entityId: null, teamId: null },
  },
  {
    name: "scanSuccessionRisk (scoped)",
    call: () => svc.scanSuccessionRisk(2, 3),
    command: "scan_succession_risk",
    args: { entityId: 2, teamId: 3 },
  },
  {
    name: "declareOwnSkill",
    call: () => svc.declareOwnSkill({ skill_code: "WELD" } as never),
    command: "declare_own_skill",
    args: { payload: { skill_code: "WELD" } },
  },
  {
    name: "listPersonnelSkillReferenceValues",
    call: () => svc.listPersonnelSkillReferenceValues(),
    command: "list_personnel_skill_reference_values",
    args: undefined,
  },
  {
    name: "createPersonnelImportBatch",
    call: () => svc.createPersonnelImportBatch({ file_name: "a.csv" } as never),
    command: "create_personnel_import_batch",
    args: { input: { file_name: "a.csv" } },
  },
  {
    name: "getPersonnelImportPreview",
    call: () => svc.getPersonnelImportPreview(6),
    command: "get_personnel_import_preview",
    args: { batchId: 6 },
  },
  {
    name: "applyPersonnelImportBatch",
    call: () => svc.applyPersonnelImportBatch(6),
    command: "apply_personnel_import_batch",
    args: { batchId: 6 },
  },
  {
    name: "getWorkforceSummaryReport",
    call: () => svc.getWorkforceSummaryReport(),
    command: "get_workforce_summary_report",
    args: undefined,
  },
  {
    name: "getWorkforceSkillsGapReport",
    call: () => svc.getWorkforceSkillsGapReport(),
    command: "get_workforce_skills_gap_report",
    args: { limit: 100 },
  },
  {
    name: "getWorkforceKpiReport",
    call: () => svc.getWorkforceKpiReport(),
    command: "get_workforce_kpi_report",
    args: undefined,
  },
  {
    name: "exportWorkforceReportCsv",
    call: () => svc.exportWorkforceReportCsv("kpi"),
    command: "export_workforce_report_csv",
    args: { reportKind: "kpi" },
  },
];

describe("personnel-service commands", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it.each(COMMANDS)("$name sends $command and refuses a payload of the wrong shape", async (c) => {
    mockInvoke.mockResolvedValue({ unexpected: true });
    const err = await c.call().catch((e: unknown) => e);
    expect(mockInvoke).toHaveBeenCalledWith(c.command, c.args, ...(c.options ? [c.options] : []));
    expect(err).toBeInstanceOf(svc.PersonnelIpcError);
    expect((err as svc.PersonnelIpcError).code).toBe("VALIDATION_FAILED");
  });

  it("drops empty filter lists before listing personnel", async () => {
    mockInvoke.mockResolvedValue({ unexpected: true });
    await svc
      .listPersonnel({ employment_type: [], availability_status: ["available"] } as never)
      .catch(() => undefined);
    expect(mockInvoke).toHaveBeenCalledWith("list_personnel", {
      filter: { employment_type: null, availability_status: ["available"] },
    });
    mockInvoke.mockClear();
    await svc.listPersonnel({ employment_type: ["permanent"] } as never).catch(() => undefined);
    expect(mockInvoke).toHaveBeenCalledWith("list_personnel", {
      filter: { employment_type: ["permanent"], availability_status: null },
    });
  });

  it("deletes an availability block without a payload to read", async () => {
    mockInvoke.mockResolvedValue(undefined);
    await expect(svc.deleteAvailabilityBlock(8, 3)).resolves.toBeUndefined();
    expect(mockInvoke).toHaveBeenCalledWith("delete_availability_block", {
      id: 8,
      expectedRowVersion: 3,
    });
  });
});

describe("personnel-service results", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("returns a crew's members and teams as the backend sends them", async () => {
    const member = {
      assignment_id: 1,
      personnel_id: 4,
      employee_code: "T-04",
      full_name: "Amine Haddad",
      position_name: null,
      role_code: "member",
      allocation_percent: 100,
      valid_from: null,
      valid_to: null,
      is_lead: 0,
      availability_status: "available",
      user_account_id: null,
    };
    mockInvoke.mockResolvedValue([member]);
    await expect(svc.listTeamMembers(3)).resolves.toEqual([member]);

    const team = {
      id: 3,
      entity_sync_id: "t-3",
      code: "MEC",
      name: "Mechanics",
      team_type: "maintenance",
      status: "active",
      primary_node_id: null,
      node_name: null,
      node_code: null,
      description: null,
      icon_key: null,
      accent_color: null,
    };
    mockInvoke.mockResolvedValue(team);
    await expect(svc.getTeam(3)).resolves.toEqual(team);
  });

  it("reads an absent rate card, a stored photo path and a CSV export", async () => {
    mockInvoke.mockResolvedValue(null);
    await expect(svc.getActiveRateCard(4)).resolves.toBeNull();
    mockInvoke.mockResolvedValue("photos/4.png");
    await expect(svc.uploadPersonnelPhoto(4, "C:/p.png")).resolves.toBe("photos/4.png");
    mockInvoke.mockResolvedValue("a,b\n1,2");
    await expect(svc.exportWorkforceReportCsv("summary")).resolves.toBe("a,b\n1,2");
    mockInvoke.mockResolvedValue(undefined);
    await expect(svc.declareOwnSkill({} as never)).resolves.toBeUndefined();
  });

  it("marks an availability reason as not withheld when the backend omits the flag", async () => {
    const block = {
      id: 8,
      personnel_id: 4,
      block_type: "leave",
      start_at: "2026-05-04T08:00:00Z",
      end_at: "2026-05-04T16:00:00Z",
      reason_note: null,
      is_critical: false,
      created_by_id: null,
      created_by_label: null,
      created_at: "2026-05-01T08:00:00Z",
      row_version: 1,
      // Always sent by the backend (personnel::absence_access). reason_masked is left out here to
      // check the schema still reads it as false.
      type_masked: false,
      critical_for_planning: false,
    };
    mockInvoke.mockResolvedValue([block]);
    const rows = await svc.listPersonnelAvailabilityBlocks(4, 10);
    expect(mockInvoke).toHaveBeenCalledWith("list_personnel_availability_blocks", {
      personnelId: 4,
      limit: 10,
    });
    expect(rows[0]?.reason_masked).toBe(false);
  });
});

describe("personnel-service errors", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("keeps the backend's error code", async () => {
    mockInvoke.mockRejectedValue({ code: "NOT_FOUND", message: "Personnel 4 not found" });
    const err = await svc.getPersonnel(4).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(svc.PersonnelIpcError);
    expect((err as svc.PersonnelIpcError).code).toBe("NOT_FOUND");
    expect((err as Error).message).toBe("Personnel 4 not found");
    expect((err as Error).name).toBe("PersonnelIpcError");
  });

  it("passes other errors through, and turns anything else into an error", async () => {
    const network = new Error("network unreachable");
    mockInvoke.mockRejectedValue(network);
    await expect(svc.getTeam(3)).rejects.toBe(network);
    mockInvoke.mockRejectedValue("bridge closed");
    await expect(svc.getTeam(3)).rejects.toThrow("bridge closed");
  });

  it("recognises the overlap confirmation the availability form asks for", () => {
    expect(
      svc.isAvailabilityOverlapConfirmError(
        new Error(`Validation failed: ${svc.AVAILABILITY_BLOCK_OVERLAP_CONFIRM}`),
      ),
    ).toBe(true);
    expect(svc.isAvailabilityOverlapConfirmError(svc.AVAILABILITY_BLOCK_OVERLAP_CONFIRM)).toBe(
      true,
    );
    expect(svc.isAvailabilityOverlapConfirmError(new Error("other"))).toBe(false);
  });
});
