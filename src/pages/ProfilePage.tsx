import { ArrowLeft, Camera, KeyRound, Lock, Shield, Smartphone } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

import { clearPin, setPin } from "@/services/auth-service";
import {
  changePassword,
  getMyProfile,
  getSessionHistory,
  updateMyProfile,
} from "@/services/user-service";
import type { SessionHistoryEntry, UpdateProfileInput, UserProfile } from "@shared/ipc-types";

// ── helpers ───────────────────────────────────────────────────────────────

function formatDate(iso: string | null): string {
  if (!iso) return "—";
  return new Date(iso).toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

function formatDuration(seconds: number | null): string {
  if (seconds == null) return "—";
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

// ── component ─────────────────────────────────────────────────────────────

export function ProfilePage() {
  const { t } = useTranslation("admin");

  const [profile, setProfile] = useState<UserProfile | null>(null);
  const [sessions, setSessions] = useState<SessionHistoryEntry[]>([]);
  const [loading, setLoading] = useState(true);

  // edit mode
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState<UpdateProfileInput>({
    display_name: null,
    email: null,
    phone: null,
    language: null,
  });
  const [saving, setSaving] = useState(false);

  // dialogs
  const [pwDialogOpen, setPwDialogOpen] = useState(false);
  const [pinDialogOpen, setPinDialogOpen] = useState(false);

  // ── load data ─────────────────────────────────────────────────────────

  const loadData = useCallback(async () => {
    setLoading(true);
    try {
      const [p, s] = await Promise.all([getMyProfile(), getSessionHistory(10)]);
      setProfile(p);
      setSessions(s);
      setDraft({
        display_name: p.display_name,
        email: p.email,
        phone: p.phone,
        language: p.language,
      });
    } catch {
      /* ignore — UI shows empty */
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadData();
  }, [loadData]);

  // ── save personal info ────────────────────────────────────────────────

  const handleSave = async () => {
    setSaving(true);
    try {
      const updated = await updateMyProfile(draft);
      setProfile(updated);
      setEditing(false);
    } catch {
      /* toast would go here */
    } finally {
      setSaving(false);
    }
  };

  // ── loading state ─────────────────────────────────────────────────────

  if (loading || !profile) {
    return (
      <div className="flex h-full items-center justify-center">
        <div className="h-6 w-6 animate-spin rounded-full border-2 border-surface-3 border-t-primary" />
      </div>
    );
  }

  const initials = (profile.display_name ?? profile.username)
    .split(" ")
    .map((w) => w[0])
    .join("")
    .slice(0, 2)
    .toUpperCase();

  return (
    <div className="mx-auto max-w-2xl space-y-8 p-6">
      {/* Back link */}
      <Link
        to="/"
        className="inline-flex items-center gap-1 text-sm text-text-muted hover:text-text-primary"
      >
        <ArrowLeft className="h-4 w-4" />
        {t("profile.cancel", "Back")}
      </Link>

      {/* ── Avatar header ──────────────────────────────────────────────── */}
      <div className="flex items-center gap-4">
        <div className="relative">
          <div className="flex h-16 w-16 items-center justify-center rounded-full bg-primary/10 text-xl font-semibold text-primary">
            {initials}
          </div>
          <button
            type="button"
            className="absolute -bottom-1 -right-1 flex h-6 w-6 items-center justify-center rounded-full bg-surface-1 border border-surface-border text-text-muted hover:text-text-primary"
            aria-label="Change avatar"
          >
            <Camera className="h-3 w-3" />
          </button>
        </div>
        <div>
          <h1 className="text-lg font-semibold text-text-primary">
            {profile.display_name ?? profile.username}
          </h1>
          <p className="text-sm text-text-secondary">
            {profile.role_name ?? "—"} · {profile.email ?? "—"}
          </p>
          <p className="text-xs text-text-muted">
            {t("profile.memberSince", "Member since")} {formatDate(profile.created_at)}
          </p>
        </div>
      </div>

      <hr className="border-surface-border" />

      {/* ── Personal Information ───────────────────────────────────────── */}
      <section>
        <div className="flex items-center justify-between mb-3">
          <h2 className="text-sm font-semibold text-text-primary">
            {t("profile.personalInfo", "Personal Information")}
          </h2>
          {!editing ? (
            <button
              type="button"
              onClick={() => setEditing(true)}
              className="text-xs font-medium text-primary hover:text-primary/80"
            >
              {t("profile.edit", "Edit")}
            </button>
          ) : (
            <div className="flex gap-2">
              <button
                type="button"
                onClick={() => {
                  setEditing(false);
                  setDraft({
                    display_name: profile.display_name,
                    email: profile.email,
                    phone: profile.phone,
                    language: profile.language,
                  });
                }}
                className="text-xs text-text-muted hover:text-text-primary"
              >
                {t("profile.cancel", "Cancel")}
              </button>
              <button
                type="button"
                onClick={handleSave}
                disabled={saving}
                className="text-xs font-medium text-primary hover:text-primary/80 disabled:opacity-50"
              >
                {saving ? t("profile.saving", "Saving...") : t("profile.save", "Save")}
              </button>
            </div>
          )}
        </div>

        <div className="rounded-lg border border-surface-border divide-y divide-surface-border">
          <InfoRow
            label={t("profile.displayName", "Display name")}
            value={draft.display_name ?? ""}
            editing={editing}
            onChange={(v) => setDraft((d) => ({ ...d, display_name: v }))}
          />
          <InfoRow
            label={t("profile.email", "Email")}
            value={draft.email ?? ""}
            editing={editing}
            onChange={(v) => setDraft((d) => ({ ...d, email: v }))}
          />
          <InfoRow
            label={t("profile.phone", "Phone")}
            value={draft.phone ?? ""}
            editing={editing}
            onChange={(v) => setDraft((d) => ({ ...d, phone: v }))}
          />
          <InfoRow
            label={t("profile.language", "Language")}
            value={draft.language ?? ""}
            editing={editing}
            onChange={(v) => setDraft((d) => ({ ...d, language: v }))}
          />
        </div>
      </section>

      {/* ── Security ───────────────────────────────────────────────────── */}
      <section>
        <h2 className="text-sm font-semibold text-text-primary mb-3">
          {t("profile.security", "Security")}
        </h2>
        <div className="rounded-lg border border-surface-border divide-y divide-surface-border">
          {/* Password */}
          <div className="flex items-center justify-between px-4 py-3">
            <div className="flex items-center gap-3">
              <Lock className="h-4 w-4 text-text-muted" />
              <div>
                <p className="text-sm text-text-primary">{t("profile.password", "Password")}</p>
                <p className="text-xs text-text-muted">
                  {t("profile.lastChanged", "Last changed:")}{" "}
                  {formatDate(profile.password_changed_at)}
                </p>
              </div>
            </div>
            <button
              type="button"
              onClick={() => setPwDialogOpen(true)}
              className="text-xs font-medium text-primary hover:text-primary/80"
            >
              {t("profile.changePassword", "Change")}
            </button>
          </div>

          {/* PIN */}
          <div className="flex items-center justify-between px-4 py-3">
            <div className="flex items-center gap-3">
              <KeyRound className="h-4 w-4 text-text-muted" />
              <div>
                <p className="text-sm text-text-primary">
                  {t("profile.pinUnlock", "Quick Unlock PIN")}
                </p>
                <p className="text-xs text-text-muted">
                  {profile.pin_configured
                    ? t("profile.pinConfigured", "Configured")
                    : t("profile.pinNotSet", "Not configured")}
                </p>
              </div>
            </div>
            <div className="flex gap-2">
              <button
                type="button"
                onClick={() => setPinDialogOpen(true)}
                className="text-xs font-medium text-primary hover:text-primary/80"
              >
                {profile.pin_configured
                  ? t("profile.changePin", "Change")
                  : t("profile.setPin", "Set up")}
              </button>
              {profile.pin_configured && (
                <button
                  type="button"
                  onClick={async () => {
                    const pw = window.prompt(
                      t("profile.enterPasswordToClearPin", "Enter your password to remove PIN"),
                    );
                    if (!pw) return;
                    try {
                      await clearPin({ password: pw });
                      await loadData();
                    } catch {
                      /* ignore */
                    }
                  }}
                  className="text-xs text-red-600 hover:text-red-500"
                >
                  {t("profile.removePin", "Remove")}
                </button>
              )}
            </div>
          </div>

          {/* Trusted devices */}
          <div className="flex items-center justify-between px-4 py-3">
            <div className="flex items-center gap-3">
              <Smartphone className="h-4 w-4 text-text-muted" />
              <p className="text-sm text-text-primary">
                {t("profile.trustedDevices", "Trusted Devices")}
              </p>
            </div>
            <button
              type="button"
              className="text-xs font-medium text-primary hover:text-primary/80"
            >
              {t("profile.viewDevices", "View / Revoke")}
            </button>
          </div>
        </div>
      </section>

      {/* ── Session History ────────────────────────────────────────────── */}
      <section>
        <h2 className="text-sm font-semibold text-text-primary mb-3">
          {t("profile.sessionHistory", "Session History")}
        </h2>
        {sessions.length === 0 ? (
          <p className="text-sm text-text-muted">
            {t("profile.noSessions", "No session history.")}
          </p>
        ) : (
          <div className="rounded-lg border border-surface-border overflow-hidden">
            <table className="w-full text-sm">
              <thead className="bg-surface-1 text-left text-xs text-text-muted">
                <tr>
                  <th className="px-4 py-2 font-medium">{t("profile.date", "Date")}</th>
                  <th className="px-4 py-2 font-medium">{t("profile.device", "Device")}</th>
                  <th className="px-4 py-2 font-medium">{t("profile.duration", "Duration")}</th>
                  <th className="px-4 py-2 font-medium">{t("profile.status", "Status")}</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-surface-border">
                {sessions.map((s) => (
                  <tr key={s.id} className="hover:bg-surface-1/50">
                    <td className="px-4 py-2 text-text-primary">{formatDate(s.started_at)}</td>
                    <td className="px-4 py-2 text-text-secondary">{s.device_label ?? "—"}</td>
                    <td className="px-4 py-2 text-text-secondary">
                      {formatDuration(s.duration_seconds)}
                    </td>
                    <td className="px-4 py-2">
                      <span
                        className={s.status === "active" ? "text-emerald-600" : "text-text-muted"}
                      >
                        {s.status}
                      </span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      {/* ── Change Password Dialog ─────────────────────────────────────── */}
      {pwDialogOpen && (
        <ChangePasswordDialog
          onClose={() => setPwDialogOpen(false)}
          onSuccess={() => {
            setPwDialogOpen(false);
            void loadData();
          }}
        />
      )}

      {/* ── PIN Setup Dialog ───────────────────────────────────────────── */}
      {pinDialogOpen && (
        <PinSetupDialog
          onClose={() => setPinDialogOpen(false)}
          onSuccess={() => {
            setPinDialogOpen(false);
            void loadData();
          }}
        />
      )}
    </div>
  );
}

// ── InfoRow sub-component ─────────────────────────────────────────────────

function InfoRow({
  label,
  value,
  editing,
  onChange,
}: {
  label: string;
  value: string;
  editing: boolean;
  onChange: (v: string) => void;
}) {
  return (
    <div className="flex items-center justify-between px-4 py-3">
      <span className="text-sm text-text-muted w-32 shrink-0">{label}</span>
      {editing ? (
        <input
          type="text"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          className="flex-1 rounded-md border border-surface-border bg-surface-1 px-2 py-1 text-sm text-text-primary focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
        />
      ) : (
        <span className="text-sm text-text-primary">{value || "—"}</span>
      )}
    </div>
  );
}

// ── ChangePasswordDialog ──────────────────────────────────────────────────

function ChangePasswordDialog({
  onClose,
  onSuccess,
}: {
  onClose: () => void;
  onSuccess: () => void;
}) {
  const { t } = useTranslation("admin");
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    if (next !== confirm) {
      setError(t("profile.passwordMismatch", "Passwords do not match."));
      return;
    }
    setSubmitting(true);
    try {
      await changePassword({ current_password: current, new_password: next });
      onSuccess();
    } catch {
      setError(t("profile.passwordError", "Password change failed."));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <DialogOverlay onClose={onClose}>
      <h3 className="text-base font-semibold text-text-primary mb-4">
        <Shield className="inline h-4 w-4 mr-1" />
        {t("profile.changePasswordTitle", "Change Password")}
      </h3>
      <form onSubmit={handleSubmit} className="space-y-3">
        <input
          type="password"
          autoComplete="current-password"
          value={current}
          onChange={(e) => setCurrent(e.target.value)}
          placeholder={t("profile.currentPassword", "Current password")}
          required
          className="w-full rounded-md border border-surface-border bg-surface-1 px-3 py-2 text-sm text-text-primary placeholder:text-text-muted focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
        />
        <input
          type="password"
          autoComplete="new-password"
          value={next}
          onChange={(e) => setNext(e.target.value)}
          placeholder={t("profile.newPassword", "New password")}
          required
          className="w-full rounded-md border border-surface-border bg-surface-1 px-3 py-2 text-sm text-text-primary placeholder:text-text-muted focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
        />
        <input
          type="password"
          autoComplete="new-password"
          value={confirm}
          onChange={(e) => setConfirm(e.target.value)}
          placeholder={t("profile.confirmPassword", "Confirm password")}
          required
          className="w-full rounded-md border border-surface-border bg-surface-1 px-3 py-2 text-sm text-text-primary placeholder:text-text-muted focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
        />
        {error && <p className="text-xs text-red-600">{error}</p>}
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded-md px-3 py-1.5 text-sm text-text-muted hover:text-text-primary"
          >
            {t("profile.cancel", "Cancel")}
          </button>
          <button
            type="submit"
            disabled={submitting}
            className="rounded-md bg-primary px-3 py-1.5 text-sm text-white hover:bg-primary/90 disabled:opacity-50"
          >
            {submitting ? t("profile.saving", "Saving...") : t("profile.save", "Save")}
          </button>
        </div>
      </form>
    </DialogOverlay>
  );
}

// ── PinSetupDialog ────────────────────────────────────────────────────────

function PinSetupDialog({ onClose, onSuccess }: { onClose: () => void; onSuccess: () => void }) {
  const { t } = useTranslation("admin");
  const [pin, setPinValue] = useState("");
  const [confirmPin, setConfirmPin] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);

    if (!/^\d{4,6}$/.test(pin)) {
      setError(t("profile.pinFormat", "PIN must be 4 to 6 digits."));
      return;
    }
    if (pin !== confirmPin) {
      setError(t("profile.pinMismatch", "PINs do not match."));
      return;
    }
    setSubmitting(true);
    try {
      await setPin({ pin, password });
      onSuccess();
    } catch {
      setError(t("profile.pinError", "PIN setup failed."));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <DialogOverlay onClose={onClose}>
      <h3 className="text-base font-semibold text-text-primary mb-4">
        <KeyRound className="inline h-4 w-4 mr-1" />
        {t("profile.setPinTitle", "Set Up PIN")}
      </h3>
      <form onSubmit={handleSubmit} className="space-y-3">
        <input
          type="password"
          autoComplete="current-password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          placeholder={t("profile.currentPassword", "Current password")}
          required
          className="w-full rounded-md border border-surface-border bg-surface-1 px-3 py-2 text-sm text-text-primary placeholder:text-text-muted focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
        />
        <input
          type="text"
          inputMode="numeric"
          pattern="\d{4,6}"
          maxLength={6}
          value={pin}
          onChange={(e) => setPinValue(e.target.value.replace(/\D/g, ""))}
          placeholder={t("profile.newPin", "New PIN (4-6 digits)")}
          required
          className="w-full rounded-md border border-surface-border bg-surface-1 px-3 py-2 text-sm text-text-primary placeholder:text-text-muted focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
        />
        <input
          type="text"
          inputMode="numeric"
          pattern="\d{4,6}"
          maxLength={6}
          value={confirmPin}
          onChange={(e) => setConfirmPin(e.target.value.replace(/\D/g, ""))}
          placeholder={t("profile.confirmPin", "Confirm PIN")}
          required
          className="w-full rounded-md border border-surface-border bg-surface-1 px-3 py-2 text-sm text-text-primary placeholder:text-text-muted focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
        />
        {error && <p className="text-xs text-red-600">{error}</p>}
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded-md px-3 py-1.5 text-sm text-text-muted hover:text-text-primary"
          >
            {t("profile.cancel", "Cancel")}
          </button>
          <button
            type="submit"
            disabled={submitting}
            className="rounded-md bg-primary px-3 py-1.5 text-sm text-white hover:bg-primary/90 disabled:opacity-50"
          >
            {submitting ? t("profile.saving", "Saving...") : t("profile.save", "Save")}
          </button>
        </div>
      </form>
    </DialogOverlay>
  );
}

// ── DialogOverlay ─────────────────────────────────────────────────────────

function DialogOverlay({ onClose, children }: { onClose: () => void; children: React.ReactNode }) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div
        className="relative w-full max-w-md rounded-lg border border-surface-border bg-surface-0 p-6 shadow-lg"
        // biome-ignore lint/a11y/useSemanticElements: custom overlay dialog
        role="dialog"
        aria-modal="true"
      >
        <button
          type="button"
          onClick={onClose}
          className="absolute right-3 top-3 text-text-muted hover:text-text-primary"
          aria-label="Close"
        >
          ✕
        </button>
        {children}
      </div>
    </div>
  );
}
