import type { ExpressionLink } from "@cucumber/language-service";
import type { StepInfo } from "./types.ts";

export type MatchResult = {
  /** Map of step def pattern → set of step texts that matched it */
  defToSteps: Map<string, StepInfo[]>;
  /** Steps with no matching definition */
  unmatchedSteps: StepInfo[];
  /** Steps with at least one matching definition */
  matchedSteps: StepInfo[];
};

export function matchStepsToDefinitions(
  steps: StepInfo[],
  expressionLinks: ExpressionLink[],
): MatchResult {
  const defToSteps = new Map<string, StepInfo[]>();
  const unmatchedSteps: StepInfo[] = [];
  const matchedSteps: StepInfo[] = [];

  // Initialize map for all definitions
  for (const link of expressionLinks) {
    defToSteps.set(link.expression.source, []);
  }

  for (const step of steps) {
    let found = false;
    for (const link of expressionLinks) {
      try {
        const m = link.expression.match(step.text);
        if (m !== null) {
          found = true;
          defToSteps.get(link.expression.source)!.push(step);
        }
      } catch {
        // Skip expressions that error during matching
      }
    }
    if (found) {
      matchedSteps.push(step);
    } else {
      unmatchedSteps.push(step);
    }
  }

  return { defToSteps, unmatchedSteps, matchedSteps };
}
