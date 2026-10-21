import { AlertTriangle } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  PLANT_SCOPE,
  type ScopeChoice,
  ScopeNodeSelect,
  scopeChoiceToInput,
} from "@/components/admin/ScopeNodeSelect";
import { Button } from "@/components/ui/button";
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
import { assignRoleScope } from "@/services/rbac-service";
import type { AssignableRoleSummary, AssignableScopeNode, UserWithRoles } from "@shared/ipc-types";

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
 * already carries a role replaces it (the screen says which one before confirming).
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
  const [roleId, setRoleId] = useState("");
  const [scope, setScope] = useState<ScopeChoice>(PLANT_SCOPE);
  const [validFrom, setValidFrom] = useState("");
  const [validTo, setValidTo] = useState("");
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (!open) return;
    setRoleId("");
    setScope(PLANT_SCOPE);
    setValidFrom("");
    setValidTo("");
  }, [open]);

  const target = scopeChoiceToInput(scope);
  // The roles the backend replaces: at a node, the role naming that node; plant-wide, every
  // plant-wide role of the user, whatever reference it was recorded with (the first
  // administrator's role names the plant, a plant-wide choice here names none).
  const replaced = useMemo(() => {
    if (!user) return [];
    return user.roles.filter(
      (r) =>
        r.scope_type === target.scope_type &&
        (target.scope_type === "tenant" ||
          (r.scope_reference ?? "") === (target.scope_reference ?? "")),
    );
  }, [user, target.scope_type, target.scope_reference]);

  const windowInvalid = validFrom !== "" && validTo !== "" && validFrom > validTo;
  const canSubmit = !!user && roleId !== "" && !windowInvalid && !submitting;

  const submit = async () => {
    if (!user || !canSubmit) return;
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
                {roles.map((r) => (
                  <SelectItem key={r.id} value={String(r.id)}>
                    {r.name}
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
              <Input
                id="assign-from"
                type="date"
                value={validFrom}
                onChange={(e) => setValidFrom(e.target.value)}
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="assign-to">{t("console.assign.validTo")}</Label>
              <Input
                id="assign-to"
                type="date"
                value={validTo}
                onChange={(e) => setValidTo(e.target.value)}
              />
            </div>
          </div>
          <p className="text-xs text-text-secondary">{t("console.assign.validityHint")}</p>
          {windowInvalid ? (
            <p className="text-xs text-status-danger" role="alert">
              {t("console.assign.windowInvalid")}
            </p>
          ) : null}

          {replaced.length > 0 ? (
            <div
              className="flex items-start gap-2 rounded-md border border-tone-attention-border bg-tone-attention-bg p-3 text-sm text-tone-attention-fg"
              role="status"
            >
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-accent" aria-hidden />
              <span>
                {replaced.length === 1
                  ? t("console.assign.replaces", { role: replaced[0]?.role_name })
                  : t("console.assign.replacesMany", {
                      roles: replaced.map((r) => r.role_name).join(", "),
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
