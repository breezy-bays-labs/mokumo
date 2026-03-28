import { describe, expect, it } from "vitest";
import { ensureRustLogInfoForApi, parseListeningPort } from "./local-server";

describe("parseListeningPort", () => {
  it("extracts port from plain tracing log line", () => {
    const line = "2026-03-28T00:00:00.000Z  INFO mokumo_api: Listening on 127.0.0.1:12345";
    expect(parseListeningPort(line)).toBe(12345);
  });

  it("extracts port from 0.0.0.0 binding", () => {
    const line = "  INFO mokumo_api: Listening on 0.0.0.0:6565";
    expect(parseListeningPort(line)).toBe(6565);
  });

  it("extracts port from ANSI-colored tracing output", () => {
    const line =
      "\x1b[2m2026-03-28T00:00:00Z\x1b[0m \x1b[32m INFO\x1b[0m \x1b[2mmokumo_api\x1b[0m\x1b[2m:\x1b[0m Listening on 127.0.0.1:53578";
    expect(parseListeningPort(line)).toBe(53578);
  });

  it("strips ANSI codes that would break the port regex", () => {
    // ANSI codes wrapping the port number itself
    const line = "Listening on 127.0.0.1:\x1b[1m8080\x1b[0m";
    expect(parseListeningPort(line)).toBe(8080);
  });

  it("returns null for unrelated log lines", () => {
    expect(parseListeningPort("INFO mokumo_api: Database initialized")).toBeNull();
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
    const line = "INFO mokumo_api: Listening on 127.0.0.1:65535";
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
  it("injects mokumo_api=info when RUST_LOG is unset", () => {
    expect(ensureRustLogInfoForApi(undefined)).toBe("mokumo_api=info");
  });

  it("injects mokumo_api=info when RUST_LOG is empty", () => {
    expect(ensureRustLogInfoForApi("")).toBe("mokumo_api=info");
  });

  it("replaces mokumo_api=warn with mokumo_api=info", () => {
    expect(ensureRustLogInfoForApi("mokumo_api=warn")).toBe("mokumo_api=info");
  });

  it("replaces mokumo_api=error with mokumo_api=info", () => {
    expect(ensureRustLogInfoForApi("mokumo_api=error,hyper=debug")).toBe(
      "mokumo_api=info,hyper=debug",
    );
  });

  it("replaces mokumo_api=off with mokumo_api=info", () => {
    expect(ensureRustLogInfoForApi("mokumo_api=off")).toBe("mokumo_api=info");
  });

  it("preserves mokumo_api=debug unchanged", () => {
    expect(ensureRustLogInfoForApi("mokumo_api=debug")).toBe("mokumo_api=debug");
  });

  it("preserves mokumo_api=trace unchanged", () => {
    expect(ensureRustLogInfoForApi("mokumo_api=trace,hyper=warn")).toBe(
      "mokumo_api=trace,hyper=warn",
    );
  });

  it("preserves mokumo_api=info unchanged", () => {
    expect(ensureRustLogInfoForApi("mokumo_api=info")).toBe("mokumo_api=info");
  });

  it("does not inject when bare global level covers INFO", () => {
    expect(ensureRustLogInfoForApi("debug")).toBe("debug");
    expect(ensureRustLogInfoForApi("trace")).toBe("trace");
    expect(ensureRustLogInfoForApi("info")).toBe("info");
  });

  it("injects when bare global level is below INFO", () => {
    expect(ensureRustLogInfoForApi("warn")).toBe("mokumo_api=info,warn");
    expect(ensureRustLogInfoForApi("error")).toBe("mokumo_api=info,error");
  });

  it("preserves other directives when injecting", () => {
    expect(ensureRustLogInfoForApi("hyper=debug,tower=trace")).toBe(
      "mokumo_api=info,hyper=debug,tower=trace",
    );
  });

  it("does not inject when global trace already covers it", () => {
    expect(ensureRustLogInfoForApi("trace,hyper=warn")).toBe("trace,hyper=warn");
  });

  it("strips suppressing duplicate when last-wins would suppress INFO", () => {
    expect(ensureRustLogInfoForApi("mokumo_api=debug,mokumo_api=error")).toBe("mokumo_api=debug");
  });

  it("strips all suppressing duplicates and keeps verbose ones", () => {
    expect(ensureRustLogInfoForApi("mokumo_api=warn,hyper=debug,mokumo_api=trace")).toBe(
      "hyper=debug,mokumo_api=trace",
    );
  });

  it("injects when all mokumo_api directives suppress INFO", () => {
    expect(ensureRustLogInfoForApi("mokumo_api=warn,mokumo_api=error")).toBe("mokumo_api=info");
  });
});
