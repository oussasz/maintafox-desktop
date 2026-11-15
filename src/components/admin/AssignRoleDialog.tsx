import { AlertTriangle } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  type ScopeChoice,
  ScopeNodeSelect,
  scopeChoiceToInput,
  wholePlantNodeId,
} from "@/components/admin/ScopeNodeSelect";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { DateField } from "@/components/ui/date-field";
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
import { useRoleText, withoutFormerRoles } from "@/lib/role-labels";
import { assignRoleScope } from "@/services/rbac-service";
import type {
  AssignableRoleSummary,
  AssignableScopeNode,
  RoleAssignmentSummary,
  UserWithRoles,
} from "@shared/ipc-types";

/** Scope types that name an org node (the node and everything below it). */
const NODE_SCOPE_TYPES = new Set(["entity", "site", "org_node"]);

/**
 * Whether an assignment gives access to the whole plant and is in force (or starts later): the
 * plant-wide scope, or the top node of the organisation. Emergency access ends on its own.
 */
function coversWholePlant(r: RoleAssignmentSummary, topId: number | null, now: string): boolean {
  if (r.is_emergency) return false;
  if (r.valid_to && r.valid_to < now) return false;
  if (r.scope_type === "tenant") return true;
  return (
    topId !== null && NODE_SCOPE_TYPES.has(r.scope_type) && r.scope_reference === String(topId)
  );
}

interface AssignRoleDialogProps {
  user: UserWithRoles | null;
  open: boolean;
  onClose: () => void;
  roles: AssignableRoleSummary[];
  nodes: AssignableScopeNode[];
  onAssigned: () => void;
  withStepUp: <T>(action: () => Promise<T>) => Promise<T>;
}

/**
 * Give a user a role, plant-wide or limited to an org node (and everything below it), with an
 * optional validity window. One role per user and scope target: assigning at a target that
 * already carries a role replaces it (the screen says which one once role and scope are chosen).
 * A role limited to part of the plant offers to end the user's plant-wide roles (ticked by
 * default, L5-07): kept, they would still open every site.
 */
export function AssignRoleDialog({
  user,
  open,
  onClose,
  roles,
  nodes,
  onAssigned,
  withStepUp,
}: AssignRoleDialogProps) {
  const { t } = useTranslation("admin");
  const roleText = useRoleText();
  // A former shipped role is not given any more: the role with the same rights is (L5-03).
  const offered = useMemo(() => withoutFormerRoles(roles), [roles]);
  const [roleId, setRoleId] = useState("");
  // Nothing chosen at first: the warnings below speak of a real choice only.
  const [scope, setScope] = useState<ScopeChoice>("");
  const [validFrom, setValidFrom] = useState("");
  const [validTo, setValidTo] = useState("");
  const [submitting, setSubmitting] = useState(false);
  // Plant-wide assignments the administrator unticked (kept beside the new role).
  const [kept, setKept] = useState<Set<number>>(() => new Set());

  useEffect(() => {
    if (!open) return;
    setRoleId("");
    setScope("");
    setValidFrom("");
    setValidTo("");
    setKept(new Set());
  }, [open]);

  const target = useMemo(() => (scope === "" ? null : scopeChoiceToInput(scope)), [scope]);
  const chosen = roleId !== "" && target !== null;
  const topId = useMemo(() => wholePlantNodeId(nodes), [nodes]);
  // The roles the backend replaces: at a node, the role naming that node; plant-wide, every
  // plant-wide role of the user, whatever reference it was recorded with (the first
  // administrator's role names the plant, a plant-wide choice here names none).
  const replaced = useMemo(() => {
    if (!user || !chosen || !target) return [];
    return user.roles.filter(
      (r) =>
        r.scope_type === target.scope_type &&
        (target.scope_type === "tenant" ||
          (r.scope_reference ?? "") === (target.scope_reference ?? "")),
    );
  }, [user, chosen, target]);
  // A role limited to part of the plant: the plant-wide roles that would still open every site.
  const plantWide = useMemo(() => {
    if (!user || !chosen || !target) return [];
    const wholePlantRef = topId === null ? null : String(topId);
    if (target.scope_type === "tenant" || target.scope_reference === wholePlantRef) return [];
    const now = new Date().toISOString();
    return user.roles.filter((r) => coversWholePlant(r, topId, now));
  }, [user, chosen, target, topId]);
  const ending = plantWide.filter((r) => !kept.has(r.assignment_id));
  const stillPlantWide = plantWide.filter((r) => kept.has(r.assignment_id));

  const toggleReplace = (assignmentId: number, replace: boolean) => {
    setKept((prev) => {
      const next = new Set(prev);
      if (replace) next.delete(assignmentId);
      else next.add(assignmentId);
      return next;
    });
  };

  const windowInvalid = validFrom !== "" && validTo !== "" && validFrom > validTo;
  const canSubmit = !!user && chosen && !windowInvalid && !submitting;

  const submit = async () => {
    if (!user || !canSubmit || !target) return;
    setSubmitting(true);
    try {
      await withStepUp(() =>
        assignRoleScope({
          user_id: user.id,
          role_id: Number(roleId),
          scope_type: target.scope_type,
          scope_reference: target.scope_reference,
          valid_from: validFrom || null,
          valid_to: validTo || null,
          end_plant_wide_assignment_ids: ending.map((r) => r.assignment_id),
        }),
      );
      onAssigned();
      onClose();
    } catch {
      // The IPC layer reports the error; a cancelled step-up leaves the dialog open.
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>
            {t("console.assign.title", { username: user?.display_name ?? user?.username ?? "" })}
          </DialogTitle>
          <DialogDescription>{t("console.assign.description")}</DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          <div className="space-y-1.5">
            <Label htmlFor="assign-role">{t("console.assign.role")}</Label>
            <Select value={roleId} onValueChange={setRoleId}>
              <SelectTrigger id="assign-role">
                <SelectValue placeholder={t("console.assign.chooseRole")} />
              </SelectTrigger>
              <SelectContent>
                {offered.map((r) => (
                  <SelectItem key={r.id} value={String(r.id)}>
                    {roleText.label(r.name)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="assign-scope">{t("console.scope.label")}</Label>
            <ScopeNodeSelect id="assign-scope" nodes={nodes} value={scope} onChange={setScope} />
            <p className="text-xs text-text-secondary">{t("console.scope.hint")}</p>
          </div>

          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div className="space-y-1.5">
              <Label htmlFor="assign-from">{t("console.assign.validFrom")}</Label>
              <DateField id="assign-from" value={validFrom} onChange={(v) => setValidFrom(v)} />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="assign-to">{t("console.assign.validTo")}</Label>
              <DateField id="assign-to" value={validTo} onChange={(v) => setValidTo(v)} />
            </div>
          </div>
          <p className="text-xs text-text-secondary">{t("console.assign.validityHint")}</p>
          {windowInvalid ? (
            <p className="text-xs text-status-danger" role="alert">
              {t("console.assign.windowInvalid")}
            </p>
          ) : null}

          {plantWide.length > 0 ? (
            <div
              className="space-y-2 rounded-md border border-tone-attention-border bg-tone-attention-bg p-3 text-sm text-tone-attention-fg"
              data-testid="assign-plant-wide"
            >
              <p className="font-medium">
                {t("console.assign.plantWide.title", { count: plantWide.length })}
              </p>
              {plantWide.map((r) => (
                <label key={r.assignment_id} className="flex items-start gap-2">
                  <Checkbox
                    className="mt-0.5"
                    checked={!kept.has(r.assignment_id)}
                    onCheckedChange={(v) => toggleReplace(r.assignment_id, v)}
                  />
                  <span>
                    {t("console.assign.plantWide.replace", { role: roleText.label(r.role_name) })}
                  </span>
                </label>
              ))}
              <p className="text-xs">{t("console.assign.plantWide.hint")}</p>
            </div>
          ) : null}

          {stillPlantWide.length > 0 ? (
            <div
              className="flex items-start gap-2 rounded-md border border-tone-danger-border bg-tone-danger-bg p-3 text-sm text-tone-danger-fg"
              role="alert"
            >
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
              <span>
                {t("console.assign.plantWide.kept", {
                  count: stillPlantWide.length,
                  roles: stillPlantWide.map((r) => roleText.label(r.role_name)).join(", "),
                })}
              </span>
            </div>
          ) : null}

          {replaced.length > 0 ? (
            <div
              className="flex items-start gap-2 rounded-md border border-tone-attention-border bg-tone-attention-bg p-3 text-sm text-tone-attention-fg"
              role="status"
            >
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-accent" aria-hidden />
              <span>
                {replaced.length === 1
                  ? t("console.assign.replaces", {
                      role: replaced[0] ? roleText.label(replaced[0].role_name) : "",
                    })
                  : t("console.assign.replacesMany", {
                      roles: replaced.map((r) => roleText.label(r.role_name)).join(", "),
                    })}
              </span>
            </div>
          ) : null}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={submitting}>
            {t("common.cancel")}
          </Button>
          <Button onClick={() => void submit()} disabled={!canSubmit}>
            {t("console.assign.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
