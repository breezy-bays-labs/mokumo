import { test as base, createBdd } from "playwright-bdd";
import type { ChildProcess } from "node:child_process";
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const STORYBOOK_PORT = 6006;
const STORYBOOK_URL = `http://localhost:${STORYBOOK_PORT}`;

type StorybookFixtures = {
  storybookUrl: string;
};

type WorkerFixtures = {
  storybookServer: void;
};

export const test = base.extend<StorybookFixtures, WorkerFixtures>({
  storybookUrl: STORYBOOK_URL,

  storybookServer: [
    // oxlint-disable-next-line eslint/no-empty-pattern -- playwright-bdd requires object destructuring
    async ({}, use) => {
      const webRoot = resolve(__dirname, "../..");
      const staticDir = resolve(webRoot, "storybook-static");
      if (!existsSync(staticDir)) {
        throw new Error(
          `storybook-static not found at ${staticDir}. Run 'moon run web:build-storybook' first.`,
        );
      }

      const server = spawn("npx", ["http-server", staticDir, "-p", String(STORYBOOK_PORT), "-s"], {
        stdio: "ignore",
        cwd: webRoot,
      });

      try {
        await waitForServer(STORYBOOK_URL, server);
        await use();
      } finally {
        server.kill("SIGTERM");
      }
    },
    { auto: true, scope: "worker" },
  ],
});

export const { Given, When, Then } = createBdd(test);

async function waitForServer(
  url: string,
  process: ChildProcess,
  timeoutMs = 15_000,
): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      const response = await fetch(url, { signal: AbortSignal.timeout(2_000) });
      if (response.ok) return;
    } catch {
      // server not ready yet
    }

    if (process.exitCode !== null) {
      throw new Error(`http-server exited with code ${process.exitCode}`);
    }

    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error(`Storybook server did not start within ${timeoutMs}ms`);
}
