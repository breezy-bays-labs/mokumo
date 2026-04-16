/**
 * Returns the API base URL for the current execution context.
 *
 * In the Tauri desktop shell, `window.__MOKUMO_API_BASE__` is injected by
 * `WebviewWindowBuilder::initialization_script()` before DOMContentLoaded,
 * resolving to the OS-assigned ephemeral loopback address for this launch.
 *
 * The fallback (`http://127.0.0.1:6565`) applies only in the SvelteKit dev
 * server context (`pnpm dev`) where no Tauri shell is present.
 * Per `adr-kikan-binary-topology §7`: desktop binds `:0` (ephemeral); the
 * `mokumo-server` binary uses 6565–6575 (hence the dev fallback value).
 *
 * ⚠ STALE-GLOBAL INVARIANT: after a demo-reset restart that assigns a
 * different loopback port, `window.__MOKUMO_API_BASE__` reflects the OLD
 * port. The initialization_script string is baked at webview-build time and
 * cannot be updated without reconstructing the WebviewWindow. This is safe
 * today because ALL SPA fetch calls use same-origin relative paths (/api/…)
 * — they resolve against the new URL origin after navigate, NOT through this
 * accessor. DO NOT migrate existing apiFetch() calls to apiBase() until the
 * restart loop reconstructs the WebviewWindow.
 * Cross-ref: apps/mokumo-desktop/src/lib.rs restart loop spawn site.
 */
export function apiBase(): string {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  return (window as any).__MOKUMO_API_BASE__ ?? "http://127.0.0.1:6565";
}
