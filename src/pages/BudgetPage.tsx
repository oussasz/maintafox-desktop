import { DollarSign, FileStack, GitBranch, RefreshCw, Save, ShieldCheck } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { PermissionGate } from "@/components/PermissionGate";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { usePermissions } from "@/hooks/use-permissions";
import {
  createBudgetActual,
  createBudgetAlertConfig,
  createBudgetCommitment,
  createBudgetLine,
  createBudgetSuccessorVersion,
  createBudgetVarianceReview,
  createBudgetVersion,
  createCostCenter,
  evaluateBudgetAlerts,
  exportApprovedReforecastsForErp,
  exportBudgetReportPack,
  exportPostedActualsForErp,
  generateBudgetForecasts,
  listIntegrationExceptions,
  listPostedExportBatches,
  recordErpExportBatch,
  updateIntegrationException,
  buildBudgetReportPack,
  importErpCostCenterMaster,
  listBudgetAlertConfigs,
  listBudgetAlertEvents,
  listBudgetActuals,
  listBudgetDashboardDrilldown,
  listBudgetDashboardRows,
  listBudgetCommitments,
  listBudgetForecasts,
  listBudgetVarianceReviews,
  listForecastRuns,
  postBudgetActual,
  reverseBudgetActual,
  acknowledgeBudgetAlert,
  transitionBudgetVarianceReview,
  listBudgetLines,
  listBudgetVersions,
  listCostCenters,
  transitionBudgetVersionLifecycle,
  updateBudgetLine,
  updateBudgetVersion,
  updateCostCenter,
} from "@/services/budget-service";
import { listOrgTree } from "@/services/org-node-service";
import { listCostOfFailure } from "@/services/reliability-service";
import { toErrorMessage } from "@/utils/errors";
import type {
  BudgetActual,
  BudgetAlertConfig,
  BudgetAlertEvent,
  BudgetReportPack,
  BudgetReportPackExport,
  BudgetCommitment,
  BudgetForecast,
  BudgetDashboardRow,
  BudgetDrilldownRow,
  BudgetLine,
  BudgetVarianceReview,
  BudgetVersion,
  CostCenter,
  ErpApprovedReforecastExportItem,
  ErpExportBatchResult,
  ErpPostedActualExportItem,
  IntegrationException,
  PostedExportBatch,
  CostOfFailureRow,
  ForecastRun,
  OrgTreeRow,
} from "@shared/ipc-types";

type CostCenterFormState = {
  code: string;
  name: string;
  entity_id: string;
  parent_cost_center_id: string;
  erp_external_id: string;
  is_active: boolean;
};

type VersionFormState = {
  fiscal_year: string;
  scenario_type: string;
  currency_code: string;
  title: string;
  planning_basis: string;
  source_basis_mix_json: string;
  labor_assumptions_json: string;
  baseline_reference: string;
  erp_external_ref: string;
};

type LineFormState = {
  cost_center_id: string;
  period_month: string;
  budget_bucket: string;
  planned_amount: string;
  source_basis: string;
  justification_note: string;
  asset_family: string;
  work_category: string;
  shutdown_package_ref: string;
  team_id: string;
  skill_pool_id: string;
  labor_lane: string;
};

const EMPTY_COST_CENTER_FORM: CostCenterFormState = {
  code: "",
  name: "",
  entity_id: "__none__",
  parent_cost_center_id: "__none__",
  erp_external_id: "",
  is_active: true,
};

const EMPTY_VERSION_FORM: VersionFormState = {
  fiscal_year: String(new Date().getUTCFullYear()),
  scenario_type: "approved",
  currency_code: "EUR",
  title: "",
  planning_basis: "",
  source_basis_mix_json: "",
  labor_assumptions_json: "",
  baseline_reference: "",
  erp_external_ref: "",
};

const EMPTY_LINE_FORM: LineFormState = {
  cost_center_id: "__none__",
  period_month: "__annual__",
  budget_bucket: "labor",
  planned_amount: "",
  source_basis: "",
  justification_note: "",
  asset_family: "",
  work_category: "",
  shutdown_package_ref: "",
  team_id: "__none__",
  skill_pool_id: "__none__",
  labor_lane: "regular",
};

function budgetStatusVariant(status: string): "secondary" | "outline" | "default" | "destructive" {
  if (status === "frozen") return "default";
  if (status === "approved") return "secondary";
  if (status === "closed" || status === "superseded") return "outline";
  return "secondary";
}

function decodeOptionalJson(raw: string | null): string {
  if (!raw) return "—";
  try {
    return JSON.stringify(JSON.parse(raw), null, 2);
  } catch {
    return raw;
  }
}

function parseOptionalNumber(value: string, noneValue = "__none__"): number | null {
  if (!value || value === noneValue) return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

export function BudgetPage() {
  const { canAny, can } = usePermissions();
  const canBudget = canAny("fin.budget", "fin.manage");
  const canPost = can("fin.post");
  const canReport = can("fin.report");

  const [costCenters, setCostCenters] = useState<CostCenter[]>([]);
  const [versions, setVersions] = useState<BudgetVersion[]>([]);
  const [lines, setLines] = useState<BudgetLine[]>([]);
  const [actuals, setActuals] = useState<BudgetActual[]>([]);
  const [commitments, setCommitments] = useState<BudgetCommitment[]>([]);
  const [forecasts, setForecasts] = useState<BudgetForecast[]>([]);
  const [forecastRuns, setForecastRuns] = useState<ForecastRun[]>([]);
  const [varianceReviews, setVarianceReviews] = useState<BudgetVarianceReview[]>([]);
  const [dashboardRows, setDashboardRows] = useState<BudgetDashboardRow[]>([]);
  const [drilldownRows, setDrilldownRows] = useState<BudgetDrilldownRow[]>([]);
  const [alertConfigs, setAlertConfigs] = useState<BudgetAlertConfig[]>([]);
  const [alertEvents, setAlertEvents] = useState<BudgetAlertEvent[]>([]);
  const [reportPack, setReportPack] = useState<BudgetReportPack | null>(null);
  const [reportExport, setReportExport] = useState<BudgetReportPackExport | null>(null);
  const [erpPostedPayload, setErpPostedPayload] = useState<ErpPostedActualExportItem[]>([]);
  const [erpReforecastPayload, setErpReforecastPayload] = useState<
    ErpApprovedReforecastExportItem[]
  >([]);
  const [postedExportBatches, setPostedExportBatches] = useState<PostedExportBatch[]>([]);
  const [integrationExceptions, setIntegrationExceptions] = useState<IntegrationException[]>([]);
  const [lastErpExportResult, setLastErpExportResult] = useState<ErpExportBatchResult | null>(null);
  const [erpTenantInput, setErpTenantInput] = useState("");
  const [selectedExceptionId, setSelectedExceptionId] = useState<number | null>(null);
  const [costOfFailureRows, setCostOfFailureRows] = useState<CostOfFailureRow[]>([]);
  const [orgNodes, setOrgNodes] = useState<OrgTreeRow[]>([]);

  const [selectedCostCenterId, setSelectedCostCenterId] = useState<number | null>(null);
  const [selectedVersionId, setSelectedVersionId] = useState<number | null>(null);
  const [selectedLineId, setSelectedLineId] = useState<number | null>(null);
  const [selectedActualId, setSelectedActualId] = useState<number | null>(null);
  const [selectedForecastId, setSelectedForecastId] = useState<number | null>(null);
  const [selectedVarianceReviewId, setSelectedVarianceReviewId] = useState<number | null>(null);
  const [selectedAlertEventId, setSelectedAlertEventId] = useState<number | null>(null);

  const [costCenterForm, setCostCenterForm] = useState<CostCenterFormState>(EMPTY_COST_CENTER_FORM);
  const [versionForm, setVersionForm] = useState<VersionFormState>(EMPTY_VERSION_FORM);
  const [lineForm, setLineForm] = useState<LineFormState>(EMPTY_LINE_FORM);

  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState("baseline");

  const selectedCostCenter = useMemo(
    () => costCenters.find((item) => item.id === selectedCostCenterId) ?? null,
    [costCenters, selectedCostCenterId],
  );
  const selectedVersion = useMemo(
    () => versions.find((item) => item.id === selectedVersionId) ?? null,
    [versions, selectedVersionId],
  );
  const selectedLine = useMemo(
    () => lines.find((item) => item.id === selectedLineId) ?? null,
    [lines, selectedLineId],
  );
  const selectedActual = useMemo(
    () => actuals.find((item) => item.id === selectedActualId) ?? null,
    [actuals, selectedActualId],
  );
  const selectedForecast = useMemo(
    () => forecasts.find((item) => item.id === selectedForecastId) ?? null,
    [forecasts, selectedForecastId],
  );
  const selectedVarianceReview = useMemo(
    () => varianceReviews.find((item) => item.id === selectedVarianceReviewId) ?? null,
    [varianceReviews, selectedVarianceReviewId],
  );
  const selectedAlertEvent = useMemo(
    () => alertEvents.find((item) => item.id === selectedAlertEventId) ?? null,
    [alertEvents, selectedAlertEventId],
  );
  const versionEditable = canBudget && selectedVersion?.status === "draft";

  async function loadBaseline() {
    setLoading(true);
    setError(null);
    try {
      const [loadedCenters, loadedVersions, loadedOrg] = await Promise.all([
        listCostCenters({ include_inactive: true }),
        listBudgetVersions({}),
        listOrgTree(),
      ]);
      setCostCenters(loadedCenters);
      setVersions(loadedVersions);
      setOrgNodes(loadedOrg.filter((item) => item.can_carry_cost_center));

      setSelectedCostCenterId((current) => current ?? loadedCenters[0]?.id ?? null);
      setSelectedVersionId((current) => current ?? loadedVersions[0]?.id ?? null);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  }

  async function loadVersionLines(versionId: number | null) {
    if (!versionId) {
      setLines([]);
      setSelectedLineId(null);
      return;
    }
    try {
      const loadedLines = await listBudgetLines({ budget_version_id: versionId });
      setLines(loadedLines);
      setSelectedLineId((current) =>
        current && loadedLines.some((item) => item.id === current) ? current : null,
      );
    } catch (err) {
      setError(toErrorMessage(err));
    }
  }

  async function loadVersionFinanceLayers(versionId: number | null) {
    if (!versionId) {
      setActuals([]);
      setCommitments([]);
      setForecasts([]);
      setForecastRuns([]);
      setVarianceReviews([]);
      setDashboardRows([]);
      setDrilldownRows([]);
      setAlertConfigs([]);
      setAlertEvents([]);
      setReportPack(null);
      setReportExport(null);
      setSelectedActualId(null);
      setSelectedForecastId(null);
      setSelectedVarianceReviewId(null);
      setSelectedAlertEventId(null);
      return;
    }
    try {
      const [
        loadedActuals,
        loadedCommitments,
        loadedForecasts,
        loadedRuns,
        loadedReviews,
        loadedDashboard,
        loadedDrilldown,
        loadedAlertConfigs,
        loadedAlertEvents,
      ] = await Promise.all([
        listBudgetActuals({ budget_version_id: versionId }),
        listBudgetCommitments({ budget_version_id: versionId }),
        listBudgetForecasts({ budget_version_id: versionId }),
        listForecastRuns(versionId),
        listBudgetVarianceReviews({ budget_version_id: versionId }),
        listBudgetDashboardRows({ budget_version_id: versionId }),
        listBudgetDashboardDrilldown({ budget_version_id: versionId }),
        listBudgetAlertConfigs({ budget_version_id: versionId }),
        listBudgetAlertEvents({ budget_version_id: versionId }),
      ]);
      setActuals(loadedActuals);
      setCommitments(loadedCommitments);
      setForecasts(loadedForecasts);
      setForecastRuns(loadedRuns);
      setVarianceReviews(loadedReviews);
      setDashboardRows(loadedDashboard);
      setDrilldownRows(loadedDrilldown);
      setAlertConfigs(loadedAlertConfigs);
      setAlertEvents(loadedAlertEvents);
      setSelectedActualId((current) =>
        current && loadedActuals.some((item) => item.id === current)
          ? current
          : (loadedActuals[0]?.id ?? null),
      );
      setSelectedForecastId((current) =>
        current && loadedForecasts.some((item) => item.id === current)
          ? current
          : (loadedForecasts[0]?.id ?? null),
      );
      setSelectedVarianceReviewId((current) =>
        current && loadedReviews.some((item) => item.id === current)
          ? current
          : (loadedReviews[0]?.id ?? null),
      );
      setSelectedAlertEventId((current) =>
        current && loadedAlertEvents.some((item) => item.id === current)
          ? current
          : (loadedAlertEvents[0]?.id ?? null),
      );
    } catch (err) {
      setError(toErrorMessage(err));
    }
  }

  useEffect(() => {
    void loadBaseline();
  }, []);

  useEffect(() => {
    if (activeTab !== "variance-erp") {
      return;
    }
    void (async () => {
      try {
        const rows = await listCostOfFailure({ limit: 48 });
        setCostOfFailureRows(rows);
      } catch (err) {
        setError(toErrorMessage(err));
      }
    })();
  }, [activeTab]);

  useEffect(() => {
    void loadVersionLines(selectedVersionId);
  }, [selectedVersionId]);

  useEffect(() => {
    void loadVersionFinanceLayers(selectedVersionId);
  }, [selectedVersionId]);

  useEffect(() => {
    if (!selectedCostCenter) {
      setCostCenterForm(EMPTY_COST_CENTER_FORM);
      return;
    }
    setCostCenterForm({
      code: selectedCostCenter.code,
      name: selectedCostCenter.name,
      entity_id: selectedCostCenter.entity_id ? String(selectedCostCenter.entity_id) : "__none__",
      parent_cost_center_id: selectedCostCenter.parent_cost_center_id
        ? String(selectedCostCenter.parent_cost_center_id)
        : "__none__",
      erp_external_id: selectedCostCenter.erp_external_id ?? "",
      is_active: selectedCostCenter.is_active === 1,
    });
  }, [selectedCostCenter]);

  useEffect(() => {
    if (!selectedVersion) {
      setVersionForm(EMPTY_VERSION_FORM);
      return;
    }
    setVersionForm({
      fiscal_year: String(selectedVersion.fiscal_year),
      scenario_type: selectedVersion.scenario_type,
      currency_code: selectedVersion.currency_code,
      title: selectedVersion.title ?? "",
      planning_basis: selectedVersion.planning_basis ?? "",
      source_basis_mix_json: selectedVersion.source_basis_mix_json ?? "",
      labor_assumptions_json: selectedVersion.labor_assumptions_json ?? "",
      baseline_reference: selectedVersion.baseline_reference ?? "",
      erp_external_ref: selectedVersion.erp_external_ref ?? "",
    });
  }, [selectedVersion]);

  useEffect(() => {
    if (!selectedLine) {
      setLineForm((current) => ({
        ...EMPTY_LINE_FORM,
        cost_center_id:
          selectedCostCenterId !== null
            ? String(selectedCostCenterId)
            : (current.cost_center_id ?? "__none__"),
      }));
      return;
    }
    setLineForm({
      cost_center_id: String(selectedLine.cost_center_id),
      period_month: selectedLine.period_month ? String(selectedLine.period_month) : "__annual__",
      budget_bucket: selectedLine.budget_bucket,
      planned_amount: String(selectedLine.planned_amount),
      source_basis: selectedLine.source_basis ?? "",
      justification_note: selectedLine.justification_note ?? "",
      asset_family: selectedLine.asset_family ?? "",
      work_category: selectedLine.work_category ?? "",
      shutdown_package_ref: selectedLine.shutdown_package_ref ?? "",
      team_id: selectedLine.team_id ? String(selectedLine.team_id) : "__none__",
      skill_pool_id: selectedLine.skill_pool_id ? String(selectedLine.skill_pool_id) : "__none__",
      labor_lane: selectedLine.labor_lane ?? "regular",
    });
  }, [selectedLine, selectedCostCenterId]);

  async function handleCreateCostCenter() {
    setSaving(true);
    setError(null);
    try {
      const created = await createCostCenter({
        code: costCenterForm.code,
        name: costCenterForm.name,
        entity_id: parseOptionalNumber(costCenterForm.entity_id),
        parent_cost_center_id: parseOptionalNumber(costCenterForm.parent_cost_center_id),
        erp_external_id: costCenterForm.erp_external_id || null,
        is_active: costCenterForm.is_active,
      });
      await loadBaseline();
      setSelectedCostCenterId(created.id);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleUpdateCostCenter() {
    if (!selectedCostCenter) return;
    setSaving(true);
    setError(null);
    try {
      const updated = await updateCostCenter(
        selectedCostCenter.id,
        selectedCostCenter.row_version,
        {
          code: costCenterForm.code,
          name: costCenterForm.name,
          entity_id: parseOptionalNumber(costCenterForm.entity_id),
          parent_cost_center_id: parseOptionalNumber(costCenterForm.parent_cost_center_id),
          erp_external_id: costCenterForm.erp_external_id || null,
          is_active: costCenterForm.is_active,
        },
      );
      await loadBaseline();
      setSelectedCostCenterId(updated.id);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleCreateVersion() {
    setSaving(true);
    setError(null);
    try {
      const created = await createBudgetVersion({
        fiscal_year: Number(versionForm.fiscal_year),
        scenario_type: versionForm.scenario_type,
        currency_code: versionForm.currency_code,
        title: versionForm.title || null,
        planning_basis: versionForm.planning_basis || null,
        source_basis_mix_json: versionForm.source_basis_mix_json || null,
        labor_assumptions_json: versionForm.labor_assumptions_json || null,
        baseline_reference: versionForm.baseline_reference || null,
        erp_external_ref: versionForm.erp_external_ref || null,
      });
      await loadBaseline();
      setSelectedVersionId(created.id);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleUpdateVersion() {
    if (!selectedVersion) return;
    setSaving(true);
    setError(null);
    try {
      const updated = await updateBudgetVersion(selectedVersion.id, selectedVersion.row_version, {
        currency_code: versionForm.currency_code,
        title: versionForm.title || null,
        planning_basis: versionForm.planning_basis || null,
        source_basis_mix_json: versionForm.source_basis_mix_json || null,
        labor_assumptions_json: versionForm.labor_assumptions_json || null,
        baseline_reference: versionForm.baseline_reference || null,
        erp_external_ref: versionForm.erp_external_ref || null,
      });
      await loadBaseline();
      setSelectedVersionId(updated.id);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleCreateSuccessor() {
    if (!selectedVersion) return;
    setSaving(true);
    setError(null);
    try {
      const successor = await createBudgetSuccessorVersion({
        source_version_id: selectedVersion.id,
        fiscal_year: Number(versionForm.fiscal_year),
        scenario_type: "reforecast",
        title: `${selectedVersion.title ?? "Baseline"} successor`,
        baseline_reference: versionForm.baseline_reference || null,
      });
      await loadBaseline();
      setSelectedVersionId(successor.id);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleTransitionVersion(nextStatus: string) {
    if (!selectedVersion) return;
    setSaving(true);
    setError(null);
    try {
      const updated = await transitionBudgetVersionLifecycle({
        version_id: selectedVersion.id,
        expected_row_version: selectedVersion.row_version,
        next_status: nextStatus,
      });
      await loadBaseline();
      setSelectedVersionId(updated.id);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleCreateLine() {
    if (!selectedVersionId) return;
    setSaving(true);
    setError(null);
    try {
      await createBudgetLine({
        budget_version_id: selectedVersionId,
        cost_center_id: Number(lineForm.cost_center_id),
        period_month: parseOptionalNumber(lineForm.period_month, "__annual__"),
        budget_bucket: lineForm.budget_bucket,
        planned_amount: Number(lineForm.planned_amount),
        source_basis: lineForm.source_basis || null,
        justification_note: lineForm.justification_note || null,
        asset_family: lineForm.asset_family || null,
        work_category: lineForm.work_category || null,
        shutdown_package_ref: lineForm.shutdown_package_ref || null,
        team_id: parseOptionalNumber(lineForm.team_id),
        skill_pool_id: parseOptionalNumber(lineForm.skill_pool_id),
        labor_lane: lineForm.labor_lane || null,
      });
      await loadVersionLines(selectedVersionId);
      setSelectedLineId(null);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleUpdateLine() {
    if (!selectedLine) return;
    setSaving(true);
    setError(null);
    try {
      const updated = await updateBudgetLine(selectedLine.id, selectedLine.row_version, {
        period_month: parseOptionalNumber(lineForm.period_month, "__annual__"),
        budget_bucket: lineForm.budget_bucket,
        planned_amount: Number(lineForm.planned_amount),
        source_basis: lineForm.source_basis || null,
        justification_note: lineForm.justification_note || null,
        asset_family: lineForm.asset_family || null,
        work_category: lineForm.work_category || null,
        shutdown_package_ref: lineForm.shutdown_package_ref || null,
        team_id: parseOptionalNumber(lineForm.team_id),
        skill_pool_id: parseOptionalNumber(lineForm.skill_pool_id),
        labor_lane: lineForm.labor_lane || null,
      });
      await loadVersionLines(selectedVersionId);
      setSelectedLineId(updated.id);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleCreateActualFromSelection(postDirectly: boolean) {
    if (!selectedVersion || !selectedLine) return;
    setSaving(true);
    setError(null);
    try {
      await createBudgetActual({
        budget_version_id: selectedVersion.id,
        cost_center_id: selectedLine.cost_center_id,
        period_month: selectedLine.period_month,
        budget_bucket: selectedLine.budget_bucket,
        amount_source: selectedLine.planned_amount,
        source_currency: selectedVersion.currency_code,
        amount_base: selectedLine.planned_amount,
        base_currency: selectedVersion.currency_code,
        source_type: selectedLine.source_basis ?? "manual_adjustment",
        source_id: `LINE-${selectedLine.id}`,
        work_order_id: null,
        equipment_id: null,
        posting_status: postDirectly ? "posted" : "provisional",
        provisional_reason: postDirectly ? null : "Awaiting posting approval",
        team_id: selectedLine.team_id,
        rate_card_lane: selectedLine.labor_lane,
      });
      await loadVersionFinanceLayers(selectedVersion.id);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function handlePostSelectedActual() {
    if (!selectedActual || selectedActual.posting_status !== "provisional") return;
    setSaving(true);
    setError(null);
    try {
      await postBudgetActual({
        actual_id: selectedActual.id,
        expected_row_version: selectedActual.row_version,
      });
      await loadVersionFinanceLayers(selectedVersionId);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleReverseSelectedActual() {
    if (!selectedActual || selectedActual.posting_status !== "posted") return;
    setSaving(true);
    setError(null);
    try {
      await reverseBudgetActual({
        actual_id: selectedActual.id,
        expected_row_version: selectedActual.row_version,
        reason: "Corrective reversal",
      });
      await loadVersionFinanceLayers(selectedVersionId);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleCreateCommitmentFromSelection() {
    if (!selectedVersion || !selectedLine) return;
    setSaving(true);
    setError(null);
    try {
      await createBudgetCommitment({
        budget_version_id: selectedVersion.id,
        cost_center_id: selectedLine.cost_center_id,
        period_month: selectedLine.period_month,
        budget_bucket: selectedLine.budget_bucket,
        commitment_type: selectedLine.shutdown_package_ref ? "shutdown" : "po",
        source_type: selectedLine.shutdown_package_ref ? "shutdown_package" : "purchase_order",
        source_id: selectedLine.shutdown_package_ref ?? `PO-LINE-${selectedLine.id}`,
        obligation_amount: selectedLine.planned_amount,
        source_currency: selectedVersion.currency_code,
        base_amount: selectedLine.planned_amount,
        base_currency: selectedVersion.currency_code,
        commitment_status: "open",
        work_order_id: null,
        planning_commitment_ref: selectedLine.source_basis,
        explainability_note: "Seeded from baseline line demand",
      });
      await loadVersionFinanceLayers(selectedVersion.id);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleGenerateForecasts() {
    if (!selectedVersion) return;
    setSaving(true);
    setError(null);
    try {
      await generateBudgetForecasts({
        budget_version_id: selectedVersion.id,
        idempotency_key: `${selectedVersion.id}-full-horizon-v1`,
        scope_signature: `version:${selectedVersion.id}:full`,
        include_pm_occurrence: true,
        include_backlog_demand: true,
        include_shutdown_demand: true,
        include_planning_demand: true,
        include_burn_rate: true,
      });
      await loadVersionFinanceLayers(selectedVersion.id);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleCreateVarianceReview() {
    if (!selectedVersion || !selectedLine) return;
    setSaving(true);
    setError(null);
    try {
      const varianceAmount =
        selectedLine.planned_amount * 0.12 * (selectedLine.budget_bucket === "labor" ? 1.1 : 1);
      const variancePct =
        selectedLine.planned_amount === 0
          ? 0
          : (varianceAmount / selectedLine.planned_amount) * 100;
      await createBudgetVarianceReview({
        budget_version_id: selectedVersion.id,
        cost_center_id: selectedLine.cost_center_id,
        period_month: selectedLine.period_month,
        budget_bucket: selectedLine.budget_bucket,
        variance_amount: varianceAmount,
        variance_pct: variancePct,
        driver_code: selectedLine.budget_bucket === "labor" ? "labor_overrun" : "estimate_error",
        action_owner_id: 1,
        review_commentary:
          "Variance opened from dashboard drilldown with accountable owner and coded driver.",
        snapshot_context_json: JSON.stringify({
          opened_from_line_id: selectedLine.id,
          selected_version_status: selectedVersion.status,
          planned_amount: selectedLine.planned_amount,
          commitments: commitments.filter(
            (item) => item.cost_center_id === selectedLine.cost_center_id,
          ).length,
          actuals: actuals.filter((item) => item.cost_center_id === selectedLine.cost_center_id)
            .length,
          forecasts: forecasts.filter((item) => item.cost_center_id === selectedLine.cost_center_id)
            .length,
        }),
      });
      await loadVersionFinanceLayers(selectedVersion.id);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleAdvanceVarianceReview(nextStatus: string) {
    if (!selectedVarianceReview) return;
    setSaving(true);
    setError(null);
    try {
      await transitionBudgetVarianceReview({
        review_id: selectedVarianceReview.id,
        expected_row_version: selectedVarianceReview.row_version,
        next_status: nextStatus,
        review_commentary:
          nextStatus === "closed"
            ? "Disposition completed with corrective action owner accountability."
            : "Review status advanced with evidence-backed commentary.",
        reopen_reason:
          nextStatus === "open"
            ? "Material posting change triggered automatic reassessment."
            : null,
      });
      await loadVersionFinanceLayers(selectedVersionId);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleImportErpMaster() {
    setSaving(true);
    setError(null);
    try {
      await importErpCostCenterMaster({
        import_batch_id: `ui-batch-${Date.now()}`,
        records: costCenters.map((item) => ({
          external_code: `ERP-${item.code}`,
          external_name: item.name,
          local_cost_center_code: item.code,
          is_active: item.is_active === 1,
        })),
      });
      if (selectedVersionId) {
        await loadVersionFinanceLayers(selectedVersionId);
      }
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleRefreshErpPayloads() {
    setSaving(true);
    setError(null);
    try {
      const [posted, reforecasts, batches, exceptions, cof] = await Promise.all([
        exportPostedActualsForErp(),
        exportApprovedReforecastsForErp(),
        listPostedExportBatches({ limit: 30 }),
        listIntegrationExceptions({ limit: 100 }),
        listCostOfFailure({ limit: 48 }),
      ]);
      setErpPostedPayload(posted);
      setErpReforecastPayload(reforecasts);
      setPostedExportBatches(batches);
      setIntegrationExceptions(exceptions);
      setCostOfFailureRows(cof);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleRecordSignedExport(kind: "posted_actuals" | "approved_reforecasts") {
    setSaving(true);
    setError(null);
    try {
      const tenant = erpTenantInput.trim();
      const result = await recordErpExportBatch({
        export_kind: kind,
        tenant_id: tenant.length > 0 ? tenant : null,
      });
      setLastErpExportResult(result);
      const [batches, exceptions] = await Promise.all([
        listPostedExportBatches({ limit: 30 }),
        listIntegrationExceptions({ limit: 100 }),
      ]);
      setPostedExportBatches(batches);
      setIntegrationExceptions(exceptions);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleMergeSelectedException() {
    const ex = integrationExceptions.find((item) => item.id === selectedExceptionId);
    if (!ex) return;
    setSaving(true);
    setError(null);
    try {
      await updateIntegrationException(ex.id, ex.row_version, {
        resolution_status: "merged",
        external_value_snapshot: ex.maintafox_value_snapshot,
        rejection_code: null,
      });
      setIntegrationExceptions(await listIntegrationExceptions({ limit: 100 }));
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleEnsureDefaultAlertRules() {
    if (!selectedVersionId) return;
    setSaving(true);
    setError(null);
    try {
      if (alertConfigs.length === 0) {
        await createBudgetAlertConfig({
          budget_version_id: selectedVersionId,
          alert_type: "threshold_80",
          threshold_pct: 80,
          dedupe_window_minutes: 240,
          requires_ack: true,
          recipient_user_id: 1,
        });
      }
      await loadVersionFinanceLayers(selectedVersionId);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleEvaluateAlerts() {
    if (!selectedVersionId) return;
    setSaving(true);
    setError(null);
    try {
      const result = await evaluateBudgetAlerts({
        budget_version_id: selectedVersionId,
        emit_notifications: true,
      });
      setAlertEvents(result.events);
      setSelectedAlertEventId(result.events[0]?.id ?? null);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleAcknowledgeSelectedAlert() {
    if (!selectedAlertEvent) return;
    setSaving(true);
    setError(null);
    try {
      const updated = await acknowledgeBudgetAlert({
        alert_event_id: selectedAlertEvent.id,
        note: "Acknowledged from budget controls workspace",
      });
      setAlertEvents((current) => current.map((item) => (item.id === updated.id ? updated : item)));
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleBuildReportPack() {
    if (!selectedVersionId) return;
    setSaving(true);
    setError(null);
    try {
      const built = await buildBudgetReportPack({ budget_version_id: selectedVersionId });
      setReportPack(built);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleExportReportPack(format: "pdf" | "excel") {
    if (!selectedVersionId) return;
    setSaving(true);
    setError(null);
    try {
      const exported = await exportBudgetReportPack({
        filter: { budget_version_id: selectedVersionId },
        format,
      });
      setReportExport(exported);
      setReportPack(exported.report);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  return (
    <ModulePageShell
      icon={DollarSign}
      title="Budget baseline authoring"
      description="Govern cost center hierarchy, versioned budget baselines, and traceable period-aware budget lines."
      actions={
        <>
          <Button
            variant="outline"
            size="sm"
            className="gap-1.5"
            onClick={() => void loadBaseline()}
            disabled={loading || saving}
          >
            <RefreshCw className="h-3.5 w-3.5" />
            Refresh
          </Button>
          <PermissionGate permission="fin.report">
            <Button variant="secondary" size="sm" disabled>
              <ShieldCheck className="mr-1.5 h-3.5 w-3.5" />
              Report-ready metadata enabled
            </Button>
          </PermissionGate>
        </>
      }
      bodyClassName="space-y-6 p-4"
    >
      {error ? (
        <Card className="border-destructive/40">
          <CardContent className="pt-6 text-sm text-destructive">{error}</CardContent>
        </Card>
      ) : null}

      <div className="grid gap-4 md:grid-cols-4">
        <Card>
          <CardHeader className="pb-2">
            <CardDescription>Cost centers</CardDescription>
            <CardTitle className="text-xl">{costCenters.length}</CardTitle>
          </CardHeader>
          <CardContent className="flex items-center gap-2 text-xs text-muted-foreground">
            <GitBranch className="h-4 w-4" />
            Active + inactive hierarchy
          </CardContent>
        </Card>
        <Card>
          <CardHeader className="pb-2">
            <CardDescription>Budget versions</CardDescription>
            <CardTitle className="text-xl">{versions.length}</CardTitle>
          </CardHeader>
          <CardContent className="flex items-center gap-2 text-xs text-muted-foreground">
            <FileStack className="h-4 w-4" />
            Draft through frozen lifecycle
          </CardContent>
        </Card>
        <Card>
          <CardHeader className="pb-2">
            <CardDescription>Budget lines</CardDescription>
            <CardTitle className="text-xl">{lines.length}</CardTitle>
          </CardHeader>
          <CardContent className="flex items-center gap-2 text-xs text-muted-foreground">
            <DollarSign className="h-4 w-4" />
            Selected baseline detail
          </CardContent>
        </Card>
        <Card>
          <CardHeader className="pb-2">
            <CardDescription>Permissions</CardDescription>
            <CardTitle className="text-xl">{canBudget ? "Editable" : "View only"}</CardTitle>
          </CardHeader>
          <CardContent className="text-xs text-muted-foreground">
            {canReport ? "Governed export/report scope present." : "No export scope detected."}
          </CardContent>
        </Card>
      </div>

      <Tabs value={activeTab} onValueChange={setActiveTab}>
        <TabsList>
          <TabsTrigger value="baseline">Baseline workspace</TabsTrigger>
          <TabsTrigger value="explainability">Baseline explainability</TabsTrigger>
          <TabsTrigger value="variance-erp">Variance + ERP alignment</TabsTrigger>
        </TabsList>

        <TabsContent value="baseline" className="space-y-6">
          <div className="grid gap-6 xl:grid-cols-[1.1fr_1.2fr_1.5fr]">
            <Card>
              <CardHeader>
                <CardTitle className="text-xl">Cost center hierarchy</CardTitle>
                <CardDescription>
                  Select a node, then update or create governed children.
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="max-h-72 overflow-auto rounded-md border">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>Code</TableHead>
                        <TableHead>Name</TableHead>
                        <TableHead>Status</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {costCenters.map((item) => (
                        <TableRow
                          key={item.id}
                          className={item.id === selectedCostCenterId ? "bg-muted" : ""}
                          onClick={() => setSelectedCostCenterId(item.id)}
                        >
                          <TableCell className="font-medium">{item.code}</TableCell>
                          <TableCell>{item.name}</TableCell>
                          <TableCell>
                            <Badge variant={item.is_active === 1 ? "secondary" : "outline"}>
                              {item.is_active === 1 ? "Active" : "Inactive"}
                            </Badge>
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>

                <div className="grid gap-3">
                  <div className="grid gap-2">
                    <Label htmlFor="cc-code">Code</Label>
                    <Input
                      id="cc-code"
                      value={costCenterForm.code}
                      onChange={(e) =>
                        setCostCenterForm((current) => ({ ...current, code: e.target.value }))
                      }
                      disabled={!canBudget || saving}
                    />
                  </div>
                  <div className="grid gap-2">
                    <Label htmlFor="cc-name">Name</Label>
                    <Input
                      id="cc-name"
                      value={costCenterForm.name}
                      onChange={(e) =>
                        setCostCenterForm((current) => ({ ...current, name: e.target.value }))
                      }
                      disabled={!canBudget || saving}
                    />
                  </div>
                  <div className="grid gap-2">
                    <Label>Entity</Label>
                    <Select
                      value={costCenterForm.entity_id}
                      onValueChange={(value) =>
                        setCostCenterForm((current) => ({ ...current, entity_id: value }))
                      }
                      disabled={!canBudget || saving}
                    >
                      <SelectTrigger>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="__none__">No entity binding</SelectItem>
                        {orgNodes.map((item) => (
                          <SelectItem key={item.node.id} value={String(item.node.id)}>
                            {item.node.code
                              ? `${item.node.name} (${item.node.code})`
                              : item.node.name}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                  <div className="grid gap-2">
                    <Label>Parent cost center</Label>
                    <Select
                      value={costCenterForm.parent_cost_center_id}
                      onValueChange={(value) =>
                        setCostCenterForm((current) => ({
                          ...current,
                          parent_cost_center_id: value,
                        }))
                      }
                      disabled={!canBudget || saving}
                    >
                      <SelectTrigger>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="__none__">No parent</SelectItem>
                        {costCenters
                          .filter((item) => item.id !== selectedCostCenterId)
                          .map((item) => (
                            <SelectItem key={item.id} value={String(item.id)}>
                              {item.code} - {item.name}
                            </SelectItem>
                          ))}
                      </SelectContent>
                    </Select>
                  </div>
                  <div className="grid gap-2">
                    <Label htmlFor="cc-erp">ERP external ID</Label>
                    <Input
                      id="cc-erp"
                      value={costCenterForm.erp_external_id}
                      onChange={(e) =>
                        setCostCenterForm((current) => ({
                          ...current,
                          erp_external_id: e.target.value,
                        }))
                      }
                      disabled={!canBudget || saving}
                    />
                  </div>
                  <div className="flex gap-2">
                    <Button
                      onClick={() => void handleCreateCostCenter()}
                      disabled={!canBudget || saving}
                    >
                      <Save className="mr-2 h-4 w-4" />
                      Create
                    </Button>
                    <Button
                      variant="outline"
                      onClick={() => void handleUpdateCostCenter()}
                      disabled={!canBudget || !selectedCostCenter || saving}
                    >
                      Update selected
                    </Button>
                  </div>
                </div>
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <CardTitle className="text-xl">Budget versions</CardTitle>
                <CardDescription>
                  Lifecycle-aware budget baseline governance with successor creation.
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="max-h-72 overflow-auto rounded-md border">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>Version</TableHead>
                        <TableHead>Status</TableHead>
                        <TableHead>Currency</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {versions.map((item) => (
                        <TableRow
                          key={item.id}
                          className={item.id === selectedVersionId ? "bg-muted" : ""}
                          onClick={() => setSelectedVersionId(item.id)}
                        >
                          <TableCell className="font-medium">
                            {item.fiscal_year} / {item.scenario_type} / v{item.version_no}
                          </TableCell>
                          <TableCell>
                            <Badge variant={budgetStatusVariant(item.status)}>{item.status}</Badge>
                          </TableCell>
                          <TableCell>{item.currency_code}</TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>

                <div className="grid gap-3">
                  <div className="grid grid-cols-3 gap-3">
                    <div className="grid gap-2">
                      <Label htmlFor="version-year">Fiscal year</Label>
                      <Input
                        id="version-year"
                        value={versionForm.fiscal_year}
                        onChange={(e) =>
                          setVersionForm((current) => ({ ...current, fiscal_year: e.target.value }))
                        }
                        disabled={!canBudget || saving}
                      />
                    </div>
                    <div className="grid gap-2">
                      <Label>Scenario</Label>
                      <Select
                        value={versionForm.scenario_type}
                        onValueChange={(value) =>
                          setVersionForm((current) => ({ ...current, scenario_type: value }))
                        }
                        disabled={!canBudget || saving}
                      >
                        <SelectTrigger>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="original">original</SelectItem>
                          <SelectItem value="approved">approved</SelectItem>
                          <SelectItem value="reforecast">reforecast</SelectItem>
                          <SelectItem value="what_if">what_if</SelectItem>
                        </SelectContent>
                      </Select>
                    </div>
                    <div className="grid gap-2">
                      <Label htmlFor="version-currency">Currency</Label>
                      <Input
                        id="version-currency"
                        value={versionForm.currency_code}
                        onChange={(e) =>
                          setVersionForm((current) => ({
                            ...current,
                            currency_code: e.target.value,
                          }))
                        }
                        disabled={!canBudget || saving}
                      />
                    </div>
                  </div>

                  <div className="grid gap-2">
                    <Label htmlFor="version-title">Title</Label>
                    <Input
                      id="version-title"
                      value={versionForm.title}
                      onChange={(e) =>
                        setVersionForm((current) => ({ ...current, title: e.target.value }))
                      }
                      disabled={selectedVersion ? !versionEditable || saving : !canBudget || saving}
                    />
                  </div>
                  <div className="grid gap-2">
                    <Label htmlFor="version-basis">Planning basis</Label>
                    <Input
                      id="version-basis"
                      value={versionForm.planning_basis}
                      onChange={(e) =>
                        setVersionForm((current) => ({
                          ...current,
                          planning_basis: e.target.value,
                        }))
                      }
                      disabled={selectedVersion ? !versionEditable || saving : !canBudget || saving}
                    />
                  </div>
                  <div className="grid gap-2">
                    <Label htmlFor="version-reference">Baseline reference</Label>
                    <Input
                      id="version-reference"
                      value={versionForm.baseline_reference}
                      onChange={(e) =>
                        setVersionForm((current) => ({
                          ...current,
                          baseline_reference: e.target.value,
                        }))
                      }
                      disabled={selectedVersion ? !versionEditable || saving : !canBudget || saving}
                    />
                  </div>
                  <div className="flex flex-wrap gap-2">
                    <Button
                      onClick={() => void handleCreateVersion()}
                      disabled={!canBudget || saving}
                    >
                      Create version
                    </Button>
                    <Button
                      variant="outline"
                      onClick={() => void handleUpdateVersion()}
                      disabled={!selectedVersion || !versionEditable || saving}
                    >
                      Update draft
                    </Button>
                    <Button
                      variant="outline"
                      onClick={() => void handleCreateSuccessor()}
                      disabled={!selectedVersion || !canBudget || saving}
                    >
                      Create successor
                    </Button>
                  </div>
                  <div className="flex flex-wrap gap-2">
                    <Button
                      variant="outline"
                      onClick={() => void handleTransitionVersion("submitted")}
                      disabled={!selectedVersion || !versionEditable || saving}
                    >
                      Submit
                    </Button>
                    <Button
                      variant="outline"
                      onClick={() => void handleTransitionVersion("approved")}
                      disabled={
                        !selectedVersion ||
                        selectedVersion.status !== "submitted" ||
                        !canBudget ||
                        saving
                      }
                    >
                      Approve
                    </Button>
                    <Button
                      onClick={() => void handleTransitionVersion("frozen")}
                      disabled={
                        !selectedVersion ||
                        selectedVersion.status !== "approved" ||
                        !canBudget ||
                        saving
                      }
                    >
                      Freeze baseline
                    </Button>
                  </div>
                </div>
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <CardTitle className="text-xl">Budget lines</CardTitle>
                <CardDescription>
                  Period-aware budget lines remain editable only while the selected version is still
                  draft.
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="max-h-72 overflow-auto rounded-md border">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>Cost center</TableHead>
                        <TableHead>Period</TableHead>
                        <TableHead>Bucket</TableHead>
                        <TableHead className="text-right">Amount</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {lines.map((item) => (
                        <TableRow
                          key={item.id}
                          className={item.id === selectedLineId ? "bg-muted" : ""}
                          onClick={() => setSelectedLineId(item.id)}
                        >
                          <TableCell className="font-medium">{item.cost_center_code}</TableCell>
                          <TableCell>{item.period_month ?? "Annual"}</TableCell>
                          <TableCell>{item.budget_bucket}</TableCell>
                          <TableCell className="text-right">
                            {item.planned_amount.toFixed(2)}
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>

                <div className="grid gap-3">
                  <div className="grid grid-cols-2 gap-3">
                    <div className="grid gap-2">
                      <Label>Cost center</Label>
                      <Select
                        value={lineForm.cost_center_id}
                        onValueChange={(value) =>
                          setLineForm((current) => ({ ...current, cost_center_id: value }))
                        }
                        disabled={!versionEditable || saving}
                      >
                        <SelectTrigger>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {costCenters.map((item) => (
                            <SelectItem key={item.id} value={String(item.id)}>
                              {item.code} - {item.name}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </div>
                    <div className="grid gap-2">
                      <Label>Period month</Label>
                      <Select
                        value={lineForm.period_month}
                        onValueChange={(value) =>
                          setLineForm((current) => ({ ...current, period_month: value }))
                        }
                        disabled={!versionEditable || saving}
                      >
                        <SelectTrigger>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="__annual__">Annual total</SelectItem>
                          {Array.from({ length: 12 }, (_, idx) => idx + 1).map((month) => (
                            <SelectItem key={month} value={String(month)}>
                              {month}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </div>
                  </div>
                  <div className="grid grid-cols-3 gap-3">
                    <div className="grid gap-2">
                      <Label>Bucket</Label>
                      <Select
                        value={lineForm.budget_bucket}
                        onValueChange={(value) =>
                          setLineForm((current) => ({ ...current, budget_bucket: value }))
                        }
                        disabled={!versionEditable || saving}
                      >
                        <SelectTrigger>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="labor">labor</SelectItem>
                          <SelectItem value="parts">parts</SelectItem>
                          <SelectItem value="services">services</SelectItem>
                          <SelectItem value="contracts">contracts</SelectItem>
                          <SelectItem value="shutdown">shutdown</SelectItem>
                          <SelectItem value="capex">capex</SelectItem>
                          <SelectItem value="other">other</SelectItem>
                        </SelectContent>
                      </Select>
                    </div>
                    <div className="grid gap-2">
                      <Label htmlFor="line-amount">Planned amount</Label>
                      <Input
                        id="line-amount"
                        value={lineForm.planned_amount}
                        onChange={(e) =>
                          setLineForm((current) => ({ ...current, planned_amount: e.target.value }))
                        }
                        disabled={!versionEditable || saving}
                      />
                    </div>
                    <div className="grid gap-2">
                      <Label>Labor lane</Label>
                      <Select
                        value={lineForm.labor_lane}
                        onValueChange={(value) =>
                          setLineForm((current) => ({ ...current, labor_lane: value }))
                        }
                        disabled={!versionEditable || saving}
                      >
                        <SelectTrigger>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="regular">regular</SelectItem>
                          <SelectItem value="overtime">overtime</SelectItem>
                          <SelectItem value="contractor">contractor</SelectItem>
                        </SelectContent>
                      </Select>
                    </div>
                  </div>
                  <div className="grid gap-2">
                    <Label htmlFor="line-source">Source basis</Label>
                    <Input
                      id="line-source"
                      value={lineForm.source_basis}
                      onChange={(e) =>
                        setLineForm((current) => ({ ...current, source_basis: e.target.value }))
                      }
                      disabled={!versionEditable || saving}
                    />
                  </div>
                  <div className="grid gap-2">
                    <Label htmlFor="line-note">Justification note</Label>
                    <textarea
                      id="line-note"
                      className="min-h-20 rounded-md border bg-background px-3 py-2 text-sm"
                      value={lineForm.justification_note}
                      onChange={(e) =>
                        setLineForm((current) => ({
                          ...current,
                          justification_note: e.target.value,
                        }))
                      }
                      disabled={!versionEditable || saving}
                    />
                  </div>
                  <div className="flex gap-2">
                    <Button
                      onClick={() => void handleCreateLine()}
                      disabled={!versionEditable || saving}
                    >
                      Create line
                    </Button>
                    <Button
                      variant="outline"
                      onClick={() => void handleUpdateLine()}
                      disabled={!selectedLine || !versionEditable || saving}
                    >
                      Update selected line
                    </Button>
                  </div>
                </div>
              </CardContent>
            </Card>
          </div>

          <div className="grid gap-6 xl:grid-cols-[1.1fr_1.1fr_1.3fr]">
            <Card>
              <CardHeader>
                <CardTitle className="text-xl">Actuals ledger drilldown</CardTitle>
                <CardDescription>
                  Provenance-preserving events with provisional, posted, and reversal traceability.
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="flex flex-wrap gap-2">
                  <Button
                    variant="outline"
                    onClick={() => void handleCreateActualFromSelection(false)}
                    disabled={!canBudget || !selectedLine || saving}
                  >
                    Add provisional from selected line
                  </Button>
                  <Button
                    variant="outline"
                    onClick={() => void handleCreateActualFromSelection(true)}
                    disabled={!canPost || !selectedLine || saving}
                  >
                    Add posted from selected line
                  </Button>
                </div>
                <div className="flex flex-wrap gap-2">
                  <Button
                    onClick={() => void handlePostSelectedActual()}
                    disabled={
                      !canPost ||
                      !selectedActual ||
                      selectedActual.posting_status !== "provisional" ||
                      saving
                    }
                  >
                    Post selected provisional
                  </Button>
                  <Button
                    variant="outline"
                    onClick={() => void handleReverseSelectedActual()}
                    disabled={
                      !canPost ||
                      !selectedActual ||
                      selectedActual.posting_status !== "posted" ||
                      saving
                    }
                  >
                    Reverse selected posted
                  </Button>
                </div>
                <div className="max-h-72 overflow-auto rounded-md border">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>Status</TableHead>
                        <TableHead>Source</TableHead>
                        <TableHead>Lane</TableHead>
                        <TableHead className="text-right">Base amount</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {actuals.map((item) => (
                        <TableRow
                          key={item.id}
                          className={item.id === selectedActualId ? "bg-muted" : ""}
                          onClick={() => setSelectedActualId(item.id)}
                        >
                          <TableCell>{item.posting_status}</TableCell>
                          <TableCell>{item.source_type}</TableCell>
                          <TableCell>{item.rate_card_lane ?? "—"}</TableCell>
                          <TableCell className="text-right">
                            {item.amount_base.toFixed(2)}
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <CardTitle className="text-xl">Commitments ledger</CardTitle>
                <CardDescription>
                  PO and contract obligations remain separate from actual costs.
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-4">
                <Button
                  onClick={() => void handleCreateCommitmentFromSelection()}
                  disabled={!canBudget || !selectedLine || saving}
                >
                  Create commitment from selected line
                </Button>
                <div className="max-h-72 overflow-auto rounded-md border">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>Status</TableHead>
                        <TableHead>Type</TableHead>
                        <TableHead>Source</TableHead>
                        <TableHead className="text-right">Amount</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {commitments.map((item) => (
                        <TableRow key={item.id}>
                          <TableCell>{item.commitment_status}</TableCell>
                          <TableCell>{item.commitment_type}</TableCell>
                          <TableCell>{item.source_type}</TableCell>
                          <TableCell className="text-right">
                            {item.base_amount.toFixed(2)}
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <CardTitle className="text-xl">Forecast generation and explainability</CardTitle>
                <CardDescription>
                  Idempotent reruns with PM/backlog/shutdown/planning drivers and confidence
                  metadata.
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="flex flex-wrap gap-2">
                  <Button
                    onClick={() => void handleGenerateForecasts()}
                    disabled={!canBudget || !selectedVersion || saving}
                  >
                    Generate forecasts (idempotent key)
                  </Button>
                  <Badge variant="outline">Runs: {forecastRuns.length}</Badge>
                </div>
                <div className="max-h-52 overflow-auto rounded-md border">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>Method</TableHead>
                        <TableHead>Confidence</TableHead>
                        <TableHead>Driver</TableHead>
                        <TableHead className="text-right">Amount</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {forecasts.map((item) => (
                        <TableRow
                          key={item.id}
                          className={item.id === selectedForecastId ? "bg-muted" : ""}
                          onClick={() => setSelectedForecastId(item.id)}
                        >
                          <TableCell>{item.forecast_method}</TableCell>
                          <TableCell>{item.confidence_level}</TableCell>
                          <TableCell>{item.driver_type ?? "—"}</TableCell>
                          <TableCell className="text-right">
                            {item.forecast_amount.toFixed(2)}
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
                <div className="rounded-md border p-3">
                  <div className="text-xs font-medium uppercase text-muted-foreground">
                    Selected forecast explainability
                  </div>
                  <pre className="mt-2 overflow-auto text-xs">
                    {selectedForecast
                      ? decodeOptionalJson(selectedForecast.explainability_json)
                      : "—"}
                  </pre>
                </div>
              </CardContent>
            </Card>
          </div>
        </TabsContent>

        <TabsContent value="explainability" className="space-y-6">
          <div className="grid gap-6 lg:grid-cols-2">
            <Card>
              <CardHeader>
                <CardTitle className="text-xl">Selected version narrative</CardTitle>
                <CardDescription>
                  Expose the baseline assumptions that later forecasts and actuals will reconcile
                  against.
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-4">
                {selectedVersion ? (
                  <>
                    <div className="flex flex-wrap gap-2">
                      <Badge variant={budgetStatusVariant(selectedVersion.status)}>
                        {selectedVersion.status}
                      </Badge>
                      <Badge variant="outline">{selectedVersion.scenario_type}</Badge>
                      <Badge variant="outline">v{selectedVersion.version_no}</Badge>
                    </div>
                    <div className="grid gap-2">
                      <Label htmlFor="explain-title">Title</Label>
                      <Input
                        id="explain-title"
                        value={versionForm.title}
                        onChange={(e) =>
                          setVersionForm((current) => ({ ...current, title: e.target.value }))
                        }
                        disabled={!versionEditable || saving}
                      />
                    </div>
                    <div className="grid gap-2">
                      <Label htmlFor="explain-planning">Planning basis</Label>
                      <Input
                        id="explain-planning"
                        value={versionForm.planning_basis}
                        onChange={(e) =>
                          setVersionForm((current) => ({
                            ...current,
                            planning_basis: e.target.value,
                          }))
                        }
                        disabled={!versionEditable || saving}
                      />
                    </div>
                    <div className="grid gap-2">
                      <Label htmlFor="explain-source-mix">Source basis mix JSON</Label>
                      <textarea
                        id="explain-source-mix"
                        className="min-h-28 rounded-md border bg-background px-3 py-2 text-sm font-mono"
                        value={versionForm.source_basis_mix_json}
                        onChange={(e) =>
                          setVersionForm((current) => ({
                            ...current,
                            source_basis_mix_json: e.target.value,
                          }))
                        }
                        disabled={!versionEditable || saving}
                      />
                    </div>
                    <div className="grid gap-2">
                      <Label htmlFor="explain-labor">Labor assumptions JSON</Label>
                      <textarea
                        id="explain-labor"
                        className="min-h-28 rounded-md border bg-background px-3 py-2 text-sm font-mono"
                        value={versionForm.labor_assumptions_json}
                        onChange={(e) =>
                          setVersionForm((current) => ({
                            ...current,
                            labor_assumptions_json: e.target.value,
                          }))
                        }
                        disabled={!versionEditable || saving}
                      />
                    </div>
                    <div className="grid gap-2">
                      <Label htmlFor="explain-erp">ERP external reference</Label>
                      <Input
                        id="explain-erp"
                        value={versionForm.erp_external_ref}
                        onChange={(e) =>
                          setVersionForm((current) => ({
                            ...current,
                            erp_external_ref: e.target.value,
                          }))
                        }
                        disabled={!versionEditable || saving}
                      />
                    </div>
                    <Button
                      onClick={() => void handleUpdateVersion()}
                      disabled={!versionEditable || saving}
                    >
                      Save explainability fields
                    </Button>
                  </>
                ) : (
                  <p className="text-sm text-muted-foreground">
                    Select a budget version to inspect its governed baseline evidence.
                  </p>
                )}
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <CardTitle className="text-xl">Current baseline evidence</CardTitle>
                <CardDescription>
                  Read-only snapshot of what the chosen baseline currently communicates.
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-4">
                {selectedVersion ? (
                  <>
                    <div className="rounded-md border p-3">
                      <div className="text-xs font-medium uppercase text-muted-foreground">
                        Planning basis
                      </div>
                      <div className="mt-1 text-sm">{selectedVersion.planning_basis ?? "—"}</div>
                    </div>
                    <div className="rounded-md border p-3">
                      <div className="text-xs font-medium uppercase text-muted-foreground">
                        Source basis mix
                      </div>
                      <pre className="mt-2 overflow-auto text-xs">
                        {decodeOptionalJson(selectedVersion.source_basis_mix_json)}
                      </pre>
                    </div>
                    <div className="rounded-md border p-3">
                      <div className="text-xs font-medium uppercase text-muted-foreground">
                        Labor assumptions
                      </div>
                      <pre className="mt-2 overflow-auto text-xs">
                        {decodeOptionalJson(selectedVersion.labor_assumptions_json)}
                      </pre>
                    </div>
                    <div className="rounded-md border p-3">
                      <div className="text-xs font-medium uppercase text-muted-foreground">
                        Baseline reference
                      </div>
                      <div className="mt-1 text-sm">
                        {selectedVersion.baseline_reference ?? "—"}
                      </div>
                    </div>
                    <div className="rounded-md border p-3">
                      <div className="text-xs font-medium uppercase text-muted-foreground">
                        ERP linkage
                      </div>
                      <div className="mt-1 text-sm">
                        {selectedVersion.erp_external_ref ?? "local only"}
                      </div>
                    </div>
                  </>
                ) : (
                  <p className="text-sm text-muted-foreground">No selected version.</p>
                )}
              </CardContent>
            </Card>
          </div>
        </TabsContent>

        <TabsContent value="variance-erp" className="space-y-6">
          <div className="grid gap-6 xl:grid-cols-3">
            <Card>
              <CardHeader>
                <CardTitle className="text-xl">Variance review workflow</CardTitle>
                <CardDescription>
                  Governed lifecycle with coded drivers, owner accountability, and reproducible
                  snapshot context.
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="flex flex-wrap gap-2">
                  <Button
                    onClick={() => void handleCreateVarianceReview()}
                    disabled={!canBudget || !selectedLine || saving}
                  >
                    Open review from selected line
                  </Button>
                  <Button
                    variant="outline"
                    onClick={() => void handleAdvanceVarianceReview("in_review")}
                    disabled={!canBudget || !selectedVarianceReview || saving}
                  >
                    Move to in_review
                  </Button>
                  <Button
                    variant="outline"
                    onClick={() => void handleAdvanceVarianceReview("actioned")}
                    disabled={!canBudget || !selectedVarianceReview || saving}
                  >
                    Move to actioned
                  </Button>
                  <Button
                    variant="outline"
                    onClick={() => void handleAdvanceVarianceReview("accepted")}
                    disabled={!canBudget || !selectedVarianceReview || saving}
                  >
                    Accept
                  </Button>
                  <Button
                    onClick={() => void handleAdvanceVarianceReview("closed")}
                    disabled={!canBudget || !selectedVarianceReview || saving}
                  >
                    Close
                  </Button>
                </div>
                <div className="max-h-64 overflow-auto rounded-md border">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>Status</TableHead>
                        <TableHead>Driver</TableHead>
                        <TableHead>Owner</TableHead>
                        <TableHead className="text-right">Variance</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {varianceReviews.map((item) => (
                        <TableRow
                          key={item.id}
                          className={item.id === selectedVarianceReviewId ? "bg-muted" : ""}
                          onClick={() => setSelectedVarianceReviewId(item.id)}
                        >
                          <TableCell>{item.review_status}</TableCell>
                          <TableCell>{item.driver_code}</TableCell>
                          <TableCell>{item.action_owner_id}</TableCell>
                          <TableCell className="text-right">
                            {item.variance_amount.toFixed(2)}
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
                <div className="rounded-md border p-3">
                  <div className="text-xs font-medium uppercase text-muted-foreground">
                    Snapshot context
                  </div>
                  <pre className="mt-2 overflow-auto text-xs">
                    {selectedVarianceReview
                      ? decodeOptionalJson(selectedVarianceReview.snapshot_context_json)
                      : "—"}
                  </pre>
                </div>
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <CardTitle className="text-xl">Variance dashboard slices</CardTitle>
                <CardDescription>
                  Planned, committed, actual, and forecast layers by spend mix, team, assignee, and
                  labor lane.
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="max-h-64 overflow-auto rounded-md border">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>Mix</TableHead>
                        <TableHead>Team</TableHead>
                        <TableHead>Lane</TableHead>
                        <TableHead className="text-right">Plan</TableHead>
                        <TableHead className="text-right">Actual</TableHead>
                        <TableHead className="text-right">Var %</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {dashboardRows.map((row, idx) => (
                        <TableRow
                          key={`${row.cost_center_id}-${row.period_month ?? "annual"}-${row.budget_bucket}-${idx}`}
                        >
                          <TableCell>{row.spend_mix}</TableCell>
                          <TableCell>{row.team_id ?? "—"}</TableCell>
                          <TableCell>{row.labor_lane ?? "—"}</TableCell>
                          <TableCell className="text-right">
                            {row.planned_amount.toFixed(2)}
                          </TableCell>
                          <TableCell className="text-right">
                            {row.actual_amount.toFixed(2)}
                          </TableCell>
                          <TableCell className="text-right">
                            {row.planned_amount === 0
                              ? "—"
                              : `${(((row.actual_amount - row.planned_amount) / row.planned_amount) * 100).toFixed(1)}%`}
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
                <div className="max-h-48 overflow-auto rounded-md border">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>Layer</TableHead>
                        <TableHead>Source</TableHead>
                        <TableHead>WO/PM/Inspection</TableHead>
                        <TableHead className="text-right">Productivity</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {drilldownRows.slice(0, 24).map((row) => (
                        <TableRow key={`${row.layer_type}-${row.record_id}`}>
                          <TableCell>{row.layer_type}</TableCell>
                          <TableCell>{row.source_type ?? row.source_id ?? "—"}</TableCell>
                          <TableCell>
                            {row.work_order_id ??
                              row.pm_occurrence_ref ??
                              row.inspection_ref ??
                              "—"}
                          </TableCell>
                          <TableCell className="text-right">
                            {row.hours_overrun_rate ?? 0} / {row.repeat_work_penalty ?? 0}
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <CardTitle className="text-xl">Cost of failure (read model)</CardTitle>
                <CardDescription>
                  Joins governed failure events with work order cost roll-ups per equipment and
                  calendar month (no double counting when one event maps to one WO).
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-3">
                <div className="max-h-48 overflow-auto rounded-md border">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>Equipment</TableHead>
                        <TableHead>Period</TableHead>
                        <TableHead className="text-right">Downtime cost</TableHead>
                        <TableHead className="text-right">Corrective cost</TableHead>
                        <TableHead>CCY</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {costOfFailureRows.map((row) => (
                        <TableRow key={`${row.equipment_id}-${row.period}`}>
                          <TableCell>{row.equipment_id}</TableCell>
                          <TableCell>{row.period}</TableCell>
                          <TableCell className="text-right">
                            {row.total_downtime_cost.toFixed(2)}
                          </TableCell>
                          <TableCell className="text-right">
                            {row.total_corrective_cost.toFixed(2)}
                          </TableCell>
                          <TableCell>{row.currency_code}</TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <CardTitle className="text-xl">ERP alignment contracts</CardTitle>
                <CardDescription>
                  Optional ERP master import plus export-ready posted actual and approved reforecast
                  payloads.
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-4">
                <PermissionGate permission="fin.report">
                  <div className="flex flex-wrap gap-2">
                    <Button
                      variant="outline"
                      onClick={() => void handleImportErpMaster()}
                      disabled={saving}
                    >
                      Import ERP master
                    </Button>
                    <Button onClick={() => void handleRefreshErpPayloads()} disabled={saving}>
                      Refresh ERP payloads
                    </Button>
                  </div>
                </PermissionGate>
                <div className="rounded-md border p-3">
                  <div className="text-xs font-medium uppercase text-muted-foreground">
                    Alert controls
                  </div>
                  <div className="mt-2 flex flex-wrap gap-2">
                    <Button
                      variant="outline"
                      onClick={() => void handleEnsureDefaultAlertRules()}
                      disabled={
                        !canBudget ||
                        !selectedVersion ||
                        selectedVersion.status !== "frozen" ||
                        saving
                      }
                    >
                      Ensure default threshold rules
                    </Button>
                    <Button
                      onClick={() => void handleEvaluateAlerts()}
                      disabled={
                        !canBudget ||
                        !selectedVersion ||
                        selectedVersion.status !== "frozen" ||
                        saving
                      }
                    >
                      Evaluate controls + emit notifications
                    </Button>
                    <Button
                      variant="outline"
                      onClick={() => void handleAcknowledgeSelectedAlert()}
                      disabled={!selectedAlertEvent || saving}
                    >
                      Acknowledge selected alert
                    </Button>
                  </div>
                  <div className="mt-2 text-xs text-muted-foreground">
                    Rules: {alertConfigs.length} | Events: {alertEvents.length}. Dedupe window
                    defaults to 240 minutes per rule and matches backend evaluation.
                  </div>
                </div>
                <div className="max-h-40 overflow-auto rounded-md border">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>Type</TableHead>
                        <TableHead>Center</TableHead>
                        <TableHead>Severity</TableHead>
                        <TableHead>Ack</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {alertEvents.slice(0, 20).map((item) => (
                        <TableRow
                          key={item.id}
                          className={item.id === selectedAlertEventId ? "bg-muted" : ""}
                          onClick={() => setSelectedAlertEventId(item.id)}
                        >
                          <TableCell>{item.alert_type}</TableCell>
                          <TableCell>{item.cost_center_code}</TableCell>
                          <TableCell>{item.severity}</TableCell>
                          <TableCell>{item.acknowledged_at ? "yes" : "no"}</TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
                <div className="rounded-md border p-3">
                  <div className="text-xs font-medium uppercase text-muted-foreground">
                    Report packs
                  </div>
                  <div className="mt-2 flex flex-wrap gap-2">
                    <Button
                      variant="outline"
                      onClick={() => void handleBuildReportPack()}
                      disabled={!selectedVersion || saving}
                    >
                      Build explainable pack
                    </Button>
                    <Button
                      onClick={() => void handleExportReportPack("pdf")}
                      disabled={!canReport || !selectedVersion || saving}
                    >
                      Export PDF pack
                    </Button>
                    <Button
                      variant="outline"
                      onClick={() => void handleExportReportPack("excel")}
                      disabled={!canReport || !selectedVersion || saving}
                    >
                      Export Excel pack
                    </Button>
                  </div>
                  {reportPack ? (
                    <div className="mt-2 text-xs text-muted-foreground">
                      Baseline {reportPack.totals.baseline_amount.toFixed(2)} | Actual{" "}
                      {reportPack.totals.posted_actual_amount.toFixed(2)} | Variance{" "}
                      {reportPack.totals.variance_amount.toFixed(2)} (
                      {reportPack.totals.variance_pct.toFixed(2)}%)
                    </div>
                  ) : null}
                  {reportExport ? (
                    <div className="mt-1 text-xs text-muted-foreground">
                      Last export: {reportExport.file_name} ({reportExport.mime_type})
                    </div>
                  ) : null}
                </div>
                {!canReport ? (
                  <p className="text-sm text-muted-foreground">
                    ERP import/export actions require `fin.report` permission. Dashboard and reviews
                    remain visible under `fin.view`.
                  </p>
                ) : null}
                <div className="rounded-md border p-3 text-sm">
                  <div>Posted actual payload records: {erpPostedPayload.length}</div>
                  <div>Approved reforecast payload records: {erpReforecastPayload.length}</div>
                </div>
                <div className="max-h-52 overflow-auto rounded-md border">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>Type</TableHead>
                        <TableHead>Center</TableHead>
                        <TableHead>Currency</TableHead>
                        <TableHead>Flags</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {erpPostedPayload.slice(0, 10).map((item) => (
                        <TableRow key={`actual-${item.actual_id}`}>
                          <TableCell>posted_actual</TableCell>
                          <TableCell>{item.local_cost_center_code}</TableCell>
                          <TableCell>
                            {item.source_currency}/{item.base_currency}
                          </TableCell>
                          <TableCell>{item.reconciliation_flags.join(", ") || "none"}</TableCell>
                        </TableRow>
                      ))}
                      {erpReforecastPayload.slice(0, 10).map((item) => (
                        <TableRow key={`forecast-${item.forecast_id}`}>
                          <TableCell>approved_reforecast</TableCell>
                          <TableCell>{item.local_cost_center_code}</TableCell>
                          <TableCell>{item.base_currency}</TableCell>
                          <TableCell>{item.reconciliation_flags.join(", ") || "none"}</TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
                <div className="rounded-md border p-3 space-y-3">
                  <div className="text-xs font-medium uppercase text-muted-foreground">
                    Signed ERP export batches (JSONL)
                  </div>
                  <div className="flex flex-wrap items-end gap-3">
                    <div className="space-y-1">
                      <Label htmlFor="erp-tenant">Tenant id (optional)</Label>
                      <Input
                        id="erp-tenant"
                        value={erpTenantInput}
                        onChange={(e) => setErpTenantInput(e.target.value)}
                        placeholder="tenant slug or UUID"
                        className="w-64"
                      />
                    </div>
                    <PermissionGate permission="fin.report">
                      <Button
                        variant="outline"
                        onClick={() => void handleRecordSignedExport("posted_actuals")}
                        disabled={saving}
                      >
                        Record posted actuals batch
                      </Button>
                      <Button
                        onClick={() => void handleRecordSignedExport("approved_reforecasts")}
                        disabled={saving}
                      >
                        Record approved reforecasts batch
                      </Button>
                    </PermissionGate>
                  </div>
                  {lastErpExportResult ? (
                    <div className="space-y-1 text-xs">
                      <div>
                        Batch {lastErpExportResult.batch.batch_uuid} ·{" "}
                        {lastErpExportResult.batch.export_kind} · lines{" "}
                        {lastErpExportResult.batch.line_count}
                        {(() => {
                          try {
                            const sig = JSON.parse(
                              lastErpExportResult.batch.relay_payload_json,
                            ) as { signature?: string };
                            return sig.signature
                              ? ` · signature ${sig.signature.slice(0, 16)}…`
                              : "";
                          } catch {
                            return "";
                          }
                        })()}
                      </div>
                      <Textarea
                        readOnly
                        className="font-mono text-xs min-h-[120px]"
                        value={lastErpExportResult.jsonl}
                      />
                    </div>
                  ) : null}
                  <div className="max-h-40 overflow-auto rounded-md border">
                    <Table>
                      <TableHeader>
                        <TableRow>
                          <TableHead>Batch</TableHead>
                          <TableHead>Kind</TableHead>
                          <TableHead>Status</TableHead>
                          <TableHead className="text-right">Lines</TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {postedExportBatches.map((b) => (
                          <TableRow key={b.id}>
                            <TableCell className="font-mono text-xs">{b.batch_uuid}</TableCell>
                            <TableCell>{b.export_kind}</TableCell>
                            <TableCell>{b.status}</TableCell>
                            <TableCell className="text-right">{b.line_count}</TableCell>
                          </TableRow>
                        ))}
                      </TableBody>
                    </Table>
                  </div>
                  <div className="text-xs font-medium uppercase text-muted-foreground">
                    Integration exceptions
                  </div>
                  <div className="flex flex-wrap gap-2">
                    <PermissionGate permission="fin.report">
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => void handleMergeSelectedException()}
                        disabled={saving || selectedExceptionId === null}
                      >
                        Mark merged (use Maintafox value)
                      </Button>
                    </PermissionGate>
                  </div>
                  <div className="max-h-44 overflow-auto rounded-md border">
                    <Table>
                      <TableHeader>
                        <TableRow>
                          <TableHead>Source</TableHead>
                          <TableHead>Status</TableHead>
                          <TableHead>Code</TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {integrationExceptions.map((ex) => (
                          <TableRow
                            key={ex.id}
                            className={ex.id === selectedExceptionId ? "bg-muted" : ""}
                            onClick={() => setSelectedExceptionId(ex.id)}
                          >
                            <TableCell className="text-xs">
                              {ex.source_record_kind} #{ex.source_record_id}
                            </TableCell>
                            <TableCell>{ex.resolution_status}</TableCell>
                            <TableCell>{ex.rejection_code ?? "—"}</TableCell>
                          </TableRow>
                        ))}
                      </TableBody>
                    </Table>
                  </div>
                </div>
              </CardContent>
            </Card>
          </div>
        </TabsContent>
      </Tabs>

      {loading ? (
        <p className="text-sm text-muted-foreground">Loading finance baseline workspace...</p>
      ) : null}
    </ModulePageShell>
  );
}
