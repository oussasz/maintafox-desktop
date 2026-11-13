import { describe, expect, it } from "vitest";

import type { TeamSummary } from "@shared/ipc-types";

import { positionLabels } from "../position-label";
import { teamsForEntity } from "../team-scope";

// Site 1 > workshop 2 (> cell 4), site 3 (sibling site).
const nodes = [
  { node_id: 1, ancestor_path: "/1/" },
  { node_id: 2, ancestor_path: "/1/2/" },
  { node_id: 3, ancestor_path: "/3/" },
  { node_id: 4, ancestor_path: "/1/2/4/" },
];

function team(id: number, primary: number | null): TeamSummary {
  return { id, primary_node_id: primary } as TeamSummary;
}

const teams = [team(10, 1), team(11, 2), team(12, 3), team(13, 4), team(14, null)];

describe("teamsForEntity", () => {
  it("gives a workshop the teams of its parent site", () => {
    const ids = teamsForEntity(teams, nodes, 2).map((t) => t.id);
    expect(ids).toContain(10);
    expect(ids).toContain(11);
  });
  it("leaves out the teams of a sibling site", () => {
    expect(teamsForEntity(teams, nodes, 2).map((t) => t.id)).not.toContain(12);
  });
  it("includes the teams of sub-units", () => {
    expect(teamsForEntity(teams, nodes, 2).map((t) => t.id)).toContain(13);
  });
  it("falls back to an exact match for an unknown entity", () => {
    expect(teamsForEntity(teams, nodes, 3.5).map((t) => t.id)).toEqual([]);
    expect(teamsForEntity(teams, [], 3).map((t) => t.id)).toEqual([12]);
  });
  it("leaves out teams without a unit", () => {
    expect(teamsForEntity(teams, nodes, 2).map((t) => t.id)).not.toContain(14);
  });
});

describe("positionLabels", () => {
  it("shows the name only, and the code only when two names are equal", () => {
    const labels = positionLabels([
      { id: 1, code: "POS-TECH", name: "Technicien" },
      { id: 2, code: "TECH_ELEC", name: "Électricien" },
      { id: 3, code: "TECH_2", name: "Technicien" },
    ]);
    expect(labels.get(2)).toBe("Électricien");
    expect(labels.get(1)).toBe("Technicien (POS-TECH)");
    expect(labels.get(3)).toBe("Technicien (TECH_2)");
  });
});
