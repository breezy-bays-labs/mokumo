import { discoverFeatureFiles, discoverStepDefFiles } from "./discover.ts";
import { parseFeatures, isExcluded } from "./parse.ts";
import { extractStepDefs } from "./extract.ts";
import { matchStepsToDefinitions } from "./match.ts";
import { findDeadSpecs, findOrphanStepDefs } from "./detect.ts";
import type { LintOptions, LintResult } from "./types.ts";

export async function lint(
  baseDir: string,
  options: LintOptions,
): Promise<LintResult> {
  // 1. Discover files
  const featureFiles = discoverFeatureFiles(baseDir, options.featureGlobs);
  const stepDefFiles = discoverStepDefFiles(baseDir, options.stepDefGlobs);

  // 2. Parse features
  const features = parseFeatures(featureFiles, options.excludeTags);
  const allSteps = features.flatMap((f) => f.steps);

  // 3. Extract step definitions
  const { stepDefs, expressionLinks } = await extractStepDefs(
    stepDefFiles,
    options.sharedStepPattern,
  );

  // 4. Match steps to definitions
  const matchResult = matchStepsToDefinitions(allSteps, expressionLinks);

  // 5. Detect dead specs and orphans
  const deadSpecs = findDeadSpecs(matchResult, options.excludeTags);
  const orphanDefs = findOrphanStepDefs(stepDefs, matchResult, options.excludeTags);

  // Count unique scenarios (excluding filtered)
  const activeScenarios = new Set<string>();
  for (const step of allSteps) {
    if (!isExcluded(step.tags, options.excludeTags)) {
      activeScenarios.add(`${step.featureFile}:${step.scenarioLine}`);
    }
  }

  const activeSteps = allSteps.filter(
    (s) => !isExcluded(s.tags, options.excludeTags),
  );

  return {
    deadSpecs,
    orphanDefs,
    stats: {
      featureFiles: featureFiles.length,
      stepDefFiles: stepDefFiles.length,
      totalScenarios: activeScenarios.size,
      totalStepDefs: stepDefs.length,
      totalSteps: activeSteps.length,
      matchedSteps: activeSteps.filter((s) =>
        matchResult.matchedSteps.some(
          (m) => m.featureFile === s.featureFile && m.line === s.line,
        ),
      ).length,
      unmatchedSteps: activeSteps.filter((s) =>
        matchResult.unmatchedSteps.some(
          (m) => m.featureFile === s.featureFile && m.line === s.line,
        ),
      ).length,
    },
  };
}
