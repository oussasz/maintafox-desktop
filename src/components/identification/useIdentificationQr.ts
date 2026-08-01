import QRCode from "qrcode";
import { useEffect, useState } from "react";

const DISPLAY_QR_SIZE = 280;

/**
 * Generates an SVG string for the given identification payload while the dialog is open.
 */
export function useIdentificationQr(
  payload: string,
  enabled: boolean,
): {
  svgHtml: string;
  loading: boolean;
  error: string | null;
} {
  const [svgHtml, setSvgHtml] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!enabled || !payload) {
      setSvgHtml("");
      setError(null);
      setLoading(false);
      return;
    }

    let cancelled = false;
    setLoading(true);
    setError(null);

    void QRCode.toString(payload, {
      type: "svg",
      width: DISPLAY_QR_SIZE,
      margin: 1,
      errorCorrectionLevel: "M",
    })
      .then((svg: string) => {
        if (!cancelled) {
          setSvgHtml(svg);
          setLoading(false);
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setSvgHtml("");
          setError(err instanceof Error ? err.message : String(err));
          setLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [enabled, payload]);

  return { svgHtml, loading, error };
}

export const IDENTIFICATION_QR_DISPLAY_SIZE = DISPLAY_QR_SIZE;
