import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ReferenceImportWizard } from "@/components/lookups/ReferenceImportWizard";
import enReference from "@/i18n/locale-data/en/reference.json";

function lookup(catalog: unknown, key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      catalog,
    );
  return typeof value === "string" ? value : undefined;
}

vi.mock("react-i18next", () => ({
  useTranslation: (ns?: string) => ({
    t: (key: string, opts?: Record<string, unknown>) => {
      if (ns !== "reference") {
        return typeof opts?.["defaultValue"] === "string" ? opts["defaultValue"] : key;
      }
      const value = lookup(enReference, key) ?? lookup(enReference, `${key}_other`);
      if (value != null) return value;
      if (typeof opts?.["defaultValue"] === "string") return opts["defaultValue"];
      throw new Error(`missing reference i18n key: ${key}`);
    },
    i18n: { language: "en" },
  }),
}));

const svc = vi.hoisted(() => ({
  createRefImportBatch: vi.fn(),
  stageRefImportRows: vi.fn(),
  validateRefImportBatch: vi.fn(),
  getRefImportPreview: vi.fn(),
  applyRefImportBatch: vi.fn(),
}));
vi.mock("@/services/reference-service", () => svc);

const batch = {
  id: 3,
  domain_id: 7,
  source_filename: "values.csv",
  source_sha256: "x",
  status: "validated",
  total_rows: 1,
  valid_rows: 0,
  warning_rows: 0,
  error_rows: 1,
  initiated_by_id: 1,
  created_at: "2026-10-17T00:00:00Z",
  updated_at: "2026-10-17T00:00:00Z",
};

describe("ReferenceImportWizard", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    Object.defineProperty(globalThis, "crypto", {
      configurable: true,
      value: { subtle: { digest: vi.fn().mockResolvedValue(new ArrayBuffer(32)) } },
    });
    svc.createRefImportBatch.mockResolvedValue({ ...batch, status: "uploaded" });
    svc.stageRefImportRows.mockResolvedValue({ ...batch, status: "uploaded" });
    svc.validateRefImportBatch.mockResolvedValue(batch);
    svc.getRefImportPreview.mockResolvedValue({
      batch,
      rows: [
        {
          id: 1,
          batch_id: 3,
          row_no: 1,
          raw_json: "{}",
          normalized_code: "PUMP",
          validation_status: "error",
          proposed_action: null,
          messages: [
            {
              category: "InvalidParent",
              severity: "error",
              message: "Ligne 1: parent 'X' introuvable dans 'EQUIPMENT.FAMILY'.",
            },
          ],
        },
      ],
    });
  });

  it("shows a translated reason for each problem and keeps the server text under Detail", async () => {
    render(<ReferenceImportWizard domainId={7} targetSetId={9} open onOpenChange={vi.fn()} />);
    const dialog = await screen.findByRole("dialog");
    const input = dialog.querySelector('input[type="file"]') as HTMLInputElement;
    const file = new File(["code,label,parent_code\nPUMP,Pump,X"], "values.csv", {
      type: "text/csv",
    });
    Object.defineProperty(file, "text", {
      value: () => Promise.resolve("code,label,parent_code\nPUMP,Pump,X"),
    });
    fireEvent.change(input, { target: { files: [file] } });
    fireEvent.click(await within(dialog).findByRole("button", { name: enReference.import.next }));
    fireEvent.click(
      await within(dialog).findByRole("button", { name: enReference.import.runValidation }),
    );

    const message = await within(dialog).findByTestId("import-message");
    expect(message).toHaveTextContent(enReference.import.category.InvalidParent);
    expect(within(message).getByText(enReference.import.messageDetail)).toBeInTheDocument();
    expect(message).toHaveTextContent("parent 'X' introuvable");
  });

  it("says a coded problem in the user's language, without the server text", async () => {
    svc.getRefImportPreview.mockResolvedValue({
      batch,
      rows: [
        {
          id: 1,
          batch_id: 3,
          row_no: 1,
          raw_json: "{}",
          normalized_code: "PUMP",
          validation_status: "error",
          proposed_action: null,
          messages: [
            {
              category: "InvalidParent",
              severity: "error",
              code: "REF_IMPORT_INVALID_PARENT",
              params: { row: "1", parentCode: "X", parentDomain: "EQUIPMENT.FAMILY" },
              message: "Ligne 1: parent 'X' introuvable dans 'EQUIPMENT.FAMILY'.",
            },
          ],
        },
      ],
    });
    render(<ReferenceImportWizard domainId={7} targetSetId={9} open onOpenChange={vi.fn()} />);
    const dialog = await screen.findByRole("dialog");
    const input = dialog.querySelector('input[type="file"]') as HTMLInputElement;
    const file = new File(["code,label,parent_code\nPUMP,Pump,X"], "values.csv", {
      type: "text/csv",
    });
    Object.defineProperty(file, "text", {
      value: () => Promise.resolve("code,label,parent_code\nPUMP,Pump,X"),
    });
    fireEvent.change(input, { target: { files: [file] } });
    fireEvent.click(await within(dialog).findByRole("button", { name: enReference.import.next }));
    fireEvent.click(
      await within(dialog).findByRole("button", { name: enReference.import.runValidation }),
    );

    const message = await within(dialog).findByTestId("import-message");
    // (this test's t() shows the text as written, without filling the values)
    expect(message).toHaveTextContent(enReference.import.problem.REF_IMPORT_INVALID_PARENT);
    expect(within(message).queryByText(enReference.import.messageDetail)).toBeNull();
    expect(message).not.toHaveTextContent("introuvable");
  });
});
