/**
 * Asset documents tab: document categories come from the plant's list (Données de référence),
 * named by exact code; a category under review, or missing from the list, is marked.
 */
import { render, screen, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useDocumentCategoriesStore } from "@/stores/document-categories-store";
import type { Asset } from "@shared/ipc-types";

const svc = vi.hoisted(() => ({
  listLibraryDocuments: vi.fn(),
  listDocumentCategories: vi.fn(),
}));
vi.mock("@/services/documentation-service", () => ({
  MAX_LIBRARY_DOCUMENT_BYTES: 25 * 1024 * 1024,
  fileToNumberArray: vi.fn(),
  getLibraryDocumentFile: vi.fn(),
  uploadLibraryDocument: vi.fn(),
  ...svc,
}));

vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => true }),
}));
vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: ReactNode }) => <>{children}</>,
}));
vi.mock("@/components/assets/AssetPhotoGallery", () => ({ AssetPhotoGallery: () => null }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "fr" },
  }),
}));

import { AssetDocumentsTab } from "../detail/AssetDocumentsTab";

function doc(id: number, category: string, title: string) {
  return {
    id,
    category,
    equipmentId: 3,
    equipmentCode: "P-101",
    equipmentName: "Pompe",
    title,
    fileName: `${title}.pdf`,
    relativePath: `library/${title}.pdf`,
    mimeType: "application/pdf",
    sizeBytes: 2048,
    uploadedById: 1,
    uploadedAt: "2026-10-01T08:00:00Z",
    notes: null,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  useDocumentCategoriesStore.setState({ categories: null, loading: false, error: null });
  svc.listDocumentCategories.mockResolvedValue([
    { code: "sops", label: "Modes opératoires", label_en: null, review_required: false },
    { code: "Plans", label: "Plans", label_en: null, review_required: true },
  ]);
  svc.listLibraryDocuments.mockResolvedValue([
    doc(1, "sops", "Graissage"),
    doc(2, "Plans", "Schéma"),
    doc(3, "SOPS", "Ancien"),
  ]);
});

describe("AssetDocumentsTab", () => {
  it("names each document's category from the plant's list, by exact code", async () => {
    render(<AssetDocumentsTab asset={{ id: 3, asset_code: "P-101" } as Asset} />);

    const row = (title: string) => screen.getByText(title).closest("li") as HTMLElement;
    await screen.findByText("Graissage");
    await within(row("Graissage")).findByText(/Modes opératoires/);
    expect(within(row("Graissage")).queryByText("listValue.review")).toBeNull();
    // Under review in the list.
    expect(within(row("Schéma")).getByText("listValue.review")).toBeInTheDocument();
    // "SOPS" is not the code "sops": shown as stored, marked for review.
    expect(within(row("Ancien")).getByText(/SOPS/)).toBeInTheDocument();
    expect(within(row("Ancien")).getByText("listValue.review")).toBeInTheDocument();
  });
});
