// Signed-out form for a vendor administrator recovery code (`auth::recovery_code`): the code, then
// the new administrator's username, name and password. The backend verifies the code (vendor
// signature, this PC's activation, validity, single use) before it looks at the account fields,
// and never changes an existing account.

import { KeyRound, Loader2 } from "lucide-react";
import { type FormEvent, useEffect, useState } from "react";
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
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { mfAlert } from "@/design-system/tokens";
import { appErrorText } from "@/lib/app-error";
import { cn } from "@/lib/utils";
import { redeemAdminRecoveryCode } from "@/services/admin-recovery-service";

export type AdminRecoveryCodeDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
};

export function AdminRecoveryCodeDialog({ open, onOpenChange }: AdminRecoveryCodeDialogProps) {
  const { t } = useTranslation("auth");
  const [code, setCode] = useState("");
  const [username, setUsername] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [password, setPassword] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [createdUsername, setCreatedUsername] = useState<string | null>(null);

  // Nothing typed here outlives the dialog (the code and the password above all).
  useEffect(() => {
    if (open) return;
    setCode("");
    setUsername("");
    setDisplayName("");
    setPassword("");
    setConfirmation("");
    setError(null);
    setSubmitting(false);
    setCreatedUsername(null);
  }, [open]);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (submitting) return;
    setError(null);
    if (password !== confirmation) {
      setError(t("login.recoveryCode.mismatch"));
      return;
    }
    setSubmitting(true);
    try {
      const result = await redeemAdminRecoveryCode({
        code: code.trim(),
        username: username.trim(),
        display_name: displayName.trim(),
        password,
        password_confirmation: confirmation,
      });
      setPassword("");
      setConfirmation("");
      setCode("");
      setCreatedUsername(result.username);
    } catch (err) {
      setError(appErrorText(err));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="max-h-[calc(100dvh-2rem)] max-w-md overflow-y-auto"
        data-testid="admin-recovery-dialog"
      >
        <DialogHeader>
          <DialogTitle>{t("login.recoveryCode.title")}</DialogTitle>
          <DialogDescription>{t("login.recoveryCode.description")}</DialogDescription>
        </DialogHeader>

        {createdUsername !== null ? (
          <div className="space-y-3" data-testid="admin-recovery-success">
            <p role="status" className={mfAlert.success}>
              {t("login.recoveryCode.success", { username: createdUsername })}
            </p>
            <DialogFooter>
              <Button type="button" onClick={() => onOpenChange(false)}>
                {t("login.recoveryCode.close")}
              </Button>
            </DialogFooter>
          </div>
        ) : (
          <form className="space-y-3" onSubmit={(e) => void handleSubmit(e)}>
            <div className="space-y-1.5">
              <Label htmlFor="admin-recovery-code">{t("login.recoveryCode.code")}</Label>
              <Textarea
                id="admin-recovery-code"
                value={code}
                onChange={(e) => setCode(e.target.value)}
                required
                rows={3}
                spellCheck={false}
                autoComplete="off"
                className="font-mono text-xs"
                placeholder="MFXREC1.…"
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="admin-recovery-username">{t("login.recoveryCode.username")}</Label>
              <Input
                id="admin-recovery-username"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                required
                autoComplete="off"
              />
              <p className="text-xs text-text-muted">{t("login.recoveryCode.usernameHint")}</p>
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="admin-recovery-display-name">
                {t("login.recoveryCode.displayName")}
              </Label>
              <Input
                id="admin-recovery-display-name"
                value={displayName}
                onChange={(e) => setDisplayName(e.target.value)}
                required
                autoComplete="off"
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="admin-recovery-password">{t("login.recoveryCode.password")}</Label>
              <Input
                id="admin-recovery-password"
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                required
                autoComplete="new-password"
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="admin-recovery-confirmation">
                {t("login.recoveryCode.confirmation")}
              </Label>
              <Input
                id="admin-recovery-confirmation"
                type="password"
                value={confirmation}
                onChange={(e) => setConfirmation(e.target.value)}
                required
                autoComplete="new-password"
              />
            </div>

            {error ? (
              <p role="alert" className={cn(mfAlert.danger)} data-testid="admin-recovery-error">
                {error}
              </p>
            ) : null}

            <DialogFooter className="gap-2">
              <Button
                type="button"
                variant="outline"
                onClick={() => onOpenChange(false)}
                disabled={submitting}
              >
                {t("login.recoveryCode.cancel")}
              </Button>
              <Button type="submit" disabled={submitting}>
                {submitting ? (
                  <Loader2 className="mr-1.5 h-4 w-4 animate-spin" aria-hidden />
                ) : (
                  <KeyRound className="mr-1.5 h-4 w-4" aria-hidden />
                )}
                {t("login.recoveryCode.submit")}
              </Button>
            </DialogFooter>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}
