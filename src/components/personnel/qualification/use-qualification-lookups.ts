/**
 * Reference lists the qualification screens pick from (certification types, skills, people,
 * teams, contractor companies). Loaded once per screen; `reload` after a catalogue edit.
 */
import { useCallback, useEffect, useState } from "react";

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

  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [ct, sk, pp, tm, co] = await Promise.all([
        listCertificationTypes(),
        listPersonnelSkillReferenceValues(),
        listPersonnel({ employment_status: ["active"], limit: PEOPLE_PICKER_LIMIT, offset: 0 }),
        listActiveTeams({ include_inactive: false }),
        listExternalCompanies(),
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
  }, []);

  useEffect(() => {
    if (enabled) void reload();
  }, [reload, enabled]);

  return { certTypes, skills, people, teams, companies, loading, error, reload };
}

/** "Name (CODE)" for pickers and tables — never a bare id. */
export function personLabel(p: Pick<Personnel, "full_name" | "employee_code">): string {
  const name = p.full_name.trim() || p.employee_code;
  return p.employee_code && name !== p.employee_code ? `${name} (${p.employee_code})` : name;
}
