import { Printer } from "lucide-react";
import { type FormEvent, useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import {
  Badge,
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  FormField,
  Input,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Textarea,
} from "@/components/ui";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState } from "@/components/ui/data-state";
import { StatusChip } from "@/components/ui/status-chip";
import { PERMIT_STATUS_TONE, toneOf } from "@/design-system/tone";
import { useSession } from "@/hooks/use-session";
import { appErrorReason } from "@/lib/app-error";
import {
  type IsolationOption,
  type IsolationOptions,
  type LotoCardViewRecord,
  type PermitHandoverLogRecord,
  type PermitIsolationRecord,
  type PermitSuspensionRecord,
  type WorkPermitRecord,
  applyPermitIsolation,
  cancelPermitIsolation,
  getLotoCardView,
  listPermitHandoverLogs,
  listPermitIsolationOptions,
  listPermitIsolations,
  listPermitSuspensions,
  recordLotoCardPrint,
  setWorkPermitStatus,
  suspendWorkPermit,
  updateWorkPermit,
  upsertPermitIsolation,
  verifyPermitIsolation,
  verifyPermitIsolationRemoval,
} from "@/services/permit-service";
import { formatDateTime } from "@/utils/format-date";
import type { WorkPermitStatus } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import { PermitHandoverForm } from "./PermitHandoverForm";
import { PermitNoSiteBadge, usePermitAccess } from "./PermitSiteScope";
import { printLotoCard } from "./loto-card-print";
import {
  APPROVED_ON_STATUSES,
  ISOLATION_REMOVAL_STATUSES,
  ISOLATION_SETUP_STATUSES,
  PERMIT_EDITABLE_STATUSES,
  activationBlocker,
  canCancel,
  canCancelIsolation,
  closeBlocker,
  formatAssetLabel,
  isolationState,
  localInputToIso,
  nextTransitions,
  optionLabel,
  toLocalInputValue,
} from "./permit-rules";
import { usePermitTypeName } from "./permit-type-name";

interface PermitDetailPanelProps {
  permit: WorkPermitRecord;
  /** Called with the updated permit after any write so the page can refresh lists. */
  onChanged: (permit: WorkPermitRecord) => void;
}

/** Energy type and method are ids of the governed lists (`permit::energy_reference`). */
const EMPTY_ISOLATION = { point: "", energy: "", method: "", lock: "" };

export function PermitDetailPanel({ permit, onChanged }: PermitDetailPanelProps) {
  const { t, i18n } = useTranslation("permits");
  const typeName = usePermitTypeName();
  const { info: session } = useSession();
  // Every action is checked at the permit's site, as the backend does; no button while the rights
  // at that site load (an explicit gate, as the work-order gate card and the create dialog have).
  const access = usePermitAccess(permit);
  const canIssue = !access.loading && access.can(P.PTW_ISSUE);
  const canClose = !access.loading && access.can(P.PTW_CLOSE);
  const me = session?.user_id ?? null;

  const [isolations, setIsolations] = useState<PermitIsolationRecord[]>([]);
  const [suspensions, setSuspensions] = useState<PermitSuspensionRecord[]>([]);
  const [handovers, setHandovers] = useState<PermitHandoverLogRecord[]>([]);
  const [cardView, setCardView] = useState<LotoCardViewRecord | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** Isolations / suspensions / hand-overs could not be read: never shown as "none". */
  const [childrenError, setChildrenError] = useState<unknown>(null);
  const [childrenLoading, setChildrenLoading] = useState(true);
  const [options, setOptions] = useState<IsolationOptions | null>(null);
  const [optionsError, setOptionsError] = useState<unknown>(null);
  const [optionsReload, setOptionsReload] = useState(0);

  const [isoDraft, setIsoDraft] = useState(EMPTY_ISOLATION);
  const [isoSubmitted, setIsoSubmitted] = useState(false);
  const [suspendOpen, setSuspendOpen] = useState(false);
  const [suspendDraft, setSuspendDraft] = useState({ reason: "", conditions: "" });
  const [cancelOpen, setCancelOpen] = useState(false);
  /** The planned isolation line whose cancellation is being confirmed. */
  const [cancelLine, setCancelLine] = useState<PermitIsolationRecord | null>(null);
  /** The isolation whose lock removal waits for the confirmation dialog. */
  const [removalConfirm, setRemovalConfirm] = useState<PermitIsolationRecord | null>(null);
  const [editingExpiry, setEditingExpiry] = useState<string | null>(null);

  const fmt = (v: string | null) => (v ? formatDateTime(v, i18n.language) : t("detail.none"));
  /** Who did a step and when (four-eyes record); a person no longer readable is named as such. */
  const byAt = (name: string | null, at: string | null) =>
    t("detail.byAt", { name: name ?? t("detail.unknownPerson"), date: fmt(at) });

  const loadChildren = useCallback(async () => {
    setChildrenLoading(true);
    try {
      const [iso, susp, hand] = await Promise.all([
        listPermitIsolations(permit.id),
        listPermitSuspensions(permit.id),
        listPermitHandoverLogs(permit.id),
      ]);
      setIsolations(iso);
      setSuspensions(susp);
      setHandovers(hand);
      setChildrenError(null);
    } catch (e) {
      setChildrenError(e);
    } finally {
      setChildrenLoading(false);
    }
  }, [permit.id]);

  // Choices of the isolation form (energy types, the plant's methods), read with the permit right.
  useEffect(() => {
    let cancelled = false;
    setOptionsError(null);
    listPermitIsolationOptions()
      .then((o) => {
        if (!cancelled) setOptions(o);
      })
      .catch((e: unknown) => {
        if (!cancelled) setOptionsError(e);
      });
    return () => {
      cancelled = true;
    };
  }, [optionsReload]);

  const optionById = useMemo(() => {
    const map = new Map<number, IsolationOption>();
    for (const o of [...(options?.energy_types ?? []), ...(options?.isolation_methods ?? [])]) {
      map.set(o.id, o);
    }
    return map;
  }, [options]);

  const lang = i18n.resolvedLanguage || i18n.language;
  const energyLabel = (iso: PermitIsolationRecord) =>
    iso.energy_type_id != null
      ? optionLabel(optionById.get(iso.energy_type_id), lang) ||
        (iso.energy_type_label ?? t("isolation.notSet"))
      : t("isolation.notSet");
  const methodLabel = (iso: PermitIsolationRecord) =>
    iso.isolation_method_id != null
      ? optionLabel(optionById.get(iso.isolation_method_id), lang) ||
        (iso.isolation_method_label ?? t("isolation.notSet"))
      : t("isolation.notSet");

  useEffect(() => {
    setError(null);
    setChildrenError(null);
    setCardView(null);
    setEditingExpiry(null);
    void loadChildren();
  }, [loadChildren, permit.row_version]);

  /** Runs a write, then reloads children; surfaces backend errors inline. */
  const run = async (fn: () => Promise<WorkPermitRecord | void>): Promise<boolean> => {
    setBusy(true);
    setError(null);
    try {
      const updated = await fn();
      if (updated) onChanged(updated);
      await loadChildren();
      return true;
    } catch (e) {
      setError(appErrorReason(e));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const transition = (to: WorkPermitStatus) =>
    run(() =>
      setWorkPermitStatus({ id: permit.id, status: to, expected_row_version: permit.row_version }),
    );

  const blockerFor = (to: WorkPermitStatus): string | null => {
    // Without the isolations the safety gates cannot be explained: no lifecycle step offered.
    if (childrenError != null || childrenLoading) return "gate.isolationsUnavailable";
    if (to === "active") return activationBlocker(permit, isolations);
    if (to === "closed") return closeBlocker(isolations);
    if (to === "handed_back" && handovers.length === 0) return "gate.handoverRequired";
    return null;
  };

  const isolationAction = (iso: PermitIsolationRecord, fn: typeof applyPermitIsolation) =>
    run(async () => {
      await fn({ isolation_id: iso.id, expected_row_version: iso.row_version, lock_number: null });
    });

  const handleAddIsolation = async (e: FormEvent) => {
    e.preventDefault();
    setIsoSubmitted(true);
    if (!isoDraft.point.trim() || !isoDraft.energy || !isoDraft.method) return;
    const ok = await run(async () => {
      await upsertPermitIsolation({
        id: null,
        permit_id: permit.id,
        isolation_point: isoDraft.point,
        energy_type_id: Number(isoDraft.energy),
        isolation_method_id: Number(isoDraft.method),
        lock_number: isoDraft.lock || null,
      });
    });
    if (ok) {
      setIsoDraft(EMPTY_ISOLATION);
      setIsoSubmitted(false);
    }
  };

  const handleSuspend = async (e: FormEvent) => {
    e.preventDefault();
    if (!suspendDraft.reason.trim()) return;
    const ok = await run(async () => {
      const res = await suspendWorkPermit({
        permit_id: permit.id,
        expected_row_version: permit.row_version,
        reason: suspendDraft.reason,
        reactivation_conditions: suspendDraft.conditions || null,
      });
      return res.permit;
    });
    if (ok) {
      setSuspendOpen(false);
      setSuspendDraft({ reason: "", conditions: "" });
    }
  };

  const handleSaveExpiry = async () => {
    if (editingExpiry == null) return;
    const ok = await run(() =>
      updateWorkPermit({
        id: permit.id,
        linked_work_order_id: null,
        asset_id: null,
        entity_id: null,
        expires_at: localInputToIso(editingExpiry),
        expected_row_version: permit.row_version,
      }),
    );
    if (ok) setEditingExpiry(null);
  };

  const showCard = async (iso: PermitIsolationRecord) => {
    try {
      setCardView(await getLotoCardView(permit.id, iso.id));
    } catch (e) {
      setError(appErrorReason(e));
    }
  };

  const setupOpen = ISOLATION_SETUP_STATUSES.has(permit.status);
  const removalOpen = ISOLATION_REMOVAL_STATUSES.has(permit.status);
  const transitions = nextTransitions(permit.status);
  const blockers = [
    ...new Set(transitions.map((tr) => blockerFor(tr.to)).filter((b): b is string => b != null)),
  ];

  return (
    <section
      aria-label={t("detail.title", { code: permit.code })}
      className="space-y-5 rounded-lg border border-surface-border bg-surface-1 p-4"
    >
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div className="space-y-1">
          <div className="flex flex-wrap items-center gap-2">
            <h2 className="text-base font-semibold text-text-primary">
              {t("detail.title", { code: permit.code })}
            </h2>
            <PermitNoSiteBadge permit={permit} />
          </div>
          <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-sm">
            <dt className="text-text-muted">{t("detail.type")}</dt>
            <dd>
              {permit.permit_type_name == null
                ? t("detail.none")
                : typeName(permit.permit_type_code, permit.permit_type_name)}
            </dd>
            <dt className="text-text-muted">{t("detail.asset")}</dt>
            <dd>{formatAssetLabel(permit) ?? t("detail.none")}</dd>
            <dt className="text-text-muted">{t("detail.workOrder")}</dt>
            <dd>
              {permit.linked_work_order_code ? (
                <LinkedEntityBadge
                  entity="work_order"
                  code={permit.linked_work_order_code}
                  entityId={permit.linked_work_order_id}
                />
              ) : (
                t("detail.none")
              )}
            </dd>
            <dt className="text-text-muted">{t("detail.requestedAt")}</dt>
            <dd>{fmt(permit.requested_at)}</dd>
            <dt className="text-text-muted">{t("detail.activatedAt")}</dt>
            <dd>{fmt(permit.activated_at)}</dd>
            <dt className="text-text-muted">{t("detail.expiresAt")}</dt>
            <dd className="flex items-center gap-2">
              {editingExpiry != null ? (
                <>
                  <Input
                    aria-label={t("detail.expiresAt")}
                    type="datetime-local"
                    className="h-8 w-auto"
                    value={editingExpiry}
                    onChange={(e) => setEditingExpiry(e.target.value)}
                  />
                  <Button size="sm" disabled={busy} onClick={() => void handleSaveExpiry()}>
                    {t("actions.save")}
                  </Button>
                  <Button size="sm" variant="ghost" onClick={() => setEditingExpiry(null)}>
                    {t("actions.cancel")}
                  </Button>
                </>
              ) : (
                <>
                  {fmt(permit.expires_at)}
                  {canIssue && PERMIT_EDITABLE_STATUSES.has(permit.status) ? (
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={() =>
                        setEditingExpiry(
                          toLocalInputValue(
                            permit.expires_at ? new Date(permit.expires_at) : new Date(),
                          ),
                        )
                      }
                    >
                      {t("actions.edit")}
                    </Button>
                  ) : null}
                </>
              )}
            </dd>
            {permit.closed_at ? (
              <>
                <dt className="text-text-muted">{t("detail.closedAt")}</dt>
                <dd>{fmt(permit.closed_at)}</dd>
              </>
            ) : null}
            {permit.handed_back_at ? (
              <>
                <dt className="text-text-muted">{t("detail.handedBackAt")}</dt>
                <dd>{fmt(permit.handed_back_at)}</dd>
              </>
            ) : null}
            {permit.created_by_id != null ? (
              <>
                <dt className="text-text-muted">{t("detail.createdBy")}</dt>
                <dd>{permit.created_by_name ?? t("detail.unknownPerson")}</dd>
              </>
            ) : null}
            {permit.submitted_at != null ? (
              <>
                <dt className="text-text-muted">{t("detail.submittedBy")}</dt>
                <dd>{byAt(permit.submitted_by_name, permit.submitted_at)}</dd>
              </>
            ) : null}
            {permit.approved_at != null ? (
              <>
                <dt className="text-text-muted">{t("detail.approvedBy")}</dt>
                <dd className="flex flex-wrap items-center gap-2" data-testid="permit-approver">
                  {byAt(permit.approved_by_name, permit.approved_at)}
                  {permit.self_approved ? (
                    <StatusChip
                      label={t("detail.selfApproved")}
                      tone="warning"
                      title={t("detail.selfApprovedHint")}
                    />
                  ) : null}
                </dd>
              </>
            ) : APPROVED_ON_STATUSES.has(permit.status) ? (
              <>
                <dt className="text-text-muted">{t("detail.approvedBy")}</dt>
                <dd
                  className="text-text-muted"
                  data-testid="permit-approver"
                  title={t("detail.approverNotRecordedHint")}
                >
                  {t("detail.approverNotRecorded")}
                </dd>
              </>
            ) : null}
          </dl>
        </div>
        <span data-testid="permit-status">
          <StatusChip
            size="md"
            label={t(`status.${permit.status}`, { defaultValue: t("status.unknown") })}
            tone={toneOf(PERMIT_STATUS_TONE, permit.status)}
          />
        </span>
      </header>

      <div className="space-y-2">
        <div className="flex flex-wrap gap-2">
          {transitions.map((tr) => {
            const allowed = tr.permission === P.PTW_CLOSE ? canClose : canIssue;
            if (!allowed) return null;
            const blocker = blockerFor(tr.to);
            return (
              <Button
                key={tr.to}
                size="sm"
                disabled={busy || blocker != null}
                title={blocker ? t(blocker) : undefined}
                onClick={() => void transition(tr.to)}
              >
                {t(tr.labelKey)}
              </Button>
            );
          })}
          {permit.status === "active" && canIssue ? (
            <Button
              size="sm"
              variant="outline"
              disabled={busy}
              onClick={() => setSuspendOpen(true)}
            >
              {t("transition.suspend")}
            </Button>
          ) : null}
          {canCancel(permit.status) && canClose ? (
            <Button
              size="sm"
              variant="destructive"
              disabled={busy}
              onClick={() => setCancelOpen(true)}
            >
              {t("transition.cancel")}
            </Button>
          ) : null}
        </div>
        {blockers.map((b) => (
          <p key={b} className="text-xs text-tone-warning-fg">
            {t(b)}
          </p>
        ))}
      </div>

      {error ? (
        <p
          role="alert"
          className="rounded-md border border-tone-danger-border bg-tone-danger-bg px-3 py-2 text-sm text-tone-danger-fg"
        >
          {error}
        </p>
      ) : null}

      <section className="space-y-2">
        <h3 className="text-sm font-medium text-text-primary">{t("isolation.title")}</h3>
        <p className="text-xs text-text-muted">{t("isolation.twoPersonHint")}</p>
        {childrenError != null ? (
          <ErrorState
            error={childrenError}
            title={t("isolation.loadError")}
            onRetry={() => void loadChildren()}
            retrying={childrenLoading}
          />
        ) : childrenLoading && isolations.length === 0 ? (
          <p className="text-sm text-text-muted" aria-busy="true">
            {t("isolation.loading")}
          </p>
        ) : isolations.length === 0 ? (
          <p className="text-sm text-text-muted">{t("isolation.empty")}</p>
        ) : (
          <ul className="space-y-2">
            {isolations.map((iso) => {
              const state = isolationState(iso);
              const appliedByMe = me != null && iso.applied_by_id === me;
              // Three-person chain of the permit's type: the isolation's verifier may not confirm
              // its removal either (the server refuses it too: PERMIT_THREE_PERSON_REMOVAL).
              const chainApplies =
                permit.permit_type_requires_three_person_removal &&
                state === "verified" &&
                removalOpen;
              const verifiedByMe = me != null && iso.verified_by_id === me;
              const removalBlockedForMe = appliedByMe || (chainApplies && verifiedByMe);
              const removalBlockedReason = appliedByMe
                ? t("isolation.selfBlocked")
                : chainApplies && verifiedByMe
                  ? t("isolation.threePersonSelf")
                  : undefined;
              return (
                <li
                  key={iso.id}
                  data-testid="permit-isolation"
                  data-cancelled={state === "cancelled" ? "true" : undefined}
                  className={`space-y-1 rounded-md border border-surface-border p-2 text-sm${
                    state === "cancelled" ? " bg-surface-0 opacity-60" : ""
                  }`}
                >
                  <div className="flex flex-wrap items-center gap-2">
                    <span
                      className={
                        state === "cancelled"
                          ? "font-medium text-text-muted line-through"
                          : "font-medium text-text-primary"
                      }
                    >
                      {iso.isolation_point}
                    </span>
                    <span className="text-text-muted">
                      {energyLabel(iso)} · {methodLabel(iso)}
                    </span>
                    {iso.mapping_review_required && state !== "cancelled" ? (
                      <StatusChip
                        label={t("isolation.toReview")}
                        tone="attention"
                        title={t("isolation.toReviewHint")}
                      />
                    ) : null}
                    {iso.lock_number ? (
                      <span className="font-mono text-xs text-text-muted">
                        {t("card.lock")} {iso.lock_number}
                      </span>
                    ) : null}
                    <Badge variant="outline" className="ml-auto">
                      {t(`isolation.state.${state}`)}
                    </Badge>
                  </div>
                  {iso.energy_type.trim() || iso.isolation_method.trim() ? (
                    <p className="text-xs text-text-muted" data-testid="isolation-original-entry">
                      {t("isolation.originalEntry", {
                        text: [iso.energy_type.trim(), iso.isolation_method.trim()]
                          .filter(Boolean)
                          .join(" · "),
                      })}
                    </p>
                  ) : null}
                  <div className="flex flex-wrap gap-x-4 text-xs text-text-muted">
                    {iso.applied_by_name ? (
                      <span>{t("isolation.appliedBy", { name: iso.applied_by_name })}</span>
                    ) : null}
                    {iso.verified_by_name ? (
                      <span>{t("isolation.verifiedBy", { name: iso.verified_by_name })}</span>
                    ) : null}
                    {iso.removal_verified_by_name ? (
                      <span>
                        {t("isolation.removalVerifiedBy", { name: iso.removal_verified_by_name })}
                      </span>
                    ) : null}
                    {iso.cancelled_at ? (
                      <span data-testid="isolation-cancelled-by">
                        {iso.cancelled_by_name
                          ? t("isolation.cancelledBy", {
                              name: iso.cancelled_by_name,
                              date: fmt(iso.cancelled_at),
                            })
                          : t("isolation.cancelledAt", { date: fmt(iso.cancelled_at) })}
                      </span>
                    ) : null}
                  </div>
                  <div className="flex flex-wrap items-center gap-2">
                    {state === "pending" && setupOpen && canIssue ? (
                      <Button
                        size="sm"
                        variant="outline"
                        disabled={busy}
                        onClick={() => void isolationAction(iso, applyPermitIsolation)}
                      >
                        {t("isolation.apply")}
                      </Button>
                    ) : null}
                    {state === "applied" && setupOpen && canIssue ? (
                      <Button
                        size="sm"
                        variant="outline"
                        disabled={busy || appliedByMe}
                        title={appliedByMe ? t("isolation.selfBlocked") : undefined}
                        onClick={() => void isolationAction(iso, verifyPermitIsolation)}
                      >
                        {t("isolation.verify")}
                      </Button>
                    ) : null}
                    {state === "verified" && removalOpen && canClose ? (
                      <Button
                        size="sm"
                        variant="outline"
                        disabled={busy || removalBlockedForMe}
                        title={removalBlockedReason}
                        onClick={() => setRemovalConfirm(iso)}
                      >
                        {t("isolation.verifyRemoval")}
                      </Button>
                    ) : null}
                    {state === "verified" || state === "removed" ? (
                      <Button size="sm" variant="ghost" onClick={() => void showCard(iso)}>
                        {t("card.show")}
                      </Button>
                    ) : null}
                    {canIssue && canCancelIsolation(permit.status, iso) ? (
                      <Button
                        size="sm"
                        variant="ghost"
                        disabled={busy}
                        title={t("isolation.cancelHint")}
                        onClick={() => setCancelLine(iso)}
                      >
                        {t("isolation.cancel")}
                      </Button>
                    ) : null}
                    {appliedByMe && (state === "applied" || state === "verified") ? (
                      <span className="text-xs text-text-muted">{t("isolation.selfBlocked")}</span>
                    ) : null}
                  </div>
                  {chainApplies ? (
                    <p
                      className="text-xs text-tone-warning-fg"
                      data-testid="isolation-three-person"
                    >
                      {verifiedByMe
                        ? t("isolation.threePersonSelf")
                        : iso.verified_by_name
                          ? t("isolation.threePersonNotice", { name: iso.verified_by_name })
                          : t("isolation.threePersonNoticeUnknown")}
                    </p>
                  ) : null}
                </li>
              );
            })}
          </ul>
        )}

        {cardView ? (
          <div className="rounded border border-surface-border bg-surface-0 p-3 font-mono text-xs leading-relaxed text-text-primary">
            <div className="mb-1 font-sans text-sm font-medium">{t("card.title")}</div>
            <div>
              {t("card.permit")}: {cardView.permit_code}
            </div>
            <div>
              {t("card.equipment")}: {cardView.equipment_label}
            </div>
            <div>
              {t("card.energy")}: {cardView.energy_type}
            </div>
            <div>
              {t("card.point")}: {cardView.isolation_point}
            </div>
            {cardView.lock_number ? (
              <div>
                {t("card.lock")}: {cardView.lock_number}
              </div>
            ) : null}
            <div>
              {t("card.verifier")}: {cardView.verifier_signature ?? t("detail.none")}
            </div>
            <div>
              {t("card.expiry")}: {fmt(cardView.expires_at)}
            </div>
            {canIssue ? (
              <Button
                size="sm"
                variant="outline"
                className="mt-2 h-11 gap-1.5 font-sans"
                disabled={busy}
                onClick={() =>
                  void run(async () => {
                    // Print first; the audit entry records a print that actually happened.
                    await printLotoCard(
                      cardView,
                      (key) => t(key),
                      i18n.resolvedLanguage || i18n.language,
                    );
                    await recordLotoCardPrint({
                      permit_id: permit.id,
                      isolation_id: cardView.isolation_id,
                    });
                  })
                }
              >
                <Printer className="h-4 w-4" aria-hidden />
                {t("card.print")}
              </Button>
            ) : null}
          </div>
        ) : null}

        {canIssue && setupOpen ? (
          <form
            className="grid gap-2 rounded-md border border-dashed border-surface-border p-2 sm:grid-cols-4"
            onSubmit={(e) => void handleAddIsolation(e)}
          >
            <FormField
              name="iso-point"
              label={t("isolation.point")}
              required
              error={isoSubmitted && !isoDraft.point.trim() ? t("errors.required") : undefined}
            >
              <Input
                id="iso-point"
                value={isoDraft.point}
                onChange={(e) => setIsoDraft((d) => ({ ...d, point: e.target.value }))}
              />
            </FormField>
            <FormField
              name="iso-energy"
              label={t("isolation.energy")}
              required
              error={isoSubmitted && !isoDraft.energy ? t("errors.required") : undefined}
            >
              <Select
                value={isoDraft.energy}
                onValueChange={(v) => setIsoDraft((d) => ({ ...d, energy: v }))}
                disabled={options == null}
              >
                <SelectTrigger id="iso-energy" aria-label={t("isolation.energy")}>
                  <SelectValue placeholder={t("isolation.energyPlaceholder")} />
                </SelectTrigger>
                <SelectContent>
                  {(options?.energy_types ?? []).map((o) => (
                    <SelectItem key={o.id} value={String(o.id)}>
                      {optionLabel(o, lang)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </FormField>
            <FormField
              name="iso-method"
              label={t("isolation.method")}
              required
              error={isoSubmitted && !isoDraft.method ? t("errors.required") : undefined}
            >
              <Select
                value={isoDraft.method}
                onValueChange={(v) => setIsoDraft((d) => ({ ...d, method: v }))}
                disabled={options == null}
              >
                <SelectTrigger id="iso-method" aria-label={t("isolation.method")}>
                  <SelectValue placeholder={t("isolation.methodPlaceholder")} />
                </SelectTrigger>
                <SelectContent>
                  {(options?.isolation_methods ?? []).map((o) => (
                    <SelectItem key={o.id} value={String(o.id)}>
                      {optionLabel(o, lang)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </FormField>
            <FormField name="iso-lock" label={t("isolation.lock")}>
              <Input
                id="iso-lock"
                value={isoDraft.lock}
                onChange={(e) => setIsoDraft((d) => ({ ...d, lock: e.target.value }))}
              />
            </FormField>
            <div className="space-y-2 sm:col-span-4">
              {optionsError != null ? (
                <div
                  role="alert"
                  className="flex flex-wrap items-center gap-2 text-xs text-tone-danger-fg"
                >
                  <span>
                    {t("isolation.optionsError")} {appErrorReason(optionsError)}
                  </span>
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    onClick={() => setOptionsReload((n) => n + 1)}
                  >
                    {t("actions.retry")}
                  </Button>
                </div>
              ) : null}
              <p className="text-xs text-text-muted">{t("isolation.listsHint")}</p>
              <Button type="submit" size="sm" disabled={busy || options == null}>
                {t("isolation.add")}
              </Button>
            </div>
          </form>
        ) : permit.status === "active" && canIssue ? (
          <p className="text-xs text-text-muted">{t("isolation.editLocked")}</p>
        ) : null}
      </section>

      <div className="grid gap-4 md:grid-cols-2">
        <section className="space-y-2">
          <h3 className="text-sm font-medium text-text-primary">{t("suspension.title")}</h3>
          {suspensions.length === 0 ? (
            <p className="text-sm text-text-muted">{t("suspension.empty")}</p>
          ) : (
            <ul className="space-y-2 text-sm">
              {suspensions.map((s) => (
                <li key={s.id} className="rounded border border-surface-border/60 p-2">
                  <div className="text-text-primary">
                    {fmt(s.suspended_at)} — {s.reason}
                  </div>
                  <div className="text-xs text-text-muted">
                    {t("suspension.suspendedBy", {
                      name: s.suspended_by_name ?? t("detail.none"),
                    })}
                  </div>
                  {s.reactivation_conditions ? (
                    <div className="text-xs text-text-muted">{s.reactivation_conditions}</div>
                  ) : null}
                  {s.reinstated_at ? (
                    <div className="text-xs text-text-muted">
                      {t("suspension.reinstated", {
                        at: fmt(s.reinstated_at),
                        name: s.reinstated_by_name ?? t("detail.none"),
                      })}
                    </div>
                  ) : null}
                </li>
              ))}
            </ul>
          )}
        </section>

        <section className="space-y-2">
          <h3 className="text-sm font-medium text-text-primary">{t("handover.title")}</h3>
          {handovers.length === 0 ? (
            <p className="text-sm text-text-muted">{t("handover.empty")}</p>
          ) : (
            <ol className="space-y-2 text-sm">
              {handovers.map((h) => (
                <li key={h.id} className="rounded-md border border-surface-border bg-surface-1 p-2">
                  <div className="flex flex-wrap items-center gap-1.5 text-text-primary">
                    <span className="font-medium">{h.handed_from_label ?? h.handed_from_role}</span>
                    <span aria-hidden className="text-text-muted">
                      →
                    </span>
                    <span className="sr-only">{t("handover.handedTo")}</span>
                    <span className="font-medium">{h.handed_to_label ?? h.handed_to_role}</span>
                    <time className="ms-auto text-xs text-text-muted" dateTime={h.signed_at}>
                      {fmt(h.signed_at)}
                    </time>
                  </div>
                  {h.confirmation_note ? (
                    <p className="mt-1 text-xs text-text-secondary">{h.confirmation_note}</p>
                  ) : null}
                  <p className="mt-1 text-xs text-text-muted">
                    {h.signed_by_name
                      ? t("handover.signedBy", { name: h.signed_by_name })
                      : t("handover.signerNotRecorded")}
                    {h.received_by_name
                      ? ` · ${t("handover.receivedBy", { name: h.received_by_name })}`
                      : ""}
                  </p>
                </li>
              ))}
            </ol>
          )}
          {permit.status === "closed" && canClose ? (
            <PermitHandoverForm permitId={permit.id} onRecorded={() => void loadChildren()} />
          ) : canClose ? (
            <p className="text-xs text-text-muted">{t("handover.onlyWhenClosed")}</p>
          ) : null}
        </section>
      </div>

      <Dialog open={suspendOpen} onOpenChange={setSuspendOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("suspension.dialogTitle")}</DialogTitle>
            <DialogDescription>{permit.code}</DialogDescription>
          </DialogHeader>
          <form className="space-y-3" onSubmit={(e) => void handleSuspend(e)}>
            <FormField name="suspend-reason" label={t("suspension.reason")} required>
              <Textarea
                id="suspend-reason"
                rows={2}
                value={suspendDraft.reason}
                onChange={(e) => setSuspendDraft((d) => ({ ...d, reason: e.target.value }))}
              />
            </FormField>
            <FormField name="suspend-conditions" label={t("suspension.conditions")}>
              <Textarea
                id="suspend-conditions"
                rows={2}
                value={suspendDraft.conditions}
                onChange={(e) => setSuspendDraft((d) => ({ ...d, conditions: e.target.value }))}
              />
            </FormField>
            <DialogFooter>
              <Button type="button" variant="outline" onClick={() => setSuspendOpen(false)}>
                {t("actions.cancel")}
              </Button>
              <Button type="submit" disabled={busy || !suspendDraft.reason.trim()}>
                {t("suspension.submit")}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={removalConfirm != null}
        onOpenChange={(open) => {
          if (!open) setRemovalConfirm(null);
        }}
        title={t("isolation.removalConfirmTitle")}
        description={t("isolation.removalConfirmDescription", {
          point: removalConfirm?.isolation_point ?? "",
          applier: removalConfirm?.applied_by_name ?? t("isolation.personUnknown"),
          verifier: removalConfirm?.verified_by_name ?? t("isolation.personUnknown"),
        })}
        confirmLabel={t("isolation.verifyRemoval")}
        onConfirm={async () => {
          const iso = removalConfirm;
          if (!iso) return;
          // A refusal is shown on the permit (translated); the dialog closes so it is visible.
          await isolationAction(iso, verifyPermitIsolationRemoval);
        }}
      />

      <ConfirmDialog
        open={cancelOpen}
        onOpenChange={setCancelOpen}
        title={t("transition.cancel")}
        description={t("transition.confirmCancel")}
        confirmLabel={t("transition.cancel")}
        destructive
        onConfirm={async () => {
          const ok = await transition("cancelled");
          // Keep the dialog open on failure (the message is shown on the permit).
          if (!ok) throw new Error("cancel refused");
        }}
      />
      <ConfirmDialog
        open={cancelLine != null}
        onOpenChange={(open) => {
          if (!open) setCancelLine(null);
        }}
        title={t("isolation.cancelTitle")}
        description={
          cancelLine
            ? t("isolation.cancelConfirm", {
                point: cancelLine.isolation_point,
                energy: energyLabel(cancelLine),
                method: methodLabel(cancelLine),
              })
            : ""
        }
        confirmLabel={t("isolation.cancel")}
        dismissLabel={t("isolation.cancelKeep")}
        destructive
        onConfirm={async () => {
          const line = cancelLine;
          if (!line) return;
          const ok = await run(async () => {
            const res = await cancelPermitIsolation({
              isolation_id: line.id,
              expected_row_version: line.row_version,
            });
            return res.permit;
          });
          // Keep the dialog open on failure (the message is shown on the permit).
          if (!ok) throw new Error("isolation cancel refused");
        }}
      >
        {permit.status === "pending_review" ? (
          <p className="text-sm text-tone-warning-fg" data-testid="isolation-cancel-reset-notice">
            {t("isolation.cancelResetNotice")}
          </p>
        ) : null}
      </ConfirmDialog>
    </section>
  );
}
