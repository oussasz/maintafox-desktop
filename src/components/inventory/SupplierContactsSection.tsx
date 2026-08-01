import { Contact } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { DetailSectionCard } from "@/components/detail";
import { Badge } from "@/components/ui/badge";
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
import {
  deleteSupplierContact,
  listSupplierContacts,
  upsertSupplierContact,
} from "@/services/inventory-service";
import { toErrorMessage } from "@/utils/errors";
import type { SupplierContact } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import { SUPPLIER_CONTACT_ROLES } from "./supplier-sourcing";

export interface SupplierContactsSectionProps {
  supplierId: number;
}

interface ContactFormState {
  id: number | null;
  contactName: string;
  contactRole: string;
  phone: string;
  email: string;
  isPrimary: boolean;
}

const NO_ROLE = "__none__";

const EMPTY_FORM: ContactFormState = {
  id: null,
  contactName: "",
  contactRole: NO_ROLE,
  phone: "",
  email: "",
  isPrimary: false,
};

export function SupplierContactsSection({ supplierId }: SupplierContactsSectionProps) {
  const { t } = useTranslation("inventory");
  const { t: tc } = useTranslation("common");

  const [contacts, setContacts] = useState<SupplierContact[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [form, setForm] = useState<ContactFormState>(EMPTY_FORM);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setContacts(await listSupplierContacts(supplierId));
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  }, [supplierId]);

  useEffect(() => {
    void load();
  }, [load]);

  const runMutation = async (mutate: () => Promise<unknown>) => {
    setSaving(true);
    setError(null);
    try {
      await mutate();
      await load();
      return true;
    } catch (err) {
      setError(toErrorMessage(err));
      return false;
    } finally {
      setSaving(false);
    }
  };

  const openAddForm = () => {
    setForm(EMPTY_FORM);
    setFormOpen(true);
  };

  const openEditForm = (contact: SupplierContact) => {
    setForm({
      id: contact.id,
      contactName: contact.contact_name,
      contactRole: contact.contact_role ?? NO_ROLE,
      phone: contact.phone ?? "",
      email: contact.email ?? "",
      isPrimary: contact.is_primary === 1,
    });
    setFormOpen(true);
  };

  const saveForm = async () => {
    const name = form.contactName.trim();
    if (!name) return;
    const ok = await runMutation(() =>
      upsertSupplierContact(form.id, {
        supplier_id: supplierId,
        contact_name: name,
        contact_role: form.contactRole === NO_ROLE ? null : form.contactRole,
        phone: form.phone.trim() || null,
        email: form.email.trim() || null,
        is_primary: form.isPrimary,
      }),
    );
    if (ok) setFormOpen(false);
  };

  return (
    <DetailSectionCard title={t("procurement.suppliers.contacts.title")} icon={Contact}>
      <div className="flex items-center justify-between">
        <p className="text-xs text-text-muted">
          {t("procurement.suppliers.contacts.resultCount", { count: contacts.length })}
        </p>
        <PermissionGate permission={P.INV_MANAGE}>
          <Button size="sm" variant="outline" onClick={openAddForm}>
            {t("procurement.suppliers.contacts.add")}
          </Button>
        </PermissionGate>
      </div>

      {error ? <p className="text-xs text-status-danger">{error}</p> : null}

      {loading ? (
        <p className="text-xs text-text-muted">{tc("app.loading")}</p>
      ) : contacts.length === 0 ? (
        <p className="text-xs text-text-muted">{t("procurement.suppliers.contacts.empty")}</p>
      ) : (
        <ul className="space-y-1.5">
          {contacts.map((contact) => (
            <li
              key={contact.id}
              className="flex flex-wrap items-center gap-2 rounded border border-surface-border p-1.5 text-xs"
            >
              <span className="font-medium">{contact.contact_name}</span>
              {contact.contact_role ? (
                <Badge variant="outline" className="h-4 text-[9px]">
                  {t(`procurement.suppliers.contacts.roles.${contact.contact_role}`, {
                    defaultValue: contact.contact_role,
                  })}
                </Badge>
              ) : null}
              {contact.is_primary === 1 ? (
                <Badge className="h-4 text-[9px]">
                  {t("procurement.suppliers.contacts.primary")}
                </Badge>
              ) : null}
              {contact.phone ? (
                <a href={`tel:${contact.phone}`} className="text-text-muted hover:underline">
                  {contact.phone}
                </a>
              ) : null}
              {contact.email ? (
                <a
                  href={`mailto:${contact.email}`}
                  className="break-all text-text-muted hover:underline"
                >
                  {contact.email}
                </a>
              ) : null}
              <PermissionGate permission={P.INV_MANAGE}>
                <div className="ml-auto flex gap-1">
                  <Button
                    size="sm"
                    variant="ghost"
                    className="h-5 px-1.5 text-[11px]"
                    disabled={saving}
                    onClick={() => openEditForm(contact)}
                  >
                    {tc("action.edit")}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    className="h-5 px-1.5 text-[11px] text-status-danger hover:text-status-danger"
                    disabled={saving}
                    onClick={() => void runMutation(() => deleteSupplierContact(contact.id))}
                  >
                    {tc("action.delete")}
                  </Button>
                </div>
              </PermissionGate>
            </li>
          ))}
        </ul>
      )}

      <Dialog open={formOpen} onOpenChange={setFormOpen}>
        <DialogContent className="max-w-md" onPointerDownOutside={(e) => e.preventDefault()}>
          <DialogHeader>
            <DialogTitle>
              {form.id
                ? t("procurement.suppliers.contacts.edit")
                : t("procurement.suppliers.contacts.add")}
            </DialogTitle>
          </DialogHeader>
          <div className="space-y-3">
            <div>
              <Label htmlFor="contact-name">
                {t("procurement.suppliers.contacts.fields.name")}
              </Label>
              <Input
                id="contact-name"
                value={form.contactName}
                onChange={(e) => setForm((prev) => ({ ...prev, contactName: e.target.value }))}
              />
            </div>
            <div>
              <Label htmlFor="contact-role">
                {t("procurement.suppliers.contacts.fields.role")}
              </Label>
              <Select
                value={form.contactRole}
                onValueChange={(v) => setForm((prev) => ({ ...prev, contactRole: v }))}
              >
                <SelectTrigger id="contact-role" className="h-8 text-sm">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={NO_ROLE}>{tc("label.none")}</SelectItem>
                  {SUPPLIER_CONTACT_ROLES.map((role) => (
                    <SelectItem key={role} value={role}>
                      {t(`procurement.suppliers.contacts.roles.${role}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div>
              <Label htmlFor="contact-phone">
                {t("procurement.suppliers.contacts.fields.phone")}
              </Label>
              <Input
                id="contact-phone"
                type="tel"
                value={form.phone}
                onChange={(e) => setForm((prev) => ({ ...prev, phone: e.target.value }))}
              />
            </div>
            <div>
              <Label htmlFor="contact-email">
                {t("procurement.suppliers.contacts.fields.email")}
              </Label>
              <Input
                id="contact-email"
                type="email"
                value={form.email}
                onChange={(e) => setForm((prev) => ({ ...prev, email: e.target.value }))}
              />
            </div>
            <div className="flex items-center gap-2">
              <Checkbox
                id="contact-primary"
                checked={form.isPrimary}
                onCheckedChange={(checked) => setForm((prev) => ({ ...prev, isPrimary: checked }))}
              />
              <Label htmlFor="contact-primary">
                {t("procurement.suppliers.contacts.fields.primary")}
              </Label>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setFormOpen(false)}>
              {tc("action.cancel")}
            </Button>
            <Button disabled={saving || !form.contactName.trim()} onClick={() => void saveForm()}>
              {tc("action.save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </DetailSectionCard>
  );
}
