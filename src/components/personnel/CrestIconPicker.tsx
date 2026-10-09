/**
 * CrestIconPicker — Lucide crest + accent swatches for crews.
 */

import { useTranslation } from "react-i18next";

import { CREW_COLOR_SWATCHES, CREW_ICON_OPTIONS } from "@/components/personnel/crew-crest";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";

export type CrestIconPickerProps = {
  iconKey: string;
  accentColor: string;
  onIconKeyChange: (key: string) => void;
  onAccentColorChange: (color: string) => void;
};

export function CrestIconPicker({
  iconKey,
  accentColor,
  onIconKeyChange,
  onAccentColorChange,
}: CrestIconPickerProps) {
  const { t } = useTranslation("personnel");

  return (
    <div className="space-y-3">
      <div className="space-y-1.5">
        <Label className="text-xs">{t("crews.form.icon")}</Label>
        <div className="flex flex-wrap gap-1.5">
          <button
            type="button"
            className={cn(
              "rounded-md border px-2 py-1 text-[10px]",
              !iconKey
                ? "border-text-primary bg-surface-2"
                : "border-surface-border text-text-muted",
            )}
            onClick={() => onIconKeyChange("")}
          >
            {t("crews.form.iconNone")}
          </button>
          {CREW_ICON_OPTIONS.map(({ key, Icon }) => (
            <button
              key={key}
              type="button"
              title={key}
              className={cn(
                "inline-flex h-8 w-8 items-center justify-center rounded-md border",
                iconKey === key
                  ? "border-text-primary bg-surface-2"
                  : "border-surface-border hover:bg-surface-2",
              )}
              onClick={() => onIconKeyChange(key)}
            >
              <Icon className="h-4 w-4" />
            </button>
          ))}
        </div>
      </div>
      <div className="space-y-1.5">
        <Label className="text-xs">{t("crews.form.accent")}</Label>
        <div className="flex flex-wrap gap-1.5">
          <button
            type="button"
            className={cn(
              "rounded-md border px-2 py-1 text-[10px]",
              !accentColor
                ? "border-text-primary bg-surface-2"
                : "border-surface-border text-text-muted",
            )}
            onClick={() => onAccentColorChange("")}
          >
            {t("crews.form.accentNone")}
          </button>
          {CREW_COLOR_SWATCHES.map((c) => (
            <button
              key={c}
              type="button"
              className={cn(
                "h-6 w-6 rounded-full border-2",
                accentColor === c ? "border-text-primary scale-110" : "border-transparent",
              )}
              style={{ backgroundColor: c }}
              onClick={() => onAccentColorChange(c)}
              aria-label={c}
            />
          ))}
        </div>
      </div>
    </div>
  );
}
