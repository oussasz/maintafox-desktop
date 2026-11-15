/**
 * Certifications belong to the training module: when the plant's edition does not include it,
 * the form neither asks for the list (the server refuses it) nor shows the block.
 */
import { render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { usePersonnelListsStore } from "@/stores/personnel-lists-store";
import { usePersonnelStore } from "@/stores/personnel-store";
import { P } from "@shared/rbac/permissions.generated";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "fr" } }),
}));

const perms = vi.hoisted(() => ({ granted: new Set<string>() }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: (p: string) => perms.granted.has(p) }),
}));

const caps = vi.hoisted(() => ({ map: {} as Record<string, boolean>, loading: false }));
vi.mock("@/hooks/use-module-capabilities", () => ({
  useModuleCapabilities: () => ({ capabilityMap: caps.map, loading: caps.loading }),
}));

const svc = vi.hoisted(() => ({
  listPersonnelReferenceOptions: vi.fn(),
  listExternalCompanies: vi.fn(),
  listPersonnel: vi.fn(),
  listActiveTeams: vi.fn(),
}));
vi.mock("@/services/personnel-service", () => svc);
const qual = vi.hoisted(() => ({ listCertificationTypes: vi.fn() }));
vi.mock("@/services/qualification-service", () => qual);
vi.mock("@/services/org-designer-service", () => ({
  getOrgDesignerSnapshot: vi.fn().mockResolvedValue({ nodes: [] }),
}));
vi.mock("@/components/personnel/PositionCombobox", () => ({ PositionCombobox: () => null }));
vi.mock("@/components/reference/ReferenceCombobox", () => ({ ReferenceCombobox: () => null }));
vi.mock("@/components/personnel/PersonnelCard", () => ({ PersonnelPickerCombobox: () => null }));
vi.mock("@/components/personnel/ExternalCompaniesPanel", () => ({
  ExternalCompanyDialog: () => null,
}));
vi.mock("@/components/entity-form/EntityFormImageUploader", () => ({
  EntityFormImageUploader: ({ children }: { children?: ReactNode }) => <>{children}</>,
}));

import { PersonnelCreateDialog } from "../PersonnelCreateDialog";

beforeEach(() => {
  vi.clearAllMocks();
  perms.granted = new Set([P.PER_VIEW, P.PER_MANAGE, P.TRN_VIEW]);
  caps.map = {};
  caps.loading = false;
  usePersonnelListsStore.setState({ options: null, loading: false, error: null });
  svc.listPersonnelReferenceOptions.mockResolvedValue({
    employment_statuses: [],
    employment_types: [],
    absence_types: [],
    position_categories: [],
  });
  svc.listExternalCompanies.mockResolvedValue([]);
  svc.listPersonnel.mockResolvedValue({ items: [], total: 0 });
  svc.listActiveTeams.mockResolvedValue([]);
  qual.listCertificationTypes.mockResolvedValue([]);
  usePersonnelStore.setState({ showCreateForm: true, editingPersonnel: null, error: null });
});

describe("certifications in the new person form", () => {
  it("are hidden and not requested when training is not in the edition", async () => {
    caps.map = { training: false, personnel: true };
    render(<PersonnelCreateDialog />);
    await waitFor(() => expect(svc.listPersonnel).toHaveBeenCalled());
    expect(qual.listCertificationTypes).not.toHaveBeenCalled();
    expect(screen.queryByText("create.section.certs")).toBeNull();
  });

  it("are shown and requested when training is licensed", async () => {
    caps.map = { training: true, personnel: true };
    render(<PersonnelCreateDialog />);
    await waitFor(() => expect(qual.listCertificationTypes).toHaveBeenCalled());
    expect(screen.getByText("create.section.certs")).toBeInTheDocument();
  });
});
