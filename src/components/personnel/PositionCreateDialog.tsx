import { Plus, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { ReferenceCombobox } from "@/components/reference/ReferenceCombobox";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import { upsertPosition } from "@/services/personnel-service";
import { listCertificationTypes } from "@/services/qualification-service";
import type { CertificationType, Position, PositionUpsertInput } from "@shared/ipc-types";

interface PositionCreateDialogProps {
  open: boolean;
  onClose: () => void;
  onCreated: (position: Position) => void;
}

type SkillEntry = { key: string; refValueIdStr: string | null };
type CertEntry = { key: string; certTypeId: number | null };

let keyCounter = 0;
function nextKey() {
  return `k${++keyCounter}`;
}

export function PositionCreateDialog({ open, onClose, onCreated }: PositionCreateDialogProps) {
  const { t } = useTranslation("personnel");

  const [code, setCode] = useState("");
  const [name, setName] = useState("");
  const [category, setCategory] = useState("technician");
  const [isActive, setIsActive] = useState(true);
  const [skills, setSkills] = useState<SkillEntry[]>([]);
  const [certs, setCerts] = useState<CertEntry[]>([]);
  const [certTypes, setCertTypes] = useState<CertificationType[]>([]);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadCertTypes = useCallback(async () => {
    try {
      const list = await listCertificationTypes();
      setCertTypes(list);
    } catch {
      setCertTypes([]);
    }
  }, []);

  useEffect(() => {
    if (!open) return;
    setCode("");
    setName("");
    setCategory("technician");
    setIsActive(true);
    setSkills([]);
    setCerts([]);
    setError(null);
    void loadCertTypes();
  }, [open, loadCertTypes]);

  const addSkill = () => {
    setSkills((prev) => [...prev, { key: nextKey(), refValueIdStr: null }]);
  };

  const removeSkill = (key: string) => {
    setSkills((prev) => prev.filter((s) => s.key !== key));
  };

  const updateSkillRef = (key: string, val: string | null) => {
    setSkills((prev) => prev.map((s) => (s.key === key ? { ...s, refValueIdStr: val } : s)));
  };

  const addCert = () => {
    setCerts((prev) => [...prev, { key: nextKey(), certTypeId: null }]);
  };

  const removeCert = (key: string) => {
    setCerts((prev) => prev.filter((c) => c.key !== key));
  };

  const updateCertType = (key: string, val: string) => {
    setCerts((prev) =>
      prev.map((c) => (c.key === key ? { ...c, certTypeId: val ? Number(val) : null } : c)),
    );
  };

  const handleSubmit = async () => {
    setError(null);
    const codeTrimmed = code.trim();
    const nameTrimmed = name.trim();
    if (!codeTrimmed) {
      setError(t("position.validation.codeRequired", "Position code is required"));
      return;
    }
    if (!nameTrimmed) {
      setError(t("position.validation.nameRequired", "Position name is required"));
      return;
    }

    const skillIds = skills
      .map((s) => (s.refValueIdStr ? Number(s.refValueIdStr) : null))
      .filter((id): id is number => id !== null && !isNaN(id));

    const certIds = certs.map((c) => c.certTypeId).filter((id): id is number => id !== null);

    const input: PositionUpsertInput = {
      code: codeTrimmed,
      name: nameTrimmed,
      category,
      is_active: isActive,
      requirement_profile:
        skillIds.length > 0 || certIds.length > 0
          ? {
              profile_name: `${codeTrimmed} — Requirements`,
              skill_reference_value_ids: skillIds,
              certification_type_ids: certIds,
            }
          : null,
    };

    setSaving(true);
    try {
      const created = await upsertPosition(input);
      onCreated(created);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("position.create.title", "New Position")}</DialogTitle>
        </DialogHeader>

        <div className="space-y-4 py-2">
          {error ? <p className="text-sm text-destructive">{error}</p> : null}

          {/* Required fields */}
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label htmlFor="pos-code">{t("position.field.code", "Code")} *</Label>
              <Input
                id="pos-code"
                value={code}
                onChange={(e) => setCode(e.target.value)}
                placeholder="TECH-01"
                aria-invalid={error !== null && !code.trim()}
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="pos-category">{t("position.field.category", "Category")}</Label>
              <Select value={category} onValueChange={setCategory}>
                <SelectTrigger id="pos-category">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {(
                    [
                      "technician",
                      "supervisor",
                      "engineer",
                      "operator",
                      "contractor",
                      "planner",
                      "storekeeper",
                      "hse",
                    ] as const
                  ).map((cat) => (
                    <SelectItem key={cat} value={cat}>
                      {t(`position.category.${cat}`, cat)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="pos-name">{t("position.field.name", "Name")} *</Label>
            <Input
              id="pos-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Senior Maintenance Technician"
              aria-invalid={error !== null && !name.trim()}
            />
          </div>

          <div className="flex items-center gap-2">
            <input
              type="checkbox"
              id="pos-active"
              checked={isActive}
              onChange={(e) => setIsActive(e.target.checked)}
              className="h-4 w-4 rounded border"
            />
            <Label htmlFor="pos-active">{t("position.field.active", "Active")}</Label>
          </div>

          {/* Skills requirement profile */}
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <p className="text-xs font-semibold uppercase tracking-wide text-text-secondary">
                {t("position.section.skills", "Required Skills")}
              </p>
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="h-7 gap-1 text-xs"
                onClick={addSkill}
              >
                <Plus className="h-3.5 w-3.5" />
                {t("position.action.addSkill", "Add skill")}
              </Button>
            </div>
            {skills.length === 0 ? (
              <p className="text-xs text-text-muted">
                {t("position.skills.empty", "No required skills defined.")}
              </p>
            ) : (
              <div className="space-y-2">
                {skills.map((skill) => (
                  <div key={skill.key} className="flex items-center gap-2">
                    <div className="flex-1">
                      <ReferenceCombobox
                        referenceType="personnel.skills"
                        valueMode="id"
                        value={skill.refValueIdStr}
                        onChange={(val) => updateSkillRef(skill.key, val)}
                        placeholder={t("position.skills.placeholder", "Select skill…")}
                      />
                    </div>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 shrink-0 text-destructive hover:text-destructive"
                      onClick={() => removeSkill(skill.key)}
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                ))}
              </div>
            )}
          </div>

          {/* Certifications requirement profile */}
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <p className="text-xs font-semibold uppercase tracking-wide text-text-secondary">
                {t("position.section.certs", "Required Certifications")}
              </p>
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="h-7 gap-1 text-xs"
                onClick={addCert}
              >
                <Plus className="h-3.5 w-3.5" />
                {t("position.action.addCert", "Add certification")}
              </Button>
            </div>
            {certs.length === 0 ? (
              <p className="text-xs text-text-muted">
                {t("position.certs.empty", "No required certifications defined.")}
              </p>
            ) : (
              <div className="space-y-2">
                {certs.map((cert) => (
                  <div key={cert.key} className="flex items-center gap-2">
                    <select
                      className={cn(
                        "h-9 flex-1 rounded-md border border-input bg-background px-3 py-1 text-sm shadow-sm",
                        "focus:outline-none focus:ring-1 focus:ring-ring",
                      )}
                      value={cert.certTypeId ?? ""}
                      onChange={(e) => updateCertType(cert.key, e.target.value)}
                    >
                      <option value="">
                        {t("position.certs.placeholder", "Select certification…")}
                      </option>
                      {certTypes.map((ct) => (
                        <option key={ct.id} value={ct.id}>
                          {ct.code} — {ct.name}
                        </option>
                      ))}
                    </select>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 shrink-0 text-destructive hover:text-destructive"
                      onClick={() => removeCert(cert.key)}
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>

        <DialogFooter className="gap-2 sm:gap-0">
          <Button type="button" variant="outline" onClick={onClose} disabled={saving}>
            {t("create.cancel")}
          </Button>
          <Button type="button" onClick={() => void handleSubmit()} disabled={saving}>
            {t("position.create.submit", "Create position")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
