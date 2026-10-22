import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enTraining from "@/i18n/locale-data/en/training.json";
import type * as GateService from "@/services/qualification-gate-service";
import type { PersonQualificationCheck } from "@/services/qualification-gate-service";

function lookup(key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      enTraining,
    );
  return typeof value === "string" ? value : undefined;
}

function interpolate(text: string, opts?: Record<string, unknown>): string {
  return text.replace(/\{\{(\w+)\}\}/g, (_, k: string) => String(opts?.[k] ?? ""));
}

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) => {
      const text =
        lookup(key) ?? (typeof opts?.["defaultValue"] === "string" ? opts["defaultValue"] : key);
      return interpolate(text, opts);
    },
    i18n: { language: "en" },
  }),
}));

const canMock = vi.fn<(perm: string) => boolean>();
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: canMock }),
}));
vi.mock("@/hooks/use-step-up", () => ({
  useStepUp: () => ({ withStepUp: <T,>(fn: () => Promise<T>) => fn(), StepUpDialogElement: null }),
}));
const recordMock = vi.fn();
vi.mock("@/services/qualification-gate-service", async (orig) => ({
  ...(await orig<typeof GateService>()),
  recordQualificationOverride: (...args: unknown[]) => recordMock(...args),
}));

import { QualificationCheckNotice } from "../QualificationCheckNotice";

const blocked: PersonQualificationCheck = {
  personnel_id: 7,
  full_name: "Samir Sous-traitant",
  employee_code: "EXT-1",
  roles: ["candidate"],
  qualified: false,
  gaps: [
    {
      requirement_key: "cert:3",
      kind: "certification",
      code: "HAB-B1V",
      label: "Electrical authorisation B1V",
      status: "expired",
      expires_at: "2026-09-30",
      held_level: null,
      required_level: null,
      source_kind: "work_order_type",
      source_code: "ELEC",
    },
  ],
  warnings: [],
  override_info: null,
};

describe("QualificationCheckNotice", () => {
  beforeEach(() => {
    canMock.mockReset();
    recordMock.mockReset();
  });

  it("explains why the person is blocked", () => {
    canMock.mockReturnValue(false);
    render(
      <QualificationCheckNotice
        check={blocked}
        overrideContext={{ context_kind: "work_order", work_order_id: 4 }}
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Samir Sous-traitant cannot be assigned to this job.",
    );
    expect(
      screen.getByText("Electrical authorisation B1V expired on 30/09/2026."),
    ).toBeInTheDocument();
    // Without trn.certify, no override action — only how to proceed.
    expect(
      screen.queryByRole("button", { name: enTraining.check.override }),
    ).not.toBeInTheDocument();
    expect(screen.getByText(enTraining.check.howToProceed)).toBeInTheDocument();
  });

  it("lets an authorised supervisor record a reasoned override", async () => {
    canMock.mockReturnValue(true);
    const after: PersonQualificationCheck = {
      ...blocked,
      qualified: true,
      override_info: {
        id: 1,
        reason: "Renewal in progress, supervised by K.",
        approved_by_id: 1,
        approved_by_name: "Admin",
        approved_at: "2026-10-14T10:00:00Z",
      },
    };
    recordMock.mockResolvedValue(after);
    const onOverridden = vi.fn();
    render(
      <QualificationCheckNotice
        check={blocked}
        overrideContext={{ context_kind: "work_order", work_order_id: 4 }}
        onOverridden={onOverridden}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: enTraining.check.override }));
    const confirm = await screen.findByRole("button", { name: enTraining.check.overrideConfirm });
    expect(confirm).toBeDisabled();
    fireEvent.change(screen.getByRole("textbox"), {
      target: { value: "Renewal in progress, supervised by K." },
    });
    expect(confirm).toBeEnabled();
    fireEvent.click(confirm);
    await waitFor(() => expect(onOverridden).toHaveBeenCalledWith(after));
    expect(recordMock).toHaveBeenCalledWith(
      7,
      { context_kind: "work_order", work_order_id: 4 },
      "Renewal in progress, supervised by K.",
    );
  });

  it("shows a qualified person and expiry warnings as status, not alert", () => {
    canMock.mockReturnValue(true);
    render(
      <QualificationCheckNotice
        check={{
          ...blocked,
          qualified: true,
          gaps: [],
          warnings: [
            { requirement_key: "cert:3", code: "HAB-B1V", label: "B1V", expires_at: "2026-11-01" },
          ],
        }}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("holds every qualification");
    expect(screen.getByText("B1V runs out on 01/11/2026: plan the renewal.")).toBeInTheDocument();
  });
});
