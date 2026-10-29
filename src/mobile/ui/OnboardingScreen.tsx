import type { ReactNode } from "react";

import { LanguageSwitch } from "../components/LanguageSwitch";

import { Screen } from "./kit";

/**
 * Layout of the screens before sign-in (phone check, start-up, plant join, sign-in): content
 * centred, no navigation, the language switch at the bottom so it can be changed at any step.
 */
export function OnboardingScreen({ title, children }: { title: string; children: ReactNode }) {
  return (
    <Screen title={title} hideTitle center>
      {children}
      <div className="mf-lang-corner">
        <LanguageSwitch />
      </div>
    </Screen>
  );
}
