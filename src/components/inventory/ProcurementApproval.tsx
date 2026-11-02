/**
 * Who approves a submitted requisition or purchase order, and why the signed-in user cannot.
 *
 * The backend decides (`inv.approve`, separation of duties, approval limits that fail closed);
 * these helpers only make the rule visible: the roles named are those holding the approval right
 * with a limit covering the document amount (`list_inventory_procurement_approver_roles`).
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { usePermissions } from "@/hooks/use-permissions";
import {
  type ApprovalDocumentType,
  type ProcurementApproverRoles,
  listProcurementApproverRoles,
} from "@/services/inventory-service";
import { useSessionStore } from "@/store/session-store";
import { P } from "@shared/rbac/permissions.generated";

/**
 * Why the signed-in user cannot approve or reject a document whose author is `authorId`
 * (translated), or null when nothing in the interface stands in the way (the backend still
 * checks the approval limit).
 */
export function useApprovalBlocker(authorId: number | null): string | null {
  const { t } = useTranslation("inventory");
  const { can } = usePermissions();
  const userId = useSessionStore((s) => s.info?.user_id ?? null);
  if (!can(P.INV_APPROVE)) return t("procurement.approval.noRight");
  if (authorId != null && userId != null && authorId === userId) {
    return t("procurement.approval.ownDocument");
  }
  return null;
}

/** "À approuver par : …" for a submitted document; reloads when `reloadKey` changes. */
export function ApproverRolesLine({
  documentType,
  documentId,
  reloadKey,
}: {
  documentType: ApprovalDocumentType;
  documentId: number;
  reloadKey?: unknown;
}) {
  const { t } = useTranslation("inventory");
  const [result, setResult] = useState<ProcurementApproverRoles | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setResult(null);
    setFailed(false);
    listProcurementApproverRoles(documentType, documentId)
      .then((value) => {
        if (!cancelled) setResult(value);
      })
      .catch(() => {
        if (!cancelled) setFailed(true);
      });
    return () => {
      cancelled = true;
    };
  }, [documentType, documentId, reloadKey]);

  let text: string;
  if (failed) text = t("procurement.approval.approversUnavailable");
  else if (!result) text = t("procurement.approval.approversLoading");
  else if (result.role_names.length === 0) text = t("procurement.approval.approversNone");
  else text = t("procurement.approval.approvers", { roles: result.role_names.join(", ") });

  return (
    <div className="space-y-0.5 text-sm" role="status" data-testid="approver-roles">
      <p className="font-medium">{text}</p>
      {result && result.amount == null ? (
        <p className="text-xs text-text-muted">{t("procurement.approval.amountUnknown")}</p>
      ) : null}
    </div>
  );
}
