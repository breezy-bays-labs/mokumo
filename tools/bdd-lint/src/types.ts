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

export type StaleWip = {
  featureFile: string;
  scenario: string;
  scenarioLine: number;
  stepCount: number;
};

export type LintResult = {
  deadSpecs: DeadSpec[];
  orphanDefs: OrphanDef[];
  staleWipScenarios: StaleWip[];
  warnings: string[];
  stats: {
    featureFiles: number;
    stepDefFiles: number;
    totalScenarios: number;
    totalStepDefs: number;
    totalSteps: number;
    matchedSteps: number;
    unmatchedSteps: number;
    wipScenarios: number;
    staleWipScenarios: number;
  };
};

export type LintOptions = {
  featureGlobs: string[];
  stepDefGlobs: string[];
  rustStepDefGlobs: string[];
  sharedStepPattern: string;
  excludeTags: string[];
  format: "text" | "json" | "ci";
};
