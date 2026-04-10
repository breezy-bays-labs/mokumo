/**
 * Reactive WebSocket connection status store (Svelte 5 runes).
 *
 * Drives the disconnect banner visibility and reconnection indicator.
 */

export const wsStatus = $state({
  connected: true,
  reconnecting: false,
  shutdownReceived: false,
  /** Brief "Reconnected" confirmation after a successful reconnect. */
  showReconnected: false,
});

export function markDisconnected() {
  wsStatus.connected = false;
  wsStatus.reconnecting = true;
  wsStatus.showReconnected = false;
}

export function markShutdown() {
  wsStatus.connected = false;
  wsStatus.shutdownReceived = true;
  wsStatus.reconnecting = false;
  wsStatus.showReconnected = false;
}

export function markConnected() {
  const wasDisconnected = !wsStatus.connected;
  wsStatus.connected = true;
  wsStatus.reconnecting = false;
  wsStatus.shutdownReceived = false;

  if (wasDisconnected) {
    wsStatus.showReconnected = true;
    setTimeout(() => {
      wsStatus.showReconnected = false;
    }, 3000);
  }
}
