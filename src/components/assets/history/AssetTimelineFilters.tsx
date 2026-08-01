import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { AssetHistoryPeriod } from "@shared/ipc-types";

type Props = {
  typeFilter: string;
  period: AssetHistoryPeriod;
  search: string;
  onTypeFilterChange: (v: string) => void;
  onPeriodChange: (v: AssetHistoryPeriod) => void;
  onSearchChange: (v: string) => void;
};

const TYPE_FILTERS = ["all", "ot", "di", "pm", "inspection", "documents", "failures"] as const;
const PERIOD_FILTERS: AssetHistoryPeriod[] = [
  "today",
  "last_7_days",
  "last_30_days",
  "this_year",
  "all",
];

export function AssetTimelineFilters({
  typeFilter,
  period,
  search,
  onTypeFilterChange,
  onPeriodChange,
  onSearchChange,
}: Props) {
  const { t } = useTranslation("equipment");
  return (
    <div className="space-y-2">
      <div className="flex flex-wrap gap-1.5">
        {TYPE_FILTERS.map((filter) => (
          <Button
            key={filter}
            type="button"
            size="sm"
            variant={typeFilter === filter ? "default" : "outline"}
            className="h-7 px-2.5 text-xs"
            onClick={() => onTypeFilterChange(filter)}
          >
            {t(`history.filters.types.${filter}`)}
          </Button>
        ))}
      </div>
      <div className="flex flex-wrap items-center gap-1.5">
        {PERIOD_FILTERS.map((value) => (
          <Button
            key={value}
            type="button"
            size="sm"
            variant={period === value ? "default" : "outline"}
            className="h-7 px-2.5 text-xs"
            onClick={() => onPeriodChange(value)}
          >
            {t(`history.filters.periods.${value}`)}
          </Button>
        ))}
        <Input
          value={search}
          onChange={(e) => onSearchChange(e.target.value)}
          placeholder={t("history.searchPlaceholder")}
          className="h-8 text-sm w-full sm:w-60"
        />
      </div>
    </div>
  );
}

