import { Lock } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { categoryLabel } from "@/components/notifications/notification-text";
import { Badge } from "@/components/ui/badge";
import { LoadingState } from "@/components/ui/data-state";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  getNotificationChannelStatus,
  getNotificationPreferences,
  type NotificationChannelStatus,
  updateNotificationPreference,
  type UserPreferenceRow,
} from "@/services/notification-service";
import { toErrorMessage } from "@/utils/errors";

const EMAIL_REASONS = ["server_endpoint_not_enabled", "licence_blocked", "device_not_activated"];

/** Category of the daily digest notification itself: always delivered instantly. */
const DIGEST_CATEGORY = "daily_digest";

export function NotificationPreferencesPanel() {
  const { t } = useTranslation("notifications");
  const [rows, setRows] = useState<UserPreferenceRow[]>([]);
  const [channels, setChannels] = useState<NotificationChannelStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [prefs, status] = await Promise.all([
        getNotificationPreferences(),
        getNotificationChannelStatus(),
      ]);
      setRows(prefs);
      setChannels(status);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const patchRow = useCallback(
    async (row: UserPreferenceRow, changes: Partial<UserPreferenceRow>) => {
      if (!row.is_user_configurable) return;
      const next = { ...row, ...changes };
      setRows((prev) =>
        prev.map((item) => (item.category_code === row.category_code ? next : item)),
      );
      try {
        const payload: {
          category_code: string;
          in_app_enabled?: boolean;
          os_enabled?: boolean;
          email_enabled?: boolean;
          digest_mode?: string;
          muted_until?: string | null;
        } = { category_code: row.category_code };
        if (changes.in_app_enabled !== undefined) payload.in_app_enabled = changes.in_app_enabled;
        if (changes.os_enabled !== undefined) payload.os_enabled = changes.os_enabled;
        if (changes.email_enabled !== undefined) payload.email_enabled = changes.email_enabled;
        if (changes.digest_mode !== undefined) payload.digest_mode = changes.digest_mode;
        if (changes.muted_until !== undefined) payload.muted_until = changes.muted_until;

        await updateNotificationPreference(payload);
      } catch (err) {
        setError(toErrorMessage(err));
        await load();
      }
    },
    [load],
  );

  if (loading) {
    return <LoadingState label={t("preferences.loading")} />;
  }

  const emailAvailable = channels?.email.available === true;
  const emailReason = channels?.email.reason ?? null;
  const emailUnavailableText =
    channels == null || emailAvailable
      ? null
      : emailReason != null && EMAIL_REASONS.includes(emailReason)
        ? t(`preferences.emailUnavailable.${emailReason}`)
        : t("preferences.emailUnavailable.unknown", { message: emailReason ?? "" });

  return (
    <div className="space-y-3 rounded-lg border p-4">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold">{t("preferences.title")}</h3>
        <Badge variant="outline">{t("preferences.categories", { count: rows.length })}</Badge>
      </div>

      {error && <div className="text-sm text-destructive">{error}</div>}
      {emailUnavailableText && (
        <div role="status" className="text-xs text-muted-foreground">
          {emailUnavailableText}
        </div>
      )}
      <div className="text-xs text-muted-foreground">{t("preferences.receiveHelp")}</div>
      <div className="text-xs text-muted-foreground">{t("preferences.digestHelp")}</div>

      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>{t("preferences.col.category")}</TableHead>
            <TableHead>{t("preferences.col.inApp")}</TableHead>
            <TableHead>{t("preferences.col.os")}</TableHead>
            <TableHead>{t("preferences.col.email")}</TableHead>
            <TableHead>{t("preferences.col.digest")}</TableHead>
            <TableHead>{t("preferences.col.muteUntil")}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((row) => {
            const disabled = !row.is_user_configurable;
            return (
              <TableRow
                key={row.category_code}
                className={disabled ? "bg-muted/40 text-muted-foreground" : undefined}
              >
                <TableCell>
                  <div className="flex items-center gap-2">
                    <span>{categoryLabel(row.category_code, t)}</span>
                    <span className="text-xs text-muted-foreground">
                      {t(`module.${row.module_code}`)}
                    </span>
                    {disabled && (
                      <span title={t("preferences.systemManaged")}>
                        <Lock className="h-3.5 w-3.5" />
                      </span>
                    )}
                  </div>
                </TableCell>
                <TableCell>
                  <Switch
                    checked={row.in_app_enabled}
                    disabled={disabled}
                    aria-label={`${t("preferences.col.inApp")} ${categoryLabel(row.category_code, t)}`}
                    onCheckedChange={(v) => void patchRow(row, { in_app_enabled: v })}
                  />
                </TableCell>
                <TableCell>
                  <Switch
                    checked={row.os_enabled}
                    disabled={disabled || !row.in_app_enabled}
                    aria-label={`${t("preferences.col.os")} ${categoryLabel(row.category_code, t)}`}
                    onCheckedChange={(v) => void patchRow(row, { os_enabled: v })}
                  />
                </TableCell>
                <TableCell>
                  <Switch
                    checked={row.email_enabled}
                    disabled={disabled || !emailAvailable || !row.in_app_enabled}
                    aria-label={t("preferences.col.email")}
                    title={emailUnavailableText ?? undefined}
                    onCheckedChange={(v) => void patchRow(row, { email_enabled: v })}
                  />
                </TableCell>
                <TableCell>
                  <Select
                    value={row.digest_mode}
                    disabled={
                      disabled || !row.in_app_enabled || row.category_code === DIGEST_CATEGORY
                    }
                    onValueChange={(v) => void patchRow(row, { digest_mode: v })}
                  >
                    <SelectTrigger
                      className="w-36"
                      aria-label={`${t("preferences.col.digest")} ${categoryLabel(row.category_code, t)}`}
                    >
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="instant">{t("preferences.digest.instant")}</SelectItem>
                      <SelectItem value="daily_digest">
                        {t("preferences.digest.daily_digest")}
                      </SelectItem>
                      <SelectItem value="off">{t("preferences.digest.off")}</SelectItem>
                    </SelectContent>
                  </Select>
                </TableCell>
                <TableCell>
                  <input
                    type="datetime-local"
                    disabled={disabled}
                    aria-label={`${t("preferences.col.muteUntil")} ${categoryLabel(row.category_code, t)}`}
                    className="h-9 rounded-md border bg-background px-2 text-sm"
                    value={toDatetimeLocalValue(row.muted_until)}
                    onChange={(e) =>
                      void patchRow(row, {
                        muted_until: e.target.value ? new Date(e.target.value).toISOString() : "",
                      })
                    }
                  />
                </TableCell>
              </TableRow>
            );
          })}
        </TableBody>
      </Table>
    </div>
  );
}

function toDatetimeLocalValue(value: string | null): string {
  if (!value) return "";
  const dt = new Date(value);
  if (Number.isNaN(dt.getTime())) return "";
  const yyyy = dt.getFullYear();
  const mm = String(dt.getMonth() + 1).padStart(2, "0");
  const dd = String(dt.getDate()).padStart(2, "0");
  const hh = String(dt.getHours()).padStart(2, "0");
  const mi = String(dt.getMinutes()).padStart(2, "0");
  return `${yyyy}-${mm}-${dd}T${hh}:${mi}`;
}
