import { describe, expect, it } from "vitest";
import { parseListeningPort } from "./local-server";

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
