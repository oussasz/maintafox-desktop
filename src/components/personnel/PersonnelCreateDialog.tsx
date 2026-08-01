import { Plus, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { EntityFormImageUploader } from "@/components/entity-form/EntityFormImageUploader";
import type { EntityFormImageItem } from "@/components/entity-form/EntityFormImageUploader";
import { PersonnelPickerCombobox } from "@/components/personnel/PersonnelCard";
import { PositionCombobox } from "@/components/personnel/PositionCombobox";
import { ReferenceCombobox } from "@/components/reference/ReferenceCombobox";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
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
import { Textarea } from "@/components/ui/textarea";
import { usePermissions } from "@/hooks/use-permissions";
import { getOrgDesignerSnapshot } from "@/services/org-designer-service";
import {
  listExternalCompanies,
  listPersonnel,
  getPositionRequirementSeed,
  uploadPersonnelPhoto,
} from "@/services/personnel-service";
import { listCertificationTypes } from "@/services/qualification-service";
import { usePersonnelStore } from "@/stores/personnel-store";
import type {
  CertificationType,
  OrgDesignerNodeRow,
  Personnel,
  PersonnelCreateInput,
  PersonnelSkillDraft,
  PersonnelCertDraft,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

const EMPLOYMENT_TYPES = ["employee", "contractor", "temp", "vendor"] as const;
const EMPLOYMENT_ORIGINS = ["internal", "external"] as const;
const EMPLOYMENT_STATUSES = ["active", "inactive", "suspended", "terminated"] as const;

type SkillRow = {
  key: string;
  refValueIdStr: string | null;
  proficiency: number;
  sourceType: "position_seed" | "manual";
};
type CertRow = { key: string; certTypeId: number | null };

let rowKey = 0;
function nextKey() {
  return `r${++rowKey}`;
}

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <p className="mt-4 border-b pb-1 text-[11px] font-semibold uppercase tracking-widest text-text-secondary">
      {children}
    </p>
  );
}

export function PersonnelCreateDialog() {
  const { t } = useTranslation("personnel");
  const { can } = usePermissions();
  const showCreateForm = usePersonnelStore((s) => s.showCreateForm);
  const saving = usePersonnelStore((s) => s.saving);
  const submitNewPersonnel = usePersonnelStore((s) => s.submitNewPersonnel);
  const closeCreateForm = usePersonnelStore((s) => s.closeCreateForm);

  // ── Identity ──────────────────────────────────────────────────────────────
  const [fullName, setFullName] = useState("");
  const [employeeCode, setEmployeeCode] = useState("");
  const [employmentType, setEmploymentType] = useState<string>("employee");
  const [positionId, setPositionId] = useState<number | null>(null);

  // ── Origin & Contract ─────────────────────────────────────────────────────
  const [employmentOrigin, setEmploymentOrigin] = useState<string>("internal");
  const [externalCompanyId, setExternalCompanyId] = useState<number | null>(null);
  const [contractNumber, setContractNumber] = useState("");
  const [contractStart, setContractStart] = useState("");
  const [contractEnd, setContractEnd] = useState("");

  // ── Org Assignment ────────────────────────────────────────────────────────
  const [entityId, setEntityId] = useState<number | null>(null);
  const [teamId, setTeamId] = useState<number | null>(null);
  const [homeScheduleRefId, setHomeScheduleRefId] = useState<string | null>(null);
  const [supervisorId, setSupervisorId] = useState<number | null>(null);
  const [employmentStatus, setEmploymentStatus] = useState<string>("active");
  const [hireDate, setHireDate] = useState("");

  // ── Photo ─────────────────────────────────────────────────────────────────
  const [photoItems, setPhotoItems] = useState<EntityFormImageItem[]>([]);

  // ── Skills ────────────────────────────────────────────────────────────────
  const [skills, setSkills] = useState<SkillRow[]>([]);

  // ── Certifications ────────────────────────────────────────────────────────
  const [certs, setCerts] = useState<CertRow[]>([]);

  // ── Contact ───────────────────────────────────────────────────────────────
  const [email, setEmail] = useState("");
  const [phone, setPhone] = useState("");

  // ── Access ────────────────────────────────────────────────────────────────
  const [createAccount, setCreateAccount] = useState(false);
  const [pendingPersonnelId, setPendingPersonnelId] = useState<number | null>(null);

  // ── Notes ─────────────────────────────────────────────────────────────────
  const [notes, setNotes] = useState("");

  // ── Lookups ───────────────────────────────────────────────────────────────
  const [entityNodes, setEntityNodes] = useState<OrgDesignerNodeRow[]>([]);
  const [teamNodes, setTeamNodes] = useState<OrgDesignerNodeRow[]>([]);
  const [companies, setCompanies] = useState<{ id: number; name: string }[]>([]);
  const [personnelList, setPersonnelList] = useState<Personnel[]>([]);
  const [certTypes, setCertTypes] = useState<CertificationType[]>([]);
  const [localError, setLocalError] = useState<string | null>(null);
  const [photoUploading, setPhotoUploading] = useState(false);

  // CreateUserDialog state — opened after create if checkbox checked
  const [showCreateUser, setShowCreateUser] = useState(false);

  const isExternal = employmentOrigin === "external";
  const canCreateUsers = can(P.ADM_USERS);

  const loadLookups = useCallback(async () => {
    try {
      const [snap, ext, personnel, certList] = await Promise.all([
        getOrgDesignerSnapshot(),
        listExternalCompanies({}),
        listPersonnel({ limit: 2000, offset: 0 }),
        listCertificationTypes(),
      ]);
      const nodes = snap.nodes.filter((n) => n.status === "active");
      const entityCandidates = nodes.filter((n) => n.active_binding_count > 0);
      setEntityNodes(
        entityCandidates.length > 0 ? entityCandidates : nodes.filter((n) => n.can_own_work),
      );
      setTeamNodes(nodes);
      setCompanies(ext.filter((c) => c.is_active !== 0).map((c) => ({ id: c.id, name: c.name })));
      setPersonnelList(personnel.items);
      setCertTypes(certList);
    } catch {
      // partial failures are OK — just leave empty lists
    }
  }, []);

  const resetForm = useCallback(() => {
    setFullName("");
    setEmployeeCode("");
    setEmploymentType("employee");
    setPositionId(null);
    setEmploymentOrigin("internal");
    setExternalCompanyId(null);
    setContractNumber("");
    setContractStart("");
    setContractEnd("");
    setEntityId(null);
    setTeamId(null);
    setHomeScheduleRefId(null);
    setSupervisorId(null);
    setEmploymentStatus("active");
    setHireDate("");
    setPhotoItems([]);
    setSkills([]);
    setCerts([]);
    setEmail("");
    setPhone("");
    setCreateAccount(false);
    setNotes("");
    setLocalError(null);
  }, []);

  useEffect(() => {
    if (!showCreateForm) return;
    resetForm();
    void loadLookups();
  }, [showCreateForm, resetForm, loadLookups]);

  // Seed skills/certs when position changes
  const handlePositionChange = useCallback(async (id: number | null) => {
    setPositionId(id);
    if (!id) {
      setSkills([]);
      setCerts([]);
      return;
    }
    try {
      const seed = await getPositionRequirementSeed(id);
      setSkills(
        seed.skill_reference_value_ids.map((refId) => ({
          key: nextKey(),
          refValueIdStr: String(refId),
          proficiency: 1,
          sourceType: "position_seed" as const,
        })),
      );
      setCerts(
        seed.certification_type_ids.map((certId) => ({
          key: nextKey(),
          certTypeId: certId,
        })),
      );
    } catch {
      // Silently ignore — user can add manually
    }
  }, []);

  // ── Photo picker ──────────────────────────────────────────────────────────
  const pickSinglePhoto = useCallback(async (): Promise<EntityFormImageItem[]> => {
    if (photoItems.length >= 1) return [];
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        multiple: false,
        filters: [{ name: "Photo", extensions: ["png", "jpg", "jpeg", "webp"] }],
      });
      if (!selected) return [];
      const path = typeof selected === "string" ? selected : (selected as { path?: string }).path;
      if (!path) return [];
      const name = path.replace(/\\/g, "/").split("/").pop() ?? "photo";
      return [{ id: `photo-${Date.now()}`, name, path, isPrimary: true }];
    } catch {
      return [];
    }
  }, [photoItems]);

  const handlePhotoChange = (items: EntityFormImageItem[]) => {
    setPhotoItems(items.slice(0, 1));
  };

  // ── Skills editor ─────────────────────────────────────────────────────────
  const addSkill = () =>
    setSkills((p) => [
      ...p,
      { key: nextKey(), refValueIdStr: null, proficiency: 1, sourceType: "manual" },
    ]);
  const removeSkill = (key: string) => setSkills((p) => p.filter((s) => s.key !== key));
  const updateSkillRef = (key: string, val: string | null) =>
    setSkills((p) =>
      p.map((s) =>
        s.key === key ? { ...s, refValueIdStr: val, sourceType: "manual" as const } : s,
      ),
    );
  const updateSkillProf = (key: string, val: string) =>
    setSkills((p) =>
      p.map((s) =>
        s.key === key ? { ...s, proficiency: Math.max(1, Math.min(5, Number(val))) } : s,
      ),
    );

  // ── Certs editor ──────────────────────────────────────────────────────────
  const addCert = () => setCerts((p) => [...p, { key: nextKey(), certTypeId: null }]);
  const removeCert = (key: string) => setCerts((p) => p.filter((c) => c.key !== key));
  const updateCertType = (key: string, val: string) =>
    setCerts((p) =>
      p.map((c) => (c.key === key ? { ...c, certTypeId: val ? Number(val) : null } : c)),
    );

  // ── Submit ────────────────────────────────────────────────────────────────
  const handleSubmit = async () => {
    setLocalError(null);
    const name = fullName.trim();
    const code = employeeCode.trim();

    const errors: string[] = [];
    if (!name) errors.push(t("validation.nameRequired"));
    if (!code) errors.push(t("validation.codeRequired", "Employee code is required"));
    if (!positionId) errors.push(t("validation.positionRequired", "Position is required"));
    if (!entityId) errors.push(t("validation.entityRequired", "Entity is required"));
    if (!teamId) errors.push(t("validation.teamRequired", "Team is required"));
    if (!homeScheduleRefId) errors.push(t("validation.scheduleRequired", "Schedule is required"));
    if (isExternal && !externalCompanyId)
      errors.push(t("validation.companyRequired", "External company is required"));

    if (errors.length > 0) {
      setLocalError(errors.join(" · "));
      return;
    }

    // Narrow required IDs for TypeScript after the validation gate above.
    if (positionId === null || entityId === null || teamId === null || !homeScheduleRefId) {
      return;
    }

    const skillDrafts: PersonnelSkillDraft[] = skills
      .filter((s) => s.refValueIdStr)
      .map((s) => ({
        reference_value_id: Number(s.refValueIdStr),
        proficiency_level: s.proficiency,
        source_type: s.sourceType,
      }));

    const certDrafts: PersonnelCertDraft[] = certs
      .filter((c): c is typeof c & { certTypeId: number } => c.certTypeId !== null)
      .map((c) => ({ certification_type_id: c.certTypeId }));

    const input: PersonnelCreateInput = {
      full_name: name,
      employee_code: code,
      employment_type: employmentType,
      employment_origin: employmentOrigin,
      employment_status: employmentStatus,
      position_id: positionId,
      primary_entity_id: entityId,
      primary_team_id: teamId,
      home_schedule_reference_value_id: Number(homeScheduleRefId),
      supervisor_id: supervisorId ?? null,
      hire_date: hireDate || null,
      email: email.trim() || null,
      phone: phone.trim() || null,
      external_company_id: isExternal ? (externalCompanyId ?? null) : null,
      contract_number: isExternal && contractNumber.trim() ? contractNumber.trim() : null,
      contract_start_date: isExternal && contractStart ? contractStart : null,
      contract_end_date: isExternal && contractEnd ? contractEnd : null,
      notes: notes.trim() || null,
      ...(skillDrafts.length > 0 ? { skills: skillDrafts } : {}),
      ...(certDrafts.length > 0 ? { certifications: certDrafts } : {}),
    };

    try {
      const created = await submitNewPersonnel(input);

      // Upload photo if staged
      const stagedPhoto = photoItems[0];
      if (stagedPhoto?.path) {
        setPhotoUploading(true);
        try {
          await uploadPersonnelPhoto(created.id, stagedPhoto.path);
        } catch (photoErr) {
          // Non-fatal — show warning but don't block
          setLocalError(
            t("create.photoUploadWarning", "Personnel created. Photo upload failed: {{msg}}", {
              msg: photoErr instanceof Error ? photoErr.message : String(photoErr),
            }),
          );
        } finally {
          setPhotoUploading(false);
        }
      }

      // Open CreateUserDialog if requested
      if (createAccount && canCreateUsers) {
        setPendingPersonnelId(created.id);
        setShowCreateUser(true);
      }
    } catch {
      /* store holds error */
    }
  };

  // Lazy-load CreateUserDialog to avoid circular bundle concerns
  const [CreateUserDialog, setCreateUserDialogComponent] = useState<React.ComponentType<{
    open: boolean;
    onClose: () => void;
    onCreated: () => void;
    initialPersonnelId?: number | null;
    lockPersonnel?: boolean;
  }> | null>(null);

  useEffect(() => {
    if (showCreateUser && !CreateUserDialog) {
      void import("@/components/admin/CreateUserDialog").then((mod) => {
        setCreateUserDialogComponent(() => mod.CreateUserDialog);
      });
    }
  }, [showCreateUser, CreateUserDialog]);

  return (
    <>
      <Dialog open={showCreateForm} onOpenChange={(open) => !open && closeCreateForm()}>
        <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>{t("create.title")}</DialogTitle>
          </DialogHeader>

          <div className="space-y-3 py-2">
            {localError ? (
              <p className="rounded border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive">
                {localError}
              </p>
            ) : null}

            {/* ── Identity ───────────────────────────────────────────── */}
            <SectionLabel>{t("create.section.identity", "Identity")}</SectionLabel>

            <div className="grid gap-3 sm:grid-cols-2">
              <div className="space-y-1.5">
                <Label htmlFor="per-full-name">{t("field.fullName")} *</Label>
                <Input
                  id="per-full-name"
                  value={fullName}
                  onChange={(e) => setFullName(e.target.value)}
                  autoComplete="name"
                />
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="per-emp-code">{t("field.employeeCode")} *</Label>
                <Input
                  id="per-emp-code"
                  value={employeeCode}
                  onChange={(e) => setEmployeeCode(e.target.value)}
                  placeholder="EMP-001"
                  autoComplete="off"
                />
              </div>
            </div>

            <div className="grid gap-3 sm:grid-cols-2">
              <div className="space-y-1.5">
                <Label>{t("field.employmentType")} *</Label>
                <Select value={employmentType} onValueChange={setEmploymentType}>
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {EMPLOYMENT_TYPES.map((et) => (
                      <SelectItem key={et} value={et}>
                        {t(`employmentType.${et}`)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-1.5">
                <Label>{t("field.position")} *</Label>
                <PositionCombobox
                  value={positionId}
                  onChange={(id) => void handlePositionChange(id)}
                />
              </div>
            </div>

            {/* ── Origin & Contract ───────────────────────────────────── */}
            <SectionLabel>{t("create.section.origin", "Origin & Contract")}</SectionLabel>

            <div className="space-y-1.5">
              <Label>{t("field.employmentOrigin", "Employment origin")} *</Label>
              <div className="flex gap-4">
                {EMPLOYMENT_ORIGINS.map((origin) => (
                  <label key={origin} className="flex cursor-pointer items-center gap-2 text-sm">
                    <input
                      type="radio"
                      name="employment-origin"
                      value={origin}
                      checked={employmentOrigin === origin}
                      onChange={() => setEmploymentOrigin(origin)}
                      className="h-4 w-4"
                    />
                    {t(
                      `employmentOrigin.${origin}`,
                      origin === "internal" ? "Internal" : "External",
                    )}
                  </label>
                ))}
              </div>
            </div>

            {isExternal ? (
              <div className="space-y-3 rounded-md border border-amber-400/40 bg-amber-50/40 p-3">
                <div className="space-y-1.5">
                  <Label>{t("field.company")} *</Label>
                  <select
                    className="h-9 w-full rounded-md border border-input bg-background px-3 py-1 text-sm shadow-sm"
                    value={externalCompanyId ?? ""}
                    onChange={(e) =>
                      setExternalCompanyId(e.target.value ? Number(e.target.value) : null)
                    }
                  >
                    <option value="">—</option>
                    {companies.map((c) => (
                      <option key={c.id} value={c.id}>
                        {c.name}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="grid gap-3 sm:grid-cols-3">
                  <div className="space-y-1.5">
                    <Label htmlFor="per-contract-no">
                      {t("field.contractNumber", "Contract No.")}
                    </Label>
                    <Input
                      id="per-contract-no"
                      value={contractNumber}
                      onChange={(e) => setContractNumber(e.target.value)}
                    />
                  </div>
                  <div className="space-y-1.5">
                    <Label htmlFor="per-contract-start">
                      {t("field.contractStart", "Contract start")}
                    </Label>
                    <Input
                      id="per-contract-start"
                      type="date"
                      value={contractStart}
                      onChange={(e) => setContractStart(e.target.value)}
                    />
                  </div>
                  <div className="space-y-1.5">
                    <Label htmlFor="per-contract-end">
                      {t("field.contractEnd", "Contract end")}
                    </Label>
                    <Input
                      id="per-contract-end"
                      type="date"
                      value={contractEnd}
                      onChange={(e) => setContractEnd(e.target.value)}
                    />
                  </div>
                </div>
              </div>
            ) : null}

            {/* ── Organizational Assignment ───────────────────────────── */}
            <SectionLabel>
              {t("create.section.assignment", "Organizational Assignment")}
            </SectionLabel>

            <div className="grid gap-3 sm:grid-cols-2">
              <div className="space-y-1.5">
                <Label>{t("field.entity")} *</Label>
                <select
                  className="h-9 w-full rounded-md border border-input bg-background px-3 py-1 text-sm shadow-sm"
                  value={entityId ?? ""}
                  onChange={(e) => setEntityId(e.target.value ? Number(e.target.value) : null)}
                >
                  <option value="">—</option>
                  {entityNodes.map((n) => (
                    <option key={n.node_id} value={n.node_id}>
                      {n.code} — {n.name}
                    </option>
                  ))}
                </select>
              </div>
              <div className="space-y-1.5">
                <Label>{t("field.team")} *</Label>
                <select
                  className="h-9 w-full rounded-md border border-input bg-background px-3 py-1 text-sm shadow-sm"
                  value={teamId ?? ""}
                  onChange={(e) => setTeamId(e.target.value ? Number(e.target.value) : null)}
                >
                  <option value="">—</option>
                  {teamNodes.map((n) => (
                    <option key={n.node_id} value={n.node_id}>
                      {n.code} — {n.name}
                    </option>
                  ))}
                </select>
              </div>
            </div>

            <div className="grid gap-3 sm:grid-cols-2">
              <div className="space-y-1.5">
                <Label>{t("field.schedule")} *</Label>
                <ReferenceCombobox
                  id="per-home-schedule"
                  referenceType="org.schedule_class"
                  valueMode="id"
                  value={homeScheduleRefId}
                  onChange={setHomeScheduleRefId}
                  placeholder={t("field.schedule")}
                  allowClear
                />
              </div>
              <div className="space-y-1.5">
                <Label>{t("field.supervisor")}</Label>
                <PersonnelPickerCombobox
                  items={personnelList}
                  value={supervisorId}
                  onChange={setSupervisorId}
                  placeholder={t("create.supervisor.placeholder", "Search supervisor…")}
                />
              </div>
            </div>

            <div className="grid gap-3 sm:grid-cols-2">
              <div className="space-y-1.5">
                <Label>{t("field.employmentStatus", "Employment status")} *</Label>
                <Select value={employmentStatus} onValueChange={setEmploymentStatus}>
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {EMPLOYMENT_STATUSES.map((s) => (
                      <SelectItem key={s} value={s}>
                        {t(`employmentStatus.${s}`, s)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="per-hire-date">{t("field.hireDate")}</Label>
                <Input
                  id="per-hire-date"
                  type="date"
                  value={hireDate}
                  onChange={(e) => setHireDate(e.target.value)}
                />
              </div>
            </div>

            {/* ── Photo ───────────────────────────────────────────────── */}
            <SectionLabel>{t("create.section.photo", "Photo")}</SectionLabel>
            <EntityFormImageUploader
              items={photoItems}
              onChange={handlePhotoChange}
              onPickFiles={pickSinglePhoto}
              uploading={photoUploading}
              maxItems={1}
              emptyLabel={t("create.photo.empty", "No photo yet")}
              hintLabel={t("create.photo.hint", "Click to select a profile photo (PNG, JPG, WEBP)")}
              addLabel={t("create.photo.add", "Add photo")}
              primaryLabel={t("create.photo.primary", "Primary")}
              removeLabel={t("create.photo.remove", "Remove")}
            />

            {/* ── Skills ──────────────────────────────────────────────── */}
            <div className="flex items-center justify-between">
              <SectionLabel>{t("create.section.skills", "Skills")}</SectionLabel>
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="mt-4 h-7 gap-1 text-xs"
                onClick={addSkill}
              >
                <Plus className="h-3.5 w-3.5" />
                {t("create.skill.add", "Add skill")}
              </Button>
            </div>
            {skills.length === 0 ? (
              <p className="text-xs text-text-muted">
                {t(
                  "create.skill.empty",
                  "No skills added. Selecting a position will pre-fill from its requirement profile.",
                )}
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
                        placeholder={t("create.skill.placeholder", "Select skill…")}
                      />
                    </div>
                    <select
                      className="h-9 w-20 rounded-md border border-input bg-background px-2 text-sm shadow-sm"
                      value={skill.proficiency}
                      onChange={(e) => updateSkillProf(skill.key, e.target.value)}
                      aria-label={t("create.skill.proficiency", "Proficiency")}
                    >
                      {[1, 2, 3, 4, 5].map((lvl) => (
                        <option key={lvl} value={lvl}>
                          {lvl}
                        </option>
                      ))}
                    </select>
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

            {/* ── Certifications ───────────────────────────────────────── */}
            <div className="flex items-center justify-between">
              <SectionLabel>{t("create.section.certs", "Certifications")}</SectionLabel>
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="mt-4 h-7 gap-1 text-xs"
                onClick={addCert}
              >
                <Plus className="h-3.5 w-3.5" />
                {t("create.cert.add", "Add certification")}
              </Button>
            </div>
            {certs.length === 0 ? (
              <p className="text-xs text-text-muted">
                {t("create.cert.empty", "No certifications added.")}
              </p>
            ) : (
              <div className="space-y-2">
                {certs.map((cert) => (
                  <div key={cert.key} className="flex items-center gap-2">
                    <select
                      className="h-9 flex-1 rounded-md border border-input bg-background px-3 py-1 text-sm shadow-sm"
                      value={cert.certTypeId ?? ""}
                      onChange={(e) => updateCertType(cert.key, e.target.value)}
                    >
                      <option value="">
                        {t("create.cert.placeholder", "Select certification…")}
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

            {/* ── Contact ─────────────────────────────────────────────── */}
            <SectionLabel>{t("create.section.contact", "Contact")}</SectionLabel>
            <div className="grid gap-3 sm:grid-cols-2">
              <div className="space-y-1.5">
                <Label htmlFor="per-email">{t("field.email")}</Label>
                <Input
                  id="per-email"
                  type="email"
                  value={email}
                  onChange={(e) => setEmail(e.target.value)}
                  autoComplete="email"
                />
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="per-phone">{t("field.phone")}</Label>
                <Input
                  id="per-phone"
                  type="tel"
                  value={phone}
                  onChange={(e) => setPhone(e.target.value)}
                  autoComplete="tel"
                />
              </div>
            </div>

            {/* ── MaintaFox Account ────────────────────────────────────── */}
            {canCreateUsers ? (
              <>
                <SectionLabel>{t("create.section.access", "Platform Access")}</SectionLabel>
                <div className="flex items-start gap-2.5">
                  <Checkbox
                    id="per-create-account"
                    checked={createAccount}
                    onCheckedChange={(checked) => setCreateAccount(checked === true)}
                    className="mt-0.5"
                  />
                  <div>
                    <label
                      htmlFor="per-create-account"
                      className="cursor-pointer text-sm font-medium"
                    >
                      {t("create.account.label", "Create MaintaFox account")}
                    </label>
                    <p className="mt-0.5 text-xs text-text-secondary">
                      {t("create.account.hint", "Opens the user creation dialog after saving.")}
                    </p>
                  </div>
                </div>
              </>
            ) : null}

            {/* ── Notes ───────────────────────────────────────────────── */}
            <SectionLabel>{t("create.section.notes", "Notes")}</SectionLabel>
            <Textarea
              id="per-notes"
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
              rows={3}
              placeholder={t("create.notes.placeholder", "Additional notes…")}
            />
          </div>

          <DialogFooter className="gap-2 sm:gap-0">
            <Button type="button" variant="outline" onClick={closeCreateForm} disabled={saving}>
              {t("create.cancel")}
            </Button>
            <Button
              type="button"
              onClick={() => void handleSubmit()}
              disabled={saving || photoUploading}
            >
              {saving ? t("common.loading") : t("create.submit")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* CreateUserDialog — opened after personnel is created if checkbox was checked */}
      {CreateUserDialog && showCreateUser ? (
        <CreateUserDialog
          open={showCreateUser}
          onClose={() => {
            setShowCreateUser(false);
            setPendingPersonnelId(null);
          }}
          onCreated={() => {
            setShowCreateUser(false);
            setPendingPersonnelId(null);
          }}
          initialPersonnelId={pendingPersonnelId}
          lockPersonnel
        />
      ) : null}
    </>
  );
}
