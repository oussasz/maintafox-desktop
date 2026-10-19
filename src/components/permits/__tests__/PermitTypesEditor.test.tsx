import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enPermits from "@/i18n/locale-data/en/permits.json";
import type { PermitTypeRecord } from "@/services/permit-service";
import { P } from "@shared/rbac/permissions.generated";

import { PermitTypesEditor } from "../PermitTypesEditor";

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
      const value = lookup(enPermits, key);
      // Other namespaces (the step-up dialog) are not under test: their key stands in.
      if (value == null && ns !== "permits") return key;
      if (value == null) throw new Error(`missing permits key: ${key}`);
      return value.replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts?.[n] ?? ""));
    },
    i18n: { language: "en" },
  }),
}));

const svc = vi.hoisted(() => ({ upsertPermitType: vi.fn() }));
vi.mock("@/services/permit-service", () => svc);
vi.mock("@/services/rbac-service", () => ({ verifyStepUp: vi.fn() }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({
    can: (p: string) => p === P.PTW_VIEW || p === P.PTW_ISSUE,
    canAny: () => true,
    isLoading: false,
  }),
}));

const HOT_WORK: PermitTypeRecord = {
  id: 2,
  entity_sync_id: "t2",
  code: "hot_work",
  name: "Hot work",
  description: "",
  requires_hse_approval: false,
  requires_operations_approval: false,
  requires_atmospheric_test: false,
  max_duration_hours: 24,
  mandatory_ppe_ids_json: "[]",
  mandatory_control_rules_json: "{}",
  requires_energy_isolation: false,
  requires_three_person_removal: false,
  row_version: 1,
};

describe("PermitTypesEditor — the code of an existing type", () => {
  beforeEach(() => vi.clearAllMocks());

  it("is read-only when editing an existing type, with the reason said", async () => {
    render(<PermitTypesEditor types={[HOT_WORK]} canEdit onSaved={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: enPermits.actions.edit }));
    const code = screen.getByLabelText(new RegExp(`^${enPermits.types.code}`));
    expect(code).toHaveValue("hot_work");
    expect(code).toHaveAttribute("readonly");
    expect(code).toHaveAccessibleDescription(enPermits.types.codeLocked);
    expect(screen.getByText(enPermits.types.codeLocked)).toBeInTheDocument();
  });

  it("can be typed for a new type", async () => {
    render(<PermitTypesEditor types={[HOT_WORK]} canEdit onSaved={vi.fn()} />);
    const code = await screen.findByLabelText(new RegExp(`^${enPermits.types.code}`));
    expect(code).not.toHaveAttribute("readonly");
    expect(code).not.toHaveAttribute("aria-describedby");
    fireEvent.change(code, { target: { value: "work_at_height" } });
    await waitFor(() => expect(code).toHaveValue("work_at_height"));
    expect(screen.queryByText(enPermits.types.codeLocked)).toBeNull();
  });
});
