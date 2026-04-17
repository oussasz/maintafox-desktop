import { AlertTriangle, Check, Pencil, Plus, Trash2, X } from "lucide-react";
import { useEffect, useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  createInventoryArticleFamily,
  deactivateInventoryArticleFamily,
  listInventoryArticleFamilies,
  updateInventoryArticleFamily,
} from "@/services/inventory-service";
import { toErrorMessage } from "@/utils/errors";
import type { ArticleFamily } from "@shared/ipc-types";

interface FamilyDraft {
  code: string;
  name: string;
  description: string;
}

const EMPTY_DRAFT: FamilyDraft = {
  code: "",
  name: "",
  description: "",
};

export function InventoryArticleFamilyManagerPanel() {
  const [families, setFamilies] = useState<ArticleFamily[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [newDraft, setNewDraft] = useState<FamilyDraft | null>(null);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editingDraft, setEditingDraft] = useState<FamilyDraft>(EMPTY_DRAFT);
  const [deleteTarget, setDeleteTarget] = useState<ArticleFamily | null>(null);

  const loadFamilies = async () => {
    setLoading(true);
    setError(null);
    try {
      const rows = await listInventoryArticleFamilies();
      setFamilies(rows);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadFamilies();
  }, []);

  const beginEdit = (family: ArticleFamily) => {
    setEditingId(family.id);
    setEditingDraft({
      code: family.code,
      name: family.name,
      description: family.description ?? "",
    });
  };

  const cancelEdit = () => {
    setEditingId(null);
    setEditingDraft(EMPTY_DRAFT);
  };

  const submitCreate = async () => {
    setSaving(true);
    setError(null);
    try {
      await createInventoryArticleFamily({
        code: newDraft?.code ?? "",
        name: newDraft?.name ?? "",
        description: newDraft?.description || null,
      });
      setNewDraft(null);
      await loadFamilies();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const startNewRow = () => {
    setNewDraft(EMPTY_DRAFT);
    setEditingId(null);
  };

  const cancelNewRow = () => {
    setNewDraft(null);
  };

  const submitEdit = async (familyId: number) => {
    setSaving(true);
    setError(null);
    try {
      await updateInventoryArticleFamily(familyId, {
        code: editingDraft.code,
        name: editingDraft.name,
        description: editingDraft.description || null,
        is_active: true,
      });
      cancelEdit();
      await loadFamilies();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const submitDeactivate = async () => {
    if (!deleteTarget) return;
    setSaving(true);
    setError(null);
    try {
      await deactivateInventoryArticleFamily(deleteTarget.id);
      setDeleteTarget(null);
      await loadFamilies();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between px-4 py-3 border-b border-surface-border">
        <div className="flex items-center gap-2 min-w-0">
          <span className="text-sm font-semibold text-text-primary truncate">
            Familles articles de stock
          </span>
          <Badge variant="default" className="text-[10px]">
            v1 — published
          </Badge>
        </div>
        <Button
          variant="outline"
          size="sm"
          className="gap-1.5"
          onClick={startNewRow}
          disabled={saving || !!newDraft}
        >
          <Plus className="h-3.5 w-3.5" />
          Add family
        </Button>
      </div>

      {error ? (
        <div className="px-4 py-2 bg-red-50 dark:bg-red-950/20 text-sm text-status-danger flex items-center gap-2">
          <AlertTriangle className="h-4 w-4 shrink-0" />
          {error}
        </div>
      ) : null}

      <div className="flex-1 overflow-auto">
        <table className="w-full text-sm">
          <thead className="sticky top-0 bg-surface-0 border-b border-surface-border z-10">
            <tr>
              <th className="px-3 py-2 text-left font-medium text-text-muted">Code</th>
              <th className="px-3 py-2 text-left font-medium text-text-muted">Name</th>
              <th className="px-3 py-2 text-left font-medium text-text-muted">Description</th>
              <th className="px-3 py-2 text-left font-medium text-text-muted">Status</th>
              <th className="px-3 py-2 text-right font-medium text-text-muted">Actions</th>
            </tr>
          </thead>
          <tbody>
            {newDraft && (
              <tr className="bg-primary/5 border-b border-surface-border">
                <td className="px-3 py-1.5">
                  <Input
                    placeholder="Code"
                    value={newDraft.code}
                    onChange={(e) => setNewDraft((s) => ({ ...(s ?? EMPTY_DRAFT), code: e.target.value }))}
                    className="h-7 text-sm"
                    autoFocus
                  />
                </td>
                <td className="px-3 py-1.5">
                  <Input
                    placeholder="Name"
                    value={newDraft.name}
                    onChange={(e) => setNewDraft((s) => ({ ...(s ?? EMPTY_DRAFT), name: e.target.value }))}
                    className="h-7 text-sm"
                  />
                </td>
                <td className="px-3 py-1.5">
                  <Input
                    placeholder="Description"
                    value={newDraft.description}
                    onChange={(e) =>
                      setNewDraft((s) => ({ ...(s ?? EMPTY_DRAFT), description: e.target.value }))
                    }
                    className="h-7 text-sm"
                  />
                </td>
                <td className="px-3 py-1.5">
                  <Badge variant="secondary" className="text-[10px]">
                    new
                  </Badge>
                </td>
                <td className="px-3 py-1.5 text-right">
                  <div className="flex items-center justify-end gap-1">
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-6 w-6"
                      onClick={() => void submitCreate()}
                      disabled={saving || !newDraft.code.trim() || !newDraft.name.trim()}
                    >
                      <Check className="h-3.5 w-3.5 text-status-success" />
                    </Button>
                    <Button variant="ghost" size="icon" className="h-6 w-6" onClick={cancelNewRow}>
                      <X className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                </td>
              </tr>
            )}
            {loading ? (
              <tr>
                <td className="px-3 py-3 text-text-muted" colSpan={5}>
                  Loading families...
                </td>
              </tr>
            ) : families.length === 0 ? (
              <tr>
                <td className="px-3 py-3 text-text-muted" colSpan={5}>
                  No families yet.
                </td>
              </tr>
            ) : (
              families.map((family) => {
                const isEditing = editingId === family.id;
                return (
                  <tr
                    key={family.id}
                    className={`border-b border-surface-border hover:bg-surface-1 ${
                      isEditing ? "bg-primary/5" : ""
                    }`}
                  >
                    <td className="px-3 py-1.5">
                      {isEditing ? (
                        <Input
                          className="h-7 text-sm bg-surface-1"
                          value={editingDraft.code}
                          onChange={(e) => setEditingDraft((s) => ({ ...s, code: e.target.value }))}
                        />
                      ) : (
                        <span className="font-mono text-xs">{family.code}</span>
                      )}
                    </td>
                    <td className="px-3 py-1.5">
                      {isEditing ? (
                        <Input
                          className="h-7 text-sm"
                          value={editingDraft.name}
                          onChange={(e) => setEditingDraft((s) => ({ ...s, name: e.target.value }))}
                        />
                      ) : (
                        family.name
                      )}
                    </td>
                    <td className="px-3 py-1.5">
                      {isEditing ? (
                        <Input
                          className="h-7 text-sm"
                          value={editingDraft.description}
                          onChange={(e) =>
                            setEditingDraft((s) => ({ ...s, description: e.target.value }))
                          }
                        />
                      ) : (
                        <span className="text-text-muted text-xs truncate max-w-[200px] inline-block">
                          {family.description ?? "—"}
                        </span>
                      )}
                    </td>
                    <td className="px-3 py-1.5">
                      <Badge
                        variant={family.is_active === 1 ? "default" : "secondary"}
                        className="text-[10px]"
                      >
                        {family.is_active === 1 ? "active" : "inactive"}
                      </Badge>
                    </td>
                    <td className="px-3 py-1.5 text-right">
                      {isEditing ? (
                        <div className="flex items-center justify-end gap-1">
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-6 w-6"
                            onClick={() => void submitEdit(family.id)}
                            disabled={saving || !editingDraft.code.trim() || !editingDraft.name.trim()}
                          >
                            <Check className="h-3.5 w-3.5 text-status-success" />
                          </Button>
                          <Button variant="ghost" size="icon" className="h-6 w-6" onClick={cancelEdit}>
                            <X className="h-3.5 w-3.5" />
                          </Button>
                        </div>
                      ) : (
                        <div className="flex items-center justify-end gap-1">
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-6 w-6"
                            onClick={() => beginEdit(family)}
                            disabled={saving}
                          >
                            <Pencil className="h-3.5 w-3.5" />
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-6 w-6"
                            onClick={() => setDeleteTarget(family)}
                            disabled={saving || family.is_active === 0}
                          >
                            <Trash2 className="h-3.5 w-3.5 text-status-danger" />
                          </Button>
                        </div>
                      )}
                    </td>
                  </tr>
                );
              })
            )}
          </tbody>
        </table>
      </div>

      <Dialog open={deleteTarget !== null} onOpenChange={(open) => !open && setDeleteTarget(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Deactivate family</DialogTitle>
            <DialogDescription>
              {`Deactivate article family ${deleteTarget?.code ?? ""}?`}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteTarget(null)} disabled={saving}>
              Cancel
            </Button>
            <Button variant="destructive" onClick={() => void submitDeactivate()} disabled={saving}>
              Deactivate
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
