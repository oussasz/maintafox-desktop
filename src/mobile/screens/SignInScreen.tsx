import { type FormEvent, useState } from "react";

import { lockoutUntil } from "@/lib/account-lockout";
import { login } from "@/services/auth-service";
import { toErrorMessage } from "@/utils/errors";
import type { SessionInfo } from "@shared/ipc-types";

import { ErrorBox, Shell } from "../components/Shell";
import { useMobileT } from "../i18n";

/** The refusal of the engine as it says it, with the end of an account lockout when it sends one. */
function signInFailure(err: unknown): string {
  const message = toErrorMessage(err);
  const until = lockoutUntil(err);
  return until ? `${message} (${until})` : message;
}

/** Sign-in with the engine's `login` command; lockout and password errors are the engine's own text. */
export function SignInScreen({ onSignedIn }: { onSignedIn: (session: SessionInfo) => void }) {
  const { t } = useMobileT();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const response = await login({ username: username.trim(), password });
      setPassword("");
      onSignedIn(response.session_info);
    } catch (err) {
      setError(signInFailure(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Shell title={t("signin.title")}>
      <form className="mf-form" onSubmit={(e) => void onSubmit(e)}>
        <label className="mf-field">
          <span>{t("signin.username")}</span>
          <input
            className="mf-input"
            value={username}
            autoComplete="username"
            autoCapitalize="none"
            spellCheck={false}
            dir="ltr"
            onChange={(e) => setUsername(e.target.value)}
          />
        </label>
        <label className="mf-field">
          <span>{t("signin.password")}</span>
          <input
            className="mf-input"
            type="password"
            value={password}
            autoComplete="current-password"
            dir="ltr"
            onChange={(e) => setPassword(e.target.value)}
          />
        </label>
        {error ? <ErrorBox title={t("common.errorTitle")} message={error} /> : null}
        <button
          type="submit"
          className="mf-btn mf-btn--primary"
          disabled={busy || username.trim() === "" || password === ""}
        >
          {busy ? t("signin.submitting") : t("signin.submit")}
        </button>
      </form>
    </Shell>
  );
}
