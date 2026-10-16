import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { useTheme } from "@/components/ui/ThemeProvider";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { CURRENCY_CODE_OPTIONS } from "@/lib/currency";
import { applyI18nFallbackLanguage } from "@/lib/i18n-fallback";
import { cn } from "@/lib/utils";
import { setSetting } from "@/services/settings-service";
import { useLocaleStore } from "@/stores/locale-store";
import type { AppSetting } from "@shared/ipc-types";

/* ------------------------------------------------------------------ */
/*  Setting type configuration                                        */
/* ------------------------------------------------------------------ */

interface SelectOption {
  value: string;
  labelKey: string;
}

const CURRENCY_SELECT_OPTIONS: SelectOption[] = CURRENCY_CODE_OPTIONS.map((code) => ({
  value: code,
  labelKey: `options.currency.${code}`,
}));

const SELECT_OPTIONS: Record<string, SelectOption[]> = {
  "locale.primary_language": [
    { value: "fr", labelKey: "options.language.fr" },
    { value: "en", labelKey: "options.language.en" },
  ],
  "locale.fallback_language": [
    { value: "fr", labelKey: "options.language.fr" },
    { value: "en", labelKey: "options.language.en" },
  ],
  "locale.date_format": [
    { value: "DD/MM/YYYY", labelKey: "options.dateFormat.dmy" },
    { value: "MM/DD/YYYY", labelKey: "options.dateFormat.mdy" },
    { value: "YYYY-MM-DD", labelKey: "options.dateFormat.iso" },
  ],
  "locale.number_format": [
    { value: "fr-FR", labelKey: "options.numberFormat.fr" },
    { value: "en-US", labelKey: "options.numberFormat.en" },
  ],
  "locale.week_start_day": [
    { value: "1", labelKey: "options.weekStart.monday" },
    { value: "0", labelKey: "options.weekStart.sunday" },
  ],
  "locale.currency_code": CURRENCY_SELECT_OPTIONS,
  // Month the company's fiscal year starts in (budget periods); stored as a number 1..12.
  "locale.fiscal_year_start_month": Array.from({ length: 12 }, (_, i) => ({
    value: String(i + 1),
    labelKey: `options.month.${i + 1}`,
  })),
  "appearance.color_mode": [
    { value: "light", labelKey: "options.colorMode.light" },
    { value: "dark", labelKey: "options.colorMode.dark" },
  ],
  "appearance.density": [
    { value: "compact", labelKey: "options.density.compact" },
    { value: "standard", labelKey: "options.density.standard" },
    { value: "comfortable", labelKey: "options.density.comfortable" },
  ],
  "appearance.text_scale": [
    { value: "0.85", labelKey: "options.textScale.small" },
    { value: "1", labelKey: "options.textScale.default" },
    { value: "1.15", labelKey: "options.textScale.large" },
    { value: "1.3", labelKey: "options.textScale.extraLarge" },
  ],
  "updater.release_channel": [
    { value: "stable", labelKey: "options.channel.stable" },
    { value: "beta", labelKey: "options.channel.beta" },
  ],
};

const BOOLEAN_SETTINGS = new Set(["updater.auto_check"]);

const NUMBER_SETTINGS = new Set([
  "backup.retention_daily",
  "backup.retention_weekly",
  "backup.retention_monthly",
  "diagnostics.log_retention_days",
  "ram.fmeca_rpn_critical_threshold",
  "wo.reopen_window_days",
]);

/** Preferred General-tab category order. */
const CATEGORY_ORDER = [
  "localization",
  "appearance",
  "system",
  "work_orders",
  "reliability",
  "backup",
] as const;

/** Complex JSON settings rendered by dedicated panels — never as scalar rows. */
const COMPLEX_SETTING_KEYS = new Set(["documents.default_header"]);

type SettingType = "select" | "boolean" | "number" | "text";

function getSettingType(key: string): SettingType {
  if (SELECT_OPTIONS[key]) return "select";
  if (BOOLEAN_SETTINGS.has(key)) return "boolean";
  if (NUMBER_SETTINGS.has(key)) return "number";
  return "text";
}

function parseJsonValue(json: string): unknown {
  try {
    return JSON.parse(json);
  } catch {
    return json;
  }
}

/** Radix Select throws if value is empty or unmatched; normalize seeded JSON (e.g. 1.0 → "1"). */
function selectControlValue(settingKey: string, parsed: unknown): string | undefined {
  const options = SELECT_OPTIONS[settingKey] ?? [];
  if (options.length === 0) return undefined;
  const raw = String(parsed);
  if (options.some((o) => o.value === raw)) return raw;
  if (typeof parsed === "number") {
    const asInt = String(parsed);
    if (options.some((o) => o.value === asInt)) return asInt;
  }
  return undefined;
}

function categorySortIndex(category: string): number {
  const idx = CATEGORY_ORDER.indexOf(category as (typeof CATEGORY_ORDER)[number]);
  return idx === -1 ? CATEGORY_ORDER.length : idx;
}

/* ------------------------------------------------------------------ */
/*  Component                                                          */
/* ------------------------------------------------------------------ */

interface SettingsValueEditorProps {
  settings: AppSetting[];
  onSettingSaved: () => void;
  onToast: (msg: {
    title: string;
    description?: string;
    variant?: "default" | "destructive" | "success";
  }) => void;
}

export function SettingsValueEditor({
  settings,
  onSettingSaved,
  onToast,
}: SettingsValueEditorProps) {
  const { t } = useTranslation("settings");
  const { setTheme } = useTheme();
  const localeSetLocale = useLocaleStore((s) => s.setLocale);
  const setCurrencyCode = useLocaleStore((s) => s.setCurrencyCode);
  const activeLocale = useLocaleStore((s) => s.activeLocale);

  const [draftValues, setDraftValues] = useState<Record<string, string>>({});
  const [saving, setSaving] = useState(false);

  const groupedSettings = useMemo(() => {
    const byCategory = new Map<string, AppSetting[]>();
    for (const setting of settings) {
      if (COMPLEX_SETTING_KEYS.has(setting.setting_key)) continue;
      const list = byCategory.get(setting.category) ?? [];
      list.push(setting);
      byCategory.set(setting.category, list);
    }
    return [...byCategory.entries()].sort(
      ([a], [b]) => categorySortIndex(a) - categorySortIndex(b),
    );
  }, [settings]);

  const settingLabel = useCallback(
    (key: string): string => {
      const labelKey = `keys.${key}` as "keys.locale.primary_language";
      const translated = t(labelKey);
      return translated === labelKey ? key : String(translated);
    },
    [t],
  );

  const categoryLabel = useCallback(
    (category: string): string => {
      const labelKey = `categories.${category}` as "categories.localization";
      const translated = t(labelKey);
      return translated === labelKey ? category : String(translated);
    },
    [t],
  );

  /** Persist a setting value and apply live side-effects. */
  const saveValue = useCallback(
    async (setting: AppSetting, valueJson: string) => {
      setSaving(true);
      try {
        await setSetting({
          key: setting.setting_key,
          scope: setting.setting_scope,
          value_json: valueJson,
          change_summary: t("editor.auditUpdated", { key: setting.setting_key }),
        });

        const parsed = parseJsonValue(valueJson);

        if (setting.setting_key === "appearance.color_mode") {
          const mode = parsed === "dark" ? "dark" : "light";
          setTheme(mode);
        }

        if (setting.setting_key === "locale.primary_language" && typeof parsed === "string") {
          await localeSetLocale(parsed, true);
        }

        if (setting.setting_key === "locale.fallback_language" && typeof parsed === "string") {
          applyI18nFallbackLanguage(parsed);
        }

        if (setting.setting_key === "locale.currency_code" && typeof parsed === "string") {
          setCurrencyCode(parsed);
        }

        if (setting.setting_key === "appearance.text_scale" && typeof parsed === "number") {
          document.documentElement.style.fontSize = `${parsed}rem`;
        }

        if (setting.setting_key === "appearance.density" && typeof parsed === "string") {
          // biome-ignore lint/complexity/useLiteralKeys: dataset uses index signature (TS4111)
          document.documentElement.dataset["density"] = parsed;
        }

        onToast({ title: t("editor.saveSuccess"), variant: "success" });
        setDraftValues((prev) => {
          const next = { ...prev };
          delete next[setting.setting_key];
          return next;
        });
        onSettingSaved();
      } catch {
        onToast({ title: t("editor.saveError"), variant: "destructive" });
      } finally {
        setSaving(false);
      }
    },
    [setTheme, localeSetLocale, setCurrencyCode, onSettingSaved, onToast, t],
  );

  const handleSelectChange = useCallback(
    (setting: AppSetting, newVal: string) => {
      const type = getSettingType(setting.setting_key);
      let valueJson: string;
      // Numeric selects are stored as JSON numbers (match seeder), not quoted strings.
      if (
        type === "select" &&
        (setting.setting_key === "locale.week_start_day" ||
          setting.setting_key === "locale.fiscal_year_start_month" ||
          setting.setting_key === "appearance.text_scale")
      ) {
        valueJson = newVal;
      } else {
        valueJson = JSON.stringify(newVal);
      }
      void saveValue(setting, valueJson);
    },
    [saveValue],
  );

  const handleToggle = useCallback(
    (setting: AppSetting, checked: boolean) => {
      void saveValue(setting, JSON.stringify(checked));
    },
    [saveValue],
  );

  const commitDraft = useCallback(
    (setting: AppSetting) => {
      const type = getSettingType(setting.setting_key);
      const draft = draftValues[setting.setting_key];
      if (draft === undefined) return;

      const current = String(parseJsonValue(setting.setting_value_json));
      if (draft === current) {
        setDraftValues((prev) => {
          const next = { ...prev };
          delete next[setting.setting_key];
          return next;
        });
        return;
      }

      if (type === "number") {
        const num = Number(draft);
        if (Number.isNaN(num)) {
          onToast({ title: t("editor.invalidJson"), variant: "destructive" });
          return;
        }
        void saveValue(setting, JSON.stringify(num));
        return;
      }

      void saveValue(setting, JSON.stringify(draft));
    },
    [draftValues, saveValue, onToast, t],
  );

  const resolvedSelectValue = useCallback(
    (setting: AppSetting, parsed: unknown): string | undefined => {
      if (setting.setting_key === "locale.primary_language") {
        if (activeLocale === "fr" || activeLocale === "en") return activeLocale;
      }
      return selectControlValue(setting.setting_key, parsed);
    },
    [activeLocale],
  );

  if (settings.length === 0) {
    return (
      <div className="flex flex-1 items-center justify-center py-8 text-text-muted">
        <p>{t("page.noSettings")}</p>
      </div>
    );
  }

  const renderControl = (s: AppSetting) => {
    const type = getSettingType(s.setting_key);
    const parsed = parseJsonValue(s.setting_value_json);
    const selectValue = type === "select" ? resolvedSelectValue(s, parsed) : undefined;
    const draft = draftValues[s.setting_key] ?? String(parsed);

    if (type === "select") {
      return (
        <Select
          {...(selectValue !== undefined ? { value: selectValue } : {})}
          onValueChange={(v) => handleSelectChange(s, v)}
          disabled={saving}
        >
          <SelectTrigger className="h-9 w-full" aria-label={settingLabel(s.setting_key)}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {(SELECT_OPTIONS[s.setting_key] ?? []).map((opt) => (
              <SelectItem key={opt.value} value={opt.value}>
                {t(opt.labelKey as "options.language.fr")}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      );
    }

    if (type === "boolean") {
      return (
        <Switch
          checked={parsed === true}
          onCheckedChange={(c) => handleToggle(s, c)}
          disabled={saving}
          aria-label={settingLabel(s.setting_key)}
        />
      );
    }

    if (type === "number") {
      return (
        <Input
          type="number"
          value={draft}
          onChange={(e) => setDraftValues((prev) => ({ ...prev, [s.setting_key]: e.target.value }))}
          onBlur={() => commitDraft(s)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.currentTarget.blur();
            }
            if (e.key === "Escape") {
              setDraftValues((prev) => {
                const next = { ...prev };
                delete next[s.setting_key];
                return next;
              });
            }
          }}
          className="h-9 w-full"
          min={1}
          disabled={saving}
          aria-label={settingLabel(s.setting_key)}
        />
      );
    }

    return (
      <Input
        value={draft}
        onChange={(e) => setDraftValues((prev) => ({ ...prev, [s.setting_key]: e.target.value }))}
        onBlur={() => commitDraft(s)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.currentTarget.blur();
          }
          if (e.key === "Escape") {
            setDraftValues((prev) => {
              const next = { ...prev };
              delete next[s.setting_key];
              return next;
            });
          }
        }}
        className="h-9 w-full"
        disabled={saving}
        aria-label={settingLabel(s.setting_key)}
      />
    );
  };

  return (
    <div className="flex flex-col gap-4">
      {groupedSettings.map(([category, rows]) => (
        <Card key={category} className="shadow-none">
          <CardHeader className="space-y-0 px-5 py-4">
            <CardTitle className="text-base font-semibold leading-snug tracking-normal">
              {categoryLabel(category)}
            </CardTitle>
          </CardHeader>
          <CardContent className="px-5 pb-4 pt-0">
            <ul className="divide-y divide-border">
              {rows.map((s) => {
                const type = getSettingType(s.setting_key);
                return (
                  <li
                    key={s.id}
                    className="flex items-center justify-between gap-4 py-3 first:pt-1 last:pb-1"
                  >
                    <Label
                      id={`setting-label-${s.id}`}
                      className="min-w-0 flex-1 text-sm font-medium leading-snug text-text-primary"
                    >
                      {settingLabel(s.setting_key)}
                    </Label>
                    <div
                      className={cn(
                        "shrink-0",
                        type === "boolean" ? "flex justify-end" : "w-[min(100%,14rem)]",
                      )}
                      aria-labelledby={`setting-label-${s.id}`}
                    >
                      {renderControl(s)}
                    </div>
                  </li>
                );
              })}
            </ul>
          </CardContent>
        </Card>
      ))}
    </div>
  );
}
