/**
 * Shared DI / OT lists on screen: the urgency scale (badge + the plant's settings screen) and
 * the link from a request / work order back to the inspection round it came from.
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enCommon from "@/i18n/en/common.json";
import enReference from "@/i18n/locale-data/en/reference.json";
import { useUrgencyScaleStore } from "@/stores/urgency-scale-store";
import type { UrgencyLevel } from "@shared/ipc-types";

import { LinkedEntityBadge, roundTitle } from "../LinkedEntityBadge";
import { UrgencyBadge } from "../urgency/UrgencyBadge";

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
      const plural = opts && typeof opts["count"] === "number" ? `${key}_other` : key;
      const value =
        ns === "reference"
          ? (lookup(enReference, key) ?? lookup(enReference, plural))
          : lookup(enCommon, key);
      if (value == null) throw new Error(`missing ${ns ?? "common"} i18n key: ${key}`);
      return value.replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts?.[n] ?? ""));
    },
    i18n: { language: "en" },
  }),
}));

const navigate = vi.hoisted(() => vi.fn());
vi.mock("react-router-dom", () => ({ useNavigate: () => navigate }));
vi.mock("@/hooks/use-permissions", () => ({ usePermissions: () => ({ can: () => true }) }));

const scale = vi.hoisted(() => ({ listUrgencyLevels: vi.fn(), updateUrgencyLevel: vi.fn() }));
vi.mock("@/services/urgency-scale-service", () => scale);

function level(
  code: UrgencyLevel["code"],
  rank: number,
  fr: string,
  en: string,
  targets: [number, number, number],
  source = "starter",
): UrgencyLevel {
  return {
    id: 500 + rank,
    code,
    rank,
    label_fr: fr,
    label_en: en,
    description: null,
    color_hex: null,
    response_target_hours: targets[0],
    completion_target_hours: targets[1],
    escalation_threshold_hours: targets[2],
    targets_source: source,
  };
}

const LEVELS: UrgencyLevel[] = [
  level("emergency", 1, "Urgence", "Emergency", [1, 8, 4], "di_sla_rule"),
  level("urgent", 2, "Urgent", "Urgent", [2, 16, 8]),
  level("high", 3, "Haute", "High", [4, 24, 8]),
  level("normal", 4, "Normale", "Planned", [24, 72, 48], "company"),
  level("low", 5, "Basse", "Low", [72, 168, 120]),
];

beforeEach(() => {
  vi.clearAllMocks();
  scale.listUrgencyLevels.mockResolvedValue(LEVELS);
  useUrgencyScaleStore.setState({ levels: LEVELS, loaded: true, loading: false, error: null });
});

describe("LinkedEntityBadge — inspection round", () => {
  it("opens the round the finding came from", () => {
    render(
      <LinkedEntityBadge
        entity="inspection_round"
        code="RONDE-POMPES"
        entityId={42}
        title={roundTitle("Ronde pompes", null, "en")}
      />,
    );
    const link = screen.getByRole("link", { name: "Ronde pompes" });
    expect(link).toHaveTextContent("Round RONDE-POMPES");
    fireEvent.click(link);
    expect(navigate).toHaveBeenCalledWith("/inspections?openRound=42");
  });

  it("names the template and the scheduled day in the tooltip", () => {
    expect(roundTitle("Ronde pompes", "2026-10-01T08:00:00Z", "en")).toMatch(/^Ronde pompes · /);
    expect(roundTitle("  ", "2026-10-01T08:00:00Z", "en")).toBeNull();
  });
});

describe("UrgencyBadge", () => {
  it("shows the plant's label of the level and its target times", () => {
    render(<UrgencyBadge code="normal" />);
    const chip = screen.getByText("Planned");
    expect(chip.closest("span[title]")).toHaveAttribute(
      "title",
      "Target times: response ≤ 24 h, completion ≤ 72 h",
    );
  });

  it("renders nothing without a level", () => {
    const { container } = render(<UrgencyBadge code={null} />);
    expect(container).toBeEmptyDOMElement();
  });
});

describe("UrgencyScaleHost", () => {
  it("lists the five fixed levels and saves the plant's labels and target times", async () => {
    const { UrgencyScaleHost } = await import("@/components/lookups/adapters/UrgencyScaleHost");
    scale.updateUrgencyLevel.mockResolvedValue({ ...LEVELS[1], label_en: "Priority" });
    render(<UrgencyScaleHost />);

    const rows = await screen.findAllByRole("row");
    // header + five levels; no add or delete action exists.
    expect(rows).toHaveLength(6);
    expect(screen.queryByRole("button", { name: /add|delete|deactivate/i })).toBeNull();
    expect(screen.getByText(enReference.urgencyScale.sourceSlaRule)).toBeInTheDocument();
    expect(screen.getByText(enReference.urgencyScale.sourceCompany)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: 'Edit level "Urgent"' }));
    const urgentRow = rows[2] as HTMLElement;
    const labelEn = within(urgentRow).getByLabelText(enReference.urgencyScale.colLabelEn);
    fireEvent.change(labelEn, { target: { value: "Priority" } });
    const completion = within(urgentRow).getByLabelText(enReference.urgencyScale.colCompletion);

    // A target that is not a whole number of hours keeps the save disabled.
    fireEvent.change(completion, { target: { value: "0" } });
    const save = within(urgentRow).getByRole("button", { name: "Save" });
    expect(save).toBeDisabled();

    fireEvent.change(completion, { target: { value: "12" } });
    expect(save).toBeEnabled();
    fireEvent.click(save);

    await waitFor(() =>
      expect(scale.updateUrgencyLevel).toHaveBeenCalledWith("urgent", {
        label_fr: "Urgent",
        label_en: "Priority",
        response_target_hours: 2,
        completion_target_hours: 12,
        escalation_threshold_hours: 8,
      }),
    );
  });
});
