/**
 * Budget & maintenance costs (PRD §6.24) — benchmarked on SAP PM/CO and Maximo financials.
 *
 * - Cockpit: budget vs committed vs actual vs forecast by cost centre and category, monthly
 *   burn, drill-down to the OTs and purchase documents behind each figure.
 * - OT settlements: closed OTs posted to the budget, pending ones with the reason to fix.
 * - Planning: cost centres, budget versions (approval lifecycle) and budget lines.
 * - Variance reviews: reason codes and the review workflow.
 * - Asset costs: lifetime maintenance cost per asset against its replacement value.
 * - Periods: month close of the fiscal year.
 *
 * Actuals come from OT close-out (`finance::settlement`), never typed from the budget itself;
 * the currency is the tenant currency.
 */
import { Wallet } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";

import { AssetCostPanel } from "@/components/finance/AssetCostPanel";
import { BudgetCockpitView, type CockpitTarget } from "@/components/finance/BudgetCockpitView";
import { BudgetPeriodsPanel } from "@/components/finance/BudgetPeriodsPanel";
import { BudgetPlanningPanel } from "@/components/finance/BudgetPlanningPanel";
import { SettlementWorklist } from "@/components/finance/SettlementWorklist";
import {
  VarianceReviewDialog,
  VarianceReviewsPanel,
} from "@/components/finance/VarianceReviewsPanel";
import { RefreshButton } from "@/components/layout/ModuleHeaderControls";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useFormatters } from "@/hooks/use-formatters";
import { listBudgetVersions } from "@/services/budget-service";
import type { BudgetCockpit, BudgetCockpitRow } from "@/services/finance-cost-service";
import type { BudgetVersion } from "@shared/ipc-types";

const TABS = ["cockpit", "settlements", "planning", "reviews", "assets", "periods"] as const;
type Tab = (typeof TABS)[number];
const AUTO = "__auto__";

function isTab(v: string | null): v is Tab {
  return v != null && (TABS as readonly string[]).includes(v);
}

/** Version the year's actuals post to (approved / frozen, approved scenario first), else latest. */
export function defaultVersion(versions: BudgetVersion[]): BudgetVersion | null {
  const rank: Record<string, number> = { approved: 0, original: 1, reforecast: 2 };
  const posting = versions
    .filter(
      (v) => (v.status === "approved" || v.status === "frozen") && v.scenario_type !== "what_if",
    )
    .sort(
      (a, b) =>
        (rank[a.scenario_type] ?? 3) - (rank[b.scenario_type] ?? 3) || b.version_no - a.version_no,
    );
  if (posting[0]) return posting[0];
  return [...versions].sort((a, b) => b.version_no - a.version_no || b.id - a.id)[0] ?? null;
}

export function BudgetPage() {
  const { t } = useTranslation("budget");
  const { currencyCode } = useFormatters();
  const [searchParams, setSearchParams] = useSearchParams();
  const tabParam = searchParams.get("tab");
  const tab: Tab = isTab(tabParam) ? tabParam : "cockpit";
  const thisYear = new Date().getFullYear();
  const [fiscalYear, setFiscalYear] = useState(thisYear);
  const [allVersions, setAllVersions] = useState<BudgetVersion[]>([]);
  const [versionChoice, setVersionChoice] = useState<string>(AUTO);
  const [reloadToken, setReloadToken] = useState(0);
  const [review, setReview] = useState<{ row: BudgetCockpitRow; cockpit: BudgetCockpit } | null>(
    null,
  );

  const loadVersions = useCallback(async () => {
    try {
      setAllVersions(await listBudgetVersions({}));
    } catch {
      // The cockpit and planning tabs report load errors themselves.
      setAllVersions([]);
    }
  }, []);

  // biome-ignore lint/correctness/useExhaustiveDependencies: reloadToken re-reads the versions
  useEffect(() => {
    void loadVersions();
  }, [loadVersions, reloadToken]);

  const years = useMemo(() => {
    const set = new Set<number>([thisYear - 1, thisYear, thisYear + 1]);
    for (const v of allVersions) set.add(v.fiscal_year);
    return [...set].sort((a, b) => b - a);
  }, [allVersions, thisYear]);
  const versions = useMemo(
    () => allVersions.filter((v) => v.fiscal_year === fiscalYear),
    [allVersions, fiscalYear],
  );
  const autoVersion = defaultVersion(versions);
  const selectedVersionId =
    versionChoice === AUTO ? (autoVersion?.id ?? null) : Number(versionChoice);
  const selectedVersion = versions.find((v) => v.id === selectedVersionId) ?? null;

  const refresh = useCallback(() => setReloadToken((n) => n + 1), []);
  const setTab = (next: string) =>
    setSearchParams(
      (prev) => {
        const p = new URLSearchParams(prev);
        p.set("tab", next);
        return p;
      },
      { replace: true },
    );
  const navigate = (target: CockpitTarget) => setTab(target);

  return (
    <ModulePageShell
      icon={Wallet}
      title={t("page.title")}
      description={t("page.description")}
      actions={
        <>
          <Select
            value={String(fiscalYear)}
            onValueChange={(v) => {
              setFiscalYear(Number(v));
              setVersionChoice(AUTO);
            }}
          >
            <SelectTrigger className="h-9 w-28" aria-label={t("page.fiscalYear")}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {years.map((y) => (
                <SelectItem key={y} value={String(y)}>
                  {y}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Select value={versionChoice} onValueChange={setVersionChoice}>
            <SelectTrigger className="h-9 w-64" aria-label={t("page.version")}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={AUTO}>
                {autoVersion
                  ? t("page.versionAuto", {
                      title: autoVersion.title ?? `v${autoVersion.version_no}`,
                    })
                  : t("page.noVersion")}
              </SelectItem>
              {versions.map((v) => (
                <SelectItem key={v.id} value={String(v.id)}>
                  {v.title ?? t("planning.untitled")} · v{v.version_no} ·{" "}
                  {t(`versionStatus.${v.status}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <RefreshButton onRefresh={refresh} />
        </>
      }
    >
      <Tabs value={tab} onValueChange={setTab} className="space-y-4">
        <TabsList className="w-fit flex-wrap">
          {TABS.map((k) => (
            <TabsTrigger key={k} value={k}>
              {t(`tabs.${k}`)}
            </TabsTrigger>
          ))}
        </TabsList>
        <TabsContent value="cockpit">
          <BudgetCockpitView
            fiscalYear={fiscalYear}
            budgetVersionId={selectedVersionId}
            reloadToken={reloadToken}
            onNavigate={navigate}
            onAnalyseVariance={(row, cockpit) => setReview({ row, cockpit })}
          />
        </TabsContent>
        <TabsContent value="settlements">
          <SettlementWorklist
            fiscalYear={fiscalYear}
            reloadToken={reloadToken}
            onChanged={refresh}
          />
        </TabsContent>
        <TabsContent value="planning">
          <BudgetPlanningPanel
            fiscalYear={fiscalYear}
            selectedVersionId={selectedVersionId}
            onSelectVersion={(id) => setVersionChoice(id == null ? AUTO : String(id))}
            onChanged={refresh}
          />
        </TabsContent>
        <TabsContent value="reviews">
          <VarianceReviewsPanel
            budgetVersionId={selectedVersionId}
            currency={selectedVersion?.currency_code ?? currencyCode}
            reloadToken={reloadToken}
          />
        </TabsContent>
        <TabsContent value="assets">
          <AssetCostPanel reloadToken={reloadToken} />
        </TabsContent>
        <TabsContent value="periods">
          <BudgetPeriodsPanel
            fiscalYear={fiscalYear}
            currency={selectedVersion?.currency_code ?? currencyCode}
            reloadToken={reloadToken}
            onChanged={refresh}
          />
        </TabsContent>
      </Tabs>
      <VarianceReviewDialog
        row={review?.row ?? null}
        cockpit={review?.cockpit ?? null}
        onClose={() => setReview(null)}
        onCreated={refresh}
      />
    </ModulePageShell>
  );
}
