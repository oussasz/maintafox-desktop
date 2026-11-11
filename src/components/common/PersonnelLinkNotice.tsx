/**
 * Up-front notice for an account without a personnel record: says what does not work and offers
 * the way to fix it that the person's rights allow (user administration to link the account,
 * Personnel to create the record), or tells them to ask an administrator.
 */
import { UserX } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

import { usePermissions } from "@/hooks/use-permissions";
import { P } from "@shared/rbac/permissions.generated";

export interface PersonnelLinkNoticeProps {
  /** What the missing link breaks on the screen showing the notice. */
  context: "rounds" | "myWork";
}

const LINK_CLASS = "font-medium underline underline-offset-2";

export function PersonnelLinkNotice({ context }: PersonnelLinkNoticeProps) {
  const { t } = useTranslation("common");
  const { can } = usePermissions();
  const canLinkAccount = can(P.ADM_USERS);
  const canOpenPersonnel = can(P.PER_VIEW);

  return (
    <div
      role="alert"
      className="flex items-start gap-2 rounded-md border border-tone-warning-border bg-tone-warning-bg px-3 py-2 text-sm text-tone-warning-fg"
      data-testid="personnel-link-notice"
    >
      <UserX className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
      <div className="space-y-1">
        <p>{t(`personnelLink.message.${context}`)}</p>
        {canLinkAccount || canOpenPersonnel ? (
          <p className="flex flex-wrap gap-x-4 gap-y-1">
            {canLinkAccount ? (
              <Link to="/admin?tab=users" className={LINK_CLASS}>
                {t("personnelLink.linkAccount")}
              </Link>
            ) : null}
            {canOpenPersonnel ? (
              <Link to="/personnel" className={LINK_CLASS}>
                {t("personnelLink.openPersonnel")}
              </Link>
            ) : null}
          </p>
        ) : (
          <p>{t("personnelLink.askAdmin")}</p>
        )}
        {canLinkAccount ? <p className="text-xs">{t("personnelLink.howTo")}</p> : null}
      </div>
    </div>
  );
}
