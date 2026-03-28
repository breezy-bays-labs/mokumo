import type { ChildProcess } from "node:child_process";
import { spawn } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
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

export async function getAvailablePort(): Promise<number> {
  return getPort({ host: TEST_SERVER_HOST, port: 0 });
}

// oxlint-disable-next-line no-control-regex -- intentional: stripping ANSI escape sequences
const ANSI_RE = /\x1b\[[0-9;]*m/g;

/**
 * Parse the actual bound port from a tracing log line.
 * Matches: `Listening on <host>:<port>`
 * Strips ANSI escape codes before matching.
 */
export function parseListeningPort(line: string): number | null {
  const clean = line.replace(ANSI_RE, "");
  const match = clean.match(/Listening on [^:]+:(\d+)/);
  if (!match) return null;
  const port = Number(match[1]);
  if (!Number.isFinite(port) || port < 1 || port > 65535) return null;
  return port;
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

  await waitForServer(url, server, "http-server");

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

  await waitForServer(url, server, "vite preview");

  return { server, url };
}

function resolveTargetDir(workspaceRoot: string): string {
  // 1. Environment variable (CI sets this — may be relative)
  if (process.env.CARGO_TARGET_DIR) {
    return resolve(workspaceRoot, process.env.CARGO_TARGET_DIR);
  }

  // 2. .cargo/config.toml redirect (worktrees share target/)
  const cargoConfig = resolve(workspaceRoot, ".cargo/config.toml");
  if (existsSync(cargoConfig)) {
    const content = readFileSync(cargoConfig, "utf-8");
    const match = content.match(/target-dir\s*=\s*"([^"]+)"/);
    if (match) return resolve(workspaceRoot, match[1]);
  }

  // 3. Default: workspace-relative
  return resolve(workspaceRoot, "target");
}

function resolveAxumBinary(webRoot: string): string {
  const workspaceRoot = resolve(webRoot, "../..");
  const targetDir = resolveTargetDir(workspaceRoot);
  const binaryPath = resolve(targetDir, "debug/mokumo-api");

  if (!existsSync(binaryPath)) {
    throw new Error(`Axum binary not found at ${binaryPath}. Run 'moon run api:build' first.`);
  }
  return binaryPath;
}

export async function startAxumServer(
  webRoot: string,
  port: number,
  dataDir: string,
): Promise<{ server: ChildProcess; url: string; port: number; setupToken: string | null }> {
  const binary = resolveAxumBinary(webRoot);

  const server = spawn(
    binary,
    ["--port", String(port), "--data-dir", dataDir, "--host", TEST_SERVER_HOST],
    {
      stdio: ["ignore", "pipe", "pipe"],
      cwd: webRoot,
    },
  );

  // Capture setup token and actual bound port from stdout/stderr.
  // tracing_subscriber::fmt() writes to stderr by default, so watch both streams.
  // Accumulate output to handle Buffer chunk splitting across line boundaries.
  let setupToken: string | null = null;
  let actualPort: number | null = null;
  let capturedOutput = "";
  const captureStartupInfo = (data: Buffer) => {
    const chunk = data.toString();
    capturedOutput += chunk;
    if (!setupToken) {
      const tokenMatch = chunk.match(/Setup required — token: ([\w-]+)/);
      if (tokenMatch) setupToken = tokenMatch[1];
    }
    if (actualPort === null) {
      actualPort = parseListeningPort(chunk);
    }
  };
  server.stdout?.on("data", captureStartupInfo);
  server.stderr?.on("data", captureStartupInfo);

  // Wait for the "Listening on" log line to discover the actual bound port.
  // This eliminates the TOCTOU race: we poll the port Axum actually bound,
  // not the port we requested.
  const startupDeadline = 30_000;
  const startTime = Date.now();
  // oxlint-disable-next-line no-unmodified-loop-condition -- actualPort is mutated by the captureStartupInfo callback on data events
  while (actualPort === null && Date.now() - startTime < startupDeadline) {
    if (server.exitCode !== null) {
      throw new Error(`mokumo-api exited with code ${server.exitCode} before binding a port`);
    }
    await new Promise((r) => setTimeout(r, 100));
  }

  // Re-check accumulated output in case the line was split across chunks
  if (actualPort === null) {
    for (const line of capturedOutput.split("\n")) {
      actualPort = parseListeningPort(line);
      if (actualPort !== null) break;
    }
  }

  if (actualPort === null) {
    server.kill("SIGTERM");
    throw new Error(
      `mokumo-api did not log a bound port within ${startupDeadline}ms. ` +
        `Captured output:\n${capturedOutput}`,
    );
  }

  const url = buildHttpUrl(TEST_SERVER_HOST, actualPort);
  await waitForServer(url, server, "mokumo-api", startupDeadline);

  // Re-check full accumulated output for setup token split across chunks
  if (!setupToken) {
    const match = capturedOutput.match(/Setup required — token: ([\w-]+)/);
    if (match) setupToken = match[1];
  }

  return { server, url, port: actualPort, setupToken };
}

export async function waitForServer(
  url: string,
  process: ChildProcess,
  processName = "http-server",
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
      throw new Error(`${processName} exited with code ${process.exitCode}`);
    }

    await new Promise((r) => setTimeout(r, 250));
  }

  throw new Error(`${processName} did not start within ${timeoutMs}ms at ${url}`);
}
