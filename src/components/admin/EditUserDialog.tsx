import { Loader2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PersonnelPickerCombobox } from "@/components/personnel/PersonnelCard";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { useToast } from "@/hooks/use-toast";
import { listPersonnel } from "@/services/personnel-service";
import { updateUser } from "@/services/rbac-service";
import type { Personnel, UpdateUserInput, UserWithRoles } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/**
 * Edit the identity of an account (name, contact, linked personnel, password-change flag).
 * Enabling / disabling is not done here: it has its own action with a reason (admin ledger).
 */
export function EditUserDialog({
  user,
  open,
  onClose,
  onSaved,
}: {
  user: UserWithRoles | null;
  open: boolean;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation("admin");
  const { toast } = useToast();
  const { can } = usePermissions();
  const { formatDateTime } = useFormatters();
  const canViewPersonnel = can(P.PER_VIEW);
  const [username, setUsername] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [email, setEmail] = useState("");
  const [phone, setPhone] = useState("");
  const [forcePasswordChange, setForcePasswordChange] = useState(false);
  const [personnelId, setPersonnelId] = useState<number | null>(null);
  const [personnelItems, setPersonnelItems] = useState<Personnel[]>([]);
  const [personnelLoading, setPersonnelLoading] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!open || !user) return;
    setUsername(user.username);
    setDisplayName(user.display_name ?? "");
    setEmail(user.email ?? "");
    setPhone(user.phone ?? "");
    setForcePasswordChange(user.force_password_change);
    setPersonnelId(user.personnel_id ?? null);
  }, [open, user]);

  useEffect(() => {
    if (!open || !canViewPersonnel) return;
    let cancelled = false;
    setPersonnelLoading(true);
    void listPersonnel({ limit: 2000, offset: 0 })
      .then((page) => {
        if (!cancelled) setPersonnelItems(page.items);
      })
      .catch(() => {
        if (!cancelled) {
          setPersonnelItems([]);
          toast({
            title: t("users.create.personnelLoadError"),
            variant: "destructive",
          });
        }
      })
      .finally(() => {
        if (!cancelled) setPersonnelLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, canViewPersonnel, t, toast]);

  const emailError = useMemo(() => {
    const normalized = email.trim();
    if (!normalized) return null;
    const basicEmailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
    if (!basicEmailRegex.test(normalized)) {
      return t("users.edit.invalidEmail");
    }
    return null;
  }, [email, t]);

  const phoneError = useMemo(() => {
    const normalized = phone.trim();
    if (!normalized) return null;
    const cleaned = normalized.replace(/[^\d+]/g, "");
    const candidate = cleaned.startsWith("00")
      ? `+${cleaned.slice(2)}`
      : cleaned.startsWith("+")
        ? cleaned
        : `+${cleaned}`;
    const digits = candidate.replace(/\D/g, "");
    if (!candidate.startsWith("+") || digits.length < 8 || digits.length > 15) {
      return t("users.edit.invalidPhone");
    }
    return null;
  }, [phone, t]);

  const canSave = username.trim().length > 0 && !emailError && !phoneError && !saving;

  const handleSave = async () => {
    if (!user || !canSave) return;

    setSaving(true);
    try {
      const payload: UpdateUserInput = {
        user_id: user.id,
        username: username.trim(),
        display_name: displayName.trim() || "",
        email: email.trim() || "",
        phone: phone.trim() || "",
        ...(personnelId != null ? { personnel_id: personnelId } : {}),
        force_password_change: forcePasswordChange,
      };
      await updateUser(payload);

      toast({ title: t("users.edit.success"), variant: "success" });
      onSaved();
      onClose();
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      toast({
        title: msg || t("users.edit.error"),
        variant: "destructive",
      });
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("users.edit.title")}</DialogTitle>
          <DialogDescription>{t("users.edit.description")}</DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          <div className="grid grid-cols-1 gap-3 rounded-md border border-surface-border bg-surface-1/50 p-3 sm:grid-cols-2">
            <div>
              <p className="text-xs text-text-secondary">{t("users.fields.identityMode")}</p>
              <p className="text-sm font-medium">{user?.identity_mode ?? "—"}</p>
            </div>
            <div>
              <p className="text-xs text-text-secondary">{t("users.fields.lastSeen")}</p>
              <p className="text-sm font-medium">
                {user?.last_seen_at ? formatDateTime(user.last_seen_at) : "—"}
              </p>
            </div>
          </div>

          <div className="space-y-1.5">
            <label className="text-sm font-medium">{t("users.fields.displayName")}</label>
            <Input
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              disabled={saving}
              autoComplete="off"
            />
          </div>

          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div className="space-y-1.5">
              <label className="text-sm font-medium">{t("users.fields.email")}</label>
              <Input
                type="email"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                disabled={saving}
                placeholder={t("users.edit.emailPlaceholder")}
                autoComplete="email"
              />
              {emailError && <p className="text-xs text-red-600">{emailError}</p>}
            </div>

            <div className="space-y-1.5">
              <label className="text-sm font-medium">{t("users.fields.phone")}</label>
              <Input
                type="tel"
                value={phone}
                onChange={(e) => setPhone(e.target.value)}
                disabled={saving}
                placeholder="+33 6 12 34 56 78"
                autoComplete="tel"
              />
              {phoneError && <p className="text-xs text-red-600">{phoneError}</p>}
            </div>
          </div>

          <div className="space-y-1.5">
            <label className="text-sm font-medium">{t("users.fields.username")}</label>
            <Input
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              disabled={saving}
              autoComplete="off"
            />
          </div>

          <div className="space-y-1.5">
            <label className="text-sm font-medium">{t("users.create.linkPersonnel")}</label>
            {canViewPersonnel ? (
              personnelLoading ? (
                <p className="text-xs text-text-secondary">{t("common.loading")}</p>
              ) : (
                <PersonnelPickerCombobox
                  items={personnelItems}
                  value={personnelId}
                  onChange={setPersonnelId}
                  disabled={saving}
                  placeholder={t("users.create.personnelSearchPlaceholder")}
                />
              )
            ) : (
              <p className="text-xs text-amber-800 dark:text-amber-200">
                {t("users.create.noPersonnelPermission")}
              </p>
            )}
          </div>

          <label className="flex items-center gap-2 text-sm">
            <Checkbox
              checked={forcePasswordChange}
              onCheckedChange={setForcePasswordChange}
              disabled={saving}
            />
            {t("users.create.forceChange")}
          </label>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={saving}>
            {t("common.cancel")}
          </Button>
          <Button onClick={handleSave} disabled={!canSave}>
            {saving ? (
              <>
                <Loader2 className="mr-1.5 h-4 w-4 animate-spin" />
                {t("common.saving")}
              </>
            ) : (
              t("common.save")
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
