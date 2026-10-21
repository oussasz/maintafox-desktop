import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PersonnelImportWizard } from "@/components/personnel/PersonnelImportWizard";
import enPersonnel from "@/i18n/locale-data/en/personnel.json";

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
      if (ns !== "personnel")
        return typeof opts?.["defaultValue"] === "string" ? opts["defaultValue"] : key;
      const value = lookup(enPersonnel, key);
      if (value != null) return value;
      if (typeof opts?.["defaultValue"] === "string") return opts["defaultValue"];
      throw new Error(`missing personnel i18n key: ${key}`);
    },
    i18n: { language: "en" },
  }),
}));

vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => true, canAny: () => true, isLoading: false }),
}));

const svc = vi.hoisted(() => ({
  createPersonnelImportBatch: vi.fn(),
  getPersonnelImportPreview: vi.fn(),
  applyPersonnelImportBatch: vi.fn(),
}));
vi.mock("@/services/personnel-service", () => svc);

const batch = {
  id: 5,
  source_filename: "people.csv",
  source_kind: "csv",
  mode: "create_and_update",
  status: "validated",
  total_rows: 1,
  valid_rows: 0,
  warning_rows: 0,
  error_rows: 1,
};

describe("PersonnelImportWizard", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    Object.defineProperty(globalThis, "crypto", {
      configurable: true,
      value: { subtle: { digest: vi.fn().mockResolvedValue(new ArrayBuffer(32)) } },
    });
    svc.createPersonnelImportBatch.mockResolvedValue(batch);
    svc.getPersonnelImportPreview.mockResolvedValue({
      batch,
      rows: [
        {
          id: 1,
          row_no: 1,
          employee_code: "E-1",
          validation_status: "error",
          proposed_action: "create",
          messages: [
            {
              category: "FieldRule.Mapped",
              severity: "error",
              message: "Unknown team_code 'XX'.",
            },
          ],
        },
      ],
    });
  });

  it("shows a translated reason for each problem and keeps the server text under Detail", async () => {
    render(<PersonnelImportWizard />);
    fireEvent.click(screen.getByRole("button", { name: enPersonnel.import.action.openWizard }));
    const dialog = await screen.findByRole("dialog");
    const input = dialog.querySelector('input[type="file"]') as HTMLInputElement;
    const file = new File(["employee_code\nE-1"], "people.csv", { type: "text/csv" });
    Object.defineProperty(file, "arrayBuffer", {
      value: () => Promise.resolve(new ArrayBuffer(8)),
    });
    fireEvent.change(input, { target: { files: [file] } });

    const message = await within(dialog).findByTestId("import-message");
    expect(message).toHaveTextContent(enPersonnel.import.category.FieldRule_Mapped);
    expect(message.textContent).not.toContain("[FieldRule.Mapped]");
    expect(within(message).getByText(enPersonnel.import.messageDetail)).toBeInTheDocument();
    expect(message).toHaveTextContent("Unknown team_code 'XX'.");
  });

  it("says a coded problem in the user's language, without the server text", async () => {
    svc.getPersonnelImportPreview.mockResolvedValue({
      batch,
      rows: [
        {
          id: 1,
          row_no: 1,
          employee_code: "E-1",
          validation_status: "error",
          proposed_action: "create",
          messages: [
            {
              category: "FieldRule.Mapped",
              severity: "error",
              code: "PERSONNEL_IMPORT_UNKNOWN_TEAM",
              params: { code: "XX" },
              message: "Unknown team_code 'XX'.",
            },
          ],
        },
      ],
    });
    render(<PersonnelImportWizard />);
    fireEvent.click(screen.getByRole("button", { name: enPersonnel.import.action.openWizard }));
    const dialog = await screen.findByRole("dialog");
    const input = dialog.querySelector('input[type="file"]') as HTMLInputElement;
    const file = new File(["employee_code\nE-1"], "people.csv", { type: "text/csv" });
    Object.defineProperty(file, "arrayBuffer", {
      value: () => Promise.resolve(new ArrayBuffer(8)),
    });
    fireEvent.change(input, { target: { files: [file] } });

    const message = await within(dialog).findByTestId("import-message");
    // (this test's t() shows the text as written, without filling the values)
    expect(message).toHaveTextContent(enPersonnel.import.problem.PERSONNEL_IMPORT_UNKNOWN_TEAM);
    expect(within(message).queryByText(enPersonnel.import.messageDetail)).toBeNull();
    expect(message).not.toHaveTextContent("Unknown team_code 'XX'.");
  });
});
