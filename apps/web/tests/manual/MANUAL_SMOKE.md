# Manual Smoke Checklist

Scenarios that cannot be automated in CI due to OS-level constraints (clock injection, OS
tray/menu interaction) or infrastructure not yet available. Each row references the SMOKE-MAP.md
disposition and any acceptance criteria that gate the milestone.

---

## SMOKE-08 — mDNS retry backoff

**Scenario:** `[SMOKE-08] server becomes reachable by hostname within the mDNS retry window`

**Why manual:** Clock injection for the mDNS retry timer requires a Rust refactor
(`mdns-sd` crate) that is deferred to a follow-up issue (D8). Until then, verifying
the retry window requires real wall-clock time, making it impractical for CI.

**Manual procedure:**

1. Start the Mokumo server on a LAN-connected machine (not loopback-only).
2. Open a browser on a **second** device on the same network.
3. Navigate to `http://{shop-name}.local` — expect the UI to load within 30 s.
4. Note the time from server start to first successful response — must be < 30 s.
5. Stop and restart the server. Repeat step 3 within 5 s of restart — expect the
   browser to reconnect via mDNS hostname within 30 s.

**Pass criteria:** The `.local` hostname resolves and the UI loads on both initial
discovery and after a restart, within the 30 s mDNS retry window.

**Follow-up:** Clock injection refactor — file issue once mDNS module is stabilised.

---

## SMOKE-09b — Tray quit wiring (OS menu path)

**Scenario:** `[SMOKE-09b] quit-from-hidden-tray invokes shutdown through the OS menu path`

**Disposition:** `needs-computer-use` — requires OS-level tray menu interaction (right-click
on system tray icon, select Quit). Cannot be automated with headless Chromium + Playwright.

**M1-gated Acceptance Criterion:**

> "Interactive-Claude SMOKE-09b pass recorded in release notes before M1 ships."

**Manual procedure:**

1. Launch the Mokumo desktop app. Confirm at least one client is connected (LAN).
2. Click the Mokumo tray icon to open the context menu.
3. Select **Quit** from the tray menu.
4. Observe: the app emits the quit dialog (or quits immediately if no clients connected).
5. Confirm the server process exits cleanly (no zombie, `pgrep -fa mokumo` returns no matches).

**Pass criteria:** Quit from the OS tray menu triggers the correct behaviour (dialog or
immediate quit per active client count) and the server shuts down within 12 s.

---

## SMOKE-10b — Quit dialog OS window

**Scenario:** `[SMOKE-10b] quit dialog renders with correct copy and buttons in the OS window`

**Disposition:** `needs-computer-use`

**M1-gated Acceptance Criterion:**

> "Interactive-Claude SMOKE-10b pass recorded in release notes before M1 ships."

**Manual procedure:**

1. Launch the Mokumo desktop app with at least one active LAN client connected.
2. From the tray menu, select **Quit**.
3. Observe the native dialog — verify:
   - Copy says "N client(s) are connected to your shop" with the correct count.
   - Two buttons are present: **Quit Anyway** and **Cancel**.
4. Click **Cancel** — confirm app continues running.
5. Reconnect a client, then select Quit again and click **Quit Anyway** — confirm app exits.

**Pass criteria:** Dialog displays correct client count, both buttons work as expected,
and force-quit exits the server cleanly.

---

## SMOKE-11b — Tray icon OS tray (live state)

**Scenario:** `[SMOKE-11b] tray icon and tooltip update visibly in the OS tray as server state changes`

**Disposition:** `needs-computer-use`

**M1-gated Acceptance Criterion:**

> "Interactive-Claude SMOKE-11b pass recorded in release notes before M1 ships."

**Manual procedure:**

1. Launch the Mokumo desktop app. Observe tray icon in the OS system tray.
2. With no clients connected — confirm the tooltip reads "Mokumo — no clients connected"
   (or equivalent) and the icon is in its idle state.
3. Open the Mokumo UI in a browser (LAN client connects). Observe the tray icon changes
   to the active state and the tooltip reflects "1 client connected".
4. Close the browser tab. Observe the tray icon returns to idle state.

**Pass criteria:** Tray icon and tooltip visibly reflect server lifecycle transitions
(idle → active → idle) in the OS system tray area.
