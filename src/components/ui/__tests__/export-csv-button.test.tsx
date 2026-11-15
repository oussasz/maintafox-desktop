import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useAppToastStore } from "@/store/app-toast-store";
import { P } from "@shared/rbac/permissions.generated";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && "shown" in opts ? `${key}:${String(opts["shown"])}/${String(opts["total"])}` : key,
  }),
}));

const allowed = { value: true };
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({
    can: () => allowed.value,
    canAny: () => allowed.value,
    isLoading: false,
  }),
}));

const saveCsvWithDialog = vi.fn();
vi.mock("@/export/csv", () => ({
  saveCsvWithDialog: (...args: unknown[]) => saveCsvWithDialog(...args),
}));

const { ExportCsvButton } = await import("@/components/ui/ExportCsvButton");

const header = ["Code", "Statut"];
const rows = [
  ["OT-1", "En cours"],
  ["OT-2", "Clôturé"],
];

beforeEach(() => {
  saveCsvWithDialog.mockReset();
  allowed.value = true;
  useAppToastStore.setState({ items: [] });
});

describe("ExportCsvButton", () => {
  it("writes the header and the rows the list shows, then reports the saved path", async () => {
    saveCsvWithDialog.mockResolvedValue("C:/exports/ot.csv");
    render(
      <ExportCsvButton
        permission={P.OT_VIEW}
        fileName="ot.csv"
        header={header}
        getRows={() => rows}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "listExport.button" }));
    await waitFor(() => expect(saveCsvWithDialog).toHaveBeenCalledTimes(1));
    expect(saveCsvWithDialog).toHaveBeenCalledWith("ot.csv", header, rows, "listExport.title");
    await waitFor(() => {
      const [toast] = useAppToastStore.getState().items;
      expect(toast?.variant).toBe("success");
      expect(toast?.description).toBe("C:/exports/ot.csv");
    });
  });

  it("says when the list holds only part of the rows", async () => {
    saveCsvWithDialog.mockResolvedValue("C:/exports/ot.csv");
    render(
      <ExportCsvButton
        permission={P.OT_VIEW}
        fileName="ot.csv"
        header={header}
        getRows={() => rows}
        total={50}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "listExport.button" }));
    await waitFor(() => {
      const [toast] = useAppToastStore.getState().items;
      expect(toast?.description).toContain("listExport.truncated:2/50");
    });
  });

  it("does not open the save dialog for an empty list", () => {
    render(
      <ExportCsvButton
        permission={P.OT_VIEW}
        fileName="ot.csv"
        header={header}
        getRows={() => []}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "listExport.button" }));
    expect(saveCsvWithDialog).not.toHaveBeenCalled();
    expect(useAppToastStore.getState().items[0]?.title).toBe("listExport.empty");
  });

  it("stays silent when the user cancels the dialog and reports a real failure", async () => {
    saveCsvWithDialog.mockRejectedValueOnce({ code: "EXPORT_CANCELLED" });
    render(
      <ExportCsvButton
        permission={P.OT_VIEW}
        fileName="ot.csv"
        header={header}
        getRows={() => rows}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "listExport.button" }));
    await waitFor(() => expect(saveCsvWithDialog).toHaveBeenCalledTimes(1));
    expect(useAppToastStore.getState().items).toHaveLength(0);

    saveCsvWithDialog.mockRejectedValueOnce(new Error("disque plein"));
    fireEvent.click(screen.getByRole("button", { name: "listExport.button" }));
    await waitFor(() => {
      expect(useAppToastStore.getState().items[0]?.variant).toBe("destructive");
    });
    expect(useAppToastStore.getState().items[0]?.title).toBe("listExport.failed");
  });

  it("is hidden without the read permission", () => {
    allowed.value = false;
    render(
      <ExportCsvButton
        permission={P.OT_VIEW}
        fileName="ot.csv"
        header={header}
        getRows={() => rows}
      />,
    );
    expect(screen.queryByRole("button")).toBeNull();
  });
});
