import { useTranslation } from "react-i18next";

import { MOBILE_LANGUAGES, type MobileLanguage, useMobileT } from "../i18n";
import { Segmented } from "../ui/kit";

/** FR / AR / EN segmented control. The language names are written in their own language. */
export function LanguageSwitch() {
  const { t, language } = useMobileT();
  const { i18n } = useTranslation();

  return (
    <Segmented<MobileLanguage>
      label={t("language.label")}
      value={language}
      options={MOBILE_LANGUAGES.map((code) => ({
        value: code,
        label: t(`language.${code}`),
        lang: code,
      }))}
      onChange={(code) => {
        void i18n.changeLanguage(code);
      }}
    />
  );
}
