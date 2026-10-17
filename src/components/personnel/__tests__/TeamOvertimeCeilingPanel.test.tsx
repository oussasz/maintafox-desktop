import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enPersonnel from "@/i18n/locale-data/en/personnel.json";
import type { CapacityRule } from "@shared/ipc-types";

import { parseHours, TeamOvertimeCeilingPanel } from "../TeamOvertimeCeilingPanel";

function lookup(key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      enPersonnel,
    );
  return typeof value === "string" ? value : undefined;
}

function translate(key: string, opts?: Record<string, unknown>): string {
  const text = lookup(key);
  if (text == null) return key;
  return text.replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts?.[n] ?? ""));
}

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: translate, i18n: { language: "en" } }),
}));

vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: ReactNode }) => <>{children}</>,
}));

const toast = vi.fn();
vi.mock("@/hooks/use-toast", () => ({ useToast: () => ({ toast }) }));

const service = vi.hoisted(() => ({
  listCapacityRules: vi.fn(),
  createCapacityRule: vi.fn(),
  updateCapacityRule: vi.fn(),
}));
vi.mock("@/services/planning-service", () => service);

function rule(overrides: Partial<CapacityRule> = {}): CapacityRule {
  return {
    id: 3,
    entity_id: null,
    team_id: 7,
    effective_start: "2026-09-01",
    effective_end: "2026-12-31",
    available_hours_per_day: 0,
    max_overtime_hours_per_day: 2,
    row_version: 4,
    created_at: "2026-09-01T00:00:00Z",
    updated_at: "2026-09-01T00:00:00Z",
    ...overrides,
  };
}

describe("parseHours", () => {
  it("reads a number of hours, comma or dot, never a negative one", () => {
    expect(parseHours("1,5")).toBe(1.5);
    expect(parseHours("0")).toBe(0);
    expect(parseHours("")).toBeNull();
    expect(parseHours("-1")).toBeNull();
    expect(parseHours("abc")).toBeNull();
  });
});

describe("TeamOvertimeCeilingPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    service.listCapacityRules.mockResolvedValue([rule()]);
  });

  it("lists the crew's overtime ceilings", async () => {
    render(<TeamOvertimeCeilingPanel teamId={7} />);
    expect(await screen.findByText("2 h")).toBeInTheDocument();
    expect(service.listCapacityRules).toHaveBeenCalledWith({ team_id: 7 });
  });

  it("adds a ceiling and tells the planner", async () => {
    service.createCapacityRule.mockResolvedValue(rule({ id: 4 }));
    const onChanged = vi.fn();
    render(<TeamOvertimeCeilingPanel teamId={7} onChanged={onChanged} />);
    await screen.findByText("2 h");
    const add = screen.getByRole("button", { name: enPersonnel.overtimeCeiling.add });
    expect(add).toBeDisabled();
    fireEvent.change(screen.getByLabelText(enPersonnel.overtimeCeiling.hoursPerDay), {
      target: { value: "1,5" },
    });
    fireEvent.click(add);
    await waitFor(() =>
      expect(service.createCapacityRule).toHaveBeenCalledWith(
        expect.objectContaining({
          team_id: 7,
          effective_end: null,
          max_overtime_hours_per_day: 1.5,
        }),
      ),
    );
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(toast).toHaveBeenCalledWith(
      expect.objectContaining({ title: enPersonnel.overtimeCeiling.created, variant: "success" }),
    );
  });

  it("removes an end date by clearing it", async () => {
    service.updateCapacityRule.mockResolvedValue(rule({ effective_end: null }));
    render(<TeamOvertimeCeilingPanel teamId={7} />);
    fireEvent.click(await screen.findByRole("button", { name: enPersonnel.overtimeCeiling.edit }));
    const end = screen.getAllByLabelText(enPersonnel.overtimeCeiling.to)[0];
    if (!end) throw new Error("end date field not rendered");
    fireEvent.change(end, { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: enPersonnel.overtimeCeiling.save }));
    await waitFor(() =>
      expect(service.updateCapacityRule).toHaveBeenCalledWith(
        3,
        4,
        expect.objectContaining({ effective_end: null, clear_effective_end: true }),
      ),
    );
  });
});
