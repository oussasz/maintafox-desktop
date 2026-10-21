import { Loader2, Mail, Phone, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { mfAlert } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import { getPasswordRecoveryContacts } from "@/services/auth-service";
import type { PasswordRecoveryContact } from "@shared/ipc-types";

export type ForgotPasswordDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
};

export function ForgotPasswordDialog({ open, onOpenChange }: ForgotPasswordDialogProps) {
  const { t } = useTranslation("auth");
  const [contacts, setContacts] = useState<PasswordRecoveryContact[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadContacts = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const rows = await getPasswordRecoveryContacts();
      setContacts(Array.isArray(rows) ? rows : []);
    } catch {
      setContacts([]);
      setError(t("login.forgotPassword.error"));
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    if (!open) {
      setContacts([]);
      setError(null);
      setLoading(false);
      return;
    }
    void loadContacts();
    // Reload only when the dialog opens; retry uses loadContacts directly.
    // eslint-disable-next-line react-hooks/exhaustive-deps -- open-gated fetch
  }, [open]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="max-h-[calc(100dvh-2rem)] max-w-md overflow-y-auto"
        data-testid="forgot-password-dialog"
      >
        <DialogHeader>
          <DialogTitle>{t("login.forgotPassword.title")}</DialogTitle>
          <DialogDescription>{t("login.forgotPassword.description")}</DialogDescription>
        </DialogHeader>

        <div className="min-h-[8rem] py-2">
          {loading ? (
            <div
              className="flex items-center justify-center gap-2 py-8 text-sm text-text-muted"
              data-testid="forgot-password-loading"
            >
              <Loader2 className="h-4 w-4 animate-spin" />
              {t("login.forgotPassword.loading")}
            </div>
          ) : error ? (
            <div className="space-y-3" data-testid="forgot-password-error">
              <p role="alert" className={cn("text-sm", mfAlert.danger)}>
                {error}
              </p>
              <Button type="button" variant="outline" size="sm" onClick={() => void loadContacts()}>
                <RefreshCw className="mr-1.5 h-3.5 w-3.5" />
                {t("login.forgotPassword.retry")}
              </Button>
            </div>
          ) : contacts.length === 0 ? (
            <p
              className="py-6 text-center text-sm text-text-secondary"
              data-testid="forgot-password-empty"
            >
              {t("login.forgotPassword.empty")}
            </p>
          ) : (
            <ul className="space-y-3" data-testid="forgot-password-list">
              {contacts.map((contact) => {
                const hasEmail = Boolean(contact.email);
                const hasPhone = Boolean(contact.phone);
                const hasDigital = hasEmail || hasPhone;
                return (
                  <li
                    key={`${contact.display_name}|${contact.email ?? ""}|${contact.phone ?? ""}`}
                    className="rounded-lg border border-surface-border bg-surface-1 px-3 py-2.5"
                  >
                    <p className="text-sm font-semibold text-text-primary">
                      {contact.display_name}
                    </p>
                    {hasEmail ? (
                      <a
                        href={`mailto:${contact.email}`}
                        className="mt-1 flex items-center gap-1.5 text-xs text-primary hover:underline"
                      >
                        <Mail className="h-3 w-3 shrink-0" />
                        {contact.email}
                      </a>
                    ) : null}
                    {hasPhone ? (
                      <a
                        href={`tel:${contact.phone}`}
                        className="mt-1 flex items-center gap-1.5 text-xs text-primary hover:underline"
                      >
                        <Phone className="h-3 w-3 shrink-0" />
                        {contact.phone}
                      </a>
                    ) : null}
                    {!hasDigital ? (
                      <p className="mt-1 text-xs text-text-muted">
                        {t("login.forgotPassword.noDigitalContact")}
                      </p>
                    ) : null}
                  </li>
                );
              })}
            </ul>
          )}
        </div>

        <DialogFooter className="flex-col items-stretch gap-2 sm:flex-col">
          <p className="text-xs text-text-muted">{t("login.forgotPassword.footer")}</p>
          <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
            {t("login.forgotPassword.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
