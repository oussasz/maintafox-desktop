import { Loader2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { DocumentHeaderEditor } from "@/components/editor/DocumentHeaderEditor";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { describeAppError } from "@/lib/app-error";
import {
  isDocumentHeaderHtmlEmpty,
  sanitizeDocumentHeaderHtml,
} from "@/lib/document-header-sanitize";
import {
  clearDocumentBrandingLogo,
  DOCUMENT_HEADER_SETTING_KEY,
  EMPTY_DOCUMENT_HEADER,
  getSetting,
  parseDocumentHeaderValue,
  readDocumentBrandingLogoPreview,
  setSetting,
  uploadDocumentBrandingLogo,
  type DocumentHeaderSettingValue,
} from "@/services/settings-service";

interface DocumentHeaderSettingsPanelProps {
  onToast: (msg: {
    title: string;
    description?: string;
    variant?: "default" | "destructive" | "success";
  }) => void;
  onSaved?: () => void;
}

export function DocumentHeaderSettingsPanel({
  onToast,
  onSaved,
}: DocumentHeaderSettingsPanelProps) {
  const { t } = useTranslation("settings");

  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [logoBusy, setLogoBusy] = useState(false);
  const [value, setValue] = useState<DocumentHeaderSettingValue>({ ...EMPTY_DOCUMENT_HEADER });
  const [draftHtml, setDraftHtml] = useState("");
  const [logoPreviewUrl, setLogoPreviewUrl] = useState<string | null>(null);
  const [pendingInsertSrc, setPendingInsertSrc] = useState<string | null>(null);
  const [loadError, setLoadError] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setLoadError(false);
    try {
      const row = await getSetting(DOCUMENT_HEADER_SETTING_KEY, "tenant");
      const rawJson = row?.setting_value_json ?? null;
      if (rawJson) {
        try {
          JSON.parse(rawJson);
        } catch {
          onToast({ title: t("documentHeader.invalidShape"), variant: "destructive" });
        }
      }
      const parsed = parseDocumentHeaderValue(rawJson);
      setValue(parsed);
      setDraftHtml(parsed.html);

      const preview = await readDocumentBrandingLogoPreview().catch(() => null);
      if (preview) {
        setLogoPreviewUrl(`data:${preview.mime_type};base64,${preview.data_base64}`);
      } else {
        setLogoPreviewUrl(null);
      }
    } catch (err) {
      console.error("[DocumentHeaderSettingsPanel] load failed:", err);
      setLoadError(true);
      onToast({ title: t("documentHeader.loadError"), variant: "destructive" });
    } finally {
      setLoading(false);
    }
  }, [onToast, t]);

  useEffect(() => {
    void load();
  }, [load]);

  const handleUploadLogo = useCallback(async () => {
    setLogoBusy(true);
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        multiple: false,
        filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "gif"] }],
      });
      if (!selected) return;
      const sourcePath =
        typeof selected === "string" ? selected : (selected as { path?: string }).path;
      if (!sourcePath) return;

      const result = await uploadDocumentBrandingLogo(sourcePath);
      const dataUrl = `data:${result.mime_type};base64,${result.data_base64}`;
      setLogoPreviewUrl(dataUrl);
      setValue((prev) => ({ ...prev, logo_relative_path: result.relative_path }));
      setPendingInsertSrc(dataUrl);
    } catch (err) {
      console.error("[DocumentHeaderSettingsPanel] logo upload failed:", err);
      onToast({
        title: t("documentHeader.logoUploadError"),
        description: describeAppError(err).title,
        variant: "destructive",
      });
    } finally {
      setLogoBusy(false);
    }
  }, [onToast, t]);

  const handleClearLogo = useCallback(async () => {
    setLogoBusy(true);
    try {
      await clearDocumentBrandingLogo();
      setLogoPreviewUrl(null);
      setValue((prev) => ({ ...prev, logo_relative_path: null }));
    } catch (err) {
      console.error("[DocumentHeaderSettingsPanel] logo clear failed:", err);
      onToast({
        title: t("documentHeader.logoClearError"),
        description: describeAppError(err).title,
        variant: "destructive",
      });
    } finally {
      setLogoBusy(false);
    }
  }, [onToast, t]);

  const handleSave = useCallback(async () => {
    setSaving(true);
    try {
      const sanitized = sanitizeDocumentHeaderHtml(draftHtml);
      const payload: DocumentHeaderSettingValue = {
        version: 1,
        content_json: value.content_json,
        html: sanitized,
        logo_relative_path: value.logo_relative_path,
      };
      await setSetting({
        key: DOCUMENT_HEADER_SETTING_KEY,
        scope: "tenant",
        value_json: JSON.stringify(payload),
        change_summary: t("documentHeader.save"),
      });
      setValue(payload);
      setDraftHtml(sanitized);
      onToast({ title: t("documentHeader.saveSuccess"), variant: "success" });
      onSaved?.();
    } catch (err) {
      console.error("[DocumentHeaderSettingsPanel] save failed:", err);
      onToast({
        title: t("documentHeader.saveError"),
        description: describeAppError(err).title,
        variant: "destructive",
      });
    } finally {
      setSaving(false);
    }
  }, [draftHtml, onSaved, onToast, t, value.content_json, value.logo_relative_path]);

  const previewHtml = sanitizeDocumentHeaderHtml(draftHtml);
  const previewEmpty = isDocumentHeaderHtmlEmpty(previewHtml);

  return (
    <Card className="shadow-none">
      <CardHeader className="space-y-1 px-5 py-4">
        <CardTitle className="text-base font-semibold leading-snug tracking-normal">
          {t("documentHeader.title")}
        </CardTitle>
        <CardDescription className="text-sm leading-relaxed">
          {t("documentHeader.description")}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-5 px-5 pb-5 pt-0">
        {loading ? (
          <div className="flex items-center gap-2 py-8 text-sm text-text-muted">
            <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
            {t("page.loading")}
          </div>
        ) : loadError ? (
          <div className="flex flex-wrap items-center gap-2">
            <p className="text-sm text-destructive">{t("documentHeader.loadError")}</p>
            <Button type="button" size="sm" variant="outline" onClick={() => void load()}>
              {t("page.retry")}
            </Button>
          </div>
        ) : (
          <>
            <div className="space-y-2">
              <p className="text-sm font-medium text-text-primary">
                {t("documentHeader.logoLabel")}
              </p>
              <p className="text-xs text-text-muted">{t("documentHeader.logoHint")}</p>
              <div className="flex flex-wrap items-center gap-3">
                {logoPreviewUrl ? (
                  <img
                    src={logoPreviewUrl}
                    alt={t("documentHeader.logoLabel")}
                    className="h-14 max-w-[10rem] rounded border border-surface-border bg-white object-contain p-1"
                  />
                ) : null}
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  disabled={logoBusy || saving}
                  onClick={() => void handleUploadLogo()}
                >
                  {logoBusy ? <Loader2 className="mr-2 h-4 w-4 animate-spin" aria-hidden /> : null}
                  {logoPreviewUrl
                    ? t("documentHeader.replaceLogo")
                    : t("documentHeader.uploadLogo")}
                </Button>
                {logoPreviewUrl ? (
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    disabled={logoBusy || saving}
                    onClick={() => void handleClearLogo()}
                  >
                    {t("documentHeader.removeLogo")}
                  </Button>
                ) : null}
              </div>
            </div>

            <div className="space-y-2">
              <p className="text-sm font-medium text-text-primary">
                {t("documentHeader.editorLabel")}
              </p>
              <DocumentHeaderEditor
                contentJson={value.content_json}
                disabled={saving}
                insertImageSrc={pendingInsertSrc}
                onInsertImageConsumed={() => setPendingInsertSrc(null)}
                onChange={({ contentJson, html }) => {
                  setValue((prev) => ({ ...prev, content_json: contentJson }));
                  setDraftHtml(html);
                }}
              />
            </div>

            <div className="space-y-2">
              <p className="text-sm font-medium text-text-primary">
                {t("documentHeader.previewLabel")}
              </p>
              {previewEmpty ? (
                <p className="rounded-md border border-dashed border-surface-border bg-muted/20 px-3 py-4 text-sm text-text-muted">
                  {t("documentHeader.previewEmpty")}
                </p>
              ) : (
                <div
                  className="document-header-preview rounded-md border border-surface-border bg-white px-4 py-3 text-sm text-black [&_img]:max-h-20 [&_img]:max-w-full"
                  dangerouslySetInnerHTML={{ __html: previewHtml }}
                />
              )}
            </div>

            <div className="flex justify-end">
              <Button type="button" disabled={saving || logoBusy} onClick={() => void handleSave()}>
                {saving ? (
                  <>
                    <Loader2 className="mr-2 h-4 w-4 animate-spin" aria-hidden />
                    {t("documentHeader.saving")}
                  </>
                ) : (
                  t("documentHeader.save")
                )}
              </Button>
            </div>
          </>
        )}
      </CardContent>
    </Card>
  );
}
