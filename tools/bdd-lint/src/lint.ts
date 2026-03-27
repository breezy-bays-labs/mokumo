import { discoverFeatureFiles, discoverStepDefFiles } from "./discover.ts";
import { parseFeatures, isExcluded } from "./parse.ts";
import { extractStepDefs } from "./extract.ts";
import { extractRustStepDefs } from "./extract-rust.ts";
import { matchStepsToDefinitions, type MatchResult } from "./match.ts";
import { findDeadSpecs, findOrphanStepDefs, findStaleWipScenarios } from "./detect.ts";
import type { LintOptions, LintResult, StepInfo } from "./types.ts";

/** Build a Set of "file:line" keys for O(1) step identity lookups. */
function stepKeySet(steps: StepInfo[]): Set<string> {
  return new Set(steps.map((s) => `${s.featureFile}:${s.line}`));
}

export async function lint(
  baseDir: string,
  options: LintOptions,
): Promise<LintResult> {
  const featureFiles = discoverFeatureFiles(baseDir, options.featureGlobs);
  const stepDefFiles = discoverStepDefFiles(baseDir, options.stepDefGlobs);
  const rustStepDefFiles = discoverStepDefFiles(baseDir, options.rustStepDefGlobs);

  // Includes @wip scenarios — we need them for staleness check
  const { features, warnings: parseWarnings } = parseFeatures(featureFiles, options.excludeTags);
  const allSteps = features.flatMap((f) => f.steps);

  const { stepDefs, expressionLinks } = await extractStepDefs(
    stepDefFiles,
    options.sharedStepPattern,
  );
  const rustResult = extractRustStepDefs(rustStepDefFiles);
  const allExpressionLinks = [...expressionLinks, ...rustResult.expressionLinks];

  const matchResult = matchStepsToDefinitions(allSteps, allExpressionLinks);

  const deadSpecs = findDeadSpecs(matchResult, options.excludeTags);
  const orphanDefs = findOrphanStepDefs(stepDefs, matchResult, options.excludeTags);
  const staleWipScenarios = findStaleWipScenarios(allSteps, matchResult, options.excludeTags);

  const activeScenarios = new Set<string>();
  const wipScenarios = new Set<string>();
  for (const step of allSteps) {
    const key = `${step.featureFile}:${step.scenarioLine}`;
    if (isExcluded(step.tags, options.excludeTags)) {
      wipScenarios.add(key);
    } else {
      activeScenarios.add(key);
    }
  }

  const activeSteps = allSteps.filter(
    (s) => !isExcluded(s.tags, options.excludeTags),
  );

  const matchedKeys = stepKeySet(matchResult.matchedSteps);
  const matchedCount = activeSteps.filter((s) => matchedKeys.has(`${s.featureFile}:${s.line}`)).length;

  return {
    deadSpecs,
    orphanDefs,
    staleWipScenarios,
    warnings: [...parseWarnings, ...matchResult.warnings, ...rustResult.warnings],
    stats: {
      featureFiles: featureFiles.length,
      stepDefFiles: stepDefFiles.length + rustStepDefFiles.length,
      totalScenarios: activeScenarios.size,
      totalStepDefs: stepDefs.length + rustResult.stepDefs.length,
      totalSteps: activeSteps.length,
      matchedSteps: matchedCount,
      unmatchedSteps: activeSteps.length - matchedCount,
      wipScenarios: wipScenarios.size,
      staleWipScenarios: staleWipScenarios.length,
    },
  };
}
