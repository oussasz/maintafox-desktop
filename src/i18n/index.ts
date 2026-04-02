import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import enCommon from "./en/common.json";
import enShell from "./en/shell.json";
import frCommon from "./fr/common.json";
import frShell from "./fr/shell.json";

void i18n.use(initReactI18next).init({
  resources: {
    fr: { common: frCommon, shell: frShell },
    en: { common: enCommon, shell: enShell },
  },
  lng: "fr",
  fallbackLng: "en",
  defaultNS: "common",
  interpolation: {
    escapeValue: false,
  },
});

export default i18n;
