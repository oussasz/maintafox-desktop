import { ActivityLogSettingsSection } from "@/components/settings/ActivityLogSettingsSection";
import { mfLayout } from "@/design-system/tokens";

/**
 * Legacy standalone activity route body — prefer Settings → Security & Logs.
 * Kept for reuse/tests; router redirects `/activity` to Settings.
 */
export function ActivityPage() {
  return (
    <div className={mfLayout.moduleRoot}>
      <div className={mfLayout.moduleWorkspace}>
        <div className={mfLayout.moduleWorkspaceInner}>
          <div className={mfLayout.moduleWorkspaceBody}>
            <ActivityLogSettingsSection />
          </div>
        </div>
      </div>
    </div>
  );
}
