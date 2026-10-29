import { Globe, LogOut, Moon, Server, Smartphone } from "lucide-react";
import { useEffect, useState } from "react";

import { getAppInfo } from "@/services/app.service";
import type { SessionInfo } from "@shared/ipc-types";

import { LanguageSwitch } from "../components/LanguageSwitch";
import { useMobileT } from "../i18n";
import { type Appearance, setAppearance, storedAppearance } from "../lib/appearance";
import { isTestServer, serverAddress } from "../lib/server";
import type { UserError } from "../lib/user-error";
import { ErrorBanner } from "../ui/feedback";
import { Actions, Banner, Button, Row, Screen, Section, Segmented, Sheet } from "../ui/kit";

/** Two letters of the user's name for the account avatar. */
function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  const letters = parts.length > 1 ? [parts[0], parts[parts.length - 1]] : [name.trim()];
  return letters
    .map((p) => (p ?? "").charAt(0))
    .join("")
    .slice(0, 2)
    .toLocaleUpperCase();
}

/** Account, language, appearance, server, version, sign out. */
export function SettingsScreen({
  session,
  onSignOut,
  signOutError,
}: {
  session: SessionInfo;
  onSignOut: () => Promise<void>;
  signOutError: UserError | null;
}) {
  const { t } = useMobileT();
  const [appearance, setAppearanceState] = useState<Appearance>(storedAppearance);
  const [version, setVersion] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [signingOut, setSigningOut] = useState(false);

  useEffect(() => {
    let cancelled = false;
    getAppInfo()
      .then((info) => {
        if (!cancelled) setVersion(info.version);
      })
      .catch(() => {
        // The version row stays hidden; nothing else depends on it.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const name = session.display_name ?? session.username ?? "";

  return (
    <Screen title={t("settings.title")} withTabs>
      <Section>
        <div className="mf-row" style={{ minBlockSize: 72 }}>
          <span
            aria-hidden
            style={{
              display: "grid",
              placeItems: "center",
              inlineSize: 52,
              blockSize: 52,
              borderRadius: "50%",
              background: "var(--mf-brand-navy)",
              color: "#ffffff",
              font: "var(--mf-title-3)",
            }}
          >
            {initials(name)}
          </span>
          <span className="mf-row__main">
            <span className="mf-row__title" style={{ font: "var(--mf-title-3)" }}>
              {name}
            </span>
            {session.username && session.username !== name ? (
              <span className="mf-row__subtitle mf-ltr">{session.username}</span>
            ) : null}
          </span>
        </div>
      </Section>

      <Section header={t("settings.preferences")}>
        <div className="mf-row mf-row--icon" style={{ flexWrap: "wrap" }}>
          <span className="mf-row__icon">
            <Globe aria-hidden />
          </span>
          <span className="mf-row__main">{t("language.label")}</span>
          <div style={{ inlineSize: "100%" }}>
            <LanguageSwitch />
          </div>
        </div>
        <div className="mf-row mf-row--icon" style={{ flexWrap: "wrap" }}>
          <span className="mf-row__icon" style={{ background: "var(--mf-brand-navy)" }}>
            <Moon aria-hidden />
          </span>
          <span className="mf-row__main">{t("settings.appearance")}</span>
          <div style={{ inlineSize: "100%" }}>
            <Segmented<Appearance>
              label={t("settings.appearance")}
              value={appearance}
              options={[
                { value: "system", label: t("settings.appearanceSystem") },
                { value: "light", label: t("settings.appearanceLight") },
                { value: "dark", label: t("settings.appearanceDark") },
              ]}
              onChange={(next) => {
                setAppearance(next);
                setAppearanceState(next);
              }}
            />
          </div>
        </div>
      </Section>

      <Section header={t("settings.about")}>
        <Row
          icon={<Server aria-hidden />}
          iconColor={isTestServer ? "var(--mf-red)" : "var(--mf-green)"}
          title={t("settings.server")}
          detail={isTestServer ? t("server.testFlag") : t("settings.serverProduction")}
        />
        {version ? (
          <Row
            icon={<Smartphone aria-hidden />}
            iconColor="var(--mf-label-3)"
            title={t("settings.version")}
            detail={<span className="mf-ltr">{version}</span>}
          />
        ) : null}
      </Section>
      {isTestServer ? (
        <Banner tone="warning" title={t("server.testTitle")}>
          {t("server.testText")} <span className="mf-ltr">{serverAddress}</span>
        </Banner>
      ) : null}

      {signOutError ? <ErrorBanner error={signOutError} /> : null}
      <Section>
        <Row
          icon={<LogOut aria-hidden />}
          iconColor="var(--mf-red)"
          title={t("settings.signOut")}
          tone="destructive"
          onClick={() => setConfirming(true)}
        />
      </Section>

      <Sheet
        open={confirming}
        title={t("settings.signOutTitle")}
        onClose={() => setConfirming(false)}
        closeLabel={t("common.close")}
      >
        <p className="mf-hero__text" style={{ marginInline: "auto", textAlign: "center" }}>
          {t("settings.signOutText")}
        </p>
        <Actions>
          <Button
            variant="destructive"
            disabled={signingOut}
            onClick={() => {
              setSigningOut(true);
              void onSignOut().finally(() => {
                setSigningOut(false);
                setConfirming(false);
              });
            }}
          >
            {t("settings.signOut")}
          </Button>
          <Button variant="plain" onClick={() => setConfirming(false)}>
            {t("common.cancel")}
          </Button>
        </Actions>
      </Sheet>
    </Screen>
  );
}
