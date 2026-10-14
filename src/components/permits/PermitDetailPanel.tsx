import { Printer } from "lucide-react";
import { type FormEvent, useCallback, useEffect, useState } from "react";
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
  Textarea,
} from "@/components/ui";
import { StatusChip } from "@/components/ui/status-chip";
import { PERMIT_STATUS_TONE, toneOf } from "@/design-system/tone";
import { usePermissions } from "@/hooks/use-permissions";
import { useSession } from "@/hooks/use-session";
import { describeAppError } from "@/lib/app-error";
import {
  type LotoCardViewRecord,
  type PermitHandoverLogRecord,
  type PermitIsolationRecord,
  type PermitSuspensionRecord,
  type WorkPermitRecord,
  appendPermitHandoverLog,
  applyPermitIsolation,
  getLotoCardView,
  listPermitHandoverLogs,
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

import { printLotoCard } from "./loto-card-print";
import {
  ISOLATION_REMOVAL_STATUSES,
  ISOLATION_SETUP_STATUSES,
  PERMIT_EDITABLE_STATUSES,
  activationBlocker,
  canCancel,
  closeBlocker,
  formatAssetLabel,
  isolationState,
  localInputToIso,
  nextTransitions,
  toLocalInputValue,
} from "./permit-rules";

interface PermitDetailPanelProps {
  permit: WorkPermitRecord;
  /** Called with the updated permit after any write so the page can refresh lists. */
  onChanged: (permit: WorkPermitRecord) => void;
}

const EMPTY_ISOLATION = { point: "", energy: "", method: "", lock: "" };

export function PermitDetailPanel({ permit, onChanged }: PermitDetailPanelProps) {
  const { t, i18n } = useTranslation("permits");
  const { can } = usePermissions();
  const { info: session } = useSession();
  const canIssue = can(P.PTW_ISSUE);
  const canClose = can(P.PTW_CLOSE);
  const me = session?.user_id ?? null;

  const [isolations, setIsolations] = useState<PermitIsolationRecord[]>([]);
  const [suspensions, setSuspensions] = useState<PermitSuspensionRecord[]>([]);
  const [handovers, setHandovers] = useState<PermitHandoverLogRecord[]>([]);
  const [cardView, setCardView] = useState<LotoCardViewRecord | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [isoDraft, setIsoDraft] = useState(EMPTY_ISOLATION);
  const [isoSubmitted, setIsoSubmitted] = useState(false);
  const [handover, setHandover] = useState({ from: "", to: "", note: "" });
  const [handoverSubmitted, setHandoverSubmitted] = useState(false);
  const [suspendOpen, setSuspendOpen] = useState(false);
  const [suspendDraft, setSuspendDraft] = useState({ reason: "", conditions: "" });
  const [cancelOpen, setCancelOpen] = useState(false);
  const [editingExpiry, setEditingExpiry] = useState<string | null>(null);

  const fmt = (v: string | null) => (v ? formatDateTime(v, i18n.language) : t("detail.none"));

  const loadChildren = useCallback(async () => {
    try {
      const [iso, susp, hand] = await Promise.all([
        listPermitIsolations(permit.id),
        listPermitSuspensions(permit.id),
        listPermitHandoverLogs(permit.id),
      ]);
      setIsolations(iso);
      setSuspensions(susp);
      setHandovers(hand);
    } catch (e) {
      setError(describeAppError(e).title);
    }
  }, [permit.id]);

  useEffect(() => {
    setError(null);
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
      setError(describeAppError(e).title);
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
    if (!isoDraft.point.trim() || !isoDraft.energy.trim() || !isoDraft.method.trim()) return;
    const ok = await run(async () => {
      await upsertPermitIsolation({
        id: null,
        permit_id: permit.id,
        isolation_point: isoDraft.point,
        energy_type: isoDraft.energy,
        isolation_method: isoDraft.method,
        lock_number: isoDraft.lock || null,
      });
    });
    if (ok) {
      setIsoDraft(EMPTY_ISOLATION);
      setIsoSubmitted(false);
    }
  };

  const handleHandover = async (e: FormEvent) => {
    e.preventDefault();
    setHandoverSubmitted(true);
    if (!handover.from.trim() || !handover.to.trim()) return;
    const ok = await run(async () => {
      await appendPermitHandoverLog({
        permit_id: permit.id,
        handed_from_role: handover.from,
        handed_to_role: handover.to,
        confirmation_note: handover.note,
      });
    });
    if (ok) {
      setHandover({ from: "", to: "", note: "" });
      setHandoverSubmitted(false);
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
      setError(describeAppError(e).title);
    }
  };

  const setupOpen = ISOLATION_SETUP_STATUSES.has(permit.status);
  const removalOpen = ISOLATION_REMOVAL_STATUSES.has(permit.status);
  const transitions = nextTransitions(permit.status);
  const blockers = transitions.map((tr) => blockerFor(tr.to)).filter((b): b is string => b != null);

  return (
    <section
      aria-label={t("detail.title", { code: permit.code })}
      className="space-y-5 rounded-lg border border-surface-border bg-surface-1 p-4"
    >
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div className="space-y-1">
          <h2 className="text-base font-semibold text-text-primary">
            {t("detail.title", { code: permit.code })}
          </h2>
          <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-sm">
            <dt className="text-text-muted">{t("detail.type")}</dt>
            <dd>{permit.permit_type_name ?? t("detail.none")}</dd>
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
          <p key={b} className="text-xs text-status-warning">
            {t(b)}
          </p>
        ))}
      </div>

      {error ? (
        <p
          role="alert"
          className="rounded-md border border-status-danger/40 px-3 py-2 text-sm text-status-danger"
        >
          {error}
        </p>
      ) : null}

      <section className="space-y-2">
        <h3 className="text-sm font-medium text-text-primary">{t("isolation.title")}</h3>
        <p className="text-xs text-text-muted">{t("isolation.twoPersonHint")}</p>
        {isolations.length === 0 ? (
          <p className="text-sm text-text-muted">{t("isolation.empty")}</p>
        ) : (
          <ul className="space-y-2">
            {isolations.map((iso) => {
              const state = isolationState(iso);
              const appliedByMe = me != null && iso.applied_by_id === me;
              return (
                <li
                  key={iso.id}
                  data-testid="permit-isolation"
                  className="space-y-1 rounded-md border border-surface-border p-2 text-sm"
                >
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="font-medium text-text-primary">{iso.isolation_point}</span>
                    <span className="text-text-muted">
                      {iso.energy_type} · {iso.isolation_method}
                    </span>
                    {iso.lock_number ? (
                      <span className="font-mono text-xs text-text-muted">
                        {t("card.lock")} {iso.lock_number}
                      </span>
                    ) : null}
                    <Badge variant="outline" className="ml-auto">
                      {t(`isolation.state.${state}`)}
                    </Badge>
                  </div>
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
                        disabled={busy || appliedByMe}
                        title={appliedByMe ? t("isolation.selfBlocked") : undefined}
                        onClick={() => void isolationAction(iso, verifyPermitIsolationRemoval)}
                      >
                        {t("isolation.verifyRemoval")}
                      </Button>
                    ) : null}
                    {state === "verified" || state === "removed" ? (
                      <Button size="sm" variant="ghost" onClick={() => void showCard(iso)}>
                        {t("card.show")}
                      </Button>
                    ) : null}
                    {appliedByMe && (state === "applied" || state === "verified") ? (
                      <span className="text-xs text-text-muted">{t("isolation.selfBlocked")}</span>
                    ) : null}
                  </div>
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
              error={isoSubmitted && !isoDraft.energy.trim() ? t("errors.required") : undefined}
            >
              <Input
                id="iso-energy"
                value={isoDraft.energy}
                onChange={(e) => setIsoDraft((d) => ({ ...d, energy: e.target.value }))}
              />
            </FormField>
            <FormField
              name="iso-method"
              label={t("isolation.method")}
              required
              error={isoSubmitted && !isoDraft.method.trim() ? t("errors.required") : undefined}
            >
              <Input
                id="iso-method"
                value={isoDraft.method}
                onChange={(e) => setIsoDraft((d) => ({ ...d, method: e.target.value }))}
              />
            </FormField>
            <FormField name="iso-lock" label={t("isolation.lock")}>
              <Input
                id="iso-lock"
                value={isoDraft.lock}
                onChange={(e) => setIsoDraft((d) => ({ ...d, lock: e.target.value }))}
              />
            </FormField>
            <div className="sm:col-span-4">
              <Button type="submit" size="sm" disabled={busy}>
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
            <ul className="space-y-2 text-sm">
              {handovers.map((h) => (
                <li key={h.id} className="rounded border border-surface-border/60 p-2">
                  <div className="text-text-primary">
                    {h.handed_from_role} → {h.handed_to_role}
                  </div>
                  {h.confirmation_note ? (
                    <div className="text-xs text-text-muted">{h.confirmation_note}</div>
                  ) : null}
                  <div className="text-xs text-text-muted">{fmt(h.signed_at)}</div>
                </li>
              ))}
            </ul>
          )}
          {permit.status === "closed" && canClose ? (
            <form className="space-y-2" onSubmit={(e) => void handleHandover(e)}>
              <FormField
                name="handover-from"
                label={t("handover.from")}
                required
                error={
                  handoverSubmitted && !handover.from.trim() ? t("errors.required") : undefined
                }
              >
                <Input
                  id="handover-from"
                  value={handover.from}
                  onChange={(e) => setHandover((h) => ({ ...h, from: e.target.value }))}
                />
              </FormField>
              <FormField
                name="handover-to"
                label={t("handover.to")}
                required
                error={handoverSubmitted && !handover.to.trim() ? t("errors.required") : undefined}
              >
                <Input
                  id="handover-to"
                  value={handover.to}
                  onChange={(e) => setHandover((h) => ({ ...h, to: e.target.value }))}
                />
              </FormField>
              <FormField name="handover-note" label={t("handover.note")}>
                <Textarea
                  id="handover-note"
                  rows={2}
                  value={handover.note}
                  onChange={(e) => setHandover((h) => ({ ...h, note: e.target.value }))}
                />
              </FormField>
              <Button type="submit" size="sm" disabled={busy}>
                {t("handover.submit")}
              </Button>
            </form>
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

      <Dialog open={cancelOpen} onOpenChange={setCancelOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("transition.cancel")}</DialogTitle>
            <DialogDescription>{t("transition.confirmCancel")}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setCancelOpen(false)}>
              {t("actions.close")}
            </Button>
            <Button
              variant="destructive"
              disabled={busy}
              onClick={() =>
                void transition("cancelled").then((ok) => {
                  if (ok) setCancelOpen(false);
                })
              }
            >
              {t("transition.cancel")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </section>
  );
}
