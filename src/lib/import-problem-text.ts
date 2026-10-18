/**
 * Text of an import problem in the user's language, from its stable code
 * (`REF_IMPORT_*`, `PERSONNEL_IMPORT_*`) and the details it carries (row, code, list…).
 * Returns null when the problem has no code this version can say (older rows, a newer code):
 * the screen then shows the server's own sentence.
 */
type Translate = (key: string, options?: Record<string, unknown>) => string;

export interface CodedImportProblem {
  code?: string | null;
  params?: Record<string, string> | null;
  message: string;
}

export function importProblemText(problem: CodedImportProblem, t: Translate): string | null {
  const code = problem.code?.trim();
  if (!code) return null;
  const key = `import.problem.${code}`;
  const text = t(key, { ...(problem.params ?? {}), defaultValue: "" });
  return text && text !== key ? text : null;
}
