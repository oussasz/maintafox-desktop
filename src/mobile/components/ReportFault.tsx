import { type FormEvent, useEffect, useState } from "react";

import { getReferenceTypeConfig } from "@/components/reference/reference-types";
import { searchAssets } from "@/services/asset-search-service";
import { createDi } from "@/services/di-service";
import { listPublishedReferenceValuesByDomainCode } from "@/services/reference-service";
import { listUrgencyLevels } from "@/services/urgency-scale-service";
import { toErrorMessage } from "@/utils/errors";
import type { AssetSearchResult, DiCreateInput, UrgencyLevel } from "@shared/ipc-types";

import { useMobileT } from "../i18n";

import { ErrorBox } from "./Shell";

const TITLE_MAX = 100;
const DESC_MAX = 1000;
/** Equipment shown per search (the engine ranks them). */
const EQUIPMENT_LIMIT = 20;
const SEARCH_DEBOUNCE_MS = 250;

/** One option of a list read from the engine's reference data. */
interface Option {
  value: string;
  label: string;
}

interface FormLists {
  origin: Option[];
  requestType: Option[];
  symptom: Option[];
  impact: Option[];
  urgency: UrgencyLevel[];
}

type ListsLoad =
  | { kind: "loading" }
  | { kind: "error"; message: string }
  | { kind: "ok"; lists: FormLists };

async function readReferenceOptions(
  type: "di.origin" | "di.request_type" | "di.impact" | "work.symptom",
  valueOf: "code" | "id",
): Promise<Option[]> {
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

function SelectField({
  id,
  label,
  options,
  value,
  onChange,
  chooseLabel,
}: {
  id: string;
  label: string;
  options: Option[];
  value: string;
  onChange: (value: string) => void;
  chooseLabel: string;
}) {
  return (
    <label className="mf-field" htmlFor={id}>
      <span>{label}</span>
      <select id={id} className="mf-input" value={value} onChange={(e) => onChange(e.target.value)}>
        <option value="">{chooseLabel}</option>
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
    </label>
  );
}

/**
 * "Report a fault": a DI (intervention request) created through the engine's `create_di`.
 * Every list comes from the engine (equipment search, reference data, urgency scale); nothing is
 * declared here. The submitter is the signed-in user.
 */
export function ReportFault({ userId }: { userId: number | null }) {
  const { t, language } = useMobileT();
  const [load, setLoad] = useState<ListsLoad>({ kind: "loading" });
  const [reloadToken, setReloadToken] = useState(0);

  const [equipment, setEquipment] = useState<AssetSearchResult | null>(null);
  const [query, setQuery] = useState("");
  const [matches, setMatches] = useState<AssetSearchResult[] | null>(null);
  const [searchError, setSearchError] = useState<string | null>(null);

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
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [savedCode, setSavedCode] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoad({ kind: "loading" });
    readFormLists()
      .then((lists) => {
        if (!cancelled) setLoad({ kind: "ok", lists });
      })
      .catch((err: unknown) => {
        if (!cancelled) setLoad({ kind: "error", message: toErrorMessage(err) });
      });
    return () => {
      cancelled = true;
    };
  }, [reloadToken]);

  useEffect(() => {
    if (equipment) return;
    let cancelled = false;
    const timer = setTimeout(() => {
      searchAssets({ query: query.trim() === "" ? null : query.trim(), limit: EQUIPMENT_LIMIT })
        .then((rows) => {
          if (cancelled) return;
          setMatches(rows);
          setSearchError(null);
        })
        .catch((err: unknown) => {
          if (!cancelled) setSearchError(toErrorMessage(err));
        });
    }, SEARCH_DEBOUNCE_MS);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [query, equipment]);

  function urgencyLabel(level: UrgencyLevel): string {
    return language === "fr" ? level.label_fr : level.label_en;
  }

  function resetForm() {
    setEquipment(null);
    setQuery("");
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
    } catch (err) {
      setSubmitError(toErrorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="mf-card" aria-labelledby="mf-di-title">
      <h2 id="mf-di-title" className="mf-card__title">
        {t("di.title")}
      </h2>
      {load.kind === "loading" ? (
        <p role="status" className="mf-status">
          {t("common.loading")}
        </p>
      ) : null}
      {load.kind === "error" ? (
        <>
          <ErrorBox title={t("di.listsError")} message={load.message} />
          <button type="button" className="mf-btn" onClick={() => setReloadToken((n) => n + 1)}>
            {t("common.retry")}
          </button>
        </>
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
              <p key={key} className="mf-warning" role="alert">
                {t("di.listEmpty", { field: t(key) })}
              </p>
            ))
        : null}
      {load.kind === "ok" ? (
        <form className="mf-form" onSubmit={(e) => void onSubmit(e)} noValidate>
          <div className="mf-field">
            <span>{t("di.equipment")}</span>
            {equipment ? (
              <div className="mf-picked">
                <div>
                  <strong dir="ltr">{equipment.asset_code}</strong> {equipment.asset_name}
                  {equipment.org_node_name ? (
                    <div className="mf-muted">{equipment.org_node_name}</div>
                  ) : null}
                </div>
                <button type="button" className="mf-btn" onClick={() => setEquipment(null)}>
                  {t("di.equipmentChange")}
                </button>
              </div>
            ) : (
              <>
                <input
                  className="mf-input"
                  type="search"
                  value={query}
                  placeholder={t("di.equipmentSearch")}
                  aria-label={t("di.equipmentSearch")}
                  onChange={(e) => setQuery(e.target.value)}
                />
                {searchError ? (
                  <ErrorBox title={t("di.searchError")} message={searchError} />
                ) : null}
                {matches && matches.length === 0 ? <p>{t("di.equipmentNone")}</p> : null}
                <ul className="mf-list" aria-label={t("di.equipment")}>
                  {(matches ?? []).map((a) => (
                    <li key={a.id}>
                      <button
                        type="button"
                        className="mf-list__pick"
                        onClick={() => setEquipment(a)}
                      >
                        <strong dir="ltr">{a.asset_code}</strong> {a.asset_name}
                        {a.org_node_name ? (
                          <span className="mf-muted"> · {a.org_node_name}</span>
                        ) : null}
                      </button>
                    </li>
                  ))}
                </ul>
              </>
            )}
          </div>

          <label className="mf-field" htmlFor="mf-di-subject">
            <span>{t("di.subject")}</span>
            <input
              id="mf-di-subject"
              className="mf-input"
              value={title}
              maxLength={TITLE_MAX}
              onChange={(e) => setTitle(e.target.value)}
            />
          </label>
          <label className="mf-field" htmlFor="mf-di-description">
            <span>{t("di.description")}</span>
            <textarea
              id="mf-di-description"
              className="mf-input mf-input--area"
              rows={4}
              value={description}
              maxLength={DESC_MAX}
              onChange={(e) => setDescription(e.target.value)}
            />
          </label>

          <SelectField
            id="mf-di-origin"
            label={t("di.origin")}
            options={load.lists.origin}
            value={origin}
            onChange={setOrigin}
            chooseLabel={t("di.choose")}
          />
          <SelectField
            id="mf-di-request-type"
            label={t("di.requestType")}
            options={load.lists.requestType}
            value={requestType}
            onChange={setRequestType}
            chooseLabel={t("di.choose")}
          />
          <SelectField
            id="mf-di-symptom"
            label={t("di.symptom")}
            options={load.lists.symptom}
            value={symptom}
            onChange={setSymptom}
            chooseLabel={t("di.choose")}
          />
          <SelectField
            id="mf-di-urgency"
            label={t("di.urgency")}
            options={load.lists.urgency.map((u) => ({ value: u.code, label: urgencyLabel(u) }))}
            value={urgency}
            onChange={setUrgency}
            chooseLabel={t("di.choose")}
          />
          <SelectField
            id="mf-di-impact"
            label={t("di.impact")}
            options={load.lists.impact}
            value={impact}
            onChange={setImpact}
            chooseLabel={t("di.choose")}
          />

          <fieldset className="mf-flags">
            <legend>{t("di.flags")}</legend>
            <label className="mf-check">
              <input
                type="checkbox"
                checked={safety}
                onChange={(e) => setSafety(e.target.checked)}
              />
              <span>{t("di.safety")}</span>
            </label>
            <label className="mf-check">
              <input
                type="checkbox"
                checked={production}
                onChange={(e) => setProduction(e.target.checked)}
              />
              <span>{t("di.production")}</span>
            </label>
            <label className="mf-check">
              <input
                type="checkbox"
                checked={environment}
                onChange={(e) => setEnvironment(e.target.checked)}
              />
              <span>{t("di.environment")}</span>
            </label>
            <label className="mf-check">
              <input
                type="checkbox"
                checked={quality}
                onChange={(e) => setQuality(e.target.checked)}
              />
              <span>{t("di.quality")}</span>
            </label>
          </fieldset>

          {problems.length > 0 ? (
            <div className="mf-error" role="alert">
              <ul>
                {problems.map((p) => (
                  <li key={p}>{p}</li>
                ))}
              </ul>
            </div>
          ) : null}
          {submitError ? <ErrorBox title={t("di.submitError")} message={submitError} /> : null}
          {savedCode ? (
            <p role="status" className="mf-success">
              {t("di.success", { code: savedCode })}
            </p>
          ) : null}
          <button type="submit" className="mf-btn mf-btn--primary" disabled={busy}>
            {busy ? t("di.submitting") : t("di.send")}
          </button>
        </form>
      ) : null}
    </section>
  );
}
