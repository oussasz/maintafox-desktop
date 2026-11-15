// DESIGN MOCKUP (step 1 of the field app, not built yet): executing a work order on the phone.
// Shown only by the design preview page with sample data passed in; the field app does not use it.

import {
  Camera,
  CheckCircle2,
  Clock,
  HardHat,
  ImageIcon,
  MapPin,
  Minus,
  Package,
  Pause,
  Play,
  Plus,
  Wrench,
  Zap,
} from "lucide-react";
import { useState } from "react";

import { useMobileT } from "../i18n";
import {
  Actions,
  BackButton,
  Banner,
  Button,
  MockupFlag,
  type PickerOption,
  PickerRow,
  Pill,
  Row,
  Screen,
  Section,
  Sheet,
  TextAreaCell,
  ToggleRow,
} from "../ui/kit";

export type TaskResult = "ok" | "nok" | "na" | null;

export interface ExecutionSample {
  code: string;
  title: string;
  equipment: string;
  location: string;
  status: string;
  statusColor: string;
  urgency: string;
  urgencyColor: string;
  type: string;
  permit: string;
  isolation: string;
  ppe: string;
  elapsed: string;
  tasks: Array<{ label: string; result: TaskResult }>;
  parts: Array<{ code: string; name: string; qty: number; unit: string }>;
  failureModes: PickerOption[];
  failureCauses: PickerOption[];
  failureActions: PickerOption[];
  failure: { mode: string; cause: string; action: string };
  photos: number;
}

export type ExecutionStage = "ready" | "running" | "finish";

function TaskRow({
  label,
  result,
  onResult,
  disabled,
}: {
  label: string;
  result: TaskResult;
  onResult: (r: TaskResult) => void;
  /** Before the work starts, results cannot be entered. */
  disabled: boolean;
}) {
  const { t } = useMobileT();
  const options: Array<{ value: Exclude<TaskResult, null>; label: string }> = [
    { value: "ok", label: t("exec.taskOk") },
    { value: "nok", label: t("exec.taskNok") },
    { value: "na", label: t("exec.taskNa") },
  ];
  return (
    <div className="mf-cell">
      <span className="mf-row__title">{label}</span>
      <div className="mf-result" role="group" aria-label={label}>
        {options.map((o) => (
          <button
            key={o.value}
            type="button"
            className="mf-result__btn"
            data-tone={o.value}
            aria-pressed={result === o.value}
            disabled={disabled}
            onClick={() => onResult(result === o.value ? null : o.value)}
          >
            {o.label}
          </button>
        ))}
      </div>
    </div>
  );
}

function Stepper({
  value,
  onChange,
  label,
  disabled,
}: {
  value: number;
  onChange: (n: number) => void;
  label: string;
  disabled: boolean;
}) {
  const { t } = useMobileT();
  return (
    <span className="mf-stepper" role="group" aria-label={label}>
      <button
        type="button"
        aria-label={t("exec.less")}
        disabled={disabled || value === 0}
        onClick={() => onChange(Math.max(0, value - 1))}
      >
        <Minus size={18} aria-hidden />
      </button>
      <output>{value}</output>
      <button
        type="button"
        aria-label={t("exec.more")}
        disabled={disabled}
        onClick={() => onChange(value + 1)}
      >
        <Plus size={18} aria-hidden />
      </button>
    </span>
  );
}

export function WorkOrderExecutionMockup({
  data,
  stage,
  onBack,
}: {
  data: ExecutionSample;
  stage: ExecutionStage;
  onBack: () => void;
}) {
  const { t } = useMobileT();
  // Nothing is answered before the work starts.
  const [tasks, setTasks] = useState(
    stage === "ready" ? data.tasks.map((task) => ({ ...task, result: null })) : data.tasks,
  );
  // Parts are counted as they are used: none before the work starts.
  const [parts, setParts] = useState(
    stage === "ready" ? data.parts.map((part) => ({ ...part, qty: 0 })) : data.parts,
  );
  const [safetyRead, setSafetyRead] = useState(stage !== "ready");
  const [mode, setMode] = useState(data.failure.mode);
  const [cause, setCause] = useState(data.failure.cause);
  const [action, setAction] = useState(data.failure.action);
  const [note, setNote] = useState("");
  const [finishing, setFinishing] = useState(stage === "finish");
  const running = stage !== "ready";
  const done = tasks.filter((task) => task.result !== null).length;

  return (
    <Screen
      title={data.code}
      inlineTitle
      leading={<BackButton label={t("wo.back")} onClick={onBack} />}
      trailing={<MockupFlag />}
    >
      <div className="mf-detail-head">
        <h2 className="mf-detail-head__title">{data.title}</h2>
        <div className="mf-detail-head__pills">
          <Pill dotColor={running ? "var(--mf-green)" : data.statusColor}>
            {running ? t("exec.inProgress") : data.status}
          </Pill>
          <Pill dotColor={data.urgencyColor}>{data.urgency}</Pill>
          <Pill>{data.type}</Pill>
        </div>
      </div>

      <Banner tone="safety" icon={<HardHat size={24} aria-hidden />} title={t("wo.safetyTitle")}>
        <ul style={{ margin: 0, paddingInlineStart: 18 }}>
          <li>{data.permit}</li>
          <li>
            <Zap size={14} aria-hidden style={{ verticalAlign: "-2px" }} /> {data.isolation}
          </li>
          <li>{data.ppe}</li>
        </ul>
      </Banner>

      <Section>
        <Row icon={<Wrench aria-hidden />} iconColor="var(--mf-tile-navy)" title={data.equipment} />
        <Row icon={<MapPin aria-hidden />} iconColor="var(--mf-label-3)" title={data.location} />
      </Section>

      {running ? (
        <div className="mf-timer">
          <div>
            <div className="mf-timer__label">{t("exec.elapsed")}</div>
            <div className="mf-timer__value mf-ltr">{data.elapsed}</div>
          </div>
          <Button variant="tinted" style={{ inlineSize: "auto" }}>
            <Pause size={20} aria-hidden />
            {t("exec.pause")}
          </Button>
        </div>
      ) : (
        <>
          <Section footer={t("exec.safetyConfirmHint")}>
            <ToggleRow
              label={t("exec.safetyConfirm")}
              icon={<HardHat aria-hidden />}
              iconColor="#000000"
              checked={safetyRead}
              onChange={setSafetyRead}
            />
          </Section>
          <Actions>
            <Button variant="go" disabled={!safetyRead}>
              <Play size={20} aria-hidden />
              {t("exec.start")}
            </Button>
          </Actions>
        </>
      )}

      <Section
        header={`${t("exec.tasks")} · ${t("exec.taskProgress", { done, total: tasks.length })}`}
      >
        {tasks.map((task, i) => (
          <TaskRow
            key={task.label}
            label={task.label}
            result={task.result}
            disabled={!running}
            onResult={(r) =>
              setTasks((list) => list.map((x, j) => (j === i ? { ...x, result: r } : x)))
            }
          />
        ))}
      </Section>

      <Section header={t("exec.parts")}>
        {parts.map((p, i) => (
          <Row
            key={p.code}
            icon={<Package aria-hidden />}
            iconColor="var(--mf-orange)"
            title={p.name}
            subtitle={
              <span className="mf-ltr">
                {p.code} · {p.unit}
              </span>
            }
            accessory={
              <Stepper
                label={p.name}
                value={p.qty}
                disabled={!running}
                onChange={(n) =>
                  setParts((list) => list.map((x, j) => (j === i ? { ...x, qty: n } : x)))
                }
              />
            }
          />
        ))}
        <Row
          icon={<Plus aria-hidden />}
          title={t("exec.addPart")}
          tone="tint"
          onClick={() => undefined}
        />
      </Section>

      <Section header={t("exec.photos")}>
        <div className="mf-photos" style={{ margin: 12 }}>
          {Array.from({ length: data.photos }, (_, i) => (
            <div key={i} className="mf-photo">
              <ImageIcon size={28} aria-hidden />
            </div>
          ))}
          <button type="button" className="mf-photo mf-photo--add" aria-label={t("exec.addPhoto")}>
            <Camera size={30} aria-hidden />
          </button>
        </div>
      </Section>

      {running ? (
        <Actions>
          <Button variant="attention" onClick={() => setFinishing(true)}>
            <CheckCircle2 size={20} aria-hidden />
            {t("exec.finish")}
          </Button>
        </Actions>
      ) : null}

      <Sheet
        open={finishing}
        title={t("exec.finishTitle")}
        onClose={() => setFinishing(false)}
        closeLabel={t("common.close")}
      >
        <Section header={t("exec.failure")} footer={t("exec.failureHint")}>
          <PickerRow
            label={t("exec.failureMode")}
            options={data.failureModes}
            value={mode}
            onChange={setMode}
            placeholder={t("common.choose")}
          />
          <PickerRow
            label={t("exec.failureCause")}
            options={data.failureCauses}
            value={cause}
            onChange={setCause}
            placeholder={t("common.choose")}
          />
          <PickerRow
            label={t("exec.failureAction")}
            options={data.failureActions}
            value={action}
            onChange={setAction}
            placeholder={t("common.choose")}
          />
        </Section>
        <Section header={t("exec.summary")}>
          <Row
            icon={<Clock aria-hidden />}
            title={t("exec.elapsed")}
            detail={<span className="mf-ltr">{data.elapsed}</span>}
          />
          <Row
            icon={<CheckCircle2 aria-hidden />}
            iconColor="var(--mf-green)"
            title={t("exec.tasks")}
            detail={t("exec.taskProgress", { done, total: tasks.length })}
          />
        </Section>
        <Section>
          <TextAreaCell
            label={t("exec.handover")}
            value={note}
            placeholder={t("exec.handoverPlaceholder")}
            onChange={(e) => setNote(e.target.value)}
          />
        </Section>
        <Actions>
          <Button variant="go">{t("exec.confirmFinish")}</Button>
        </Actions>
      </Sheet>
    </Screen>
  );
}
