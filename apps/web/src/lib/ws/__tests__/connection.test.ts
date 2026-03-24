import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// These imports will fail until implementation exists (RED phase)
import { createWebSocketConnection, calculateBackoff, type BroadcastEvent } from "../index.js";

// --- Mock WebSocket ---

class MockWebSocket {
  static instances: MockWebSocket[] = [];
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSING = 2;
  static CLOSED = 3;

  url: string;
  readyState: number = MockWebSocket.CONNECTING;
  onopen: ((ev: Event) => void) | null = null;
  onmessage: ((ev: MessageEvent) => void) | null = null;
  onclose: ((ev: CloseEvent) => void) | null = null;
  onerror: ((ev: Event) => void) | null = null;

  constructor(url: string) {
    this.url = url;
    MockWebSocket.instances.push(this);
  }

  send(_data: string): void {}
  close(_code?: number, _reason?: string): void {
    this.readyState = MockWebSocket.CLOSED;
  }

  // Test helpers
  simulateOpen(): void {
    this.readyState = MockWebSocket.OPEN;
    this.onopen?.(new Event("open"));
  }

  simulateMessage(data: unknown): void {
    this.onmessage?.(new MessageEvent("message", { data: JSON.stringify(data) }));
  }

  simulateClose(code = 1000, reason = ""): void {
    this.readyState = MockWebSocket.CLOSED;
    this.onclose?.({ type: "close", code, reason } as CloseEvent);
  }

  simulateError(): void {
    this.onerror?.(new Event("error"));
  }

  simulateRawMessage(raw: string): void {
    this.onmessage?.(new MessageEvent("message", { data: raw }));
  }
}

// ---

describe("WebSocket connection", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    MockWebSocket.instances = [];
    vi.stubGlobal("WebSocket", MockWebSocket);
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("connects to WebSocket URL", () => {
    const url = "ws://localhost:3000/ws";
    createWebSocketConnection(url, { onMessage: () => {} });

    expect(MockWebSocket.instances).toHaveLength(1);
    expect(MockWebSocket.instances[0].url).toBe(url);
  });

  it("calls onMessage when message received", () => {
    const onMessage = vi.fn();
    const event: BroadcastEvent = {
      v: 1,
      type: "customer.created",
      topic: "customers",
      payload: { id: 42 },
    };

    createWebSocketConnection("ws://localhost:3000/ws", { onMessage });
    const ws = MockWebSocket.instances[0];
    ws.simulateOpen();
    ws.simulateMessage(event);

    expect(onMessage).toHaveBeenCalledOnce();
    expect(onMessage).toHaveBeenCalledWith(event);
  });

  it("calls onReconnect after successful reconnection", () => {
    const onReconnect = vi.fn();
    createWebSocketConnection("ws://localhost:3000/ws", {
      onMessage: () => {},
      onReconnect,
    });

    const ws1 = MockWebSocket.instances[0];
    ws1.simulateOpen();
    // Server closes connection
    ws1.simulateClose(1006);

    // Advance past first backoff timer
    vi.advanceTimersByTime(1500);

    // New WebSocket should have been created
    expect(MockWebSocket.instances).toHaveLength(2);
    const ws2 = MockWebSocket.instances[1];
    ws2.simulateOpen();

    expect(onReconnect).toHaveBeenCalledOnce();
  });

  it("backoff doubles per attempt up to 30s cap", () => {
    expect(calculateBackoff(1)).toBe(1000);
    expect(calculateBackoff(2)).toBe(2000);
    expect(calculateBackoff(3)).toBe(4000);
    expect(calculateBackoff(4)).toBe(8000);
    expect(calculateBackoff(5)).toBe(16000);
    expect(calculateBackoff(6)).toBe(30000);
    expect(calculateBackoff(7)).toBe(30000);
  });

  it("backoff includes jitter within ±25%", () => {
    vi.useRealTimers(); // Need real Math.random

    for (let attempt = 1; attempt <= 5; attempt++) {
      const base = calculateBackoff(attempt);
      // Run multiple times and verify jitter bounds
      for (let i = 0; i < 50; i++) {
        const withJitter = calculateBackoff(attempt, { jitter: true });
        const lowerBound = base * 0.75;
        const upperBound = base * 1.25;
        expect(withJitter).toBeGreaterThanOrEqual(lowerBound);
        expect(withJitter).toBeLessThanOrEqual(upperBound);
      }
    }

    vi.useFakeTimers(); // Restore for afterEach
  });

  it("resets backoff after successful connection", () => {
    const onReconnect = vi.fn();
    createWebSocketConnection("ws://localhost:3000/ws", {
      onMessage: () => {},
      onReconnect,
    });

    const ws1 = MockWebSocket.instances[0];
    ws1.simulateOpen();

    // First disconnect — attempt 1, ~1s backoff
    ws1.simulateClose(1006);
    vi.advanceTimersByTime(1500);
    expect(MockWebSocket.instances).toHaveLength(2);

    const ws2 = MockWebSocket.instances[1];
    // Successful reconnect — should reset attempt counter
    ws2.simulateOpen();

    // Second disconnect — should be attempt 1 again (~1s), not attempt 2 (~2s)
    ws2.simulateClose(1006);

    // At 1.5s the reconnect should fire (attempt 1 = ~1s backoff)
    vi.advanceTimersByTime(1500);
    expect(MockWebSocket.instances).toHaveLength(3);
  });

  it("begins reconnecting on server close frame", () => {
    createWebSocketConnection("ws://localhost:3000/ws", { onMessage: () => {} });

    const ws = MockWebSocket.instances[0];
    ws.simulateOpen();
    ws.simulateClose(1001, "going away");

    // After backoff period, a new connection attempt should be made
    vi.advanceTimersByTime(1500);
    expect(MockWebSocket.instances).toHaveLength(2);
  });

  it("dispatches typed BroadcastEvent", () => {
    const onMessage = vi.fn();
    const event: BroadcastEvent = {
      v: 1,
      type: "order.updated",
      topic: "orders",
      payload: { orderId: 99, status: "shipped" },
    };

    createWebSocketConnection("ws://localhost:3000/ws", { onMessage });
    const ws = MockWebSocket.instances[0];
    ws.simulateOpen();
    ws.simulateMessage(event);

    const received = onMessage.mock.calls[0][0] as BroadcastEvent;
    expect(received.v).toBe(1);
    expect(received.type).toBe("order.updated");
    expect(received.topic).toBe("orders");
    expect(received.payload).toEqual({ orderId: 99, status: "shipped" });
  });

  it("close() stops reconnection attempts", () => {
    const handle = createWebSocketConnection("ws://localhost:3000/ws", {
      onMessage: () => {},
    });

    const ws = MockWebSocket.instances[0];
    ws.simulateOpen();
    ws.simulateClose(1006);

    // Close before the reconnect timer fires
    handle.close();

    // Advance well past any backoff period
    vi.advanceTimersByTime(60_000);

    // No new WebSocket should have been created
    expect(MockWebSocket.instances).toHaveLength(1);
  });

  it("onClose fires on intentional close, not on unintentional disconnect", () => {
    const onClose = vi.fn();
    const handle = createWebSocketConnection("ws://localhost:3000/ws", {
      onMessage: () => {},
      onClose,
    });

    const ws = MockWebSocket.instances[0];
    ws.simulateOpen();

    // Unintentional disconnect — onClose should NOT fire
    ws.simulateClose(1006);
    expect(onClose).not.toHaveBeenCalled();

    // Advance to trigger reconnect
    vi.advanceTimersByTime(1500);
    const ws2 = MockWebSocket.instances[1];
    ws2.simulateOpen();

    // Intentional close — onClose SHOULD fire
    handle.close();
    ws2.simulateClose(1000);
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("malformed JSON messages are silently ignored", () => {
    const onMessage = vi.fn();
    createWebSocketConnection("ws://localhost:3000/ws", { onMessage });

    const ws = MockWebSocket.instances[0];
    ws.simulateOpen();

    // Send malformed JSON
    ws.simulateRawMessage("not valid json {{{");
    ws.simulateRawMessage("");

    expect(onMessage).not.toHaveBeenCalled();
  });

  it("onReconnect is not called on initial connection", () => {
    const onReconnect = vi.fn();
    createWebSocketConnection("ws://localhost:3000/ws", {
      onMessage: () => {},
      onReconnect,
    });

    const ws = MockWebSocket.instances[0];
    ws.simulateOpen();

    expect(onReconnect).not.toHaveBeenCalled();
  });
});
