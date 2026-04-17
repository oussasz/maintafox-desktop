import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { listSkillsMatrix } from "@/services/personnel-service";
import type { SkillMatrixRow } from "@shared/ipc-types";

interface SkillsMatrixPanelProps {
  entityId?: number | null;
  teamId?: number | null;
}

export function SkillsMatrixPanel({ entityId, teamId }: SkillsMatrixPanelProps) {
  const { t } = useTranslation("personnel");
  const [rows, setRows] = useState<SkillMatrixRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    void listSkillsMatrix({
      entity_id: entityId ?? null,
      team_id: teamId ?? null,
      include_inactive: false,
    })
      .then((data) => {
        if (!cancelled) setRows(data);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [entityId, teamId]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return rows;
    return rows.filter((row) =>
      [row.full_name, row.employee_code, row.skill_label ?? "", row.skill_code ?? ""]
        .join(" ")
        .toLowerCase()
        .includes(q),
    );
  }, [query, rows]);

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-lg">{t("tabs.skillsMatrix")}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <Input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t("skills.searchPlaceholder")}
          className="max-w-sm"
        />
        {error ? <div className="text-sm text-destructive">{error}</div> : null}
        <div className="overflow-auto rounded border">
          <table className="w-full text-sm">
            <thead className="bg-muted/50 text-left">
              <tr>
                <th className="px-3 py-2">{t("skills.columns.personnel")}</th>
                <th className="px-3 py-2">{t("skills.columns.team")}</th>
                <th className="px-3 py-2">{t("skills.columns.skill")}</th>
                <th className="px-3 py-2">{t("skills.columns.level")}</th>
                <th className="px-3 py-2">{t("skills.columns.coverage")}</th>
              </tr>
            </thead>
            <tbody>
              {loading ? (
                <tr>
                  <td className="px-3 py-3 text-text-muted" colSpan={5}>
                    {t("common.loading")}
                  </td>
                </tr>
              ) : filtered.length === 0 ? (
                <tr>
                  <td className="px-3 py-3 text-text-muted" colSpan={5}>
                    {t("common.noData")}
                  </td>
                </tr>
              ) : (
                filtered.map((row) => (
                  <tr key={`${row.personnel_id}-${row.skill_code ?? "none"}`} className="border-t">
                    <td className="px-3 py-2">
                      {row.full_name}
                      <span className="ml-2 text-xs text-text-muted">{row.employee_code}</span>
                    </td>
                    <td className="px-3 py-2">{row.team_name ?? " "}</td>
                    <td className="px-3 py-2">{row.skill_label ?? " "}</td>
                    <td className="px-3 py-2">{row.proficiency_level ?? " "}</td>
                    <td className="px-3 py-2">{t(`skills.coverage.${row.coverage_status}`)}</td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
      </CardContent>
    </Card>
  );
}


