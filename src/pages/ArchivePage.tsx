import { ArchiveExplorer } from "@/components/archive/ArchiveExplorer";
import { RetentionPolicyPanel } from "@/components/archive/RetentionPolicyPanel";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

export function ArchivePage() {
  return (
    <div className="space-y-4">
      <div>
        <h1 className="text-xl font-semibold">Archivage</h1>
        <p className="text-sm text-muted-foreground">
          Browse archived records, verify integrity, and manage retention policies.
        </p>
      </div>

      <Tabs defaultValue="explorer">
        <TabsList className="grid w-full max-w-sm grid-cols-2">
          <TabsTrigger value="explorer">Archive Explorer</TabsTrigger>
          <TabsTrigger value="retention">Retention Policies</TabsTrigger>
        </TabsList>
        <TabsContent value="explorer">
          <ArchiveExplorer className="mt-2" />
        </TabsContent>
        <TabsContent value="retention">
          <div className="mt-2">
            <RetentionPolicyPanel />
          </div>
        </TabsContent>
      </Tabs>
    </div>
  );
}
