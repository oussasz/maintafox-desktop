/**
 * Records the hand-over of a closed permit: roles from the plant's list (Données de référence ›
 * PERMIT.HANDOVER_ROLES, nothing preselected, no free text), an optional receiving person picked
 * by name from a paged, searchable list ("show more" asks the next page of the same search) (never the signer — the server signs with the session
 * user, shown here read-only), and a confirmation note of at least HANDOVER_NOTE_MIN characters.
 * Refusals are shown with the server's coded, translated reason.
 */

import { type FormEvent, useEffect, useRef, useState } from "react";
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
  Textarea,
} from "@/components/ui";
import { ErrorState } from "@/components/ui/data-state";
import { useSession } from "@/hooks/use-session";
import { useDebouncedValue } from "@/hooks/useDebouncedValue";
import { appErrorReason } from "@/lib/app-error";
import {
  appendPermitHandoverLog,
  listPermitHandoverReceivers,
  listPermitHandoverRoles,
} from "@/services/permit-service";
import type {
  PermitHandoverReceiver,
  PermitHandoverReceiverPage,
  PermitHandoverRoleOption,
} from "@shared/ipc-types";

/** Shortest confirmation note the server accepts (`MIN_HANDOVER_NOTE_CHARS`). */
export const HANDOVER_NOTE_MIN = 10;
/** Value of the "nobody named" receiver choice. */
const NO_RECEIVER = "__none__";

interface PermitHandoverFormProps {
  permitId: number;
  /** Called after a hand-over was recorded (the panel reloads its lists). */
  onRecorded: () => void;
}

export function PermitHandoverForm({ permitId, onRecorded }: PermitHandoverFormProps) {
  const { t } = useTranslation("permits");
  const { info: session } = useSession();
  const signerName = session?.display_name?.trim() || session?.username || null;

  const [roles, setRoles] = useState<PermitHandoverRoleOption[] | null>(null);
  const [rolesError, setRolesError] = useState<unknown>(null);
  const [rolesReload, setRolesReload] = useState(0);
  const [receivers, setReceivers] = useState<PermitHandoverReceiverPage | null>(null);
  const [receiversError, setReceiversError] = useState<unknown>(null);
  const [receiversReload, setReceiversReload] = useState(0);
  const [receiverSearch, setReceiverSearch] = useState("");
  const search = useDebouncedValue(receiverSearch.trim());
  const searchRef = useRef(search);
  searchRef.current = search;
  const [loadingMore, setLoadingMore] = useState(false);
  /** The chosen receiver, kept even when a later search no longer lists them. */
  const [receiver, setReceiver] = useState<PermitHandoverReceiver | null>(null);
  const [draft, setDraft] = useState({ from: "", to: "", note: "" });
  const [noteTouched, setNoteTouched] = useState(false);
  const [submitted, setSubmitted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setRolesError(null);
    listPermitHandoverRoles()
      .then((r) => {
        if (!cancelled) setRoles(r);
      })
      .catch((e: unknown) => {
        if (!cancelled) setRolesError(e);
      });
    return () => {
      cancelled = true;
    };
  }, [rolesReload]);

  useEffect(() => {
    let cancelled = false;
    setReceiversError(null);
    listPermitHandoverReceivers(search || null)
      .then((page) => {
        if (!cancelled) setReceivers(page);
      })
      .catch((e: unknown) => {
        if (!cancelled) setReceiversError(e);
      });
    return () => {
      cancelled = true;
    };
  }, [search, receiversReload]);

  /** Appends the next page of the current search (the list's own offset), never a stale one. */
  const showMore = async () => {
    if (!receivers) return;
    const asked = search;
    setLoadingMore(true);
    try {
      const page = await listPermitHandoverReceivers(
        asked || null,
        undefined,
        receivers.items.length,
      );
      if (searchRef.current !== asked) return;
      setReceivers((prev) =>
        prev
          ? {
              total: page.total,
              items: [
                ...prev.items,
                ...page.items.filter((p) => !prev.items.some((r) => r.id === p.id)),
              ],
            }
          : page,
      );
      setReceiversError(null);
    } catch (e) {
      setReceiversError(e);
    } finally {
      setLoadingMore(false);
    }
  };

  const note = draft.note.trim();
  const noteTooShort = note.length < HANDOVER_NOTE_MIN;
  const noRoles = roles != null && roles.length === 0;

  // The listed people, plus the chosen one when the current search does not list them.
  const listed = receivers?.items ?? [];
  const options =
    receiver && !listed.some((r) => r.id === receiver.id) ? [receiver, ...listed] : listed;

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    setSaveError(null);
    if (!draft.from || !draft.to || noteTooShort) return;
    setBusy(true);
    try {
      await appendPermitHandoverLog({
        permit_id: permitId,
        handed_from_role: draft.from,
        handed_to_role: draft.to,
        confirmation_note: note,
        received_by_id: receiver?.id ?? null,
      });
      setDraft({ from: "", to: "", note: "" });
      setReceiver(null);
      setReceiverSearch("");
      setNoteTouched(false);
      setSubmitted(false);
      onRecorded();
    } catch (err) {
      setSaveError(appErrorReason(err));
    } finally {
      setBusy(false);
    }
  };

  const roleSelect = (field: "from" | "to", id: string, label: string) => (
    <FormField
      name={id}
      label={label}
      required
      error={submitted && !draft[field] ? t("errors.required") : undefined}
    >
      <Select
        value={draft[field]}
        onValueChange={(v) => setDraft((d) => ({ ...d, [field]: v }))}
        disabled={roles == null || noRoles}
      >
        <SelectTrigger id={id} aria-label={label}>
          <SelectValue placeholder={t("handover.rolePlaceholder")} />
        </SelectTrigger>
        <SelectContent>
          {(roles ?? []).map((r) => (
            <SelectItem key={r.code} value={r.code}>
              {r.review_required ? t("handover.roleToReview", { label: r.label }) : r.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </FormField>
  );

  return (
    <form
      className="space-y-2 rounded-md border border-surface-border p-3"
      aria-labelledby="handover-form-title"
      onSubmit={(e) => void handleSubmit(e)}
    >
      <h4 id="handover-form-title" className="text-sm font-medium text-text-primary">
        {t("handover.add")}
      </h4>
      <p className="text-xs text-text-muted">{t("handover.rolesHint")}</p>
      <p className="text-xs text-text-primary" data-testid="handover-signer">
        {signerName ? t("handover.signerYou", { name: signerName }) : t("handover.signerSession")}
      </p>
      {rolesError != null ? (
        <ErrorState error={rolesError} onRetry={() => setRolesReload((n) => n + 1)} />
      ) : noRoles ? (
        <p role="status" className="text-xs text-tone-warning-fg">
          {t("handover.noRoles")}
        </p>
      ) : null}
      <div className="grid gap-2 sm:grid-cols-2">
        {roleSelect("from", "handover-from", t("handover.from"))}
        {roleSelect("to", "handover-to", t("handover.to"))}
      </div>
      <FormField name="handover-receiver" label={t("handover.receiver")}>
        <div className="space-y-1">
          <Input
            id="handover-receiver-search"
            aria-label={t("handover.receiverSearch")}
            placeholder={t("handover.receiverSearch")}
            value={receiverSearch}
            onChange={(e) => setReceiverSearch(e.target.value)}
          />
          <Select
            value={receiver ? String(receiver.id) : NO_RECEIVER}
            onValueChange={(v) =>
              setReceiver(
                v === NO_RECEIVER ? null : (options.find((r) => String(r.id) === v) ?? null),
              )
            }
            disabled={receivers == null && receiver == null}
          >
            <SelectTrigger id="handover-receiver" aria-label={t("handover.receiver")}>
              <SelectValue placeholder={t("handover.receiverNone")} />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={NO_RECEIVER}>{t("handover.receiverNone")}</SelectItem>
              {options.map((r) => (
                <SelectItem key={r.id} value={String(r.id)}>
                  {r.display_name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          {receivers != null ? (
            <p className="text-xs text-text-muted" data-testid="handover-receivers-count">
              {receivers.total > receivers.items.length
                ? t("handover.receiversPartial", {
                    shown: receivers.items.length,
                    total: receivers.total,
                  })
                : receivers.total === 0
                  ? t("handover.receiversNone")
                  : t("handover.receiversAll", { total: receivers.total })}
            </p>
          ) : null}
          {receivers != null && receivers.total > receivers.items.length ? (
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={loadingMore}
              onClick={() => void showMore()}
            >
              {t("handover.receiversMore")}
            </Button>
          ) : null}
          {receiversError != null ? (
            <ErrorState error={receiversError} onRetry={() => setReceiversReload((n) => n + 1)} />
          ) : null}
          <p className="text-xs text-text-muted">{t("handover.receiverHint")}</p>
        </div>
      </FormField>
      <FormField
        name="handover-note"
        label={t("handover.note")}
        required
        description={t("handover.noteCount", { length: note.length, min: HANDOVER_NOTE_MIN })}
        error={
          (submitted || noteTouched) && noteTooShort
            ? t("handover.noteMin", { min: HANDOVER_NOTE_MIN })
            : undefined
        }
      >
        <Textarea
          id="handover-note"
          rows={2}
          value={draft.note}
          onChange={(e) => {
            setNoteTouched(true);
            setDraft((d) => ({ ...d, note: e.target.value }));
          }}
        />
      </FormField>
      {saveError ? (
        <p role="alert" className="text-xs text-tone-danger-fg">
          {saveError}
        </p>
      ) : null}
      <Button type="submit" className="h-10" disabled={busy || roles == null || noRoles}>
        {t("handover.submit")}
      </Button>
    </form>
  );
}
