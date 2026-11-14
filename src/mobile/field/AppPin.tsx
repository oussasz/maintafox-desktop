// DESIGN MOCKUP (step 1 of the field app, not built yet): unlock the app with its own PIN after
// the phone was locked or the app was in the background. Shown only by the design preview page.

import { Delete } from "lucide-react";

import { useMobileT } from "../i18n";
import { MockupFlag, Screen } from "../ui/kit";

const PIN_LENGTH = 4;
const KEYS = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];

export function AppPinMockup({ name, filled }: { name: string; filled: number }) {
  const { t } = useMobileT();
  return (
    <Screen title={t("pin.title")} hideTitle center trailing={<MockupFlag />}>
      <div className="mf-hero">
        <span className="mf-hero__mark">
          <img src="/icons/maintafox.svg" alt="" />
        </span>
        <h1 className="mf-hero__title">{t("pin.title")}</h1>
        <p className="mf-hero__text">{t("pin.subtitle", { name })}</p>
      </div>
      <div className="mf-pin-dots" aria-label={t("pin.entered", { count: filled })}>
        {Array.from({ length: PIN_LENGTH }, (_, i) => (
          <span key={i} className={i < filled ? "is-filled" : undefined} />
        ))}
      </div>
      <div className="mf-pinpad">
        {KEYS.map((k) => (
          <button key={k} type="button">
            {k}
          </button>
        ))}
        <button type="button" className="mf-pinpad__plain">
          {t("pin.signOut")}
        </button>
        <button type="button">0</button>
        <button type="button" className="mf-pinpad__plain" aria-label={t("pin.delete")}>
          <Delete size={26} aria-hidden />
        </button>
      </div>
    </Screen>
  );
}
