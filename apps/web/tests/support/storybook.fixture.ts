import { test as base, createBdd } from "playwright-bdd";
import type { ChildProcess } from "node:child_process";
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { getPort } from "get-port-please";

const __dirname = dirname(fileURLToPath(import.meta.url));

type WorkerFixtures = {
  storybookUrl: string;
};

export const test = base.extend<object, WorkerFixtures>({
  storybookUrl: [
    // oxlint-disable-next-line eslint/no-empty-pattern -- playwright-bdd requires object destructuring
    async ({}, use) => {
      const webRoot = resolve(__dirname, "../..");
      const staticDir = resolve(webRoot, "storybook-static");
      if (!existsSync(staticDir)) {
        throw new Error(
          `storybook-static not found at ${staticDir}. Run 'moon run web:build-storybook' first.`,
        );
      }

      const port = await getPort({ random: true });
      const url = `http://localhost:${port}`;

      const httpServerBin = resolve(webRoot, "node_modules/.bin/http-server");
      const server = spawn(httpServerBin, [staticDir, "-p", String(port), "-s"], {
        stdio: "ignore",
        cwd: webRoot,
      });

      try {
        await waitForServer(url, server);
        await use(url);
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
