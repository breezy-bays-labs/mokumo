import type { ChildProcess } from "node:child_process";
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { getPort } from "get-port-please";

export const TEST_SERVER_HOST = process.env.PLAYWRIGHT_TEST_HOST ?? "127.0.0.1";

type StaticServerOptions = {
  outputDir: string;
  outputName: string;
  prepareCommand: string;
  webRoot: string;
};

export function buildHttpUrl(host: string, port: number): string {
  return `http://${host}:${port}`;
}

export function resolveWebRoot(importMetaUrl: string): string {
  return resolve(dirname(fileURLToPath(importMetaUrl)), "../..");
}

function resolveLocalBinary(webRoot: string, binaryName: string): string {
  const binaryPath = resolve(webRoot, "node_modules/.bin", binaryName);
  if (!existsSync(binaryPath)) {
    throw new Error(`${binaryName} not found at ${binaryPath}. Run 'pnpm install' first.`);
  }

  return binaryPath;
}

async function getAvailablePort(): Promise<number> {
  return getPort({ host: TEST_SERVER_HOST, port: 0 });
}

export async function startStaticServer({
  outputDir,
  outputName,
  prepareCommand,
  webRoot,
}: StaticServerOptions): Promise<{ server: ChildProcess; url: string }> {
  if (!existsSync(outputDir)) {
    throw new Error(`${outputName} not found at ${outputDir}. Run '${prepareCommand}' first.`);
  }

  const httpServerBin = resolveLocalBinary(webRoot, "http-server");
  const port = await getAvailablePort();
  const url = buildHttpUrl(TEST_SERVER_HOST, port);

  const server = spawn(
    httpServerBin,
    [outputDir, "-a", TEST_SERVER_HOST, "-p", String(port), "-s"],
    {
      stdio: "ignore",
      cwd: webRoot,
    },
  );

  await waitForServer(url, server);

  return { server, url };
}

export async function startPreviewServer(
  webRoot: string,
): Promise<{ server: ChildProcess; url: string }> {
  const viteBin = resolveLocalBinary(webRoot, "vite");
  const port = await getAvailablePort();
  const url = buildHttpUrl(TEST_SERVER_HOST, port);

  const server = spawn(
    viteBin,
    ["preview", "--host", TEST_SERVER_HOST, "--port", String(port), "--strictPort"],
    {
      stdio: "ignore",
      cwd: webRoot,
    },
  );

  await waitForServer(url, server);

  return { server, url };
}

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

  throw new Error(`Static server did not start within ${timeoutMs}ms`);
}
