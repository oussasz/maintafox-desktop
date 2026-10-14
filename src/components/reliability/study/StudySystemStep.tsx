/**
 * Study step 5 — system model: reliability block diagram and fault tree fed by the study (fit,
 * recorded MTBF of the study equipment, or typed values), and the advanced tools. Opening the step
 * writes nothing; "Enregistrer le modèle" stores both models with the study.
 */
import { Save } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import { mfLayout } from "@/design-system/tokens";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import {
  type FtResult,
  type FtSpec,
  type RbdResult,
  type RbdSpec,
  type SystemModel,
  evaluateReliabilityStudyFaultTree,
  evaluateReliabilityStudyRbd,
  getReliabilityStudySystemModel,
  saveReliabilityStudySystemModel,
} from "@/services/reliability-model-service";
import type { ReliabilityStudy } from "@/services/reliability-study-service";
import { P } from "@shared/rbac/permissions.generated";

import { AdvancedTools } from "./AdvancedTools";
import { serializeSource } from "./SourceFields";
import { SystemFtaEditor } from "./SystemFtaEditor";
import { SystemRbdEditor, newNodeId } from "./SystemRbdEditor";

type Tab = "rbd" | "fta" | "advanced";

const HOURS_PER_YEAR = 8760;

function cleanRbd(spec: RbdSpec): RbdSpec {
  return { ...spec, nodes: spec.nodes.map((n) => ({ ...n, source: serializeSource(n.source) })) };
}
function cleanFt(spec: FtSpec): FtSpec {
  return { ...spec, nodes: spec.nodes.map((n) => ({ ...n, source: serializeSource(n.source) })) };
}

export function StudySystemStep({
  study,
  editable,
}: {
  study: ReliabilityStudy;
  editable: boolean;
}) {
  const { t } = useTranslation("reliability");
  const { formatNumber } = useFormatters();
  const { can } = usePermissions();
  const [model, setModel] = useState<SystemModel | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);
  const [tab, setTab] = useState<Tab>("rbd");
  const [mission, setMission] = useState("");
  const [rbd, setRbd] = useState<RbdSpec | null>(null);
  const [fta, setFta] = useState<FtSpec | null>(null);
  const [rbdSel, setRbdSel] = useState<string | null>(null);
  const [ftaSel, setFtaSel] = useState<string | null>(null);
  const [rbdResult, setRbdResult] = useState<RbdResult | null>(null);
  const [ftaResult, setFtaResult] = useState<FtResult | null>(null);
  const [saving, setSaving] = useState(false);
  const [dirty, setDirty] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const m = await getReliabilityStudySystemModel(study.id);
      setModel(m);
      setMission(m.mission_hours != null ? String(m.mission_hours) : "");
      setRbd(m.rbd?.spec ?? null);
      setFta(m.fta?.spec ?? null);
      setRbdSel(m.rbd?.spec.root_id ?? null);
      setFtaSel(m.fta?.spec.top_id ?? null);
      setDirty(false);
    } catch (e) {
      setError(e);
    } finally {
      setLoading(false);
    }
  }, [study.id]);

  useEffect(() => {
    void load();
  }, [load]);

  const missionN = Number(mission.replace(",", "."));
  const missionOk = Number.isFinite(missionN) && missionN > 0;

  useEffect(() => {
    if (!rbd || !missionOk) {
      setRbdResult(null);
      return;
    }
    const h = setTimeout(() => {
      void evaluateReliabilityStudyRbd({
        study_id: study.id,
        mission_hours: missionN,
        spec: cleanRbd(rbd),
      })
        .then(setRbdResult)
        .catch(() => setRbdResult(null));
    }, 300);
    return () => clearTimeout(h);
  }, [rbd, missionN, missionOk, study.id]);

  useEffect(() => {
    if (!fta || !missionOk) {
      setFtaResult(null);
      return;
    }
    const h = setTimeout(() => {
      void evaluateReliabilityStudyFaultTree({
        study_id: study.id,
        mission_hours: missionN,
        spec: cleanFt(fta),
      })
        .then(setFtaResult)
        .catch(() => setFtaResult(null));
    }, 300);
    return () => clearTimeout(h);
  }, [fta, missionN, missionOk, study.id]);

  if (error != null && model == null)
    return <ErrorState error={error} onRetry={() => void load()} retrying={loading} />;
  if (model == null) return <LoadingState />;

  const canEdit = editable && model.editable && can(P.RAM_ANALYZE);
  const fit = model.fit;

  const startRbdFromEquipment = () => {
    const root = newNodeId("s");
    const blocks = study.assets.map((a) => ({
      id: newNodeId("b"),
      label: a.asset_code,
      node_type: "block" as const,
      children: [],
      k: null,
      source: fit
        ? ({ kind: "study_fit" } as const)
        : ({ kind: "equipment_history", equipment_id: a.equipment_id } as const),
      group: a.class_name,
      mttr_hours: null,
    }));
    setRbd({
      root_id: root,
      nodes: [
        {
          id: root,
          label: t("rams.system.newSystem"),
          node_type: "parallel",
          children: blocks.map((b) => b.id),
          k: 1,
          source: null,
          group: null,
          mttr_hours: null,
        },
        ...blocks,
      ],
    });
    setRbdSel(root);
    setDirty(true);
  };
  const startEmptyRbd = () => {
    const root = newNodeId("s");
    setRbd({
      root_id: root,
      nodes: [
        {
          id: root,
          label: t("rams.system.newSystem"),
          node_type: "series",
          children: [],
          k: null,
          source: null,
          group: null,
          mttr_hours: null,
        },
      ],
    });
    setRbdSel(root);
    setDirty(true);
  };
  const startFta = () => {
    const top = newNodeId("g");
    setFta({
      top_id: top,
      nodes: [
        { id: top, label: t("rams.system.newTop"), node_type: "or", inputs: [], source: null },
      ],
    });
    setFtaSel(top);
    setDirty(true);
  };

  const save = async () => {
    if (!missionOk) return;
    setSaving(true);
    try {
      const m = await saveReliabilityStudySystemModel({
        study_id: study.id,
        mission_hours: missionN,
        rbd: rbd ? cleanRbd(rbd) : null,
        fta: fta ? cleanFt(fta) : null,
      });
      setModel(m);
      setDirty(false);
    } catch {
      // global IPC toast (incomplete model)
    } finally {
      setSaving(false);
    }
  };
  const incomplete =
    (rbd != null && (rbdResult?.issues.length ?? 1) > 0) ||
    (fta != null && (ftaResult?.issues.length ?? 1) > 0);

  return (
    <div className="grid gap-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div
          className="inline-flex rounded-md border border-surface-border bg-surface-1 p-0.5"
          role="group"
          aria-label={t("rams.system.tool")}
        >
          {(["rbd", "fta", "advanced"] as Tab[]).map((k) => (
            <button
              key={k}
              type="button"
              aria-pressed={tab === k}
              onClick={() => setTab(k)}
              className={cn(
                "min-h-9 rounded px-3 text-sm",
                tab === k ? "bg-primary text-white" : "text-text-secondary hover:bg-surface-2",
              )}
            >
              {t(`rams.system.tab.${k}`)}
            </button>
          ))}
        </div>
        <div className="flex flex-wrap items-end gap-2">
          <label className="grid gap-1 text-xs font-medium text-text-secondary">
            {t("rams.system.mission", {
              unit:
                model.time_basis === "operating"
                  ? t("rams.units.opHours")
                  : t("rams.units.calHours"),
            })}
            <Input
              className="h-10 w-36"
              inputMode="decimal"
              value={mission}
              aria-invalid={mission !== "" && !missionOk}
              onChange={(e) => {
                setMission(e.target.value);
                setDirty(true);
              }}
            />
          </label>
          <Button
            variant="outline"
            className="h-10"
            onClick={() => {
              setMission(String(HOURS_PER_YEAR));
              setDirty(true);
            }}
          >
            {t("rams.system.oneYear", { hours: formatNumber(HOURS_PER_YEAR) })}
          </Button>
          {canEdit ? (
            <Button
              className={mfLayout.primaryAction}
              disabled={
                !missionOk || saving || !dirty || incomplete || (rbd == null && fta == null)
              }
              title={incomplete ? t("rams.system.saveIncomplete") : undefined}
              onClick={() => void save()}
            >
              <Save className="h-4 w-4" aria-hidden />
              {t("rams.system.save")}
            </Button>
          ) : null}
        </div>
      </div>
      {!missionOk && tab !== "advanced" ? (
        <p className="text-sm text-text-muted">{t("rams.system.missionNeeded")}</p>
      ) : null}
      {tab === "rbd" ? (
        rbd ? (
          <SystemRbdEditor
            spec={rbd}
            onChange={(s) => {
              setRbd(s);
              setDirty(true);
            }}
            selected={rbdSel}
            onSelect={setRbdSel}
            result={missionOk ? rbdResult : null}
            equipment={model.equipment}
            fit={fit}
            editable={canEdit}
          />
        ) : (
          <EmptyState
            title={t("rams.system.noRbdTitle")}
            description={t("rams.system.noRbdBody")}
            {...(canEdit
              ? {
                  actionLabel: t("rams.system.startFromEquipment"),
                  onAction: startRbdFromEquipment,
                }
              : {})}
          />
        )
      ) : null}
      {tab === "rbd" && !rbd && canEdit ? (
        <div className="flex justify-center">
          <Button variant="ghost" className="h-10" onClick={startEmptyRbd}>
            {t("rams.system.startEmpty")}
          </Button>
        </div>
      ) : null}
      {tab === "fta" ? (
        fta ? (
          <SystemFtaEditor
            spec={fta}
            onChange={(s) => {
              setFta(s);
              setDirty(true);
            }}
            selected={ftaSel}
            onSelect={setFtaSel}
            result={missionOk ? ftaResult : null}
            equipment={model.equipment}
            fit={fit}
            editable={canEdit}
          />
        ) : (
          <EmptyState
            title={t("rams.system.noFtaTitle")}
            description={t("rams.system.noFtaBody")}
            {...(canEdit ? { actionLabel: t("rams.system.startFta"), onAction: startFta } : {})}
          />
        )
      ) : null}
      {tab === "advanced" ? (
        <AdvancedTools
          studyId={study.id}
          rbd={rbd ? cleanRbd(rbd) : null}
          missionHours={missionOk ? missionN : null}
          equipment={model.equipment}
        />
      ) : null}
    </div>
  );
}
