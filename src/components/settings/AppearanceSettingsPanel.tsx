import { Monitor, Moon, Sun } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";

import { type ThemePreference, useTheme } from "@/components/ui/ThemeProvider";
import { cn } from "@/lib/utils";

const OPTIONS: { value: ThemePreference; icon: LucideIcon }[] = [
  { value: "light", icon: Sun },
  { value: "dark", icon: Moon },
  { value: "system", icon: Monitor },
];

/**
 * Settings → Appearance: Clair / Sombre / Système. The choice is stored on this PC only
 * (localStorage of the app, per Windows session, shared by the Maintafox accounts of this PC); "Système" follows the Windows setting.
 * It is not a plant setting, so it needs no permission and is not synchronised.
 */
export function AppearanceSettingsPanel() {
  const { t } = useTranslation("settings");
  const { theme, preference, setPreference } = useTheme();

  return (
    <section className="space-y-3" aria-labelledby="appearance-panel-title">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 id="appearance-panel-title" className="text-sm font-semibold text-text-primary">
          {t("appearance.title")}
        </h3>
        <span className="rounded-full border border-surface-border bg-surface-2 px-2 py-0.5 text-[11px] font-medium text-text-secondary">
          {t("appearance.storedLocally")}
        </span>
      </div>
      <p className="text-sm text-text-muted">{t("appearance.intro")}</p>
      <fieldset className="grid gap-2 sm:grid-cols-3">
        <legend className="sr-only">{t("appearance.legend")}</legend>
        {OPTIONS.map(({ value, icon: Icon }) => {
          const selected = preference === value;
          return (
            <label
              key={value}
              className={cn(
                "flex min-h-11 cursor-pointer items-start gap-2 rounded-md border px-3 py-2 text-sm transition-colors focus-within:ring-2 focus-within:ring-ring",
                selected
                  ? "border-primary bg-surface-2 text-text-primary"
                  : "border-surface-border bg-surface-1 text-text-secondary hover:bg-surface-2",
              )}
            >
              <input
                type="radio"
                name="color-mode"
                value={value}
                checked={selected}
                onChange={() => setPreference(value)}
                className="mt-1 h-4 w-4 shrink-0 accent-primary"
              />
              <span className="flex flex-col gap-0.5">
                <span className="flex items-center gap-1.5 font-medium">
                  <Icon className="h-4 w-4" aria-hidden />
                  {t(`appearance.${value}`)}
                </span>
                <span className="text-xs text-text-muted">{t(`appearance.${value}Hint`)}</span>
              </span>
            </label>
          );
        })}
      </fieldset>
      <p className="text-xs text-text-muted" aria-live="polite">
        {t("appearance.current", {
          mode: t(theme === "dark" ? "appearance.currentDark" : "appearance.currentLight"),
        })}
        {" · "}
        {t("appearance.authNote")}
      </p>
    </section>
  );
}
