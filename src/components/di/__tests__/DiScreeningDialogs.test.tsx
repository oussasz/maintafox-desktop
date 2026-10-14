import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enDi from "@/i18n/locale-data/en/di.json";
import type { InterventionRequest } from "@shared/ipc-types";

import { DiDeferDialog, localIsoDate } from "../DiDeferDialog";
import { DiScreenDialog } from "../DiScreenDialog";

function lookup(key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>((node, part) => (node as Record<string, unknown> | undefined)?.[part], enDi);
  return typeof value === "string" ? value : undefined;
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

vi.mock("@/hooks/use-session", () => ({
  useSession: () => ({ info: { user_id: 5 } }),
}));

const DI = {
  id: 31,
  code: "DI-0031",
  row_version: 3,
  status: "in_review",
  reported_urgency: "medium",
  validated_urgency: null,
  classification_code_id: null,
  symptom_code_id: 77,
  reviewer_note: null,
} as unknown as InterventionRequest;

describe("DiScreenDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    ref.listPublishedReferenceValuesByDomainCode.mockResolvedValue([
      { id: 77, code: "VIB", label: "Vibration", is_active: true },
      { id: 78, code: "LEAK", label: "Leak", is_active: true },
    ]);
    store.screen.mockResolvedValue({ ...DI, status: "awaiting_approval" });
  });

  it("sends what the reviewer chose, not the reported values", async () => {
    const onScreened = vi.fn();
    render(<DiScreenDialog di={DI} open onOpenChange={vi.fn()} onScreened={onScreened} />);
    expect(ref.listPublishedReferenceValuesByDomainCode).toHaveBeenCalledWith("DI.SYMPTOM");
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
        validated_urgency: "medium",
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
