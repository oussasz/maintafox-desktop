/**
 * L7-16: the work order shows the documents of its equipment from the document library, so the
 * technician finds the machine manual from the order. No equipment, no card.
 */
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useDocumentCategoriesStore } from "@/stores/document-categories-store";

const svc = vi.hoisted(() => ({
  listLibraryDocuments: vi.fn(),
  listDocumentCategories: vi.fn(),
  getLibraryDocumentFile: vi.fn(),
}));
vi.mock("@/services/documentation-service", () => svc);

const perms = vi.hoisted(() => ({ canView: true }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => perms.canView }),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts?.["asset"] ? `${key}:${String(opts["asset"])}` : key,
    i18n: { language: "fr" },
  }),
}));

import { WoAssetDocumentsCard } from "../WoAssetDocumentsCard";

beforeEach(() => {
  vi.clearAllMocks();
  perms.canView = true;
  useDocumentCategoriesStore.setState({ categories: null, loading: false, error: null });
  svc.listDocumentCategories.mockResolvedValue([
    { code: "manuals", label: "Manuels", label_en: null, review_required: false },
  ]);
  svc.listLibraryDocuments.mockResolvedValue([
    {
      id: 4,
      category: "manuals",
      equipmentId: 7,
      equipmentCode: "P-101",
      equipmentName: "Pompe",
      title: "Manuel pompe",
      fileName: "manuel.pdf",
      relativePath: "library/manuel.pdf",
      mimeType: "application/pdf",
      sizeBytes: 2048,
      uploadedById: 1,
      uploadedAt: "2026-10-01T08:00:00Z",
      notes: null,
    },
  ]);
});

describe("WoAssetDocumentsCard", () => {
  it("lists the documents of the order's equipment", async () => {
    render(<WoAssetDocumentsCard equipmentId={7} assetLabel="P-101 Pompe" />);
    expect(screen.getByText("documents.onWoTitle:P-101 Pompe")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByText("Manuel pompe")).toBeInTheDocument());
    expect(svc.listLibraryDocuments).toHaveBeenCalledWith(null, 7);
    expect(screen.getByRole("button", { name: "documents.downloadNamed" })).toBeInTheDocument();
  });

  it("shows nothing when the order has no equipment", () => {
    const { container } = render(<WoAssetDocumentsCard equipmentId={null} assetLabel="" />);
    expect(container).toBeEmptyDOMElement();
    expect(svc.listLibraryDocuments).not.toHaveBeenCalled();
  });

  it("says the documentation right is needed instead of loading", () => {
    perms.canView = false;
    render(<WoAssetDocumentsCard equipmentId={7} assetLabel="P-101" />);
    expect(screen.getByText("documents.noPermission")).toBeInTheDocument();
    expect(svc.listLibraryDocuments).not.toHaveBeenCalled();
  });
});
