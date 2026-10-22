import { Lock } from "lucide-react";
import { type FormEvent, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { BuiltinAdminWarning } from "@/components/auth/BuiltinAdminWarning";
import { AuthField } from "@/components/auth/shell/AuthField";
import { AuthAlert, AuthButton } from "@/components/auth/shell/AuthPrimitives";
import { AuthShell } from "@/components/auth/shell/AuthShell";
import { mfAuth } from "@/design-system/tokens";
import { describeAppError } from "@/lib/app-error";
import { getPasswordRequirements, type PasswordRequirements } from "@/services/auth-service";
import { useSessionStore } from "@/store/session-store";

interface ForcePasswordChangePageProps {
  onComplete: (newPassword: string) => Promise<void>;
}

/**
 * Mandatory password change (account flagged by an administrator, or the server answered
 * PASSWORD_CHANGE_REQUIRED). Same auth shell as the login screen; `onComplete` and what it sends
 * are unchanged.
 */
export function ForcePasswordChangePage({ onComplete }: ForcePasswordChangePageProps) {
  const { t } = useTranslation("auth");
  const sessionUserId = useSessionStore((s) => s.info?.user_id ?? null);
  const [newPassword, setNewPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [fieldError, setFieldError] = useState<{ field: "new" | "confirm"; text: string } | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  // The plant's own password rules. Until they are read, the screen checks nothing itself and the
  // server decides (no guessed minimum).
  const [requirements, setRequirements] = useState<PasswordRequirements | null>(null);

  useEffect(() => {
    let cancelled = false;
    getPasswordRequirements()
      .then((r) => {
        if (!cancelled) setRequirements(r);
      })
      .catch(() => {
        // The server still refuses a weak password with its own explanation.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const rules = requirements
    ? [
        requirements.require_uppercase && t("session.forcePasswordChange.rule.uppercase"),
        requirements.require_lowercase && t("session.forcePasswordChange.rule.lowercase"),
        requirements.require_digit && t("session.forcePasswordChange.rule.digit"),
        requirements.require_special && t("session.forcePasswordChange.rule.special"),
      ].filter(Boolean)
    : [];

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    setFieldError(null);

    if (requirements && newPassword.length < requirements.min_length) {
      setFieldError({
        field: "new",
        text: t("session.forcePasswordChange.tooShort", { min: requirements.min_length }),
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
      // Translated text for the failure's code; the policy details (length, history …) as the
      // description when the server gives them.
      const described = describeAppError(err);
      setError(
        [described.title || t("login.error.unknown"), described.description]
          .filter(Boolean)
          .join(" — "),
      );
    } finally {
      setLoading(false);
    }
  };

  return (
    <AuthShell width="sm">
      <h1 className={mfAuth.title}>{t("session.forcePasswordChange.title")}</h1>
      <p className={mfAuth.subtitle}>{t("session.forcePasswordChange.message")}</p>
      <BuiltinAdminWarning sessionUserId={sessionUserId} className="mt-4" />

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
          hint={
            requirements
              ? [
                  t("session.forcePasswordChange.hint", { min: requirements.min_length }),
                  rules.length > 0
                    ? t("session.forcePasswordChange.rulesLine", { rules: rules.join(", ") })
                    : null,
                ]
                  .filter(Boolean)
                  .join(" ")
              : t("session.forcePasswordChange.hintGeneric")
          }
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
