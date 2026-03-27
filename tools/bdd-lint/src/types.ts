export type StepInfo = {
  featureFile: string;
  featureName: string;
  scenario: string;
  scenarioLine: number;
  keyword: string;
  text: string;
  line: number;
  tags: string[];
};

export type StepDefInfo = {
  file: string;
  pattern: string;
  line: number;
  isShared: boolean;
};

export type DeadSpec = {
  featureFile: string;
  scenario: string;
  scenarioLine: number;
  unmatchedSteps: { keyword: string; text: string; line: number }[];
};

export type OrphanDef = {
  file: string;
  pattern: string;
  line: number;
};

export type LintResult = {
  deadSpecs: DeadSpec[];
  orphanDefs: OrphanDef[];
  warnings: string[];
  stats: {
    featureFiles: number;
    stepDefFiles: number;
    totalScenarios: number;
    totalStepDefs: number;
    totalSteps: number;
    matchedSteps: number;
    unmatchedSteps: number;
  };
};

export type LintOptions = {
  featureGlobs: string[];
  stepDefGlobs: string[];
  sharedStepPattern: string;
  excludeTags: string[];
  format: "text" | "json" | "ci";
};
