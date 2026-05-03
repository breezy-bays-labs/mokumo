# Mokumo Security

Mokumo is self-hosted production-management software for small businesses. A security failure here means a shop loses customer data, payment information, or production records. This document describes the threat model, the trust boundaries Kikan enforces, and how to report vulnerabilities.

If you're looking for the architectural why behind these boundaries, see [ARCHITECTURE.md §3 Deployment topology](ARCHITECTURE.md#3-deployment-topology) and [§4 Control plane vs data plane](ARCHITECTURE.md#4-control-plane-vs-data-plane).

---

## Reporting a vulnerability

**Do not open a public issue for security vulnerabilities.**

Email `cmbays91@gmail.com` with:
- A clear description of the issue
- Steps to reproduce
- The deployment mode you tested in (`Lan`, `Internet`, or `ReverseProxy`)
- Mokumo version (from `mokumo-server --version`) and OS

You will receive an acknowledgment within 72 hours. Coordinated disclosure: we ask for 90 days from acknowledgment to fix and ship a release before public disclosure. We will publicly credit you in the release notes unless you prefer otherwise.

GitHub Security Advisories are also accepted via `breezy-bays-labs/mokumo` → Security tab once the repo's private advisory feature is configured.

---

## Threat model

### What Mokumo is designed to defend against

- **Network attackers on a hostile LAN** (e.g. a coffee-shop guest network, a poorly-segmented office Wi-Fi) — Mokumo's `Lan` mode binds to mDNS and the host-allow-list rejects requests that don't carry a recognized `Host:` header.
- **Drive-by browser attacks** (CSRF in `Internet`/`ReverseProxy`, header smuggling, XSS-via-cookie-theft) — security-headers middleware (CSP, X-Frame-Options, X-Content-Type-Options, Referrer-Policy, Permissions-Policy), `SameSite=Lax` cookies, CSRF token middleware in non-LAN modes, and adapter-side input validation on every endpoint.
- **Brute-force credential attack** — login rate-limiting (per-user) in all modes; per-IP rate-limiting added in `Internet`/`ReverseProxy`; `argon2id` password hashing.
- **SQL injection** — parameterized queries via SeaORM and `sqlx::query!()` macros only; CI grep gate against string-concatenated SQL.
- **Supply-chain compromise** of dependencies — `cargo-deny` advisory + license + source gate in CI; `gitleaks` pre-commit + CI; `pnpm` `minimumReleaseAge` enforced via workspace-root `packageManager` pin (member-pin override is forbidden per `feedback_pnpm-workspace-package-manager-pin.md`).
- **Catastrophic upgrade data loss** — backup-before-migrate, transactional migration, refuse-to-downgrade, N-2→N fixture upgrade tests. See [ARCHITECTURE.md §7 Upgrade safety model](ARCHITECTURE.md#7-upgrade-safety-model).

### What Mokumo does NOT defend against (out of scope today)

- **An attacker with shell access to the host.** This is the explicit trust boundary: physical / shell access = admin. Anyone who can read filesystem path `~/Library/Application Support/com.breezybayslabs.mokumo/` (macOS) or its equivalents can read the SQLite databases directly. Anyone who can connect to the Unix socket at mode 0600 is admin.
- **A compromised browser on a trusted client machine.** If a user's laptop is malware-infected, that user's session is at risk. We rely on standard browser security (no-XSS) for our part of the trust chain.
- **A compromised network between the user and a `ReverseProxy`-mode deployment** if the proxy isn't terminating TLS. The proxy is responsible for transport security in this mode; Mokumo trusts the `X-Forwarded-*` headers it receives.
- **Encryption at rest** for the SQLite databases. SQLite files are not encrypted today. Operators who need encryption-at-rest should rely on full-disk encryption (FileVault, LUKS, BitLocker). SQLCipher integration is on the post-M00 watchlist.

---

## Trust boundaries by deployment mode

The same Axum router runs in all three modes. The middleware stack reads `kikan::DataPlaneConfig::deployment_mode` and adjusts cookie flags, CSRF, rate limits, mDNS, and the host-allow-list at boot time.

### `Lan` mode (default for `mokumo-desktop`)

- Bind: `0.0.0.0` on the LAN interface (or loopback when launched via Tauri).
- Trust model: anyone on the LAN with the correct `Host:` header (`{shop}.local` or loopback) reaches the data plane. Browsers are expected to come from the same LAN as the server.
- Cookies: `Secure=false`, `SameSite=Lax`. We accept the trade-off because shops typically don't have HTTPS on their LAN.
- Control plane: reachable only over Tauri loopback or the Unix socket. LAN clients cannot reach control routes; the host-allow-list blocks them.
- mDNS: on. Server advertises itself as `{shop}.local`.
- CSRF: off. The trust boundary is "you're on the LAN."

**Recommended for**: a shop's office LAN where employees connect from desktop / laptop / phone browsers and only people inside the building (or on the office Wi-Fi) should reach the app.

### `Internet` mode (direct public exposure)

- Bind: configured public interface.
- Trust model: every request is potentially hostile. CSRF tokens required. Per-IP rate-limit added on top of per-user.
- Cookies: `Secure=true`, `SameSite=Lax`.
- Control plane: still loopback / UDS only. Even in `Internet` mode, admin operations are not network-exposed.
- mDNS: off.
- TLS: **mandatory**. Mokumo does not terminate TLS itself in `Internet` mode; you must front it with a reverse proxy or use Tailscale (see below). Running `Internet` mode without TLS is a deployment error.

**Recommended for**: nothing today. We strongly prefer `ReverseProxy` mode (Caddy/nginx terminating TLS, validated certs) or `Lan` + Tailscale (described next).

### `ReverseProxy` mode (behind Caddy / nginx / Traefik / Cloudflare Tunnel)

- Bind: loopback or a Unix socket; reverse proxy connects on the back side.
- Trust model: same as `Internet` for application logic. Trusts `X-Forwarded-Host`, `X-Forwarded-For`, `X-Forwarded-Proto` from the proxy.
- Cookies: `Secure=true`, `SameSite=Lax`. The proxy is responsible for the actual TLS.
- Control plane: still loopback / UDS only.
- mDNS: off.
- CSRF: on.

**Recommended for**: self-hosters who want to expose Mokumo to remote employees over a public domain. The proxy gives you TLS, request logging, IP allow-listing, and an HTTP-layer attack surface that Mokumo doesn't have to implement.

---

## Recommended remote-access paths (use one)

Mokumo's remote access strategy is to **delegate transport security to a tool that does it well** rather than build it ourselves.

### Tailscale (preferred for small teams)

Run `mokumo-server` (or the `mokumo-desktop` headless variant) on a node in your Tailnet. Employees install the Tailscale client and reach the server via its Tailnet IP. Tailscale handles WireGuard transport, peer auth, ACLs.

- Start `mokumo-server --deployment-mode lan --bind 100.x.y.z:PORT` (or simply `lan` with the Tailnet IP — Tailscale presents itself like a LAN).
- Add ACL rules in your Tailnet admin to gate which users see the Mokumo node.
- Tailscale Funnel can also expose the node to non-Tailnet users with proper auth — but for small teams, "everyone is on the Tailnet" is simpler.

A 4th `DeploymentMode::TailscaleMesh` (Tailnet-peer-attested cookies) is post-M00 — see watchlist `reference_tailscale-rs.md` in the ops repo.

### Cloudflare Tunnel (preferred for customer-facing portals later)

For exposing Mokumo to customers (artwork approval, order tracking) without opening a port on the LAN:

- Install `cloudflared` on the Mokumo host.
- `cloudflared tunnel create mokumo`
- Add `tunnel: <id>` config, route a public hostname to `http://localhost:PORT`.
- Run `mokumo-server --deployment-mode reverse-proxy --bind 127.0.0.1:PORT`.

Cloudflare Access can sit in front for SSO / IP allow-listing if you want enterprise-grade gating.

### Caddy or nginx (preferred for traditional VPS deployments)

If you already run a reverse proxy on the same host, point a vhost at `http://127.0.0.1:PORT` and run Mokumo as `--deployment-mode reverse-proxy`. Caddy gives you Let's Encrypt automatically; nginx requires `certbot` or similar.

---

## Operational hardening checklist

For self-hosters running Mokumo on a server they own:

- [ ] **Full-disk encryption** on the host (FileVault / LUKS / BitLocker).
- [ ] **Regular backups off the host.** Mokumo writes `VACUUM INTO`-snapshots to `~/.../backups/`; sync that directory to a separate location (Time Machine, restic, rclone-to-cloud).
- [ ] **Reverse proxy** for any non-LAN access. Don't run `Internet` mode bare.
- [ ] **Firewall** — block all inbound except the proxy's port.
- [ ] **Fail2ban or equivalent** on SSH if the host is on the public internet.
- [ ] **Run `mokumo-server` as a non-root user**. The Unix socket only needs to be readable by Mokumo and the operator running `kikan-cli`.
- [ ] **Subscribe to Mokumo release announcements** so you see security patches.
- [ ] **Test your backup restore quarterly.** A backup you've never restored is a hope, not a backup.

---

## Known limitations

- **No automatic security alerts to operators yet.** Critical-event email notifications (`#524`) and HealthResolver-driven menubar alerts (`#522`, `#523`) land in Wave 4 (Diagnostics + Supportability). Until then, operators should manually monitor `mokumo-server diagnose` output.
- **No audit trail UI yet.** The `activity_log` table is being populated by adapter-layer mutations (per `adr-entity-type-placement`); a viewer / export UI lands in Wave 4.
- **No SQLite encryption at rest.** Tracked on the post-M00 watchlist.
- **Code signing for desktop releases is in flight.** Windows code signing (`#525`) lands in Wave 6 (Installer + Release Packaging). Until then, Windows users see a SmartScreen warning on first install.

---

## Related ADRs

The decisions behind the trust boundaries and threat-model choices above live in `ops/decisions/mokumo/` (private repo). The full Y-statement summaries for the architectural ADRs are in [`ARCHITECTURE.md` §11](ARCHITECTURE.md#11-decision-index); the security-shaped ADRs below are the ones load-bearing for the contents of this document.

| ADR | What it pins |
|---|---|
| `adr-control-plane-data-plane-split` | Admin handlers stay reachable only over loopback (Tauri webview) or the Unix domain socket at mode 0600 (CLI). LAN clients never reach the control plane. Physical filesystem / shell access is the trust boundary for admin operations. |
| `adr-auth-security-under-cp-dp` | Session and recovery-flow handling under the control-plane / data-plane split: argon2id password hashing, login rate-limiting (per-user always; per-IP in `Internet` / `ReverseProxy`), opaque `RecoverySessionId` so rejected sessions can't enumerate emails, TOCTOU-safe atomic remove+reinsert in the recovery PIN registry, and a uniform 400 response across recovery rejection modes. |
| `adr-container-security-hardening` (with the 2026-04-29 amendment) | Container-runtime posture for cmux / Docker hosts: dev-container `RUSTUP_TOOLCHAIN` / `RUSTC_WRAPPER` overrides, `cargo-deny` invocation envelope, `/workspace` mount as the worktree (no nested git worktrees inside the container), and the operational guidance behind the [Operational hardening checklist](#operational-hardening-checklist) above. |

The decisions behind invariants I1–I5 (workspace boundary purity, headless Tauri-free build, one-way DAG) live in `adr-workspace-split-kikan` and are summarized in [`ARCHITECTURE.md` §8 Quality invariants](ARCHITECTURE.md#8-quality-invariants).

---

## Security standards we follow

- **OWASP ASVS L1** (target) — application security verification standard, level 1, for self-hosted commercial software.
- **CWE Top 25** — every PR's review checklist explicitly considers CWE Top 25 categories.
- **Negative-path testing** — for any conditional / path-matching / range-checking code, the boundary cases and the "almost right" rejection case are tested before the happy path. Standard at `ops/standards/testing/negative-path.md`.
- **Silent-failure-hunter review agent** — every PR with a `catch`, `unwrap_or`, or fallback path runs through the silent-failure-hunter agent before merge.

The full security standard (which is being realigned to the Axum/SQLite/Tauri stack as part of Wave 5 #381) lives in the ops repo at `ops/standards/security.md`.
