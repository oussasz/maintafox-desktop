/**
 * Lifecycle status badge for equipment — shared by list and detail views.
 */

import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";

const STATUS_STYLE: Record<string, string> = {
  ACTIVE: "bg-green-100 text-green-800 dark:bg-green-950/40 dark:text-green-300",
  ACTIVE_IN_SERVICE: "bg-green-100 text-green-800 dark:bg-green-950/40 dark:text-green-300",
  IN_STOCK: "bg-sky-100 text-sky-800 dark:bg-sky-950/40 dark:text-sky-300",
  OUT_OF_SERVICE: "bg-slate-200 text-slate-800 dark:bg-slate-800 dark:text-slate-200",
  UNDER_MAINTENANCE: "bg-amber-100 text-amber-900 dark:bg-amber-950/40 dark:text-amber-200",
  STANDBY: "bg-amber-100 text-amber-800 dark:bg-amber-950/40 dark:text-amber-200",
  MAINTENANCE: "bg-blue-100 text-blue-800 dark:bg-blue-950/40 dark:text-blue-200",
  DECOMMISSIONED: "bg-red-100 text-red-800 dark:bg-red-950/40 dark:text-red-200",
  SCRAPPED: "bg-slate-100 text-slate-600 dark:bg-slate-800 dark:text-slate-300",
  SPARE: "bg-slate-100 text-slate-600 dark:bg-slate-800 dark:text-slate-300",
};

const STATUS_ALIAS: Record<string, string> = {
  OPERATIONAL: "ACTIVE",
  ACTIVE_IN_SERVICE: "ACTIVE",
};

function normalizeStatusCode(code: string): string {
  const normalized = code.trim().toUpperCase();
  return STATUS_ALIAS[normalized] ?? normalized;
}

function formatFallbackStatusLabel(code: string): string {
  return code
    .toLowerCase()
    .split("_")
    .filter(Boolean)
    .map((part) => part[0]?.toUpperCase() + part.slice(1))
    .join(" ");
}

interface AssetStatusBadgeProps {
  code: string;
  /** Larger badge for detail hero */
  size?: "sm" | "md";
}

export function AssetStatusBadge({ code, size = "sm" }: AssetStatusBadgeProps) {
  const { t } = useTranslation("equipment");
  const normalizedCode = normalizeStatusCode(code);
  const sizeClass = size === "md" ? "text-sm px-3 py-1" : "text-[10px]";
  const className = `${sizeClass} border-0 ${STATUS_STYLE[normalizedCode] ?? "bg-gray-100 text-gray-600 dark:bg-gray-800 dark:text-gray-300"}`;

  switch (normalizedCode) {
    case "ACTIVE":
      return (
        <Badge variant="outline" className={className}>
          {t("status.operational")}
        </Badge>
      );
    case "STANDBY":
      return (
        <Badge variant="outline" className={className}>
          {t("status.standby")}
        </Badge>
      );
    case "MAINTENANCE":
    case "UNDER_MAINTENANCE":
      return (
        <Badge variant="outline" className={className}>
          {t("status.maintenance")}
        </Badge>
      );
    case "IN_STOCK":
      return (
        <Badge variant="outline" className={className}>
          {t("status.inStock")}
        </Badge>
      );
    case "OUT_OF_SERVICE":
      return (
        <Badge variant="outline" className={className}>
          {t("status.outOfService")}
        </Badge>
      );
    case "DECOMMISSIONED":
      return (
        <Badge variant="outline" className={className}>
          {t("status.decommissioned")}
        </Badge>
      );
    case "SCRAPPED":
      return (
        <Badge variant="outline" className={className}>
          {t("status.scrapped")}
        </Badge>
      );
    default:
      return (
        <Badge variant="outline" className={className}>
          {formatFallbackStatusLabel(normalizedCode)}
        </Badge>
      );
  }
}
