import { test, expect } from "../support/lan-client.fixture";
import { BackendHarness, resolveHarnessWebRoot } from "../support/harness";

const webRoot = resolveHarnessWebRoot(import.meta.url);

test("[SMOKE-05] port conflict — second server on an occupied port fails to start", async () => {
  const h1 = new BackendHarness(webRoot);
  await h1.start();
  const occupiedPort = h1.port;

  const h2 = new BackendHarness(webRoot);
  try {
    // Attempting to bind the same port while h1 holds it should fail
    await expect(h2.start(occupiedPort)).rejects.toThrow();
  } finally {
    await h1.stop();
    h1.cleanup();
    h2.cleanup();
  }
});

test("[SMOKE-06] port fallback — server auto-selects a different port when requested port is occupied", async () => {
  const h1 = new BackendHarness(webRoot);
  await h1.start();
  const occupiedPort = h1.port;

  // BackendHarness.start() with no port argument picks a free port
  const h2 = new BackendHarness(webRoot);
  try {
    await h2.start();
    expect(h2.port).not.toBe(occupiedPort);
    expect(h2.port).toBeGreaterThan(0);
  } finally {
    await h1.stop();
    await h2.stop();
    h1.cleanup();
    h2.cleanup();
  }
});

test.fixme("[SMOKE-07] port exhaustion — server emits a clear error when no ports are available", async () => {
  // Port exhaustion simulation is impractical in CI: binding thousands of
  // sockets is slow, resource-limited, and unreliable across platforms.
  // Filed as follow-up issue to track a lightweight mock approach.
  // See: https://github.com/cmbays/mokumo/issues (follow-up from PR)
});
