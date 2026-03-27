import { readFileSync } from "node:fs";
import { CucumberExpression, ParameterTypeRegistry } from "@cucumber/cucumber-expressions";
import type { ExpressionLink } from "@cucumber/language-service";
import type { StepDefInfo } from "./types.ts";

export type RustExtractResult = {
  stepDefs: StepDefInfo[];
  expressionLinks: ExpressionLink[];
  warnings: string[];
};

// Matches #[given(...)], #[when(...)], #[then(...)] with expr = "..." or bare "..." or regex = r#"..."#
// Non-greedy match for r#"..."# raw strings since they can contain quotes.
const RUST_STEP_RE = /^[ \t]*#\[(given|when|then)\((?:expr\s*=\s*"([^"]+)"|"([^"]+)"|regex\s*=\s*r#"([\s\S]*?)"#)\)\]/gm;

/** Count newlines before `offset` to get a 1-based line number. */
export function lineNumberAt(content: string, offset: number): number {
  let line = 1;
  for (let i = 0; i < offset && i < content.length; i++) {
    if (content[i] === "\n") line++;
  }
  return line;
}

/** Incremental line counter — counts only the delta between calls. */
function createLineCounter() {
  let lastOffset = 0;
  let currentLine = 1;
  return (content: string, offset: number): number => {
    for (let i = lastOffset; i < offset && i < content.length; i++) {
      if (content[i] === "\n") currentLine++;
    }
    lastOffset = offset;
    return currentLine;
  };
}

export function extractRustStepDefs(files: string[]): RustExtractResult {
  const stepDefs: StepDefInfo[] = [];
  const expressionLinks: ExpressionLink[] = [];
  const warnings: string[] = [];
  const registry = new ParameterTypeRegistry();

  for (const file of files) {
    let content: string;
    try {
      content = readFileSync(file, "utf-8");
    } catch {
      warnings.push(`Could not read Rust step def file: ${file}`);
      continue;
    }

    RUST_STEP_RE.lastIndex = 0;
    const lineAt = createLineCounter();
    let match;
    while ((match = RUST_STEP_RE.exec(content)) !== null) {
      const exprPattern = match[2] ?? match[3];
      const regexPattern = match[4];
      const lineNum = lineAt(content, match.index);

      if (regexPattern) {
        warnings.push(`Skipping regex step def at ${file}:${lineNum} — regex patterns not supported for staleness matching`);
        continue;
      }

      if (!exprPattern) continue;

      try {
        const expression = new CucumberExpression(exprPattern, registry);
        // Only register the step def if the expression parses — prevents phantom orphans
        stepDefs.push({ file, pattern: exprPattern, line: lineNum, isShared: false });
        expressionLinks.push({
          expression,
          locationLink: {
            targetUri: `file://${file}`,
            targetRange: {
              start: { line: lineNum - 1, character: 0 },
              end: { line: lineNum - 1, character: 0 },
            },
            targetSelectionRange: {
              start: { line: lineNum - 1, character: 0 },
              end: { line: lineNum - 1, character: 0 },
            },
          },
        } as ExpressionLink);
      } catch (e) {
        warnings.push(
          `Could not parse Cucumber Expression at ${file}:${lineNum}: ${e instanceof Error ? e.message : String(e)}`,
        );
      }
    }
  }

  return { stepDefs, expressionLinks, warnings };
}
