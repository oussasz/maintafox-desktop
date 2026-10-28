/**
 * The second planner who signs a break-in (owner decision D6, CMMS Advisor Q1): chosen among the
 * people holding plan.confirm at the crew's site (`listBreakInApprovers`, never the signed-in
 * user), who types their password on this PC. An urgent break-in, or one that forces a blocking
 * check, must be signed; the backend checks all of it again (right at the site, password, lockout).
 */

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { listBreakInApprovers } from "@/services/planning-service";
import type { BreakInApproverOption } from "@shared/ipc-types";

type BreakInSignerFieldsProps = {
  /** Loads the approvers only while the dialog is open. */
  open: boolean;
  /** The crew the break-in is for: the approver holds plan.confirm at its site. */
  teamId: number | null;
  /** The crew the work leaves, when it changes crew: the approver holds the right there too. */
  fromTeamId?: number | null;
  approverUserId: number | null;
  onApproverChange: (userId: number | null) => void;
  password: string;
  onPasswordChange: (password: string) => void;
  /** An urgent reason: the signature is required. */
  required: boolean;
  idPrefix: string;
};

export function BreakInSignerFields({
  open,
  teamId,
  fromTeamId = null,
  approverUserId,
  onApproverChange,
  password,
  onPasswordChange,
  required,
  idPrefix,
}: BreakInSignerFieldsProps) {
  const { t } = useTranslation("planning");
  const [approvers, setApprovers] = useState<BreakInApproverOption[] | null>(null);

  useEffect(() => {
    if (!open || teamId == null) {
      setApprovers(null);
      return;
    }
    let cancelled = false;
    listBreakInApprovers(teamId, fromTeamId != null && fromTeamId !== teamId ? fromTeamId : null)
      .then((list) => {
        if (!cancelled) setApprovers(list);
      })
      .catch(() => {
        if (!cancelled) setApprovers([]);
      });
    return () => {
      cancelled = true;
    };
  }, [open, teamId, fromTeamId]);

  // An approver who is no longer offered (another crew chosen) is not kept.
  useEffect(() => {
    if (
      approvers != null &&
      approverUserId != null &&
      !approvers.some((a) => a.user_id === approverUserId)
    ) {
      onApproverChange(null);
    }
  }, [approvers, approverUserId, onApproverChange]);

  const passwordId = `${idPrefix}-approver-password`;
  return (
    <div className="grid gap-2">
      <div>
        <Label>{t("breakIns.approver")}</Label>
        <Select
          value={approverUserId != null ? String(approverUserId) : "__none__"}
          onValueChange={(v) => onApproverChange(v === "__none__" ? null : Number(v))}
        >
          <SelectTrigger className="h-8 text-sm" aria-label={t("breakIns.approver")}>
            <SelectValue placeholder={t("breakIns.selectApprover")} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__none__">{t("breakIns.selectApprover")}</SelectItem>
            {(approvers ?? []).map((user) => (
              <SelectItem key={user.user_id} value={String(user.user_id)}>
                {user.display_name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        {approvers != null && approvers.length === 0 ? (
          <p className="mt-1 text-xs text-text-warning" role="status">
            {t("breakIns.noApprovers")}
          </p>
        ) : null}
      </div>
      {approverUserId != null || required ? (
        <div>
          <Label htmlFor={passwordId}>{t("breakIns.approverPassword")}</Label>
          <Input
            id={passwordId}
            type="password"
            autoComplete="off"
            className="h-8 text-sm"
            value={password}
            onChange={(e) => onPasswordChange(e.target.value)}
            placeholder={t("breakIns.approverPasswordPlaceholder")}
          />
          <p className="mt-1 text-xs text-text-secondary">{t("breakIns.signatureHint")}</p>
        </div>
      ) : null}
    </div>
  );
}

/** The signature is complete enough to send: required → an approver and a password; an approver
 * chosen → their password. */
export function signatureMissing(
  required: boolean,
  approverUserId: number | null,
  password: string,
): boolean {
  if (approverUserId == null) return required;
  return password === "";
}
