/**
 * "Téléphones": the technicians' phones of the plant, their licence slots, and the administrator's
 * two actions on them: enrol one (admin-issued single-use QR, the licence key is never given to a
 * phone) and revoke one. Issuing and revoking ask for a fresh step-up.
 *
 * The QR payload and the typed code are secrets of a single-use enrolment: they stay in this
 * component's state, are shown on screen only, and are never logged.
 */
import type { ColumnDef } from "@tanstack/react-table";
import { Smartphone } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { DataTable } from "@/components/data/DataTable";
import { useIdentificationQr } from "@/components/identification/useIdentificationQr";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { ErrorState } from "@/components/ui/data-state";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useFormatters } from "@/hooks/use-formatters";
import { isStepUpCancelled, useStepUp } from "@/hooks/use-step-up";
import {
  type EnrolmentCandidates,
  type IssuedMobileEnrolment,
  type MobileEnrolmentStatus,
  type MobilePhone,
  type MobilePhonesView,
  cancelMobileEnrolment,
  getMobileEnrolmentStatus,
  issueMobileEnrolment,
  listMobileEnrolmentCandidates,
  listMobilePhones,
  revokeMobilePhone,
} from "@/services/mobile-enrolment-service";
import { pushAppToast } from "@/store/app-toast-store";

/** How often the open QR dialog asks the licence server whether the phone used the enrolment. */
const STATUS_POLL_MS = 3000;

function minutesSeconds(totalSeconds: number): string {
  const s = Math.max(0, totalSeconds);
  const mm = String(Math.floor(s / 60)).padStart(2, "0");
  const ss = String(s % 60).padStart(2, "0");
  return `${mm}:${ss}`;
}

/** Seconds left until `expiresAt`, re-read every second. */
function useSecondsLeft(expiresAt: string | null): number {
  const read = useCallback(
    () =>
      expiresAt ? Math.max(0, Math.floor((new Date(expiresAt).getTime() - Date.now()) / 1000)) : 0,
    [expiresAt],
  );
  const [left, setLeft] = useState(read);
  useEffect(() => {
    setLeft(read());
    if (!expiresAt) return undefined;
    const timer = setInterval(() => setLeft(read()), 1000);
    return () => clearInterval(timer);
  }, [expiresAt, read]);
  return left;
}

// ── Enrolment dialog ────────────────────────────────────────────────────────────────────────────

type EnrolStatus = MobileEnrolmentStatus["status"];

function EnrolDialog({
  open,
  onClose,
  onEnrolled,
}: {
  open: boolean;
  onClose: () => void;
  onEnrolled: () => void;
}) {
  const { t } = useTranslation("admin");
  const { withStepUp, StepUpDialogElement } = useStepUp();

  const [candidates, setCandidates] = useState<EnrolmentCandidates | null>(null);
  const [candidatesError, setCandidatesError] = useState<unknown>(null);
  const [personId, setPersonId] = useState("");
  const [siteId, setSiteId] = useState("");
  const [issued, setIssued] = useState<IssuedMobileEnrolment | null>(null);
  const [status, setStatus] = useState<EnrolStatus>("pending");
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const onEnrolledRef = useRef(onEnrolled);
  onEnrolledRef.current = onEnrolled;

  // A fresh form each time the dialog opens.
  useEffect(() => {
    if (!open) return undefined;
    setPersonId("");
    setSiteId("");
    setIssued(null);
    setStatus("pending");
    setCandidates(null);
    setCandidatesError(null);
    let cancelled = false;
    listMobileEnrolmentCandidates()
      .then((c) => {
        if (!cancelled) setCandidates(c);
      })
      .catch((err: unknown) => {
        if (!cancelled) setCandidatesError(err);
      });
    return () => {
      cancelled = true;
    };
  }, [open]);

  // Follows the enrolment until the phone used it, or it was cancelled or expired.
  const enrolmentId = issued?.enrolment_id ?? null;
  useEffect(() => {
    if (!open || !enrolmentId || status !== "pending") return undefined;
    let cancelled = false;
    let inFlight = false;
    const timer = setInterval(() => {
      if (inFlight) return;
      inFlight = true;
      getMobileEnrolmentStatus(enrolmentId)
        .then((s) => {
          if (cancelled || s.status === "pending") return;
          setStatus(s.status);
          if (s.status === "redeemed") onEnrolledRef.current();
        })
        .catch(() => {
          // Not read this round (offline, busy): the next tick asks again.
        })
        .finally(() => {
          inFlight = false;
        });
    }, STATUS_POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [open, enrolmentId, status]);

  const secondsLeft = useSecondsLeft(issued && status === "pending" ? issued.expires_at : null);
  const { svgHtml, error: qrError } = useIdentificationQr(
    issued?.qr_payload ?? "",
    open && issued != null && status === "pending",
  );

  const handleCreate = useCallback(async () => {
    if (!personId || !siteId || busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    try {
      const result = await withStepUp(() =>
        issueMobileEnrolment({ user_id: Number(personId), site_node_id: Number(siteId) }),
      );
      setIssued(result);
      setStatus("pending");
    } catch (err) {
      // Refusals are toasted once by the global handler; closing the password prompt is no failure.
      if (isStepUpCancelled(err)) return;
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }, [personId, siteId, withStepUp]);

  const handleCancelQr = useCallback(async () => {
    if (!issued || busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    try {
      const result = await cancelMobileEnrolment(issued.enrolment_id);
      setStatus(result.status === "pending" ? "cancelled" : result.status);
      if (result.status === "redeemed") onEnrolledRef.current();
    } catch {
      // Toasted by the global handler; the QR code stays as it was.
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }, [issued]);

  const restart = useCallback(() => {
    setIssued(null);
    setStatus("pending");
  }, []);

  const noChoice =
    candidates != null && (candidates.people.length === 0 || candidates.sites.length === 0);

  return (
    <>
      <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
        <DialogContent className="max-w-lg">
          <DialogHeader>
            <DialogTitle>{t("phones.enrol.title")}</DialogTitle>
            <DialogDescription>{t("phones.enrol.help")}</DialogDescription>
          </DialogHeader>

          {issued == null ? (
            <div className="space-y-4 py-2">
              {candidatesError != null ? (
                <ErrorState error={candidatesError} title={t("phones.enrol.candidatesError")} />
              ) : (
                <>
                  <div className="space-y-1.5">
                    <Label>{t("phones.enrol.person")}</Label>
                    <Select value={personId} onValueChange={setPersonId}>
                      <SelectTrigger>
                        <SelectValue placeholder={t("phones.enrol.personPlaceholder")} />
                      </SelectTrigger>
                      <SelectContent>
                        {(candidates?.people ?? []).map((p) => (
                          <SelectItem key={p.user_id} value={String(p.user_id)}>
                            {p.label}
                            {p.label !== p.username ? ` (${p.username})` : ""}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    {candidates != null && candidates.people.length === 0 ? (
                      <p className="text-xs text-text-muted">{t("phones.enrol.noPerson")}</p>
                    ) : null}
                  </div>
                  <div className="space-y-1.5">
                    <Label>{t("phones.enrol.site")}</Label>
                    <Select value={siteId} onValueChange={setSiteId}>
                      <SelectTrigger>
                        <SelectValue placeholder={t("phones.enrol.sitePlaceholder")} />
                      </SelectTrigger>
                      <SelectContent>
                        {(candidates?.sites ?? []).map((s) => (
                          <SelectItem key={s.node_id} value={String(s.node_id)}>
                            {s.label} ({s.code})
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    {candidates != null && candidates.sites.length === 0 ? (
                      <p className="text-xs text-text-muted">{t("phones.enrol.noSite")}</p>
                    ) : null}
                  </div>
                </>
              )}
            </div>
          ) : status === "pending" ? (
            <div className="flex flex-col items-center gap-3 py-2">
              <div className="rounded-md bg-white p-2">
                {svgHtml ? (
                  <div
                    role="img"
                    aria-label={t("phones.enrol.qrAlt")}
                    // Generated locally by the qrcode package from the payload: no outside markup.
                    dangerouslySetInnerHTML={{ __html: svgHtml }}
                  />
                ) : (
                  <div className="h-[280px] w-[280px]" />
                )}
              </div>
              {qrError ? <p className="text-xs text-destructive">{qrError}</p> : null}
              <p className="font-mono text-2xl font-semibold tracking-widest text-text-primary">
                {t("phones.enrol.code", { code: issued.code })}
              </p>
              <p className="text-sm tabular-nums text-text-secondary">
                {t("phones.enrol.validFor", { time: minutesSeconds(secondsLeft) })}
              </p>
              <p className="text-sm text-text-secondary">
                {t("phones.enrol.for", { person: issued.person_label, site: issued.site_label })}
              </p>
            </div>
          ) : (
            <div className="py-6 text-center" role="status">
              <p className="text-base font-medium text-text-primary">
                {status === "redeemed"
                  ? t("phones.enrol.enrolled", { person: issued.person_label })
                  : status === "cancelled"
                    ? t("phones.enrol.cancelled")
                    : t("phones.enrol.expired")}
              </p>
            </div>
          )}

          <DialogFooter>
            {issued == null ? (
              <>
                <Button variant="outline" onClick={onClose}>
                  {t("common.cancel")}
                </Button>
                <Button
                  onClick={() => void handleCreate()}
                  disabled={busy || !personId || !siteId || noChoice}
                >
                  {t("phones.enrol.create")}
                </Button>
              </>
            ) : status === "pending" ? (
              <>
                <Button variant="outline" disabled={busy} onClick={() => void handleCancelQr()}>
                  {t("phones.enrol.cancelQr")}
                </Button>
                <Button variant="outline" onClick={onClose}>
                  {t("common.close")}
                </Button>
              </>
            ) : (
              <>
                {status !== "redeemed" ? (
                  <Button onClick={restart}>{t("phones.enrol.newQr")}</Button>
                ) : null}
                <Button variant="outline" onClick={onClose}>
                  {t("common.close")}
                </Button>
              </>
            )}
          </DialogFooter>
        </DialogContent>
      </Dialog>
      {StepUpDialogElement}
    </>
  );
}

// ── Panel ───────────────────────────────────────────────────────────────────────────────────────

export function MobilePhonesPanel() {
  const { t } = useTranslation("admin");
  const { formatDateTime } = useFormatters();
  const { withStepUp, StepUpDialogElement } = useStepUp();

  const [view, setView] = useState<MobilePhonesView | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [enrolOpen, setEnrolOpen] = useState(false);
  const [revokeTarget, setRevokeTarget] = useState<MobilePhone | null>(null);
  const [lost, setLost] = useState(false);
  const [revoking, setRevoking] = useState(false);
  const revokingRef = useRef(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      setView(await listMobilePhones());
    } catch (err) {
      setLoadError(err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const handleRevoke = useCallback(async () => {
    if (!revokeTarget || revokingRef.current) return;
    revokingRef.current = true;
    setRevoking(true);
    try {
      await withStepUp(() => revokeMobilePhone({ device_id: revokeTarget.device_id, lost }));
      pushAppToast({ title: t("phones.revoke.done"), variant: "success" });
      setRevokeTarget(null);
      void refresh();
    } catch (err) {
      // Refusals are toasted once by the global handler; closing the password prompt is no failure.
      if (isStepUpCancelled(err)) return;
    } finally {
      revokingRef.current = false;
      setRevoking(false);
    }
  }, [revokeTarget, lost, withStepUp, refresh, t]);

  const columns: ColumnDef<MobilePhone>[] = useMemo(
    () => [
      {
        id: "person",
        header: t("phones.columns.person"),
        cell: ({ row }) => <span className="font-medium">{row.original.person_label ?? "—"}</span>,
      },
      {
        id: "site",
        header: t("phones.columns.site"),
        cell: ({ row }) => row.original.site_label ?? "—",
      },
      {
        id: "enrolledBy",
        header: t("phones.columns.enrolledBy"),
        cell: ({ row }) => row.original.enrolled_by_label ?? "—",
      },
      {
        id: "enrolledAt",
        header: t("phones.columns.enrolledAt"),
        cell: ({ row }) =>
          row.original.enrolled_at ? formatDateTime(row.original.enrolled_at) : "—",
      },
      {
        id: "lastSync",
        header: t("phones.columns.lastSync"),
        cell: ({ row }) =>
          row.original.last_sync_at ? formatDateTime(row.original.last_sync_at) : t("phones.never"),
      },
      {
        id: "status",
        header: t("phones.columns.status"),
        cell: ({ row }) =>
          row.original.status === "active" ? (
            <Badge variant="default">{t("phones.status.active")}</Badge>
          ) : (
            <Badge variant="secondary">{t("phones.status.wipePending")}</Badge>
          ),
      },
      {
        id: "actions",
        header: "",
        cell: ({ row }) =>
          row.original.status === "active" ? (
            <Button
              variant="ghost"
              size="sm"
              className="text-destructive hover:text-destructive"
              onClick={() => {
                setLost(false);
                setRevokeTarget(row.original);
              }}
            >
              {t("phones.revoke.action")}
            </Button>
          ) : null,
      },
    ],
    [t, formatDateTime],
  );

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="space-y-1">
          <h2 className="text-lg font-semibold text-text-primary">{t("phones.title")}</h2>
          {view != null ? (
            <p className="text-sm text-text-secondary">
              {view.max_mobile_devices === 0
                ? t("phones.noLicence")
                : t("phones.slots", {
                    active: view.active_mobile_devices,
                    max: view.max_mobile_devices,
                  })}
            </p>
          ) : null}
        </div>
        <Button
          size="sm"
          disabled={loading || loadError != null}
          onClick={() => setEnrolOpen(true)}
        >
          <Smartphone className="mr-1.5 h-4 w-4" aria-hidden />
          {t("phones.enrol.action")}
        </Button>
      </div>

      {view != null && view.pending_revocations.length > 0 ? (
        <div
          role="status"
          className="rounded-md border border-tone-attention-border bg-tone-attention-bg px-4 py-3 text-sm text-text-primary"
        >
          {t("phones.pendingRevocations", { names: view.pending_revocations.join(", ") })}
        </div>
      ) : null}

      {loadError != null ? (
        <ErrorState
          error={loadError}
          title={t("phones.loadError")}
          onRetry={() => void refresh()}
          retrying={loading}
        />
      ) : (
        <DataTable<MobilePhone, unknown>
          columns={columns}
          data={view?.devices ?? []}
          isLoading={loading && view == null}
          skeletonRows={4}
          pageSize={20}
          emptyMessage={t("phones.empty")}
        />
      )}

      <EnrolDialog
        open={enrolOpen}
        onClose={() => setEnrolOpen(false)}
        onEnrolled={() => void refresh()}
      />

      <Dialog open={revokeTarget != null} onOpenChange={(v) => !v && setRevokeTarget(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("phones.revoke.title")}</DialogTitle>
            <DialogDescription>
              {t("phones.revoke.confirm", { person: revokeTarget?.person_label ?? "—" })}
            </DialogDescription>
          </DialogHeader>
          <div className="flex items-start gap-2">
            <Checkbox
              id="phones-revoke-lost"
              checked={lost}
              onCheckedChange={setLost}
              className="mt-0.5"
            />
            <Label htmlFor="phones-revoke-lost" className="text-sm font-normal">
              {t("phones.revoke.lost")}
            </Label>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setRevokeTarget(null)}>
              {t("common.cancel")}
            </Button>
            <Button variant="destructive" disabled={revoking} onClick={() => void handleRevoke()}>
              {t("phones.revoke.action")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {StepUpDialogElement}
    </div>
  );
}
