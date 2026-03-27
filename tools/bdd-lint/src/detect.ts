import type { StepInfo, StepDefInfo, DeadSpec, OrphanDef, StaleWip } from "./types.ts";
import type { MatchResult } from "./match.ts";
import { isExcluded } from "./parse.ts";

export function findDeadSpecs(
  matchResult: MatchResult,
  excludeTags: string[],
): DeadSpec[] {
  // Group unmatched steps by scenario, excluding @wip etc.
  const scenarioMap = new Map<string, {
    featureFile: string;
    scenario: string;
    scenarioLine: number;
    steps: { keyword: string; text: string; line: number }[];
  }>();

  for (const step of matchResult.unmatchedSteps) {
    if (isExcluded(step.tags, excludeTags)) continue;

    const key = `${step.featureFile}:${step.scenarioLine}`;
    if (!scenarioMap.has(key)) {
      scenarioMap.set(key, {
        featureFile: step.featureFile,
        scenario: step.scenario,
        scenarioLine: step.scenarioLine,
        steps: [],
      });
    }
    scenarioMap.get(key)!.steps.push({
      keyword: step.keyword,
      text: step.text,
      line: step.line,
    });
  }

  return [...scenarioMap.values()].map((s) => ({
    featureFile: s.featureFile,
    scenario: s.scenario,
    scenarioLine: s.scenarioLine,
    unmatchedSteps: s.steps,
  }));
}

export function findOrphanStepDefs(
  stepDefs: StepDefInfo[],
  matchResult: MatchResult,
  excludeTags: string[],
): OrphanDef[] {
  const orphans: OrphanDef[] = [];

  for (const def of stepDefs) {
    const matchingSteps = matchResult.defToSteps.get(def.pattern) ?? [];

    // Filter out steps from excluded scenarios
    const activeMatches = matchingSteps.filter(
      (s) => !isExcluded(s.tags, excludeTags),
    );

    if (activeMatches.length === 0) {
      orphans.push({
        file: def.file,
        pattern: def.pattern,
        line: def.line,
      });
    }
  }

  return orphans;
}

/**
 * Find @wip scenarios where ALL steps have matching definitions.
 * These are "stale" — the @wip tag should be removed so CI runs them.
 */
export function findStaleWipScenarios(
  allSteps: StepInfo[],
  matchResult: MatchResult,
  excludeTags: string[],
): StaleWip[] {
  // Group steps by scenario, only for @wip-tagged scenarios
  const wipScenarios = new Map<string, {
    featureFile: string;
    scenario: string;
    scenarioLine: number;
    steps: StepInfo[];
  }>();

  for (const step of allSteps) {
    if (!isExcluded(step.tags, excludeTags)) continue; // only @wip scenarios

    const key = `${step.featureFile}:${step.scenarioLine}`;
    if (!wipScenarios.has(key)) {
      wipScenarios.set(key, {
        featureFile: step.featureFile,
        scenario: step.scenario,
        scenarioLine: step.scenarioLine,
        steps: [],
      });
    }
    wipScenarios.get(key)!.steps.push(step);
  }

  const matchedKeys = new Set(
    matchResult.matchedSteps.map((m) => `${m.featureFile}:${m.line}`),
  );

  const stale: StaleWip[] = [];

  for (const [, sc] of wipScenarios) {
    const allMatched = sc.steps.every((step) =>
      matchedKeys.has(`${step.featureFile}:${step.line}`),
    );

    if (allMatched && sc.steps.length > 0) {
      stale.push({
        featureFile: sc.featureFile,
        scenario: sc.scenario,
        scenarioLine: sc.scenarioLine,
        stepCount: sc.steps.length,
      });
    }
  }

  return stale;
}
