# Verify security headers are present on every response.
# Uses /api/health as a public, unauthenticated endpoint.

GET http://{{host}}/api/health

HTTP 200
[Asserts]
header "Content-Security-Policy" contains "default-src 'self'"
header "Content-Security-Policy" contains "frame-ancestors 'none'"
header "X-Content-Type-Options" == "nosniff"
header "X-Frame-Options" == "DENY"
header "X-XSS-Protection" == "0"
header "Referrer-Policy" == "strict-origin-when-cross-origin"
# HSTS is only set when behind Cloudflare Tunnel (cf-connecting-ip present).
# LAN-only requests must NOT get HSTS to avoid breaking plain HTTP access.
header "Strict-Transport-Security" not exists
