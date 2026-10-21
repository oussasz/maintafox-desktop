import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enCommon from "@/i18n/en/common.json";
import enDi from "@/i18n/locale-data/en/di.json";
import { useUrgencyScaleStore } from "@/stores/urgency-scale-store";
import type { InterventionRequest, UrgencyLevel } from "@shared/ipc-types";

import { DiDeferDialog, localIsoDate } from "../DiDeferDialog";
import { DiScreenDialog } from "../DiScreenDialog";

function lookupIn(root: unknown, key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>((node, part) => (node as Record<string, unknown> | undefined)?.[part], root);
  return typeof value === "string" ? value : undefined;
}

/** `di` keys, then the shared `common` keys (urgency select). */
function lookup(key: string): string | undefined {
  return lookupIn(enDi, key) ?? lookupIn(enCommon, key);
}

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) => {
      const value = lookup(key);
      if (value == null) throw new Error(`missing di i18n key: ${key}`);
      return value.replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts?.[n] ?? ""));
    },
    i18n: { language: "en" },
  }),
}));

const store = vi.hoisted(() => ({ screen: vi.fn(), defer: vi.fn() }));
vi.mock("@/stores/di-review-store", () => ({
  useDiReviewStore: (sel: (s: typeof store) => unknown) => sel(store),
}));

const ref = vi.hoisted(() => ({ listPublishedReferenceValuesByDomainCode: vi.fn() }));
vi.mock("@/services/reference-service", () => ref);

/** The plant's reason lists are reference data: the picker hands back a code of the list. */
vi.mock("@/components/reference/ReferenceCombobox", () => ({
  ReferenceCombobox: ({
    id,
    referenceType,
    onChange,
  }: {
    id?: string;
    referenceType: string;
    onChange: (code: string) => void;
  }) => (
    <button type="button" id={id} onClick={() => onChange("awaiting_parts")}>
      {`pick ${referenceType}`}
    </button>
  ),
}));

/** The shared DI / OT urgency scale as the backend serves it (labels set by the plant). */
const LEVELS: UrgencyLevel[] = [
  ["emergency", 1, "Urgence", "Emergency"],
  ["urgent", 2, "Urgent", "Urgent"],
  ["high", 3, "Haute", "High"],
  ["normal", 4, "Normale", "Normal"],
  ["low", 5, "Basse", "Low"],
].map(([code, rank, fr, en]) => ({
  id: 100 + (rank as number),
  code: code as UrgencyLevel["code"],
  rank: rank as number,
  label_fr: fr as string,
  label_en: en as string,
  description: null,
  color_hex: null,
  response_target_hours: 4,
  completion_target_hours: 24,
  escalation_threshold_hours: 8,
  targets_source: "company",
}));
vi.mock("@/services/urgency-scale-service", () => ({
  listUrgencyLevels: vi.fn(() => Promise.resolve(LEVELS)),
}));

vi.mock("@/hooks/use-session", () => ({
  useSession: () => ({ info: { user_id: 5 } }),
}));

const DI = {
  id: 31,
  code: "DI-0031",
  row_version: 3,
  status: "in_review",
  reported_urgency: "normal",
  validated_urgency: null,
  classification_code_id: null,
  symptom_code_id: 77,
  reviewer_note: null,
} as unknown as InterventionRequest;

describe("DiScreenDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useUrgencyScaleStore.setState({ levels: LEVELS, loaded: true, loading: false, error: null });
    ref.listPublishedReferenceValuesByDomainCode.mockResolvedValue([
      { id: 77, code: "VIB", label: "Vibration", is_active: true },
      { id: 78, code: "LEAK", label: "Leak", is_active: true },
    ]);
    store.screen.mockResolvedValue({ ...DI, status: "awaiting_approval" });
  });

  it("sends what the reviewer chose, not the reported values", async () => {
    const onScreened = vi.fn();
    render(<DiScreenDialog di={DI} open onOpenChange={vi.fn()} onScreened={onScreened} />);
    // Classification uses the failure-symptom list shared with the OT close-out (ISO 14224).
    expect(ref.listPublishedReferenceValuesByDomainCode).toHaveBeenCalledWith("WORK.SYMPTOMS");
    expect(screen.getByText(enDi.screening.urgencyAsReported)).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText(enDi.screening.note), {
      target: { value: "Bearing noise confirmed on site" },
    });
    fireEvent.click(screen.getByRole("button", { name: enDi.screening.submit }));

    await waitFor(() =>
      expect(store.screen).toHaveBeenCalledWith({
        di_id: 31,
        actor_id: 0,
        expected_row_version: 3,
        validated_urgency: "normal",
        classification_code_id: 77,
        reviewer_note: "Bearing noise confirmed on site",
      }),
    );
    expect(onScreened).toHaveBeenCalledWith(
      expect.objectContaining({ status: "awaiting_approval" }),
    );
  });
});

describe("DiDeferDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    store.defer.mockResolvedValue(undefined);
  });

  it("computes local calendar dates", () => {
    expect(localIsoDate(1, new Date(2026, 9, 14, 23, 30))).toBe("2026-10-15");
    expect(localIsoDate(0, new Date(2026, 11, 31, 8))).toBe("2026-12-31");
  });

  it("requires a date and a reason instead of a hard-coded 30 days", async () => {
    render(<DiDeferDialog di={DI} open onOpenChange={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: enDi.deferral.submit }));
    expect(await screen.findByText(enDi.deferral.dateRequired)).toBeInTheDocument();
    expect(screen.getByText(enDi.deferral.reasonRequired)).toBeInTheDocument();
    expect(store.defer).not.toHaveBeenCalled();
  });

  it("sends the reason picked from the plant's deferral reasons", async () => {
    const onDeferred = vi.fn();
    render(<DiDeferDialog di={DI} open onOpenChange={vi.fn()} onDeferred={onDeferred} />);
    fireEvent.change(screen.getByLabelText(enDi.deferral.until), {
      target: { value: localIsoDate(3) },
    });
    fireEvent.click(screen.getByText("pick di.deferral_reason"));
    fireEvent.click(screen.getByRole("button", { name: enDi.deferral.submit }));
    await waitFor(() =>
      expect(store.defer).toHaveBeenCalledWith(
        expect.objectContaining({
          reason_code: "awaiting_parts",
          deferred_until: `${localIsoDate(3)}T00:00:00Z`,
        }),
      ),
    );
    expect(onDeferred).toHaveBeenCalled();
  });

  it("refuses a past date", async () => {
    render(<DiDeferDialog di={DI} open onOpenChange={vi.fn()} />);
    fireEvent.change(screen.getByLabelText(enDi.deferral.until), {
      target: { value: localIsoDate(0) },
    });
    fireEvent.click(screen.getByRole("button", { name: enDi.deferral.submit }));
    expect(await screen.findByText(enDi.deferral.dateFuture)).toBeInTheDocument();
    expect(store.defer).not.toHaveBeenCalled();
  });
});
