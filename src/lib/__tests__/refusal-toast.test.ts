import { beforeAll, beforeEach, describe, expect, it } from "vitest";

import { i18n, initI18n } from "@/i18n";
import { describeAppError } from "@/lib/app-error";
import { withdrawRefusalToast } from "@/lib/refusal-toast";
import { pushAppToast, useAppToastStore } from "@/store/app-toast-store";
import { P } from "@shared/rbac/permissions.generated";

const refused = {
  code: "PERMISSION_DENIED",
  message: `Permission denied: Permission requise : ${P.OT_CLOSE}`,
};

describe("a refused right is said once", () => {
  beforeAll(async () => {
    initI18n();
    await i18n.changeLanguage("fr");
    await i18n.loadNamespaces("admin");
  });

  beforeEach(async () => {
    useAppToastStore.setState({ items: [] });
    await i18n.changeLanguage("fr");
    await i18n.loadNamespaces("admin");
  });

  it("shows two identical refusals as one toast", () => {
    const { title, description } = describeAppError(refused);
    pushAppToast({ title, description: description ?? "", variant: "destructive" });
    pushAppToast({ title, description: description ?? "", variant: "destructive" });
    expect(useAppToastStore.getState().items).toHaveLength(1);
  });

  it("withdraws the toast when the screen shows the same refusal", () => {
    const { title, description } = describeAppError(refused);
    pushAppToast({ title, description: description ?? "", variant: "destructive" });
    pushAppToast({ title: "Autre", variant: "destructive" });
    withdrawRefusalToast(refused);
    expect(useAppToastStore.getState().items.map((i) => i.title)).toEqual(["Autre"]);
  });

  it("leaves the toast of a real technical error alone", () => {
    pushAppToast({ title: "Erreur interne", variant: "destructive" });
    withdrawRefusalToast({ code: "INTERNAL_ERROR", message: "boom" });
    expect(useAppToastStore.getState().items).toHaveLength(1);
  });
});
