/**
 * The people working under a permit (L4-05): personnel or external workers with their company and
 * role, the safety induction of external staff, what each lacks for the permit (L4-06, the
 * server's verdict), and the personal padlocks on the group lock box (L4-18): a worker places and
 * removes only their own; an absent worker's padlock comes off by the isolation locks' forced-removal
 * process (a `ptw.close` holder authorises a named person, with the reason, the attestation and the
 * contact attempts), and the worker is told (their acknowledgement, or a `ptw.close` holder records
 * it) before placing a padlock again. Every rule is judged by the server (`permit::workers`); this
 * panel shows its verdict.
 */

import { type FormEvent, useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  Button,
  FormField,
  Input,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui";
import { Checkbox } from "@/components/ui/checkbox";
import { ErrorState } from "@/components/ui/data-state";
import { StatusChip } from "@/components/ui/status-chip";
import { useFormatters } from "@/hooks/use-formatters";
import { isStepUpCancelled } from "@/hooks/use-step-up";
import { appErrorReason } from "@/lib/app-error";
import { type WorkPermitRecord, listPermitContactMethods } from "@/services/permit-service";
import {
  type PermitPersonalLock,
  type PermitWorker,
  type PermitWorkersView,
  acknowledgeForcedPadlockRemoval,
  addPermitWorker,
  forceRemovePermitLock,
  listPermitPadlockRemovers,
  listPermitWorkers,
  placeMyPermitLock,
  recordPadlockWorkerInformed,
  recordPermitWorkerInduction,
  removeMyPermitLock,
  withdrawPermitWorker,
} from "@/services/permit-workers-service";
import { listExternalCompanies, listPersonnel } from "@/services/personnel-service";
import type { ExternalCompany, PermitHandoverRoleOption, Personnel } from "@shared/ipc-types";

import { ForcedLockRemovalDialog } from "./ForcedRemovalDialog";
import { useIssueBlockerText } from "./PermitSafetyContentPanel";

/** `permit::workers::WORKER_EDIT_STATUSES`. */
const WORKER_EDIT_STATUSES: ReadonlySet<string> = new Set([
  "draft",
  "pending_review",
  "approved",
  "issued",
  "active",
  "suspended",
  "revalidation_required",
]);
/** `permit::workers::LOCK_PLACE_STATUSES`. */
const LOCK_PLACE_STATUSES: ReadonlySet<string> = new Set(["issued", "active"]);
/** `permit::workers::LOCK_REMOVE_STATUSES`. */
const LOCK_REMOVE_STATUSES: ReadonlySet<string> = new Set([
  "issued",
  "active",
  "suspended",
  "revalidation_required",
  "cancelled",
  "expired",
  "closed",
  "handed_back",
]);

interface PermitWorkersPanelProps {
  permit: WorkPermitRecord;
  /** `ptw.issue` at the permit's site: add, induct and withdraw workers. */
  canIssue: boolean;
  /** `ptw.close` at the permit's site: forced removal of an absent worker's padlock. */
  canClose: boolean;
  /** `ptw.lock_own` at the permit's site: place and remove one's own padlock. */
  canLockOwn: boolean;
  /** After a change: what blocks the issue may have changed. */
  onChanged: () => void;
}

function workerName(w: PermitWorker): string {
  return w.personnel_name ?? w.external_name ?? String(w.id);
}

function nowIso(): string {
  return new Date().toISOString();
}

export function PermitWorkersPanel({
  permit,
  canIssue,
  canClose,
  canLockOwn,
  onChanged,
}: PermitWorkersPanelProps) {
  const { t } = useTranslation("permits");
  const blockerText = useIssueBlockerText();
  const { formatDateTime } = useFormatters();
  const [view, setView] = useState<PermitWorkersView | null>(null);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [lockNumber, setLockNumber] = useState("");
  const [forcing, setForcing] = useState<PermitPersonalLock | null>(null);
  const [methods, setMethods] = useState<PermitHandoverRoleOption[] | null>(null);
  const [informMethod, setInformMethod] = useState("");

  const load = useCallback(async () => {
    try {
      setView(await listPermitWorkers(permit.id));
      setLoadError(null);
    } catch (e) {
      setLoadError(e);
    }
  }, [permit.id]);

  useEffect(() => {
    void load();
  }, [load, permit.status, permit.row_version]);

  // The plant's contact methods, to record that a worker was told of a forced removal.
  const untold = view?.locks.some((l) => l.forced_removal && l.informed_at == null) ?? false;
  useEffect(() => {
    if (!canClose || !untold || methods != null) return;
    let cancelled = false;
    listPermitContactMethods()
      .then((m) => {
        if (!cancelled) setMethods(m);
      })
      .catch(() => {
        if (!cancelled) setMethods([]);
      });
    return () => {
      cancelled = true;
    };
  }, [canClose, untold, methods]);

  const run = async (fn: () => Promise<PermitWorkersView>): Promise<boolean> => {
    setBusy(true);
    setError(null);
    try {
      setView(await fn());
      onChanged();
      return true;
    } catch (e) {
      if (!isStepUpCancelled(e)) setError(appErrorReason(e));
      return false;
    } finally {
      setBusy(false);
    }
  };

  if (loadError != null) {
    return (
      <ErrorState
        error={loadError}
        title={t("workers.loadError")}
        onRetry={() => void load()}
        retrying={false}
      />
    );
  }
  if (view == null) {
    return (
      <p className="text-sm text-text-muted" aria-busy="true">
        {t("workers.loading")}
      </p>
    );
  }

  const status = permit.status;
  const editable = canIssue && WORKER_EDIT_STATUSES.has(status);
  const listed = view.workers.filter((w) => w.withdrawn_at == null);
  const me = listed.find((w) => w.is_me) ?? null;
  const removed = view.locks.filter((l) => l.removed_at != null);
  const workerOf = (id: number) => view.workers.find((w) => w.id === id);

  const forcingWorker = forcing ? workerOf(forcing.worker_id) : undefined;
  const untoldLocks = view.locks.filter((l) => l.forced_removal && l.informed_at == null);

  return (
    <section className="space-y-2" data-testid="permit-workers">
      <h3 className="text-sm font-medium text-text-primary">{t("workers.title")}</h3>
      <p className="text-xs text-text-muted">{t("workers.hint")}</p>
      {view.open_lock_count > 0 ? (
        <StatusChip
          label={t("workers.openLocks", { count: view.open_lock_count })}
          tone="attention"
        />
      ) : null}

      {error ? (
        <p
          role="alert"
          className="rounded-md border border-tone-danger-border bg-tone-danger-bg px-3 py-2 text-sm text-tone-danger-fg"
        >
          {error}
        </p>
      ) : null}

      {listed.length === 0 ? (
        <p className="text-xs text-text-muted">{t("workers.empty")}</p>
      ) : (
        <ul className="space-y-2">
          {listed.map((w) => (
            <li
              key={w.id}
              data-testid="permit-worker"
              className="space-y-1 rounded-md border border-surface-border p-2 text-sm"
            >
              <div className="flex flex-wrap items-center gap-2">
                <span className="font-medium">{workerName(w)}</span>
                <StatusChip
                  label={w.is_external ? t("workers.external") : t("workers.internal")}
                  tone={w.is_external ? "info" : "neutral"}
                />
                {w.is_me ? <StatusChip label={t("workers.me")} tone="progress" /> : null}
                <span className="text-xs text-text-muted">
                  {t("workers.role", { role: w.role_on_permit })}
                </span>
                {w.company_name ? (
                  <span className="text-xs text-text-muted">
                    {t("workers.company", { company: w.company_name })}
                  </span>
                ) : null}
              </div>
              {w.is_external ? (
                <p className="text-xs text-text-muted">
                  {w.induction_at
                    ? t("workers.inductionDone", {
                        date: formatDateTime(w.induction_at),
                        name: w.induction_by_name ?? t("detail.unknownPerson"),
                      })
                    : t("workers.inductionMissing")}
                </p>
              ) : null}
              {w.blockers.length > 0 ? (
                <ul
                  className="list-disc pl-4 text-xs text-tone-warning-fg"
                  data-testid="permit-worker-blockers"
                >
                  {w.blockers.map((b) => (
                    <li key={`${b.code}-${JSON.stringify(b.params)}`}>{blockerText(b)}</li>
                  ))}
                </ul>
              ) : null}
              <p className="text-xs">
                {w.open_lock
                  ? t("workers.lockOn", {
                      lock: w.open_lock.lock_number,
                      date: formatDateTime(w.open_lock.placed_at),
                    })
                  : t("workers.noLock")}
              </p>
              <div className="flex flex-wrap gap-2">
                {editable && w.is_external && w.induction_at == null ? (
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={busy}
                    onClick={() =>
                      void run(() =>
                        recordPermitWorkerInduction({
                          worker_id: w.id,
                          expected_row_version: w.row_version,
                          induction_at: nowIso(),
                        }),
                      )
                    }
                  >
                    {t("workers.recordInduction")}
                  </Button>
                ) : null}
                {w.is_me && w.open_lock && canLockOwn && LOCK_REMOVE_STATUSES.has(status) ? (
                  <Button
                    size="sm"
                    disabled={busy}
                    onClick={() => {
                      const lock = w.open_lock;
                      if (lock == null) return;
                      void run(() =>
                        removeMyPermitLock({
                          lock_id: lock.id,
                          expected_row_version: lock.row_version,
                        }),
                      );
                    }}
                  >
                    {t("workers.myLock.remove")}
                  </Button>
                ) : null}
                {!w.is_me && w.open_lock && canClose && LOCK_REMOVE_STATUSES.has(status) ? (
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={busy}
                    onClick={() => setForcing(w.open_lock)}
                  >
                    {t("workers.force.open")}
                  </Button>
                ) : null}
                {editable && w.open_lock == null ? (
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={busy}
                    onClick={() =>
                      void run(() =>
                        withdrawPermitWorker({
                          worker_id: w.id,
                          expected_row_version: w.row_version,
                        }),
                      )
                    }
                  >
                    {t("workers.withdraw")}
                  </Button>
                ) : null}
              </div>
            </li>
          ))}
        </ul>
      )}

      {me != null && me.open_lock == null && canLockOwn && LOCK_PLACE_STATUSES.has(status) ? (
        <form
          className="flex flex-wrap items-end gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            void run(() =>
              placeMyPermitLock({ permit_id: permit.id, lock_number: lockNumber.trim() }),
            ).then((ok) => {
              if (ok) setLockNumber("");
            });
          }}
        >
          <FormField name="my-lock-number" label={t("workers.myLock.number")} required>
            <Input
              id="my-lock-number"
              value={lockNumber}
              onChange={(e) => setLockNumber(e.target.value)}
            />
          </FormField>
          <Button type="submit" size="sm" disabled={busy || lockNumber.trim() === ""}>
            {t("workers.myLock.place")}
          </Button>
        </form>
      ) : null}

      <ForcedLockRemovalDialog
        target={
          forcing == null
            ? null
            : {
                key: `padlock-${forcing.id}`,
                point: t("workers.force.point", { lock: forcing.lock_number }),
                worker: forcingWorker ? workerName(forcingWorker) : t("detail.unknownPerson"),
              }
        }
        onOpenChange={(open) => {
          if (!open) setForcing(null);
        }}
        listRemovers={(search) =>
          forcing == null
            ? Promise.reject(new Error("no padlock"))
            : listPermitPadlockRemovers(forcing.id, search)
        }
        submit={async (payload) => {
          if (forcing == null) return;
          const saved = await forceRemovePermitLock({
            lock_id: forcing.id,
            expected_row_version: forcing.row_version,
            ...payload,
          });
          setView(saved);
          onChanged();
        }}
      />

      {untoldLocks.length > 0 ? (
        <ul className="space-y-2" data-testid="permit-lock-untold">
          {untoldLocks.map((l) => {
            const w = workerOf(l.worker_id);
            const mine = w?.is_me ?? false;
            return (
              <li
                key={l.id}
                className="space-y-1 rounded-md border border-tone-warning-border p-2 text-xs"
              >
                <p>
                  {t("workers.told.pending", {
                    lock: l.lock_number,
                    name: w ? workerName(w) : t("detail.unknownPerson"),
                    by: l.forced_authorized_by_name ?? t("detail.unknownPerson"),
                  })}
                </p>
                {mine ? (
                  <Button
                    size="sm"
                    disabled={busy}
                    onClick={() => void run(() => acknowledgeForcedPadlockRemoval(l.id))}
                  >
                    {t("workers.told.acknowledge")}
                  </Button>
                ) : canClose ? (
                  <div className="flex flex-wrap items-end gap-2">
                    <Select value={informMethod} onValueChange={setInformMethod}>
                      <SelectTrigger
                        className="w-56"
                        aria-label={t("workers.told.method")}
                        disabled={methods == null}
                      >
                        <SelectValue placeholder={t("workers.told.method")} />
                      </SelectTrigger>
                      <SelectContent>
                        {(methods ?? []).map((m) => (
                          <SelectItem key={m.code} value={m.code}>
                            {m.label}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={busy || informMethod === ""}
                      onClick={() =>
                        void run(() =>
                          recordPadlockWorkerInformed({
                            lock_id: l.id,
                            expected_row_version: l.row_version,
                            method: informMethod,
                            informed_at: null,
                          }),
                        ).then((ok) => {
                          if (ok) setInformMethod("");
                        })
                      }
                    >
                      {t("workers.told.record")}
                    </Button>
                  </div>
                ) : null}
              </li>
            );
          })}
        </ul>
      ) : null}

      {editable ? <AddWorkerForm permit={permit} busy={busy} run={run} /> : null}

      {removed.length > 0 ? (
        <details className="text-xs">
          <summary>{t("workers.history")}</summary>
          <ul className="space-y-1 pl-4">
            {removed.map((l) => {
              const w = workerOf(l.worker_id);
              return (
                <li key={l.id}>
                  {t("workers.removedAt", {
                    lock: l.lock_number,
                    name: w ? workerName(w) : "",
                    date: formatDateTime(l.removed_at ?? ""),
                    by: l.removed_by_name ?? t("detail.unknownPerson"),
                  })}
                  {l.forced_removal
                    ? ` — ${t("workers.force.forced", { reason: l.forced_reason ?? "" })}`
                    : ""}
                </li>
              );
            })}
          </ul>
        </details>
      ) : null}
    </section>
  );
}

interface AddWorkerFormProps {
  permit: WorkPermitRecord;
  busy: boolean;
  run: (fn: () => Promise<PermitWorkersView>) => Promise<boolean>;
}

/** A person from the personnel list, or an external worker by name and company of the list. */
function AddWorkerForm({ permit, busy, run }: AddWorkerFormProps) {
  const { t } = useTranslation("permits");
  const [kind, setKind] = useState<"personnel" | "external">("personnel");
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<Personnel[] | null>(null);
  const [personId, setPersonId] = useState("");
  const [name, setName] = useState("");
  const [companies, setCompanies] = useState<ExternalCompany[] | null>(null);
  const [companiesError, setCompaniesError] = useState(false);
  const [companyId, setCompanyId] = useState("");
  const [role, setRole] = useState("");
  const [inductionNow, setInductionNow] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);

  useEffect(() => {
    if (kind !== "external" || companies != null) return;
    listExternalCompanies({})
      .then((rows) => setCompanies(rows.filter((c) => c.is_active)))
      .catch(() => setCompaniesError(true));
  }, [kind, companies]);

  const search = () => {
    setSearchError(null);
    listPersonnel({ search: query.trim(), limit: 15, employment_status: ["active"] })
      .then((page) => setResults(page.items))
      .catch((e: unknown) => {
        setResults([]);
        setSearchError(appErrorReason(e));
      });
  };

  const chosenPerson = results?.find((p) => String(p.id) === personId) ?? null;
  const external = kind === "external" || chosenPerson?.employment_origin === "external";

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    const ok = await run(() =>
      addPermitWorker({
        permit_id: permit.id,
        personnel_id: kind === "personnel" && personId !== "" ? Number(personId) : null,
        external_name: kind === "external" ? name.trim() : null,
        external_company_id: kind === "external" && companyId !== "" ? Number(companyId) : null,
        role_on_permit: role.trim(),
        induction_at: external && inductionNow ? nowIso() : null,
      }),
    );
    if (ok) {
      setPersonId("");
      setName("");
      setRole("");
      setInductionNow(false);
    }
  };

  return (
    <form
      className="space-y-2 rounded-md border border-surface-border p-2"
      onSubmit={(e) => void handleSubmit(e)}
      data-testid="permit-worker-add"
    >
      <p className="text-sm font-medium">{t("workers.add.title")}</p>
      <div className="flex flex-wrap gap-3 text-sm">
        <label className="flex items-center gap-1">
          <input
            type="radio"
            name="worker-kind"
            checked={kind === "personnel"}
            onChange={() => setKind("personnel")}
          />
          {t("workers.add.kindPersonnel")}
        </label>
        <label className="flex items-center gap-1">
          <input
            type="radio"
            name="worker-kind"
            checked={kind === "external"}
            onChange={() => setKind("external")}
          />
          {t("workers.add.kindExternal")}
        </label>
      </div>
      {kind === "personnel" ? (
        <div className="space-y-2">
          <div className="flex flex-wrap items-end gap-2">
            <FormField name="worker-search" label={t("workers.add.search")}>
              <Input id="worker-search" value={query} onChange={(e) => setQuery(e.target.value)} />
            </FormField>
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={query.trim() === ""}
              onClick={search}
            >
              {t("workers.add.searchButton")}
            </Button>
          </div>
          {searchError ? <p className="text-xs text-tone-danger-fg">{searchError}</p> : null}
          {results != null && results.length === 0 && searchError == null ? (
            <p className="text-xs text-text-muted">{t("workers.add.noResult")}</p>
          ) : null}
          {results != null && results.length > 0 ? (
            <FormField name="worker-person" label={t("workers.add.person")} required>
              <select
                id="worker-person"
                className="h-9 w-full rounded-md border border-surface-border bg-surface-1 px-2 text-sm"
                value={personId}
                onChange={(e) => setPersonId(e.target.value)}
              >
                <option value="">{t("workers.add.choose")}</option>
                {results.map((p) => (
                  <option key={p.id} value={String(p.id)}>
                    {p.full_name} ({p.employee_code})
                  </option>
                ))}
              </select>
            </FormField>
          ) : null}
        </div>
      ) : (
        <div className="grid gap-2 sm:grid-cols-2">
          <FormField name="worker-name" label={t("workers.add.name")} required>
            <Input id="worker-name" value={name} onChange={(e) => setName(e.target.value)} />
          </FormField>
          <FormField name="worker-company" label={t("workers.add.company")} required>
            {companiesError ? (
              <p className="text-xs text-tone-danger-fg">{t("workers.add.companiesError")}</p>
            ) : (
              <select
                id="worker-company"
                className="h-9 w-full rounded-md border border-surface-border bg-surface-1 px-2 text-sm"
                value={companyId}
                onChange={(e) => setCompanyId(e.target.value)}
              >
                <option value="">{t("workers.add.choose")}</option>
                {(companies ?? []).map((c) => (
                  <option key={c.id} value={String(c.id)}>
                    {c.name}
                  </option>
                ))}
              </select>
            )}
          </FormField>
        </div>
      )}
      <FormField name="worker-role" label={t("workers.add.role")} required>
        <Input id="worker-role" value={role} onChange={(e) => setRole(e.target.value)} />
      </FormField>
      {external ? (
        <label className="flex items-start gap-2 text-sm">
          <Checkbox checked={inductionNow} onCheckedChange={setInductionNow} />
          <span>{t("workers.add.inductionNow")}</span>
        </label>
      ) : null}
      <Button type="submit" size="sm" disabled={busy || role.trim() === ""}>
        {t("workers.add.submit")}
      </Button>
    </form>
  );
}
