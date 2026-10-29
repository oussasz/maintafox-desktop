import { useTranslation } from "react-i18next";

import { MOBILE_LANGUAGES, type MobileLanguage, useMobileT } from "../i18n";

/** FR / AR / EN switch shown on every screen. The language names are written in their own language. */
export function LanguageSwitch() {
  const { t, language } = useMobileT();
  const { i18n } = useTranslation();

  return (
    <div className="mf-lang" role="group" aria-label={t("language.label")}>
      {MOBILE_LANGUAGES.map((code: MobileLanguage) => (
        <button
          key={code}
          type="button"
          className="mf-lang__btn"
          aria-pressed={language === code}
          lang={code}
          onClick={() => {
            void i18n.changeLanguage(code);
          }}
        >
          {t(`language.${code}`)}
        </button>
      ))}
    </div>
  );
}
