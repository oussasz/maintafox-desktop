import { useMemo, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { UserRound } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { cn } from "@/lib/utils";
import type { Personnel } from "@shared/ipc-types";

import { personnelAvailabilityBadgeClass, personnelAvailabilityDotClass } from "./personnel-status-styles";

export interface PersonnelCardProps {
  personnel: Personnel;
  onViewDetails: () => void;
}

export function PersonnelCard({ personnel, onViewDetails }: PersonnelCardProps) {
  const { t } = useTranslation("personnel");

  const empKey = personnel.employment_type as "employee" | "contractor" | "temp" | "vendor";
  const employmentLabel = t(`employmentType.${empKey}`, { defaultValue: personnel.employment_type });

  return (
    <Card className="flex flex-col overflow-hidden border-surface-border shadow-sm">
      <CardHeader className="flex flex-row items-start gap-3 space-y-0 pb-2">
        <div className="flex h-12 w-12 shrink-0 items-center justify-center rounded-full bg-muted">
          {personnel.photo_path ? (
            <img
              src={convertFileSrc(personnel.photo_path)}
              className="h-12 w-12 rounded-full object-cover"
              alt=""
            />
          ) : (
            <UserRound className="h-7 w-7 text-muted-foreground" aria-hidden />
          )}
        </div>
        <div className="min-w-0 flex-1">
          <div className="truncate font-semibold leading-tight">{personnel.full_name}</div>
          <div className="truncate text-xs text-muted-foreground">
            {[personnel.position_name, personnel.entity_name].filter(Boolean).join(" • ") || "—"}
          </div>
        </div>
      </CardHeader>
      <CardContent className="flex flex-1 flex-col gap-3 pt-0">
        <div className="h-px w-full bg-border" />
        <div className="flex items-center gap-2 text-sm">
          <span
            className={cn("inline-block h-2 w-2 shrink-0 rounded-full", personnelAvailabilityDotClass(personnel.availability_status))}
            aria-hidden
          />
          <span className="text-muted-foreground">{t("card.statusLabel")}:</span>
          <Badge className={cn("text-xs font-normal", personnelAvailabilityBadgeClass(personnel.availability_status))}>
            {t(`status.${personnel.availability_status}` as const)}
          </Badge>
        </div>
        <div className="text-sm">
          <span className="text-muted-foreground">{t("card.teamLabel")}: </span>
          <span>{personnel.team_name ?? "—"}</span>
        </div>
        <div className="text-sm">
          <span className="text-muted-foreground">{t("card.scheduleLabel")}: </span>
          <span>{personnel.schedule_name ?? "—"}</span>
        </div>
        <div className="text-sm">
          <span className="text-muted-foreground">{t("field.employmentType")}: </span>
          <span>{employmentLabel}</span>
        </div>
        {personnel.company_name ? (
          <div className="text-sm">
            <span className="text-muted-foreground">{t("card.companyLabel")}: </span>
            <span>{personnel.company_name}</span>
          </div>
        ) : null}
        <div className="text-sm">
          <span className="text-muted-foreground">{t("card.phoneLabel")}: </span>
          <span>{personnel.phone ?? "—"}</span>
        </div>
        <Button type="button" variant="secondary" size="sm" className="mt-auto w-full" onClick={onViewDetails}>
          {t("card.viewDetails")}
        </Button>
      </CardContent>
    </Card>
  );
}

interface PersonnelPickerComboboxProps {
  items: Personnel[];
  value: number | null;
  onChange: (personnelId: number | null) => void;
  placeholder?: string;
  disabled?: boolean;
}

export function PersonnelPickerCombobox({
  items,
  value,
  onChange,
  placeholder = "Select personnel",
  disabled = false,
}: PersonnelPickerComboboxProps) {
  const [query, setQuery] = useState("");
  const filtered = useMemo(() => {
    const term = query.trim().toLowerCase();
    let base: Personnel[];
    if (!term) {
      base = items.slice(0, 100);
    } else {
      base = items
        .filter((p) => `${p.full_name} ${p.employee_code}`.toLowerCase().includes(term))
        .slice(0, 100);
    }
    if (value != null) {
      const selected = items.find((p) => p.id === value);
      if (selected && !base.some((p) => p.id === value)) {
        base = [selected, ...base].slice(0, 100);
      }
    }
    return base;
  }, [items, query, value]);

  return (
    <div className="space-y-2">
      <input
        type="text"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder={placeholder}
        className="h-9 w-full rounded border bg-background px-2 text-sm"
        disabled={disabled}
      />
      <select
        className="h-9 w-full rounded border bg-background px-2 text-sm"
        value={value ?? ""}
        onChange={(e) => onChange(e.target.value ? Number(e.target.value) : null)}
        disabled={disabled}
      >
        <option value="">--</option>
        {filtered.map((p) => (
          <option key={p.id} value={p.id}>
            {p.full_name} ({p.employee_code})
          </option>
        ))}
      </select>
    </div>
  );
}
