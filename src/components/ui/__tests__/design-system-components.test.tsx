import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { Columns3, List } from "lucide-react";
import { describe, expect, it, vi } from "vitest";

import { ViewToggle } from "@/components/layout/ModuleHeaderControls";
import { KpiTile } from "@/components/metrics/KpiTile";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState, TruncationNotice } from "@/components/ui/data-state";
import { StatusChip } from "@/components/ui/status-chip";
import { WO_STATUS_TONE, mfTone, toneOf } from "@/design-system/tone";
import { describeAppError } from "@/lib/app-error";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && "shown" in opts ? `${key}:${String(opts.shown)}/${String(opts.total)}` : key,
    i18n: { language: "fr" },
  }),
}));

describe("tone vocabulary", () => {
  it("maps every work-order lifecycle status to a tone and unknown codes to neutral", () => {
    expect(toneOf(WO_STATUS_TONE, "in_progress")).toBe("progress");
    expect(toneOf(WO_STATUS_TONE, "cancelled")).toBe("danger");
    expect(toneOf(WO_STATUS_TONE, "not_a_status")).toBe("neutral");
    expect(toneOf(WO_STATUS_TONE, null)).toBe("neutral");
  });
});

describe("StatusChip", () => {
  it("renders the translated label with the tone classes, never a raw code", () => {
    render(<StatusChip label="En cours" tone="progress" dot />);
    const chip = screen.getByText("En cours").parentElement as HTMLElement;
    for (const cls of mfTone.progress.chip.split(" ")) {
      expect(chip.className).toContain(cls);
    }
  });
});

describe("ErrorState", () => {
  it("shows the translated failure, keeps technical details folded, and retries", () => {
    const onRetry = vi.fn();
    render(
      <ErrorState
        error={{ code: "UNKNOWN_CODE_FOR_TEST", message: "Validation failed: disk full" }}
        onRetry={onRetry}
      />,
    );
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(screen.getByText("ds.state.loadErrorTitle")).toBeInTheDocument();
    // Unknown codes fall back to the backend text, without the technical prefix.
    expect(screen.getByText("disk full")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "ds.state.retry" }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });
});

describe("describeAppError", () => {
  it("never yields [object Object] for plain IPC reject objects", () => {
    const d = describeAppError({ code: "SOMETHING", message: "Pompe introuvable" });
    expect(d.title).not.toContain("[object Object]");
    expect(`${d.title} ${d.description ?? ""}`).toContain("Pompe introuvable");
    expect(d.code).toBe("SOMETHING");
  });
});

describe("TruncationNotice", () => {
  it("is hidden when every record is shown", () => {
    const { container } = render(<TruncationNotice shown={10} total={10} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("says how many records are shown and loads more on demand", () => {
    const onLoadMore = vi.fn();
    render(<TruncationNotice shown={200} total={1240} onLoadMore={onLoadMore} />);
    expect(screen.getByText("ds.state.truncated:200/1240")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "ds.state.loadMore" }));
    expect(onLoadMore).toHaveBeenCalledTimes(1);
  });
});

describe("ViewToggle", () => {
  it("exposes each view as a labelled radio and hides permission-gated views", () => {
    const onChange = vi.fn();
    render(
      <ViewToggle
        value="list"
        onChange={onChange}
        options={[
          { value: "list", label: "Liste", icon: List },
          { value: "kanban", label: "Tableau", icon: Columns3 },
          { value: "secret", label: "Intégrité", icon: Columns3, hidden: true },
        ]}
      />,
    );
    expect(screen.getByRole("radio", { name: "Liste" })).toHaveAttribute("aria-checked", "true");
    expect(screen.queryByRole("radio", { name: "Intégrité" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("radio", { name: "Tableau" }));
    expect(onChange).toHaveBeenCalledWith("kanban");
  });
});

describe("KpiTile", () => {
  it("is a button that opens the list behind the figure when onOpen is given", () => {
    const onOpen = vi.fn();
    render(<KpiTile label="En retard" value="12" tone="danger" onOpen={onOpen} />);
    fireEvent.click(screen.getByRole("button"));
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it("is static text without onOpen", () => {
    render(<KpiTile label="Total" value="40" />);
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.getByText("40")).toBeInTheDocument();
  });
});

describe("ConfirmDialog", () => {
  it("runs the action, then closes", async () => {
    const onConfirm = vi.fn().mockResolvedValue(undefined);
    const onOpenChange = vi.fn();
    render(
      <ConfirmDialog
        open
        onOpenChange={onOpenChange}
        description="L'article sera masqué des opérations."
        confirmLabel="Désactiver l'article"
        destructive
        onConfirm={onConfirm}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Désactiver l'article" }));
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("stays open when the action fails", async () => {
    const onConfirm = vi.fn().mockRejectedValue({ code: "X", message: "refused" });
    const onOpenChange = vi.fn();
    render(
      <ConfirmDialog
        open
        onOpenChange={onOpenChange}
        description="d"
        confirmLabel="Supprimer"
        onConfirm={onConfirm}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Supprimer" }));
    await waitFor(() => expect(onConfirm).toHaveBeenCalled());
    expect(onOpenChange).not.toHaveBeenCalledWith(false);
  });
});
