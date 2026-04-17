import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

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
import { Textarea } from "@/components/ui/textarea";
import { getOrgDesignerSnapshot } from "@/services/org-designer-service";
import { listExternalCompanies, listPositions } from "@/services/personnel-service";
import { usePersonnelStore } from "@/stores/personnel-store";
import type { OrgDesignerNodeRow, PersonnelCreateInput } from "@shared/ipc-types";

const EMPLOYMENT_TYPES = ["employee", "contractor", "temp", "vendor"] as const;

export function PersonnelCreateDialog() {
  const { t } = useTranslation("personnel");
  const showCreateForm = usePersonnelStore((s) => s.showCreateForm);
  const saving = usePersonnelStore((s) => s.saving);
  const submitNewPersonnel = usePersonnelStore((s) => s.submitNewPersonnel);
  const closeCreateForm = usePersonnelStore((s) => s.closeCreateForm);

  const [fullName, setFullName] = useState("");
  const [employmentType, setEmploymentType] = useState<string>("employee");
  const [positionId, setPositionId] = useState<string>("__none__");
  const [entityId, setEntityId] = useState<string>("__none__");
  const [teamId, setTeamId] = useState<string>("__none__");
  const [email, setEmail] = useState("");
  const [phone, setPhone] = useState("");
  const [notes, setNotes] = useState("");
  const [externalCompanyId, setExternalCompanyId] = useState<string>("__none__");
  const [positions, setPositions] = useState<{ id: number; code: string; name: string }[]>([]);
  const [entityNodes, setEntityNodes] = useState<OrgDesignerNodeRow[]>([]);
  const [teamNodes, setTeamNodes] = useState<OrgDesignerNodeRow[]>([]);
  const [companies, setCompanies] = useState<{ id: number; name: string }[]>([]);
  const [localError, setLocalError] = useState<string | null>(null);

  const loadLookups = useCallback(async () => {
    try {
      const [pos, snap] = await Promise.all([listPositions(), getOrgDesignerSnapshot()]);
      setPositions(pos.filter((p) => p.is_active !== 0).map((p) => ({ id: p.id, code: p.code, name: p.name })));
      const nodes = snap.nodes.filter((n) => n.status === "active");
      const entityCandidates = nodes.filter((n) => n.active_binding_count > 0);
      const entities = entityCandidates.length > 0 ? entityCandidates : nodes.filter((n) => n.can_own_work);
      setEntityNodes(entities.length > 0 ? entities : nodes);
      setTeamNodes(nodes);
    } catch {
      setEntityNodes([]);
      setTeamNodes([]);
    }
    try {
      const ext = await listExternalCompanies({});
      setCompanies(ext.filter((c) => c.is_active !== 0).map((c) => ({ id: c.id, name: c.name })));
    } catch {
      setCompanies([]);
    }
  }, []);

  useEffect(() => {
    if (!showCreateForm) return;
    void loadLookups();
    setLocalError(null);
    setFullName("");
    setEmploymentType("employee");
    setPositionId("__none__");
    setEntityId("__none__");
    setTeamId("__none__");
    setEmail("");
    setPhone("");
    setNotes("");
    setExternalCompanyId("__none__");
  }, [showCreateForm, loadLookups]);

  const handleSubmit = async () => {
    setLocalError(null);
    const name = fullName.trim();
    if (!name) {
      setLocalError(t("validation.nameRequired"));
      return;
    }
    if (!employmentType) {
      setLocalError(t("validation.employmentTypeRequired"));
      return;
    }

    const needsCompany = employmentType === "contractor" || employmentType === "vendor";
    const input: PersonnelCreateInput = {
      full_name: name,
      employment_type: employmentType,
      position_id: positionId === "__none__" ? null : Number(positionId),
      primary_entity_id: entityId === "__none__" ? null : Number(entityId),
      primary_team_id: teamId === "__none__" ? null : Number(teamId),
      email: email.trim() || null,
      phone: phone.trim() || null,
      notes: notes.trim() || null,
      external_company_id:
        needsCompany && externalCompanyId !== "__none__" ? Number(externalCompanyId) : null,
    };

    try {
      await submitNewPersonnel(input);
    } catch {
      /* store holds error */
    }
  };

  return (
    <Dialog open={showCreateForm} onOpenChange={(open) => !open && closeCreateForm()}>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("create.title")}</DialogTitle>
        </DialogHeader>
        <div className="grid gap-4 py-2">
          {localError ? <p className="text-sm text-destructive">{localError}</p> : null}
          <div className="grid gap-2">
            <Label htmlFor="personnel-full-name">{t("field.fullName")}</Label>
            <Input
              id="personnel-full-name"
              value={fullName}
              onChange={(e) => setFullName(e.target.value)}
              autoComplete="name"
            />
          </div>
          <div className="grid gap-2">
            <Label>{t("field.employmentType")}</Label>
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
          {employmentType === "contractor" || employmentType === "vendor" ? (
            <div className="grid gap-2">
              <Label>{t("field.company")}</Label>
              <Select value={externalCompanyId} onValueChange={setExternalCompanyId}>
                <SelectTrigger>
                  <SelectValue placeholder={t("filters.all")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="__none__">{t("filters.all")}</SelectItem>
                  {companies.map((c) => (
                    <SelectItem key={c.id} value={String(c.id)}>
                      {c.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          ) : null}
          <div className="grid gap-2">
            <Label>{t("field.position")}</Label>
            <Select value={positionId} onValueChange={setPositionId}>
              <SelectTrigger>
                <SelectValue placeholder={t("filters.all")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__none__">{t("filters.all")}</SelectItem>
                {positions.map((p) => (
                  <SelectItem key={p.id} value={String(p.id)}>
                    {p.code} — {p.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="grid gap-2">
            <Label>{t("field.entity")}</Label>
            <Select value={entityId} onValueChange={setEntityId}>
              <SelectTrigger>
                <SelectValue placeholder={t("filters.all")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__none__">{t("filters.all")}</SelectItem>
                {entityNodes.map((n) => (
                  <SelectItem key={n.node_id} value={String(n.node_id)}>
                    {n.code} — {n.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="grid gap-2">
            <Label>{t("field.team")}</Label>
            <Select value={teamId} onValueChange={setTeamId}>
              <SelectTrigger>
                <SelectValue placeholder={t("filters.all")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__none__">{t("filters.all")}</SelectItem>
                {teamNodes.map((n) => (
                  <SelectItem key={n.node_id} value={String(n.node_id)}>
                    {n.code} — {n.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="grid gap-2">
            <Label htmlFor="personnel-email">{t("field.email")}</Label>
            <Input
              id="personnel-email"
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              autoComplete="email"
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="personnel-phone">{t("field.phone")}</Label>
            <Input
              id="personnel-phone"
              type="tel"
              value={phone}
              onChange={(e) => setPhone(e.target.value)}
              autoComplete="tel"
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="personnel-notes">{t("field.notes")}</Label>
            <Textarea id="personnel-notes" value={notes} onChange={(e) => setNotes(e.target.value)} rows={3} />
          </div>
        </div>
        <DialogFooter className="gap-2 sm:gap-0">
          <Button type="button" variant="outline" onClick={closeCreateForm}>
            {t("create.cancel")}
          </Button>
          <Button type="button" onClick={() => void handleSubmit()} disabled={saving}>
            {t("create.submit")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
