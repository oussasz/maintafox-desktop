import { Building2 } from "lucide-react";

import { AuthStatusChip } from "@/components/auth/shell/AuthPrimitives";
import { i18n } from "@/i18n";
import { cn } from "@/lib/utils";

export type OrganizationLicenseCardFields = {
  companyName: string;
  tenantSlug?: string | null;
  licensePlan?: string | null;
  licenseStatus?: string | null;
  channel?: string | null;
  expiresAt?: string | null;
  allowedDevices?: number | null;
  activatedDevices?: number | null;
  deviceName?: string | null;
};

type Row = { label: string; value: string; mono?: boolean };

type OrganizationLicenseCardProps = {
  fields: OrganizationLicenseCardFields;
  labels: {
    companyBelongsTo: string;
    slug: string;
    plan: string;
    status: string;
    channel: string;
    expiration: string;
    allowedDevices: string;
    activatedDevices: string;
    deviceName: string;
    notSpecified: string;
  };
};

function display(value: string | number | null | undefined, fallback: string): string {
  if (value == null) return fallback;
  if (typeof value === "string" && value.trim().length === 0) return fallback;
  return String(value);
}

/**
 * Human-readable licence date in the active UI language ("10 Sep 2027"). Values that are not a
 * valid date are returned unchanged so nothing the server sent is hidden.
 */
export function formatLicenseDate(value: string, locale: string): string {
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) return value;
  try {
    return new Intl.DateTimeFormat(locale, {
      day: "numeric",
      month: "short",
      year: "numeric",
    }).format(parsed);
  } catch {
    return value;
  }
}

/** Summary of the organisation and licence a device is about to be activated against. */
export function OrganizationLicenseCard({ fields, labels }: OrganizationLicenseCardProps) {
  const locale = i18n.language || "en";
  const expiration = display(fields.expiresAt, labels.notSpecified);
  const statusText = display(fields.licenseStatus, labels.notSpecified);
  const statusOk = fields.licenseStatus?.trim().toLowerCase() === "active";

  const rows: Row[] = [
    { label: labels.slug, value: display(fields.tenantSlug, labels.notSpecified), mono: true },
    { label: labels.plan, value: display(fields.licensePlan, labels.notSpecified) },
    { label: labels.channel, value: display(fields.channel, labels.notSpecified) },
    {
      label: labels.expiration,
      value: fields.expiresAt ? formatLicenseDate(expiration, locale) : expiration,
    },
    { label: labels.allowedDevices, value: display(fields.allowedDevices, labels.notSpecified) },
    {
      label: labels.activatedDevices,
      value: display(fields.activatedDevices, labels.notSpecified),
    },
    { label: labels.deviceName, value: display(fields.deviceName, labels.notSpecified) },
  ];

  return (
    <section
      className="rounded-lg border border-surface-border bg-[var(--mfa-ice)]/70 p-4"
      aria-labelledby="activation-org-heading"
    >
      <div className="flex items-start gap-3">
        <span
          className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-white text-[var(--mfa-blue)] ring-1 ring-surface-border"
          aria-hidden
        >
          <Building2 className="h-[18px] w-[18px]" />
        </span>
        <div className="min-w-0 flex-1">
          <p className="text-xs font-medium text-text-muted">{labels.companyBelongsTo}</p>
          <h2
            id="activation-org-heading"
            className="mt-0.5 break-words text-base font-semibold leading-snug text-text-primary"
          >
            {fields.companyName}
          </h2>
        </div>
        <AuthStatusChip tone={statusOk ? "ok" : "neutral"} className="shrink-0">
          <span className="sr-only">{labels.status}: </span>
          {statusText}
        </AuthStatusChip>
      </div>
      <dl className="mt-4 grid grid-cols-2 gap-x-4 gap-y-3 border-t border-surface-border pt-4 sm:gap-x-6">
        {rows.map((row) => (
          <div key={row.label} className="min-w-0">
            <dt className="text-xs text-text-muted">{row.label}</dt>
            <dd
              className={cn(
                "mt-0.5 truncate text-sm font-medium text-text-primary",
                row.mono && "font-mono text-[13px]",
              )}
              title={row.value}
            >
              {row.value}
            </dd>
          </div>
        ))}
      </dl>
    </section>
  );
}
