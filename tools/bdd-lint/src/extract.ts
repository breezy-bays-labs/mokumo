import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { WasmParserAdapter } from "@cucumber/language-service/wasm";
import { ExpressionBuilder } from "@cucumber/language-service";
import type { Source, LanguageName, ExpressionLink } from "@cucumber/language-service";
import type { StepDefInfo } from "./types.ts";
import { isSharedStepFile } from "./discover.ts";

let cachedAdapter: WasmParserAdapter | null = null;

export async function initParserAdapter(): Promise<WasmParserAdapter> {
  if (cachedAdapter) return cachedAdapter;

  // Resolve WASM files — they live in the package's dist/ directory
  const require = createRequire(import.meta.url);
  const langServiceEntry = require.resolve("@cucumber/language-service");
  // Entry resolves to dist/cjs/src/index.js; WASM files are in dist/
  const langServiceDir = resolve(dirname(langServiceEntry), "../..");

  const adapter = new WasmParserAdapter(langServiceDir);
  await adapter.init();
  cachedAdapter = adapter;
  return adapter;
}

export type ExtractResult = {
  stepDefs: StepDefInfo[];
  expressionLinks: ExpressionLink[];
};

export async function extractStepDefs(
  stepDefFiles: string[],
  sharedPattern: string,
): Promise<ExtractResult> {
  const adapter = await initParserAdapter();
  const builder = new ExpressionBuilder(adapter);

  // tree-sitter language name — "tsx" handles both .ts and .tsx syntax
  const sources: Source<LanguageName>[] = stepDefFiles.map((path) => ({
    languageName: "tsx" as const,
    uri: `file://${path}`,
    content: readFileSync(path, "utf-8"),
  }));

  const result = builder.build(sources, []);

  const stepDefs: StepDefInfo[] = result.expressionLinks.map((link) => {
    const uri = link.locationLink.targetUri;
    const filePath = uri.startsWith("file://") ? fileURLToPath(uri) : uri;
    const line = (link.locationLink.targetRange?.start?.line ?? 0) + 1; // 0-indexed → 1-indexed

    return {
      file: filePath,
      pattern: link.expression.source,
      line,
      isShared: isSharedStepFile(filePath, sharedPattern),
    };
  });

  return { stepDefs, expressionLinks: result.expressionLinks };
}
