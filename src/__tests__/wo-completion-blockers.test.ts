/**
 * Unit tests for wo-service preflight blocker mapping.
 *
 * Sprint S4 V2 (frontend): verifies that normalizePreflightCode correctly
 * maps backend error messages (bilingual EN/FR) to structured codes, and
 * that CompletionBlockedError carries the mapped preflight errors.
 */

import { describe, it, expect } from "vitest";

import { CompletionBlockedError, normalizePreflightCode } from "@/services/wo-service";

describe("normalizePreflightCode", () => {
  it("maps English open-labor message to OPEN_LABOR", () => {
    expect(normalizePreflightCode("Open labor entries must be closed first.")).toBe("OPEN_LABOR");
  });

  it("maps French open-labor message to OPEN_LABOR", () => {
    expect(normalizePreflightCode("main-d'œuvre ouvertes doivent être fermées")).toBe("OPEN_LABOR");
  });

  it("maps English mandatory-tasks message to INCOMPLETE_TASKS", () => {
    expect(normalizePreflightCode("Mandatory tasks incomplete: Task A")).toBe("INCOMPLETE_TASKS");
  });

  it("maps French mandatory-tasks message to INCOMPLETE_TASKS", () => {
    expect(normalizePreflightCode("tâches obligatoires incomplètes")).toBe("INCOMPLETE_TASKS");
  });

  it("maps English parts-actuals message to MISSING_PARTS", () => {
    expect(normalizePreflightCode("Parts actuals not confirmed.")).toBe("MISSING_PARTS");
  });

  it("maps French pièces message to MISSING_PARTS", () => {
    expect(normalizePreflightCode("pièces non confirmées")).toBe("MISSING_PARTS");
  });

  it("maps English open-downtime message to OPEN_DOWNTIME", () => {
    expect(normalizePreflightCode("Open downtime segments must be closed.")).toBe("OPEN_DOWNTIME");
  });

  it("maps French temps d'arrêt message to OPEN_DOWNTIME", () => {
    expect(normalizePreflightCode("temps d'arrêt ouverts doivent être fermés")).toBe(
      "OPEN_DOWNTIME",
    );
  });

  it("returns BLOCKING_ERROR for unrecognized messages", () => {
    expect(normalizePreflightCode("Something unexpected happened")).toBe("BLOCKING_ERROR");
    expect(normalizePreflightCode("")).toBe("BLOCKING_ERROR");
  });

  it("is case-insensitive", () => {
    expect(normalizePreflightCode("OPEN LABOR entries")).toBe("OPEN_LABOR");
    expect(normalizePreflightCode("Mandatory Tasks Incomplete")).toBe("INCOMPLETE_TASKS");
  });
});

describe("CompletionBlockedError", () => {
  it("carries mapped preflight errors from multiple backend messages", () => {
    const errors = [
      { code: "OPEN_LABOR", message: "Open labor entries must be closed first." },
      { code: "INCOMPLETE_TASKS", message: "Mandatory tasks incomplete: Lubricate bearing" },
    ];
    const err = new CompletionBlockedError(errors);

    expect(err.name).toBe("CompletionBlockedError");
    expect(err.errors).toHaveLength(2);
    expect(err.errors[0]?.code).toBe("OPEN_LABOR");
    expect(err.errors[1]?.code).toBe("INCOMPLETE_TASKS");
    expect(err.message).toMatch(/blocked/i);
  });

  it("is an instance of Error", () => {
    const err = new CompletionBlockedError([]);
    expect(err).toBeInstanceOf(Error);
  });
});
