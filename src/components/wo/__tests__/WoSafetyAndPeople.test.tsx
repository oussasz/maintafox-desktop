import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";

import enOt from "@/i18n/locale-data/en/ot.json";
import type { WoListFilter, WorkOrder } from "@shared/ipc-types";

import { WoKanbanView, filtersArchiveOnly, woLanes } from "../WoKanbanView";
import { WoPermitRequirementField } from "../WoPermitRequirementField";
import { WoShutdownBadge, WoShutdownRequirementField } from "../WoShutdownRequirementField";
import { mergeWoPersonOptions, toWoPersonOptions, woPersonLabel } from "../use-wo-people";
import { woActionFailureText } from "../wo-action-error";

function lookup(key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>((node, part) => (node as Record<string, unknown> | undefined)?.[part], enOt);
  return typeof value === "string" ? value : undefined;
}

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      lookup(key) ?? (typeof opts?.["defaultValue"] === "string" ? opts["defaultValue"] : key),
    i18n: { language: "en" },
  }),
}));

const woSvc = vi.hoisted(() => ({ listWos: vi.fn() }));
vi.mock("@/services/wo-service", () => woSvc);

const BOARD_FILTER: WoListFilter = { limit: 100, offset: 0 };

vi.mock("@/lib/app-error", () => ({
  describeAppError: (err: unknown) => ({
    title: err instanceof Error ? `translated: ${err.message}` : "translated",
    description: null,
    code: null,
  }),
}));

describe("WoPermitRequirementField", () => {
  it("shows the label and help text and reports changes", () => {
    const onChange = vi.fn();
    render(<WoPermitRequirementField checked={false} onCheckedChange={onChange} />);
    const box = screen.getByRole("checkbox", { name: enOt.permitRequirement.label });
    expect(box).toHaveAccessibleDescription(enOt.permitRequirement.help);
    fireEvent.click(box);
    expect(onChange).toHaveBeenCalledWith(true);
  });

  it("explains why it is locked once execution started", () => {
    render(
      <WoPermitRequirementField
        checked
        onCheckedChange={vi.fn()}
        disabled
        disabledReason={enOt.permitRequirement.frozen}
      />,
    );
    expect(screen.getByRole("checkbox")).toBeDisabled();
    expect(screen.getByText(enOt.permitRequirement.frozen)).toBeInTheDocument();
  });
});

describe("woActionFailureText", () => {
  it("keeps only the context for IPC failures (the global toast shows the reason)", () => {
    expect(
      woActionFailureText("Could not start", { code: "VALIDATION_FAILED", message: "x" }),
    ).toBe("Could not start");
    const conflict = new Error("version");
    conflict.name = "VersionConflictError";
    expect(woActionFailureText("Could not start", conflict)).toBe("Could not start");
  });

  it("adds the translated reason for client-side failures that raise no toast", () => {
    expect(woActionFailureText("Could not start", new Error("parse"))).toBe(
      "Could not start — translated: parse",
    );
  });
});

describe("WO people options (personnel register)", () => {
  const candidates = [
    {
      user_id: 12,
      personnel_id: 3,
      employee_code: "EMP-003",
      full_name: "Yacine Benali",
      position_name: "Électricien",
      availability_status: "available",
    },
    {
      user_id: 9,
      personnel_id: 1,
      employee_code: "EMP-001",
      full_name: "Amel Haddad",
      position_name: null,
      availability_status: "available",
    },
  ];

  it("labels people by name and employee code, keyed by login id, sorted", () => {
    expect(woPersonLabel(candidates[0] as (typeof candidates)[number])).toBe(
      "Yacine Benali (EMP-003)",
    );
    expect(toWoPersonOptions(candidates)).toEqual([
      { value: "9", label: "Amel Haddad (EMP-001)" },
      { value: "12", label: "Yacine Benali (EMP-003)" },
    ]);
  });

  it("keeps already-booked people selectable without duplicates", () => {
    const merged = mergeWoPersonOptions(toWoPersonOptions(candidates), [
      { value: "12", label: "Yacine" },
      { value: "40", label: "Former contractor" },
    ]);
    expect(merged.map((o) => o.value)).toEqual(["9", "40", "12"]);
  });
});

describe("WoKanbanView", () => {
  it("has no lane for closed or cancelled work orders: they live in the archive", () => {
    const statuses = woLanes(null).flatMap((l) => l.statuses);
    expect(statuses).not.toContain("closed");
    expect(statuses).not.toContain("cancelled");
    expect(woLanes(null).map((l) => l.id)).toEqual([
      "draft",
      "planning",
      "ready",
      "executing",
      "completed",
    ]);
  });

  it("points to the archive when the status filter is only closed or cancelled", async () => {
    expect(filtersArchiveOnly(["closed"])).toBe(true);
    expect(filtersArchiveOnly(["closed", "cancelled"])).toBe(true);
    expect(filtersArchiveOnly(["closed", "ready"])).toBe(false);
    expect(filtersArchiveOnly(null)).toBe(false);
    woSvc.listWos.mockClear();
    render(
      <MemoryRouter>
        <WoKanbanView
          filter={{ ...BOARD_FILTER, status_codes: ["closed"] }}
          onCardClick={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(await screen.findByTestId("wo-kanban-archive-pointer")).toHaveTextContent(
      enOt.kanban.archiveOnlyNotice,
    );
    expect(woSvc.listWos).not.toHaveBeenCalled();
    // A mixed filter still loads the active lanes it asks for, and none of the archived ones.
    expect(woLanes(["closed", "ready"]).flatMap((l) => l.statuses)).toEqual(["ready"]);
  });

  it("does not offer drag-and-drop and says where status changes happen", async () => {
    const wo = {
      id: 1,
      code: "OT-0001",
      title: "Replace bearing",
      status_code: "planning",
      urgency_id: null,
      urgency_label: null,
      asset_label: null,
      responsible_username: null,
      planned_end: null,
    } as unknown as WorkOrder;
    const onOpen = vi.fn();
    woSvc.listWos.mockImplementation(({ status_codes }: WoListFilter) =>
      Promise.resolve(
        status_codes?.includes("planning") ? { items: [wo], total: 1 } : { items: [], total: 0 },
      ),
    );
    render(
      <MemoryRouter>
        <WoKanbanView filter={BOARD_FILTER} onCardClick={onOpen} />
      </MemoryRouter>,
    );
    expect(await screen.findByTestId("wo-kanban-drag-hint")).toHaveTextContent(
      enOt.kanban.statusFromDetailHint,
    );
    const card = screen.getByText("Replace bearing").closest("[draggable]");
    expect(card?.getAttribute("draggable")).not.toBe("true");
  });

  it("loads a lane's next page from the server and counts the lane's real total", async () => {
    const page = (from: number, n: number) =>
      Array.from({ length: n }, (_, i) => ({
        id: from + i,
        code: `OT-${from + i}`,
        title: `Ready job ${from + i}`,
        status_code: "ready",
      })) as unknown as WorkOrder[];
    woSvc.listWos.mockReset();
    woSvc.listWos.mockImplementation(({ status_codes, offset }: WoListFilter) =>
      Promise.resolve(
        status_codes?.includes("ready")
          ? { items: offset === 0 ? page(1, 20) : page(21, 5), total: 25 }
          : { items: [], total: 0 },
      ),
    );
    render(
      <MemoryRouter>
        <WoKanbanView filter={{ ...BOARD_FILTER, search: "pump" }} onCardClick={vi.fn()} />
      </MemoryRouter>,
    );
    expect(await screen.findByText("Ready job 20")).toBeInTheDocument();
    expect(screen.getByText("25")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Load more/ }));
    expect(await screen.findByText("Ready job 25")).toBeInTheDocument();
    await waitFor(() =>
      expect(woSvc.listWos).toHaveBeenLastCalledWith(
        expect.objectContaining({
          offset: 20,
          search: "pump",
          status_codes: expect.arrayContaining(["ready"]),
        }),
      ),
    );
    expect(screen.queryByRole("button", { name: /Load more/ })).not.toBeInTheDocument();
  });
});

describe("Needs shutdown", () => {
  it("keeps the confirmation open when the save is refused", async () => {
    const save = vi.fn().mockRejectedValue({ code: "VALIDATION_FAILED", message: "no" });
    render(<WoShutdownRequirementField woCode="OT-7" checked={false} onConfirmChange={save} />);
    const box = screen.getByRole("checkbox", { name: enOt.shutdownRequirement.label });
    expect(box).toHaveAccessibleDescription(enOt.shutdownRequirement.help);
    fireEvent.click(box);
    // (this test's t() shows the text as written, without filling {{code}})
    expect(screen.getByText(enOt.shutdownRequirement.confirmOnBody)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: enOt.shutdownRequirement.confirmOn }));
    await waitFor(() => expect(save).toHaveBeenCalledWith(true));
    expect(screen.getByText(enOt.shutdownRequirement.confirmOnTitle)).toBeInTheDocument();
    expect(box).not.toBeChecked();
  });

  it("shows the badge on the board card of a work order that needs a shutdown", async () => {
    const wo = (id: number, requires_shutdown: boolean) =>
      ({
        id,
        code: `OT-${id}`,
        title: `Job ${id}`,
        status_code: "planning",
        requires_shutdown,
      }) as unknown as WorkOrder;
    woSvc.listWos.mockReset();
    woSvc.listWos.mockImplementation(({ status_codes }: WoListFilter) =>
      Promise.resolve(
        status_codes?.includes("planning")
          ? { items: [wo(1, true), wo(2, false)], total: 2 }
          : { items: [], total: 0 },
      ),
    );
    render(
      <MemoryRouter>
        <WoKanbanView filter={BOARD_FILTER} onCardClick={vi.fn()} />
      </MemoryRouter>,
    );
    const flagged = (await screen.findByText("Job 1")).closest("[data-kanban-card], li, article");
    expect(screen.getAllByText(enOt.shutdownRequirement.badge)).toHaveLength(1);
    expect(flagged ?? document.body).toHaveTextContent(enOt.shutdownRequirement.badge);
  });

  it("badge names the condition", () => {
    render(<WoShutdownBadge />);
    expect(screen.getByText(enOt.shutdownRequirement.badge)).toBeInTheDocument();
  });
});
