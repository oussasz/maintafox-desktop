import type { ReactNode } from "react";

import { LanguageSwitch } from "../components/LanguageSwitch";

import { Screen } from "./kit";

/**
 * Layout of the screens before sign-in (phone check, start-up, plant join, sign-in): content
 * centred, no navigation, the language switch at the bottom so it can be changed at any step.
 */
export function OnboardingScreen({
  title,
  language = false,
  children,
}: {
  title: string;
  /** Show the FR / AR / EN switch (first screens a person meets: phone check, licence, sign-in). */
  language?: boolean;
  children: ReactNode;
}) {
  return (
    <Screen title={title} hideTitle center>
      {children}
      {language ? (
        <div className="mf-lang-corner">
          <LanguageSwitch />
        </div>
      ) : null}
    </Screen>
  );
}
