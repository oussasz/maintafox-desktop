import { ActivityFeedPanel } from "@/components/activity/ActivityFeedPanel";
import { AuditLogViewer } from "@/components/activity/AuditLogViewer";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

export function ActivityPage() {
  return (
    <div className="space-y-4">
      <div>
        <h1 className="text-xl font-semibold">Journal d'activite</h1>
        <p className="text-sm text-muted-foreground">
          Operational feed and immutable audit trail for security and compliance.
        </p>
      </div>

      <Tabs defaultValue="activity">
        <TabsList className="grid w-full max-w-md grid-cols-2">
          <TabsTrigger value="activity">Activity Feed</TabsTrigger>
          <TabsTrigger value="audit">Audit Log</TabsTrigger>
        </TabsList>
        <TabsContent value="activity">
          <ActivityFeedPanel />
        </TabsContent>
        <TabsContent value="audit">
          <AuditLogViewer />
        </TabsContent>
      </Tabs>
    </div>
  );
}
