/**
 * Document library category page — states, explicit upload, confirmed delete.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mockList = vi.fn();
const mockUpload = vi.fn();
const mockDelete = vi.fn();

vi.mock("@/services/documentation-service", () => ({
  MAX_LIBRARY_DOCUMENT_BYTES: 25 * 1024 * 1024,
  listLibraryDocuments: (...a: unknown[]) => mockList(...a),
  uploadLibraryDocument: (...a: unknown[]) => mockUpload(...a),
  deleteLibraryDocument: (...a: unknown[]) => mockDelete(...a),
  updateLibraryDocument: vi.fn(),
  getLibraryDocumentFile: vi.fn(),
  fileToNumberArray: vi.fn().mockResolvedValue([1, 2, 3]),
}));

vi.mock("@/services/asset-service", () => ({
  getAssetByIdSilent: vi.fn().mockResolvedValue(null),
}));

vi.mock("@/components/sync/MediaFileStatus", () => ({
  useMediaFileStates: () => ({ states: new Map(), refresh: vi.fn() }),
  MediaFileStateBadge: () => null,
  isMediaFileAvailable: () => true,
}));

vi.mock("@/components/assets/AssetPicker", () => ({
  AssetPicker: ({ id }: { id?: string }) => <input id={id} aria-label="asset-picker" readOnly />,
}));

vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => true }),
}));

vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: ReactNode }) => <>{children}</>,
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "fr", changeLanguage: vi.fn() },
  }),
}));

import { DocumentationCategoryPage } from "../DocumentationCategoryPage";

const doc = {
  id: 7,
  category: "sops",
  equipmentId: null,
  equipmentCode: null,
  equipmentName: null,
  title: "Lubrication SOP",
  fileName: "lube.pdf",
  relativePath: "library/lube.pdf",
  mimeType: "application/pdf",
  sizeBytes: 2048,
  uploadedById: 1,
  uploadedAt: "2026-10-01T08:00:00Z",
  notes: null,
};

function renderPage() {
  return render(
    <MemoryRouter initialEntries={["/documentation/sops"]}>
      <Routes>
        <Route path="/documentation/:categorySlug" element={<DocumentationCategoryPage />} />
      </Routes>
    </MemoryRouter>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("DocumentationCategoryPage", () => {
  it("shows a load failure with retry, never an empty library", async () => {
    mockList.mockRejectedValueOnce({ code: "DB", message: "down" });
    renderPage();

    expect(await screen.findByText("list.loadError")).toBeInTheDocument();
    expect(screen.queryByText("list.empty")).not.toBeInTheDocument();

    mockList.mockResolvedValueOnce([doc]);
    fireEvent.click(screen.getByRole("button", { name: "ds.state.retry" }));
    expect(await screen.findByText("Lubrication SOP")).toBeInTheDocument();
    expect(mockList).toHaveBeenLastCalledWith("sops", null);
  });

  it("uploads only when the user presses Upload, with the suggested title", async () => {
    mockList.mockResolvedValue([]);
    mockUpload.mockResolvedValueOnce({ ...doc, id: 8 });
    renderPage();

    fireEvent.click(await screen.findByRole("button", { name: "upload.open" }));
    const file = new File(["abc"], "pump-manual.pdf", { type: "application/pdf" });
    fireEvent.change(screen.getByLabelText("upload.fields.file"), { target: { files: [file] } });

    // Picking the file does not upload anything.
    expect(mockUpload).not.toHaveBeenCalled();
    expect(screen.getByLabelText(/upload\.fields\.title/)).toHaveValue("pump-manual");

    fireEvent.click(screen.getByRole("button", { name: "upload.submit" }));
    await waitFor(() =>
      expect(mockUpload).toHaveBeenCalledWith(
        expect.objectContaining({
          category: "sops",
          title: "pump-manual",
          fileName: "pump-manual.pdf",
          equipmentId: null,
        }),
      ),
    );
  });

  it("asks before deleting a document", async () => {
    mockList.mockResolvedValue([doc]);
    mockDelete.mockResolvedValueOnce(undefined);
    renderPage();

    fireEvent.click(await screen.findByRole("button", { name: "list.deleteNamed" }));
    expect(await screen.findByText("confirm.deleteTitle")).toBeInTheDocument();
    expect(mockDelete).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "confirm.deleteAction" }));
    await waitFor(() => expect(mockDelete).toHaveBeenCalledWith(7));
  });
});
