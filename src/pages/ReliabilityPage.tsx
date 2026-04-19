import { listen } from "@tauri-apps/api/event";
import * as htmlToImage from "html-to-image";
import { FlaskConical, LineChart } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Link, useLocation } from "react-router-dom";
import { Bar, BarChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";

import { RamMethodAssumptions } from "@/components/RamMethodAssumptions";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { mfChart } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import { listPmPlans } from "@/services/pm-service";
import {
  deactivateFailureCode,
  dismissRamDataQualityIssue,
  getRamEquipmentQualityBadge,
  listEquipmentMissingExposure90d,
  listFailureCodes,
  listFailureEvents,
  listFailureHierarchies,
  listRamDataQualityIssues,
  cancelComputationJob,
  createEventTreeModel,
  createMarkovModel,
  createMcModel,
  createRamExpertSignOff,
  createFmecaAnalysis,
  createFtaModel,
  createRbdModel,
  createRcmStudy,
  deleteEventTreeModel,
  deleteMarkovModel,
  deleteMcModel,
  deleteRamExpertSignOff,
  deleteFmecaAnalysis,
  deleteFtaModel,
  deleteRbdModel,
  deleteFmecaItem,
  deleteRcmDecision,
  deleteRcmStudy,
  evaluateEventTreeModel,
  evaluateMarkovModel,
  evaluateMcModel,
  evaluateFtaModel,
  evaluateRbdModel,
  evaluateReliabilityAnalysisInput,
  getReliabilityKpiSnapshot,
  getRamAdvancedGuardrails,
  listEventTreeModels,
  listMarkovModels,
  listMcModels,
  listRamExpertSignOffs,
  listFmecaAnalyses,
  listFmecaItems,
  listFtaModels,
  listRbdModels,
  listReliabilityKpiSnapshots,
  listRcmDecisions,
  listRcmStudies,
  listRuntimeExposureLogs,
  listWosMissingFailureMode,
  refreshReliabilityKpiSnapshot,
  signRamExpertReview,
  runWeibullFit,
  submitReliabilityKpiComputationJob,
  upsertFailureCode,
  setRamAdvancedGuardrails,
  updateEventTreeModel,
  updateMarkovModel,
  updateMcModel,
  upsertFailureHierarchy,
  upsertFmecaItem,
  upsertRcmDecision,
  upsertRuntimeExposureLog,
  updateFtaModel,
  updateRbdModel,
} from "@/services/reliability-service";
import type {
  EquipmentMissingExposureRow,
  EventTreeModel,
  FailureCode,
  MarkovModel,
  McModel,
  RamAdvancedGuardrailFlags,
  FailureEvent,
  FailureHierarchy,
  FmecaAnalysis,
  FmecaItem,
  FtaModel,
  RamDataQualityIssue,
  RamEquipmentQualityBadge,
  ComputationJobProgressEvent,
  ReliabilityAnalysisInputEvaluation,
  ReliabilityKpiSnapshot,
  RbdModel,
  RcmDecision,
  RcmStudy,
  RuntimeExposureLog,
  WeibullFitRecord,
  WoMissingFailureModeRow,
  PmPlan,
  RamExpertSignOff,
} from "@shared/ipc-types";

const DEFAULT_FTA_GRAPH = `{"spec_version":1,"top_id":"top","nodes":{"top":{"kind":"or","inputs":["a","b"]},"a":{"kind":"basic","p":0.01},"b":{"kind":"basic","p":0.02}}}`;
const DEFAULT_RBD_GRAPH = `{"spec_version":1,"root_id":"root","nodes":{"root":{"kind":"series","children":["x","y"]},"x":{"kind":"block","r":0.99},"y":{"kind":"block","r":0.95}}}`;
const DEFAULT_ETA_GRAPH = `{"spec_version":1,"root_id":"s","nodes":{"s":{"kind":"split","label":"Init","branches":[{"target":"o1","p":0.3},{"target":"o2","p":0.7}]},"o1":{"kind":"outcome","label":"A"},"o2":{"kind":"outcome","label":"B"}}}`;
const DEFAULT_MC_GRAPH = `{"spec_version":1,"kind":"mc_sample","distribution":{"type":"uniform","low":0,"high":1}}`;
const DEFAULT_MARKOV_GRAPH = `{"spec_version":1,"kind":"discrete","states":["A","B"],"matrix":[[0.9,0.1],[0.4,0.6]]}`;

type AnalysisInputSpecParsed = {
  spec_version?: number;
  gates?: { exposure_hours_positive?: boolean; min_eligible_events_met?: boolean };
  analysis_ready?: boolean;
};

function KpiAnalysisInputGates({ evalRow }: { evalRow: ReliabilityAnalysisInputEvaluation }) {
  let spec: AnalysisInputSpecParsed = {};
  try {
    spec = JSON.parse(evalRow.analysis_input_spec_json) as AnalysisInputSpecParsed;
  } catch {
    spec = {};
  }
  const ready = spec.analysis_ready === true;
  const g = spec.gates;
  return (
    <div
      className={`mb-3 rounded border p-2 text-xs ${
        ready ? "border-green-600/40 bg-green-950/20" : "border-amber-600/50 bg-amber-950/20"
      }`}
    >
      <p className="mb-1 font-medium text-fg-1">Qualification des entrées (avant analyse KPI)</p>
      <ul className="mb-1 list-inside list-disc text-fg-2">
        <li>
          Exposition (h) sur la période : {evalRow.exposure_hours.toFixed(2)} —{" "}
          {g?.exposure_hours_positive ? "OK" : "insuffisant ou nul"}
        </li>
        <li>
          Événements éligibles : {evalRow.eligible_event_count} (seuil min. {evalRow.min_sample_n})
          — {g?.min_eligible_events_met ? "OK" : "non atteint"}
        </li>
        <li>
          Jeu de données prêt pour analyse : <strong>{ready ? "oui" : "non"}</strong>
        </li>
      </ul>
      <p className="font-mono text-[10px] text-fg-2">
        SHA-256 (canonical) : {evalRow.analysis_dataset_hash_sha256}
      </p>
    </div>
  );
}

type PlotPayloadParsed = {
  spec_version?: number;
  dataset_hash_sha256?: string;
  bar_chart?: { name: string; value: number }[];
};

function KpiSnapshotPlotDetail({
  snapshot,
  onReopenFilters,
}: {
  snapshot: ReliabilityKpiSnapshot;
  onReopenFilters: () => void;
}) {
  const chartRef = useRef<HTMLDivElement>(null);
  const parsed = useMemo((): PlotPayloadParsed | null => {
    try {
      return JSON.parse(snapshot.plot_payload_json) as PlotPayloadParsed;
    } catch {
      return null;
    }
  }, [snapshot.plot_payload_json]);
  const data = parsed?.bar_chart?.filter((b) => typeof b.value === "number") ?? [];

  const downloadDataUrl = (dataUrl: string, filename: string) => {
    const a = document.createElement("a");
    a.href = dataUrl;
    a.download = filename;
    a.click();
  };

  const onExportPng = async () => {
    if (!chartRef.current || data.length === 0) {
      return;
    }
    const dataUrl = await htmlToImage.toPng(chartRef.current, {
      cacheBust: true,
      backgroundColor: mfChart.exportPngBackground,
    });
    downloadDataUrl(dataUrl, `kpi-snapshot-${snapshot.id}.png`);
  };

  const onExportSvg = async () => {
    if (!chartRef.current || data.length === 0) {
      return;
    }
    const svg = await htmlToImage.toSvg(chartRef.current, { cacheBust: true });
    const blob = new Blob([svg], { type: "image/svg+xml;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `kpi-snapshot-${snapshot.id}.svg`;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div className="mt-4 rounded border border-border-1 bg-bg-0 p-3">
      <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
        <p className="text-xs font-medium text-fg-1">
          Instantané #{snapshot.id}
          {snapshot.equipment_id != null ? ` — équip. ${snapshot.equipment_id}` : ""}
        </p>
        <div className="flex flex-wrap gap-2">
          <button
            type="button"
            className="rounded border border-border-1 px-2 py-1 text-xs text-fg-2 hover:text-fg-1"
            onClick={onReopenFilters}
          >
            Reprendre les filtres
          </button>
          <button
            type="button"
            className="rounded border border-border-1 px-2 py-1 text-xs text-fg-2 hover:text-fg-1 disabled:opacity-40"
            disabled={data.length === 0}
            onClick={() => void onExportPng()}
          >
            Export PNG
          </button>
          <button
            type="button"
            className="rounded border border-border-1 px-2 py-1 text-xs text-fg-2 hover:text-fg-1 disabled:opacity-40"
            disabled={data.length === 0}
            onClick={() => void onExportSvg()}
          >
            Export SVG
          </button>
        </div>
      </div>
      <p className="mb-2 font-mono text-[10px] text-fg-2">
        SHA-256 (plot) : {parsed?.dataset_hash_sha256 ?? snapshot.analysis_dataset_hash_sha256}
      </p>
      <div
        ref={chartRef}
        className="rounded border border-border-1/60 bg-bg-1 p-2"
        style={{ minHeight: 240 }}
      >
        {data.length === 0 ? (
          <p className="py-8 text-center text-xs text-fg-2">
            Aucune donnée graphique dans ce snapshot (recalculez le KPI pour générer le payload).
          </p>
        ) : (
          <ResponsiveContainer width="100%" height={240}>
            <BarChart data={data} margin={{ top: 8, right: 8, left: 0, bottom: 4 }}>
              <CartesianGrid strokeDasharray="3 3" stroke={mfChart.gridStroke} />
              <XAxis dataKey="name" tick={{ fontSize: 10, fill: mfChart.axisTickFill }} />
              <YAxis tick={{ fontSize: 10, fill: mfChart.axisTickFill }} />
              <Tooltip
                contentStyle={{
                  backgroundColor: mfChart.tooltipBg,
                  border: `1px solid ${mfChart.tooltipBorder}`,
                  fontSize: 11,
                }}
              />
              <Bar dataKey="value" fill={mfChart.barFill} radius={[2, 2, 0, 0]} />
            </BarChart>
          </ResponsiveContainer>
        )}
      </div>
    </div>
  );
}

export function ReliabilityPage(props: { embedded?: boolean; equipmentId?: number | null } = {}) {
  const { embedded = false, equipmentId = null } = props;
  const equipSeed = embedded && equipmentId != null ? String(equipmentId) : "1";
  const location = useLocation();
  const [hierarchies, setHierarchies] = useState<FailureHierarchy[]>([]);
  const [codes, setCodes] = useState<FailureCode[]>([]);
  const [selectedHid, setSelectedHid] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [hName, setHName] = useState("");
  const [hScope, setHScope] = useState("{}");
  const [hVer, setHVer] = useState("1");

  const [cCode, setCCode] = useState("");
  const [cLabel, setCLabel] = useState("");
  const [cType, setCType] = useState("mode");

  const [failureEvents, setFailureEvents] = useState<FailureEvent[]>([]);
  const [feEquipFilter, setFeEquipFilter] = useState("");

  const [runtimeLogs, setRuntimeLogs] = useState<RuntimeExposureLog[]>([]);
  const [reEquipFilter, setReEquipFilter] = useState("");
  const [reEquipId, setReEquipId] = useState(equipSeed);
  const [reValue, setReValue] = useState("168");
  const [reRecordedAt, setReRecordedAt] = useState(() => new Date().toISOString());
  const [reExposureType, setReExposureType] = useState("hours");
  const [reSourceType, setReSourceType] = useState("manual");

  const [kpiRows, setKpiRows] = useState<ReliabilityKpiSnapshot[]>([]);
  const [selectedKpiSnapshot, setSelectedKpiSnapshot] = useState<ReliabilityKpiSnapshot | null>(
    null,
  );
  const [kpiEquipFilter, setKpiEquipFilter] = useState("");
  const [kpiEquip, setKpiEquip] = useState(equipSeed);
  const [kpiStart, setKpiStart] = useState(() => {
    const d = new Date();
    d.setUTCDate(1);
    d.setUTCHours(0, 0, 0, 0);
    return d.toISOString();
  });
  const [kpiEnd, setKpiEnd] = useState(() => new Date().toISOString());
  const [kpiInputEval, setKpiInputEval] = useState<ReliabilityAnalysisInputEvaluation | null>(null);
  const [kpiBgJobId, setKpiBgJobId] = useState<number | null>(null);
  const [kpiJobProgress, setKpiJobProgress] = useState<ComputationJobProgressEvent | null>(null);

  const [dqIssues, setDqIssues] = useState<RamDataQualityIssue[]>([]);
  const [dqEquipFilter, setDqEquipFilter] = useState("");
  const [dqWos, setDqWos] = useState<WoMissingFailureModeRow[]>([]);
  const [dqExp, setDqExp] = useState<EquipmentMissingExposureRow[]>([]);
  const [dqBadgeEquip, setDqBadgeEquip] = useState(equipSeed);
  const [dqBadge, setDqBadge] = useState<RamEquipmentQualityBadge | null>(null);

  const [wbEquip, setWbEquip] = useState(equipSeed);
  const [wbStart, setWbStart] = useState("");
  const [wbEnd, setWbEnd] = useState("");
  const [wbResult, setWbResult] = useState<WeibullFitRecord | null>(null);

  const [fmEquipFilter, setFmEquipFilter] = useState("");
  const [fmAnalyses, setFmAnalyses] = useState<FmecaAnalysis[]>([]);
  const [fmSelectedId, setFmSelectedId] = useState<number | null>(null);
  const [fmNewEquip, setFmNewEquip] = useState(equipSeed);
  const [fmNewTitle, setFmNewTitle] = useState("");
  const [fmNewBoundary, setFmNewBoundary] = useState("");
  const [fmItems, setFmItems] = useState<FmecaItem[]>([]);
  const [fmItemId, setFmItemId] = useState<number | null>(null);
  const [fmItemRv, setFmItemRv] = useState<number | null>(null);
  const [fmFf, setFmFf] = useState("");
  const [fmFe, setFmFe] = useState("");
  const [fmS, setFmS] = useState("5");
  const [fmO, setFmO] = useState("5");
  const [fmD, setFmD] = useState("5");
  const [fmRa, setFmRa] = useState("");
  const [fmCc, setFmCc] = useState("");
  const [fmPmId, setFmPmId] = useState("");
  const [fmFmId, setFmFmId] = useState("");
  const [fmRevRpn, setFmRevRpn] = useState("");

  const [rcmEquipFilter, setRcmEquipFilter] = useState("");
  const [rcmStudies, setRcmStudies] = useState<RcmStudy[]>([]);
  const [rcmSelectedId, setRcmSelectedId] = useState<number | null>(null);
  const [rcmNewEquip, setRcmNewEquip] = useState(equipSeed);
  const [rcmNewTitle, setRcmNewTitle] = useState("");
  const [rcmDecisions, setRcmDecisions] = useState<RcmDecision[]>([]);
  const [rcmDecId, setRcmDecId] = useState<number | null>(null);
  const [rcmDecRv, setRcmDecRv] = useState<number | null>(null);
  const [rcmFnDesc, setRcmFnDesc] = useState("");
  const [rcmFf, setRcmFf] = useState("");
  const [rcmCc, setRcmCc] = useState("");
  const [rcmTactic, setRcmTactic] = useState("time_based");
  const [rcmJust, setRcmJust] = useState("");
  const [rcmPmId, setRcmPmId] = useState("");
  const [rcmReview, setRcmReview] = useState("");
  const [rcmFmId, setRcmFmId] = useState("");

  const [pmPlans, setPmPlans] = useState<PmPlan[]>([]);

  const [gfFtaFilter, setGfFtaFilter] = useState("");
  const [gfFtaRows, setGfFtaRows] = useState<FtaModel[]>([]);
  const [gfFtaEq, setGfFtaEq] = useState(equipSeed);
  const [gfFtaTitle, setGfFtaTitle] = useState("FTA");
  const [gfFtaJson, setGfFtaJson] = useState(DEFAULT_FTA_GRAPH);
  const [gfFtaSel, setGfFtaSel] = useState<FtaModel | null>(null);

  const [gfRbdFilter, setGfRbdFilter] = useState("");
  const [gfRbdRows, setGfRbdRows] = useState<RbdModel[]>([]);
  const [gfRbdEq, setGfRbdEq] = useState(equipSeed);
  const [gfRbdTitle, setGfRbdTitle] = useState("RBD");
  const [gfRbdJson, setGfRbdJson] = useState(DEFAULT_RBD_GRAPH);
  const [gfRbdSel, setGfRbdSel] = useState<RbdModel | null>(null);

  const [gfEtaFilter, setGfEtaFilter] = useState("");
  const [gfEtaRows, setGfEtaRows] = useState<EventTreeModel[]>([]);
  const [gfEtaEq, setGfEtaEq] = useState(equipSeed);
  const [gfEtaTitle, setGfEtaTitle] = useState("ETA");
  const [gfEtaJson, setGfEtaJson] = useState(DEFAULT_ETA_GRAPH);
  const [gfEtaSel, setGfEtaSel] = useState<EventTreeModel | null>(null);

  const [grMcEnabled, setGrMcEnabled] = useState(true);
  const [grMkEnabled, setGrMkEnabled] = useState(true);
  const [grMcMaxTrials, setGrMcMaxTrials] = useState("50000");
  const [grMkMaxStates, setGrMkMaxStates] = useState("32");
  const [mcFilter, setMcFilter] = useState("");
  const [mcRows, setMcRows] = useState<McModel[]>([]);
  const [mcEq, setMcEq] = useState(equipSeed);
  const [mcTitle, setMcTitle] = useState("MC");
  const [mcJson, setMcJson] = useState(DEFAULT_MC_GRAPH);
  const [mcTrials, setMcTrials] = useState("5000");
  const [mcSeed, setMcSeed] = useState("42");
  const [mcSel, setMcSel] = useState<McModel | null>(null);

  const [mkFilter, setMkFilter] = useState("");
  const [mkRows, setMkRows] = useState<MarkovModel[]>([]);
  const [mkEq, setMkEq] = useState(equipSeed);
  const [mkTitle, setMkTitle] = useState("Markov");
  const [mkJson, setMkJson] = useState(DEFAULT_MARKOV_GRAPH);
  const [mkSel, setMkSel] = useState<MarkovModel | null>(null);

  const [soFilter, setSoFilter] = useState("");
  const [soRows, setSoRows] = useState<RamExpertSignOff[]>([]);
  const [soEq, setSoEq] = useState(equipSeed);
  const [soMethod, setSoMethod] = useState("general");
  const [soTarget, setSoTarget] = useState("");
  const [soTitle, setSoTitle] = useState("");
  const [soNotes, setSoNotes] = useState("");
  const [soRole, setSoRole] = useState("");
  const [soReviewer, setSoReviewer] = useState("");
  const [soSel, setSoSel] = useState<RamExpertSignOff | null>(null);
  const [soSignName, setSoSignName] = useState("");

  type RamHubTab = "quality" | "failures" | "models" | "kpis";
  const [ramHubTab, setRamHubTab] = useState<RamHubTab>("kpis");

  const [mcUniformLow, setMcUniformLow] = useState("0");
  const [mcUniformHigh, setMcUniformHigh] = useState("1");
  const [mkS0, setMkS0] = useState("A");
  const [mkS1, setMkS1] = useState("B");
  const [mk00, setMk00] = useState("0.9");
  const [mk01, setMk01] = useState("0.1");
  const [mk10, setMk10] = useState("0.4");
  const [mk11, setMk11] = useState("0.6");

  useEffect(() => {
    if (!embedded || equipmentId == null) {
      return;
    }
    const s = String(equipmentId);
    setReEquipId(s);
    setKpiEquip(s);
    setDqBadgeEquip(s);
    setWbEquip(s);
    setFmNewEquip(s);
    setRcmNewEquip(s);
    setGfFtaEq(s);
    setGfRbdEq(s);
    setGfEtaEq(s);
    setMcEq(s);
    setMkEq(s);
    setSoEq(s);
  }, [embedded, equipmentId]);

  const loadH = useCallback(async () => {
    setError(null);
    try {
      const h = await listFailureHierarchies();
      setHierarchies(h);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void loadH();
  }, [loadH]);

  useEffect(() => {
    if (location.hash !== "#ram-data-quality") {
      return;
    }
    setRamHubTab("quality");
    const el = document.getElementById("ram-data-quality");
    if (el) {
      queueMicrotask(() => el.scrollIntoView({ behavior: "smooth", block: "start" }));
    }
  }, [location.hash]);

  useEffect(() => {
    const first = hierarchies[0];
    if (first != null && selectedHid == null) {
      setSelectedHid(first.id);
    }
  }, [hierarchies, selectedHid]);

  const loadCodes = useCallback(async () => {
    if (selectedHid == null) {
      setCodes([]);
      return;
    }
    setError(null);
    try {
      const c = await listFailureCodes({ hierarchy_id: selectedHid, include_inactive: true });
      setCodes(c);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [selectedHid]);

  useEffect(() => {
    void loadCodes();
  }, [loadCodes]);

  const loadFailureEvents = useCallback(async () => {
    setError(null);
    try {
      const n = feEquipFilter.trim() === "" ? null : Number.parseInt(feEquipFilter.trim(), 10);
      const equipment_id = n != null && !Number.isNaN(n) ? n : null;
      const ev = await listFailureEvents({ equipment_id, limit: 200 });
      setFailureEvents(ev);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [feEquipFilter]);

  useEffect(() => {
    void loadFailureEvents();
  }, [loadFailureEvents]);

  const loadRuntimeLogs = useCallback(async () => {
    setError(null);
    try {
      const n = reEquipFilter.trim() === "" ? null : Number.parseInt(reEquipFilter.trim(), 10);
      const equipment_id = n != null && !Number.isNaN(n) ? n : null;
      const rows = await listRuntimeExposureLogs({ equipment_id, limit: 200 });
      setRuntimeLogs(rows);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [reEquipFilter]);

  useEffect(() => {
    void loadRuntimeLogs();
  }, [loadRuntimeLogs]);

  const loadKpiSnapshots = useCallback(async () => {
    setError(null);
    try {
      const n = kpiEquipFilter.trim() === "" ? null : Number.parseInt(kpiEquipFilter.trim(), 10);
      const equipment_id = n != null && !Number.isNaN(n) ? n : null;
      const rows = await listReliabilityKpiSnapshots({ equipment_id, limit: 50 });
      setKpiRows(rows);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [kpiEquipFilter]);

  useEffect(() => {
    void loadKpiSnapshots();
  }, [loadKpiSnapshots]);

  const loadKpiInputEval = useCallback(async () => {
    const eq = Number.parseInt(kpiEquip.trim(), 10);
    if (Number.isNaN(eq)) {
      setKpiInputEval(null);
      return;
    }
    setError(null);
    try {
      const ev = await evaluateReliabilityAnalysisInput({
        equipment_id: eq,
        period_start: kpiStart.trim(),
        period_end: kpiEnd.trim(),
      });
      setKpiInputEval(ev);
    } catch (e) {
      setKpiInputEval(null);
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [kpiEquip, kpiStart, kpiEnd]);

  useEffect(() => {
    void loadKpiInputEval();
  }, [loadKpiInputEval]);

  const loadKpiSnapRef = useRef(loadKpiSnapshots);
  loadKpiSnapRef.current = loadKpiSnapshots;
  const loadKpiEvalRef = useRef(loadKpiInputEval);
  loadKpiEvalRef.current = loadKpiInputEval;

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen<ComputationJobProgressEvent>("computation-job-progress", (ev) => {
      const p = ev.payload;
      setKpiJobProgress(p);
      if (p.status === "completed" || p.status === "failed" || p.status === "cancelled") {
        setKpiBgJobId(null);
        void loadKpiSnapRef.current();
        void loadKpiEvalRef.current();
      }
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, []);

  const loadDqIssues = useCallback(async () => {
    setError(null);
    try {
      const n = dqEquipFilter.trim() === "" ? null : Number.parseInt(dqEquipFilter.trim(), 10);
      const equipment_id = n != null && !Number.isNaN(n) ? n : null;
      const rows = await listRamDataQualityIssues({ equipment_id });
      setDqIssues(rows);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [dqEquipFilter]);

  useEffect(() => {
    void loadDqIssues();
  }, [loadDqIssues]);

  const loadDqDrill = useCallback(async () => {
    setError(null);
    try {
      const n = dqEquipFilter.trim() === "" ? null : Number.parseInt(dqEquipFilter.trim(), 10);
      const equipment_id = n != null && !Number.isNaN(n) ? n : null;
      const [wos, exp] = await Promise.all([
        listWosMissingFailureMode(equipment_id, 100),
        listEquipmentMissingExposure90d(200),
      ]);
      setDqWos(wos);
      setDqExp(exp);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [dqEquipFilter]);

  const loadDqBadge = useCallback(async () => {
    setError(null);
    const eq = Number.parseInt(dqBadgeEquip.trim(), 10);
    if (Number.isNaN(eq)) {
      setDqBadge(null);
      return;
    }
    try {
      const b = await getRamEquipmentQualityBadge(eq);
      setDqBadge(b);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [dqBadgeEquip]);

  useEffect(() => {
    void loadDqBadge();
  }, [loadDqBadge]);

  const onDismissDq = async (row: RamDataQualityIssue) => {
    setError(null);
    try {
      await dismissRamDataQualityIssue({
        equipment_id: row.equipment_id,
        issue_code: row.issue_code,
      });
      await loadDqIssues();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onAddRuntimeExposure = async () => {
    setError(null);
    const eq = Number.parseInt(reEquipId.trim(), 10);
    const val = Number.parseFloat(reValue);
    if (Number.isNaN(eq) || Number.isNaN(val)) {
      setError("Équipement et valeur numériques requis.");
      return;
    }
    try {
      await upsertRuntimeExposureLog({
        equipment_id: eq,
        exposure_type: reExposureType,
        value: val,
        recorded_at: reRecordedAt.trim() || new Date().toISOString(),
        source_type: reSourceType,
      });
      await loadRuntimeLogs();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onSelectKpiRow = async (id: number) => {
    setError(null);
    try {
      const s = await getReliabilityKpiSnapshot(id);
      setSelectedKpiSnapshot(s);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onReopenKpiFilters = () => {
    if (selectedKpiSnapshot?.equipment_id == null) {
      return;
    }
    setKpiEquip(String(selectedKpiSnapshot.equipment_id));
    setKpiStart(selectedKpiSnapshot.period_start);
    setKpiEnd(selectedKpiSnapshot.period_end);
  };

  const onRefreshKpi = async () => {
    setError(null);
    const eq = Number.parseInt(kpiEquip.trim(), 10);
    if (Number.isNaN(eq)) {
      setError("Équipement KPI requis.");
      return;
    }
    try {
      const snap = await refreshReliabilityKpiSnapshot({
        equipment_id: eq,
        period_start: kpiStart.trim(),
        period_end: kpiEnd.trim(),
      });
      setSelectedKpiSnapshot(snap);
      await loadKpiSnapshots();
      await loadKpiInputEval();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onKpiBackgroundJob = async () => {
    setError(null);
    const eq = Number.parseInt(kpiEquip.trim(), 10);
    if (Number.isNaN(eq)) {
      setError("Équipement KPI requis.");
      return;
    }
    try {
      setKpiJobProgress(null);
      const id = await submitReliabilityKpiComputationJob({
        equipment_id: eq,
        period_start: kpiStart.trim(),
        period_end: kpiEnd.trim(),
      });
      setKpiBgJobId(id);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onCancelKpiJob = async () => {
    if (kpiBgJobId == null) {
      return;
    }
    setError(null);
    try {
      await cancelComputationJob(kpiBgJobId);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onAddHierarchy = async () => {
    setError(null);
    try {
      await upsertFailureHierarchy({
        name: hName || "Hierarchy",
        asset_scope_json: hScope || "{}",
        version_no: Number.parseInt(hVer, 10) || 1,
        is_active: true,
      });
      setHName("");
      await loadH();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onAddCode = async () => {
    if (selectedHid == null) {
      return;
    }
    setError(null);
    try {
      await upsertFailureCode({
        hierarchy_id: selectedHid,
        code: cCode.trim(),
        label: cLabel.trim(),
        code_type: cType,
        is_active: true,
      });
      setCCode("");
      setCLabel("");
      await loadCodes();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onDeactivate = async (row: FailureCode) => {
    setError(null);
    try {
      await deactivateFailureCode({ id: row.id, expected_row_version: row.row_version });
      await loadCodes();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const loadPmPlansList = useCallback(async () => {
    setError(null);
    try {
      const p = await listPmPlans({});
      setPmPlans(p);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void loadPmPlansList();
  }, [loadPmPlansList]);

  const loadFmecaAnalysesList = useCallback(async () => {
    setError(null);
    try {
      const n = fmEquipFilter.trim() === "" ? null : Number.parseInt(fmEquipFilter.trim(), 10);
      const equipment_id = n != null && !Number.isNaN(n) ? n : null;
      const rows = await listFmecaAnalyses({ equipment_id, limit: 100 });
      setFmAnalyses(rows);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [fmEquipFilter]);

  const loadFmecaItemsList = useCallback(async () => {
    if (fmSelectedId == null) {
      setFmItems([]);
      return;
    }
    setError(null);
    try {
      const rows = await listFmecaItems(fmSelectedId);
      setFmItems(rows);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [fmSelectedId]);

  useEffect(() => {
    void loadFmecaItemsList();
  }, [loadFmecaItemsList]);

  const loadRcmStudiesList = useCallback(async () => {
    setError(null);
    try {
      const n = rcmEquipFilter.trim() === "" ? null : Number.parseInt(rcmEquipFilter.trim(), 10);
      const equipment_id = n != null && !Number.isNaN(n) ? n : null;
      const rows = await listRcmStudies({ equipment_id, limit: 100 });
      setRcmStudies(rows);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [rcmEquipFilter]);

  const loadRcmDecisionsList = useCallback(async () => {
    if (rcmSelectedId == null) {
      setRcmDecisions([]);
      return;
    }
    setError(null);
    try {
      const rows = await listRcmDecisions(rcmSelectedId);
      setRcmDecisions(rows);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [rcmSelectedId]);

  useEffect(() => {
    void loadRcmDecisionsList();
  }, [loadRcmDecisionsList]);

  useEffect(() => {
    void loadFmecaAnalysesList();
  }, [loadFmecaAnalysesList]);

  useEffect(() => {
    void loadRcmStudiesList();
  }, [loadRcmStudiesList]);

  const loadGfFta = useCallback(async () => {
    setError(null);
    try {
      const n = gfFtaFilter.trim() === "" ? null : Number.parseInt(gfFtaFilter.trim(), 10);
      const equipment_id = n != null && !Number.isNaN(n) ? n : null;
      const rows = await listFtaModels({ equipment_id, limit: 50 });
      setGfFtaRows(rows);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [gfFtaFilter]);

  const loadGfRbd = useCallback(async () => {
    setError(null);
    try {
      const n = gfRbdFilter.trim() === "" ? null : Number.parseInt(gfRbdFilter.trim(), 10);
      const equipment_id = n != null && !Number.isNaN(n) ? n : null;
      const rows = await listRbdModels({ equipment_id, limit: 50 });
      setGfRbdRows(rows);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [gfRbdFilter]);

  const loadGfEta = useCallback(async () => {
    setError(null);
    try {
      const n = gfEtaFilter.trim() === "" ? null : Number.parseInt(gfEtaFilter.trim(), 10);
      const equipment_id = n != null && !Number.isNaN(n) ? n : null;
      const rows = await listEventTreeModels({ equipment_id, limit: 50 });
      setGfEtaRows(rows);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [gfEtaFilter]);

  useEffect(() => {
    void loadGfFta();
  }, [loadGfFta]);

  useEffect(() => {
    void loadGfRbd();
  }, [loadGfRbd]);

  useEffect(() => {
    void loadGfEta();
  }, [loadGfEta]);

  const loadGr = useCallback(async () => {
    setError(null);
    try {
      const g = await getRamAdvancedGuardrails();
      setGrMcEnabled(g.monte_carlo_enabled);
      setGrMkEnabled(g.markov_enabled);
      setGrMcMaxTrials(String(g.mc_max_trials));
      setGrMkMaxStates(String(g.markov_max_states));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void loadGr();
  }, [loadGr]);

  const applyMcUniformFromGraphJson = useCallback((json: string) => {
    setMcJson(json);
    try {
      const o = JSON.parse(json) as {
        distribution?: { type?: string; low?: number; high?: number };
      };
      if (o.distribution?.type === "uniform") {
        setMcUniformLow(String(o.distribution.low ?? 0));
        setMcUniformHigh(String(o.distribution.high ?? 1));
      }
    } catch {
      /* keep typed fields */
    }
  }, []);

  const applyMkDiscreteFromGraphJson = useCallback((json: string) => {
    setMkJson(json);
    try {
      const o = JSON.parse(json) as {
        kind?: string;
        states?: string[];
        matrix?: number[][];
      };
      if (
        o.kind === "discrete" &&
        Array.isArray(o.states) &&
        o.states.length >= 2 &&
        Array.isArray(o.matrix) &&
        o.matrix.length >= 2
      ) {
        setMkS0(o.states[0] ?? "A");
        setMkS1(o.states[1] ?? "B");
        const m = o.matrix;
        setMk00(String(m[0]?.[0] ?? 0));
        setMk01(String(m[0]?.[1] ?? 0));
        setMk10(String(m[1]?.[0] ?? 0));
        setMk11(String(m[1]?.[1] ?? 0));
      }
    } catch {
      /* noop */
    }
  }, []);

  const patchMcUniform = useCallback((low: string, high: string) => {
    setMcUniformLow(low);
    setMcUniformHigh(high);
    const l = Number.parseFloat(low);
    const h = Number.parseFloat(high);
    if (!Number.isNaN(l) && !Number.isNaN(h)) {
      setMcJson(
        JSON.stringify({
          spec_version: 1,
          kind: "mc_sample",
          distribution: { type: "uniform", low: l, high: h },
        }),
      );
    }
  }, []);

  const patchMkDiscrete2 = useCallback(
    (p: { s0: string; s1: string; m00: string; m01: string; m10: string; m11: string }) => {
      setMkS0(p.s0);
      setMkS1(p.s1);
      setMk00(p.m00);
      setMk01(p.m01);
      setMk10(p.m10);
      setMk11(p.m11);
      const a = Number.parseFloat(p.m00);
      const b = Number.parseFloat(p.m01);
      const c = Number.parseFloat(p.m10);
      const d = Number.parseFloat(p.m11);
      if ([a, b, c, d].some((x) => Number.isNaN(x))) {
        return;
      }
      setMkJson(
        JSON.stringify({
          spec_version: 1,
          kind: "discrete",
          states: [p.s0, p.s1],
          matrix: [
            [a, b],
            [c, d],
          ],
        }),
      );
    },
    [],
  );

  const loadMcList = useCallback(async () => {
    setError(null);
    try {
      const n = mcFilter.trim() === "" ? null : Number.parseInt(mcFilter.trim(), 10);
      const equipment_id = n != null && !Number.isNaN(n) ? n : null;
      const rows = await listMcModels({ equipment_id, limit: 50 });
      setMcRows(rows);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [mcFilter]);

  useEffect(() => {
    void loadMcList();
  }, [loadMcList]);

  const loadMkList = useCallback(async () => {
    setError(null);
    try {
      const n = mkFilter.trim() === "" ? null : Number.parseInt(mkFilter.trim(), 10);
      const equipment_id = n != null && !Number.isNaN(n) ? n : null;
      const rows = await listMarkovModels({ equipment_id, limit: 50 });
      setMkRows(rows);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [mkFilter]);

  useEffect(() => {
    void loadMkList();
  }, [loadMkList]);

  const loadSoList = useCallback(async () => {
    setError(null);
    try {
      const n = soFilter.trim() === "" ? null : Number.parseInt(soFilter.trim(), 10);
      const equipment_id = n != null && !Number.isNaN(n) ? n : null;
      const rows = await listRamExpertSignOffs({ equipment_id, limit: 80 });
      setSoRows(rows);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [soFilter]);

  useEffect(() => {
    void loadSoList();
  }, [loadSoList]);

  const fmPreviewRpn = useMemo(() => {
    const s = Number.parseInt(fmS, 10);
    const o = Number.parseInt(fmO, 10);
    const d = Number.parseInt(fmD, 10);
    if ([s, o, d].some((x) => Number.isNaN(x))) {
      return null;
    }
    return s * o * d;
  }, [fmS, fmO, fmD]);

  const onRunWeibull = async () => {
    setError(null);
    const eq = Number.parseInt(wbEquip.trim(), 10);
    if (Number.isNaN(eq)) {
      setError("Équipement Weibull requis.");
      return;
    }
    const ps = wbStart.trim() || null;
    const pe = wbEnd.trim() || null;
    try {
      const r = await runWeibullFit({
        equipment_id: eq,
        period_start: ps,
        period_end: pe,
      });
      setWbResult(r);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onCreateFmeca = async () => {
    const eq = Number.parseInt(fmNewEquip.trim(), 10);
    if (Number.isNaN(eq) || fmNewTitle.trim() === "") {
      setError("FMECA : équipement et titre requis.");
      return;
    }
    setError(null);
    try {
      await createFmecaAnalysis({
        equipment_id: eq,
        title: fmNewTitle.trim(),
        boundary_definition: fmNewBoundary.trim() || null,
        status: "draft",
      });
      setFmNewTitle("");
      setFmNewBoundary("");
      await loadFmecaAnalysesList();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onDeleteFmeca = async (id: number) => {
    setError(null);
    try {
      await deleteFmecaAnalysis(id);
      if (fmSelectedId === id) {
        setFmSelectedId(null);
      }
      await loadFmecaAnalysesList();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onSaveFmecaItem = async () => {
    if (fmSelectedId == null) {
      return;
    }
    const s = Number.parseInt(fmS, 10);
    const o = Number.parseInt(fmO, 10);
    const d = Number.parseInt(fmD, 10);
    if ([s, o, d].some((x) => Number.isNaN(x))) {
      setError("S, O, D doivent être des entiers.");
      return;
    }
    if (s < 1 || s > 10 || o < 1 || o > 10 || d < 1 || d > 10) {
      setError("S, O, D doivent être entre 1 et 10.");
      return;
    }
    let failure_mode_id: number | null = null;
    if (fmFmId.trim() !== "") {
      const n = Number.parseInt(fmFmId, 10);
      if (Number.isNaN(n)) {
        setError("Mode de défaillance (id) invalide.");
        return;
      }
      failure_mode_id = n;
    }
    let linked_pm_plan_id: number | null = null;
    if (fmPmId.trim() !== "") {
      const n = Number.parseInt(fmPmId, 10);
      if (Number.isNaN(n)) {
        setError("Plan PM invalide.");
        return;
      }
      linked_pm_plan_id = n;
    }
    let revised_rpn: number | null = null;
    if (fmRevRpn.trim() !== "") {
      const n = Number.parseInt(fmRevRpn, 10);
      if (Number.isNaN(n)) {
        setError("RPN révisé invalide.");
        return;
      }
      revised_rpn = n;
    }
    setError(null);
    try {
      await upsertFmecaItem({
        id: fmItemId ?? null,
        analysis_id: fmSelectedId,
        expected_row_version: fmItemRv ?? null,
        functional_failure: fmFf.trim() || null,
        failure_effect: fmFe.trim() || null,
        severity: s,
        occurrence: o,
        detectability: d,
        recommended_action: fmRa.trim() || null,
        current_control: fmCc.trim() || null,
        failure_mode_id,
        linked_pm_plan_id,
        revised_rpn,
      });
      setFmItemId(null);
      setFmItemRv(null);
      setFmFf("");
      setFmFe("");
      setFmS("5");
      setFmO("5");
      setFmD("5");
      setFmRa("");
      setFmCc("");
      setFmPmId("");
      setFmFmId("");
      setFmRevRpn("");
      await loadFmecaItemsList();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onDeleteFmecaItemRow = async (row: FmecaItem) => {
    setError(null);
    try {
      await deleteFmecaItem(row.id);
      if (fmItemId === row.id) {
        setFmItemId(null);
        setFmItemRv(null);
      }
      await loadFmecaItemsList();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onCreateRcm = async () => {
    const eq = Number.parseInt(rcmNewEquip.trim(), 10);
    if (Number.isNaN(eq) || rcmNewTitle.trim() === "") {
      setError("RCM : équipement et titre requis.");
      return;
    }
    setError(null);
    try {
      await createRcmStudy({
        equipment_id: eq,
        title: rcmNewTitle.trim(),
        status: "draft",
      });
      setRcmNewTitle("");
      await loadRcmStudiesList();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onDeleteRcm = async (id: number) => {
    setError(null);
    try {
      await deleteRcmStudy(id);
      if (rcmSelectedId === id) {
        setRcmSelectedId(null);
      }
      await loadRcmStudiesList();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onSaveRcmDecision = async () => {
    if (rcmSelectedId == null) {
      return;
    }
    let failure_mode_id: number | null = null;
    if (rcmFmId.trim() !== "") {
      const n = Number.parseInt(rcmFmId, 10);
      if (Number.isNaN(n)) {
        setError("Mode de défaillance (id) invalide.");
        return;
      }
      failure_mode_id = n;
    }
    let linked_pm_plan_id: number | null = null;
    if (rcmPmId.trim() !== "") {
      const n = Number.parseInt(rcmPmId, 10);
      if (Number.isNaN(n)) {
        setError("Plan PM invalide.");
        return;
      }
      linked_pm_plan_id = n;
    }
    const review_due_at = rcmReview.trim() === "" ? null : rcmReview.trim();
    setError(null);
    try {
      await upsertRcmDecision({
        id: rcmDecId ?? null,
        study_id: rcmSelectedId,
        expected_row_version: rcmDecRv ?? null,
        function_description: rcmFnDesc.trim() || null,
        functional_failure: rcmFf.trim() || null,
        failure_mode_id,
        consequence_category: rcmCc.trim() || null,
        selected_tactic: rcmTactic,
        justification: rcmJust.trim() || null,
        review_due_at,
        linked_pm_plan_id,
      });
      setRcmDecId(null);
      setRcmDecRv(null);
      setRcmFnDesc("");
      setRcmFf("");
      setRcmCc("");
      setRcmTactic("time_based");
      setRcmJust("");
      setRcmPmId("");
      setRcmReview("");
      setRcmFmId("");
      await loadRcmDecisionsList();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onDeleteRcmDecisionRow = async (row: RcmDecision) => {
    setError(null);
    try {
      await deleteRcmDecision(row.id);
      if (rcmDecId === row.id) {
        setRcmDecId(null);
        setRcmDecRv(null);
      }
      await loadRcmDecisionsList();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onCreateGfFta = async () => {
    const eq = Number.parseInt(gfFtaEq.trim(), 10);
    if (Number.isNaN(eq) || gfFtaTitle.trim() === "") {
      setError("FTA : équipement et titre requis.");
      return;
    }
    setError(null);
    try {
      await createFtaModel({
        equipment_id: eq,
        title: gfFtaTitle.trim(),
        graph_json: gfFtaJson,
        status: "draft",
      });
      await loadGfFta();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onUpdateGfFta = async () => {
    if (gfFtaSel == null) {
      return;
    }
    setError(null);
    try {
      const m = await updateFtaModel({
        id: gfFtaSel.id,
        expected_row_version: gfFtaSel.row_version,
        graph_json: gfFtaJson,
      });
      setGfFtaSel(m);
      await loadGfFta();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onEvalGfFta = async () => {
    if (gfFtaSel == null) {
      return;
    }
    setError(null);
    try {
      const m = await evaluateFtaModel(gfFtaSel.id);
      setGfFtaSel(m);
      setGfFtaJson(m.graph_json);
      await loadGfFta();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onDeleteGfFta = async () => {
    if (gfFtaSel == null) {
      return;
    }
    setError(null);
    try {
      await deleteFtaModel(gfFtaSel.id);
      setGfFtaSel(null);
      setGfFtaJson(DEFAULT_FTA_GRAPH);
      await loadGfFta();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onCreateGfRbd = async () => {
    const eq = Number.parseInt(gfRbdEq.trim(), 10);
    if (Number.isNaN(eq) || gfRbdTitle.trim() === "") {
      setError("RBD : équipement et titre requis.");
      return;
    }
    setError(null);
    try {
      await createRbdModel({
        equipment_id: eq,
        title: gfRbdTitle.trim(),
        graph_json: gfRbdJson,
        status: "draft",
      });
      await loadGfRbd();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onUpdateGfRbd = async () => {
    if (gfRbdSel == null) {
      return;
    }
    setError(null);
    try {
      const m = await updateRbdModel({
        id: gfRbdSel.id,
        expected_row_version: gfRbdSel.row_version,
        graph_json: gfRbdJson,
      });
      setGfRbdSel(m);
      await loadGfRbd();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onEvalGfRbd = async () => {
    if (gfRbdSel == null) {
      return;
    }
    setError(null);
    try {
      const m = await evaluateRbdModel(gfRbdSel.id);
      setGfRbdSel(m);
      setGfRbdJson(m.graph_json);
      await loadGfRbd();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onDeleteGfRbd = async () => {
    if (gfRbdSel == null) {
      return;
    }
    setError(null);
    try {
      await deleteRbdModel(gfRbdSel.id);
      setGfRbdSel(null);
      setGfRbdJson(DEFAULT_RBD_GRAPH);
      await loadGfRbd();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onCreateGfEta = async () => {
    const eq = Number.parseInt(gfEtaEq.trim(), 10);
    if (Number.isNaN(eq) || gfEtaTitle.trim() === "") {
      setError("ETA : équipement et titre requis.");
      return;
    }
    setError(null);
    try {
      await createEventTreeModel({
        equipment_id: eq,
        title: gfEtaTitle.trim(),
        graph_json: gfEtaJson,
        status: "draft",
      });
      await loadGfEta();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onUpdateGfEta = async () => {
    if (gfEtaSel == null) {
      return;
    }
    setError(null);
    try {
      const m = await updateEventTreeModel({
        id: gfEtaSel.id,
        expected_row_version: gfEtaSel.row_version,
        graph_json: gfEtaJson,
      });
      setGfEtaSel(m);
      await loadGfEta();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onEvalGfEta = async () => {
    if (gfEtaSel == null) {
      return;
    }
    setError(null);
    try {
      const m = await evaluateEventTreeModel(gfEtaSel.id);
      setGfEtaSel(m);
      setGfEtaJson(m.graph_json);
      await loadGfEta();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onDeleteGfEta = async () => {
    if (gfEtaSel == null) {
      return;
    }
    setError(null);
    try {
      await deleteEventTreeModel(gfEtaSel.id);
      setGfEtaSel(null);
      setGfEtaJson(DEFAULT_ETA_GRAPH);
      await loadGfEta();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onSaveGr = async () => {
    setError(null);
    try {
      const mcTrialsN = Number.parseInt(grMcMaxTrials.trim(), 10);
      const mkStatesN = Number.parseInt(grMkMaxStates.trim(), 10);
      if (Number.isNaN(mcTrialsN) || Number.isNaN(mkStatesN)) {
        setError("Garde-fous : nombres invalides.");
        return;
      }
      const payload: RamAdvancedGuardrailFlags = {
        monte_carlo_enabled: grMcEnabled,
        markov_enabled: grMkEnabled,
        mc_max_trials: mcTrialsN,
        markov_max_states: mkStatesN,
      };
      await setRamAdvancedGuardrails(payload);
      await loadGr();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onCreateMc = async () => {
    const eq = Number.parseInt(mcEq.trim(), 10);
    const tr = Number.parseInt(mcTrials.trim(), 10);
    if (Number.isNaN(eq) || mcTitle.trim() === "" || Number.isNaN(tr)) {
      setError("MC : équipement, titre et essais requis.");
      return;
    }
    const seedValue = mcSeed.trim();
    const sd = seedValue === "" ? null : Number.parseInt(seedValue, 10);
    if (seedValue !== "" && (sd === null || Number.isNaN(sd))) {
      setError("Graine MC invalide.");
      return;
    }
    setError(null);
    try {
      await createMcModel({
        equipment_id: eq,
        title: mcTitle.trim(),
        graph_json: mcJson,
        trials: tr,
        seed: sd,
        status: "draft",
      });
      await loadMcList();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onUpdateMc = async () => {
    if (mcSel == null) {
      return;
    }
    const tr = Number.parseInt(mcTrials.trim(), 10);
    const sd = mcSeed.trim() === "" ? null : Number.parseInt(mcSeed.trim(), 10);
    if (Number.isNaN(tr) || (mcSeed.trim() !== "" && sd != null && Number.isNaN(sd))) {
      setError("Essais / graine MC invalides.");
      return;
    }
    setError(null);
    try {
      const m = await updateMcModel({
        id: mcSel.id,
        expected_row_version: mcSel.row_version,
        graph_json: mcJson,
        trials: tr,
        seed: sd,
      });
      setMcSel(m);
      await loadMcList();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onEvalMc = async () => {
    if (mcSel == null) {
      return;
    }
    setError(null);
    try {
      const m = await evaluateMcModel(mcSel.id);
      setMcSel(m);
      applyMcUniformFromGraphJson(m.graph_json);
      setMcTrials(String(m.trials));
      setMcSeed(m.seed != null ? String(m.seed) : "");
      await loadMcList();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onDeleteMc = async () => {
    if (mcSel == null) {
      return;
    }
    setError(null);
    try {
      await deleteMcModel(mcSel.id);
      setMcSel(null);
      applyMcUniformFromGraphJson(DEFAULT_MC_GRAPH);
      await loadMcList();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onCreateMk = async () => {
    const eq = Number.parseInt(mkEq.trim(), 10);
    if (Number.isNaN(eq) || mkTitle.trim() === "") {
      setError("Markov : équipement et titre requis.");
      return;
    }
    setError(null);
    try {
      await createMarkovModel({
        equipment_id: eq,
        title: mkTitle.trim(),
        graph_json: mkJson,
        status: "draft",
      });
      await loadMkList();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onUpdateMk = async () => {
    if (mkSel == null) {
      return;
    }
    setError(null);
    try {
      const m = await updateMarkovModel({
        id: mkSel.id,
        expected_row_version: mkSel.row_version,
        graph_json: mkJson,
      });
      setMkSel(m);
      await loadMkList();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onEvalMk = async () => {
    if (mkSel == null) {
      return;
    }
    setError(null);
    try {
      const m = await evaluateMarkovModel(mkSel.id);
      setMkSel(m);
      applyMkDiscreteFromGraphJson(m.graph_json);
      await loadMkList();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onDeleteMk = async () => {
    if (mkSel == null) {
      return;
    }
    setError(null);
    try {
      await deleteMarkovModel(mkSel.id);
      setMkSel(null);
      applyMkDiscreteFromGraphJson(DEFAULT_MARKOV_GRAPH);
      await loadMkList();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onCreateSo = async () => {
    const eq = Number.parseInt(soEq.trim(), 10);
    if (Number.isNaN(eq) || soTitle.trim() === "") {
      setError("Revue : équipement et titre requis.");
      return;
    }
    setError(null);
    try {
      await createRamExpertSignOff({
        equipment_id: eq,
        method_category: soMethod,
        target_ref: soTarget.trim() || null,
        title: soTitle.trim(),
        reviewer_name: soReviewer.trim() || null,
        reviewer_role: soRole.trim() || null,
        notes: soNotes.trim() || null,
      });
      await loadSoList();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onSignSo = async () => {
    if (soSel == null || soSignName.trim() === "") {
      setError("Nom du signataire requis pour la signature.");
      return;
    }
    setError(null);
    try {
      await signRamExpertReview({
        id: soSel.id,
        expected_row_version: soSel.row_version,
        reviewer_name: soSignName.trim(),
        notes: soNotes.trim() || null,
      });
      setSoSel(null);
      setSoSignName("");
      await loadSoList();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onDeleteSo = async () => {
    if (soSel == null) {
      return;
    }
    setError(null);
    try {
      await deleteRamExpertSignOff(soSel.id);
      setSoSel(null);
      await loadSoList();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const workspace = (
    <>
      {error ? <p className="mb-2 text-red-600">{error}</p> : null}

      <div className="mb-4 flex flex-wrap gap-1 border-b border-border-1 pb-3">
        {(
          [
            ["kpis", "KPI & analyses"],
            ["quality", "Qualité données"],
            ["failures", "Défaillances & fondations"],
            ["models", "Modèles RAM"],
          ] as const
        ).map(([id, label]) => (
          <button
            key={id}
            type="button"
            className={cn(
              "rounded-md border px-3 py-1.5 text-xs",
              ramHubTab === id
                ? "border-accent bg-bg-0 text-fg-1"
                : "border-border-1 text-fg-2 hover:border-fg-2/40",
            )}
            onClick={() => setRamHubTab(id)}
          >
            {label}
          </button>
        ))}
      </div>

      {ramHubTab === "kpis" ? (
        <>
          <RamMethodAssumptions />

          <section className="mb-6 rounded border border-border-1 bg-bg-1 p-3">
            <h2 className="mb-2 font-medium">Revue expert — signalement</h2>
            <p className="mb-2 text-xs text-fg-2">
              Enregistrement local signé (ram.manage / ram.view).
            </p>
            <div className="mb-2 flex flex-wrap gap-2">
              <input
                className="w-24 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                placeholder="filtrer équip."
                value={soFilter}
                onChange={(e) => setSoFilter(e.target.value)}
              />
              <button
                type="button"
                className="rounded border border-border-1 px-2 py-1 text-xs"
                onClick={() => void loadSoList()}
              >
                Actualiser
              </button>
            </div>
            <div className="mb-2 flex flex-wrap gap-2">
              <input
                className="w-14 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                value={soEq}
                onChange={(e) => setSoEq(e.target.value)}
              />
              <input
                className="min-w-[8rem] flex-1 rounded border border-border-1 bg-bg-0 px-2 py-1 text-xs"
                value={soTitle}
                onChange={(e) => setSoTitle(e.target.value)}
                placeholder="titre"
              />
              <select
                className="rounded border border-border-1 bg-bg-0 px-2 py-1 text-xs"
                value={soMethod}
                onChange={(e) => setSoMethod(e.target.value)}
              >
                <option value="general">general</option>
                <option value="weibull">weibull</option>
                <option value="fmeca">fmeca</option>
                <option value="fta">fta</option>
                <option value="rbd">rbd</option>
                <option value="eta">eta</option>
                <option value="mc">mc</option>
                <option value="markov">markov</option>
                <option value="rcm">rcm</option>
              </select>
              <input
                className="w-24 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-[10px]"
                value={soTarget}
                onChange={(e) => setSoTarget(e.target.value)}
                placeholder="cible ref"
              />
              <input
                className="w-28 rounded border border-border-1 bg-bg-0 px-2 py-1 text-xs"
                value={soReviewer}
                onChange={(e) => setSoReviewer(e.target.value)}
                placeholder="reviseur"
              />
              <input
                className="w-28 rounded border border-border-1 bg-bg-0 px-2 py-1 text-xs"
                value={soRole}
                onChange={(e) => setSoRole(e.target.value)}
                placeholder="rôle"
              />
              <button
                type="button"
                className="rounded bg-accent px-2 py-1 text-xs text-bg-0"
                onClick={() => void onCreateSo()}
              >
                Créer fiche
              </button>
            </div>
            <textarea
              className="mb-2 h-16 w-full rounded border border-border-1 bg-bg-0 p-2 text-xs"
              placeholder="notes"
              value={soNotes}
              onChange={(e) => setSoNotes(e.target.value)}
            />
            <table className="mb-2 w-full border-collapse text-left text-[10px]">
              <thead>
                <tr className="border-b border-border-1">
                  <th className="py-1 pr-2">id</th>
                  <th className="py-1 pr-2">méthode</th>
                  <th className="py-1 pr-2">statut</th>
                  <th className="py-1 pr-2">titre</th>
                  <th className="py-1 pr-2">signé</th>
                </tr>
              </thead>
              <tbody>
                {soRows.map((r) => (
                  <tr
                    key={r.id}
                    className={`cursor-pointer border-b border-border-1/60 ${soSel?.id === r.id ? "bg-accent/10" : ""}`}
                    onClick={() => {
                      setSoSel(r);
                      setSoNotes(r.notes);
                      setSoSignName("");
                    }}
                  >
                    <td className="py-1 pr-2 font-mono">{r.id}</td>
                    <td className="py-1 pr-2">{r.method_category}</td>
                    <td className="py-1 pr-2">{r.status}</td>
                    <td className="py-1 pr-2">{r.title}</td>
                    <td className="py-1 pr-2 font-mono">{r.signed_at ?? "—"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            {soSel != null && soSel.status !== "signed" ? (
              <div className="flex flex-wrap items-end gap-2 border-t border-border-1 pt-2">
                <input
                  className="min-w-[12rem] flex-1 rounded border border-border-1 bg-bg-0 px-2 py-1 text-xs"
                  placeholder="Nom complet signataire"
                  value={soSignName}
                  onChange={(e) => setSoSignName(e.target.value)}
                />
                <button
                  type="button"
                  className="rounded bg-accent px-2 py-1 text-xs text-bg-0"
                  onClick={() => void onSignSo()}
                >
                  Signer
                </button>
                <button
                  type="button"
                  className="rounded border border-red-600/50 px-2 py-1 text-xs text-red-600"
                  onClick={() => void onDeleteSo()}
                >
                  Supprimer fiche
                </button>
              </div>
            ) : null}
          </section>
        </>
      ) : null}

      {ramHubTab === "failures" ? (
        <>
          <section className="mb-6 rounded border border-border-1 bg-bg-1 p-3">
            <h2 className="mb-2 font-medium">Hiérarchies</h2>
            <ul className="mb-2 max-h-32 overflow-y-auto">
              {hierarchies.map((h) => (
                <li key={h.id}>
                  <button
                    type="button"
                    className={
                      selectedHid === h.id ? "font-semibold text-fg-1" : "text-fg-2 hover:text-fg-1"
                    }
                    onClick={() => setSelectedHid(h.id)}
                  >
                    {h.name} (v{h.version_no}) {h.is_active ? "" : "[inactive]"}
                  </button>
                </li>
              ))}
            </ul>
            <div className="flex flex-wrap gap-2">
              <input
                className="rounded border border-border-1 bg-bg-0 px-2 py-1"
                placeholder="Nom"
                value={hName}
                onChange={(e) => setHName(e.target.value)}
              />
              <input
                className="rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                placeholder="asset_scope JSON"
                value={hScope}
                onChange={(e) => setHScope(e.target.value)}
              />
              <input
                className="w-20 rounded border border-border-1 bg-bg-0 px-2 py-1"
                placeholder="ver"
                value={hVer}
                onChange={(e) => setHVer(e.target.value)}
              />
              <button
                type="button"
                className="rounded bg-accent px-3 py-1 text-bg-0"
                onClick={() => void onAddHierarchy()}
              >
                Ajouter hiérarchie
              </button>
            </div>
          </section>

          <section className="mb-6 rounded border border-border-1 bg-bg-1 p-3">
            <h2 className="mb-2 font-medium">Événements de défaillance (WO fermés)</h2>
            <div className="mb-2 flex flex-wrap items-center gap-2">
              <input
                className="w-40 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                placeholder="filtrer équipement id"
                value={feEquipFilter}
                onChange={(e) => setFeEquipFilter(e.target.value)}
              />
              <button
                type="button"
                className="rounded border border-border-1 px-2 py-1 text-fg-2 hover:text-fg-1"
                onClick={() => void loadFailureEvents()}
              >
                Actualiser
              </button>
            </div>
            <table className="w-full border-collapse text-left text-xs">
              <thead>
                <tr className="border-b border-border-1">
                  <th className="py-1 pr-2">id</th>
                  <th className="py-1 pr-2">source</th>
                  <th className="py-1 pr-2">équip.</th>
                  <th className="py-1 pr-2">arrêt (h)</th>
                  <th className="py-1 pr-2">vérif.</th>
                  <th className="py-1 pr-2">éligibilité (JSON)</th>
                </tr>
              </thead>
              <tbody>
                {failureEvents.map((fe) => (
                  <tr key={fe.id} className="border-b border-border-1/60">
                    <td className="py-1 pr-2 font-mono">{fe.id}</td>
                    <td className="py-1 pr-2">
                      {fe.source_type} #{fe.source_id}
                    </td>
                    <td className="py-1 pr-2">{fe.equipment_id}</td>
                    <td className="py-1 pr-2">{fe.downtime_duration_hours}</td>
                    <td className="py-1 pr-2">{fe.verification_status}</td>
                    <td
                      className="max-w-[14rem] truncate py-1 pr-2 font-mono text-[10px]"
                      title={fe.eligible_flags_json}
                    >
                      {fe.eligible_flags_json}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>

          <section className="mb-6 rounded border border-border-1 bg-bg-1 p-3">
            <h2 className="mb-2 font-medium">Journal d&apos;exposition runtime (h)</h2>
            <p className="mb-2 text-fg-2">
              PRD §6.10.2 — dénominateur T<sub>exp</sub> (somme des entrées `hours` sur la période).
            </p>
            <div className="mb-2 flex flex-wrap items-end gap-2">
              <input
                className="w-36 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                placeholder="filtrer équip. id"
                value={reEquipFilter}
                onChange={(e) => setReEquipFilter(e.target.value)}
              />
              <button
                type="button"
                className="rounded border border-border-1 px-2 py-1 text-fg-2 hover:text-fg-1"
                onClick={() => void loadRuntimeLogs()}
              >
                Actualiser
              </button>
            </div>
            <div className="mb-3 flex flex-wrap gap-2 border-b border-border-1 pb-3">
              <input
                className="w-24 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono"
                title="equipment_id"
                value={reEquipId}
                onChange={(e) => setReEquipId(e.target.value)}
              />
              <input
                className="w-24 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono"
                title="value (hours)"
                value={reValue}
                onChange={(e) => setReValue(e.target.value)}
              />
              <input
                className="min-w-[12rem] flex-1 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                title="recorded_at ISO"
                value={reRecordedAt}
                onChange={(e) => setReRecordedAt(e.target.value)}
              />
              <select
                className="rounded border border-border-1 bg-bg-0 px-2 py-1"
                value={reExposureType}
                onChange={(e) => setReExposureType(e.target.value)}
              >
                <option value="hours">hours</option>
                <option value="cycles">cycles</option>
                <option value="output_distance">output_distance</option>
                <option value="production_output">production_output</option>
              </select>
              <select
                className="rounded border border-border-1 bg-bg-0 px-2 py-1"
                value={reSourceType}
                onChange={(e) => setReSourceType(e.target.value)}
              >
                <option value="manual">manual</option>
                <option value="calendar_operating_schedule">calendar_operating_schedule</option>
              </select>
              <button
                type="button"
                className="rounded bg-accent px-3 py-1 text-bg-0"
                onClick={() => void onAddRuntimeExposure()}
              >
                Enregistrer
              </button>
            </div>
            <table className="w-full border-collapse text-left text-xs">
              <thead>
                <tr className="border-b border-border-1">
                  <th className="py-1 pr-2">id</th>
                  <th className="py-1 pr-2">équip.</th>
                  <th className="py-1 pr-2">type</th>
                  <th className="py-1 pr-2">valeur</th>
                  <th className="py-1 pr-2">source</th>
                  <th className="py-1 pr-2">enregistré</th>
                </tr>
              </thead>
              <tbody>
                {runtimeLogs.map((r) => (
                  <tr key={r.id} className="border-b border-border-1/60">
                    <td className="py-1 pr-2 font-mono">{r.id}</td>
                    <td className="py-1 pr-2">{r.equipment_id}</td>
                    <td className="py-1 pr-2">{r.exposure_type}</td>
                    <td className="py-1 pr-2">{r.value}</td>
                    <td className="py-1 pr-2">{r.source_type}</td>
                    <td className="py-1 pr-2 font-mono">{r.recorded_at}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>
        </>
      ) : null}

      {ramHubTab === "kpis" ? (
        <>
          <section className="mb-6 rounded border border-border-1 bg-bg-1 p-3">
            <h2 className="mb-2 font-medium">Instantanés KPI fiabilité</h2>
            <p className="mb-2 text-fg-2">
              MTBF, MTTR, disponibilité, taux de défaillance, répétition — événements éligibles +
              exposition.
            </p>
            {kpiInputEval ? (
              <KpiAnalysisInputGates evalRow={kpiInputEval} />
            ) : (
              <p className="mb-2 text-xs text-fg-2">
                Sélectionnez un équipement et une période pour qualifier les entrées d&apos;analyse.
              </p>
            )}
            <div className="mb-2 flex flex-wrap items-end gap-2">
              <input
                className="w-36 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                placeholder="filtrer équip. id"
                value={kpiEquipFilter}
                onChange={(e) => setKpiEquipFilter(e.target.value)}
              />
              <button
                type="button"
                className="rounded border border-border-1 px-2 py-1 text-fg-2 hover:text-fg-1"
                onClick={() => void loadKpiSnapshots()}
              >
                Actualiser liste
              </button>
            </div>
            <div className="mb-3 flex flex-wrap gap-2 border-b border-border-1 pb-3">
              <input
                className="w-24 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono"
                title="equipment_id"
                value={kpiEquip}
                onChange={(e) => setKpiEquip(e.target.value)}
              />
              <input
                className="min-w-[10rem] flex-1 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                title="period_start"
                value={kpiStart}
                onChange={(e) => setKpiStart(e.target.value)}
              />
              <input
                className="min-w-[10rem] flex-1 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                title="period_end"
                value={kpiEnd}
                onChange={(e) => setKpiEnd(e.target.value)}
              />
              <button
                type="button"
                className="rounded bg-accent px-3 py-1 text-bg-0"
                onClick={() => void onRefreshKpi()}
              >
                Calculer / mettre à jour
              </button>
              <button
                type="button"
                className="rounded border border-border-1 px-3 py-1 text-fg-2 hover:text-fg-1"
                onClick={() => void onKpiBackgroundJob()}
              >
                Job async (Rust)
              </button>
              {kpiBgJobId != null ? (
                <button
                  type="button"
                  className="rounded border border-red-600/60 px-2 py-1 text-xs text-red-600"
                  onClick={() => void onCancelKpiJob()}
                >
                  Annuler job #{kpiBgJobId}
                </button>
              ) : null}
            </div>
            {kpiJobProgress != null ? (
              <p className="mb-2 font-mono text-[10px] text-fg-2">
                Job {kpiJobProgress.job_id} — {kpiJobProgress.status} —{" "}
                {kpiJobProgress.progress_pct.toFixed(0)}%
              </p>
            ) : null}
            <table className="w-full border-collapse text-left text-xs">
              <thead>
                <tr className="border-b border-border-1">
                  <th className="py-1 pr-2">id</th>
                  <th className="py-1 pr-2">équip.</th>
                  <th className="py-1 pr-2">période</th>
                  <th className="py-1 pr-2">MTBF</th>
                  <th className="py-1 pr-2">MTTR</th>
                  <th className="py-1 pr-2">A</th>
                  <th className="py-1 pr-2">λ</th>
                  <th className="py-1 pr-2">rép.</th>
                  <th className="py-1 pr-2">n</th>
                  <th className="py-1 pr-2">qualité</th>
                  <th className="py-1 pr-2">empreinte</th>
                </tr>
              </thead>
              <tbody>
                {kpiRows.map((k) => (
                  <tr
                    key={k.id}
                    className={`cursor-pointer border-b border-border-1/60 hover:bg-bg-0/80 ${
                      selectedKpiSnapshot?.id === k.id ? "bg-accent/10" : ""
                    }`}
                    onClick={() => void onSelectKpiRow(k.id)}
                  >
                    <td className="py-1 pr-2 font-mono">{k.id}</td>
                    <td className="py-1 pr-2">{k.equipment_id ?? "—"}</td>
                    <td className="max-w-[10rem] py-1 pr-2 font-mono text-[10px]">
                      {k.period_start} → {k.period_end}
                    </td>
                    <td className="py-1 pr-2">{k.mtbf ?? "—"}</td>
                    <td className="py-1 pr-2">{k.mttr ?? "—"}</td>
                    <td className="py-1 pr-2">{k.availability ?? "—"}</td>
                    <td className="py-1 pr-2">{k.failure_rate ?? "—"}</td>
                    <td className="py-1 pr-2">{k.repeat_failure_rate ?? "—"}</td>
                    <td className="py-1 pr-2">{k.event_count}</td>
                    <td className="py-1 pr-2">{k.data_quality_score.toFixed(2)}</td>
                    <td
                      className="max-w-[6rem] truncate py-1 pr-2 font-mono text-[9px]"
                      title={k.analysis_dataset_hash_sha256}
                    >
                      {k.analysis_dataset_hash_sha256.slice(0, 12)}…
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            {selectedKpiSnapshot != null ? (
              <KpiSnapshotPlotDetail
                snapshot={selectedKpiSnapshot}
                onReopenFilters={onReopenKpiFilters}
              />
            ) : (
              <p className="mt-3 text-xs text-fg-2">
                Cliquez une ligne pour recharger le snapshot et afficher le graphique.
              </p>
            )}
          </section>

          <section className="mb-6 rounded border border-border-1 bg-bg-1 p-3">
            <h2 className="mb-2 font-medium">Weibull — ajustement (inter-arrivées)</h2>
            <p className="mb-2 text-xs text-fg-2">
              PRD 6.10.4 — MLE 2 paramètres ; intervalles asymptotiques si échantillon suffisant.
              Nécessite <code className="font-mono">ram.analyze</code>.
            </p>
            <div className="mb-3 flex flex-wrap items-end gap-2">
              <label className="flex flex-col text-xs text-fg-2">
                équip.
                <input
                  className="mt-0.5 w-20 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono"
                  value={wbEquip}
                  onChange={(e) => setWbEquip(e.target.value)}
                />
              </label>
              <label className="flex min-w-[10rem] flex-1 flex-col text-xs text-fg-2">
                période début (optionnel)
                <input
                  className="mt-0.5 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-[10px]"
                  value={wbStart}
                  onChange={(e) => setWbStart(e.target.value)}
                />
              </label>
              <label className="flex min-w-[10rem] flex-1 flex-col text-xs text-fg-2">
                période fin (optionnel)
                <input
                  className="mt-0.5 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-[10px]"
                  value={wbEnd}
                  onChange={(e) => setWbEnd(e.target.value)}
                />
              </label>
              <button
                type="button"
                className="rounded bg-accent px-3 py-1 text-bg-0"
                onClick={() => void onRunWeibull()}
              >
                Lancer l&apos;ajustement
              </button>
            </div>
            {wbResult != null ? (
              <div className="rounded border border-border-1/60 bg-bg-0 p-2 text-xs">
                <p className="mb-1 font-mono text-[10px] text-fg-2">
                  n={wbResult.n_points} — adéquat : {wbResult.adequate_sample ? "oui" : "non"} —{" "}
                  {wbResult.message}
                </p>
                <p className="mb-1">
                  β (shape) : {wbResult.beta != null ? wbResult.beta.toFixed(4) : "—"} [
                  {wbResult.beta_ci_low != null ? wbResult.beta_ci_low.toFixed(4) : "—"} ;{" "}
                  {wbResult.beta_ci_high != null ? wbResult.beta_ci_high.toFixed(4) : "—"}]
                </p>
                <p>
                  η (scale h) : {wbResult.eta != null ? wbResult.eta.toFixed(2) : "—"} [
                  {wbResult.eta_ci_low != null ? wbResult.eta_ci_low.toFixed(2) : "—"} ;{" "}
                  {wbResult.eta_ci_high != null ? wbResult.eta_ci_high.toFixed(2) : "—"}]
                </p>
              </div>
            ) : (
              <p className="text-xs text-fg-2">
                Aucun résultat — lancez un ajustement sur des événements de défaillance.
              </p>
            )}
          </section>
        </>
      ) : null}

      {ramHubTab === "models" ? (
        <>
          <section className="mb-6 rounded border border-border-1 bg-bg-1 p-3">
            <h2 className="mb-2 font-medium">FMECA — analyses et matrice RPN</h2>
            <p className="mb-2 text-xs text-fg-2">
              PRD 6.10.5 — CRUD analyses et lignes ; RPN = S×O×D (1–10). Écriture :{" "}
              <code className="font-mono">ram.manage</code>.
            </p>
            <div className="mb-3 flex flex-wrap items-end gap-2 border-b border-border-1 pb-3">
              <input
                className="w-28 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                placeholder="filtrer équip."
                value={fmEquipFilter}
                onChange={(e) => setFmEquipFilter(e.target.value)}
              />
              <button
                type="button"
                className="rounded border border-border-1 px-2 py-1 text-fg-2 hover:text-fg-1"
                onClick={() => void loadFmecaAnalysesList()}
              >
                Actualiser analyses
              </button>
            </div>
            <div className="mb-3 flex flex-wrap items-end gap-2">
              <input
                className="w-20 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                title="equipment_id"
                value={fmNewEquip}
                onChange={(e) => setFmNewEquip(e.target.value)}
              />
              <input
                className="min-w-[8rem] flex-1 rounded border border-border-1 bg-bg-0 px-2 py-1 text-xs"
                placeholder="titre analyse"
                value={fmNewTitle}
                onChange={(e) => setFmNewTitle(e.target.value)}
              />
              <input
                className="min-w-[12rem] flex-[2] rounded border border-border-1 bg-bg-0 px-2 py-1 text-xs"
                placeholder="frontière / périmètre"
                value={fmNewBoundary}
                onChange={(e) => setFmNewBoundary(e.target.value)}
              />
              <button
                type="button"
                className="rounded bg-accent px-3 py-1 text-bg-0"
                onClick={() => void onCreateFmeca()}
              >
                Créer analyse
              </button>
            </div>
            <table className="mb-4 w-full border-collapse text-left text-xs">
              <thead>
                <tr className="border-b border-border-1">
                  <th className="py-1 pr-2">id</th>
                  <th className="py-1 pr-2">équip.</th>
                  <th className="py-1 pr-2">titre</th>
                  <th className="py-1 pr-2">statut</th>
                  <th className="py-1 pr-2">actions</th>
                </tr>
              </thead>
              <tbody>
                {fmAnalyses.map((a) => (
                  <tr
                    key={a.id}
                    className={`cursor-pointer border-b border-border-1/60 hover:bg-bg-0/80 ${
                      fmSelectedId === a.id ? "bg-accent/10" : ""
                    }`}
                    onClick={() => setFmSelectedId(a.id)}
                  >
                    <td className="py-1 pr-2 font-mono">{a.id}</td>
                    <td className="py-1 pr-2">{a.equipment_id}</td>
                    <td className="py-1 pr-2">{a.title}</td>
                    <td className="py-1 pr-2">{a.status}</td>
                    <td className="py-1 pr-2">
                      <button
                        type="button"
                        className="text-fg-2 underline hover:text-fg-1"
                        onClick={(e) => {
                          e.stopPropagation();
                          void onDeleteFmeca(a.id);
                        }}
                      >
                        supprimer
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>

            {fmSelectedId != null ? (
              <>
                <h3 className="mb-1 font-medium">Lignes FMECA (analyse #{fmSelectedId})</h3>
                <div className="mb-3 grid max-w-4xl grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-3">
                  <input
                    className="rounded border border-border-1 bg-bg-0 px-2 py-1 text-xs"
                    placeholder="défaillance fonctionnelle"
                    value={fmFf}
                    onChange={(e) => setFmFf(e.target.value)}
                  />
                  <input
                    className="rounded border border-border-1 bg-bg-0 px-2 py-1 text-xs"
                    placeholder="effet"
                    value={fmFe}
                    onChange={(e) => setFmFe(e.target.value)}
                  />
                  <input
                    className="rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                    placeholder="mode ISO (id)"
                    value={fmFmId}
                    onChange={(e) => setFmFmId(e.target.value)}
                  />
                  <input
                    className="w-16 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                    title="S"
                    value={fmS}
                    onChange={(e) => setFmS(e.target.value)}
                  />
                  <input
                    className="w-16 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                    title="O"
                    value={fmO}
                    onChange={(e) => setFmO(e.target.value)}
                  />
                  <input
                    className="w-16 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                    title="D"
                    value={fmD}
                    onChange={(e) => setFmD(e.target.value)}
                  />
                  <p className="text-xs text-fg-2 sm:col-span-3">
                    RPN (aperçu S×O×D) :{" "}
                    <strong>{fmPreviewRpn != null ? fmPreviewRpn : "—"}</strong>
                    {fmItemId != null ? ` — édition ligne #${fmItemId}` : null}
                  </p>
                  <input
                    className="sm:col-span-2 rounded border border-border-1 bg-bg-0 px-2 py-1 text-xs"
                    placeholder="action recommandée"
                    value={fmRa}
                    onChange={(e) => setFmRa(e.target.value)}
                  />
                  <input
                    className="rounded border border-border-1 bg-bg-0 px-2 py-1 text-xs"
                    placeholder="contrôle courant"
                    value={fmCc}
                    onChange={(e) => setFmCc(e.target.value)}
                  />
                  <label className="flex flex-col text-xs text-fg-2 sm:col-span-2">
                    Lier plan PM (optionnel)
                    <select
                      className="mt-0.5 rounded border border-border-1 bg-bg-0 px-2 py-1 text-xs text-fg-1"
                      value={fmPmId}
                      onChange={(e) => setFmPmId(e.target.value)}
                    >
                      <option value="">—</option>
                      {pmPlans.map((p) => (
                        <option key={p.id} value={String(p.id)}>
                          #{p.id} {p.code} — {p.title}
                        </option>
                      ))}
                    </select>
                  </label>
                  <input
                    className="rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                    placeholder="RPN révisé (optionnel)"
                    value={fmRevRpn}
                    onChange={(e) => setFmRevRpn(e.target.value)}
                  />
                </div>
                <div className="mb-3 flex flex-wrap gap-2">
                  <button
                    type="button"
                    className="rounded bg-accent px-3 py-1 text-bg-0"
                    onClick={() => void onSaveFmecaItem()}
                  >
                    {fmItemId != null ? "Enregistrer ligne" : "Ajouter ligne"}
                  </button>
                  <button
                    type="button"
                    className="rounded border border-border-1 px-2 py-1 text-fg-2 hover:text-fg-1"
                    onClick={() => {
                      setFmItemId(null);
                      setFmItemRv(null);
                      setFmFf("");
                      setFmFe("");
                      setFmS("5");
                      setFmO("5");
                      setFmD("5");
                      setFmRa("");
                      setFmCc("");
                      setFmPmId("");
                      setFmFmId("");
                      setFmRevRpn("");
                    }}
                  >
                    Nouvelle ligne
                  </button>
                </div>
                <table className="w-full border-collapse text-left text-xs">
                  <thead>
                    <tr className="border-b border-border-1">
                      <th className="py-1 pr-2">id</th>
                      <th className="py-1 pr-2">S</th>
                      <th className="py-1 pr-2">O</th>
                      <th className="py-1 pr-2">D</th>
                      <th className="py-1 pr-2">RPN</th>
                      <th className="py-1 pr-2">défaillance f.</th>
                      <th className="py-1 pr-2">effet</th>
                      <th className="py-1 pr-2">PM</th>
                      <th className="py-1 pr-2" />
                    </tr>
                  </thead>
                  <tbody>
                    {fmItems.map((it) => (
                      <tr key={it.id} className="border-b border-border-1/60">
                        <td className="py-1 pr-2 font-mono">{it.id}</td>
                        <td className="py-1 pr-2">{it.severity}</td>
                        <td className="py-1 pr-2">{it.occurrence}</td>
                        <td className="py-1 pr-2">{it.detectability}</td>
                        <td className="py-1 pr-2 font-medium">{it.rpn}</td>
                        <td
                          className="max-w-[8rem] truncate py-1 pr-2"
                          title={it.functional_failure}
                        >
                          {it.functional_failure}
                        </td>
                        <td className="max-w-[8rem] truncate py-1 pr-2" title={it.failure_effect}>
                          {it.failure_effect}
                        </td>
                        <td className="py-1 pr-2 font-mono">{it.linked_pm_plan_id ?? "—"}</td>
                        <td className="py-1 pr-2">
                          <button
                            type="button"
                            className="mr-2 text-fg-2 underline hover:text-fg-1"
                            onClick={() => {
                              setFmItemId(it.id);
                              setFmItemRv(it.row_version);
                              setFmFf(it.functional_failure);
                              setFmFe(it.failure_effect);
                              setFmS(String(it.severity));
                              setFmO(String(it.occurrence));
                              setFmD(String(it.detectability));
                              setFmRa(it.recommended_action);
                              setFmCc(it.current_control);
                              setFmPmId(
                                it.linked_pm_plan_id != null ? String(it.linked_pm_plan_id) : "",
                              );
                              setFmFmId(
                                it.failure_mode_id != null ? String(it.failure_mode_id) : "",
                              );
                              setFmRevRpn(it.revised_rpn != null ? String(it.revised_rpn) : "");
                            }}
                          >
                            éditer
                          </button>
                          <button
                            type="button"
                            className="text-fg-2 underline hover:text-fg-1"
                            onClick={() => void onDeleteFmecaItemRow(it)}
                          >
                            suppr.
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </>
            ) : (
              <p className="text-xs text-fg-2">
                Sélectionnez une analyse dans le tableau pour éditer les lignes.
              </p>
            )}
          </section>

          <section className="mb-6 rounded border border-border-1 bg-bg-1 p-3">
            <h2 className="mb-2 font-medium">RCM — décisions liées au PM</h2>
            <p className="mb-2 text-xs text-fg-2">
              PRD 6.10.6 — tactique (liste blanche) et{" "}
              <code className="font-mono">linked_pm_plan_id</code> optionnel.
            </p>
            <div className="mb-3 flex flex-wrap items-end gap-2 border-b border-border-1 pb-3">
              <input
                className="w-28 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                placeholder="filtrer équip."
                value={rcmEquipFilter}
                onChange={(e) => setRcmEquipFilter(e.target.value)}
              />
              <button
                type="button"
                className="rounded border border-border-1 px-2 py-1 text-fg-2 hover:text-fg-1"
                onClick={() => void loadRcmStudiesList()}
              >
                Actualiser études
              </button>
            </div>
            <div className="mb-3 flex flex-wrap items-end gap-2">
              <input
                className="w-20 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                value={rcmNewEquip}
                onChange={(e) => setRcmNewEquip(e.target.value)}
              />
              <input
                className="min-w-[12rem] flex-1 rounded border border-border-1 bg-bg-0 px-2 py-1 text-xs"
                placeholder="titre étude RCM"
                value={rcmNewTitle}
                onChange={(e) => setRcmNewTitle(e.target.value)}
              />
              <button
                type="button"
                className="rounded bg-accent px-3 py-1 text-bg-0"
                onClick={() => void onCreateRcm()}
              >
                Créer étude
              </button>
            </div>
            <table className="mb-4 w-full border-collapse text-left text-xs">
              <thead>
                <tr className="border-b border-border-1">
                  <th className="py-1 pr-2">id</th>
                  <th className="py-1 pr-2">équip.</th>
                  <th className="py-1 pr-2">titre</th>
                  <th className="py-1 pr-2">statut</th>
                  <th className="py-1 pr-2">actions</th>
                </tr>
              </thead>
              <tbody>
                {rcmStudies.map((s) => (
                  <tr
                    key={s.id}
                    className={`cursor-pointer border-b border-border-1/60 hover:bg-bg-0/80 ${
                      rcmSelectedId === s.id ? "bg-accent/10" : ""
                    }`}
                    onClick={() => setRcmSelectedId(s.id)}
                  >
                    <td className="py-1 pr-2 font-mono">{s.id}</td>
                    <td className="py-1 pr-2">{s.equipment_id}</td>
                    <td className="py-1 pr-2">{s.title}</td>
                    <td className="py-1 pr-2">{s.status}</td>
                    <td className="py-1 pr-2">
                      <button
                        type="button"
                        className="text-fg-2 underline hover:text-fg-1"
                        onClick={(e) => {
                          e.stopPropagation();
                          void onDeleteRcm(s.id);
                        }}
                      >
                        supprimer
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>

            {rcmSelectedId != null ? (
              <>
                <h3 className="mb-1 font-medium">Décisions (étude #{rcmSelectedId})</h3>
                <div className="mb-3 grid max-w-4xl grid-cols-1 gap-2 sm:grid-cols-2">
                  <input
                    className="rounded border border-border-1 bg-bg-0 px-2 py-1 text-xs"
                    placeholder="fonction / fonctionnement"
                    value={rcmFnDesc}
                    onChange={(e) => setRcmFnDesc(e.target.value)}
                  />
                  <input
                    className="rounded border border-border-1 bg-bg-0 px-2 py-1 text-xs"
                    placeholder="défaillance fonctionnelle"
                    value={rcmFf}
                    onChange={(e) => setRcmFf(e.target.value)}
                  />
                  <input
                    className="rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                    placeholder="mode ISO (id)"
                    value={rcmFmId}
                    onChange={(e) => setRcmFmId(e.target.value)}
                  />
                  <input
                    className="rounded border border-border-1 bg-bg-0 px-2 py-1 text-xs"
                    placeholder="catégorie de conséquence"
                    value={rcmCc}
                    onChange={(e) => setRcmCc(e.target.value)}
                  />
                  <label className="flex flex-col text-xs text-fg-2">
                    Tactique RCM
                    <select
                      className="mt-0.5 rounded border border-border-1 bg-bg-0 px-2 py-1 text-xs text-fg-1"
                      value={rcmTactic}
                      onChange={(e) => setRcmTactic(e.target.value)}
                    >
                      <option value="condition_based">condition_based</option>
                      <option value="time_based">time_based</option>
                      <option value="failure_finding">failure_finding</option>
                      <option value="run_to_failure">run_to_failure</option>
                      <option value="redesign">redesign</option>
                    </select>
                  </label>
                  <label className="flex flex-col text-xs text-fg-2">
                    Lier plan PM (optionnel)
                    <select
                      className="mt-0.5 rounded border border-border-1 bg-bg-0 px-2 py-1 text-xs text-fg-1"
                      value={rcmPmId}
                      onChange={(e) => setRcmPmId(e.target.value)}
                    >
                      <option value="">—</option>
                      {pmPlans.map((p) => (
                        <option key={p.id} value={String(p.id)}>
                          #{p.id} {p.code} — {p.title}
                        </option>
                      ))}
                    </select>
                  </label>
                  <input
                    className="sm:col-span-2 rounded border border-border-1 bg-bg-0 px-2 py-1 text-xs"
                    placeholder="justification"
                    value={rcmJust}
                    onChange={(e) => setRcmJust(e.target.value)}
                  />
                  <input
                    className="sm:col-span-2 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-[10px]"
                    placeholder="revue due (ISO datetime, optionnel)"
                    value={rcmReview}
                    onChange={(e) => setRcmReview(e.target.value)}
                  />
                </div>
                <p className="mb-2 text-xs text-fg-2">
                  {rcmDecId != null ? `Édition décision #${rcmDecId}` : "Nouvelle décision"}
                </p>
                <div className="mb-3 flex flex-wrap gap-2">
                  <button
                    type="button"
                    className="rounded bg-accent px-3 py-1 text-bg-0"
                    onClick={() => void onSaveRcmDecision()}
                  >
                    {rcmDecId != null ? "Enregistrer décision" : "Ajouter décision"}
                  </button>
                  <button
                    type="button"
                    className="rounded border border-border-1 px-2 py-1 text-fg-2 hover:text-fg-1"
                    onClick={() => {
                      setRcmDecId(null);
                      setRcmDecRv(null);
                      setRcmFnDesc("");
                      setRcmFf("");
                      setRcmCc("");
                      setRcmTactic("time_based");
                      setRcmJust("");
                      setRcmPmId("");
                      setRcmReview("");
                      setRcmFmId("");
                    }}
                  >
                    Nouvelle décision
                  </button>
                </div>
                <table className="w-full border-collapse text-left text-xs">
                  <thead>
                    <tr className="border-b border-border-1">
                      <th className="py-1 pr-2">id</th>
                      <th className="py-1 pr-2">tactique</th>
                      <th className="py-1 pr-2">PM</th>
                      <th className="py-1 pr-2">fonction</th>
                      <th className="py-1 pr-2">défaillance f.</th>
                      <th className="py-1 pr-2" />
                    </tr>
                  </thead>
                  <tbody>
                    {rcmDecisions.map((d) => (
                      <tr key={d.id} className="border-b border-border-1/60">
                        <td className="py-1 pr-2 font-mono">{d.id}</td>
                        <td className="py-1 pr-2 font-mono">{d.selected_tactic}</td>
                        <td className="py-1 pr-2 font-mono">{d.linked_pm_plan_id ?? "—"}</td>
                        <td
                          className="max-w-[8rem] truncate py-1 pr-2"
                          title={d.function_description}
                        >
                          {d.function_description}
                        </td>
                        <td
                          className="max-w-[8rem] truncate py-1 pr-2"
                          title={d.functional_failure}
                        >
                          {d.functional_failure}
                        </td>
                        <td className="py-1 pr-2">
                          <button
                            type="button"
                            className="mr-2 text-fg-2 underline hover:text-fg-1"
                            onClick={() => {
                              setRcmDecId(d.id);
                              setRcmDecRv(d.row_version);
                              setRcmFnDesc(d.function_description);
                              setRcmFf(d.functional_failure);
                              setRcmCc(d.consequence_category);
                              setRcmTactic(d.selected_tactic);
                              setRcmJust(d.justification);
                              setRcmPmId(
                                d.linked_pm_plan_id != null ? String(d.linked_pm_plan_id) : "",
                              );
                              setRcmReview(d.review_due_at ?? "");
                              setRcmFmId(
                                d.failure_mode_id != null ? String(d.failure_mode_id) : "",
                              );
                            }}
                          >
                            éditer
                          </button>
                          <button
                            type="button"
                            className="text-fg-2 underline hover:text-fg-1"
                            onClick={() => void onDeleteRcmDecisionRow(d)}
                          >
                            suppr.
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </>
            ) : (
              <p className="text-xs text-fg-2">
                Sélectionnez une étude pour gérer les décisions RCM.
              </p>
            )}
          </section>

          <section className="mb-6 rounded border border-border-1 bg-bg-1 p-3">
            <h2 className="mb-2 font-medium">FTA / RBD / arbre d&apos;événements (PRD 6.10)</h2>
            <p className="mb-2 text-xs text-fg-2">
              Modèles graphe locaux — FTA (ET/OU + prob. top), RBD (série/parallèle), ETA
              (séquence). Évaluer : <code className="font-mono">ram.analyze</code> ; CRUD :{" "}
              <code className="font-mono">ram.manage</code>.
            </p>
            <div className="mb-3 flex flex-wrap items-center gap-2 border-b border-border-1/60 pb-2">
              <Link
                to="/analytics/reliability/lab"
                className="inline-flex items-center gap-1.5 rounded border border-border-1 bg-bg-0 px-2 py-1 text-[11px] text-fg-1 hover:bg-bg-1"
                title="Éditeur schématique FTA / RBD"
              >
                <FlaskConical className="h-3.5 w-3.5 shrink-0" />
                Ouvrir le laboratoire visuel (FTA/RBD)
              </Link>
            </div>

            <h3 className="mb-1 font-medium">FTA</h3>
            <div className="mb-2 flex flex-wrap gap-2">
              <input
                className="w-24 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                placeholder="filtrer équip."
                value={gfFtaFilter}
                onChange={(e) => setGfFtaFilter(e.target.value)}
              />
              <button
                type="button"
                className="rounded border border-border-1 px-2 py-1 text-xs"
                onClick={() => void loadGfFta()}
              >
                Actualiser
              </button>
            </div>
            <div className="mb-2 flex flex-wrap gap-2">
              <input
                className="w-16 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                value={gfFtaEq}
                onChange={(e) => setGfFtaEq(e.target.value)}
              />
              <input
                className="min-w-[8rem] flex-1 rounded border border-border-1 bg-bg-0 px-2 py-1 text-xs"
                value={gfFtaTitle}
                onChange={(e) => setGfFtaTitle(e.target.value)}
              />
              <button
                type="button"
                className="rounded bg-accent px-2 py-1 text-xs text-bg-0"
                onClick={() => void onCreateGfFta()}
              >
                Créer
              </button>
              <button
                type="button"
                className="rounded border border-border-1 px-2 py-1 text-xs"
                disabled={gfFtaSel == null}
                onClick={() => void onUpdateGfFta()}
              >
                Enregistrer JSON
              </button>
              <button
                type="button"
                className="rounded border border-border-1 px-2 py-1 text-xs"
                disabled={gfFtaSel == null}
                onClick={() => void onEvalGfFta()}
              >
                Évaluer
              </button>
              <button
                type="button"
                className="rounded border border-red-600/50 px-2 py-1 text-xs text-red-600"
                disabled={gfFtaSel == null}
                onClick={() => void onDeleteGfFta()}
              >
                Suppr.
              </button>
            </div>
            <table className="mb-2 w-full border-collapse text-left text-[10px]">
              <thead>
                <tr className="border-b border-border-1">
                  <th className="py-1 pr-2">id</th>
                  <th className="py-1 pr-2">titre</th>
                </tr>
              </thead>
              <tbody>
                {gfFtaRows.map((r) => (
                  <tr
                    key={r.id}
                    className={`cursor-pointer border-b border-border-1/60 ${gfFtaSel?.id === r.id ? "bg-accent/10" : ""}`}
                    onClick={() => {
                      setGfFtaSel(r);
                      setGfFtaJson(r.graph_json);
                    }}
                  >
                    <td className="py-1 pr-2 font-mono">{r.id}</td>
                    <td className="py-1 pr-2">{r.title}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <details className="mb-2 text-xs text-fg-2">
              <summary className="cursor-pointer select-none text-fg-1">Graphe JSON (FTA)</summary>
              <textarea
                className="mt-1 h-28 w-full rounded border border-border-1 bg-bg-0 p-2 font-mono text-[10px] text-fg-1"
                value={gfFtaJson}
                onChange={(e) => setGfFtaJson(e.target.value)}
              />
            </details>
            {gfFtaSel != null ? (
              <pre className="max-h-24 overflow-auto rounded border border-border-1/60 bg-bg-0 p-2 text-[9px] text-fg-2">
                {gfFtaSel.result_json}
              </pre>
            ) : null}

            <h3 className="mb-1 mt-4 font-medium">RBD</h3>
            <div className="mb-2 flex flex-wrap gap-2">
              <input
                className="w-24 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                value={gfRbdFilter}
                onChange={(e) => setGfRbdFilter(e.target.value)}
              />
              <button
                type="button"
                className="rounded border border-border-1 px-2 py-1 text-xs"
                onClick={() => void loadGfRbd()}
              >
                Actualiser
              </button>
            </div>
            <div className="mb-2 flex flex-wrap gap-2">
              <input
                className="w-16 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                value={gfRbdEq}
                onChange={(e) => setGfRbdEq(e.target.value)}
              />
              <input
                className="min-w-[8rem] flex-1 rounded border border-border-1 bg-bg-0 px-2 py-1 text-xs"
                value={gfRbdTitle}
                onChange={(e) => setGfRbdTitle(e.target.value)}
              />
              <button
                type="button"
                className="rounded bg-accent px-2 py-1 text-xs text-bg-0"
                onClick={() => void onCreateGfRbd()}
              >
                Créer
              </button>
              <button
                type="button"
                className="rounded border border-border-1 px-2 py-1 text-xs"
                disabled={gfRbdSel == null}
                onClick={() => void onUpdateGfRbd()}
              >
                Enregistrer JSON
              </button>
              <button
                type="button"
                className="rounded border border-border-1 px-2 py-1 text-xs"
                disabled={gfRbdSel == null}
                onClick={() => void onEvalGfRbd()}
              >
                Évaluer
              </button>
              <button
                type="button"
                className="rounded border border-red-600/50 px-2 py-1 text-xs text-red-600"
                disabled={gfRbdSel == null}
                onClick={() => void onDeleteGfRbd()}
              >
                Suppr.
              </button>
            </div>
            <table className="mb-2 w-full border-collapse text-left text-[10px]">
              <thead>
                <tr className="border-b border-border-1">
                  <th className="py-1 pr-2">id</th>
                  <th className="py-1 pr-2">titre</th>
                </tr>
              </thead>
              <tbody>
                {gfRbdRows.map((r) => (
                  <tr
                    key={r.id}
                    className={`cursor-pointer border-b border-border-1/60 ${gfRbdSel?.id === r.id ? "bg-accent/10" : ""}`}
                    onClick={() => {
                      setGfRbdSel(r);
                      setGfRbdJson(r.graph_json);
                    }}
                  >
                    <td className="py-1 pr-2 font-mono">{r.id}</td>
                    <td className="py-1 pr-2">{r.title}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <details className="mb-2 text-xs text-fg-2">
              <summary className="cursor-pointer select-none text-fg-1">Graphe JSON (RBD)</summary>
              <textarea
                className="mt-1 h-28 w-full rounded border border-border-1 bg-bg-0 p-2 font-mono text-[10px] text-fg-1"
                value={gfRbdJson}
                onChange={(e) => setGfRbdJson(e.target.value)}
              />
            </details>
            {gfRbdSel != null ? (
              <pre className="max-h-24 overflow-auto rounded border border-border-1/60 bg-bg-0 p-2 text-[9px] text-fg-2">
                {gfRbdSel.result_json}
              </pre>
            ) : null}

            <h3 className="mb-1 mt-4 font-medium">Arbre d&apos;événements</h3>
            <div className="mb-2 flex flex-wrap gap-2">
              <input
                className="w-24 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                value={gfEtaFilter}
                onChange={(e) => setGfEtaFilter(e.target.value)}
              />
              <button
                type="button"
                className="rounded border border-border-1 px-2 py-1 text-xs"
                onClick={() => void loadGfEta()}
              >
                Actualiser
              </button>
            </div>
            <div className="mb-2 flex flex-wrap gap-2">
              <input
                className="w-16 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                value={gfEtaEq}
                onChange={(e) => setGfEtaEq(e.target.value)}
              />
              <input
                className="min-w-[8rem] flex-1 rounded border border-border-1 bg-bg-0 px-2 py-1 text-xs"
                value={gfEtaTitle}
                onChange={(e) => setGfEtaTitle(e.target.value)}
              />
              <button
                type="button"
                className="rounded bg-accent px-2 py-1 text-xs text-bg-0"
                onClick={() => void onCreateGfEta()}
              >
                Créer
              </button>
              <button
                type="button"
                className="rounded border border-border-1 px-2 py-1 text-xs"
                disabled={gfEtaSel == null}
                onClick={() => void onUpdateGfEta()}
              >
                Enregistrer JSON
              </button>
              <button
                type="button"
                className="rounded border border-border-1 px-2 py-1 text-xs"
                disabled={gfEtaSel == null}
                onClick={() => void onEvalGfEta()}
              >
                Évaluer
              </button>
              <button
                type="button"
                className="rounded border border-red-600/50 px-2 py-1 text-xs text-red-600"
                disabled={gfEtaSel == null}
                onClick={() => void onDeleteGfEta()}
              >
                Suppr.
              </button>
            </div>
            <table className="mb-2 w-full border-collapse text-left text-[10px]">
              <thead>
                <tr className="border-b border-border-1">
                  <th className="py-1 pr-2">id</th>
                  <th className="py-1 pr-2">titre</th>
                </tr>
              </thead>
              <tbody>
                {gfEtaRows.map((r) => (
                  <tr
                    key={r.id}
                    className={`cursor-pointer border-b border-border-1/60 ${gfEtaSel?.id === r.id ? "bg-accent/10" : ""}`}
                    onClick={() => {
                      setGfEtaSel(r);
                      setGfEtaJson(r.graph_json);
                    }}
                  >
                    <td className="py-1 pr-2 font-mono">{r.id}</td>
                    <td className="py-1 pr-2">{r.title}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <details className="mb-2 text-xs text-fg-2">
              <summary className="cursor-pointer select-none text-fg-1">Graphe JSON (ETA)</summary>
              <textarea
                className="mt-1 h-28 w-full rounded border border-border-1 bg-bg-0 p-2 font-mono text-[10px] text-fg-1"
                value={gfEtaJson}
                onChange={(e) => setGfEtaJson(e.target.value)}
              />
            </details>
            {gfEtaSel != null ? (
              <pre className="max-h-24 overflow-auto rounded border border-border-1/60 bg-bg-0 p-2 text-[9px] text-fg-2">
                {gfEtaSel.result_json}
              </pre>
            ) : null}
          </section>

          <section className="mb-6 rounded border border-border-1 bg-bg-1 p-3">
            <h2 className="mb-2 font-medium">Monte Carlo, Markov, garde-fous</h2>
            <p className="mb-2 text-xs text-fg-2">
              MC : essais + graine reproductible ; Markov : CMTH discrète (puissances). Garde-fous :
              limites et activation des méthodes.
            </p>
            <h3 className="mb-1 font-medium">Garde-fous (ram.manage)</h3>
            <div className="mb-3 grid max-w-xl grid-cols-1 gap-2 rounded border border-border-1/60 bg-bg-0 p-2 sm:grid-cols-2">
              <label className="flex items-center gap-2 text-xs text-fg-2">
                <input
                  type="checkbox"
                  checked={grMcEnabled}
                  onChange={(e) => setGrMcEnabled(e.target.checked)}
                />
                Monte Carlo activé
              </label>
              <label className="flex items-center gap-2 text-xs text-fg-2">
                <input
                  type="checkbox"
                  checked={grMkEnabled}
                  onChange={(e) => setGrMkEnabled(e.target.checked)}
                />
                Markov activé
              </label>
              <label className="flex flex-col gap-0.5 text-xs text-fg-2">
                MC — essais max
                <input
                  className="rounded border border-border-1 bg-bg-1 px-2 py-1 font-mono text-[11px]"
                  value={grMcMaxTrials}
                  onChange={(e) => setGrMcMaxTrials(e.target.value)}
                />
              </label>
              <label className="flex flex-col gap-0.5 text-xs text-fg-2">
                Markov — états max
                <input
                  className="rounded border border-border-1 bg-bg-1 px-2 py-1 font-mono text-[11px]"
                  value={grMkMaxStates}
                  onChange={(e) => setGrMkMaxStates(e.target.value)}
                />
              </label>
            </div>
            <button
              type="button"
              className="mb-4 rounded bg-accent px-2 py-1 text-xs text-bg-0"
              onClick={() => void onSaveGr()}
            >
              Enregistrer garde-fous
            </button>

            <h3 className="mb-1 font-medium">Monte Carlo</h3>
            <div className="mb-2 flex flex-wrap gap-2">
              <input
                className="w-24 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                value={mcFilter}
                onChange={(e) => setMcFilter(e.target.value)}
              />
              <button
                type="button"
                className="rounded border border-border-1 px-2 py-1 text-xs"
                onClick={() => void loadMcList()}
              >
                Actualiser
              </button>
            </div>
            <div className="mb-2 flex flex-wrap gap-2">
              <input
                className="w-14 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                value={mcEq}
                onChange={(e) => setMcEq(e.target.value)}
              />
              <input
                className="min-w-[6rem] rounded border border-border-1 bg-bg-0 px-2 py-1 text-xs"
                value={mcTitle}
                onChange={(e) => setMcTitle(e.target.value)}
              />
              <input
                className="w-20 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                title="essais"
                value={mcTrials}
                onChange={(e) => setMcTrials(e.target.value)}
              />
              <input
                className="w-20 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                title="graine"
                value={mcSeed}
                onChange={(e) => setMcSeed(e.target.value)}
              />
              <button
                type="button"
                className="rounded bg-accent px-2 py-1 text-xs text-bg-0"
                onClick={() => void onCreateMc()}
              >
                Créer
              </button>
              <button
                type="button"
                className="rounded border border-border-1 px-2 py-1 text-xs"
                disabled={mcSel == null}
                onClick={() => void onUpdateMc()}
              >
                Enregistrer
              </button>
              <button
                type="button"
                className="rounded border border-border-1 px-2 py-1 text-xs"
                disabled={mcSel == null}
                onClick={() => void onEvalMc()}
              >
                Évaluer
              </button>
              <button
                type="button"
                className="rounded border border-red-600/50 px-2 py-1 text-xs text-red-600"
                disabled={mcSel == null}
                onClick={() => void onDeleteMc()}
              >
                Suppr.
              </button>
            </div>
            <table className="mb-2 w-full border-collapse text-left text-[10px]">
              <thead>
                <tr className="border-b border-border-1">
                  <th className="py-1 pr-2">id</th>
                  <th className="py-1 pr-2">essais</th>
                  <th className="py-1 pr-2">titre</th>
                </tr>
              </thead>
              <tbody>
                {mcRows.map((r) => (
                  <tr
                    key={r.id}
                    className={`cursor-pointer border-b border-border-1/60 ${mcSel?.id === r.id ? "bg-accent/10" : ""}`}
                    onClick={() => {
                      setMcSel(r);
                      applyMcUniformFromGraphJson(r.graph_json);
                      setMcTrials(String(r.trials));
                      setMcSeed(r.seed != null ? String(r.seed) : "");
                    }}
                  >
                    <td className="py-1 pr-2 font-mono">{r.id}</td>
                    <td className="py-1 pr-2">{r.trials}</td>
                    <td className="py-1 pr-2">{r.title}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <div className="mb-2 rounded border border-border-1/60 bg-bg-0 p-2">
              <p className="mb-2 text-[11px] text-fg-2">Échantillonnage uniforme (graph_json)</p>
              <div className="flex flex-wrap gap-2">
                <label className="flex flex-col text-xs text-fg-2">
                  min
                  <input
                    className="mt-0.5 w-24 rounded border border-border-1 bg-bg-1 px-2 py-1 font-mono"
                    value={mcUniformLow}
                    onChange={(e) => patchMcUniform(e.target.value, mcUniformHigh)}
                  />
                </label>
                <label className="flex flex-col text-xs text-fg-2">
                  max
                  <input
                    className="mt-0.5 w-24 rounded border border-border-1 bg-bg-1 px-2 py-1 font-mono"
                    value={mcUniformHigh}
                    onChange={(e) => patchMcUniform(mcUniformLow, e.target.value)}
                  />
                </label>
              </div>
              <details className="mt-2 text-xs text-fg-2">
                <summary className="cursor-pointer select-none text-fg-2">
                  JSON brut (avancé)
                </summary>
                <textarea
                  className="mt-1 h-20 w-full rounded border border-border-1 bg-bg-1 p-2 font-mono text-[10px]"
                  value={mcJson}
                  onChange={(e) => applyMcUniformFromGraphJson(e.target.value)}
                />
              </details>
            </div>
            {mcSel != null ? (
              <pre className="max-h-20 overflow-auto rounded border border-border-1/60 bg-bg-0 p-2 text-[9px] text-fg-2">
                {mcSel.result_json}
              </pre>
            ) : null}

            <h3 className="mb-1 mt-4 font-medium">Markov (DTMC)</h3>
            <div className="mb-2 flex flex-wrap gap-2">
              <input
                className="w-24 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                value={mkFilter}
                onChange={(e) => setMkFilter(e.target.value)}
              />
              <button
                type="button"
                className="rounded border border-border-1 px-2 py-1 text-xs"
                onClick={() => void loadMkList()}
              >
                Actualiser
              </button>
            </div>
            <div className="mb-2 flex flex-wrap gap-2">
              <input
                className="w-14 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
                value={mkEq}
                onChange={(e) => setMkEq(e.target.value)}
              />
              <input
                className="min-w-[8rem] flex-1 rounded border border-border-1 bg-bg-0 px-2 py-1 text-xs"
                value={mkTitle}
                onChange={(e) => setMkTitle(e.target.value)}
              />
              <button
                type="button"
                className="rounded bg-accent px-2 py-1 text-xs text-bg-0"
                onClick={() => void onCreateMk()}
              >
                Créer
              </button>
              <button
                type="button"
                className="rounded border border-border-1 px-2 py-1 text-xs"
                disabled={mkSel == null}
                onClick={() => void onUpdateMk()}
              >
                Enregistrer
              </button>
              <button
                type="button"
                className="rounded border border-border-1 px-2 py-1 text-xs"
                disabled={mkSel == null}
                onClick={() => void onEvalMk()}
              >
                Évaluer
              </button>
              <button
                type="button"
                className="rounded border border-red-600/50 px-2 py-1 text-xs text-red-600"
                disabled={mkSel == null}
                onClick={() => void onDeleteMk()}
              >
                Suppr.
              </button>
            </div>
            <table className="mb-2 w-full border-collapse text-left text-[10px]">
              <thead>
                <tr className="border-b border-border-1">
                  <th className="py-1 pr-2">id</th>
                  <th className="py-1 pr-2">titre</th>
                </tr>
              </thead>
              <tbody>
                {mkRows.map((r) => (
                  <tr
                    key={r.id}
                    className={`cursor-pointer border-b border-border-1/60 ${mkSel?.id === r.id ? "bg-accent/10" : ""}`}
                    onClick={() => {
                      setMkSel(r);
                      applyMkDiscreteFromGraphJson(r.graph_json);
                    }}
                  >
                    <td className="py-1 pr-2 font-mono">{r.id}</td>
                    <td className="py-1 pr-2">{r.title}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <div className="mb-2 rounded border border-border-1/60 bg-bg-0 p-2">
              <p className="mb-2 text-[11px] text-fg-2">Chaîne de Markov discrète (2×2)</p>
              <div className="mb-2 flex flex-wrap gap-2">
                <label className="flex flex-col text-xs text-fg-2">
                  État 1
                  <input
                    className="mt-0.5 w-20 rounded border border-border-1 bg-bg-1 px-2 py-1 font-mono"
                    value={mkS0}
                    onChange={(e) =>
                      patchMkDiscrete2({
                        s0: e.target.value,
                        s1: mkS1,
                        m00: mk00,
                        m01: mk01,
                        m10: mk10,
                        m11: mk11,
                      })
                    }
                  />
                </label>
                <label className="flex flex-col text-xs text-fg-2">
                  État 2
                  <input
                    className="mt-0.5 w-20 rounded border border-border-1 bg-bg-1 px-2 py-1 font-mono"
                    value={mkS1}
                    onChange={(e) =>
                      patchMkDiscrete2({
                        s0: mkS0,
                        s1: e.target.value,
                        m00: mk00,
                        m01: mk01,
                        m10: mk10,
                        m11: mk11,
                      })
                    }
                  />
                </label>
              </div>
              <div className="inline-grid grid-cols-[auto_1fr_1fr] gap-1 text-xs">
                <span />
                <span className="text-center font-mono text-fg-2">→1</span>
                <span className="text-center font-mono text-fg-2">→2</span>
                <span className="self-center font-mono text-fg-2">1→</span>
                <input
                  className="w-full rounded border border-border-1 bg-bg-1 px-1 py-0.5 font-mono"
                  value={mk00}
                  onChange={(e) =>
                    patchMkDiscrete2({
                      s0: mkS0,
                      s1: mkS1,
                      m00: e.target.value,
                      m01: mk01,
                      m10: mk10,
                      m11: mk11,
                    })
                  }
                />
                <input
                  className="w-full rounded border border-border-1 bg-bg-1 px-1 py-0.5 font-mono"
                  value={mk01}
                  onChange={(e) =>
                    patchMkDiscrete2({
                      s0: mkS0,
                      s1: mkS1,
                      m00: mk00,
                      m01: e.target.value,
                      m10: mk10,
                      m11: mk11,
                    })
                  }
                />
                <span className="self-center font-mono text-fg-2">2→</span>
                <input
                  className="w-full rounded border border-border-1 bg-bg-1 px-1 py-0.5 font-mono"
                  value={mk10}
                  onChange={(e) =>
                    patchMkDiscrete2({
                      s0: mkS0,
                      s1: mkS1,
                      m00: mk00,
                      m01: mk01,
                      m10: e.target.value,
                      m11: mk11,
                    })
                  }
                />
                <input
                  className="w-full rounded border border-border-1 bg-bg-1 px-1 py-0.5 font-mono"
                  value={mk11}
                  onChange={(e) =>
                    patchMkDiscrete2({
                      s0: mkS0,
                      s1: mkS1,
                      m00: mk00,
                      m01: mk01,
                      m10: mk10,
                      m11: e.target.value,
                    })
                  }
                />
              </div>
              <details className="mt-2 text-xs text-fg-2">
                <summary className="cursor-pointer select-none text-fg-2">
                  JSON brut (avancé)
                </summary>
                <textarea
                  className="mt-1 h-20 w-full rounded border border-border-1 bg-bg-1 p-2 font-mono text-[10px]"
                  value={mkJson}
                  onChange={(e) => applyMkDiscreteFromGraphJson(e.target.value)}
                />
              </details>
            </div>
            {mkSel != null ? (
              <pre className="max-h-24 overflow-auto rounded border border-border-1/60 bg-bg-0 p-2 text-[9px] text-fg-2">
                {mkSel.result_json}
              </pre>
            ) : null}
          </section>
        </>
      ) : null}

      {ramHubTab === "quality" ? (
        <section
          id="ram-data-quality"
          className="mb-6 scroll-mt-4 rounded border border-border-1 bg-bg-1 p-3"
        >
          <h2 className="mb-2 font-medium">Qualité des données RAM</h2>
          <p className="mb-2 text-fg-2">
            Badges (vert si score ≥ 0,85 et aucun code bloquant). Drill-through : OT sans mode de
            défaillance, équipements sans exposition 90j.
          </p>
          <p className="mb-3 text-fg-2 text-xs">
            Intervalle de confiance (Wilson, taux de répétition) : phase 2 (placeholder).
          </p>
          <div className="mb-3 flex flex-wrap items-end gap-2 border-b border-border-1 pb-3">
            <input
              className="w-36 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono text-xs"
              placeholder="filtrer équip. id"
              value={dqEquipFilter}
              onChange={(e) => setDqEquipFilter(e.target.value)}
            />
            <button
              type="button"
              className="rounded border border-border-1 px-2 py-1 text-fg-2 hover:text-fg-1"
              onClick={() => void loadDqIssues()}
            >
              Actualiser issues
            </button>
            <button
              type="button"
              className="rounded border border-border-1 px-2 py-1 text-fg-2 hover:text-fg-1"
              onClick={() => void loadDqDrill()}
            >
              Charger drill-through
            </button>
          </div>
          <div className="mb-3 flex flex-wrap items-center gap-2">
            <span className="text-fg-2">Badge équipement</span>
            <input
              className="w-24 rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono"
              value={dqBadgeEquip}
              onChange={(e) => setDqBadgeEquip(e.target.value)}
            />
            <button
              type="button"
              className="rounded border border-border-1 px-2 py-1 text-fg-2 hover:text-fg-1"
              onClick={() => void loadDqBadge()}
            >
              Actualiser badge
            </button>
            {dqBadge ? (
              <span
                className={
                  dqBadge.badge === "green"
                    ? "font-medium text-green-600"
                    : dqBadge.badge === "red"
                      ? "font-medium text-red-600"
                      : "font-medium text-amber-600"
                }
              >
                {dqBadge.badge.toUpperCase()} — score KPI:{" "}
                {dqBadge.data_quality_score != null ? dqBadge.data_quality_score.toFixed(2) : "—"}
                {dqBadge.blocking_issue_codes.length > 0
                  ? ` — bloquants: ${dqBadge.blocking_issue_codes.join(", ")}`
                  : ""}
              </span>
            ) : null}
          </div>
          <table className="mb-4 w-full border-collapse text-left text-xs">
            <thead>
              <tr className="border-b border-border-1">
                <th className="py-1 pr-2">équip.</th>
                <th className="py-1 pr-2">code</th>
                <th className="py-1 pr-2">gravité</th>
                <th className="py-1 pr-2">lien</th>
                <th className="py-1 pr-2">masquer</th>
              </tr>
            </thead>
            <tbody>
              {dqIssues.map((d) => (
                <tr
                  key={`${d.equipment_id}-${d.issue_code}`}
                  className="border-b border-border-1/60"
                >
                  <td className="py-1 pr-2">{d.equipment_id}</td>
                  <td className="py-1 pr-2 font-mono">{d.issue_code}</td>
                  <td className="py-1 pr-2">{d.severity}</td>
                  <td
                    className="max-w-[12rem] truncate py-1 pr-2 font-mono text-[10px]"
                    title={d.remediation_url}
                  >
                    {d.remediation_url}
                  </td>
                  <td className="py-1 pr-2">
                    <button
                      type="button"
                      className="text-fg-2 underline hover:text-fg-1"
                      onClick={() => void onDismissDq(d)}
                    >
                      masquer
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <h3 className="mb-1 font-medium">OT fermés sans mode ISO (drill)</h3>
          <table className="mb-4 w-full border-collapse text-left text-xs">
            <thead>
              <tr className="border-b border-border-1">
                <th className="py-1 pr-2">WO</th>
                <th className="py-1 pr-2">équip.</th>
                <th className="py-1 pr-2">type</th>
                <th className="py-1 pr-2">fermé</th>
              </tr>
            </thead>
            <tbody>
              {dqWos.map((w) => (
                <tr key={w.work_order_id} className="border-b border-border-1/60">
                  <td className="py-1 pr-2 font-mono">{w.work_order_id}</td>
                  <td className="py-1 pr-2">{w.equipment_id}</td>
                  <td className="py-1 pr-2">{w.type_code}</td>
                  <td className="py-1 pr-2 font-mono">{w.closed_at ?? "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <h3 className="mb-1 font-medium">Équipements sans exposition runtime (90 jours)</h3>
          <table className="w-full border-collapse text-left text-xs">
            <thead>
              <tr className="border-b border-border-1">
                <th className="py-1 pr-2">id</th>
                <th className="py-1 pr-2">nom</th>
              </tr>
            </thead>
            <tbody>
              {dqExp.map((x) => (
                <tr key={x.equipment_id} className="border-b border-border-1/60">
                  <td className="py-1 pr-2 font-mono">{x.equipment_id}</td>
                  <td className="py-1 pr-2">{x.equipment_name}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      ) : null}

      {ramHubTab === "failures" ? (
        <section className="rounded border border-border-1 bg-bg-1 p-3">
          <h2 className="mb-2 font-medium">Codes ({selectedHid ?? "—"})</h2>
          <div className="mb-2 flex flex-wrap gap-2">
            <input
              className="rounded border border-border-1 bg-bg-0 px-2 py-1 font-mono"
              placeholder="code"
              value={cCode}
              onChange={(e) => setCCode(e.target.value)}
            />
            <input
              className="rounded border border-border-1 bg-bg-0 px-2 py-1"
              placeholder="libellé"
              value={cLabel}
              onChange={(e) => setCLabel(e.target.value)}
            />
            <select
              className="rounded border border-border-1 bg-bg-0 px-2 py-1"
              value={cType}
              onChange={(e) => setCType(e.target.value)}
            >
              <option value="class">class</option>
              <option value="mode">mode</option>
              <option value="mechanism">mechanism</option>
              <option value="cause">cause</option>
              <option value="effect">effect</option>
              <option value="remedy">remedy</option>
            </select>
            <button
              type="button"
              className="rounded bg-accent px-3 py-1 text-bg-0"
              disabled={selectedHid == null}
              onClick={() => void onAddCode()}
            >
              Ajouter code
            </button>
          </div>
          <table className="w-full border-collapse text-left text-xs">
            <thead>
              <tr className="border-b border-border-1">
                <th className="py-1 pr-2">code</th>
                <th className="py-1 pr-2">type</th>
                <th className="py-1 pr-2">libellé</th>
                <th className="py-1 pr-2">actif</th>
                <th className="py-1" />
              </tr>
            </thead>
            <tbody>
              {codes.map((c) => (
                <tr key={c.id} className="border-b border-border-1/60">
                  <td className="py-1 pr-2 font-mono">{c.code}</td>
                  <td className="py-1 pr-2">{c.code_type}</td>
                  <td className="py-1 pr-2">{c.label}</td>
                  <td className="py-1 pr-2">{c.is_active ? "oui" : "non"}</td>
                  <td className="py-1">
                    {c.is_active ? (
                      <button
                        type="button"
                        className="text-fg-2 underline hover:text-fg-1"
                        onClick={() => void onDeactivate(c)}
                      >
                        désactiver
                      </button>
                    ) : null}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      ) : null}
    </>
  );

  if (embedded) {
    return <div className="min-h-0 flex-1 p-4 text-sm text-fg-1">{workspace}</div>;
  }

  return (
    <ModulePageShell
      icon={LineChart}
      title="Fiabilité (RAMS) — taxonomie défaillance"
      description="PRD 6.10.1 — hiérarchies et codes ISO 14224 (aperçu local)."
      bodyClassName="p-4 text-sm text-fg-1"
    >
      {workspace}
    </ModulePageShell>
  );
}
