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

const LEVELS_THAT_INCLUDE_INFO = new Set(["info", "debug", "trace"]);

/**
 * Build a RUST_LOG value that guarantees mokumo_api emits at INFO level,
 * while preserving the caller's other directives and not downgrading
 * debug/trace levels for mokumo_api.
 *
 * EnvFilter precedence: target-specific directives (mokumo_api=X) override
 * bare global levels (trace, debug). We only inject mokumo_api=info when
 * the effective level for that target would suppress INFO.
 */
export function ensureRustLogInfoForApi(envRustLog: string | undefined): string {
  const directives = (envRustLog ?? "").split(",").filter(Boolean);

  // EnvFilter uses last-wins for duplicate targets, so we must process ALL
  // mokumo_api directives. Strip every one that suppresses INFO; keep those
  // that include it. If any survive, no injection is needed.
  let hasSurvivingMokumo = false;
  const filtered = directives.filter((d) => {
    if (!d.startsWith("mokumo_api=")) return true;
    const level = d.split("=")[1];
    if (LEVELS_THAT_INCLUDE_INFO.has(level)) {
      hasSurvivingMokumo = true;
      return true;
    }
    return false; // strip — this directive suppresses INFO
  });

  if (hasSurvivingMokumo) {
    return filtered.join(",");
  }

  // No mokumo_api directive survives. Check if the effective bare global level
  // covers INFO. EnvFilter uses last-wins, so use findLast.
  const globalLevel = filtered.findLast((d) => !d.includes("="));
  if (globalLevel && LEVELS_THAT_INCLUDE_INFO.has(globalLevel)) {
    return filtered.join(",");
  }
  return ["mokumo_api=info", ...filtered].join(",") || "mokumo_api=info";
}

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

  // Guarantee the "Listening on" INFO line is emitted so we can discover
  // the actual bound port. See ensureRustLogInfoForApi() for precedence rules.
  const rustLog = ensureRustLogInfoForApi(process.env.RUST_LOG);

  const server = spawn(
    binary,
    ["--port", String(port), "--data-dir", dataDir, "--host", TEST_SERVER_HOST],
    {
      stdio: ["ignore", "pipe", "pipe"],
      cwd: webRoot,
      env: { ...process.env, RUST_LOG: rustLog },
    },
  );

  // Capture setup token and actual bound port from stdout/stderr.
  // tracing_subscriber::fmt() writes to stderr by default, so watch both streams.
  // The callback only accumulates output — port parsing happens in the polling
  // loop below, which scans only *complete* lines (terminated by \n) to avoid
  // accepting a truncated port from a mid-line chunk boundary.
  let setupToken: string | null = null;
  let actualPort: number | null = null;
  let capturedOutput = "";
  const captureOutput = (data: Buffer) => {
    capturedOutput += data.toString();
  };
  server.stdout?.on("data", captureOutput);
  server.stderr?.on("data", captureOutput);

  // Wait for the "Listening on" log line to discover the actual bound port.
  // This eliminates the TOCTOU race: we poll the port Axum actually bound,
  // not the port we requested.
  const startupDeadline = 30_000;
  const startTime = Date.now();
  while (actualPort === null && Date.now() - startTime < startupDeadline) {
    if (server.exitCode !== null || server.signalCode !== null) {
      throw new Error(
        `mokumo-api terminated before binding a port ` +
          `(exitCode=${server.exitCode}, signal=${server.signalCode}). ` +
          `Output:\n${capturedOutput}`,
      );
    }
    // Parse only complete lines (all elements except the last, which may be
    // an unterminated fragment). This prevents accepting a truncated port
    // when a chunk boundary splits "Listening on 127.0.0.1:53" + "578\n".
    const lines = capturedOutput.split("\n");
    for (let i = 0; i < lines.length - 1; i++) {
      actualPort = parseListeningPort(lines[i]);
      if (actualPort !== null) break;
    }
    if (actualPort === null) {
      await new Promise((r) => setTimeout(r, 100));
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

  // Extract setup token from accumulated output
  const tokenMatch = capturedOutput.match(/Setup required — token: ([\w-]+)/);
  if (tokenMatch) setupToken = tokenMatch[1];

  return { server, url, port: actualPort, setupToken };
}

/**
 * Returns true if the error is an expected "server not ready yet" error that
 * should be retried: connection refused, or AbortSignal timeout on the fetch.
 * All other errors (malformed URL, DNS failure, unexpected runtime errors)
 * propagate immediately so they aren't silently retried until timeout.
 */
export function isExpectedServerNotReady(error: unknown): boolean {
  // AbortSignal.timeout() throws DOMException with name "TimeoutError"
  if (error instanceof DOMException && error.name === "TimeoutError") return true;

  // Connection refused: TypeError with cause.code === "ECONNREFUSED" (undici/Node)
  if (error instanceof TypeError) {
    const cause = (error as TypeError & { cause?: { code?: string } }).cause;
    if (!cause) return false; // e.g. Invalid URL TypeError has no cause

    if (cause.code === "ECONNREFUSED" || cause.code === "ECONNRESET") return true;
  }

  return false;
}

export async function waitForServer(
  url: string,
  process: ChildProcess,
  processName = "http-server",
  timeoutMs = 15_000,
): Promise<void> {
  const start = Date.now();
  let lastError: unknown = null;
  while (Date.now() - start < timeoutMs) {
    try {
      const response = await fetch(url, { signal: AbortSignal.timeout(2_000) });
      if (response.ok) return;
    } catch (error) {
      if (!isExpectedServerNotReady(error)) throw error;
      lastError = error;
    }

    if (process.exitCode !== null) {
      throw new Error(`${processName} exited with code ${process.exitCode}`);
    }

    await new Promise((r) => setTimeout(r, 250));
  }

  const lastErrorMsg = lastError instanceof Error ? lastError.message : String(lastError);
  throw new Error(
    `${processName} did not start within ${timeoutMs}ms at ${url} (last error: ${lastErrorMsg})`,
  );
}
