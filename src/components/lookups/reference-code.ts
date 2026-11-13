/**
 * Proposes a reference value code from its label (L5-18): the user types "Pompe centrifuge",
 * the code field shows POMPE_CENTRIFUGE and stays editable. Same rules as the server
 * (`validate_code`): uppercase ASCII letters, digits, "_" and ".", starts with a letter, ≤ 64.
 */
export function codeFromLabel(label: string): string {
  const ascii = label
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .replace(/[œŒ]/g, "OE")
    .replace(/[æÆ]/g, "AE")
    .toUpperCase();
  const code = ascii
    .replace(/[^A-Z0-9]+/g, "_")
    .replace(/^[^A-Z]+/, "")
    .replace(/_+$/, "");
  return code.slice(0, 64).replace(/_+$/, "");
}
