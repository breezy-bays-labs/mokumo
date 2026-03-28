/**
 * Seed script: spins up a fresh Axum server, runs setup wizard, creates
 * demo customers via API, then produces a pre-seeded demo.db file.
 *
 * Usage: pnpm tsx scripts/seed-demo.ts
 */

import { copyFileSync, existsSync, mkdtempSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

import { runSetupWizard, login } from "../tests/support/api-client.ts";
import { startAxumServer, getAvailablePort } from "../tests/support/local-server.ts";
import { seedCustomers, type CreateCustomerBody } from "../src/lib/fixtures/customer.ts";

const SCRIPT_DIR = resolve(fileURLToPath(import.meta.url), "..");
const WEB_ROOT = resolve(SCRIPT_DIR, "..");
const OUTPUT_PATH = join(SCRIPT_DIR, "demo.db");

const SEED_COUNT = 25;
const FAKER_SEED = 20260328;

const ADMIN_EMAIL = "admin@demo.local";
const ADMIN_NAME = "Demo Admin";
const ADMIN_PASSWORD = "demo1234";
const SHOP_NAME = "Mokumo Prints";

async function main(): Promise<void> {
  // Pre-flight: verify sqlite3 CLI is available
  try {
    execFileSync("sqlite3", ["--version"], { encoding: "utf-8" });
  } catch {
    throw new Error(
      "sqlite3 CLI is required but not found on PATH. Install it before running the seed script.",
    );
  }

  const tempDir = mkdtempSync(join(tmpdir(), "mokumo-seed-"));
  console.log(`[seed] Temp dir: ${tempDir}`);

  let serverProcess: ReturnType<typeof import("node:child_process").spawn> | null = null;

  try {
    // 1. Start Axum server (requested port is a hint; actual bound port may differ)
    const requestedPort = await getAvailablePort();
    console.log(`[seed] Starting Axum (requested port ${requestedPort})...`);
    const {
      server,
      url,
      port: actualPort,
      setupToken,
    } = await startAxumServer(WEB_ROOT, requestedPort, tempDir);
    serverProcess = server;

    if (!setupToken) {
      throw new Error("Server started but no setup token was captured. Is this a fresh data dir?");
    }
    if (actualPort !== requestedPort) {
      console.log(`[seed] Port ${requestedPort} was unavailable, bound to ${actualPort}`);
    }
    console.log(`[seed] Server ready at ${url}`);

    // 2. Run setup wizard
    console.log("[seed] Running setup wizard...");
    await runSetupWizard(url, {
      setupToken,
      adminEmail: ADMIN_EMAIL,
      adminName: ADMIN_NAME,
      adminPassword: ADMIN_PASSWORD,
      shopName: SHOP_NAME,
    });

    // 3. Login
    console.log("[seed] Logging in...");
    const { setCookie } = await login(url, ADMIN_EMAIL, ADMIN_PASSWORD);

    // 4. Seed customers via API
    const customers = seedCustomers(SEED_COUNT, FAKER_SEED);
    console.log(`[seed] Creating ${customers.length} customers...`);

    for (const customer of customers) {
      await createCustomerViaApi(url, setCookie, customer);
    }
    console.log(`[seed] Created ${customers.length} customers`);

    // 5. Stop server and wait for clean exit (WAL flush)
    await stopServer(serverProcess);
    serverProcess = null;

    // 6. Find the database file
    const dbPath = findDatabase(tempDir);
    console.log(`[seed] Database found at: ${dbPath}`);

    // 7. Post-seed SQLite operations
    console.log("[seed] Running post-seed operations...");
    sqlite3(dbPath, "INSERT OR REPLACE INTO settings (key, value) VALUES ('setup_mode', 'demo');");
    sqlite3(dbPath, "VACUUM;");
    sqlite3(dbPath, "PRAGMA journal_mode=DELETE;");

    // 8. Copy to output
    copyFileSync(dbPath, OUTPUT_PATH);

    // 9. Summary
    const customerCount = sqlite3(OUTPUT_PATH, "SELECT COUNT(*) FROM customers;").trim();
    const activityCount = sqlite3(OUTPUT_PATH, "SELECT COUNT(*) FROM activity_log;").trim();
    const setupMode = sqlite3(
      OUTPUT_PATH,
      "SELECT value FROM settings WHERE key='setup_mode';",
    ).trim();
    const journalMode = sqlite3(OUTPUT_PATH, "PRAGMA journal_mode;").trim();
    const fileSizeKb = Math.round(statSync(OUTPUT_PATH).size / 1024);

    console.log("\n[seed] === Demo DB Summary ===");
    console.log(`  Customers:    ${customerCount}`);
    console.log(`  Activity log: ${activityCount} entries`);
    console.log(`  Setup mode:   ${setupMode}`);
    console.log(`  Journal mode: ${journalMode}`);
    console.log(`  File size:    ${fileSizeKb} KB`);
    console.log(`  Output:       ${OUTPUT_PATH}`);
    console.log("[seed] Done!");
  } finally {
    if (serverProcess) {
      await stopServer(serverProcess).catch(() => {});
    }
    try {
      rmSync(tempDir, { recursive: true, force: true });
    } catch (cleanupErr) {
      console.warn(`[seed] Warning: failed to clean up temp dir ${tempDir}:`, cleanupErr);
    }
  }
}

async function createCustomerViaApi(
  baseUrl: string,
  cookie: string,
  customer: CreateCustomerBody,
): Promise<void> {
  const res = await fetch(`${baseUrl}/api/customers`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Cookie: cookie,
    },
    body: JSON.stringify(customer),
  });
  if (!res.ok) {
    const body = await res.text();
    throw new Error(
      `Failed to create customer "${customer.display_name}" (${res.status}): ${body}`,
    );
  }
}

function findDatabase(dataDir: string): string {
  // Dual-dir layout (S0.1): data_dir/demo/mokumo.db
  const dualDirPath = join(dataDir, "demo", "mokumo.db");
  if (existsSync(dualDirPath)) return dualDirPath;

  // Fallback: data_dir/mokumo.db
  const flatPath = join(dataDir, "mokumo.db");
  if (existsSync(flatPath)) {
    console.warn(
      `[seed] Warning: expected dual-dir layout (${dualDirPath}) not found, using flat layout`,
    );
    return flatPath;
  }

  throw new Error(
    `Database not found. Checked:\n  ${dualDirPath}\n  ${flatPath}\nDoes the Axum server create the DB in the expected location?`,
  );
}

function sqlite3(dbPath: string, sql: string): string {
  try {
    return execFileSync("sqlite3", [dbPath, sql], { encoding: "utf-8" });
  } catch (err) {
    throw new Error(`sqlite3 command failed.\n  DB: ${dbPath}\n  SQL: ${sql}`, { cause: err });
  }
}

type ChildProcess = ReturnType<typeof import("node:child_process").spawn>;

function stopServer(proc: ChildProcess, timeoutMs = 5000): Promise<void> {
  return new Promise((done) => {
    const timeout = setTimeout(() => {
      console.warn("[seed] Server did not exit within timeout, sending SIGKILL");
      proc.kill("SIGKILL");
      done();
    }, timeoutMs);

    proc.on("exit", () => {
      clearTimeout(timeout);
      done();
    });

    if (!proc.kill("SIGTERM")) {
      console.warn("[seed] Warning: SIGTERM could not be delivered to server process");
      clearTimeout(timeout);
      done();
    }
  });
}

main().catch((err: unknown) => {
  console.error("[seed] Fatal error:", err);
  process.exit(1);
});
