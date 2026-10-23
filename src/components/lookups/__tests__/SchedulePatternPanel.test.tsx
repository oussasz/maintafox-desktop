/**
 * Classe horaire timetable: the site rest days (`list_site_work_weeks`) are read by reference-data
 * readers as well as people managers, and a refused or failed read leaves only the rest days out,
 * never the whole timetable.
 */
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ClasseHoraireEditor } from "@/components/lookups/SchedulePatternPanel";
import type { ReferenceValue } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

const held = vi.hoisted(() => ({ perms: new Set<string>() }));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en", resolvedLanguage: "en" },
  }),
}));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({
    can: (p: string) => held.perms.has(p),
    canAny: (...ps: string[]) => ps.some((p) => held.perms.has(p)),
    isLoading: false,
  }),
}));

const service = vi.hoisted(() => ({
  listScheduleClassVersions: vi.fn(),
  listSiteWorkWeeks: vi.fn(),
}));
vi.mock("@/services/workforce-service", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  listScheduleClassVersions: service.listScheduleClassVersions,
  listSiteWorkWeeks: service.listSiteWorkWeeks,
}));

const value = { id: 7, code: "JOUR", label: "Jour" } as unknown as ReferenceValue;

describe("ClasseHoraireEditor site rest days", () => {
  beforeEach(() => {
    held.perms = new Set();
    service.listScheduleClassVersions.mockReset().mockResolvedValue([]);
    service.listSiteWorkWeeks.mockReset().mockResolvedValue([]);
  });

  it("reads the site work weeks with ref.view alone", async () => {
    held.perms = new Set([P.REF_VIEW]);
    render(<ClasseHoraireEditor value={value} canMutate={false} />);
    await waitFor(() => expect(service.listSiteWorkWeeks).toHaveBeenCalled());
  });

  it("does not ask for them without ref.view or per.view", async () => {
    held.perms = new Set([P.OT_VIEW]);
    render(<ClasseHoraireEditor value={value} canMutate={false} />);
    await waitFor(() => expect(service.listScheduleClassVersions).toHaveBeenCalled());
    expect(service.listSiteWorkWeeks).not.toHaveBeenCalled();
  });

  it("keeps the timetable when the site work weeks are refused", async () => {
    held.perms = new Set([P.PER_VIEW]);
    service.listSiteWorkWeeks.mockRejectedValue({ code: "PERMISSION_DENIED", message: "refused" });
    render(<ClasseHoraireEditor value={value} canMutate={false} />);
    await waitFor(() => expect(service.listSiteWorkWeeks).toHaveBeenCalled());
    await waitFor(() => expect(screen.queryByRole("status")).not.toBeInTheDocument());
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
