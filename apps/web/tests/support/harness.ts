import type { ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  buildHttpUrl,
  getAvailablePort,
  resolveWebRoot,
  startAxumServer,
  TEST_SERVER_HOST,
} from "./local-server";

const STOP_DRAIN_TIMEOUT_MS = 12_000;

/**
 * Owns the Axum binary lifecycle for a single test backend instance.
 *
 * Usage:
 *   const h = new BackendHarness(webRoot);
 *   await h.start();
 *   // ... interact via h.url ...
 *   await h.stop();
 *   h.cleanup();
 */
export class BackendHarness {
  private readonly _webRoot: string;
  private readonly _wsPingMs: number | undefined;
  private _process: ChildProcess | null = null;
  private _port = 0;
  private _url = "";
  private _dataDir: string | null = null;
  private _setupToken: string | null = null;
  private _tmpDirs: string[] = [];

  constructor(webRoot: string, options?: { wsPingMs?: number }) {
    this._webRoot = webRoot;
    this._wsPingMs = options?.wsPingMs;
  }

  /**
   * Start the Axum binary. If `port` is omitted, a free port is chosen.
   * If `dataDir` is omitted, a fresh temp directory is created.
   *
   * Safe to call again after stop() — reuses the same harness instance.
   */
  async start(port?: number, dataDir?: string): Promise<void> {
    const resolvedPort = port ?? (await getAvailablePort());

    let resolvedDataDir: string;
    if (dataDir !== undefined) {
      resolvedDataDir = dataDir;
    } else {
      resolvedDataDir = mkdtempSync(join(tmpdir(), "mokumo-test-"));
      this._tmpDirs.push(resolvedDataDir);
    }

    const {
      server,
      url,
      port: actualPort,
      setupToken,
    } = await startAxumServer(this._webRoot, resolvedPort, resolvedDataDir, this._wsPingMs);

    this._process = server;
    this._port = actualPort;
    this._url = url;
    this._dataDir = resolvedDataDir;
    this._setupToken = setupToken;
  }

  /**
   * Graceful shutdown: sends SIGTERM and waits up to 12 s for exit.
   * Falls back to SIGKILL if the process is still alive after the ceiling.
   */
  async stop(): Promise<void> {
    const proc = this._process;
    if (!proc || proc.exitCode !== null) return;

    proc.kill("SIGTERM");

    await Promise.race([
      new Promise<void>((resolve) => {
        proc.once("exit", resolve);
      }),
      new Promise<void>((resolve) => setTimeout(resolve, STOP_DRAIN_TIMEOUT_MS)),
    ]);

    // Ceiling exceeded — force kill if still alive
    if (proc.exitCode === null) {
      proc.kill("SIGKILL");
    }
  }

  /**
   * Send SIGTERM without waiting. For use when you don't need to observe the exit.
   */
  kill(): void {
    if (this._process?.exitCode === null) {
      this._process.kill("SIGTERM");
    }
  }

  /**
   * Send SIGKILL immediately. No-op if the process has already exited.
   * Use for kill-observe specs (SMOKE-02, SMOKE-03).
   */
  killHard(): void {
    if (this._process?.exitCode === null) {
      this._process.kill("SIGKILL");
    }
  }

  /** Base URL of the running server (e.g. `http://127.0.0.1:12345`). */
  get url(): string {
    return this._url;
  }

  /** Actual bound port (may differ from requested port if OS chose a free one). */
  get port(): number {
    return this._port;
  }

  /**
   * Data directory for the current run.
   * Capture this before stop() if you need to restart with the same SQLite DB
   * (e.g. SMOKE-12: close-to-tray preference persists across restarts).
   */
  get dataDir(): string {
    if (!this._dataDir) throw new Error("BackendHarness: start() has not been called yet");
    return this._dataDir;
  }

  /** Setup token emitted on first launch, or null if setup is already complete. */
  get setupToken(): string | null {
    return this._setupToken;
  }

  /** The underlying ChildProcess, or null before start(). */
  get process(): ChildProcess | null {
    return this._process;
  }

  /** Exit code of the process, or null if still running / not yet started. */
  get exitCode(): number | null {
    return this._process?.exitCode ?? null;
  }

  /**
   * Remove all temp directories created by this harness.
   * Call after stop() in a finally block.
   */
  cleanup(): void {
    for (const dir of this._tmpDirs) {
      rmSync(dir, { recursive: true, force: true });
    }
    this._tmpDirs = [];
  }
}

/** Resolve the webRoot path from an import.meta.url (same convention as local-server.ts). */
export function resolveHarnessWebRoot(importMetaUrl: string): string {
  return resolveWebRoot(importMetaUrl);
}

/** Build an HTTP URL for the given host and port. */
export function buildHarnessUrl(port: number): string {
  return buildHttpUrl(TEST_SERVER_HOST, port);
}
