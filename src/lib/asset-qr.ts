/**
 * What an asset identification label (QR code) carries, and how a scan is read back.
 *
 * A label is stuck on a machine and read on every device of the site, so it carries the asset
 * code — the identity people know and every replica shares — never a local database id (ids
 * differ from one device to another after synchronisation).
 *
 * - current labels: `maintafox://asset/code/<code, URI-encoded>`;
 * - labels printed before carried `maintafox://asset/<local id>`: they cannot be trusted on
 *   another device and are reported as "to reprint", never resolved to a possibly wrong asset;
 * - anything else is what the user typed or a plain barcode: it is searched as text.
 */

const CODE_PREFIX = "maintafox://asset/code/";
const LEGACY_ID = /^maintafox:\/\/asset\/\d+$/i;

export function assetQrPayload(assetCode: string): string {
  return `${CODE_PREFIX}${encodeURIComponent(assetCode)}`;
}

export type AssetScan =
  | { kind: "code"; code: string }
  | { kind: "legacy_label" }
  | { kind: "text"; text: string };

export function parseAssetScan(raw: string): AssetScan {
  const text = raw.trim();
  if (text.toLowerCase().startsWith(CODE_PREFIX)) {
    const encoded = text.slice(CODE_PREFIX.length);
    try {
      const code = decodeURIComponent(encoded).trim();
      if (code) return { kind: "code", code };
    } catch {
      // A malformed escape is not one of our labels: search it as typed.
    }
    return { kind: "text", text };
  }
  if (LEGACY_ID.test(text)) return { kind: "legacy_label" };
  return { kind: "text", text };
}
