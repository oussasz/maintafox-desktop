import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PermitTypesHost } from "@/components/lookups/adapters/PermitTypesHost";
import enCommon from "@/i18n/en/common.json";
import enPermits from "@/i18n/locale-data/en/permits.json";
import enReference from "@/i18n/locale-data/en/reference.json";
import enTraining from "@/i18n/locale-data/en/training.json";
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
      if (value != null) {
        return value.replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts?.[n] ?? ""));
      }
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

// The step-up dialog is the app's own; only its password check (IPC) is replaced.
const rbac = vi.hoisted(() => ({ verifyStepUp: vi.fn() }));
vi.mock("@/services/rbac-service", () => rbac);

// A coded refusal is shown with its translated text (training.json "errors").
vi.mock("@/lib/app-error", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  appErrorReason: (e: { code?: string }) =>
    ((e.code && lookup(enTraining, `errors.${e.code}`)) || "unknown").replace(/\{\{\w+\}\}/g, ""),
}));

const perms = vi.hoisted(() => ({
  granted: new Set<string>(),
  loading: false,
  asked: [] as (number | null | undefined)[],
}));
// Permit types are plant-wide: the rights are read at no node (node 0).
vi.mock("@/hooks/use-access-at", () => ({
  useAccessAt: (node: number | null | undefined) => {
    perms.asked.push(node);
    return {
      loading: perms.loading,
      can: (p: string) => !perms.loading && node === 0 && perms.granted.has(p),
    };
  },
}));

// The editor reads the administrator right (three-person setting) from the held-anywhere set.
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
  requires_three_person_removal: false,
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
    perms.loading = false;
    perms.asked = [];
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

  describe("third person for lock removal", () => {
    const switchOf = () => screen.getByRole("switch", { name: enPermits.types.threePerson });
    const reasonMin = enPermits.types.reasonMin.replace("{{min}}", "10");

    it("is shown to everyone, and only an administrator can change it", async () => {
      svc.listPermitTypes.mockResolvedValue([
        LOTO_TYPE,
        { ...HOT_WORK, requires_three_person_removal: true },
      ]);
      render(<PermitTypesHost />);
      // The table says which types ask for a third person.
      const hot = (await screen.findByText("Hot work")).closest("tr") as HTMLElement;
      expect(within(hot).getAllByText(enPermits.types.yes).length).toBe(2);

      const editButtons = screen.getAllByRole("button", { name: enPermits.actions.edit });
      fireEvent.click(editButtons[1] as HTMLElement);
      expect(switchOf()).toBeDisabled();
      expect(switchOf()).toHaveAttribute("aria-checked", "true");
      expect(
        screen.getByText(enPermits.types.threePersonAdminOnly, { exact: false }),
      ).toBeInTheDocument();
    });

    it("asks a reason of at least 10 characters, then saves through the step-up", async () => {
      perms.granted = new Set<string>([P.PTW_VIEW, P.PTW_ISSUE, P.ADM_SETTINGS]);
      // The server asks for the password once, then accepts.
      svc.upsertPermitType
        .mockRejectedValueOnce({ code: "STEP_UP_REQUIRED", message: "StepUpRequired" })
        .mockResolvedValue({ ...HOT_WORK, requires_three_person_removal: true });
      rbac.verifyStepUp.mockResolvedValue(undefined);
      const onSaved = vi.fn();
      render(<PermitTypesHost onSaved={onSaved} />);
      const editButtons = await screen.findAllByRole("button", { name: enPermits.actions.edit });
      fireEvent.click(editButtons[1] as HTMLElement);

      expect(screen.queryByText(enPermits.types.threePersonReason)).toBeNull();
      fireEvent.click(switchOf());
      expect(switchOf()).toHaveAttribute("aria-checked", "true");
      const reason = screen.getByRole("textbox", {
        name: new RegExp(enPermits.types.threePersonReason.replace("?", "?")),
      });
      fireEvent.change(reason, { target: { value: "too short" } });
      expect(screen.getByText(reasonMin)).toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: enPermits.types.save }));
      expect(svc.upsertPermitType).not.toHaveBeenCalled();

      fireEvent.change(reason, { target: { value: "  Shared isolations on line 3  " } });
      expect(screen.queryByText(reasonMin)).toBeNull();
      fireEvent.click(screen.getByRole("button", { name: enPermits.types.save }));

      // The step-up dialog opens; once the password is confirmed the save is retried.
      const password = await screen.findByPlaceholderText("stepUp.passwordLabel");
      fireEvent.change(password, { target: { value: "secret" } });
      fireEvent.click(screen.getByRole("button", { name: "stepUp.submit" }));
      await waitFor(() => expect(svc.upsertPermitType).toHaveBeenCalledTimes(2));
      expect(svc.upsertPermitType).toHaveBeenLastCalledWith(
        expect.objectContaining({
          id: 2,
          requires_three_person_removal: true,
          three_person_change_reason: "Shared isolations on line 3",
        }),
      );
      await waitFor(() => expect(onSaved).toHaveBeenCalled());
    });

    it("does not ask a reason or a step-up, and leaves the setting alone, when it is left as it is", async () => {
      perms.granted = new Set<string>([P.PTW_VIEW, P.PTW_ISSUE, P.ADM_SETTINGS]);
      render(<PermitTypesHost />);
      const editButtons = await screen.findAllByRole("button", { name: enPermits.actions.edit });
      fireEvent.click(editButtons[1] as HTMLElement);
      fireEvent.click(screen.getByRole("button", { name: enPermits.types.save }));
      await waitFor(() =>
        expect(svc.upsertPermitType).toHaveBeenCalledWith(
          expect.objectContaining({
            id: 2,
            // null = unchanged: the server keeps its current value.
            requires_three_person_removal: null,
            three_person_change_reason: null,
          }),
        ),
      );
    });

    it("saves another field of the type without touching the setting or asking a step-up", async () => {
      // A permit issuer (not an administrator) changes only the maximum duration. The server may
      // hold another value of the setting by now (changed elsewhere): the save does not send it.
      const onSaved = vi.fn();
      render(<PermitTypesHost onSaved={onSaved} />);
      const editButtons = await screen.findAllByRole("button", { name: enPermits.actions.edit });
      fireEvent.click(editButtons[1] as HTMLElement);
      fireEvent.change(screen.getByLabelText(enPermits.types.maxHours), {
        target: { value: "8" },
      });
      fireEvent.click(screen.getByRole("button", { name: enPermits.types.save }));
      await waitFor(() => expect(onSaved).toHaveBeenCalled());
      expect(svc.upsertPermitType).toHaveBeenCalledTimes(1);
      expect(svc.upsertPermitType).toHaveBeenCalledWith(
        expect.objectContaining({
          id: 2,
          max_duration_hours: 8,
          requires_three_person_removal: null,
          three_person_change_reason: null,
        }),
      );
      expect(screen.queryByPlaceholderText("stepUp.passwordLabel")).toBeNull();
      expect(rbac.verifyStepUp).not.toHaveBeenCalled();
    });

    it.each(["PERMIT_TYPE_THREE_PERSON_FORBIDDEN", "PERMIT_TYPE_THREE_PERSON_REASON"])(
      "shows the translated refusal %s",
      async (code) => {
        perms.granted = new Set<string>([P.PTW_VIEW, P.PTW_ISSUE, P.ADM_SETTINGS]);
        svc.upsertPermitType.mockRejectedValue({ code });
        render(<PermitTypesHost />);
        const editButtons = await screen.findAllByRole("button", {
          name: enPermits.actions.edit,
        });
        fireEvent.click(editButtons[1] as HTMLElement);
        fireEvent.click(switchOf());
        fireEvent.change(
          screen.getByRole("textbox", {
            name: new RegExp(enPermits.types.threePersonReason.replace("?", "?")),
          }),
          { target: { value: "Shared isolations on line 3" } },
        );
        fireEvent.click(screen.getByRole("button", { name: enPermits.types.save }));
        const text = String(lookup(enTraining, `errors.${code}`)).replace(/\{\{\w+\}\}/g, "");
        expect(await screen.findByText(text.replace(/\s+/g, " ").trim())).toBeInTheDocument();
      },
    );
  });
  it("reads the rights at no node (permit types are plant-wide)", async () => {
    render(<PermitTypesHost />);
    await screen.findByText("LOTO");
    expect(perms.asked.length).toBeGreaterThan(0);
    expect(perms.asked.every((n) => n === 0)).toBe(true);
  });

  it("shows neither the list nor the no-right message while the rights load", () => {
    perms.loading = true;
    render(<PermitTypesHost />);
    expect(screen.queryByText(enPermits.types.noViewRight)).toBeNull();
    expect(svc.listPermitTypes).not.toHaveBeenCalled();
  });
});
