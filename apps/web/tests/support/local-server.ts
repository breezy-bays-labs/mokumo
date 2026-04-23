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

/** Canonical regex for the "Listening on host:port" tracing log line.
 * Cross-reference: crates/mokumo-shop/tests/log_format.rs (insta snapshots) */
export const LISTENING_LOG_RE = /Listening on [^:]+:(\d+)/;

/** Canonical regex for the "Setup required — token: X" tracing log line.
 * Cross-reference: crates/mokumo-shop/tests/log_format.rs (insta snapshots) */
export const SETUP_TOKEN_RE = /Setup required — token: ([\w-]+)/;

const LEVELS_THAT_INCLUDE_INFO = new Set(["info", "debug", "trace"]);

/**
 * Build a RUST_LOG value that guarantees the setup-token and
 * listening-port info lines are visible. The "Listening on…" line is
 * emitted by `kikan::data_plane::bind::try_bind`; the "Setup required —
 * token: X" line comes from the `mokumo_server` binary itself.
 *
 * EnvFilter precedence: target-specific directives (foo=X) override
 * bare global levels (trace, debug). We only inject target=info when
 * the effective level for that target would suppress INFO.
 */
export function ensureRustLogInfoForApi(envRustLog: string | undefined): string {
  const targets = ["kikan", "mokumo_server", "mokumo_shop"];
  const directives = (envRustLog ?? "").split(",").filter(Boolean);

  const survivingTargets = new Set<string>();
  const filtered = directives.filter((d) => {
    const target = targets.find((t) => d.startsWith(`${t}=`));
    if (!target) return true;
    const level = d.split("=")[1];
    if (LEVELS_THAT_INCLUDE_INFO.has(level)) {
      survivingTargets.add(target);
      return true;
    }
    return false; // strip — this directive suppresses INFO
  });

  const globalLevel = filtered.findLast((d) => !d.includes("="));
  const globalCoversInfo = !!globalLevel && LEVELS_THAT_INCLUDE_INFO.has(globalLevel);

  const injections = targets
    .filter((t) => !survivingTargets.has(t) && !globalCoversInfo)
    .map((t) => `${t}=info`);

  if (injections.length === 0) {
    return filtered.join(",");
  }
  return [...injections, ...filtered].join(",");
}

/**
 * Parse the actual bound port from a tracing log line.
 * Matches: `Listening on <host>:<port>`
 * Strips ANSI escape codes before matching.
 */
export function parseListeningPort(line: string): number | null {
  const clean = line.replace(ANSI_RE, "");
  const match = clean.match(LISTENING_LOG_RE);
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
  const binaryPath = resolve(targetDir, "debug/mokumo-server");

  if (!existsSync(binaryPath)) {
    throw new Error(
      `Axum binary not found at ${binaryPath}. Run 'cargo build -p mokumo-server' first.`,
    );
  }
  return binaryPath;
}

export async function startAxumServer(
  webRoot: string,
  port: number,
  dataDir: string,
  wsPingMs?: number,
): Promise<{ server: ChildProcess; url: string; port: number; setupToken: string | null }> {
  const binary = resolveAxumBinary(webRoot);

  // Guarantee the "Listening on" INFO line is emitted so we can discover
  // the actual bound port. See ensureRustLogInfoForApi() for precedence rules.
  const rustLog = ensureRustLogInfoForApi(process.env.RUST_LOG);

  // --ws-ping-ms is a hidden debug-only flag (absent in release builds).
  // Only pass it when the binary path indicates a debug build to avoid
  // crashing a release binary with an unknown argument.
  const wsPingArgs =
    wsPingMs !== undefined && binary.includes("/debug/") ? ["--ws-ping-ms", String(wsPingMs)] : [];

  // mokumo-server takes: --data-dir <dir> serve --port <port>
  //                      --deployment-mode lan --host <127.0.0.1|0.0.0.0>
  // When PLAYWRIGHT_TEST_HOST is a LAN IP or other non-loopback value, bind
  // to 0.0.0.0 and append it to the LAN host allowlist — Lan mode only
  // admits loopback + mDNS by default, so a non-loopback Host header would
  // otherwise be rejected.
  const host = TEST_SERVER_HOST === "127.0.0.1" ? "127.0.0.1" : "0.0.0.0";
  const allowedHostArgs =
    TEST_SERVER_HOST === "127.0.0.1" || TEST_SERVER_HOST === "localhost"
      ? []
      : ["--allowed-host", TEST_SERVER_HOST];
  const server = spawn(
    binary,
    [
      "--data-dir",
      dataDir,
      "serve",
      "--port",
      String(port),
      "--deployment-mode",
      "lan",
      "--host",
      host,
      ...allowedHostArgs,
      ...wsPingArgs,
    ],
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
        `mokumo-server terminated before binding a port ` +
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
      `mokumo-server did not log a bound port within ${startupDeadline}ms. ` +
        `Captured output:\n${capturedOutput}`,
    );
  }

  const url = buildHttpUrl(TEST_SERVER_HOST, actualPort);
  // Use /api/health for readiness — mokumo-server is headless (no SPA fallback
  // at /) so only API routes respond. The base `url` returned to callers stays
  // without the path suffix.
  await waitForServer(`${url}/api/health`, server, "mokumo-server", startupDeadline);

  // Extract setup token from accumulated output
  const tokenMatch = capturedOutput.match(SETUP_TOKEN_RE);
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
