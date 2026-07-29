import { mfCard } from "@/design-system/tokens";
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

type Row = { label: string; value: string };

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

export function OrganizationLicenseCard({ fields, labels }: OrganizationLicenseCardProps) {
  const rows: Row[] = [
    { label: labels.slug, value: display(fields.tenantSlug, labels.notSpecified) },
    { label: labels.plan, value: display(fields.licensePlan, labels.notSpecified) },
    { label: labels.status, value: display(fields.licenseStatus, labels.notSpecified) },
    { label: labels.channel, value: display(fields.channel, labels.notSpecified) },
    { label: labels.expiration, value: display(fields.expiresAt, labels.notSpecified) },
    {
      label: labels.allowedDevices,
      value: display(fields.allowedDevices, labels.notSpecified),
    },
    {
      label: labels.activatedDevices,
      value: display(fields.activatedDevices, labels.notSpecified),
    },
    { label: labels.deviceName, value: display(fields.deviceName, labels.notSpecified) },
  ];

  return (
    <section className={cn(mfCard.panel, "space-y-4")} aria-labelledby="activation-org-heading">
      <div>
        <p className="text-xs font-medium uppercase tracking-wide text-text-muted">
          {labels.companyBelongsTo}
        </p>
        <h2 id="activation-org-heading" className="mt-1 text-xl font-semibold text-text-primary">
          {fields.companyName}
        </h2>
      </div>
      <dl className="grid grid-cols-1 gap-3 sm:grid-cols-2">
        {rows.map((row) => (
          <div key={row.label} className="min-w-0">
            <dt className="text-xs text-text-muted">{row.label}</dt>
            <dd className="mt-0.5 truncate text-sm font-medium text-text-primary">{row.value}</dd>
          </div>
        ))}
      </dl>
    </section>
  );
}
