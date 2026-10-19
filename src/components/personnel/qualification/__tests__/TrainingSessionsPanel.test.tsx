import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enTraining from "@/i18n/locale-data/en/training.json";
import frTraining from "@/i18n/locale-data/fr/training.json";

function lookup(key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      enTraining,
    );
  return typeof value === "string" ? value : undefined;
}

const t = (key: string, opts?: Record<string, unknown>) =>
  (
    lookup(key) ?? (typeof opts?.["defaultValue"] === "string" ? opts["defaultValue"] : key)
  ).replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts?.[n] ?? ""));
const i18n = { language: "en" };
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t, i18n }),
}));

vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => true }),
}));

const upsertMock = vi.fn();
vi.mock("@/services/qualification-service", () => ({
  listTrainingSessions: vi.fn().mockResolvedValue([]),
  listTrainingAttendance: vi.fn().mockResolvedValue([]),
  upsertTrainingSession: (...args: unknown[]) => upsertMock(...args),
  upsertTrainingAttendance: vi.fn(),
}));

import { TrainingSessionsPanel } from "../TrainingSessionsPanel";

describe("TrainingSessionsPanel session form", () => {
  beforeEach(() => {
    upsertMock.mockReset();
  });

  it("has FR and EN text for the form section", () => {
    expect(enTraining.sessions.formSection).toBeTruthy();
    expect(frTraining.sessions.formSection).toBeTruthy();
  });

  it("opens the form in the standard shell, save waits for a course, cancel closes", async () => {
    render(<TrainingSessionsPanel people={[]} certTypes={[]} />);
    const add = await screen.findAllByRole("button", { name: enTraining.sessions.add });
    fireEvent.click(add[0] as HTMLElement);

    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    expect(screen.getByText(enTraining.sessions.addTitle)).toBeInTheDocument();
    expect(screen.getByText(enTraining.sessions.formSection)).toBeInTheDocument();
    expect(screen.getByText(enTraining.sessions.formDescription)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: enTraining.action.save })).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: enTraining.action.cancel }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(upsertMock).not.toHaveBeenCalled();
  });
});
