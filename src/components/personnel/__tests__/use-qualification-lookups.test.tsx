/**
 * Qualification lookups only ask for the lists the user may read: a technician with training
 * rights but no personnel rights must not trigger a refused (and locking) personnel request.
 */
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const svc = vi.hoisted(() => ({
  listActiveTeams: vi.fn(),
  listExternalCompanies: vi.fn(),
  listPersonnel: vi.fn(),
  listPersonnelSkillReferenceValues: vi.fn(),
  listCertificationTypes: vi.fn(),
}));
vi.mock("@/services/personnel-service", () => ({
  listActiveTeams: svc.listActiveTeams,
  listExternalCompanies: svc.listExternalCompanies,
  listPersonnel: svc.listPersonnel,
  listPersonnelSkillReferenceValues: svc.listPersonnelSkillReferenceValues,
}));
vi.mock("@/services/qualification-service", () => ({
  listCertificationTypes: svc.listCertificationTypes,
}));

import { useQualificationLookups } from "@/components/personnel/qualification/use-qualification-lookups";
import { PermissionContext, type PermissionContextValue } from "@/contexts/PermissionContext";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

function withPermissions(granted: PermissionName[]) {
  const value: PermissionContextValue = {
    permissions: [],
    isLoading: false,
    can: (p) => granted.includes(p),
    canAny: (...ps) => ps.some((p) => granted.includes(p)),
    canAll: (...ps) => ps.every((p) => granted.includes(p)),
    refresh: async () => undefined,
  };
  return ({ children }: { children: ReactNode }) => (
    <PermissionContext.Provider value={value}>{children}</PermissionContext.Provider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  svc.listActiveTeams.mockResolvedValue([]);
  svc.listExternalCompanies.mockResolvedValue([]);
  svc.listPersonnel.mockResolvedValue({ items: [], total: 0 });
  svc.listPersonnelSkillReferenceValues.mockResolvedValue([]);
  svc.listCertificationTypes.mockResolvedValue([]);
});

describe("useQualificationLookups", () => {
  it("does not request personnel lists without the personnel right", async () => {
    const { result } = renderHook(() => useQualificationLookups(true), {
      wrapper: withPermissions([P.TRN_VIEW]),
    });
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(svc.listCertificationTypes).toHaveBeenCalled();
    expect(svc.listPersonnel).not.toHaveBeenCalled();
    expect(svc.listActiveTeams).not.toHaveBeenCalled();
    expect(svc.listExternalCompanies).not.toHaveBeenCalled();
    expect(result.current.error).toBeNull();
    // Pickers can say why they are empty.
    expect(result.current.peopleRestricted).toBe(true);
  });

  it("reload does nothing when the lookups are disabled", async () => {
    const { result } = renderHook(() => useQualificationLookups(false), {
      wrapper: withPermissions([]),
    });
    await result.current.reload();
    expect(svc.listCertificationTypes).not.toHaveBeenCalled();
    expect(svc.listPersonnelSkillReferenceValues).not.toHaveBeenCalled();
  });
});
