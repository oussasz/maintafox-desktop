import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PermitTypesHost } from "@/components/lookups/adapters/PermitTypesHost";
import enCommon from "@/i18n/en/common.json";
import enPermits from "@/i18n/locale-data/en/permits.json";
import enReference from "@/i18n/locale-data/en/reference.json";
import type { PermitTypeRecord } from "@/services/permit-service";
import { P } from "@shared/rbac/permissions.generated";

// Resolve keys against the real English catalogues so a missing key fails the test.
function lookup(catalog: unknown, key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      catalog,
    );
  return typeof value === "string" ? value : undefined;
}

const CATALOGS: Record<string, unknown> = {
  permits: enPermits,
  reference: enReference,
  common: enCommon,
};

vi.mock("react-i18next", () => ({
  useTranslation: (ns?: string) => ({
    t: (key: string, opts?: Record<string, unknown>) => {
      const value = lookup(CATALOGS[opts?.["ns"] === "common" ? "common" : (ns ?? "common")], key);
      if (value != null) return value;
      if (typeof opts?.["defaultValue"] === "string") return opts["defaultValue"];
      if (ns === "permits" || ns === "reference") {
        throw new Error(`missing ${ns} i18n key: ${key}`);
      }
      return key;
    },
    i18n: { language: "en", resolvedLanguage: "en" },
  }),
}));

const svc = vi.hoisted(() => ({
  listPermitTypes: vi.fn(),
  upsertPermitType: vi.fn(),
}));
vi.mock("@/services/permit-service", () => svc);

const perms = vi.hoisted(() => ({ granted: new Set<string>() }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({
    can: (p: string) => perms.granted.has(p),
    canAny: (...p: string[]) => p.some((x) => perms.granted.has(x)),
    isLoading: false,
  }),
}));

const LOTO_TYPE: PermitTypeRecord = {
  id: 1,
  entity_sync_id: "t1",
  code: "loto",
  name: "LOTO",
  description: "Lockout/Tagout",
  requires_hse_approval: true,
  requires_operations_approval: true,
  requires_atmospheric_test: false,
  max_duration_hours: 24,
  mandatory_ppe_ids_json: "[]",
  mandatory_control_rules_json: "{}",
  requires_energy_isolation: true,
  row_version: 1,
};

const HOT_WORK: PermitTypeRecord = {
  ...LOTO_TYPE,
  id: 2,
  entity_sync_id: "t2",
  code: "hot_work",
  name: "Hot work",
};

describe("PermitTypesHost (Données de référence)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    perms.granted = new Set<string>([P.PTW_VIEW, P.PTW_ISSUE]);
    svc.listPermitTypes.mockResolvedValue([LOTO_TYPE, HOT_WORK]);
    svc.upsertPermitType.mockResolvedValue(HOT_WORK);
  });

  it("asks a reason to stop requiring an isolation and locks it on the LOTO type", async () => {
    render(<PermitTypesHost />);
    const editButtons = await screen.findAllByRole("button", { name: enPermits.actions.edit });

    // LOTO: the rule is shown ticked and cannot be changed.
    fireEvent.click(editButtons[0] as HTMLElement);
    expect(screen.getByText(enPermits.types.energyIsolationLocked)).toBeInTheDocument();

    // Another type: unticking asks for a reason before anything is saved.
    fireEvent.click(editButtons[1] as HTMLElement);
    fireEvent.click(screen.getByRole("checkbox", { name: enPermits.types.energyIsolation }));
    expect(screen.getByText(enPermits.types.disableReason)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: enPermits.types.save }));
    expect(await screen.findByText(enPermits.errors.required)).toBeInTheDocument();
    expect(svc.upsertPermitType).not.toHaveBeenCalled();

    fireEvent.change(screen.getByLabelText(new RegExp(enPermits.types.disableReason)), {
      target: { value: "Isolated lines only" },
    });
    fireEvent.click(screen.getByRole("button", { name: enPermits.types.save }));
    await waitFor(() =>
      expect(svc.upsertPermitType).toHaveBeenCalledWith(
        expect.objectContaining({
          id: 2,
          requires_energy_isolation: false,
          disable_isolation_reason: "Isolated lines only",
        }),
      ),
    );
  });

  it("is read-only without the permit issuer right", async () => {
    perms.granted = new Set<string>([P.PTW_VIEW]);
    render(<PermitTypesHost />);
    expect(await screen.findByText("Hot work")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: enPermits.actions.edit })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: enPermits.types.save })).not.toBeInTheDocument();
    expect(screen.getByText(enPermits.types.readOnly)).toBeInTheDocument();
  });

  it("does not read the types without the permit view right", async () => {
    perms.granted = new Set();
    render(<PermitTypesHost />);
    expect(await screen.findByText(enPermits.types.noViewRight)).toBeInTheDocument();
    expect(svc.listPermitTypes).not.toHaveBeenCalled();
  });

  it("explains a failed load and retries", async () => {
    svc.listPermitTypes.mockRejectedValueOnce(new Error("db locked"));
    render(<PermitTypesHost />);
    const alert = await screen.findByRole("alert");
    fireEvent.click(within(alert).getByRole("button"));
    expect(await screen.findByText("Hot work")).toBeInTheDocument();
  });
});
