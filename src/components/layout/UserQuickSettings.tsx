/**
 * UserQuickSettings — language and colour mode, one click from the user menu (the full choices
 * stay in Paramètres). The language is the one of this user; the colour mode is kept on this PC.
 */
import { Monitor, Moon, Sun } from "lucide-react";
import { useTranslation } from "react-i18next";

import { useOptionalTheme, type ThemePreference } from "@/components/ui/ThemeProvider";
import { cn } from "@/lib/utils";
import { useLocaleStore } from "@/stores/locale-store";

const THEMES: {
  value: ThemePreference;
  icon: typeof Sun;
  labelKey: "user.themeLight" | "user.themeDark" | "user.themeSystem";
}[] = [
  { value: "light", icon: Sun, labelKey: "user.themeLight" },
  { value: "dark", icon: Moon, labelKey: "user.themeDark" },
  { value: "system", icon: Monitor, labelKey: "user.themeSystem" },
];

const segment =
  "min-h-8 min-w-9 rounded px-2 text-xs font-semibold transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";

export function UserQuickSettings() {
  const { t } = useTranslation("shell");
  const { activeLocale, supportedLocales, followsSystemLanguage, setLocale } = useLocaleStore();
  const theme = useOptionalTheme();

  return (
    <div className="space-y-2 border-b border-surface-border px-3 py-2">
      <div className="flex items-center justify-between gap-2">
        <span className="text-xs text-text-muted">{t("user.language")}</span>
        <div role="group" aria-label={t("user.language")} className="inline-flex gap-0.5">
          {supportedLocales.map((loc) => {
            const active = !followsSystemLanguage && loc === activeLocale;
            return (
              <button
                key={loc}
                type="button"
                aria-pressed={active}
                onClick={() => void setLocale(loc)}
                className={cn(
                  segment,
                  active ? "bg-primary text-white" : "text-text-secondary hover:bg-surface-2",
                )}
              >
                {loc.toUpperCase()}
              </button>
            );
          })}
        </div>
      </div>
      {theme ? (
        <div className="flex items-center justify-between gap-2">
          <span className="text-xs text-text-muted">{t("user.theme")}</span>
          <div role="group" aria-label={t("user.theme")} className="inline-flex gap-0.5">
            {THEMES.map(({ value, icon: Icon, labelKey }) => (
              <button
                key={value}
                type="button"
                aria-pressed={theme.preference === value}
                aria-label={t(labelKey)}
                title={t(labelKey)}
                onClick={() => theme.setPreference(value)}
                className={cn(
                  segment,
                  theme.preference === value
                    ? "bg-primary text-white"
                    : "text-text-secondary hover:bg-surface-2",
                )}
              >
                <Icon className="mx-auto h-4 w-4" aria-hidden />
              </button>
            ))}
          </div>
        </div>
      ) : null}
    </div>
  );
}
