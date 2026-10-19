/**
 * A permit refusal is shown in the user's language from the first permit screen opened, through
 * the app's real translation path (i18n, appErrorReason): the refusal texts live in a namespace
 * loaded on demand, and the permit screens load it themselves.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";

import { PermitTypesHost } from "@/components/lookups/adapters/PermitTypesHost";
import { i18n, initI18n } from "@/i18n";
import type { PermitTypeRecord } from "@/services/permit-service";
import { P } from "@shared/rbac/permissions.generated";

const svc = vi.hoisted(() => ({
  listPermitTypes: vi.fn(),
  upsertPermitType: vi.fn(),
}));
vi.mock("@/services/permit-service", () => svc);

const rbac = vi.hoisted(() => ({ verifyStepUp: vi.fn() }));
vi.mock("@/services/rbac-service", () => rbac);

vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({
    can: (p: string) => p === P.PTW_VIEW || p === P.PTW_ISSUE,
    canAny: (...p: string[]) => p.some((x) => x === P.PTW_VIEW || x === P.PTW_ISSUE),
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

describe("permit refusals in French", () => {
  beforeAll(async () => {
    initI18n();
    await i18n.changeLanguage("fr");
    await i18n.loadNamespaces("permits");
  });

  it("shows the server's coded refusal in French on a cold start", async () => {
    // Nothing has loaded the refusal texts yet: the permit screen must do it.
    expect(i18n.hasResourceBundle("fr", "training")).toBe(false);
    svc.listPermitTypes.mockResolvedValue([HOT_WORK]);
    svc.upsertPermitType.mockRejectedValue({
      code: "VALIDATION_FAILED",
      message:
        "Validation failed: Only an administrator can change the three-person rule of Hot work",
      details: [
        {
          code: "PERMIT_TYPE_THREE_PERSON_FORBIDDEN",
          message: "Only an administrator can change the three-person rule of Hot work",
          params: { type: "Hot work" },
        },
      ],
    });

    render(<PermitTypesHost />);
    await waitFor(() => expect(i18n.hasResourceBundle("fr", "training")).toBe(true));

    fireEvent.click(await screen.findByRole("button", { name: "Modifier" }));
    fireEvent.change(screen.getByLabelText(String(i18n.t("permits:types.maxHours"))), {
      target: { value: "8" },
    });
    fireEvent.click(screen.getByRole("button", { name: String(i18n.t("permits:types.save")) }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Seul un administrateur, après avoir ressaisi son mot de passe, peut modifier la règle des trois personnes du type de permis Hot work.",
    );
  });
});
