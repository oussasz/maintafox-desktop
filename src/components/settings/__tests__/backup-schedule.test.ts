import { describe, expect, it } from "vitest";

import fr from "@/i18n/locale-data/fr/settings.json";

import {
  BACKUP_SCHEDULE_PRESETS,
  backupSchedulePreset,
  describeBackupSchedule,
} from "../backup-schedule";

const t = (key: string, opts?: Record<string, unknown>) => {
  const leaf = key.split(".").reduce<unknown>((node, part) => {
    return node && typeof node === "object" ? (node as Record<string, unknown>)[part] : undefined;
  }, fr);
  const text = typeof leaf === "string" ? leaf : key;
  return text.replace(/\{\{(\w+)\}\}/g, (_, name: string) => String(opts?.[name] ?? ""));
};

describe("backup schedule wording", () => {
  it("names every preset in French, never the expression", () => {
    for (const preset of BACKUP_SCHEDULE_PRESETS) {
      const text = describeBackupSchedule(preset.cron, t);
      expect(text).not.toContain("*");
      expect(text).not.toContain("policy.schedule");
    }
    expect(describeBackupSchedule("0 2 * * *", t)).toBe("Chaque nuit à 02:00");
  });

  it("recognises a preset written with extra spaces and keeps any other expression as custom", () => {
    expect(backupSchedulePreset(" 0  2 * * * ")).toBe("daily2");
    expect(backupSchedulePreset("30 3 * * *")).toBeNull();
    expect(describeBackupSchedule("30 3 * * *", t)).toBe("Personnalisée (30 3 * * *)");
    expect(describeBackupSchedule("", t)).toBe("Aucune sauvegarde automatique");
  });
});
