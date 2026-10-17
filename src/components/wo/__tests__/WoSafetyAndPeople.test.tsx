import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";

import enOt from "@/i18n/locale-data/en/ot.json";
import type { WorkOrder } from "@shared/ipc-types";

import { WoKanbanView } from "../WoKanbanView";
import { WoPermitRequirementField } from "../WoPermitRequirementField";
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
  it("does not offer drag-and-drop and says where status changes happen", () => {
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
    render(
      <MemoryRouter>
        <WoKanbanView items={[wo]} onCardClick={onOpen} />
      </MemoryRouter>,
    );
    expect(screen.getByTestId("wo-kanban-drag-hint")).toHaveTextContent(
      enOt.kanban.statusFromDetailHint,
    );
    const card = screen.getByText("Replace bearing").closest("[draggable]");
    expect(card?.getAttribute("draggable")).not.toBe("true");
  });
});
