# ADR: HTTP Security Headers

**Status**: Accepted
**Date**: 2026-04-09
**Issue**: #380

## Context

Mokumo serves a SvelteKit SPA from an Axum backend. The Cloudflare Tunnel
feature (M4) makes the server internet-facing. Without security headers, XSS
vulnerabilities allow full database access, and the app can be clickjacked via
iframes.

## Decision

Add a Tower middleware (`axum::middleware::from_fn`) that sets security headers
on every HTTP response. The middleware is the outermost layer so it covers all
routes including SPA fallback, auth errors, and health checks.

### Headers (always set)

| Header | Value | Rationale |
|--------|-------|-----------|
| `Content-Security-Policy` | See below | Primary XSS defense |
| `X-Content-Type-Options` | `nosniff` | Prevent MIME sniffing |
| `X-Frame-Options` | `DENY` | Clickjacking protection (legacy browsers) |
| `X-XSS-Protection` | `0` | Disable legacy XSS filter (causes more harm than good; rely on CSP) |
| `Referrer-Policy` | `strict-origin-when-cross-origin` | Limit referrer leakage |

### Content-Security-Policy

```
default-src 'self';
script-src 'self' 'unsafe-inline';
style-src 'self' 'unsafe-inline';
img-src 'self' data:;
connect-src 'self' ws: wss:;
frame-ancestors 'none'
```

**Why `script-src 'unsafe-inline'`**: SvelteKit adapter-static emits an inline
bootstrap `<script>` in `index.html` that initializes the SPA. Without
`'unsafe-inline'`, the app will not load. Nonce-based CSP is not feasible
because the HTML is served from rust-embed (static bytes) with no per-request
rewriting.

**Why `style-src 'unsafe-inline'`**: SvelteKit inlines scoped component styles
and the root `<div style="display: contents">`. This is a known SvelteKit
limitation (sveltejs/kit#11747).

**Tightening roadmap**: When SvelteKit moves to external stylesheets or the Web
Animations API, `'unsafe-inline'` can be removed. For `script-src`, a future
enhancement could compute the SHA-256 hash of the inline bootstrap script at
server startup and include it in the CSP header, allowing removal of
`'unsafe-inline'` for scripts.

### Conditional HSTS

`Strict-Transport-Security: max-age=63072000; includeSubDomains` is set **only**
when the request arrives through Cloudflare Tunnel, detected by the presence of
the `cf-connecting-ip` header. LAN-only HTTP deployments would break with
unconditional HSTS.

No `preload` directive: self-hosted domains vary per shop and should never join
the browser HSTS preload list.

## Alternatives Considered

1. **`tower-http::SetResponseHeaderLayer`** — one layer per header, verbose for
   6+ headers. No request access for conditional HSTS.
2. **`axum-helmet` crate** — helmet.js port. Adds a dependency for what is 30
   lines of code. Preset CSP doesn't match SvelteKit needs.
3. **CSP via `<meta>` tag in HTML** — cannot set `frame-ancestors` via meta tag
   (spec limitation). Also doesn't cover non-HTML responses.

## Consequences

- Every response from the Axum server includes defensive security headers.
- CSP with `'unsafe-inline'` is not ideal but is the pragmatic starting point
  given SvelteKit's current constraints.
- HSTS is safe for Tunnel users and does not affect LAN-only deployments.
- Future work: OWASP recommends additional headers (Permissions-Policy, COOP,
  CORP, COEP) which can be added incrementally.
