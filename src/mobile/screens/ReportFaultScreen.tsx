import { BadgeCheck, CheckCircle2, Factory, HardHat, Leaf, Wrench } from "lucide-react";
import { type FormEvent, useCallback, useEffect, useRef, useState } from "react";

import { getReferenceTypeConfig } from "@/components/reference/reference-types";
import { searchAssets } from "@/services/asset-search-service";
import { createDi } from "@/services/di-service";
import { listPublishedReferenceValuesByDomainCode } from "@/services/reference-service";
import { listUrgencyLevels } from "@/services/urgency-scale-service";
import type { AssetSearchResult, DiCreateInput, UrgencyLevel } from "@shared/ipc-types";

import { SyncFreshness } from "../components/SyncFreshness";
import { useAfterBackgroundSync, useSyncStatus } from "../hooks/use-sync-status";
import { type MobileLanguage, useMobileT } from "../i18n";
import { type UserError, toUserError } from "../lib/user-error";
import { ErrorBanner } from "../ui/feedback";
import {
  Actions,
  Banner,
  Button,
  PickerRow,
  type PickerOption,
  Progress,
  Row,
  Screen,
  SearchField,
  Section,
  Sheet,
  TextAreaCell,
  TextCell,
  Toast,
  ToggleRow,
} from "../ui/kit";

const TITLE_MAX = 100;
const DESC_MAX = 1000;
/** Equipment shown per search (the engine ranks them). */
const EQUIPMENT_LIMIT = 20;
const SEARCH_DEBOUNCE_MS = 250;

interface FormLists {
  origin: PickerOption[];
  requestType: PickerOption[];
  symptom: PickerOption[];
  impact: PickerOption[];
  urgency: UrgencyLevel[];
}

type ListsLoad =
  | { kind: "loading" }
  | { kind: "error"; error: UserError }
  | { kind: "ok"; lists: FormLists };

async function readReferenceOptions(
  type: "di.origin" | "di.request_type" | "di.impact" | "work.symptom",
  valueOf: "code" | "id",
): Promise<PickerOption[]> {
  const values = await listPublishedReferenceValuesByDomainCode(
    getReferenceTypeConfig(type).domainCode,
  );
  return values.map((v) => ({ value: valueOf === "id" ? String(v.id) : v.code, label: v.label }));
}

async function readFormLists(): Promise<FormLists> {
  const [origin, requestType, symptom, impact, urgency] = await Promise.all([
    readReferenceOptions("di.origin", "code"),
    readReferenceOptions("di.request_type", "code"),
    // The symptom is stored by its reference value id, the other lists by code (as the desktop).
    readReferenceOptions("work.symptom", "id"),
    readReferenceOptions("di.impact", "code"),
    listUrgencyLevels(),
  ]);
  return { origin, requestType, symptom, impact, urgency };
}

/** The urgency scale has French and English labels; Arabic screens show the French one. */
function urgencyLabel(level: UrgencyLevel, language: MobileLanguage): string {
  return language === "en" ? level.label_en : level.label_fr;
}

/** Equipment search in a bottom sheet; the engine ranks the matches. */
function EquipmentPicker({
  open,
  onClose,
  onPick,
}: {
  open: boolean;
  onClose: () => void;
  onPick: (asset: AssetSearchResult) => void;
}) {
  const { t } = useMobileT();
  const [query, setQuery] = useState("");
  const [matches, setMatches] = useState<AssetSearchResult[] | null>(null);
  const [error, setError] = useState<UserError | null>(null);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    const timer = setTimeout(() => {
      searchAssets({ query: query.trim() === "" ? null : query.trim(), limit: EQUIPMENT_LIMIT })
        .then((rows) => {
          if (cancelled) return;
          setMatches(rows);
          setError(null);
        })
        .catch((err: unknown) => {
          if (!cancelled) setError(toUserError(err, "report_fault.equipment_search"));
        });
    }, SEARCH_DEBOUNCE_MS);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [query, open]);

  return (
    <Sheet open={open} title={t("di.equipment")} onClose={onClose} closeLabel={t("common.close")}>
      <SearchField value={query} onChange={setQuery} placeholder={t("di.equipmentSearch")} />
      {error ? <ErrorBanner error={error} /> : null}
      {matches === null && !error ? <Progress text={t("common.loading")} /> : null}
      {matches && matches.length === 0 ? (
        <p className="mf-section__footer" style={{ textAlign: "center" }}>
          {t("di.equipmentNone")}
        </p>
      ) : null}
      {matches && matches.length > 0 ? (
        <Section list label={t("di.equipment")}>
          {matches.map((a) => (
            <li key={a.id}>
              <Row
                icon={<Wrench aria-hidden />}
                iconColor="var(--mf-tile-navy)"
                title={
                  <>
                    <span className="mf-ltr" style={{ fontWeight: 600 }}>
                      {a.asset_code}
                    </span>{" "}
                    {a.asset_name}
                  </>
                }
                subtitle={a.org_path ?? a.org_node_name ?? undefined}
                onClick={() => onPick(a)}
              />
            </li>
          ))}
        </Section>
      ) : null}
    </Sheet>
  );
}

/**
 * "Report a fault": a DI (intervention request) created through the engine's `create_di`.
 * Every list comes from the engine (equipment search, reference data, urgency scale); nothing is
 * declared here. The submitter is the signed-in user.
 */
export function ReportFaultScreen({ userId }: { userId: number | null }) {
  const { t, language } = useMobileT();
  const [load, setLoad] = useState<ListsLoad>({ kind: "loading" });
  const [reloadToken, setReloadToken] = useState(0);
  // Set when the lists are re-read after a background sync: they are replaced in place, the form
  // stays on screen and keeps what the technician typed.
  const silentReload = useRef(false);
  const { status } = useSyncStatus();

  const [equipment, setEquipment] = useState<AssetSearchResult | null>(null);
  const [pickingEquipment, setPickingEquipment] = useState(false);

  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [origin, setOrigin] = useState("");
  const [requestType, setRequestType] = useState("");
  const [symptom, setSymptom] = useState("");
  const [urgency, setUrgency] = useState("");
  const [impact, setImpact] = useState("");
  const [safety, setSafety] = useState(false);
  const [production, setProduction] = useState(false);
  const [environment, setEnvironment] = useState(false);
  const [quality, setQuality] = useState(false);

  const [busy, setBusy] = useState(false);
  const [problems, setProblems] = useState<string[]>([]);
  const problemsRef = useRef<HTMLDivElement>(null);
  const [submitError, setSubmitError] = useState<UserError | null>(null);
  const [savedCode, setSavedCode] = useState<string | null>(null);
  const clearSaved = useCallback(() => setSavedCode(null), []);

  useEffect(() => {
    let cancelled = false;
    const silent = silentReload.current;
    silentReload.current = false;
    if (!silent) setLoad({ kind: "loading" });
    readFormLists()
      .then((lists) => {
        if (!cancelled) setLoad({ kind: "ok", lists });
      })
      .catch((err: unknown) => {
        // A failed background re-read keeps the lists already on screen.
        if (!cancelled && !silent) {
          setLoad({ kind: "error", error: toUserError(err, "report_fault.lists") });
        }
      });
    return () => {
      cancelled = true;
    };
  }, [reloadToken]);

  useAfterBackgroundSync(status, () => {
    silentReload.current = true;
    setReloadToken((n) => n + 1);
  });

  function resetForm() {
    setEquipment(null);
    setTitle("");
    setDescription("");
    setOrigin("");
    setRequestType("");
    setSymptom("");
    setUrgency("");
    setImpact("");
    setSafety(false);
    setProduction(false);
    setEnvironment(false);
    setQuality(false);
  }

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    setSavedCode(null);
    setSubmitError(null);
    const found: string[] = [];
    const required = (ok: boolean, labelKey: string) => {
      if (!ok) found.push(t("di.required", { field: t(labelKey) }));
    };
    required(equipment !== null, "di.equipment");
    required(title.trim() !== "", "di.subject");
    required(description.trim() !== "", "di.description");
    required(origin !== "", "di.origin");
    required(requestType !== "", "di.requestType");
    required(symptom !== "", "di.symptom");
    required(urgency !== "", "di.urgency");
    required(impact !== "", "di.impact");
    if (equipment && equipment.org_node_id == null) found.push(t("di.noOrgNode"));
    if (userId == null) found.push(t("di.noUser"));
    setProblems(found);
    if (found.length > 0 || equipment === null || equipment.org_node_id == null || userId == null) {
      // Bring the list of what is missing into view (it sits just above the send button).
      requestAnimationFrame(() =>
        problemsRef.current?.scrollIntoView({ behavior: "smooth", block: "center" }),
      );
      return;
    }

    const input: DiCreateInput = {
      equipment_id: equipment.id,
      org_node_id: equipment.org_node_id,
      title: title.trim(),
      description: description.trim(),
      origin_type: origin,
      request_type: requestType,
      symptom_code_id: Number(symptom),
      impact_level: impact,
      production_impact: production,
      safety_flag: safety,
      environmental_flag: environment,
      quality_flag: quality,
      reported_urgency: urgency,
      observed_at: null,
      source_inspection_anomaly_id: null,
      submitter_id: userId,
    };
    setBusy(true);
    try {
      const di = await createDi(input);
      setSavedCode(di.code);
      resetForm();
      window.scrollTo({ top: 0, behavior: "smooth" });
    } catch (err) {
      setSubmitError(toUserError(err, "report_fault.submit"));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Screen
      title={t("di.title")}
      subtitle={
        <>
          {t("di.subtitle")}
          <SyncFreshness status={status} />
        </>
      }
      withTabs
    >
      {load.kind === "loading" ? <Progress text={t("common.loading")} /> : null}
      {load.kind === "error" ? (
        <ErrorBanner error={load.error} onRetry={() => setReloadToken((n) => n + 1)} />
      ) : null}
      {load.kind === "ok"
        ? (
            [
              ["di.origin", load.lists.origin.length],
              ["di.requestType", load.lists.requestType.length],
              ["di.symptom", load.lists.symptom.length],
              ["di.urgency", load.lists.urgency.length],
              ["di.impact", load.lists.impact.length],
            ] as const
          )
            .filter(([, count]) => count === 0)
            .map(([key]) => (
              <Banner key={key} tone="warning" alert title={t("di.listEmptyTitle")}>
                {t("di.listEmpty", { field: t(key) })}
              </Banner>
            ))
        : null}
      {load.kind === "ok" ? (
        <form className="mf-form-stack" onSubmit={(e) => void onSubmit(e)} noValidate>
          <Section header={t("di.equipment")}>
            {equipment ? (
              <Row
                icon={<Wrench aria-hidden />}
                iconColor="var(--mf-tile-navy)"
                title={
                  <>
                    <span className="mf-ltr" style={{ fontWeight: 600 }}>
                      {equipment.asset_code}
                    </span>{" "}
                    {equipment.asset_name}
                  </>
                }
                subtitle={equipment.org_path ?? equipment.org_node_name ?? undefined}
                detail={t("di.equipmentChange")}
                onClick={() => setPickingEquipment(true)}
              />
            ) : (
              <Row
                icon={<Wrench aria-hidden />}
                title={t("di.equipmentChoose")}
                tone="tint"
                chevron
                onClick={() => setPickingEquipment(true)}
              />
            )}
          </Section>

          <Section header={t("di.problem")}>
            <TextCell
              label={t("di.subject")}
              value={title}
              maxLength={TITLE_MAX}
              placeholder={t("di.subjectPlaceholder")}
              onChange={(e) => setTitle(e.target.value)}
            />
            <TextAreaCell
              label={t("di.description")}
              value={description}
              maxLength={DESC_MAX}
              placeholder={t("di.descriptionPlaceholder")}
              onChange={(e) => setDescription(e.target.value)}
            />
          </Section>

          <Section header={t("di.classification")}>
            <PickerRow
              label={t("di.origin")}
              options={load.lists.origin}
              value={origin}
              onChange={setOrigin}
              placeholder={t("common.choose")}
            />
            <PickerRow
              label={t("di.requestType")}
              options={load.lists.requestType}
              value={requestType}
              onChange={setRequestType}
              placeholder={t("common.choose")}
            />
            <PickerRow
              label={t("di.symptom")}
              options={load.lists.symptom}
              value={symptom}
              onChange={setSymptom}
              placeholder={t("common.choose")}
            />
            <PickerRow
              label={t("di.urgency")}
              options={load.lists.urgency.map((u) => ({
                value: u.code,
                label: urgencyLabel(u, language),
                color: u.color_hex,
              }))}
              value={urgency}
              onChange={setUrgency}
              placeholder={t("common.choose")}
            />
            <PickerRow
              label={t("di.impact")}
              options={load.lists.impact}
              value={impact}
              onChange={setImpact}
              placeholder={t("common.choose")}
            />
          </Section>

          <Section header={t("di.flags")}>
            <ToggleRow
              label={t("di.safety")}
              icon={<HardHat aria-hidden />}
              iconColor="var(--mf-red)"
              checked={safety}
              onChange={setSafety}
            />
            <ToggleRow
              label={t("di.production")}
              icon={<Factory aria-hidden />}
              iconColor="var(--mf-orange)"
              checked={production}
              onChange={setProduction}
            />
            <ToggleRow
              label={t("di.environment")}
              icon={<Leaf aria-hidden />}
              iconColor="var(--mf-green)"
              checked={environment}
              onChange={setEnvironment}
            />
            <ToggleRow
              label={t("di.quality")}
              icon={<BadgeCheck aria-hidden />}
              iconColor="var(--mf-tint)"
              checked={quality}
              onChange={setQuality}
            />
          </Section>

          {problems.length > 0 ? (
            <div ref={problemsRef}>
              <Banner tone="danger" alert title={t("di.problemsTitle")}>
                <ul style={{ margin: 0, paddingInlineStart: 18 }}>
                  {problems.map((p) => (
                    <li key={p}>{p}</li>
                  ))}
                </ul>
              </Banner>
            </div>
          ) : null}
          {submitError ? <ErrorBanner error={submitError} /> : null}
          <Actions>
            <Button type="submit" variant="attention" disabled={busy}>
              {busy ? t("di.submitting") : t("di.send")}
            </Button>
          </Actions>
        </form>
      ) : null}
      <EquipmentPicker
        open={pickingEquipment}
        onClose={() => setPickingEquipment(false)}
        onPick={(asset) => {
          setEquipment(asset);
          setPickingEquipment(false);
        }}
      />
      {savedCode ? (
        <Toast onDone={clearSaved}>
          <CheckCircle2 size={24} color="var(--mf-green)" aria-hidden />
          {t("di.success", { code: savedCode })}
        </Toast>
      ) : null}
    </Screen>
  );
}
