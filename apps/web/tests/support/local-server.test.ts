import type { ChildProcess } from "node:child_process";
import { describe, expect, it, vi } from "vitest";
import {
  ensureRustLogInfoForApi,
  isExpectedServerNotReady,
  parseListeningPort,
  waitForServer,
} from "./local-server";

describe("parseListeningPort", () => {
  it("extracts port from plain tracing log line", () => {
    const line = "2026-03-28T00:00:00.000Z  INFO kikan: Listening on 127.0.0.1:12345";
    expect(parseListeningPort(line)).toBe(12345);
  });

  it("extracts port from 0.0.0.0 binding", () => {
    const line = "  INFO kikan: Listening on 0.0.0.0:6565";
    expect(parseListeningPort(line)).toBe(6565);
  });

  it("extracts port from ANSI-colored tracing output", () => {
    const line =
      "\x1b[2m2026-03-28T00:00:00Z\x1b[0m \x1b[32m INFO\x1b[0m \x1b[2mkikan\x1b[0m\x1b[2m:\x1b[0m Listening on 127.0.0.1:53578";
    expect(parseListeningPort(line)).toBe(53578);
  });

  it("strips ANSI codes that would break the port regex", () => {
    // ANSI codes wrapping the port number itself
    const line = "Listening on 127.0.0.1:\x1b[1m8080\x1b[0m";
    expect(parseListeningPort(line)).toBe(8080);
  });

  it("returns null for unrelated log lines", () => {
    expect(parseListeningPort("INFO kikan: Database initialized")).toBeNull();
    expect(parseListeningPort("Setup required — token: abc-123")).toBeNull();
  });

  it("returns null for empty string", () => {
    expect(parseListeningPort("")).toBeNull();
  });

  it("returns null when port is not a valid number", () => {
    const line = "Listening on 127.0.0.1:notaport";
    expect(parseListeningPort(line)).toBeNull();
  });

  it("handles port at u16 boundary", () => {
    const line = "INFO kikan: Listening on 127.0.0.1:65535";
    expect(parseListeningPort(line)).toBe(65535);
  });

  it("returns null for port 0", () => {
    expect(parseListeningPort("Listening on 127.0.0.1:0")).toBeNull();
  });

  it("returns null for port exceeding u16 max", () => {
    expect(parseListeningPort("Listening on 127.0.0.1:65536")).toBeNull();
  });

  it("returns null for partial match without port", () => {
    expect(parseListeningPort("Listening on 127.0.0.1:")).toBeNull();
  });
});

describe("ensureRustLogInfoForApi", () => {
  it("injects both targets when RUST_LOG is unset", () => {
    expect(ensureRustLogInfoForApi(undefined)).toBe(
      "kikan=info,mokumo_server=info,mokumo_shop=info",
    );
  });

  it("injects both targets when RUST_LOG is empty", () => {
    expect(ensureRustLogInfoForApi("")).toBe("kikan=info,mokumo_server=info,mokumo_shop=info");
  });

  it("replaces kikan=warn and still injects mokumo_server", () => {
    expect(ensureRustLogInfoForApi("kikan=warn")).toBe(
      "kikan=info,mokumo_server=info,mokumo_shop=info",
    );
  });

  it("replaces kikan=error and preserves other directives", () => {
    expect(ensureRustLogInfoForApi("kikan=error,hyper=debug")).toBe(
      "kikan=info,mokumo_server=info,mokumo_shop=info,hyper=debug",
    );
  });

  it("replaces kikan=off and still injects mokumo_server", () => {
    expect(ensureRustLogInfoForApi("kikan=off")).toBe(
      "kikan=info,mokumo_server=info,mokumo_shop=info",
    );
  });

  it("preserves kikan=debug but still injects mokumo_server and mokumo_shop", () => {
    expect(ensureRustLogInfoForApi("kikan=debug")).toBe(
      "mokumo_server=info,mokumo_shop=info,kikan=debug",
    );
  });

  it("preserves kikan=trace but still injects mokumo_server and mokumo_shop", () => {
    expect(ensureRustLogInfoForApi("kikan=trace,hyper=warn")).toBe(
      "mokumo_server=info,mokumo_shop=info,kikan=trace,hyper=warn",
    );
  });

  it("preserves both targets when both already cover INFO", () => {
    expect(ensureRustLogInfoForApi("kikan=info,mokumo_server=info,mokumo_shop=info")).toBe(
      "kikan=info,mokumo_server=info,mokumo_shop=info",
    );
  });

  it("does not inject when bare global level covers INFO", () => {
    expect(ensureRustLogInfoForApi("debug")).toBe("debug");
    expect(ensureRustLogInfoForApi("trace")).toBe("trace");
    expect(ensureRustLogInfoForApi("info")).toBe("info");
  });

  it("injects all targets when bare global level is below INFO", () => {
    expect(ensureRustLogInfoForApi("warn")).toBe(
      "kikan=info,mokumo_server=info,mokumo_shop=info,warn",
    );
    expect(ensureRustLogInfoForApi("error")).toBe(
      "kikan=info,mokumo_server=info,mokumo_shop=info,error",
    );
  });

  it("preserves other directives when injecting", () => {
    expect(ensureRustLogInfoForApi("hyper=debug,tower=trace")).toBe(
      "kikan=info,mokumo_server=info,mokumo_shop=info,hyper=debug,tower=trace",
    );
  });

  it("does not inject when global trace already covers it", () => {
    expect(ensureRustLogInfoForApi("trace,hyper=warn")).toBe("trace,hyper=warn");
  });

  it("strips suppressing duplicate when last-wins would suppress INFO", () => {
    expect(ensureRustLogInfoForApi("kikan=debug,kikan=error")).toBe(
      "mokumo_server=info,mokumo_shop=info,kikan=debug",
    );
  });

  it("strips all suppressing duplicates and keeps verbose ones", () => {
    expect(ensureRustLogInfoForApi("kikan=warn,hyper=debug,kikan=trace")).toBe(
      "mokumo_server=info,mokumo_shop=info,hyper=debug,kikan=trace",
    );
  });

  it("injects when all kikan directives suppress INFO", () => {
    expect(ensureRustLogInfoForApi("kikan=warn,kikan=error")).toBe(
      "kikan=info,mokumo_server=info,mokumo_shop=info",
    );
  });

  it("injects when last bare global suppresses INFO (last-wins)", () => {
    expect(ensureRustLogInfoForApi("trace,warn")).toBe(
      "kikan=info,mokumo_server=info,mokumo_shop=info,trace,warn",
    );
    expect(ensureRustLogInfoForApi("info,error")).toBe(
      "kikan=info,mokumo_server=info,mokumo_shop=info,info,error",
    );
  });

  it("does not inject when last bare global covers INFO", () => {
    expect(ensureRustLogInfoForApi("warn,debug")).toBe("warn,debug");
  });
});

describe("isExpectedServerNotReady", () => {
  it("returns true for DOMException TimeoutError", () => {
    expect(isExpectedServerNotReady(new DOMException("aborted", "TimeoutError"))).toBe(true);
  });

  it("returns false for DOMException AbortError", () => {
    expect(isExpectedServerNotReady(new DOMException("aborted", "AbortError"))).toBe(false);
  });

  it("returns true for TypeError with ECONNREFUSED cause", () => {
    const err = new TypeError("fetch failed");
    Object.assign(err, { cause: { code: "ECONNREFUSED" } });
    expect(isExpectedServerNotReady(err)).toBe(true);
  });

  it("returns true for TypeError with ECONNRESET cause", () => {
    const err = new TypeError("fetch failed");
    Object.assign(err, { cause: { code: "ECONNRESET" } });
    expect(isExpectedServerNotReady(err)).toBe(true);
  });

  it("returns false for fetch failed TypeError with unrecognized cause", () => {
    const err = new TypeError("fetch failed");
    Object.assign(err, { cause: { message: "some network error" } });
    expect(isExpectedServerNotReady(err)).toBe(false);
  });

  it("returns false for TypeError with ENOTFOUND cause (DNS failure)", () => {
    const err = new TypeError("fetch failed");
    Object.assign(err, { cause: { code: "ENOTFOUND" } });
    expect(isExpectedServerNotReady(err)).toBe(false);
  });

  it("returns false for TypeError without cause (e.g. Invalid URL)", () => {
    expect(isExpectedServerNotReady(new TypeError("Invalid URL"))).toBe(false);
  });

  it("returns false for RangeError", () => {
    expect(isExpectedServerNotReady(new RangeError("out of range"))).toBe(false);
  });

  it("returns false for plain Error", () => {
    expect(isExpectedServerNotReady(new Error("generic"))).toBe(false);
  });

  it("returns false for non-Error values", () => {
    expect(isExpectedServerNotReady("string error")).toBe(false);
    expect(isExpectedServerNotReady(null)).toBe(false);
    expect(isExpectedServerNotReady(undefined)).toBe(false);
  });
});

/** Minimal stub that satisfies the ChildProcess shape waitForServer inspects. */
function fakeProcess(overrides: { exitCode?: number | null } = {}): ChildProcess {
  return { exitCode: overrides.exitCode ?? null, signalCode: null } as unknown as ChildProcess;
}

describe("waitForServer", () => {
  it("returns immediately when fetch succeeds on first try", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValueOnce({ ok: true }));

    await expect(
      waitForServer("http://127.0.0.1:9999", fakeProcess(), "test-server", 5_000),
    ).resolves.toBeUndefined();

    vi.unstubAllGlobals();
  });

  it("retries on connection-refused TypeError then succeeds", async () => {
    const connRefused = new TypeError("fetch failed");
    Object.assign(connRefused, { cause: { code: "ECONNREFUSED" } });

    vi.stubGlobal(
      "fetch",
      vi.fn().mockRejectedValueOnce(connRefused).mockResolvedValueOnce({ ok: true }),
    );

    await expect(
      waitForServer("http://127.0.0.1:9999", fakeProcess(), "test-server", 5_000),
    ).resolves.toBeUndefined();

    vi.unstubAllGlobals();
  });

  it("retries on AbortSignal timeout then succeeds", async () => {
    const abortTimeout = new DOMException("The operation was aborted", "TimeoutError");

    vi.stubGlobal(
      "fetch",
      vi.fn().mockRejectedValueOnce(abortTimeout).mockResolvedValueOnce({ ok: true }),
    );

    await expect(
      waitForServer("http://127.0.0.1:9999", fakeProcess(), "test-server", 5_000),
    ).resolves.toBeUndefined();

    vi.unstubAllGlobals();
  });

  it("re-throws unexpected TypeError (e.g. malformed URL)", async () => {
    const malformedUrl = new TypeError("Invalid URL");

    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(malformedUrl));

    await expect(waitForServer("not-a-url", fakeProcess(), "test-server", 5_000)).rejects.toThrow(
      "Invalid URL",
    );

    vi.unstubAllGlobals();
  });

  it("re-throws unexpected non-network errors", async () => {
    const unexpected = new RangeError("something completely unexpected");

    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(unexpected));

    await expect(
      waitForServer("http://127.0.0.1:9999", fakeProcess(), "test-server", 5_000),
    ).rejects.toThrow("something completely unexpected");

    vi.unstubAllGlobals();
  });

  it("includes last error in timeout message", async () => {
    const connRefused = new TypeError("fetch failed");
    Object.assign(connRefused, { cause: { code: "ECONNREFUSED" } });

    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(connRefused));

    await expect(
      waitForServer("http://127.0.0.1:9999", fakeProcess(), "test-server", 500),
    ).rejects.toThrow(/did not start within 500ms.*last error.*fetch failed/is);

    vi.unstubAllGlobals();
  });

  it("throws process exit error even when fetch errors are expected", async () => {
    const connRefused = new TypeError("fetch failed");
    Object.assign(connRefused, { cause: { code: "ECONNREFUSED" } });

    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(connRefused));

    await expect(
      waitForServer("http://127.0.0.1:9999", fakeProcess({ exitCode: 1 }), "test-server", 5_000),
    ).rejects.toThrow("test-server exited with code 1");

    vi.unstubAllGlobals();
  });
});
