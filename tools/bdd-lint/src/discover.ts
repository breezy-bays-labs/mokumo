import { globSync } from "node:fs";

function discoverFiles(baseDir: string, globs: string[]): string[] {
  const files: string[] = [];
  for (const pattern of globs) {
    files.push(...globSync(pattern, { cwd: baseDir }));
  }
  return [...new Set(files)].sort().map((f) => `${baseDir}/${f}`);
}

export function discoverFeatureFiles(
  baseDir: string,
  globs: string[],
): string[] {
  return discoverFiles(baseDir, globs);
}

export function discoverStepDefFiles(
  baseDir: string,
  globs: string[],
): string[] {
  return discoverFiles(baseDir, globs);
}

export function isSharedStepFile(
  filePath: string,
  pattern: string,
): boolean {
  const basename = filePath.split("/").pop() ?? "";
  // Default pattern: *-shared.steps.ts
  const regex = new RegExp(
    pattern.replace("*", ".*").replace("{feature}", "[^/]+"),
  );
  return regex.test(basename);
}
