import { Check, Pencil, X } from "lucide-react";
import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import { useTheme } from "@/components/ui/ThemeProvider";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
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
]);

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

  const [editingKey, setEditingKey] = useState<string | null>(null);
  const [editValue, setEditValue] = useState("");
  const [saving, setSaving] = useState(false);

  /* ---- helpers ---- */

  const settingLabel = useCallback(
    (key: string): string => {
      const labelKey = `keys.${key}` as "keys.locale.primary_language";
      const translated = t(labelKey);
      return translated === labelKey ? key : String(translated);
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
          change_summary: `Setting '${setting.setting_key}' updated via Settings UI`,
        });

        // --- Apply live side-effects ---
        const parsed = parseJsonValue(valueJson);

        if (setting.setting_key === "appearance.color_mode") {
          const mode = parsed === "dark" ? "dark" : "light";
          setTheme(mode);
        }

        if (setting.setting_key === "locale.primary_language" && typeof parsed === "string") {
          await localeSetLocale(parsed, true);
        }

        if (setting.setting_key === "appearance.text_scale" && typeof parsed === "number") {
          document.documentElement.style.fontSize = `${parsed}rem`;
        }

        if (setting.setting_key === "appearance.density" && typeof parsed === "string") {
          // biome-ignore lint/complexity/useLiteralKeys: dataset uses index signature (TS4111)
          document.documentElement.dataset["density"] = parsed;
        }

        onToast({ title: t("editor.saveSuccess"), variant: "success" });
        setEditingKey(null);
        setEditValue("");
        onSettingSaved();
      } catch {
        onToast({ title: t("editor.saveError"), variant: "destructive" });
      } finally {
        setSaving(false);
      }
    },
    [setTheme, localeSetLocale, onSettingSaved, onToast, t],
  );

  /* ---- inline editing for number / text fallback ---- */

  const startEdit = useCallback((setting: AppSetting) => {
    setEditingKey(setting.setting_key);
    const parsed = parseJsonValue(setting.setting_value_json);
    setEditValue(String(parsed));
  }, []);

  const cancelEdit = useCallback(() => {
    setEditingKey(null);
    setEditValue("");
  }, []);

  const confirmEdit = useCallback(
    (setting: AppSetting) => {
      const type = getSettingType(setting.setting_key);
      let valueJson: string;
      if (type === "number") {
        const num = Number(editValue);
        if (Number.isNaN(num)) {
          onToast({ title: t("editor.invalidJson"), variant: "destructive" });
          return;
        }
        valueJson = JSON.stringify(num);
      } else {
        valueJson = JSON.stringify(editValue);
      }
      void saveValue(setting, valueJson);
    },
    [editValue, saveValue, onToast, t],
  );

  /* ---- select change handler ---- */

  const handleSelectChange = useCallback(
    (setting: AppSetting, newVal: string) => {
      const type = getSettingType(setting.setting_key);
      let valueJson: string;
      if (type === "select" && setting.setting_key === "locale.week_start_day") {
        valueJson = newVal; // already numeric string like "1"
      } else if (type === "select" && setting.setting_key === "appearance.text_scale") {
        valueJson = newVal; // numeric string like "1.15"
      } else {
        valueJson = JSON.stringify(newVal);
      }
      void saveValue(setting, valueJson);
    },
    [saveValue],
  );

  /* ---- toggle handler ---- */

  const handleToggle = useCallback(
    (setting: AppSetting, checked: boolean) => {
      void saveValue(setting, JSON.stringify(checked));
    },
    [saveValue],
  );

  /* ---- render ---- */

  if (settings.length === 0) {
    return (
      <div className="flex flex-1 items-center justify-center text-text-muted">
        <p>{t("page.noSettings")}</p>
      </div>
    );
  }

  return (
    <div className="flex-1 overflow-auto">
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b text-left text-text-muted">
            <th className="pb-2 pr-4 font-medium">{t("table.key")}</th>
            <th className="pb-2 pr-4 font-medium">{t("table.value")}</th>
            <th className="pb-2 pr-4 font-medium">{t("table.scope")}</th>
            <th className="pb-2 pr-4 font-medium">{t("table.risk")}</th>
            <th className="pb-2 font-medium">{t("table.actions")}</th>
          </tr>
        </thead>
        <tbody>
          {settings.map((s) => {
            const type = getSettingType(s.setting_key);
            const isEditing = editingKey === s.setting_key;
            const parsed = parseJsonValue(s.setting_value_json);

            return (
              <tr key={s.id} className="border-b last:border-b-0">
                {/* Label */}
                <td className="py-3 pr-4">
                  <div className="font-medium text-text-primary">{settingLabel(s.setting_key)}</div>
                  <div className="text-xs text-text-muted">{s.setting_key}</div>
                </td>

                {/* Value control */}
                <td className="py-3 pr-4">
                  {type === "select" && (
                    <Select
                      value={String(parsed)}
                      onValueChange={(v) => handleSelectChange(s, v)}
                      disabled={saving}
                    >
                      <SelectTrigger className="h-8 w-52">
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
                  )}

                  {type === "boolean" && (
                    <Switch
                      checked={parsed === true}
                      onCheckedChange={(c) => handleToggle(s, c)}
                      disabled={saving}
                    />
                  )}

                  {type === "number" &&
                    (isEditing ? (
                      <Input
                        type="number"
                        value={editValue}
                        onChange={(e) => setEditValue(e.target.value)}
                        className="h-8 w-28 text-xs"
                        min={1}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") confirmEdit(s);
                          if (e.key === "Escape") cancelEdit();
                        }}
                        autoFocus
                      />
                    ) : (
                      <code className="rounded bg-muted px-2 py-1 text-xs">{String(parsed)}</code>
                    ))}

                  {type === "text" &&
                    (isEditing ? (
                      <Input
                        value={editValue}
                        onChange={(e) => setEditValue(e.target.value)}
                        className="h-8 w-48 font-mono text-xs"
                        onKeyDown={(e) => {
                          if (e.key === "Enter") confirmEdit(s);
                          if (e.key === "Escape") cancelEdit();
                        }}
                        autoFocus
                      />
                    ) : (
                      <code className="rounded bg-muted px-2 py-1 text-xs">{String(parsed)}</code>
                    ))}
                </td>

                {/* Scope */}
                <td className="py-3 pr-4">
                  <Badge variant="outline" className="text-xs">
                    {t(`scope.${s.setting_scope}` as "scope.tenant")}
                  </Badge>
                </td>

                {/* Risk */}
                <td className="py-3 pr-4">
                  <Badge
                    variant={s.setting_risk === "high" ? "destructive" : "secondary"}
                    className="text-xs"
                  >
                    {t(`risk.${s.setting_risk}` as "risk.low")}
                  </Badge>
                </td>

                {/* Actions — only for number/text types that need edit mode */}
                <td className="py-3">
                  {(type === "number" || type === "text") &&
                    (isEditing ? (
                      <div className="flex items-center gap-1">
                        <Button
                          size="sm"
                          variant="ghost"
                          onClick={() => confirmEdit(s)}
                          disabled={saving}
                          className="h-7 w-7 p-0"
                          aria-label={t("editor.save")}
                        >
                          <Check className="h-4 w-4 text-green-600" />
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          onClick={cancelEdit}
                          disabled={saving}
                          className="h-7 w-7 p-0"
                          aria-label={t("editor.cancel")}
                        >
                          <X className="h-4 w-4 text-red-500" />
                        </Button>
                      </div>
                    ) : (
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() => startEdit(s)}
                        className="h-7 w-7 p-0"
                        aria-label={t("editor.edit")}
                      >
                        <Pencil className="h-4 w-4" />
                      </Button>
                    ))}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
