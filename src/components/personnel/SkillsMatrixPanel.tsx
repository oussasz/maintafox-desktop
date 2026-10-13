/**
 * SkillsMatrixPanel — people × skills coverage heatmap (team-scoped preferred).
 */

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { TeamSkillsHeatmap } from "@/components/personnel/TeamSkillsHeatmap";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
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

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-lg">{t("tabs.skillsMatrix")}</CardTitle>
      </CardHeader>
      <CardContent>
        <TeamSkillsHeatmap rows={rows} loading={loading} error={error} />
      </CardContent>
    </Card>
  );
}
