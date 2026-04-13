import { Lock } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import {
  getNotificationPreferences,
  updateNotificationPreference,
  type UserPreferenceRow,
} from "@/services/notification-service";
import { toErrorMessage } from "@/utils/errors";

export function NotificationPreferencesPanel() {
  const [rows, setRows] = useState<UserPreferenceRow[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setRows(await getNotificationPreferences());
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
          sms_enabled?: boolean;
          digest_mode?: string;
          muted_until?: string | null;
        } = { category_code: row.category_code };
        if (changes.in_app_enabled !== undefined) payload.in_app_enabled = changes.in_app_enabled;
        if (changes.os_enabled !== undefined) payload.os_enabled = changes.os_enabled;
        if (changes.email_enabled !== undefined) payload.email_enabled = changes.email_enabled;
        if (changes.sms_enabled !== undefined) payload.sms_enabled = changes.sms_enabled;
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
    return <div className="text-sm text-muted-foreground">Loading preferences...</div>;
  }

  return (
    <div className="space-y-3 rounded-lg border p-4">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold">Notification Preferences</h3>
        <Badge variant="outline">{rows.length} categories</Badge>
      </div>

      {error && <div className="text-sm text-destructive">{error}</div>}

      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Category</TableHead>
            <TableHead>In-app</TableHead>
            <TableHead>OS</TableHead>
            <TableHead>Email</TableHead>
            <TableHead>SMS</TableHead>
            <TableHead>Digest</TableHead>
            <TableHead>Mute until</TableHead>
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
                    <span>{row.label}</span>
                    {disabled && (
                      <span title="System-managed">
                        <Lock className="h-3.5 w-3.5" />
                      </span>
                    )}
                  </div>
                </TableCell>
                <TableCell>
                  <Switch
                    checked={row.in_app_enabled}
                    disabled={disabled}
                    onCheckedChange={(v) => void patchRow(row, { in_app_enabled: v })}
                  />
                </TableCell>
                <TableCell>
                  <Switch
                    checked={row.os_enabled}
                    disabled={disabled}
                    onCheckedChange={(v) => void patchRow(row, { os_enabled: v })}
                  />
                </TableCell>
                <TableCell>
                  <Switch
                    checked={row.email_enabled}
                    disabled={disabled}
                    onCheckedChange={(v) => void patchRow(row, { email_enabled: v })}
                  />
                </TableCell>
                <TableCell>
                  <Switch
                    checked={row.sms_enabled}
                    disabled={disabled}
                    onCheckedChange={(v) => void patchRow(row, { sms_enabled: v })}
                  />
                </TableCell>
                <TableCell>
                  <Select
                    value={row.digest_mode}
                    disabled={disabled}
                    onValueChange={(v) => void patchRow(row, { digest_mode: v })}
                  >
                    <SelectTrigger className="w-36">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="instant">instant</SelectItem>
                      <SelectItem value="daily_digest">daily_digest</SelectItem>
                      <SelectItem value="off">off</SelectItem>
                    </SelectContent>
                  </Select>
                </TableCell>
                <TableCell>
                  <input
                    type="datetime-local"
                    disabled={disabled}
                    className="h-9 rounded-md border bg-background px-2 text-sm"
                    value={toDatetimeLocalValue(row.muted_until)}
                    onChange={(e) =>
                      void patchRow(row, {
                        muted_until: e.target.value
                          ? new Date(e.target.value).toISOString()
                          : "",
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
