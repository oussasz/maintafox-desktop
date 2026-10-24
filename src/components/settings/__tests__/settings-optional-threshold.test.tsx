/**
 * An optional threshold (`optional_decimal`, the count four-eyes tolerance) is never turned into
 * zero by the settings screen: a cleared field saves `null` (no value), an unset value reads
 * "Non défini(e)", and a typed 0 is saved as 0 (a value).
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { SettingsCatalogSections } from "@/components/settings/SettingsCatalogSections";
import frSettings from "@/i18n/locale-data/fr/settings.json";
import type { SettingDefinitionView } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const setSetting = vi.fn();
vi.mock("@/services/settings-service", () => ({
  setSetting: (...args: unknown[]) => setSetting(...args) as unknown,
  listSettingChangeEvents: vi.fn().mockResolvedValue([]),
}));

vi.mock("@/hooks/use-step-up", () => ({
  useStepUp: () => ({
    withStepUp: <T,>(action: () => Promise<T>) => action(),
    StepUpDialogElement: null,
  }),
}));

function thresholdDef(current: string | null): SettingDefinitionView {
  return {
    key: "inventory.count_four_eyes_percent_threshold",
    scope: "tenant",
    category: "inventory",
    editor: "section",
    risk: "low",
    required_permission: P.ADM_SETTINGS,
    kind: "optional_decimal",
    options: [],
    min: 0,
    max: null,
    shipped_value_json: "null",
    current_value_json: current,
    differs_from_shipped: current != null,
    locked_reason: null,
    last_modified_at: null,
    last_modified_by: null,
  };
}

function renderThreshold(current: string | null) {
  render(
    <SettingsCatalogSections
      categories={["inventory"]}
      definitions={[thresholdDef(current)]}
      currencies={{ options: [], error: null, onRetry: () => undefined }}
      rolesAllowed={[]}
      canEdit
      onSaved={() => undefined}
      onToast={() => undefined}
    />,
  );
  return screen.getByRole("spinbutton", {
    name: "keys.inventory.count_four_eyes_percent_threshold",
  });
}

describe("optional threshold setting", () => {
  beforeEach(() => {
    setSetting.mockReset();
    setSetting.mockResolvedValue(undefined);
  });

  it("shows an unset value as not set, never as 0", () => {
    const input = renderThreshold(null);
    expect(input).toHaveValue(null);
    expect(input).toHaveAttribute("placeholder", "catalog.notSet");
    expect(screen.getAllByText("catalog.notSet").length).toBeGreaterThan(0);
    // In French it reads "Non définie" (the tolerance).
    expect(frSettings.catalog.notSet).toBe("Non définie");
  });

  it("saves a cleared field as null (no value)", async () => {
    const input = renderThreshold("25");
    fireEvent.change(input, { target: { value: "" } });
    fireEvent.blur(input);
    await waitFor(() => expect(setSetting).toHaveBeenCalledTimes(1));
    expect(setSetting.mock.calls[0]?.[0]).toMatchObject({
      key: "inventory.count_four_eyes_percent_threshold",
      value_json: "null",
    });
  });

  it("saves a typed 0 as the value 0", async () => {
    const input = renderThreshold(null);
    fireEvent.change(input, { target: { value: "0" } });
    fireEvent.blur(input);
    await waitFor(() => expect(setSetting).toHaveBeenCalledTimes(1));
    expect(setSetting.mock.calls[0]?.[0]).toMatchObject({ value_json: "0" });
  });
});
