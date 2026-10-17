/**
 * ResponsibilityAssignDialog.tsx
 *
 * Name who is responsible for a unit (maintenance owner, planner, approver, …): a person or a
 * team. When the type already has an active assignee, the change is a handover: the impact
 * preview opens and its confirm ends the previous assignment and creates the new one in one
 * transaction. Otherwise the assignment is created directly.
 */

import { Check, Search } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { lookupLabel } from "@/components/org/org-display";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import { assignOrgNodeResponsibility } from "@/services/org-node-service";
import { listActiveTeams, listPersonnel } from "@/services/personnel-service";
import { useOrgDesignerStore } from "@/stores/org-designer-store";
import type {
  LookupValueOption,
  OrgNodeResponsibility,
  Personnel,
  TeamSummary,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/** Rows shown per person search (the backend pages the personnel list). */
const PERSON_SEARCH_LIMIT = 20;
/** Wait after the last keystroke before searching personnel. */
const PERSON_SEARCH_DEBOUNCE_MS = 250;

type AssigneeKind = "person" | "team";

interface ResponsibilityAssignDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Unit selected in the designer (draft copy or published unit) — used for the preview. */
  nodeId: number;
  nodeName: string;
  /** Published unit that carries the operational records. */
  operationalNodeId: number;
  types: LookupValueOption[];
  /** Active assignments on the unit (to tell an assignment from a handover). */
  activeAssignments: OrgNodeResponsibility[];
  /** Pre-selected responsibility type (e.g. "Replace" on an existing row). */
  initialType?: string | null;
  /** Called after a direct assignment was saved. */
  onAssigned: () => void;
}

export function ResponsibilityAssignDialog({
  open,
  onOpenChange,
  nodeId,
  nodeName,
  operationalNodeId,
  types,
  activeAssignments,
  initialType,
  onAssigned,
}: ResponsibilityAssignDialogProps) {
  const { t, i18n } = useTranslation("org");
  const { can } = usePermissions();
  const openPreview = useOrgDesignerStore((s) => s.openPreview);
  const canBrowsePeople = can(P.PER_VIEW);

  const [typeCode, setTypeCode] = useState<string>("");
  const [kind, setKind] = useState<AssigneeKind>("person");
  const [personQuery, setPersonQuery] = useState("");
  const [people, setPeople] = useState<Personnel[]>([]);
  const [peopleLoading, setPeopleLoading] = useState(false);
  const [peopleError, setPeopleError] = useState<unknown>(null);
  const [person, setPerson] = useState<Personnel | null>(null);
  const [teams, setTeams] = useState<TeamSummary[]>([]);
  const [teamsError, setTeamsError] = useState<unknown>(null);
  const [teamId, setTeamId] = useState<string>("");
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!open) return;
    const initial = initialType
      ? types.find((v) => v.code.toLowerCase() === initialType.toLowerCase())?.code
      : undefined;
    setTypeCode(initial ?? "");
    setKind("person");
    setPersonQuery("");
    setPeople([]);
    setPerson(null);
    setTeamId("");
    setPeopleError(null);
  }, [open, initialType, types]);

  // Person search (debounced)
  useEffect(() => {
    if (!open || kind !== "person" || !canBrowsePeople) return;
    const query = personQuery.trim();
    let cancelled = false;
    const handle = window.setTimeout(() => {
      setPeopleLoading(true);
      setPeopleError(null);
      listPersonnel({
        search: query || null,
        employment_status: ["active"],
        limit: PERSON_SEARCH_LIMIT,
        offset: 0,
      })
        .then((page) => {
          if (!cancelled) setPeople(page.items);
        })
        .catch((err: unknown) => {
          if (!cancelled) setPeopleError(err);
        })
        .finally(() => {
          if (!cancelled) setPeopleLoading(false);
        });
    }, PERSON_SEARCH_DEBOUNCE_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(handle);
    };
  }, [open, kind, personQuery, canBrowsePeople]);

  // Teams (loaded once per opening)
  useEffect(() => {
    if (!open || kind !== "team" || !canBrowsePeople) return;
    let cancelled = false;
    setTeamsError(null);
    listActiveTeams({})
      .then((rows) => {
        if (!cancelled) setTeams(rows);
      })
      .catch((err: unknown) => {
        if (!cancelled) setTeamsError(err);
      });
    return () => {
      cancelled = true;
    };
  }, [open, kind, canBrowsePeople]);

  const activeForType = useMemo(
    () =>
      typeCode
        ? (activeAssignments.find(
            (a) =>
              a.valid_to == null && a.responsibility_type.toLowerCase() === typeCode.toLowerCase(),
          ) ?? null)
        : null,
    [activeAssignments, typeCode],
  );

  const team = teams.find((tm) => String(tm.id) === teamId) ?? null;
  const assigneeName = kind === "person" ? (person?.full_name ?? null) : (team?.name ?? null);
  const hasAssignee = kind === "person" ? person != null : team != null;
  const isHandover = activeForType != null;
  const typeLabel = (code: string) => {
    const v = types.find((x) => x.code === code);
    return v ? lookupLabel(v, i18n.language ?? "fr") : code;
  };

  const submit = async () => {
    if (!typeCode || !hasAssignee) return;
    const personId = kind === "person" ? (person?.id ?? null) : null;
    const selectedTeamId = kind === "team" ? (team?.id ?? null) : null;
    if (isHandover) {
      // The responsibility type of the existing row is kept (legacy rows may differ in case).
      onOpenChange(false);
      void openPreview(
        {
          action: "ReassignResponsibility",
          node_id: nodeId,
          responsibility_type: activeForType.responsibility_type,
          replacement_person_id: personId,
          replacement_team_id: selectedTeamId,
        },
        { targetLabel: assigneeName },
      );
      return;
    }
    setSaving(true);
    try {
      await assignOrgNodeResponsibility({
        node_id: operationalNodeId,
        responsibility_type: typeCode,
        person_id: personId,
        team_id: selectedTeamId,
        replace_active: false,
      });
      onOpenChange(false);
      onAssigned();
    } catch {
      // Reported by the global IPC toast; keep the dialog open to correct and retry.
    } finally {
      setSaving(false);
    }
  };

  const submitDisabledReason = !typeCode
    ? t("responsibilities.dialog.needType")
    : !hasAssignee
      ? t("responsibilities.dialog.needAssignee")
      : null;

  return (
    <Dialog open={open} onOpenChange={(next) => !saving && onOpenChange(next)}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("responsibilities.dialog.title")}</DialogTitle>
          <DialogDescription>
            {t("responsibilities.dialog.description", { unit: nodeName })}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="org-resp-type">{t("responsibilities.dialog.type")}</Label>
            <Select value={typeCode} onValueChange={setTypeCode}>
              <SelectTrigger id="org-resp-type" className="h-10">
                <SelectValue placeholder={t("responsibilities.dialog.typePlaceholder")} />
              </SelectTrigger>
              <SelectContent>
                {types.map((v) => (
                  <SelectItem key={v.code} value={v.code}>
                    {typeLabel(v.code)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            {activeForType && (
              <p className="text-xs text-text-secondary">
                {t("responsibilities.dialog.currentHolder", {
                  name:
                    activeForType.person_display_name ??
                    activeForType.team_label ??
                    t("responsibilities.unknownAssignee"),
                })}
              </p>
            )}
          </div>

          <div className="space-y-1.5">
            <span className="text-sm font-medium text-text-primary">
              {t("responsibilities.dialog.assignee")}
            </span>
            <div
              role="radiogroup"
              aria-label={t("responsibilities.dialog.assignee")}
              className="flex gap-2"
            >
              {(["person", "team"] as const).map((k) => (
                <Button
                  key={k}
                  type="button"
                  role="radio"
                  aria-checked={kind === k}
                  variant={kind === k ? "default" : "outline"}
                  className="h-10 flex-1"
                  onClick={() => setKind(k)}
                >
                  {t(`responsibilities.dialog.kind.${k}`)}
                </Button>
              ))}
            </div>
          </div>

          {!canBrowsePeople ? (
            <p className="rounded-md border border-surface-border px-3 py-2 text-sm text-text-secondary">
              {t("responsibilities.dialog.needPersonnelView")}
            </p>
          ) : kind === "person" ? (
            <div className="space-y-2">
              <div className="relative">
                <Search
                  className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted"
                  aria-hidden
                />
                <Input
                  value={personQuery}
                  onChange={(e) => setPersonQuery(e.target.value)}
                  placeholder={t("responsibilities.dialog.personSearch")}
                  aria-label={t("responsibilities.dialog.personSearch")}
                  className="h-10 pl-9"
                />
              </div>
              {peopleError != null ? (
                <ErrorState error={peopleError} />
              ) : peopleLoading && people.length === 0 ? (
                <LoadingState />
              ) : people.length === 0 ? (
                <p className="px-1 text-sm text-text-muted">
                  {t("responsibilities.dialog.noPerson")}
                </p>
              ) : (
                <div
                  role="radiogroup"
                  aria-label={t("responsibilities.dialog.kind.person")}
                  className="max-h-56 space-y-1 overflow-y-auto rounded-md border border-surface-border p-1"
                >
                  {people.map((p) => {
                    const selected = person?.id === p.id;
                    return (
                      <button
                        key={p.id}
                        type="button"
                        role="radio"
                        aria-checked={selected}
                        onClick={() => setPerson(p)}
                        className={cn(
                          "flex min-h-11 w-full items-center gap-2 rounded-md px-3 py-2 text-left text-sm",
                          "hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                          selected && "bg-primary/10",
                        )}
                      >
                        <span className="min-w-0 flex-1 truncate">
                          <span className="font-medium text-text-primary">{p.full_name}</span>
                          <span className="ml-2 font-mono text-xs text-text-muted">
                            {p.employee_code}
                          </span>
                        </span>
                        {selected && (
                          <Check className="h-4 w-4 shrink-0 text-primary" aria-hidden />
                        )}
                      </button>
                    );
                  })}
                </div>
              )}
            </div>
          ) : teamsError != null ? (
            <ErrorState error={teamsError} />
          ) : (
            <div className="space-y-1.5">
              <Label htmlFor="org-resp-team">{t("responsibilities.dialog.kind.team")}</Label>
              <Select value={teamId} onValueChange={setTeamId}>
                <SelectTrigger id="org-resp-team" className="h-10">
                  <SelectValue placeholder={t("responsibilities.dialog.teamPlaceholder")} />
                </SelectTrigger>
                <SelectContent>
                  {teams.map((tm) => (
                    <SelectItem key={tm.id} value={String(tm.id)}>
                      {tm.code} — {tm.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}

          {isHandover && hasAssignee && (
            <p className="rounded-md border border-surface-border bg-surface-1 px-3 py-2 text-sm text-text-secondary">
              {t("responsibilities.dialog.handoverHint")}
            </p>
          )}
          {submitDisabledReason && (
            <p className="text-xs text-text-muted">{submitDisabledReason}</p>
          )}
        </div>

        <DialogFooter className="gap-2">
          <Button
            variant="outline"
            className="h-11"
            onClick={() => onOpenChange(false)}
            disabled={saving}
          >
            {t("responsibilities.dialog.cancel")}
          </Button>
          <Button
            className="h-11"
            onClick={() => void submit()}
            disabled={submitDisabledReason != null || saving}
          >
            {isHandover
              ? t("responsibilities.dialog.reviewHandover")
              : saving
                ? t("responsibilities.dialog.saving")
                : t("responsibilities.dialog.assign")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
