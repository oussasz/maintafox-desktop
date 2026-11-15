import { type FormEvent, useState } from "react";

import { login } from "@/services/auth-service";
import type { SessionInfo } from "@shared/ipc-types";

import { useMobileT } from "../i18n";
import { type UserError, toUserError } from "../lib/user-error";
import { OnboardingScreen } from "../ui/OnboardingScreen";
import { ErrorBanner } from "../ui/feedback";
import { Actions, Button, Hero, Section, Steps, TextCell } from "../ui/kit";

import { ONBOARDING_STEPS } from "./ActivationGate";

/** Sign-in with the engine's `login` command; refusals are shown in the user's words. */
export function SignInScreen({
  onSignedIn,
  error: outerError = null,
}: {
  onSignedIn: (session: SessionInfo) => void;
  /** A failure from before this screen (reading the session), shown above the form. */
  error?: UserError | null;
}) {
  const { t } = useMobileT();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<UserError | null>(null);

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const response = await login({ username: username.trim(), password });
      setPassword("");
      onSignedIn(response.session_info);
    } catch (err) {
      setError(toUserError(err, "sign_in", "signin"));
    } finally {
      setBusy(false);
    }
  }

  const shown = error ?? outerError;
  return (
    <OnboardingScreen title={t("signin.title")} language>
      <Hero title={t("signin.title")} text={t("signin.subtitle")} />
      <Steps count={ONBOARDING_STEPS} current={2} label={t("onboarding.steps")} />
      <form className="mf-form-stack" onSubmit={(e) => void onSubmit(e)}>
        <Section>
          <TextCell
            label={t("signin.username")}
            value={username}
            placeholder={t("signin.usernamePlaceholder")}
            autoComplete="username"
            autoCapitalize="none"
            spellCheck={false}
            dir="ltr"
            onChange={(e) => setUsername(e.target.value)}
          />
          <TextCell
            label={t("signin.password")}
            type="password"
            value={password}
            placeholder={t("signin.passwordPlaceholder")}
            autoComplete="current-password"
            dir="ltr"
            onChange={(e) => setPassword(e.target.value)}
          />
        </Section>
        {shown ? <ErrorBanner error={shown} /> : null}
        <Actions>
          <Button type="submit" disabled={busy || username.trim() === "" || password === ""}>
            {busy ? t("signin.submitting") : t("signin.submit")}
          </Button>
        </Actions>
      </form>
    </OnboardingScreen>
  );
}
