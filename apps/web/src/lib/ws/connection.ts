/**
 * WebSocket client with automatic reconnection and exponential backoff.
 *
 * Pairs with the Rust ConnectionManager (services/api) to receive
 * BroadcastEvent messages over a persistent WebSocket connection.
 */

import type { BroadcastEvent } from "$lib/types/kikan/BroadcastEvent";

export type { BroadcastEvent };

export interface ConnectionOptions {
  onMessage: (event: BroadcastEvent) => void;
  onReconnect?: () => void;
  onClose?: () => void;
  onDisconnect?: () => void;
  onShutdown?: () => void;
  /** Milliseconds of silence before the client force-closes and reconnects.
   * Defaults to 75 000 ms (2.5 × the 30 s server heartbeat interval).
   * Set to 0 to disable the liveness timer. */
  livenessTimeoutMs?: number;
}

interface BackoffOptions {
  jitter?: boolean;
}

const MAX_BACKOFF_MS = 30_000;
const BASE_BACKOFF_MS = 1_000;

/**
 * Calculate exponential backoff delay for a given attempt number.
 *
 * Formula: min(1000 * 2^(attempt-1), 30000)
 * With optional ±25% jitter.
 */
export function calculateBackoff(attempt: number, options?: BackoffOptions): number {
  const base = Math.min(BASE_BACKOFF_MS * Math.pow(2, attempt - 1), MAX_BACKOFF_MS);

  if (options?.jitter) {
    // ±25% jitter: multiply by random value in [0.75, 1.25]
    const jitterFactor = 0.75 + Math.random() * 0.5;
    return Math.round(base * jitterFactor);
  }

  return base;
}

/**
 * Create a WebSocket connection with automatic reconnection.
 *
 * Returns a handle with a `close()` method to cleanly shut down.
 */
export function createWebSocketConnection(
  url: string,
  options: ConnectionOptions,
): { close(): void } {
  const livenessMs = options.livenessTimeoutMs ?? 75_000;
  let attempt = 0;
  let intentionallyClosed = false;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let livenessTimer: ReturnType<typeof setTimeout> | null = null;
  let currentWs: WebSocket | null = null;

  function resetLiveness(ws: WebSocket): void {
    if (livenessMs <= 0 || intentionallyClosed) return;
    stopLiveness();
    livenessTimer = setTimeout(() => {
      // No message received within the liveness window — force-close so the
      // reconnect loop fires and the disconnect banner appears.
      ws.close();
    }, livenessMs);
  }

  function stopLiveness(): void {
    if (livenessTimer !== null) {
      clearTimeout(livenessTimer);
      livenessTimer = null;
    }
  }

  function connect(): void {
    const ws = new WebSocket(url);
    currentWs = ws;

    ws.onopen = () => {
      const isReconnect = attempt > 0;
      attempt = 0;
      resetLiveness(ws);

      if (isReconnect) {
        options.onReconnect?.();
      }
    };

    ws.onmessage = (event: MessageEvent) => {
      let data: BroadcastEvent;
      try {
        data = JSON.parse(event.data as string) as BroadcastEvent;
      } catch {
        // Silently ignore malformed JSON — don't propagate parse failures
        return;
      }

      // Reset liveness only on a successfully parsed message so that a stream
      // of malformed frames cannot defeat the liveness-timeout force-close.
      resetLiveness(ws);
      if (data.type === "server_shutting_down") {
        options.onShutdown?.();
      }
      options.onMessage(data);
    };

    ws.onerror = () => {
      // Connection failures (TLS, DNS, refused) are surfaced here.
      // onclose will fire next and trigger reconnection.
      stopLiveness();
    };

    ws.onclose = () => {
      stopLiveness();
      if (intentionallyClosed) {
        options.onClose?.();
        return;
      }

      options.onDisconnect?.();
      attempt += 1;
      const delay = calculateBackoff(attempt, { jitter: true });
      reconnectTimer = setTimeout(() => {
        connect();
      }, delay);
    };
  }

  connect();

  return {
    close() {
      intentionallyClosed = true;
      stopLiveness();
      if (reconnectTimer !== null) {
        clearTimeout(reconnectTimer);
      }
      currentWs?.close();
    },
  };
}
