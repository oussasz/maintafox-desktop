/**
 * A new person and a new position start with no value of the plant's lists chosen for the user:
 * employment type, employment status and position category stay empty until chosen, and saving
 * without them says which one is missing (nothing is sent).
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PermissionContext, type PermissionContextValue } from "@/contexts/PermissionContext";
import { usePersonnelListsStore } from "@/stores/personnel-lists-store";
import { usePersonnelStore } from "@/stores/personnel-store";
import type { PersonnelOption } from "@shared/ipc-types";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "fr" },
  }),
}));

const perms = vi.hoisted(() => ({ granted: new Set<string>() }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: (p: string) => perms.granted.has(p) }),
}));

const svc = vi.hoisted(() => ({
  listPersonnelReferenceOptions: vi.fn(),
  listExternalCompanies: vi.fn(),
  listPersonnel: vi.fn(),
  listActiveTeams: vi.fn(),
  upsertPosition: vi.fn(),
  createPersonnel: vi.fn(),
  updatePersonnel: vi.fn(),
}));
vi.mock("@/services/personnel-service", () => svc);
const qual = vi.hoisted(() => ({ listCertificationTypes: vi.fn() }));
vi.mock("@/services/qualification-service", () => qual);
vi.mock("@/services/org-designer-service", () => ({
  getOrgDesignerSnapshot: vi.fn().mockResolvedValue({ nodes: [] }),
}));

// Pickers fed by other modules are not what this test is about.
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
import { PositionCreateDialog } from "../PositionCreateDialog";

function opt(code: string, label: string): PersonnelOption {
  return { code, label, label_en: null, tag: null, review_required: false };
}

beforeEach(() => {
  vi.clearAllMocks();
  perms.granted = new Set([P.PER_VIEW, P.PER_MANAGE]);
  usePersonnelListsStore.setState({ options: null, loading: false, error: null });
  svc.listPersonnelReferenceOptions.mockResolvedValue({
    employment_statuses: [opt("active", "Actif")],
    employment_types: [opt("employee", "Salarié")],
    absence_types: [],
    position_categories: [opt("technician", "Technicien")],
  });
  svc.listExternalCompanies.mockResolvedValue([]);
  svc.listPersonnel.mockResolvedValue({ items: [], total: 0 });
  svc.listActiveTeams.mockResolvedValue([]);
  qual.listCertificationTypes.mockResolvedValue([]);
});

describe("new position", () => {
  it("leaves the category empty and refuses to save without it", async () => {
    render(<PositionCreateDialog open onClose={vi.fn()} onCreated={vi.fn()} />);
    await waitFor(() => expect(svc.listPersonnelReferenceOptions).toHaveBeenCalled());
    const category = document.getElementById("pos-category");
    expect(category).not.toBeNull();
    await waitFor(() => expect(category).toBeEnabled());
    expect(category).toHaveTextContent("ds.state.choose");

    fireEvent.change(screen.getByLabelText(/position.field.code/), {
      target: { value: "TECH-01" },
    });
    fireEvent.change(screen.getByLabelText(/position.field.name/), {
      target: { value: "Technicien" },
    });
    fireEvent.click(screen.getByRole("button", { name: "position.create.submit" }));

    expect(await screen.findByText("position.validation.categoryRequired")).toBeInTheDocument();
    expect(svc.upsertPosition).not.toHaveBeenCalled();
  });
});

describe("new person", () => {
  it("leaves employment type and status empty and names both as missing", async () => {
    usePersonnelStore.setState({ showCreateForm: true, editingPersonnel: null, error: null });
    render(<PersonnelCreateDialog />);
    await waitFor(() => expect(svc.listPersonnelReferenceOptions).toHaveBeenCalled());
    await waitFor(() => expect(svc.listPersonnel).toHaveBeenCalled());
    // Give the lists time to arrive: an empty value must survive them.
    await waitFor(() => expect(usePersonnelListsStore.getState().options).not.toBeNull());

    const choose = screen.getAllByText("ds.state.choose");
    expect(choose).toHaveLength(2);

    fireEvent.click(screen.getByRole("button", { name: "create.submit" }));
    const alert = await screen.findByText(/validation\.employmentTypeRequired/);
    expect(alert).toHaveTextContent("validation.employmentStatusRequired");
    expect(svc.createPersonnel).not.toHaveBeenCalled();
  });
});

describe("new person: lists and rights (AU-57)", () => {
  const rights = (granted: PermissionName[], isLoading = false): PermissionContextValue => ({
    permissions: [],
    isLoading,
    can: (p) => granted.includes(p),
    canAny: (...ps) => ps.some((p) => granted.includes(p)),
    canAll: (...ps) => ps.every((p) => granted.includes(p)),
    refresh: async () => {},
  });

  it("asks each list once the rights are known, even when the dialog opened before", async () => {
    usePersonnelStore.setState({ showCreateForm: true, editingPersonnel: null, error: null });
    const { rerender } = render(
      <PermissionContext.Provider value={rights([], true)}>
        <PersonnelCreateDialog />
      </PermissionContext.Provider>,
    );
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(svc.listPersonnel).not.toHaveBeenCalled();
    expect(svc.listExternalCompanies).not.toHaveBeenCalled();

    rerender(
      <PermissionContext.Provider value={rights([P.PER_VIEW])}>
        <PersonnelCreateDialog />
      </PermissionContext.Provider>,
    );
    await waitFor(() => expect(svc.listPersonnel).toHaveBeenCalledTimes(1));
    expect(svc.listExternalCompanies).toHaveBeenCalledTimes(1);
    // Training list without trn.view: not asked, the picker names the missing right.
    expect(qual.listCertificationTypes).not.toHaveBeenCalled();
    expect(screen.getByTestId("list-access-certificationTypes")).toBeInTheDocument();
  });
});
