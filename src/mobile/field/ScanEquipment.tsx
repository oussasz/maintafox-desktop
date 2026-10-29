// DESIGN MOCKUP (step 1 of the field app, not built yet): scan an equipment QR code and open its
// card. Shown only by the design preview page with sample data passed in.

import { AlertTriangle, ClipboardList, Flashlight, History, Keyboard, MapPin } from "lucide-react";

import { useMobileT } from "../i18n";
import {
  Actions,
  BackButton,
  Button,
  MockupFlag,
  NavButton,
  Pill,
  Row,
  Screen,
  Section,
} from "../ui/kit";

export interface ScanSample {
  code: string;
  name: string;
  location: string;
  criticality: string;
  status: string;
  openWos: Array<{ code: string; title: string; status: string }>;
  history: Array<{ date: string; text: string }>;
}

export function ScanEquipmentMockup({ found }: { found: ScanSample | null }) {
  const { t } = useMobileT();

  if (!found) {
    return (
      <Screen
        title={t("scan.title")}
        withTabs
        trailing={
          <>
            <MockupFlag />
            <NavButton
              label={t("scan.torch")}
              icon={<Flashlight size={22} aria-hidden />}
              onClick={() => undefined}
            />
          </>
        }
      >
        <div className="mf-scanner" aria-label={t("scan.hint")}>
          <div className="mf-scanner__frame" />
          <div className="mf-scanner__line" />
          <p className="mf-scanner__hint">{t("scan.hint")}</p>
        </div>
        <Actions>
          <Button variant="gray">
            <Keyboard size={20} aria-hidden />
            {t("scan.manual")}
          </Button>
        </Actions>
      </Screen>
    );
  }

  return (
    <Screen
      title={t("scan.equipment")}
      inlineTitle
      withTabs
      leading={<BackButton label={t("scan.title")} onClick={() => undefined} />}
      trailing={<MockupFlag />}
    >
      <div className="mf-asset-card">
        <span className="mf-asset-card__code mf-ltr">{found.code}</span>
        <h2 className="mf-asset-card__name">{found.name}</h2>
        <span className="mf-wo__meta">
          <span>
            <MapPin size={15} aria-hidden />
            {found.location}
          </span>
        </span>
        <div className="mf-detail-head__pills">
          <Pill tone="green" dot>
            {found.status}
          </Pill>
          <Pill tone="red">{found.criticality}</Pill>
        </div>
      </div>
      <Actions>
        <Button variant="attention">
          <AlertTriangle size={20} aria-hidden />
          {t("scan.reportFault")}
        </Button>
      </Actions>
      <Section header={t("scan.openWos")}>
        {found.openWos.map((wo) => (
          <Row
            key={wo.code}
            icon={<ClipboardList aria-hidden />}
            title={wo.title}
            subtitle={<span className="mf-ltr">{wo.code}</span>}
            detail={wo.status}
            chevron
            onClick={() => undefined}
          />
        ))}
      </Section>
      <Section header={t("scan.history")}>
        {found.history.map((h) => (
          <Row
            key={h.date + h.text}
            icon={<History aria-hidden />}
            iconColor="var(--mf-label-3)"
            title={h.text}
            subtitle={h.date}
          />
        ))}
      </Section>
    </Screen>
  );
}
