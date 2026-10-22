/**
 * Qualification matrix — people × certifications and skills, each cell coloured by state
 * (valid, runs out soon, expired, waiting for verification, self-declared). A click opens the
 * person's qualification sheet to record or renew.
 */
import { Grid3x3 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import type { SmartFilterDef } from "@/components/filters/smart-filter-types";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusChip } from "@/components/ui/status-chip";
import { Switch } from "@/components/ui/switch";
import { mfTone } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { cn } from "@/lib/utils";
import {
  type QualificationMatrix,
  type QualificationMatrixCell,
  type QualificationMatrixColumn,
  type QualificationMatrixRow,
  getQualificationMatrix,
} from "@/services/qualification-gate-service";
import type { ExternalCompany, TeamSummary } from "@shared/ipc-types";

import { PersonQualificationDialog } from "./PersonQualificationSheet";
import { MATRIX_CELL_TONE } from "./qualification-tones";

const PROBLEM_STATUSES = new Set(["expired", "expiring_soon", "pending", "rejected"]);
/** Cell states for which clicking opens the "record / renew" form directly. */
const RENEWABLE = new Set(["expired", "expiring_soon", "rejected"]);

export interface QualificationMatrixViewProps {
  teams: TeamSummary[];
  companies: ExternalCompany[];
  /** Teams and companies were not read (no personnel right): those filters are not offered. */
  peopleRestricted?: boolean;
  /** Bumped by the page after a change elsewhere (catalogue, training) to reload. */
  refreshKey?: number;
}

export function QualificationMatrixView({
  teams,
  companies,
  peopleRestricted = false,
  refreshKey = 0,
}: QualificationMatrixViewProps) {
  const { t } = useTranslation("training");
  const { formatDate } = useFormatters();
  const [matrix, setMatrix] = useState<QualificationMatrix | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);
  const [teamId, setTeamId] = useState<string | null>(null);
  const [companyId, setCompanyId] = useState<string | null>(null);
  const [family, setFamily] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [problemsOnly, setProblemsOnly] = useState(false);
  const [allColumns, setAllColumns] = useState(false);
  const [open, setOpen] = useState<{
    row: QualificationMatrixRow;
    column: QualificationMatrixColumn | null;
    cell: QualificationMatrixCell | null;
  } | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setMatrix(
        await getQualificationMatrix({
          team_id: teamId ? Number(teamId) : null,
          company_id: companyId ? Number(companyId) : null,
        }),
      );
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, [teamId, companyId]);

  useEffect(() => {
    void load();
  }, [load, refreshKey]);

  const cellIndex = useMemo(() => {
    const map = new Map<string, QualificationMatrixCell>();
    for (const row of matrix?.rows ?? []) {
      for (const cell of row.cells) map.set(`${row.personnel_id}|${cell.key}`, cell);
    }
    return map;
  }, [matrix]);

  const rows = useMemo(() => {
    const q = search.trim().toLowerCase();
    return (matrix?.rows ?? []).filter((r) => {
      if (q && !`${r.full_name} ${r.employee_code}`.toLowerCase().includes(q)) return false;
      if (problemsOnly && !r.cells.some((c) => PROBLEM_STATUSES.has(c.status))) return false;
      return true;
    });
  }, [matrix, search, problemsOnly]);

  const columns = useMemo(() => {
    const used = new Set((matrix?.rows ?? []).flatMap((r) => r.cells.map((c) => c.key)));
    return (matrix?.columns ?? []).filter((c) => {
      if (family === "skills" && c.kind !== "skill") return false;
      if (family && family !== "skills" && c.category !== family) return false;
      return allColumns || used.has(c.key);
    });
  }, [matrix, family, allColumns]);

  const families = useMemo(
    () => [
      ...new Set(
        (matrix?.columns ?? []).filter((c) => c.kind === "certification").map((c) => c.category),
      ),
    ],
    [matrix],
  );

  const kpis = useMemo(() => {
    const cells = (matrix?.rows ?? []).flatMap((r) => r.cells);
    return {
      people: matrix?.rows.length ?? 0,
      expired: cells.filter((c) => c.status === "expired").length,
      expiring: cells.filter((c) => c.status === "expiring_soon").length,
      pending: cells.filter((c) => c.status === "pending" || c.status === "unvalidated").length,
    };
  }, [matrix]);

  const filters: SmartFilterDef[] = [
    // Without the personnel right the team and company lists cannot be read: no empty filter.
    ...(peopleRestricted
      ? []
      : ([
          {
            id: "team",
            kind: "select",
            label: t("filter.team"),
            allLabel: t("filter.allTeams"),
            options: teams.map((tm) => ({ value: String(tm.id), label: tm.name })),
            value: teamId,
            onChange: setTeamId,
          },
          {
            id: "company",
            kind: "select",
            label: t("filter.company"),
            allLabel: t("filter.allCompanies"),
            options: companies.map((co) => ({ value: String(co.id), label: co.name })),
            value: companyId,
            onChange: setCompanyId,
          },
        ] satisfies SmartFilterDef[])),
    {
      id: "family",
      kind: "select",
      label: t("filter.family"),
      allLabel: t("filter.allFamilies"),
      options: [
        ...families.map((f) => ({ value: f, label: t(`category.${f}`) })),
        { value: "skills", label: t("filter.skillsOnly") },
      ],
      value: family,
      onChange: setFamily,
    },
  ];

  const cellLabel = (
    row: QualificationMatrixRow,
    col: QualificationMatrixColumn,
    cell?: QualificationMatrixCell,
  ) => {
    if (!cell) return t("matrix.cellNone", { person: row.full_name, qualification: col.label });
    return t("matrix.cellAria", {
      person: row.full_name,
      qualification: col.label,
      status: t(`cellStatus.${cell.status}`),
      level: cell.level ?? "",
      expiresAt: cell.expires_at ? formatDate(cell.expires_at) : t("sheet.noExpiry"),
    });
  };

  return (
    <div className="space-y-3">
      <SmartFilterBar
        searchPlaceholder={t("matrix.search")}
        searchValue={search}
        onSearchInputChange={setSearch}
        onSearchChange={setSearch}
        filters={filters}
        resultCount={rows.length}
        onReset={() => {
          setTeamId(null);
          setCompanyId(null);
          setFamily(null);
          setSearch("");
          setProblemsOnly(false);
        }}
      />
      <div className="flex flex-wrap items-center gap-x-6 gap-y-2 text-sm">
        <span className="text-text-muted">{t("matrix.kpiPeople", { count: kpis.people })}</span>
        <span className={mfTone.danger.text}>
          {t("matrix.kpiExpired", { count: kpis.expired })}
        </span>
        <span className={mfTone.attention.text}>
          {t("matrix.kpiExpiring", { count: kpis.expiring })}
        </span>
        <span className={mfTone.info.text}>{t("matrix.kpiPending", { count: kpis.pending })}</span>
        <label className="ml-auto flex items-center gap-2">
          <Switch
            checked={problemsOnly}
            onCheckedChange={setProblemsOnly}
            aria-label={t("matrix.problemsOnly")}
          />
          {t("matrix.problemsOnly")}
        </label>
        <label className="flex items-center gap-2">
          <Switch
            checked={allColumns}
            onCheckedChange={setAllColumns}
            aria-label={t("matrix.allColumns")}
          />
          {t("matrix.allColumns")}
        </label>
      </div>
      <div className="flex flex-wrap gap-2" aria-label={t("matrix.legend")}>
        {(["valid", "expiring_soon", "expired", "pending", "unvalidated"] as const).map((s) => (
          <StatusChip key={s} label={t(`cellStatus.${s}`)} tone={MATRIX_CELL_TONE[s]} dot />
        ))}
      </div>

      {error && !matrix ? (
        <ErrorState error={error} onRetry={() => void load()} retrying={loading} />
      ) : loading && !matrix ? (
        <LoadingState />
      ) : rows.length === 0 || columns.length === 0 ? (
        <EmptyState
          icon={<Grid3x3 className="h-8 w-8" aria-hidden />}
          title={matrix?.rows.length ? t("matrix.noMatch") : t("matrix.empty")}
          {...(matrix?.rows.length ? {} : { description: t("matrix.emptyHint") })}
        />
      ) : (
        <div
          className="overflow-auto rounded-md border border-surface-border"
          style={{ maxHeight: "70vh" }}
        >
          <table className="border-collapse text-xs">
            <caption className="sr-only">{t("matrix.caption")}</caption>
            <thead className="sticky top-0 z-20 bg-surface-1">
              <tr>
                <th
                  scope="col"
                  className="sticky left-0 z-30 min-w-[220px] border-b border-r border-surface-border bg-surface-1 px-3 py-2 text-left text-sm font-semibold"
                >
                  {t("col.person")}
                </th>
                {columns.map((c) => (
                  <th
                    key={c.key}
                    scope="col"
                    title={c.label}
                    className="h-36 min-w-[44px] border-b border-surface-border px-1 align-bottom font-medium text-text-secondary"
                  >
                    <span className="inline-block max-h-32 overflow-hidden text-ellipsis whitespace-nowrap [writing-mode:vertical-rl] rotate-180">
                      {c.kind === "skill" ? c.label : c.code}
                    </span>
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr key={row.personnel_id} className="hover:bg-surface-2/50">
                  <th
                    scope="row"
                    className="sticky left-0 z-10 border-b border-r border-surface-border bg-surface-0 px-3 py-1.5 text-left font-normal"
                  >
                    <button
                      type="button"
                      className="text-left text-sm font-medium text-text-primary hover:underline focus:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                      onClick={() => setOpen({ row, column: null, cell: null })}
                    >
                      {row.full_name}
                    </button>
                    <div className="text-[11px] text-text-muted">
                      {[row.employee_code, row.company_name ?? row.team_name]
                        .filter(Boolean)
                        .join(" · ")}
                    </div>
                  </th>
                  {columns.map((col) => {
                    const cell = cellIndex.get(`${row.personnel_id}|${col.key}`);
                    return (
                      <td
                        key={col.key}
                        className="border-b border-surface-border p-0.5 text-center"
                      >
                        <button
                          type="button"
                          aria-label={cellLabel(row, col, cell)}
                          title={cellLabel(row, col, cell)}
                          onClick={() => setOpen({ row, column: col, cell: cell ?? null })}
                          className={cn(
                            "flex h-8 w-10 items-center justify-center rounded border text-[11px] tabular-nums focus:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                            cell
                              ? mfTone[MATRIX_CELL_TONE[cell.status]].chip
                              : "border-transparent text-text-muted hover:border-surface-border",
                          )}
                        >
                          {cell
                            ? col.kind === "skill"
                              ? cell.level
                              : (cell.expires_at?.slice(2, 7) ?? "∞")
                            : "·"}
                        </button>
                      </td>
                    );
                  })}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {open ? (
        <PersonQualificationDialog
          open
          onOpenChange={(o) => {
            if (!o) setOpen(null);
          }}
          personName={open.row.full_name}
          personnelId={open.row.personnel_id}
          focusCertificationTypeId={
            open.column?.kind === "certification" &&
            (open.cell == null || RENEWABLE.has(open.cell.status))
              ? open.column.id
              : null
          }
          onChanged={() => void load()}
        />
      ) : null}
    </div>
  );
}
