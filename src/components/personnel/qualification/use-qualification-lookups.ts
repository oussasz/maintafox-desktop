/**
 * Reference lists the qualification screens pick from (certification types, skills, people,
 * teams, contractor companies). Loaded once per screen; `reload` after a catalogue edit.
 */
import { useCallback, useContext, useEffect, useState } from "react";

import { PermissionContext } from "@/contexts/PermissionContext";
import {
  listActiveTeams,
  listExternalCompanies,
  listPersonnel,
  listPersonnelSkillReferenceValues,
} from "@/services/personnel-service";
import { listCertificationTypes } from "@/services/qualification-service";
import type {
  CertificationType,
  ExternalCompany,
  Personnel,
  PersonnelSkillReferenceValue,
  TeamSummary,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/** Upper bound of people loaded for pickers (the register's own pages are paged). */
export const PEOPLE_PICKER_LIMIT = 2000;

export interface QualificationLookups {
  certTypes: CertificationType[];
  skills: PersonnelSkillReferenceValue[];
  people: Personnel[];
  teams: TeamSummary[];
  companies: ExternalCompany[];
  loading: boolean;
  error: unknown;
  reload: () => Promise<void>;
  /**
   * People, teams and companies were not read because the user lacks the personnel view right:
   * pickers fed by them are empty for that reason, not because there is nobody.
   */
  peopleRestricted: boolean;
}

/** `enabled = false` skips loading (the user may not read these lists). */
export function useQualificationLookups(enabled = true): QualificationLookups {
  const [certTypes, setCertTypes] = useState<CertificationType[]>([]);
  const [skills, setSkills] = useState<PersonnelSkillReferenceValue[]>([]);
  const [people, setPeople] = useState<Personnel[]>([]);
  const [teams, setTeams] = useState<TeamSummary[]>([]);
  const [companies, setCompanies] = useState<ExternalCompany[]>([]);
  const [loading, setLoading] = useState(enabled);
  const [error, setError] = useState<unknown>(null);

  // Only the lists the user may read are requested: asking for one they may not read is refused
  // by the server and used to end in a permission error (or the lock screen) on Refresh.
  // Outside a permission provider (unit tests) every list is requested.
  const permissions = useContext(PermissionContext);
  const canTraining = permissions?.can(P.TRN_VIEW) ?? true;
  const canPersonnel = permissions?.can(P.PER_VIEW) ?? true;

  const reload = useCallback(async () => {
    if (!enabled) return;
    setLoading(true);
    setError(null);
    try {
      const [ct, sk, pp, tm, co] = await Promise.all([
        canTraining ? listCertificationTypes() : Promise.resolve([]),
        listPersonnelSkillReferenceValues(),
        canPersonnel
          ? listPersonnel({ employment_status: ["active"], limit: PEOPLE_PICKER_LIMIT, offset: 0 })
          : Promise.resolve({ items: [] as Personnel[], total: 0 }),
        canPersonnel ? listActiveTeams({ include_inactive: false }) : Promise.resolve([]),
        canPersonnel ? listExternalCompanies() : Promise.resolve([]),
      ]);
      setCertTypes(ct);
      setSkills(sk);
      setPeople(pp.items);
      setTeams(tm);
      setCompanies(co);
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, [enabled, canTraining, canPersonnel]);

  useEffect(() => {
    if (enabled) void reload();
  }, [reload, enabled]);

  return {
    certTypes,
    skills,
    people,
    teams,
    companies,
    loading,
    error,
    reload,
    peopleRestricted: !canPersonnel,
  };
}

/** "Name (CODE)" for pickers and tables — never a bare id. */
export function personLabel(p: Pick<Personnel, "full_name" | "employee_code">): string {
  const name = p.full_name.trim() || p.employee_code;
  return p.employee_code && name !== p.employee_code ? `${name} (${p.employee_code})` : name;
}
