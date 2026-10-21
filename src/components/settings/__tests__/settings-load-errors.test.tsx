/**
 * Failed loads are shown, never replaced by an empty list: the currency picker stays disabled
 * with an error and a retry until Données de référence › Devises is loaded.
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { templatePermissions } from "@/components/admin/RoleEditorPanel";
import { parseSnapshotValues } from "@/components/settings/PolicyEditorPanel";
import { SettingsCatalogSections } from "@/components/settings/SettingsCatalogSections";
import type { SettingDefinitionView } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const currencyDef: SettingDefinitionView = {
  key: "locale.currency_code",
  scope: "tenant",
  category: "localization",
  editor: "section",
  risk: "high",
  required_permission: P.ADM_SETTINGS,
  kind: "currency",
  options: [],
  min: null,
  max: null,
  shipped_value_json: '"EUR"',
  current_value_json: '"DZD"',
  differs_from_shipped: true,
  locked_reason: null,
  last_modified_at: null,
  last_modified_by: null,
};

function renderCurrency(currencies: Parameters<typeof SettingsCatalogSections>[0]["currencies"]) {
  return render(
    <SettingsCatalogSections
      categories={["localization"]}
      definitions={[currencyDef]}
      currencies={currencies}
      rolesAllowed={[]}
      canEdit
      onSaved={() => undefined}
      onToast={() => undefined}
    />,
  );
}

describe("currency picker load states", () => {
  it("shows the error with a retry and keeps the picker disabled", () => {
    const onRetry = vi.fn();
    renderCurrency({ options: null, error: new Error("db locked"), onRetry });

    expect(screen.getByRole("alert")).toHaveTextContent("catalog.currencyLoadError");
    expect(screen.getByRole("combobox", { name: "keys.locale.currency_code" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "catalog.restoreShipped" })).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "ds.state.retry" }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it("stays disabled while the list loads, without an error", () => {
    renderCurrency({ options: null, error: null, onRetry: vi.fn() });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "keys.locale.currency_code" })).toBeDisabled();
    expect(screen.getByText("catalog.currencyLoading")).toBeInTheDocument();
  });

  it("enables the picker once the list is loaded", () => {
    renderCurrency({
      options: [
        {
          code: "DZD",
          label: "Dinar algérien",
          label_en: "Algerian dinar",
          symbol: "DA",
          minor_units: 2,
          is_active: true,
        },
      ],
      error: null,
      onRetry: vi.fn(),
    });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "keys.locale.currency_code" })).toBeEnabled();
  });
});

describe("unreadable stored data is reported, not blanked", () => {
  it("policy snapshot", () => {
    expect(parseSnapshotValues(undefined)).toEqual({});
    expect(parseSnapshotValues('{"idle_timeout_minutes":15}')).toEqual({
      idle_timeout_minutes: 15,
    });
    expect(parseSnapshotValues("{not json")).toBeNull();
    expect(parseSnapshotValues("[1,2]")).toBeNull();
  });

  it("role template permissions", () => {
    const stored = JSON.stringify([P.OT_VIEW, P.DI_VIEW]);
    expect(templatePermissions({ module_set_json: stored })).toEqual([P.OT_VIEW, P.DI_VIEW]);
    expect(templatePermissions({ module_set_json: "{broken" })).toBeNull();
    expect(templatePermissions({ module_set_json: JSON.stringify([P.OT_VIEW, 3]) })).toBeNull();
  });
});
