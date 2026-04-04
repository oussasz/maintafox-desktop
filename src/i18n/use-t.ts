// src/i18n/use-t.ts
// Convenience re-export of useTranslation with a typed namespace.
// Usage: import { useT } from "../i18n/use-t";
//         const { t } = useT("auth");  // typed to auth namespace keys

export { useTranslation as useT } from "react-i18next";
