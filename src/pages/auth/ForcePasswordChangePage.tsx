import { Lock } from "lucide-react";
import { type FormEvent, useState } from "react";
import { useTranslation } from "react-i18next";

import { AuthField } from "@/components/auth/shell/AuthField";
import { AuthAlert, AuthButton } from "@/components/auth/shell/AuthPrimitives";
import { AuthShell } from "@/components/auth/shell/AuthShell";
import { mfAuth } from "@/design-system/tokens";
import { describeAppError } from "@/lib/app-error";

interface ForcePasswordChangePageProps {
  onComplete: (newPassword: string) => Promise<void>;
}

/** Minimum length checked here before the request; the server applies the full policy. */
const MIN_LENGTH = 8;

/**
 * Mandatory password change (account flagged by an administrator, or the server answered
 * PASSWORD_CHANGE_REQUIRED). Same auth shell as the login screen; `onComplete` and what it sends
 * are unchanged.
 */
export function ForcePasswordChangePage({ onComplete }: ForcePasswordChangePageProps) {
  const { t } = useTranslation("auth");
  const [newPassword, setNewPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [fieldError, setFieldError] = useState<{ field: "new" | "confirm"; text: string } | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    setFieldError(null);

    if (newPassword.length < MIN_LENGTH) {
      setFieldError({
        field: "new",
        text: t("session.forcePasswordChange.tooShort", { min: MIN_LENGTH }),
      });
      return;
    }

    if (newPassword !== confirmPassword) {
      setFieldError({ field: "confirm", text: t("session.forcePasswordChange.mismatch") });
      return;
    }

    setLoading(true);
    try {
      await onComplete(newPassword);
    } catch (err) {
      // The session store rethrows the server's message (password policy, history …).
      setError(
        err instanceof Error && err.message
          ? err.message
          : describeAppError(err).title || t("login.error.unknown"),
      );
    } finally {
      setLoading(false);
    }
  };

  return (
    <AuthShell width="sm">
      <h1 className={mfAuth.title}>{t("session.forcePasswordChange.title")}</h1>
      <p className={mfAuth.subtitle}>{t("session.forcePasswordChange.message")}</p>

      <form onSubmit={(e) => void handleSubmit(e)} className="mt-6 space-y-4">
        <AuthField
          id="new-password"
          label={t("session.forcePasswordChange.newPassword")}
          icon={<Lock />}
          type="password"
          autoComplete="new-password"
          autoFocus
          required
          value={newPassword}
          onChange={(e) => setNewPassword(e.target.value)}
          disabled={loading}
          hint={t("session.forcePasswordChange.hint", { min: MIN_LENGTH })}
          error={fieldError?.field === "new" ? fieldError.text : null}
        />

        <AuthField
          id="confirm-password"
          label={t("session.forcePasswordChange.confirmPassword")}
          icon={<Lock />}
          type="password"
          autoComplete="new-password"
          required
          value={confirmPassword}
          onChange={(e) => setConfirmPassword(e.target.value)}
          disabled={loading}
          error={fieldError?.field === "confirm" ? fieldError.text : null}
        />

        {error ? (
          <AuthAlert tone="danger" role="alert">
            {error}
          </AuthAlert>
        ) : null}

        <AuthButton type="submit" loading={loading} disabled={loading}>
          {loading ? t("login.form.submitting") : t("session.forcePasswordChange.submit")}
        </AuthButton>
      </form>
    </AuthShell>
  );
}
