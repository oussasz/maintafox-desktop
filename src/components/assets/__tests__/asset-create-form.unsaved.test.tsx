import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AssetCreateForm } from "@/components/assets/AssetCreateForm";
import type * as EntityForm from "@/components/entity-form";
import { useAssetStore } from "@/stores/asset-store";
import { useEquipmentTaxonomyStore } from "@/stores/equipment-taxonomy-store";

// A stable `t`: the form's effects depend on it.
const i18nMock = vi.hoisted(() => ({ t: (key: string) => key, i18n: { language: "fr" } }));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));
vi.mock("@/services/asset-service", () => ({
  getEquipmentTaxonomyCatalog: vi.fn(() => Promise.resolve(null)),
}));
vi.mock("@/services/org-node-service", () => ({ listOrgTree: vi.fn(() => Promise.resolve([])) }));
vi.mock("@/services/asset-lifecycle-service", () => ({
  uploadAssetPhoto: vi.fn(),
}));
// The gallery is not under test; the native pickers it loads need the desktop shell.
vi.mock("@/components/entity-form", async (importOriginal) => ({
  ...(await importOriginal<typeof EntityForm>()),
  EntityFormImageUploader: () => null,
}));
vi.mock("@/components/reference/ReferenceCombobox", () => ({ ReferenceCombobox: () => null }));

const closeCreateForm = vi.fn();
beforeEach(() => {
  vi.clearAllMocks();
  useEquipmentTaxonomyStore.setState({
    catalog: { statuses: [], criticalities: [], classes: [], families: [], subfamilies: [] },
    loading: false,
    error: null,
  });
  useAssetStore.setState({
    showCreateForm: true,
    parentPreFill: null,
    closeCreateForm,
  } as never);
});

describe("AssetCreateForm unsaved input", () => {
  it("closes at once on Cancel when nothing was typed", async () => {
    render(<AssetCreateForm />);
    fireEvent.click(await screen.findByRole("button", { name: "decommission.cancel" }));
    expect(closeCreateForm).toHaveBeenCalledTimes(1);
  });

  it("asks on Cancel once a field was typed, and closes only when confirmed", async () => {
    render(<AssetCreateForm />);
    const code = await screen.findByPlaceholderText("form.identity.code.placeholder");
    fireEvent.change(code, { target: { value: "PMP-200" } });
    fireEvent.click(screen.getByRole("button", { name: "decommission.cancel" }));
    expect(closeCreateForm).not.toHaveBeenCalled();
    fireEvent.click(await screen.findByRole("button", { name: "discardGuard.leave" }));
    await waitFor(() => expect(closeCreateForm).toHaveBeenCalledTimes(1));
  });
});

describe("AssetCreateForm disabled reason", () => {
  it("says the lists are loading while Créer is grey, then says nothing", async () => {
    render(<AssetCreateForm />);
    const create = screen.getByRole("button", { name: "createForm.submit" });
    expect(create).toBeDisabled();
    expect(screen.getByText("actionReason.catalogLoading")).toBeInTheDocument();
    await waitFor(() => expect(create).toBeEnabled());
    expect(screen.queryByText("actionReason.catalogLoading")).not.toBeInTheDocument();
  });
});
