import { readFileSync } from "node:fs";
import * as Gherkin from "@cucumber/gherkin";
import * as Messages from "@cucumber/messages";
import type { StepInfo } from "./types.ts";

export type ParsedFeature = {
  file: string;
  name: string;
  steps: StepInfo[];
};

export type ParseFeaturesResult = {
  features: ParsedFeature[];
  warnings: string[];
};

export function parseFeatures(
  featureFiles: string[],
  excludeTags: string[],
): ParseFeaturesResult {
  const uuidFn = Messages.IdGenerator.uuid();
  const results: ParsedFeature[] = [];
  const warnings: string[] = [];

  for (const file of featureFiles) {
    const content = readFileSync(file, "utf-8");
    const astBuilder = new Gherkin.AstBuilder(uuidFn);
    const matcher = new Gherkin.GherkinClassicTokenMatcher();
    const parser = new Gherkin.Parser(astBuilder, matcher);

    let doc;
    try {
      doc = parser.parse(content);
    } catch (e) {
      warnings.push(`Failed to parse feature file: ${file}: ${e instanceof Error ? e.message : String(e)}`);
      continue;
    }

    if (!doc.feature) continue;

    const featureTags = doc.feature.tags.map((t) => t.name);
    const featureExcluded = featureTags.some((t) => excludeTags.includes(t));

    const steps: StepInfo[] = [];

    for (const child of doc.feature.children) {
      if (!child.scenario) continue;

      const scenarioTags = [
        ...featureTags,
        ...child.scenario.tags.map((t) => t.name),
      ];
      const excluded = featureExcluded ||
        child.scenario.tags.some((t) => excludeTags.includes(t.name));

      for (const step of child.scenario.steps) {
        steps.push({
          featureFile: file,
          featureName: doc.feature.name,
          scenario: child.scenario.name,
          scenarioLine: child.scenario.location.line,
          keyword: step.keyword.trim(),
          text: step.text,
          line: step.location.line,
          tags: scenarioTags,
        });
      }
    }

    results.push({
      file,
      name: doc.feature.name,
      steps,
    });
  }

  return { features: results, warnings };
}

export function isExcluded(tags: string[], excludeTags: string[]): boolean {
  return tags.some((t) => excludeTags.includes(t));
}
