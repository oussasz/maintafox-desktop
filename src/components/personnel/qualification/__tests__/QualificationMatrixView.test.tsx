import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import enTraining from "@/i18n/locale-data/en/training.json";
import type * as GateService from "@/services/qualification-gate-service";
import type { QualificationMatrix } from "@/services/qualification-gate-service";

function lookup(key: string, count?: unknown): string | undefined {
  const path = key.split(".");
  const node = path.reduce<unknown>(
    (n, part) => (n as Record<string, unknown> | undefined)?.[part],
    enTraining,
  );
  if (typeof node === "string") return node;
  if (typeof count === "number") {
    const parent = path
      .slice(0, -1)
      .reduce<unknown>((n, part) => (n as Record<string, unknown>)?.[part], enTraining);
    const leaf = path[path.length - 1];
    const plural = (parent as Record<string, unknown> | undefined)?.[
      `${leaf}_${count === 1 ? "one" : "other"}`
    ];
    if (typeof plural === "string") return plural;
  }
  return undefined;
}

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) => {
      const text =
        lookup(key, opts?.["count"]) ??
        (typeof opts?.["defaultValue"] === "string" ? opts["defaultValue"] : key);
      return text.replace(/\{\{(\w+)\}\}/g, (_, k: string) => String(opts?.[k] ?? ""));
    },
    i18n: { language: "en" },
  }),
}));

const matrix: QualificationMatrix = {
  generated_on: "2026-10-14",
  columns: [
    {
      key: "cert:1",
      kind: "certification",
      id: 1,
      code: "HAB-B2V",
      label: "Electrical B2V",
      category: "electrical",
    },
    {
      key: "cert:2",
      kind: "certification",
      id: 2,
      code: "CATEC",
      label: "Confined space",
      category: "confined_space",
    },
    {
      key: "skill:9",
      kind: "skill",
      id: 9,
      code: "HYDRAULICS",
      label: "Hydraulics",
      category: "skill",
    },
  ],
  rows: [
    {
      personnel_id: 1,
      full_name: "Karim Électricien",
      employee_code: "MNT-001",
      employment_type: "employee",
      team_id: 1,
      team_name: "Electrical",
      position_name: null,
      company_name: null,
      cells: [
        { key: "cert:1", status: "valid", level: null, expires_at: "2028-03-01" },
        { key: "skill:9", status: "valid", level: 4, expires_at: null },
      ],
    },
    {
      personnel_id: 2,
      full_name: "Samir Sous-traitant",
      employee_code: "EXT-1",
      employment_type: "contractor",
      team_id: null,
      team_name: null,
      position_name: null,
      company_name: "Elec Services SARL",
      cells: [{ key: "cert:1", status: "expired", level: null, expires_at: "2026-09-30" }],
    },
  ],
};

const getMatrix = vi.fn().mockResolvedValue(matrix);
vi.mock("@/services/qualification-gate-service", async (orig) => ({
  ...(await orig<typeof GateService>()),
  getQualificationMatrix: (...args: unknown[]) => getMatrix(...args),
}));
vi.mock("../PersonQualificationSheet", () => ({
  PersonQualificationDialog: (props: {
    personName: string;
    focusCertificationTypeId?: number | null;
  }) => (
    <div role="dialog">
      sheet:{props.personName}:{String(props.focusCertificationTypeId ?? "none")}
    </div>
  ),
}));

import { QualificationMatrixView } from "../QualificationMatrixView";

describe("QualificationMatrixView", () => {
  it("shows people × qualifications with readable cell states and counts", async () => {
    render(<QualificationMatrixView teams={[]} companies={[]} />);
    expect(
      await screen.findByRole("button", {
        name: "Samir Sous-traitant — Electrical B2V: Expired, level , expires 2026-09-30",
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: "Karim Électricien — Hydraulics: Valid, level 4, expires No expiry",
      }),
    ).toBeInTheDocument();
    expect(screen.getByText("1 expired")).toBeInTheDocument();
    expect(screen.getByText("2 people")).toBeInTheDocument();
    // Columns nobody holds are hidden by default.
    expect(screen.queryByRole("columnheader", { name: "Confined space" })).not.toBeInTheDocument();
  });

  it("opens the renewal form on an expired cell, not on a valid one", async () => {
    render(<QualificationMatrixView teams={[]} companies={[]} />);
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Samir Sous-traitant — Electrical B2V: Expired, level , expires 2026-09-30",
      }),
    );
    expect(screen.getByRole("dialog")).toHaveTextContent("sheet:Samir Sous-traitant:1");
  });

  it("filters to people with something to do", async () => {
    render(<QualificationMatrixView teams={[]} companies={[]} />);
    await screen.findByText("Karim Électricien");
    fireEvent.click(screen.getByRole("switch", { name: enTraining.matrix.problemsOnly }));
    await waitFor(() => expect(screen.queryByText("Karim Électricien")).not.toBeInTheDocument());
    expect(screen.getByText("Samir Sous-traitant")).toBeInTheDocument();
  });
});
