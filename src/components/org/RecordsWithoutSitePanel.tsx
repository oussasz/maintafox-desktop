/**
 * "Records without a site" (CMMS Advisor decision Q1, step 6 site-scoped roles).
 *
 * Old work orders, requests and planning windows recorded before a site was required have none:
 * only plant-wide users can act on them ("To review: no site"). A plant-wide organisation
 * administrator picks records of one kind, a site (pre-filled with the site where their equipment
 * is installed today, as a suggestion to confirm) and one reason, then confirms (step-up). The
 * backend gives the site only to records without one, writes one audit entry per record and
 * refuses the whole batch otherwise; the counts go down.
 *
 * A planning window without a site is valid (it applies to the whole plant): it is listed so it
 * can be given a site, and counted "to review" only when it lost its site ("Site to re-enter").
 *
 * Dialog overlay like the other organisation tools of the page (NodeTypeManagerPanel).
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { type ScopeChoice, ScopeNodeSelect } from "@/components/admin/ScopeNodeSelect";
import { diStatusToI18nKey } from "@/components/di/status-meta";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { ErrorState, LoadingState, TruncationNotice } from "@/components/ui/data-state";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { EmptyState } from "@/components/ui/empty-state";
import { Label } from "@/components/ui/label";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { useFormatters } from "@/hooks/use-formatters";
import { isStepUpCancelled, useStepUp } from "@/hooks/use-step-up";
import { appErrorReason } from "@/lib/app-error";
import { assignSiteToRecords, listRecordsWithoutSite } from "@/services/site-assignment-service";
import { pushAppToast } from "@/store/app-toast-store";
import { statusToI18nKey } from "@/utils/wo-status";
import type { NoSiteCounts, NoSiteKind, NoSiteRecord, RecordsWithoutSite } from "@shared/ipc-types";

const KINDS: NoSiteKind[] = ["work_order", "request", "planning_window"];

function countOf(counts: NoSiteCounts | undefined, kind: NoSiteKind): number {
  if (!counts) return 0;
  switch (kind) {
    case "work_order":
      return counts.work_orders;
    case "request":
      return counts.requests;
    case "planning_window":
      return counts.planning_windows_to_review + counts.planning_windows_plant_wide;
  }
}

/** The node suggested for every selected record, when they all share one; else none. */
export function commonSuggestion(records: NoSiteRecord[], selected: Set<number>): number | null {
  let common: number | null = null;
  for (const r of records) {
    if (!selected.has(r.id)) continue;
    if (r.suggested_node_id == null) return null;
    if (common == null) common = r.suggested_node_id;
    else if (common !== r.suggested_node_id) return null;
  }
  return common;
}

interface RecordsWithoutSitePanelProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Called with the new counts after sites were given. */
  onChanged?: (counts: NoSiteCounts) => void;
}

export function RecordsWithoutSitePanel({
  open,
  onOpenChange,
  onChanged,
}: RecordsWithoutSitePanelProps) {
  const { t } = useTranslation(["org", "ot", "di"]);
  const { formatDate, formatDateTime } = useFormatters();
  const { withStepUp, StepUpDialogElement } = useStepUp();

  const [kind, setKind] = useState<NoSiteKind>("work_order");
  const [data, setData] = useState<RecordsWithoutSite | null>(null);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [node, setNode] = useState<ScopeChoice>("");
  /** The administrator chose the site: the suggestion no longer replaces it. */
  const [nodeChosen, setNodeChosen] = useState(false);
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const [assignError, setAssignError] = useState<string | null>(null);

  const load = useCallback(async (which: NoSiteKind) => {
    setLoading(true);
    setLoadError(null);
    try {
      setData(await listRecordsWithoutSite(which));
    } catch (err) {
      // Shown in place of the list (never an empty list "because there is nothing").
      setLoadError(err);
      setData(null);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!open) return;
    setSelected(new Set());
    setNode("");
    setNodeChosen(false);
    setAssignError(null);
    void load(kind);
  }, [open, kind, load]);

  const records = useMemo(() => data?.records ?? [], [data]);
  const suggestion = useMemo(() => commonSuggestion(records, selected), [records, selected]);

  // The equipment's site is offered, never applied by itself: the administrator confirms it.
  useEffect(() => {
    if (nodeChosen) return;
    setNode(suggestion != null ? String(suggestion) : "");
  }, [suggestion, nodeChosen]);

  const toggle = (id: number, checked: boolean) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (checked) next.add(id);
      else next.delete(id);
      return next;
    });
  };
  const allSelected = records.length > 0 && records.every((r) => selected.has(r.id));
  const toggleAll = (checked: boolean) => {
    setSelected(checked ? new Set(records.map((r) => r.id)) : new Set());
  };

  const minReason = data?.reason_min_chars ?? 0;
  const maxBatch = data?.max_batch ?? 0;
  const reasonOk = data != null && reason.trim().length >= minReason;
  const tooMany = data != null && selected.size > maxBatch;
  const canAssign =
    !busy && data != null && selected.size > 0 && !tooMany && node !== "" && reasonOk;
  const siteName = data?.nodes.find((n) => String(n.id) === node)?.name ?? "";

  const handleAssign = useCallback(async () => {
    if (!canAssign || busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setAssignError(null);
    try {
      const result = await withStepUp(() =>
        assignSiteToRecords({
          kind,
          record_ids: Array.from(selected),
          org_node_id: Number(node),
          reason: reason.trim(),
        }),
      );
      pushAppToast({
        title: t("org:noSite.assigned", { count: result.assigned.length, site: siteName }),
        variant: "success",
      });
      onChanged?.(result.counts);
      setReason("");
      setSelected(new Set());
      setNodeChosen(false);
      await load(kind);
    } catch (err) {
      // The refusal is already shown as a notification; kept here next to the button too.
      if (!isStepUpCancelled(err)) setAssignError(appErrorReason(err));
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }, [canAssign, kind, selected, node, reason, siteName, withStepUp, onChanged, load, t]);

  const statusLabel = (r: NoSiteRecord): string => {
    if (r.status_code == null) return "—";
    if (r.kind === "work_order") return t(`ot:status.${statusToI18nKey(r.status_code)}`);
    if (r.kind === "request") return t(`di:status.${diStatusToI18nKey(r.status_code)}`);
    return r.status_code;
  };

  const counts = data?.counts;
  const isWindows = kind === "planning_window";

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="flex max-h-[90vh] max-w-5xl flex-col">
          <DialogHeader>
            <DialogTitle>{t("org:noSite.title")}</DialogTitle>
            <DialogDescription>{t("org:noSite.description")}</DialogDescription>
          </DialogHeader>

          <Tabs value={kind} onValueChange={(v) => setKind(v as NoSiteKind)}>
            <TabsList className="h-auto w-full flex-wrap justify-start">
              {KINDS.map((k) => (
                <TabsTrigger key={k} value={k} className="min-h-10">
                  {t(`org:noSite.kinds.${k}`, { count: countOf(counts, k) })}
                </TabsTrigger>
              ))}
            </TabsList>
          </Tabs>

          {isWindows ? (
            <p className="text-xs text-text-muted">{t("org:noSite.windowsHint")}</p>
          ) : null}

          <div className="min-h-0 flex-1 overflow-auto">
            {loadError != null ? (
              <ErrorState
                error={loadError}
                title={t("org:noSite.loadFailed")}
                onRetry={() => void load(kind)}
                retrying={loading}
              />
            ) : loading && data == null ? (
              <LoadingState />
            ) : records.length === 0 ? (
              <EmptyState
                title={t("org:noSite.emptyTitle")}
                description={t(`org:noSite.empty.${kind}`)}
              />
            ) : (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead className="w-10">
                      <Checkbox
                        aria-label={t("org:noSite.selectAll")}
                        checked={allSelected}
                        onCheckedChange={toggleAll}
                      />
                    </TableHead>
                    <TableHead>{t("org:noSite.columns.record")}</TableHead>
                    <TableHead>
                      {isWindows ? t("org:noSite.columns.review") : t("org:noSite.columns.status")}
                    </TableHead>
                    <TableHead>{t("org:noSite.columns.equipment")}</TableHead>
                    <TableHead>{t("org:noSite.columns.suggested")}</TableHead>
                    <TableHead>
                      {isWindows ? t("org:noSite.columns.period") : t("org:noSite.columns.date")}
                    </TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {records.map((r) => {
                    const label = r.code ?? r.title ?? `#${r.id}`;
                    return (
                      <TableRow key={r.id}>
                        <TableCell>
                          <Checkbox
                            aria-label={t("org:noSite.selectRow", { record: label })}
                            checked={selected.has(r.id)}
                            onCheckedChange={(checked) => toggle(r.id, checked)}
                          />
                        </TableCell>
                        <TableCell>
                          <div className="font-medium">{label}</div>
                          {r.code != null && r.title ? (
                            <div className="text-xs text-text-muted">{r.title}</div>
                          ) : null}
                        </TableCell>
                        <TableCell>
                          {isWindows ? (
                            <Badge variant={r.to_review ? "default" : "secondary"}>
                              {r.to_review
                                ? t("org:noSite.windowToReview")
                                : t("org:noSite.windowPlantWide")}
                            </Badge>
                          ) : (
                            statusLabel(r)
                          )}
                        </TableCell>
                        <TableCell>
                          {r.equipment_id != null ? (
                            <span>
                              {r.equipment_code ?? `#${r.equipment_id}`}
                              {r.equipment_name ? (
                                <span className="ml-1 text-xs text-text-muted">
                                  {r.equipment_name}
                                </span>
                              ) : null}
                            </span>
                          ) : (
                            <span className="text-text-muted">{t("org:noSite.noEquipment")}</span>
                          )}
                        </TableCell>
                        <TableCell>{r.suggested_node_name ?? "—"}</TableCell>
                        <TableCell className="whitespace-nowrap text-xs">
                          {isWindows
                            ? `${formatDateTime(r.window_start)} – ${formatDateTime(r.window_end)}`
                            : formatDate(r.created_at)}
                        </TableCell>
                      </TableRow>
                    );
                  })}
                </TableBody>
              </Table>
            )}
            {data != null ? (
              <TruncationNotice
                shown={records.length}
                total={countOf(counts, kind)}
                className="mt-2"
              />
            ) : null}
          </div>

          {data != null && records.length > 0 ? (
            <div className="space-y-3 border-t border-surface-border pt-3">
              <p className="text-sm text-text-secondary" role="status">
                {t("org:noSite.selected", { count: selected.size })}
              </p>
              {tooMany ? (
                <p className="text-xs text-destructive">
                  {t("org:noSite.tooMany", { max: maxBatch })}
                </p>
              ) : null}
              <div className="grid gap-3 md:grid-cols-2">
                <div className="space-y-1.5">
                  <Label htmlFor="no-site-node">{t("org:noSite.site")}</Label>
                  <ScopeNodeSelect
                    id="no-site-node"
                    nodes={data.nodes}
                    value={node}
                    onChange={(v) => {
                      setNode(v);
                      setNodeChosen(true);
                    }}
                    includePlant={false}
                    ariaLabel={t("org:noSite.site")}
                  />
                  <p className="text-xs text-text-muted">
                    {suggestion != null && !nodeChosen
                      ? t("org:noSite.siteHint")
                      : t("org:noSite.siteHintNone")}
                  </p>
                </div>
                <div className="space-y-1.5">
                  <Label htmlFor="no-site-reason">{t("org:noSite.reason")}</Label>
                  <Textarea
                    id="no-site-reason"
                    value={reason}
                    onChange={(e) => setReason(e.target.value)}
                    rows={2}
                    maxLength={1000}
                  />
                  <p className="text-xs text-text-muted">
                    {t("org:noSite.reasonHint", { min: minReason })}
                  </p>
                </div>
              </div>
              {assignError ? (
                <p className="text-sm text-destructive" role="alert">
                  {assignError}
                </p>
              ) : null}
            </div>
          ) : null}

          <DialogFooter>
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              {t("org:noSite.close")}
            </Button>
            {data != null && records.length > 0 ? (
              <Button onClick={() => void handleAssign()} disabled={!canAssign}>
                {t("org:noSite.confirm", { count: selected.size })}
              </Button>
            ) : null}
          </DialogFooter>
        </DialogContent>
      </Dialog>
      {StepUpDialogElement}
    </>
  );
}
