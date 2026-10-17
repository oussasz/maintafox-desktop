import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AssetEditForm } from "@/components/assets/AssetEditForm";
import type * as EntityForm from "@/components/entity-form";
import { PermissionContext, type PermissionContextValue } from "@/contexts/PermissionContext";
import { useAssetStore } from "@/stores/asset-store";
import { useEquipmentTaxonomyStore } from "@/stores/equipment-taxonomy-store";
import type { Asset, EquipmentTaxonomyCatalog } from "@shared/ipc-types";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

const i18nMock = vi.hoisted(() => ({
  t: (key: string) => key,
  i18n: { language: "fr" },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));

const lifecycle = vi.hoisted(() => ({
  listAssetPhotos: vi.fn(),
  readAssetPhotoPreview: vi.fn(),
  deleteAssetPhoto: vi.fn(),
  uploadAssetPhoto: vi.fn(),
}));
vi.mock("@/services/asset-lifecycle-service", () => lifecycle);
vi.mock("@/services/asset-service", () => ({
  getEquipmentTaxonomyCatalog: vi.fn(() => Promise.resolve(null)),
}));
const orgSvc = vi.hoisted(() => ({ listOrgTree: vi.fn(() => Promise.resolve([])) }));
vi.mock("@/services/org-node-service", () => orgSvc);

// Reference lists have their own tests: here only what the form passes them matters.
vi.mock("@/components/reference/ReferenceCombobox", () => ({
  ReferenceCombobox: (props: {
    id: string;
    disabled?: boolean;
    excludeCodes?: readonly string[];
  }) => (
    <div data-testid={props.id} data-disabled={String(Boolean(props.disabled))}>
      {(props.excludeCodes ?? []).join(",")}
    </div>
  ),
}));
// The gallery is reduced to "the user removed every photo".
vi.mock("@/components/entity-form", async (importOriginal) => ({
  ...(await importOriginal<typeof EntityForm>()),
  EntityFormImageUploader: (props: {
    items: { id: string }[];
    emptyLabel: string;
    onChange: (next: { id: string }[]) => void;
  }) => (
    <div>
      <span>{props.items.length === 0 ? props.emptyLabel : `${props.items.length} photos`}</span>
      <button type="button" onClick={() => props.onChange([])}>
        remove all photos
      </button>
    </div>
  ),
}));

const opt = (id: number, code: string, tag: string) => ({
  id,
  code,
  label: code,
  parent_id: null,
  color_hex: null,
  semantic_tag: tag,
  is_system: true,
});
const CATALOG: EquipmentTaxonomyCatalog = {
  statuses: [
    opt(1, "ACTIVE_IN_SERVICE", "up"),
    opt(2, "DECOMMISSIONED", "retired"),
    opt(3, "SCRAPPED", "retired"),
  ],
  criticalities: [],
  classes: [],
  families: [],
  subfamilies: [],
};

const ASSET = {
  id: 5,
  asset_code: "PMP-101",
  asset_name: "Pompe",
  class_code: "PUMP",
  criticality_code: "A",
  status_code: "ACTIVE_IN_SERVICE",
  maintainable_boundary: true,
  org_node_id: 1,
  decommissioned_at: null,
  row_version: 1,
  updated_at: "2026-10-01T10:00:00Z",
} as unknown as Asset;

function open(asset: Asset) {
  useAssetStore.setState({ showEditForm: true, editingAsset: asset });
  render(<AssetEditForm />);
}

beforeEach(() => {
  vi.clearAllMocks();
  useEquipmentTaxonomyStore.setState({ catalog: CATALOG, loading: false, error: null });
  lifecycle.listAssetPhotos.mockResolvedValue([{ id: 41, file_name: "a.jpg" }]);
  lifecycle.readAssetPhotoPreview.mockResolvedValue({ mime_type: "image/jpeg", data_base64: "" });
  lifecycle.deleteAssetPhoto.mockResolvedValue(undefined);
});

describe("AssetEditForm", () => {
  it("names the missing right for the location instead of an empty list", async () => {
    const can = (p: PermissionName) => p !== P.ORG_VIEW;
    const perms: PermissionContextValue = {
      permissions: [],
      isLoading: false,
      can,
      canAny: (...ps) => ps.some(can),
      canAll: (...ps) => ps.every(can),
      refresh: vi.fn(),
    };
    useAssetStore.setState({ showEditForm: true, editingAsset: ASSET });
    render(
      <PermissionContext.Provider value={perms}>
        <AssetEditForm />
      </PermissionContext.Provider>,
    );
    expect(await screen.findByTestId("list-access-orgNodes")).toBeInTheDocument();
    expect(orgSvc.listOrgTree).not.toHaveBeenCalled();
  });

  it("never offers a retired status: retirement goes through Decommission", async () => {
    open(ASSET);
    const status = await screen.findByTestId("status_code");
    expect(status).toHaveTextContent("DECOMMISSIONED,SCRAPPED");
    expect(status).toHaveAttribute("data-disabled", "false");
    expect(screen.getByText("editForm.retireHint")).toBeInTheDocument();
    expect(screen.queryByLabelText("detail.fields.endOfLifeDate")).not.toBeInTheDocument();
    expect(screen.queryByText("createForm.sections.attachments")).not.toBeInTheDocument();
  });

  it("shows a retired asset's status and end of life read-only", async () => {
    open({ ...ASSET, status_code: "SCRAPPED", decommissioned_at: "2026-09-30" } as Asset);
    const status = await screen.findByTestId("status_code");
    expect(status).toHaveAttribute("data-disabled", "true");
    expect(screen.getByText("editForm.retiredHint")).toBeInTheDocument();
    expect(screen.getByLabelText("detail.fields.endOfLifeDate")).toHaveAttribute("readonly");
  });

  it("asks before deleting a stored photo, and keeps it when cancelled", async () => {
    open(ASSET);
    expect(await screen.findByText("1 photos")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "remove all photos" }));
    const ask = await screen.findByRole("dialog", { name: "editForm.photoDeleteTitle" });
    expect(lifecycle.deleteAssetPhoto).not.toHaveBeenCalled();
    fireEvent.click(within(ask).getByRole("button", { name: "ds.confirm.cancel" }));
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "editForm.photoDeleteTitle" }),
      ).not.toBeInTheDocument(),
    );
    expect(screen.getByText("1 photos")).toBeInTheDocument();
    expect(lifecycle.deleteAssetPhoto).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "remove all photos" }));
    const again = await screen.findByRole("dialog", { name: "editForm.photoDeleteTitle" });
    lifecycle.listAssetPhotos.mockResolvedValue([]);
    fireEvent.click(within(again).getByRole("button", { name: "editForm.photoDeleteConfirm" }));
    await waitFor(() => expect(lifecycle.deleteAssetPhoto).toHaveBeenCalledWith(41));
    expect(await screen.findByText("editForm.imagesEmpty")).toBeInTheDocument();
  });

  it("says the photo list could not be read instead of showing an empty gallery", async () => {
    lifecycle.listAssetPhotos.mockRejectedValue({ code: "DATABASE_ERROR", message: "busy" });
    open(ASSET);
    await waitFor(() => expect(screen.queryByText("editForm.imagesEmpty")).not.toBeInTheDocument());
    expect(lifecycle.listAssetPhotos).toHaveBeenCalled();
    expect(screen.queryByText("0 photos")).not.toBeInTheDocument();
  });
});
