/**
 * People × skills coverage heatmap for a crew (read-only).
 */

import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { EmptyState } from "@/components/ui/empty-state";
import { RESOURCE_TIMELINE_STATE_CLASS } from "@/design-system/resource-timeline-tokens";
import { formatPersonLabel } from "@/lib/display";
import { cn } from "@/lib/utils";
import type { SkillMatrixRow } from "@shared/ipc-types";

export type TeamSkillsHeatmapProps = {
  rows: SkillMatrixRow[];
  loading?: boolean;
  error?: string | null;
};

type Coverage = "active" | "expired" | "missing";

function coverageClass(status: string): string {
  if (status === "active") return RESOURCE_TIMELINE_STATE_CLASS.free;
  if (status === "expired") return RESOURCE_TIMELINE_STATE_CLASS.critical;
  return RESOURCE_TIMELINE_STATE_CLASS.rest;
}

export function TeamSkillsHeatmap({ rows, loading, error }: TeamSkillsHeatmapProps) {
  const { t } = useTranslation("personnel");

  const { people, skills, cell } = useMemo(() => {
    const peopleMap = new Map<number, { id: number; name: string; code: string }>();
    const skillsMap = new Map<string, string>();
    const cells = new Map<string, Coverage>();

    for (const row of rows) {
      peopleMap.set(row.personnel_id, {
        id: row.personnel_id,
        name: row.full_name,
        code: row.employee_code,
      });
      if (row.skill_code) {
        skillsMap.set(row.skill_code, row.skill_label ?? row.skill_code);
        const status = (row.coverage_status || "missing") as Coverage;
        cells.set(`${row.personnel_id}:${row.skill_code}`, status);
      }
    }

    return {
      people: [...peopleMap.values()].sort((a, b) => a.name.localeCompare(b.name)),
      skills: [...skillsMap.entries()]
        .map(([code, label]) => ({ code, label }))
        .sort((a, b) => a.label.localeCompare(b.label)),
      cell: cells,
    };
  }, [rows]);

  if (error) {
    return <div className="text-sm text-destructive">{error}</div>;
  }
  if (loading) {
    return <p className="text-sm text-text-muted">{t("common.loading")}</p>;
  }
  if (people.length === 0 || skills.length === 0) {
    return <EmptyState title={t("crews.detail.emptySkills")} />;
  }

  return (
    <div className="overflow-auto rounded-md border border-surface-border">
      <table className="min-w-full border-collapse text-xs">
        <thead>
          <tr className="bg-muted/40">
            <th className="sticky left-0 z-10 bg-muted/40 px-2 py-2 text-left font-medium">
              {t("crews.detail.skillMapPerson")}
            </th>
            {skills.map((s) => (
              <th
                key={s.code}
                className="max-w-[88px] truncate px-1.5 py-2 text-left font-medium"
                title={s.label}
              >
                {s.label}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {people.map((p) => (
            <tr key={p.id} className="border-t border-surface-border">
              <td className="sticky left-0 z-10 bg-surface-1 px-2 py-1.5 font-medium">
                {formatPersonLabel(p.name)}
                <span className="ml-1 text-[10px] text-text-muted">{p.code}</span>
              </td>
              {skills.map((s) => {
                const status = cell.get(`${p.id}:${s.code}`) ?? "missing";
                return (
                  <td key={s.code} className="px-1 py-1">
                    <span
                      className={cn(
                        "inline-flex min-w-[3.5rem] justify-center rounded border px-1 py-0.5",
                        coverageClass(status),
                      )}
                      title={t(`crews.detail.coverageStatus.${status}`)}
                    >
                      {t(`crews.detail.coverageStatus.${status}`)}
                    </span>
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
